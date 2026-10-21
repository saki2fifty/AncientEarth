"""Independently verify bundled water labels against licensed native WBM/DSM.

Uses no prepare_water/prepare_terrain functions. Requires the optional pinned
scripts/requirements-terrain.txt dependencies and the cached original sources.
Checks all labels and ocean posts, then samples every canonical triangle using
PROJ's independent Cartesian/geodetic transforms. Finite interior sampling is an
additional check, not a replacement for the importer's analytic enclosure proof.
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
SOURCES = [("fuji", "N35E138", "N35_00_E138_00"),
           ("tokyo", "N35E139", "N35_00_E139_00"),
           ("houston", "N29W096", "N29_00_W096_00"),
           ("galveston", "N29W095", "N29_00_W095_00")]


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def digest(path: Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def verify(identifier: str, tile: str, mask_tile: str, source_dir: Path,
           mask_dir: Path, asset_dir: Path, geoid: pyproj.Transformer) -> dict:
    terrain_path = asset_dir / f"{identifier}.aet"
    terrain = terrain_path.read_bytes()
    water_path = terrain_path.with_suffix(".aew")
    water = water_path.read_bytes()
    if water[:8] == b"AEWATR02":
        from verify_shoreline import verify as verify_refined
        return verify_refined(identifier, tile, mask_tile, source_dir, mask_dir, asset_dir, geoid)
    provenance = json.loads(terrain_path.with_suffix(".provenance.json").read_text())
    water_provenance = json.loads(terrain_path.with_suffix(".water.provenance.json").read_text())
    magic, width, height, west, north, dx, dy = struct.unpack("<8sII4d", terrain[:48])
    require(magic == b"AETERR01" and len(terrain) == 48 + width * height * 8, "terrain format")
    require(digest(terrain_path) == provenance["output_sha256"] == water_provenance["terrain_sha256"],
            "original terrain changed")
    require(water[:8] == b"AEWATR01" and water[8:48] == terrain[8:48]
            and len(water) == 56 + 2 * (width - 1) * (height - 1), "water header/length")
    require(digest(water_path) == water_provenance["output_sha256"], "water provenance hash")
    codes = np.frombuffer(water, dtype=np.uint8, offset=56).reshape(height - 1, width - 1, 2)
    require(np.all(codes <= 4), "invalid output class")
    longitude, latitude = np.meshgrid(west + np.arange(width) * dx, north - np.arange(height) * dy)
    ellipsoidal = np.frombuffer(terrain, dtype="<f8", offset=48).reshape(height, width)
    source_path, mask_path = source_dir / f"{tile}.tif", mask_dir / f"{mask_tile}-WBM.tif"
    require(digest(source_path) == provenance["dem"]["source_sha256"], "source DSM hash")
    require(digest(mask_path) == water_provenance["source"]["sha256"], "source WBM hash")
    with rasterio.open(source_path) as source, rasterio.open(mask_path) as mask:
        require(source.crs.to_epsg() == mask.crs.to_epsg() == 4326, "horizontal CRS")
        require(source.tags().get("AREA_OR_POINT") == mask.tags().get("AREA_OR_POINT") == "Point",
                "sample registration")
        require(source.shape == mask.shape and source.transform == mask.transform, "source alignment")
        require(source.count == mask.count == 1 and mask.dtypes == ("uint8",), "source bands")
        affine = mask.transform
        require(affine.b == affine.d == 0 and affine.a > 0 and affine.e < 0, "source orientation")
        cc, rr = (~affine) * (longitude, latitude)
        columns, rows = np.rint(cc - 0.5).astype(int), np.rint(rr - 0.5).astype(int)
        require(np.max(np.abs(cc - 0.5 - columns)) < 1e-7
                and np.max(np.abs(rr - 0.5 - rows)) < 1e-7, "post alignment")
        require(np.all(np.diff(rows, axis=0) == 3) and np.all(np.diff(columns, axis=1) == 3)
                and np.all(np.diff(rows, axis=1) == 0) and np.all(np.diff(columns, axis=0) == 0),
                "source stride")
        raw = mask.read(1, masked=True)
        require(not np.ma.getmaskarray(raw).any() and np.isin(raw, [0, 1, 2, 3]).all(), "mask codes")
        raw = np.asarray(raw)
        # Independently obtain every 4x4 native window via a strided array view.
        neighborhoods = np.lib.stride_tricks.sliding_window_view(raw, (4, 4))[rows[:-1, :-1], columns[:-1, :-1]]
        lo, hi = neighborhoods.min(axis=(-1, -2)), neighborhoods.max(axis=(-1, -2))
        expected = np.where(lo == hi, lo, 4)
        require(np.all(codes == expected[:, :, None]), "a label differs from its complete native stencil")
        original = source.read(1, masked=True)[rows, columns]
        require(not np.ma.getmaskarray(original).any(), "missing source elevation")
        original = np.asarray(original, dtype=float)
        ocean = expected == 1
        ocean_posts = np.zeros((height, width), dtype=bool)
        for y, x in [(0, 0), (0, 1), (1, 0), (1, 1)]:
            ocean_posts[y:y + height - 1, x:x + width - 1] |= ocean
        require(np.all(original[ocean_posts] == 0), "an ocean vertex is not source H=0")
        _, _, projected_height = geoid.transform(longitude, latitude, original, errcheck=True)
        conversion_error = float(np.max(np.abs(projected_height - ellipsoidal)))
        require(conversion_error <= 1e-8, "source H plus independent PROJ N differs")
        to_ecef = pyproj.Transformer.from_crs(4979, 4978, always_xy=True)
        to_geo = pyproj.Transformer.from_crs(4978, 4979, always_xy=True)
        xyz = np.stack(to_ecef.transform(longitude, latitude, ellipsoidal, errcheck=True), axis=-1)
        nw, ne, sw, se = xyz[:-1, :-1], xyz[:-1, 1:], xyz[1:, :-1], xyz[1:, 1:]
        sample_count = 0
        minimum_cell_margin = float("inf")
        # Ten barycentric points per triangle include vertices, edges and centre.
        for corners in [(nw, sw, se), (nw, se, ne)]:
            for i in range(4):
                for j in range(4 - i):
                    weights = np.array([i, j, 3 - i - j]) / 3.0
                    p = sum(weight * corner for weight, corner in zip(weights, corners))
                    lon, lat, _ = to_geo.transform(p[..., 0], p[..., 1], p[..., 2], errcheck=True)
                    c, r = (~affine) * (lon, lat)
                    base_c, base_r = columns[:-1, :-1], rows[:-1, :-1]
                    margins = np.stack([c - base_c, base_c + 4 - c, r - base_r, base_r + 4 - r])
                    minimum_cell_margin = min(minimum_cell_margin, float(margins.min()))
                    require(np.all(margins > 0), "sample escaped classified nearest-post cell union")
                    sampled = raw[np.floor(r).astype(int), np.floor(c).astype(int)]
                    pure = expected < 4
                    require(np.all(sampled[pure] == expected[pure]), "interior source class differs")
                    sample_count += p.shape[0] * p.shape[1]
    return {"patch": identifier, "triangles_verified": int(codes.size),
            "counts_land_ocean_lake_river_mixed": np.bincount(codes.ravel(), minlength=5).tolist(),
            "ocean_vertices_checked_H0": int(ocean_posts.sum()),
            "all_vertices_H_plus_PROJ_N_max_error_m": conversion_error,
            "independent_PROJ_triangle_samples": sample_count,
            "minimum_sample_margin_inside_native_cell_union_pixels": minimum_cell_margin,
            "unchanged_original_terrain_sha256": digest(terrain_path)}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-dir", type=Path, default=ROOT / ".cache/terrain")
    parser.add_argument("--mask-dir", type=Path, default=ROOT / ".cache/water")
    parser.add_argument("--asset-dir", type=Path, default=ROOT / "assets/terrain")
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    geoid_path = (args.source_dir / "us_nga_egm08_25.tif").resolve()
    transformer = pyproj.Transformer.from_pipeline(
        "+proj=pipeline +step +proj=unitconvert +xy_in=deg +xy_out=rad "
        f'+step +proj=vgridshift +grids="{geoid_path.as_posix()}" +multiplier=1 '
        "+step +proj=unitconvert +xy_in=rad +xy_out=deg")
    results = [verify(name, tile, mask, args.source_dir, args.mask_dir, args.asset_dir, transformer)
               for name, tile, mask in SOURCES]
    report = json.dumps({"scope": "source agreement and finite geometry checks, not physical water accuracy",
                         "patches": results}, indent=2, allow_nan=False) + "\n"
    if args.report:
        args.report.write_text(report, encoding="utf-8")
    print(report, end="")


if __name__ == "__main__":
    main()
