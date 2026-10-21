"""Prepare height-free WBM borders. No DEM, geoid or imagery input is read.

Use the pinned optional terrain-processing dependencies. Original WBM source
files and their exact redistribution review are retained from the water import.
"""
from __future__ import annotations
import argparse
import hashlib
import json
from pathlib import Path
import struct
import numpy as np
import rasterio
from boundary_geometry import boundary_mesh

ROOT = Path(__file__).resolve().parents[1]
# Fixed reviewed source-post crops, retained for geographic coverage continuity.
# They define no city, waypoint, elevation or observer destination.
CROPS = [
    ("region-01", "N35_00_E138_00", [1980, 2628], [2232, 3024]),
    ("region-02", "N35_00_E139_00", [795, 1440], [2094, 2886]),
    ("region-03", "N29_00_W096_00", [528, 1176], [1920, 2664]),
    ("region-04", "N29_00_W095_00", [2193, 2838], [360, 1098]),
]


def sha256(path):
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def prepare(crop, check=False, output_dir=None):
    identifier, tile, row_bounds, column_bounds = crop
    source = ROOT / ".cache/water" / f"{tile}-WBM.tif"
    review = json.loads((ROOT / "licenses/water-sources.json").read_text())
    license_record = next(item for item in review["files"] if item["filename"] == source.name)
    if sha256(source) != license_record["sha256"]:
        raise ValueError("WBM differs from the exact licensed source")
    with rasterio.open(source) as raster:
        affine = raster.transform
        if (raster.crs.to_epsg() != 4326 or raster.count != 1 or raster.dtypes != ("uint8",)
                or raster.tags().get("AREA_OR_POINT") != "Point"
                or affine.b != 0 or affine.d != 0 or affine.a <= 0 or affine.e >= 0
                or raster.scales != (1.0,) or raster.offsets != (0.0,)):
            raise ValueError("Unsupported native categorical registration")
        mask = raster.read(1, masked=True)
        if np.ma.getmaskarray(mask).any() or not np.isin(mask, [0, 1, 2, 3]).all():
            raise ValueError("Missing or unsupported WBM category")
        mask = np.asarray(mask)
        rows = np.arange(row_bounds[0], row_bounds[1] + 1, 3)
        columns = np.arange(column_bounds[0], column_bounds[1] + 1, 3)
        if (rows[-1] != row_bounds[1] or columns[-1] != column_bounds[1]
                or rows[0] < 0 or columns[0] < 0
                or rows[-1] >= raster.height or columns[-1] >= raster.width):
            raise ValueError("Source crop leaves native coverage")
        window = np.lib.stride_tricks.sliding_window_view(mask, (4, 4))[
            rows[:-1, None], columns[None, :-1]]
        low, high = window.min(axis=(2, 3)), window.max(axis=(2, 3))
        codes = np.repeat(np.where(low == high, low, 4)[:, :, None], 2, axis=2).astype(np.uint8)
        parameters, faces, reconstruction = boundary_mesh(mask, rows, columns, codes)
        west, north = affine * (float(columns[0]) + 0.5, float(rows[0]) + 0.5)
        dx, dy = 3 * affine.a, -3 * affine.e
        local_parameters = parameters - [rows[0], columns[0]]
    packed_faces = np.empty(len(faces), dtype=np.dtype([("ids", "<u4", (3,)), ("code", "u1")]))
    packed_faces["ids"], packed_faces["code"] = faces[:, :3], faces[:, 3]
    data = (struct.pack("<8sII4dII", b"AEBNDR01", len(columns), len(rows), west, north, dx, dy,
                        len(parameters), len(faces))
            + local_parameters.astype("<f8").tobytes() + packed_faces.tobytes())
    output_dir = output_dir or ROOT / "assets/boundaries"
    output = output_dir / f"{identifier}.aeb"
    provenance = {
        "schema": "ancientearth.boundaries.v1", "id": identifier, "prepared_date": "2026-10-21",
        "output_file": output.name, "output_sha256": hashlib.sha256(data).hexdigest(),
        "source": {**license_record, "license_review": "licenses/water-sources.json"},
        "source_affine_gdal": list(affine.to_gdal()), "source_row_bounds": row_bounds,
        "source_column_bounds": column_bounds, "source_stride": 3,
        "registration": {"width": len(columns), "height": len(rows), "west": west, "north": north,
                         "step_longitude_deg": dx, "step_latitude_deg": dy},
        "format": "AEBNDR01: u32base_width,u32base_height;4f64 west,north,coarse_step_lon,coarse_step_lat;u32vertex_count,u32face_count;vertex_count local[row,column]f64pairs;face_count[3u32indices,u8class]. No height or ECEF coordinate is stored.",
        "surface": {"model": "Exact WGS84 ellipsoid for all physical queries", "ellipsoidal_height_m": 0.0,
                    "presentation": "All parameters projected directly to WGS84 h=0; finite triangle chords are display approximations only"},
        "reconstruction": reconstruction,
        "triangle_counts_land_ocean_lake_river_mixed": np.bincount(faces[:, 3], minlength=5).tolist(),
        "limitations": ["Regional categorical coverage only; no inferred global land or ocean.",
                        "Smooth reconstructed borders do not add surveyed source resolution.",
                        "No DEM, geoid displacement, terrain relief, tides, wave geometry or bathymetry."]}
    metadata = output.with_suffix(".provenance.json")
    if check:
        if output.read_bytes() != data or json.loads(metadata.read_text()) != provenance:
            raise ValueError(f"{identifier}: boundary regeneration differs")
    else:
        output_dir.mkdir(parents=True, exist_ok=True)
        output.write_bytes(data)
        metadata.write_text(json.dumps(provenance, indent=2, allow_nan=False) + "\n")
    print(f"{identifier}: {len(parameters)} height-free vertices; {len(faces)} categorical faces")
    return provenance


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--output-dir", type=Path)
    args = parser.parse_args()
    for crop in CROPS:
        prepare(crop, args.check, args.output_dir)


if __name__ == "__main__":
    main()
