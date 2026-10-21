"""Losslessly package every native EGM2008 2.5-arc-minute grid sample.

Requires numpy, rasterio and pyproj. No resampling, precision reduction, or
vertical datum change is applied. AGEGEO01 is gzip-compressed and contains a
48-byte little-endian header followed by predicted rows of native float32 bits.
Within each row, four little-endian byte planes are concatenated, then each byte
is replaced by its difference modulo 256 from the previous shuffled byte. The
previous byte resets to zero at each row. Decode by a modulo-256 prefix sum,
split into four byte planes, interleave, and reinterpret as little-endian f32.
"""

from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import math
from pathlib import Path
import struct
import zlib

import numpy as np
import pyproj
import rasterio


ROOT = Path(__file__).resolve().parents[1]
HEADER = struct.Struct("<8sII4d")
MAGIC = b"AGEGEO01"
SOURCE_URL = "https://cdn.proj.org/us_nga_egm08_25.tif"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def predict_row(row: np.ndarray) -> bytes:
    shuffled = row.astype("<f4", copy=False).view(np.uint8).reshape(-1, 4).T.reshape(-1)
    result = np.empty_like(shuffled)
    result[0] = shuffled[0]
    np.subtract(shuffled[1:], shuffled[:-1], out=result[1:])
    return result.tobytes()


def decode_row(encoded: bytes, width: int) -> bytes:
    if len(encoded) != width * 4:
        raise ValueError("Truncated predicted row")
    shuffled = np.cumsum(np.frombuffer(encoded, dtype=np.uint8), dtype=np.uint32).astype(np.uint8)
    return shuffled.reshape(4, width).T.copy().tobytes()


def interpolate_native(
    values: np.ndarray, longitude: float, latitude: float,
    west: float, north: float, step_lon: float, step_lat: float,
) -> float:
    """Bilinear longitude-periodic interpolation, including both polar rows."""
    if not math.isfinite(longitude) or not math.isfinite(latitude) or not -90 <= latitude <= 90:
        raise ValueError("Expected finite longitude and valid geodetic latitude")
    height, width = values.shape
    x = ((longitude - west) / step_lon) % width
    y = (north - latitude) / step_lat
    left = math.floor(x)
    top = min(math.floor(y), height - 2)
    right = (left + 1) % width
    wx, wy = x - left, y - top
    return (
        float(values[top, left]) * (1.0 - wx) * (1.0 - wy)
        + float(values[top, right]) * wx * (1.0 - wy)
        + float(values[top + 1, left]) * (1.0 - wx) * wy
        + float(values[top + 1, right]) * wx * wy
    )


def independent_proj_fixtures(
    source: Path, values: np.ndarray, west: float, north: float,
    step_lon: float, step_lat: float,
) -> tuple[list[dict], float]:
    transformer = pyproj.Transformer.from_pipeline(
        "+proj=pipeline +step +proj=unitconvert +xy_in=deg +xy_out=rad "
        f"+step +proj=vgridshift +grids={source.resolve().as_posix()} +multiplier=1 "
        "+step +proj=unitconvert +xy_in=rad +xy_out=deg"
    )
    points = [
        ("Greenwich equator", 0.0, 0.0),
        ("Tokyo", 139.69171, 35.6895),
        ("Fuji", 138.73, 35.36),
        ("Sydney", 151.2093, -33.8688),
        ("Denver", -104.9903, 39.7392),
        ("Reykjavik", -21.9426, 64.1466),
        ("Pacific", -145.125, -25.375),
        ("West seam", -180.0, 45.123),
        ("East seam", 180.0, 45.123),
        ("Just west of seam", 179.999, 45.123),
        ("Just east of seam", -179.999, 45.123),
        ("North pole Greenwich", 0.0, 90.0),
        ("North pole seam", 180.0, 90.0),
        ("South pole Greenwich", 0.0, -90.0),
        ("South pole seam", -180.0, -90.0),
        ("Near north pole", 123.456, 89.999),
        ("Near south pole", -123.456, -89.999),
    ]
    fixtures = []
    for label, longitude, latitude in points:
        _, _, expected = transformer.transform(longitude, latitude, 0.0, errcheck=True)
        actual = interpolate_native(values, longitude, latitude, west, north, step_lon, step_lat)
        error = abs(actual - expected)
        if error > 1e-8:
            raise ValueError(f"{label}: bilinear result differs from PROJ by {error} m")
        fixtures.append({
            "label": label,
            "longitude_degrees": longitude,
            "latitude_degrees": latitude,
            "proj_geoid_undulation_metres": expected,
            "native_bilinear_geoid_undulation_metres": actual,
            "absolute_difference_metres": error,
        })
    return fixtures, max(fixture["absolute_difference_metres"] for fixture in fixtures)


