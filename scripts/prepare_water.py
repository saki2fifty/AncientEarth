"""Reconstruct regional water from registered Copernicus DSM/WBM source posts.

Run with the pinned optional terrain requirements and PYTHONPATH=.cache/python.
No download or source-post height editing. The declared shoreline interpolation
changes between-post geometry; it does not add surveyed coastline information.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
from pathlib import Path
import struct

import numpy as np
import rasterio

from prepare_terrain import (ROOT, HEADER, PATCHES, WGS84_A, WGS84_E2,
                             geoid_undulations, sha256, validate_grid, write_json)
from water_refinement import refine


def fnv64(data: bytes) -> int:
    value = 0xCBF29CE484222325
    for byte in data:
        value = ((value ^ byte) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return value


def conservative_codes(mask: np.ndarray, rows: np.ndarray, columns: np.ndarray) -> np.ndarray:
    """Categorical bounding-cell test, deliberately conservative at shorelines.

    Both NW-SE half triangles receive the class of the entire grid quadrilateral.
    Native nearest-post cell footprints extend half a source interval beyond
    the retained corners. assert_mask_enclosure proves they contain the actual
    Cartesian surface, rather than assuming geographic straight edges. Mixed cells receive4,
    prohibiting grass even when no water material can safely be assigned.
    """
    output = np.zeros((len(rows) - 1, len(columns) - 1, 2), dtype=np.uint8)
    for row in range(len(rows) - 1):
        for column in range(len(columns) - 1):
            values = mask[rows[row]:rows[row + 1] + 1,
                          columns[column]:columns[column + 1] + 1]
            first = int(values.flat[0])
            code = first if np.all(values == first) else (4 if np.any(values != 0) else 0)
            output[row, column, :] = code
    return output


def assert_mask_enclosure(heights: np.ndarray, longitude: np.ndarray,
                          latitude: np.ndarray, native_step: float) -> dict:
    """Prove every Cartesian triangle lies inside its categorical cell union.

    For northern latitude phi, F_phi=Z*cos(phi)-rho*sin(phi)+C(phi),
    C=e²*N(phi)*sin(phi)*cos(phi), vanishes on its geodetic latitude cone.
    The concave south-boundary F has its minimum at vertices. At the north
    boundary rho >= q dot (X,Y) bounds F by an affine expression whose maximum
    is at vertices. Positive/negative signs enclose the whole triangle. The
    longitude wedge is convex over these small <180degree spans.
    """
    if (heights.ndim != 2 or min(heights.shape) < 2
            or longitude.shape != heights.shape or latitude.shape != heights.shape
            or not np.isfinite(native_step)
            or not all(np.isfinite(array).all() for array in (heights, longitude, latitude))):
        raise ValueError("Mask enclosure requires matching finite nonempty coordinate grids")
    dx = np.diff(longitude[0, :])
    dy = -np.diff(latitude[:, 0])
    if (not np.all(longitude == longitude[:1, :]) or not np.all(latitude == latitude[:, :1])
            or not np.all(dx > 0) or not np.all(dy > 0)
            or not np.allclose(dx, dx[0], rtol=0, atol=1e-12)
            or not np.allclose(dy, dy[0], rtol=0, atol=1e-12)
            or longitude.min() < -180 or longitude.max() > 180
            or longitude.max() - longitude.min() >= 180):
        raise ValueError("Mask enclosure requires a regular eastward/southward unwrapped grid")
    if (heights.min() < -10_000 or heights.max() > 100_000
            or latitude.min() - native_step / 2 <= 0 or latitude.max() + native_step / 2 >= 89
            or not 0 < native_step <= 1 / 3600 + 1e-15):
        raise ValueError("Mask enclosure is outside its reviewed northern near-surface domain")
    phi, lam = np.radians(latitude), np.radians(longitude)
    n = WGS84_A / np.sqrt(1 - WGS84_E2 * np.sin(phi)**2)
    radial = (n + heights) * np.cos(phi)
    x, y = radial * np.cos(lam), radial * np.sin(lam)
    z = (n * (1 - WGS84_E2) + heights) * np.sin(phi)
    phi_s = np.radians(latitude[1:, :-1] - native_step / 2)
    phi_n = np.radians(latitude[:-1, :-1] + native_step / 2)
    midpoint_lon = (lam[:-1, :-1] + lam[:-1, 1:]) / 2
    if np.max(lam[:-1, 1:] - lam[:-1, :-1]) >= np.pi:
        raise ValueError("Longitude interval does not define a convex wedge")
    def cone_offset(angle: np.ndarray) -> np.ndarray:
        return (WGS84_E2 * WGS84_A / np.sqrt(1 - WGS84_E2 * np.sin(angle)**2)
                * np.sin(angle) * np.cos(angle))
    south_min, north_max = float("inf"), -float("inf")
    for oy, ox in ((0, 0), (0, 1), (1, 0), (1, 1)):
        corner = (slice(oy, heights.shape[0] - 1 + oy), slice(ox, heights.shape[1] - 1 + ox))
        cx, cy, cz = x[corner], y[corner], z[corner]
        south = cz * np.cos(phi_s) - np.hypot(cx, cy) * np.sin(phi_s) + cone_offset(phi_s)
        projection = cx * np.cos(midpoint_lon) + cy * np.sin(midpoint_lon)
        north = cz * np.cos(phi_n) - projection * np.sin(phi_n) + cone_offset(phi_n)
        south_min = min(south_min, float(south.min()))
        north_max = max(north_max, float(north.max()))
    if south_min <= 0.001 or north_max >= -0.001:
        raise ValueError("Native mask cells do not provably enclose the entire canonical triangles")
    return {"minimum_south_F_metres": south_min, "maximum_north_affine_F_metres": north_max,
            "required_sign_margin_metres": 0.001,
            "method": "Geodetic latitude cone concave south bound and affine north bound at all four quad vertices; longitude convex wedge"}


def prepare(patch: dict, check: bool, output_dir: Path | None = None) -> dict:
    identifier = patch["id"]
    terrain_path = ROOT / "assets/terrain" / f"{identifier}.aet"
    metadata = json.loads(terrain_path.with_suffix(".provenance.json").read_text())
    terrain = terrain_path.read_bytes()
    if hashlib.sha256(terrain).hexdigest() != metadata["output_sha256"]:
        raise ValueError("Terrain differs from its reviewed provenance")
    magic, width, height, west, north, dx, dy = HEADER.unpack_from(terrain)
    if magic != b"AETERR01" or len(terrain) != HEADER.size + width * height * 8:
        raise ValueError("Invalid canonical terrain file")
    source_path = ROOT / ".cache/terrain" / f"{patch['tile']}.tif"
    if sha256(source_path) != metadata["dem"]["source_sha256"]:
        raise ValueError("DSM differs from the terrain's original source")
    tile = f"{patch['tile'][:3]}_00_{patch['tile'][3:]}_00"
    mask_path = ROOT / ".cache/water" / f"{tile}-WBM.tif"
    review = json.loads((ROOT / "licenses/water-sources.json").read_text())
    source_license = next(item for item in review["files"] if item["filename"] == mask_path.name)
    if sha256(mask_path) != source_license["sha256"]:
        raise ValueError("Mask differs from the exact licensed source")
    with rasterio.open(source_path) as source, rasterio.open(mask_path) as wbm:
        validate_grid(source, 4326)
        validate_grid(wbm, 4326)
        if (source.shape != wbm.shape or source.transform != wbm.transform
                or wbm.dtypes != ("uint8",)):
            raise ValueError("WBM and DSM must have identical native post registration")
        mask_values = wbm.read(1, masked=True)
        if np.ma.getmaskarray(mask_values).any() or not np.isin(mask_values, [0, 1, 2, 3]).all():
            raise ValueError("Invalid or missing native water categories")
        inverse = ~source.transform
        c0, r0 = inverse * (west, north)
        c0, r0 = c0 - 0.5, r0 - 0.5
        sx, sy = dx / source.transform.a, dy / -source.transform.e
        if max(abs(v - round(v)) for v in (c0, r0, sx, sy)) > 1e-7:
            raise ValueError("Terrain posts are not aligned with native WBM posts")
        rows = round(r0) + np.arange(height, dtype=np.int64) * round(sy)
        columns = round(c0) + np.arange(width, dtype=np.int64) * round(sx)
        if rows[0] < 0 or columns[0] < 0 or rows[-1] >= wbm.height or columns[-1] >= wbm.width:
            raise ValueError("Missing classification coverage")
        # This reviewed importer is intentionally limited to the existing three-
        # arcsecond patches, rather than silently generalizing its margin bound.
        if round(sx) != 3 or round(sy) != 3 or max(dx, dy) > 1.0 / 1200.0 + 1e-15:
            raise ValueError("Unsupported sampling interval")
        codes = conservative_codes(np.asarray(mask_values), rows, columns)
        native_heights = source.read(1, masked=True)
        heights = np.asarray(native_heights[np.ix_(rows, columns)], dtype=np.float64)
        if np.ma.getmaskarray(native_heights[np.ix_(rows, columns)]).any():
            raise ValueError("Missing DSM height")
        longitude, latitude = np.meshgrid(west + np.arange(width) * dx, north - np.arange(height) * dy)
        registration = list(source.transform.to_gdal())
        native_affine = source.transform
    # Validate ALL classified ocean vertices, not a few plausible example points.
    ocean_cells = codes[:, :, 0] == 1
    ocean_posts = np.zeros((height, width), dtype=bool)
    for oy, ox in ((0, 0), (0, 1), (1, 0), (1, 1)):
        ocean_posts[oy:height - 1 + oy, ox:width - 1 + ox] |= ocean_cells
    if ocean_posts.any() and not np.all(heights[ocean_posts] == 0.0):
        raise ValueError("Classified ocean has nonzero source EGM2008 height; refusing to flatten")
    geoid_path = ROOT / ".cache/terrain/us_nga_egm08_25.tif"
    if sha256(geoid_path) != metadata["geoid"]["source_sha256"]:
        raise ValueError("Geoid differs from the terrain's original conversion source")
    with rasterio.open(geoid_path) as geoid:
        validate_grid(geoid, 4979)
        undulation = geoid_undulations(geoid, longitude, latitude)
    existing = np.frombuffer(terrain, dtype="<f8", offset=48).reshape(height, width)
    enclosure = assert_mask_enclosure(existing, longitude, latitude, float(dx / round(sx)))
    conversion_error = float(np.max(np.abs(existing - (heights + undulation))))
    if conversion_error > 1e-8:
        raise ValueError("Existing terrain does not match registered source H + N")
    with rasterio.open(geoid_path) as geoid:
        extra_xyz, faces, refinement = refine(np.asarray(mask_values), native_heights, rows, columns,
                                               native_affine, existing, geoid, codes)
    packed_faces = np.empty(len(faces), dtype=np.dtype([("vertices", "<u4", (3,)), ("code", "u1")]))
    packed_faces["vertices"], packed_faces["code"] = faces[:, :3], faces[:, 3]
    companion = (b"AEWATR02" + terrain[8:48] + struct.pack("<QII", fnv64(terrain), len(extra_xyz), len(faces))
                 + extra_xyz.astype("<f8").tobytes(order="C") + packed_faces.tobytes(order="C"))
    output_dir = output_dir or terrain_path.parent
    output = output_dir / f"{identifier}.aew"
    counts = np.bincount(faces[:, 3], minlength=5).tolist()
    provenance = {
        "schema": "ancientearth.water-provenance.v3", "id": identifier,
        "prepared_date": "2026-10-21", "output_file": output.name,
        "output_sha256": hashlib.sha256(companion).hexdigest(),
        "terrain_file": terrain_path.name, "terrain_sha256": metadata["output_sha256"],
        "format": "AEWATR02: magic8, exact AETERR01 header bytes8..48, little-endian FNV-1a64 terrain checksum; u32 addedXYZcount,u32 facecount; addedXYZcount ECEFf64 triples; facecount records of3u32 globalvertexindices+u8class (13bytes). Original vertices precede added ones.",
        "source": {**source_license, "license_review": "licenses/water-sources.json",
                   "dataset": "Copernicus GLO-30 AWS matching Water Body Mask",
                   "area_or_point": "Point", "affine_gdal": registration,
                   "source_codes": {"0": "non-water", "1": "ocean", "2": "lake", "3": "river"}},
        "classification": "Ocean coast quads retain original native DEM/WBM posts. Binary native cells use outer-edge midpoints joined by a bounded, parent-contained G1 cubic reconstruction; only ambiguous checkerboards retain the fixed diagonal topology. Retained coarse quads use conservative unanimous4x4 source categories; unresolved inland mixtures are class4 without grass.",
        "shoreline_refinement": refinement,
        "cartesian_footprint_enclosure": enclosure,
        "triangle_counts_land_ocean_lake_river_mixed": counts,
        "source_row_bounds": [int(rows[0]), int(rows[-1])],
        "source_column_bounds": [int(columns[0]), int(columns[-1])],
        "checked_ocean_post_count": int(ocean_posts.sum()),
        "ocean_orthometric_height": "Every retained/new native ocean post is checked against sourceH=0; modeled shoreline crossings and curve samples use H=0+N. Source-post heights unchanged; coast/transition interpolation refined.",
        "all_posts_H_plus_N_max_difference_m": conversion_error,
        "geoid_source_sha256": metadata["geoid"]["source_sha256"],
        "limitations": [
            "The ocean coast is reconstructed from native roughly30m posts using a declared smooth interpolation; its fine finite triangles do not add surveyed or subpixel source accuracy. Inland water and unresolved category mixtures remain conservative.",
            "Source-post heights are unchanged, but new native and modeled shoreline vertices change interpolation and ranges in coastal/stitched transition cells. Original .aet files remain unchanged.",
            "Cartesian triangles approximate the geoid between posts; this is a static geoid-referenced surface, not actual tides, waves, mean dynamic topography, or historical shorelines.",
            "Lake and river triangles retain their original source DSM levels, never forced to ocean zero. No bathymetry or worldwide ocean coverage is added.",
        ],
    }
    provenance_path = output_dir / f"{identifier}.water.provenance.json"
    if check:
        if output.read_bytes() != companion or json.loads(provenance_path.read_text()) != provenance:
            raise ValueError(f"{identifier}: generated water data differs")
    else:
        output_dir.mkdir(parents=True, exist_ok=True)
        output.write_bytes(companion)
        write_json(provenance_path, provenance)
    print(f"{identifier}: land/ocean/lake/river/mixed triangles {counts}, checked {ocean_posts.sum()} ocean posts")
    return provenance


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--region", choices=[patch["id"] for patch in PATCHES])
    parser.add_argument("--output-dir", type=Path)
    args = parser.parse_args()
    for patch in PATCHES:
        if args.region is None or patch["id"] == args.region:
            prepare(patch, args.check, args.output_dir)


if __name__ == "__main__":
    main()
