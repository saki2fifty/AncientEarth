"""Create source-backed WGS84 ellipsoidal terrain patches.

Usage: python scripts/prepare_terrain.py
Requires numpy, rasterio and pyproj. Source downloads live in .cache/terrain.
No DEM interpolation or averaging is performed: every third native sample is
retained. EGM2008 geoid undulation alone is interpolated bilinearly, then h=H+N.
The 20 km request is a local-curvature approximation; actual sample bounds and
geodesic center-line extents are recorded rather than advertised as exactly 20 km.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
from pathlib import Path
import struct

import numpy as np
import pyproj
import rasterio
from rasterio.windows import Window

from imagery_coordinates import bounds_for_tiles, tile_range, naip_export_plan


ROOT = Path(__file__).resolve().parents[1]
WGS84_A = 6_378_137.0
WGS84_F = 1.0 / 298.257223563
WGS84_E2 = WGS84_F * (2.0 - WGS84_F)
STRIDE = 3
HEADER = struct.Struct("<8sII4d")
PATCHES = [
    {"id": "fuji", "latitude": 35.36, "longitude": 138.73, "tile": "N35E138", "imagery": "gsi"},
    {"id": "tokyo", "latitude": 35.6895, "longitude": 139.69171, "tile": "N35E139", "imagery": "gsi"},
    {"id": "houston", "latitude": 29.76328, "longitude": -95.36327, "tile": "N29W096",
     "imagery": "usgs_naip", "geonames_id": 4699066},
    {"id": "galveston", "latitude": 29.30135, "longitude": -94.7977, "tile": "N29W095",
     "imagery": "usgs_naip", "geonames_id": 4692883},
]
COPERNICUS_INFO = (
    "https://dataspace.copernicus.eu/explore-data/data-collections/"
    "copernicus-contributing-missions/collections-description/COP-DEM"
)
GEOID_SIGN_REFERENCE = "https://proj.org/en/stable/specifications/geodetictiffgrids.html"
GEOID_SOURCE = "https://cdn.proj.org/us_nga_egm08_25.tif"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def write_json(path: Path, value: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, allow_nan=False) + "\n", encoding="utf-8")


def requested_bounds(latitude: float, longitude: float, width_m: float) -> dict[str, float]:
    """Local rectangular request from WGS84 meridional/prime-vertical curvature."""
    phi = math.radians(latitude)
    denominator = 1.0 - WGS84_E2 * math.sin(phi) ** 2
    prime_vertical = WGS84_A / math.sqrt(denominator)
    meridional = WGS84_A * (1.0 - WGS84_E2) / denominator**1.5
    half_lat = math.degrees(width_m / (2.0 * meridional))
    half_lon = math.degrees(width_m / (2.0 * prime_vertical * math.cos(phi)))
    return {
        "west": longitude - half_lon,
        "south": latitude - half_lat,
        "east": longitude + half_lon,
        "north": latitude + half_lat,
    }


def validate_grid(dataset: rasterio.io.DatasetReader, expected_epsg: int) -> None:
    affine = dataset.transform
    if dataset.crs is None or dataset.crs.to_epsg() != expected_epsg:
        raise ValueError(f"{dataset.name}: expected EPSG:{expected_epsg}")
    if affine.b != 0 or affine.d != 0 or affine.a <= 0 or affine.e >= 0:
        raise ValueError(f"{dataset.name}: expected eastward columns and southward rows")
    if dataset.tags().get("AREA_OR_POINT") != "Point":
        raise ValueError(f"{dataset.name}: expected point sample registration")
    if dataset.count != 1 or dataset.scales[0] != 1 or dataset.offsets[0] != 0:
        raise ValueError(f"{dataset.name}: expected one unscaled height band")


def checked_values(values: np.ma.MaskedArray, label: str) -> np.ndarray:
    if np.ma.getmaskarray(values).any():
        raise ValueError(f"{label}: nodata found; refusing to substitute heights")
    result = np.asarray(values, dtype=np.float64)
    if not np.isfinite(result).all() or (result == -32767).any():
        raise ValueError(f"{label}: non-finite or void sentinel height")
    return result


def geoid_undulations(
    grid: rasterio.io.DatasetReader, longitudes: np.ndarray, latitudes: np.ndarray
) -> np.ndarray:
    """Bilinear interpolation between GDAL-registered geoid sample centers."""
    inverse = ~grid.transform
    columns, rows = inverse * (longitudes, latitudes)
    columns = np.asarray(columns) - 0.5
    rows = np.asarray(rows) - 0.5
    left = np.floor(columns).astype(np.int64)
    top = np.floor(rows).astype(np.int64)
    if left.min() < 0 or top.min() < 0 or left.max() + 1 >= grid.width or top.max() + 1 >= grid.height:
        raise ValueError("Requested crop lacks a complete geoid interpolation neighborhood")
    x0, y0 = int(left.min()), int(top.min())
    x1, y1 = int(left.max()) + 1, int(top.max()) + 1
    sample = checked_values(
        grid.read(1, window=Window(x0, y0, x1 - x0 + 1, y1 - y0 + 1), masked=True),
        "EGM2008 geoid",
    )
    c, r = left - x0, top - y0
    wx, wy = columns - left, rows - top
    return (
        sample[r, c] * (1.0 - wx) * (1.0 - wy)
        + sample[r, c + 1] * wx * (1.0 - wy)
        + sample[r + 1, c] * (1.0 - wx) * wy
        + sample[r + 1, c + 1] * wx * wy
    )


def crop_samples(
    source: rasterio.io.DatasetReader, bounds: dict[str, float]
) -> tuple[np.ndarray, np.ndarray, np.ndarray, dict]:
    inverse = ~source.transform
    west_column, north_row = inverse * (bounds["west"], bounds["north"])
    east_column, south_row = inverse * (bounds["east"], bounds["south"])
    # The inverse affine returns pixel-edge coordinates; -0.5 gives sample indices.
    # Anchor the stride at the source's row/column zero for reproducibility.
    c0 = math.ceil((west_column - 0.5) / STRIDE) * STRIDE
    r0 = math.ceil((north_row - 0.5) / STRIDE) * STRIDE
    c1 = math.floor((east_column - 0.5) / STRIDE) * STRIDE
    r1 = math.floor((south_row - 0.5) / STRIDE) * STRIDE
    if not (0 <= c0 < c1 < source.width and 0 <= r0 < r1 < source.height):
        raise ValueError("Requested terrain crop is not contained in the source tile")
    heights = checked_values(
        source.read(1, window=Window(c0, r0, c1 - c0 + 1, r1 - r0 + 1), masked=True)[::STRIDE, ::STRIDE],
        source.name,
    )
    columns, rows = np.meshgrid(
        np.arange(c0, c1 + 1, STRIDE, dtype=np.float64),
        np.arange(r0, r1 + 1, STRIDE, dtype=np.float64),
    )
    # Rasterio/GDAL already honors PixelIsPoint; do not apply a second half-pixel shift.
    longitudes, latitudes = source.transform * (columns + 0.5, rows + 0.5)
    registration = {
        "source_area_or_point": source.tags()["AREA_OR_POINT"],
        "source_affine_gdal": list(source.transform.to_gdal()),
        "source_first_sample_lon_lat_deg": list(source.transform * (0.5, 0.5)),
        "first_source_row_column": [r0, c0],
        "last_source_row_column": [r1, c1],
        "row_and_column_stride": STRIDE,
        "coordinate_method": "GDAL affine at (column+0.5,row+0.5); PixelIsPoint handled once by GDAL",
    }
    return heights, longitudes, latitudes, registration


def verify_geoid_with_proj(
    geoid_path: Path, longitude: np.ndarray, latitude: np.ndarray, undulation: np.ndarray
) -> float:
    """Independently compare interpolation/sign to PROJ's vertical-grid operation."""
    transformer = pyproj.Transformer.from_pipeline(
        "+proj=pipeline +step +proj=unitconvert +xy_in=deg +xy_out=rad "
        f"+step +proj=vgridshift +grids={geoid_path.resolve().as_posix()} +multiplier=1 "
        "+step +proj=unitconvert +xy_in=rad +xy_out=deg"
    )
    samples = [(0, 0), (0, -1), (-1, 0), (-1, -1),
               (longitude.shape[0] // 2, longitude.shape[1] // 2)]
    differences = []
    for row, column in samples:
        _, _, proj_n = transformer.transform(
            float(longitude[row, column]), float(latitude[row, column]), 0.0, errcheck=True
        )
        differences.append(abs(proj_n - float(undulation[row, column])))
    maximum = max(differences)
    if maximum > 1e-8:
        raise ValueError(f"Geoid interpolation differs from PROJ by {maximum} m")
    return maximum


def prepare_patch(patch: dict, source_dir: Path, output_dir: Path, geoid_path: Path) -> dict:
    source_path = source_dir / f"{patch['tile']}.tif"
    bounds = requested_bounds(patch["latitude"], patch["longitude"], 20_000.0)
    with rasterio.open(source_path) as source, rasterio.open(geoid_path) as geoid:
        validate_grid(source, 4326)
        validate_grid(geoid, 4979)
        geoid_tags = geoid.tags()
        if (geoid_tags.get("TYPE") != "VERTICAL_OFFSET_GEOGRAPHIC_TO_VERTICAL"
                or geoid_tags.get("target_crs_epsg_code") != "3855"):
            raise ValueError("Expected the official WGS84-to-EGM2008 geoid grid")
        orthometric, longitude, latitude, registration = crop_samples(source, bounds)
        undulation = geoid_undulations(geoid, longitude, latitude)
        proj_error = verify_geoid_with_proj(geoid_path, longitude, latitude, undulation)
        ellipsoidal = orthometric + undulation
        step_lon = float(source.transform.a * STRIDE)
        step_lat = float(-source.transform.e * STRIDE)
        source_dimensions = [source.width, source.height]

    height, width = ellipsoidal.shape
    actual = {
        "west": float(longitude[0, 0]),
        "south": float(latitude[-1, 0]),
        "east": float(longitude[0, -1]),
        "north": float(latitude[0, 0]),
    }
    output_dir.mkdir(parents=True, exist_ok=True)
    output_path = output_dir / f"{patch['id']}.aet"
    output_path.write_bytes(
        HEADER.pack(b"AETERR01", width, height, actual["west"], actual["north"], step_lon, step_lat)
        + ellipsoidal.astype("<f8").tobytes(order="C")
    )
    if output_path.stat().st_size != HEADER.size + width * height * 8:
        raise ValueError("Terrain binary length validation failed")
    geod = pyproj.Geod(ellps="WGS84")
    extent_east = geod.inv(actual["west"], patch["latitude"], actual["east"], patch["latitude"])[2]
    extent_north = geod.inv(patch["longitude"], actual["south"], patch["longitude"], actual["north"])[2]
    cog_name = f"Copernicus_DSM_COG_10_{patch['tile'][:3]}_00_{patch['tile'][3:]}_00_DEM"
    sample_spacing = {
        "east_west": geod.inv(patch["longitude"], patch["latitude"],
                             patch["longitude"] + step_lon, patch["latitude"])[2],
        "north_south": geod.inv(patch["longitude"], patch["latitude"],
                               patch["longitude"], patch["latitude"] + step_lat)[2],
    }
    metadata = {
        "schema": "ancientearth.terrain-provenance.v1",
        "id": patch["id"],
        "prepared_date": "2026-10-20",
        "format": "AETERR01: little-endian 8-byte magic, u32 width, u32 height, f64 west/north/positive longitude step/positive latitude step, row-major f64 ellipsoidal metres; rows progress south",
        "header_bytes": HEADER.size,
        "output_file": output_path.name,
        "output_sha256": sha256(output_path),
        "dimensions_samples": [width, height],
        "sample_bounds_degrees": actual,
        "sample_step_degrees": {"longitude": step_lon, "latitude": step_lat},
        "requested_center_lon_lat_degrees": [patch["longitude"], patch["latitude"]],
        "requested_center_source": (
            {"dataset": "GeoNames cities15000 snapshot", "geonames_id": patch["geonames_id"],
             "coordinate_fields": "Unchanged source latitude and longitude; representative place point",
             "source_reference": "https://download.geonames.org/export/dump/readme.txt"}
            if "geonames_id" in patch else {"meaning": "Application-selected regional window center"}
        ),
        "requested_bounds_degrees": bounds,
        "request_extent_method": "20 km local rectangle using WGS84 meridional and prime-vertical curvature at center; approximate extent, then crop inward to retained source samples",
        "actual_center_line_geodesic_extent_metres": {"east_west": extent_east, "north_south": extent_north},
        "registration": registration,
        "height_reference": "WGS84 ellipsoidal height (EPSG:4979), metres",
        "height_conversion": "h = H_EGM2008 + N_EGM2008; positive geoid undulation is above WGS84 ellipsoid",
        "orthometric_height_range_metres": [float(orthometric.min()), float(orthometric.max())],
        "geoid_undulation_range_metres": [float(undulation.min()), float(undulation.max())],
        "ellipsoidal_height_range_metres": [float(ellipsoidal.min()), float(ellipsoidal.max())],
        "dem": {
            "dataset": "Copernicus DEM GLO-30 Public, AWS 2021 release, digital surface model",
            "source_file": source_path.name,
            "source_sha256": sha256(source_path),
            "source_url": f"https://copernicus-dem-30m.s3.amazonaws.com/{cog_name}/{cog_name}.tif",
            "source_dimensions_samples": source_dimensions,
            "horizontal_crs": "WGS84 geographic (EPSG:4326)",
            "vertical_crs": "EGM2008 height (EPSG:3855)",
            "sampling": "Every third native sample in both axes; no averaging or height interpolation; 3 arc seconds in these regions",
            "sample_spacing_at_requested_center_metres": sample_spacing,
            "product_reference": COPERNICUS_INFO,
            "aws_processing_reference": "https://copernicus-dem-30m.s3.amazonaws.com/readme.html",
            "license_reference": "https://spacedata.copernicus.eu/collections/copernicus-digital-elevation-model",
            "citation": "Copernicus Digital Elevation Model was accessed on 2026-10-20 from https://registry.opendata.aws/copernicus-dem.",
        },
        "geoid": {
            "dataset": "NGA EGM2008, 2.5 arc-minute grid distributed by PROJ",
            "source_file": geoid_path.name,
            "source_sha256": sha256(geoid_path),
            "source_url": GEOID_SOURCE,
            "metadata": geoid_tags,
            "interpolation": "Bilinear in geographic sample-center longitude/latitude",
            "sign_reference": GEOID_SIGN_REFERENCE,
            "license": "Public domain",
            "provenance_reference": "https://github.com/OSGeo/PROJ-data/blob/master/us_nga/us_nga_README.txt",
            "independent_proj_check_samples": 5,
            "independent_proj_max_difference_metres": proj_error,
        },
        "software": {"rasterio": rasterio.__version__, "pyproj": pyproj.__version__, "numpy": np.__version__},
        "limitations": [
            "A finite-resolution DSM includes vegetation and buildings; it is not bare-earth ground or survey truth.",
            "Decimation can omit peaks and small features; source and geoid errors remain.",
            "The binary bounds locate outermost retained sample centers, not half-cell image edges.",
            "Surface geometry between samples depends on the application's declared mesh interpolation.",
        ],
    }
    write_json(output_dir / f"{patch['id']}.provenance.json", metadata)
    if patch["imagery"] == "usgs_naip":
        result = {"id": patch["id"], "bounds": actual, **naip_export_plan(actual)}
        print(f"{patch['id']}: {width}x{height}, {actual}, NAIP export, PROJ check {proj_error:.3g} m")
        return result
    tiles = tile_range(actual["west"], actual["south"], actual["east"], actual["north"], 14)
    result = {
        "id": patch["id"],
        "provider": "gsi",
        "bounds": actual,
        "tile_range": list(tiles),
        "mosaic_bounds_degrees": bounds_for_tiles(*tiles, 14),
        "zoom": 14,
        "tile_size_pixels": 256,
        "url_template": "https://cyberjapandata.gsi.go.jp/xyz/seamlessphoto/{z}/{x}/{y}.jpg",
    }
    print(f"{patch['id']}: {width}x{height}, {actual}, XYZ z14 {tiles}, PROJ check {proj_error:.3g} m")
    return result


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-dir", type=Path, default=ROOT / ".cache/terrain")
    parser.add_argument("--output-dir", type=Path, default=ROOT / "assets/terrain")
    parser.add_argument("--patch", action="append", choices=[patch["id"] for patch in PATCHES],
                        help="Prepare selected region(s); default is every configured region")
    args = parser.parse_args()
    geoid_path = args.source_dir / "us_nga_egm08_25.tif"
    plan = {
        "schema": "ancientearth.imagery-plan.v1",
        "projection": "EPSG:3857 spherical Web Mercator; no geographic-linear resampling",
        "bounds_semantics": "Terrain bounds are sample centers; mosaic bounds are full tile outer edges",
        "tile_range_order": ["xmin", "ymin", "xmax_inclusive", "ymax_inclusive"],
        "patches": [prepare_patch(patch, args.source_dir, args.output_dir, geoid_path) for patch in PATCHES
                    if args.patch is None or patch["id"] in args.patch],
    }
    write_json(args.source_dir / "imagery-plan.json", plan)


if __name__ == "__main__":
    main()