class CountingSink:
    """Count a comparison gzip stream without retaining a second large asset."""

    def __init__(self) -> None:
        self.size = 0

    def write(self, data: bytes) -> int:
        self.size += len(data)
        return len(data)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=ROOT / ".cache/terrain/us_nga_egm08_25.tif")
    parser.add_argument("--output", type=Path, default=ROOT / "assets/geoid/egm2008-native.aeg.gz")
    parser.add_argument("--compare-plain", action="store_true")
    args = parser.parse_args()
    with rasterio.open(args.source) as source:
        tags = source.tags()
        affine = source.transform
        if (source.width, source.height, source.count) != (8640, 4321, 1):
            raise ValueError("Expected every sample of the native 8640x4321 EGM2008 grid")
        if source.crs is None or source.crs.to_epsg() != 4979:
            raise ValueError("Expected source WGS84 geographic 3D CRS EPSG:4979")
        if (tags.get("target_crs_epsg_code") != "3855"
                or tags.get("TYPE") != "VERTICAL_OFFSET_GEOGRAPHIC_TO_VERTICAL"
                or tags.get("AREA_OR_POINT") != "Point"):
            raise ValueError("Expected an EGM2008 geographic-to-vertical point grid")
        if source.dtypes != ("float32",) or source.scales != (1.0,) or source.offsets != (0.0,):
            raise ValueError("Expected native unscaled float32 geoid undulations")
        if affine.b != 0 or affine.d != 0 or affine.a <= 0 or affine.e >= 0:
            raise ValueError("Expected north-up geographic point grid")
        west, north = map(float, source.xy(0, 0))
        step_lon, step_lat = float(affine.a), float(-affine.e)
        if west != -180.0 or north != 90.0 or step_lon != 1.0 / 24.0 or step_lat != 1.0 / 24.0:
            raise ValueError("Unexpected native sample registration")
        masked = source.read(1, masked=True)
        if np.ma.getmaskarray(masked).any():
            raise ValueError("Source contains missing values; refusing to invent geoid heights")
        values = np.asarray(masked, dtype="<f4")
        if not np.isfinite(values).all():
            raise ValueError("Source contains non-finite geoid heights")
        for row in (values[0], values[-1]):
            if not np.all(row.view("<u4") == row[0:1].view("<u4")):
                raise ValueError("Native polar samples are not bitwise constant")
        header = HEADER.pack(MAGIC, source.width, source.height, west, north, step_lon, step_lat)
        source_affine = list(affine.to_gdal())
    fixtures, max_proj_error = independent_proj_fixtures(
        args.source, values, west, north, step_lon, step_lat
    )
    print(f"Verified source registration and {len(fixtures)} PROJ fixtures; max difference {max_proj_error:.3g} m", flush=True)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("wb") as output:
        with gzip.GzipFile(fileobj=output, filename="", mode="wb", compresslevel=9, mtime=0) as packed:
            packed.write(header)
            for row in values:
                packed.write(predict_row(row))
    native_digest = hashlib.sha256()
    decoded_digest = hashlib.sha256()
    with gzip.open(args.output, "rb") as packed:
        if packed.read(HEADER.size) != header:
            raise ValueError("Round-trip header mismatch")
        for row in values:
            native = row.tobytes()
            decoded = decode_row(packed.read(values.shape[1] * 4), values.shape[1])
            if decoded != native:
                raise ValueError("Lossless round-trip failed: native float32 bits changed")
            native_digest.update(native)
            decoded_digest.update(decoded)
        if packed.read(1):
            raise ValueError("Unexpected bytes after final row")
    print(f"Full native-bit round trip passed; output {args.output.stat().st_size} bytes", flush=True)
    comparison = None
    if args.compare_plain:
        sink = CountingSink()
        with gzip.GzipFile(fileobj=sink, filename="", mode="wb", compresslevel=9, mtime=0) as plain:
            plain.write(header)
            for row in values:
                plain.write(row.tobytes())
        comparison = {"plain_float32_gzip_bytes": sink.size,
                      "predicted_float32_gzip_bytes": args.output.stat().st_size}
        print(f"Plain float32 gzip comparison: {sink.size} bytes", flush=True)
    metadata = {
        "schema": "ancientearth.geoid-provenance.v1",
        "prepared_date": "2026-10-20",
        "dataset": "NGA EGM2008 native 2.5 arc-minute geoid undulation grid distributed by PROJ",
        "source_file": args.source.name,
        "source_url": SOURCE_URL,
        "source_sha256": sha256(args.source),
        "source_metadata": tags,
        "source_affine_gdal": source_affine,
        "output_file": args.output.name,
        "output_sha256": sha256(args.output),
        "output_bytes": args.output.stat().st_size,
        "native_float32_payload_bytes": values.nbytes,
        "native_float32_payload_sha256": native_digest.hexdigest(),
        "decoded_float32_payload_sha256": decoded_digest.hexdigest(),
        "decoded_payload_bitwise_equals_source": True,
        "dimensions_samples": [values.shape[1], values.shape[0]],
        "west_sample_center_degrees": west,
        "north_sample_center_degrees": north,
        "positive_longitude_step_degrees": step_lon,
        "positive_latitude_step_degrees": step_lat,
        "east_sample_center_degrees": west + (values.shape[1] - 1) * step_lon,
        "south_sample_center_degrees": north - (values.shape[0] - 1) * step_lat,
        "registration": "PixelIsPoint; GDAL source.xy(row,column) returns sample centers, with no additional half-cell shift",
        "longitude_periodic": True,
        "longitude_seam": "8640 unique longitudes; interpolate east neighbor of final column using column zero",
        "north_pole_undulation_metres": float(values[0, 0]),
        "south_pole_undulation_metres": float(values[-1, 0]),
        "polar_rows_bitwise_constant": True,
        "geoid_undulation_range_metres": [float(values.min()), float(values.max())],
        "resampling": "None: all 8640x4321 native float32 samples preserved bit-for-bit",
        "height_semantics": "N is geoid undulation above the WGS84 ellipsoid; ellipsoidal height h = EGM2008 orthometric height H + N",
        "recommended_sampling": "Bilinear in geographic longitude/latitude, wrapping the longitude seam; explicit polar rows",
        "container": "gzip, compression level 9, mtime=0, no original filename",
        "binary_header": {
            "bytes": HEADER.size,
            "struct_format": "<8sII4d",
            "fields": ["AGEGEO01", "u32 width", "u32 height", "f64 west sample center degrees",
                       "f64 north sample center degrees", "f64 positive longitude step degrees",
                       "f64 positive latitude step degrees"],
        },
        "binary_payload_codec": "Rows north-to-south. For each row: native little-endian f32 words -> concatenate byte planes 0,1,2,3 -> byte differences modulo 256 against previous shuffled byte, resetting previous=0 at row start. Decoder prefix-sums modulo 256 across the full row, unshuffles four planes, then decodes little-endian f32.",
        "independent_reference": "PROJ vgridshift using the original official GeoTIFF, multiplier=+1",
        "independent_proj_max_difference_metres": max_proj_error,
        "independent_proj_fixtures": fixtures,
        "compression_comparison": comparison,
        "license": "Public domain; source TIFF credits NGA",
        "license_reference": "https://github.com/OSGeo/PROJ-data/blob/master/us_nga/us_nga_README.txt",
        "sign_and_registration_reference": "https://proj.org/en/stable/specifications/geodetictiffgrids.html",
        "software": {"numpy": np.__version__, "rasterio": rasterio.__version__,
                     "pyproj": pyproj.__version__, "zlib": zlib.ZLIB_VERSION},
        "limitations": [
            "This finite-resolution gravity model is not terrain, bathymetry, local mean sea level, or a live tide model.",
            "Lossless packaging preserves the source precision; it does not remove model uncertainty.",
            "Bilinear interpolation is an approximation between native posts; no resampling or extra grid decimation occurred in this asset.",
        ],
    }
    provenance = args.output.with_name("egm2008-native.provenance.json")
    provenance.write_text(json.dumps(metadata, indent=2, allow_nan=False) + "\n", encoding="utf-8")
    print(f"SHA256 {metadata['output_sha256']}", flush=True)


if __name__ == "__main__":
    main()
