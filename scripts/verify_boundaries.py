"""Independent WBM-only boundary registration, topology and class verification.

Does not import the preparation implementation and never reads DEM/geoid data.
Reuses independently reviewed planar topology and cubic-curve audit routines.
"""
from __future__ import annotations
import argparse
import hashlib
import json
from pathlib import Path
import struct
import numpy as np
import rasterio
from verify_shoreline import audit_curves, audit_topology, require

ROOT = Path(__file__).resolve().parents[1]


def verify(path: Path, source_dir: Path) -> dict:
    data = path.read_bytes()
    meta = json.loads(path.with_suffix(".provenance.json").read_text())
    require(hashlib.sha256(data).hexdigest() == meta["output_sha256"], "output hash differs")
    require(len(data) >= 56, "truncated boundary header")
    magic, width, height, west, north, dx, dy, count, face_count = struct.unpack_from("<8sII4dII", data)
    require(magic == b"AEBNDR01" and width >= 2 and height >= 2 and count >= width*height,
            "invalid boundary dimensions/magic")
    require(len(data) == 56 + 16*count + 13*face_count, "invalid boundary byte length")
    local = np.frombuffer(data, dtype="<f8", count=2*count, offset=56).reshape(-1, 2)
    packed = np.frombuffer(data, dtype=np.dtype([("ids", "<u4", (3,)), ("code", "u1")]),
                           count=face_count, offset=56+16*count)
    faces, codes = packed["ids"].astype(np.int64), packed["code"]
    require(np.isfinite(local).all() and np.all(faces < count) and np.all(codes <= 4),
            "invalid parameters/indices/classes")
    rows, columns = meta["source_row_bounds"], meta["source_column_bounds"]
    bounds = [*rows, *columns]
    require((height-1)*3 == rows[1]-rows[0] and (width-1)*3 == columns[1]-columns[0],
            "source stride differs")
    parameters = local + [rows[0], columns[0]]
    base_count = width*height
    expected_base = np.stack(np.meshgrid(np.arange(rows[0], rows[1]+1, 3),
                                         np.arange(columns[0], columns[1]+1, 3), indexing="ij"), axis=-1).reshape(-1, 2)
    require(np.array_equal(parameters[:base_count], expected_base), "retained source coordinates differ")
    records = meta["reconstruction"]["added_vertex_records"]
    require(len(records) == count-base_count, "record count differs")
    native_ids, curved_ids = list(range(base_count)), []
    for index, record in enumerate(records, base_count):
        if record[0] in [0, 1, 3]:
            require(np.array_equal(parameters[index], record[1:3]), "geographic record differs")
            if record[0] == 0:
                native_ids.append(index)
            elif record[0] == 3:
                curved_ids.append(index)
        elif record[0] == 2:
            parent = np.asarray(record[1:], dtype=int)
            require(len(parent) == 3 and np.all(parent >= 0) and np.all(parent < index), "invalid stitch parents")
            require(np.max(np.abs(parameters[index] - parameters[parent].mean(axis=0))) < 1e-10,
                    "stitch centre is not a source-parameter barycenter")
        else:
            raise ValueError("unknown added vertex record")
    native_rc = parameters[native_ids].astype(int)
    require(np.array_equal(parameters[native_ids], native_rc), "source post is nonintegral")
    source = source_dir / meta["source"]["filename"]
    with source.open("rb") as stream:
        require(hashlib.file_digest(stream, "sha256").hexdigest() == meta["source"]["sha256"],
                "native WBM hash differs")
    with rasterio.open(source) as raster:
        require(raster.crs.to_epsg() == 4326 and raster.tags().get("AREA_OR_POINT") == "Point",
                "source point registration differs")
        affine = raster.transform
        require(list(affine.to_gdal()) == meta["source_affine_gdal"], "provenance geotransform differs")
        x, y = affine * (columns[0]+0.5, rows[0]+0.5)
        require([west, north, dx, dy] == [x, y, 3*affine.a, -3*affine.e],
                "half-pixel georeference or source stride differs")
        raw_mask = raster.read(1)
        require(np.isin(raw_mask, [0, 1, 2, 3]).all(), "source has unknown classes")
    topology = audit_topology(parameters, faces, bounds)
    # Centroids identify convex source parents. Region boundaries and native
    # corner incidence independently certify the curved binary classification.
    centres = parameters[faces].mean(axis=1)
    coarse_r = ((centres[:, 0] - bounds[0]) // 3).astype(int)
    coarse_c = ((centres[:, 1] - bounds[2]) // 3).astype(int)
    coarse_origin = np.stack((bounds[0] + coarse_r * 3,
                              bounds[2] + coarse_c * 3), axis=1)
    coarse_offsets = parameters[faces] - coarse_origin[:, None, :]
    require(np.all(coarse_offsets >= -1e-9) and np.all(coarse_offsets <= 3 + 1e-9),
            "face crosses its source coarse-cell boundary")
    neighborhoods = np.lib.stride_tricks.sliding_window_view(raw_mask, (4, 4))[
        bounds[0] + coarse_r * 3, bounds[2] + coarse_c * 3]
    refined = np.any(neighborhoods == 1, axis=(1, 2)) & np.any(neighborhoods != 1, axis=(1, 2))
    low, high = neighborhoods.min(axis=(1, 2)), neighborhoods.max(axis=(1, 2))
    expected_code = np.where(low == high, low, 4)
    rr, cc = np.floor(centres[refined]).astype(int).T
    fr, fc = (centres[refined] - np.floor(centres[refined])).T
    quad_classes = np.stack((raw_mask[rr, cc], raw_mask[rr+1, cc],
                             raw_mask[rr+1, cc+1], raw_mask[rr, cc+1]), axis=1)
    transitions = np.sum(quad_classes != np.roll(quad_classes, -1, axis=1), axis=1)
    full_quad = np.all(quad_classes <= 1, axis=1) & (transitions == 2)
    lower = fr >= fc
    vertex_classes = np.stack((quad_classes[:, 0], quad_classes[:, 2],
                               np.where(lower, quad_classes[:, 1], quad_classes[:, 3])), axis=1)
    binary = np.all(vertex_classes <= 1, axis=1) & (np.ptp(vertex_classes, axis=1) != 0)
    curved = full_quad | binary
    require(np.all(codes[refined][curved] <= 1), "binary source region receives nonbinary class")
    uniform = np.all(vertex_classes == vertex_classes[:, :1], axis=1)
    refined_code = np.where(uniform, vertex_classes[:, 0], 4)
    refined_code[curved] = codes[refined][curved]
    expected_code[refined] = refined_code
    require(np.all(codes == expected_code), "face class differs from native contour/stencil policy")
    offsets = parameters[faces[refined]] - np.stack((rr, cc), axis=1)[:, None, :]
    require(np.all(offsets >= -1e-9) and np.all(offsets <= 1+1e-9), "refined face leaves native quad")
    vr, vc = offsets[:, :, 0], offsets[:, :, 1]
    lower_weights = np.stack((1 - vr, vc, vr - vc), axis=2)
    upper_weights = np.stack((1 - vc, vr, vc - vr), axis=2)
    vertex_weights = np.where(lower[:, None, None], lower_weights, upper_weights)
    require(np.all(vertex_weights[~full_quad] >= -1e-9), "refined face leaves its parent native triangle")
    expected_parents = set()
    for r, c in np.unique(np.stack((rr, cc), axis=1), axis=0):
        quad = [(int(r), int(c)), (int(r+1), int(c)), (int(r+1), int(c+1)), (int(r), int(c+1))]
        classes = [int(raw_mask[p]) for p in quad]
        changes = sum(classes[i] != classes[(i+1) % 4] for i in range(4))
        if set(classes) == {0, 1} and changes == 2:
            expected_parents.add(tuple(quad))
        else:
            for parent in [quad[:3], [quad[0], quad[2], quad[3]]]:
                if {int(raw_mask[p]) for p in parent} == {0, 1}:
                    expected_parents.add(tuple(parent))
    curve_audit = audit_curves(parameters, faces, codes, meta["reconstruction"],
                              raw_mask, expected_parents)
    require(curve_audit.pop("curve_vertex_ids") == set(map(int, curved_ids)), "unaccounted cubic sample")
    native_classes = np.full(len(parameters), -1, dtype=int)
    native_classes[native_ids] = raw_mask[native_rc[:, 0], native_rc[:, 1]]
    corner_classes = native_classes[faces]
    known = (corner_classes >= 0) & (codes[:, None] != 4)
    require(np.all((corner_classes == codes[:, None])[known]), "source-post class changed across reconstructed interface")
    actual_counts = np.bincount(codes, minlength=5).tolist()
    require(actual_counts == meta["triangle_counts_land_ocean_lake_river_mixed"], "count metadata differs")
    require(meta["surface"]["ellipsoidal_height_m"] == 0.0, "surface is not h=0")
    return {"patch": path.stem, "vertices": count, "faces": face_count,
            "native_posts_checked": len(native_ids), "class_counts": actual_counts,
            "topology": topology, "curve_audit": curve_audit,
            "source_registration_verified": True, "no_height_data_read": True}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--asset-dir", type=Path, default=ROOT / "assets/boundaries")
    parser.add_argument("--source-dir", type=Path, default=ROOT / ".cache/water")
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    paths = sorted(args.asset_dir.glob("*.aeb"))
    require(len(paths) == 4, "expected four registered regional boundary files")
    report = [verify(path, args.source_dir) for path in paths]
    for item in report:
        print(f"{item['patch']}: {item['vertices']} height-free vertices, {item['faces']} faces verified")
    if args.report:
        args.report.write_text(json.dumps(report, indent=2, allow_nan=False) + "\n")


if __name__ == "__main__":
    main()
