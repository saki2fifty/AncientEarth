"""Independently verify every bundled terrain post against its source and PROJ.

Install optional dependencies with:
    python -m pip install -r scripts/requirements-terrain.txt
Then run:
    python scripts/verify_terrain.py

Original COGs and us_nga_egm08_25.tif default to .cache/terrain; AETERR01
files and provenance default to assets/terrain. No importer functions are
used. This checks registration, stride, hashes and h=H+N conversion, not the
physical accuracy of the DSM, geoid, or the mesh between retained posts.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import struct

import numpy as np
import pyproj
import rasterio


ROOT = Path(__file__).resolve().parents[1]
PATCH_SOURCES = (
    ("fuji", "N35E138.tif"),
    ("tokyo", "N35E139.tif"),
    ("houston", "N29W096.tif"),
    ("galveston", "N29W095.tif"),
)
REGISTRATION_TOLERANCE_DEG = 1e-12
HEIGHT_TOLERANCE_M = 1e-8


def require(condition: bool, message: str) -> None:
    """Keep verification active even when Python is run with -O."""
    if not condition:
        raise ValueError(message)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def verify_patch(
    patch: str,
    source_filename: str,
    source_dir: Path,
    terrain_dir: Path,
    geoid_sha256: str,
    transform: pyproj.Transformer,
) -> dict:
    binary = terrain_dir / f"{patch}.aet"
    source_path = source_dir / source_filename
    metadata = json.loads((terrain_dir / f"{patch}.provenance.json").read_text(encoding="utf-8"))
    raw = binary.read_bytes()
    require(len(raw) >= 48, f"{patch}: truncated terrain header")
    magic, width, height, west, north, step_lon, step_lat = struct.unpack("<8sII4d", raw[:48])
    require(magic == b"AETERR01", f"{patch}: unexpected terrain format")
    require(width >= 2 and height >= 2, f"{patch}: incomplete grid dimensions")
    require(len(raw) == 48 + 8 * width * height, f"{patch}: incomplete or trailing height data")
    require(np.isfinite([west, north, step_lon, step_lat]).all(), f"{patch}: nonfinite registration")
    require(step_lon > 0 and step_lat > 0, f"{patch}: invalid grid orientation")
    require([width, height] == metadata["dimensions_samples"], f"{patch}: provenance dimensions differ")
    require(hashlib.sha256(raw).hexdigest() == metadata["output_sha256"], f"{patch}: output hash differs")
    require(sha256(source_path) == metadata["dem"]["source_sha256"], f"{patch}: source COG hash differs")
    require(geoid_sha256 == metadata["geoid"]["source_sha256"], f"{patch}: source geoid hash differs")

    heights = np.frombuffer(raw, dtype="<f8", offset=48).reshape(height, width)
    require(np.isfinite(heights).all(), f"{patch}: nonfinite imported heights")
    longitude, latitude = np.meshgrid(
        west + np.arange(width) * step_lon,
        north - np.arange(height) * step_lat,
    )
    with rasterio.open(source_path) as source:
        require(source.crs is not None and source.crs.to_epsg() == 4326, f"{patch}: source CRS differs")
        require(source.tags().get("AREA_OR_POINT") == "Point", f"{patch}: source is not point-registered")
        require(source.count == 1 and source.scales[0] == 1 and source.offsets[0] == 0,
                f"{patch}: expected one unscaled elevation band")
        columns, rows = (~source.transform) * (longitude, latitude)
        # GDAL's affine locates outer pixel edges even for PixelIsPoint.
        # Recover original sample indices independently of the importer's crop.
        source_columns = np.rint(columns - 0.5).astype(np.int64)
        source_rows = np.rint(rows - 0.5).astype(np.int64)
        require(source_columns.min() >= 0 and source_columns.max() < source.width
                and source_rows.min() >= 0 and source_rows.max() < source.height,
                f"{patch}: sample outside source raster")
        checked_lon, checked_lat = source.transform * (source_columns + 0.5, source_rows + 0.5)
        registration_error = float(max(
            np.max(np.abs(longitude - checked_lon)),
            np.max(np.abs(latitude - checked_lat)),
        ))
        require(registration_error <= REGISTRATION_TOLERANCE_DEG,
                f"{patch}: source-post registration error {registration_error} degrees")
        require(np.all(np.diff(source_columns, axis=1) == 3)
                and np.all(np.diff(source_rows, axis=0) == 3)
                and np.all(np.diff(source_columns, axis=0) == 0)
                and np.all(np.diff(source_rows, axis=1) == 0)
                and np.all(source_columns % 3 == 0)
                and np.all(source_rows % 3 == 0),
                f"{patch}: retained posts are not the declared source-anchored stride of three")
        original = source.read(1, masked=True)[source_rows, source_columns]
        require(not np.ma.getmaskarray(original).any(), f"{patch}: source contains nodata")
        orthometric = np.asarray(original, dtype=np.float64)
        require(np.isfinite(orthometric).all() and not (orthometric == -32767).any(),
                f"{patch}: invalid source elevation")

    # PROJ performs its own geoid interpolation. Its positive multiplier adds
    # geoid undulation N to the original EGM2008 height H, yielding ellipsoid h.
    _, _, converted = transform.transform(
        longitude.ravel(), latitude.ravel(), orthometric.ravel(), errcheck=True,
    )
    converted = np.asarray(converted).reshape(height, width)
    require(np.isfinite(converted).all(), f"{patch}: nonfinite PROJ result")
    height_error = float(np.max(np.abs(converted - heights)))
    require(height_error <= HEIGHT_TOLERANCE_M,
            f"{patch}: imported height differs from source plus PROJ by {height_error} metres")
    return {
        "patch": patch,
        "samples_verified": width * height,
        "max_sample_registration_difference_degrees": registration_error,
        "max_height_difference_vs_independent_proj_metres": height_error,
        "ellipsoidal_height_range_metres": [float(heights.min()), float(heights.max())],
        "posts_below_reference_ellipsoid": int(np.count_nonzero(heights < 0.0)),
        "source_stride": 3,
        "source_cog_geoid_and_binary_hashes_match": True,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-dir", type=Path, default=ROOT / ".cache/terrain")
    parser.add_argument("--terrain-dir", type=Path, default=ROOT / "assets/terrain")
    parser.add_argument("--report", type=Path, help="Optional JSON report output path")
    args = parser.parse_args()
    source_dir = args.source_dir.resolve()
    geoid = source_dir / "us_nga_egm08_25.tif"
    geoid_hash = sha256(geoid)
    transform = pyproj.Transformer.from_pipeline(
        "+proj=pipeline "
        "+step +proj=unitconvert +xy_in=deg +xy_out=rad "
        f'+step +proj=vgridshift +grids="{geoid.as_posix()}" +multiplier=1 '
        "+step +proj=unitconvert +xy_in=rad +xy_out=deg"
    )
    report = {
        "schema": "ancientearth.independent-terrain-verification.v1",
        "method": "Native source posts plus independent PROJ vertical-grid operation; no importer helpers",
        "registration_tolerance_degrees": REGISTRATION_TOLERANCE_DEG,
        "height_tolerance_metres": HEIGHT_TOLERANCE_M,
        "software": {
            "numpy": np.__version__,
            "rasterio": rasterio.__version__,
            "pyproj": pyproj.__version__,
            "proj": pyproj.proj_version_str,
        },
        "patches": [
            verify_patch(patch, filename, source_dir, args.terrain_dir, geoid_hash, transform)
            for patch, filename in PATCH_SOURCES
        ],
        "limitation": "Numerical conversion and registration agreement is not physical survey accuracy.",
    }
    encoded = json.dumps(report, indent=2, allow_nan=False) + "\n"
    if args.report is not None:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(encoded, encoding="utf-8")
    print(encoded, end="")


if __name__ == "__main__":
    main()
