"""Independent AEWATR02 source, construction and watertight-topology audit.

This verifier does not import either water preparation module. It evaluates
recorded vertex recipes using the original raster sources and PROJ, checks all
face classes against the native binary-mask policy, and audits oriented edges
and coverage in the source parameter plane. These are model consistency checks,
not validation against surveyed shorelines or instantaneous ocean levels.
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


def cubic_value(control: np.ndarray, t: float) -> np.ndarray:
    """Bernstein evaluation, independent of production de Casteljau sampling."""
    u = 1 - t
    weights = np.array([u**3, 3 * u * u * t, 3 * u * t * t, t**3])
    return control[0] + weights @ (control - control[0])


def cubic_derivative(control: np.ndarray, t: float) -> np.ndarray:
    differences = np.diff(control, axis=0)
    return 3 * np.array([(1 - t)**2, 2 * t * (1 - t), t*t]) @ differences


def cubic_signed_area(control: np.ndarray) -> float:
    """Exact polynomial integral of (row*dcolumn-column*drow)/2."""
    a, b, c, d = control
    polynomial = [a, 3 * (b - a), 3 * (a - 2*b + c), d - a + 3*(b - c)]
    return sum(j * (polynomial[i][0] * polynomial[j][1]
                    - polynomial[i][1] * polynomial[j][0]) / (i+j)
               for i in range(4) for j in range(1, 4)) / 2


def audit_curves(parameters: np.ndarray, faces: np.ndarray, codes: np.ndarray,
                 metadata: dict, raw_mask: np.ndarray, expected_parents: set) -> dict:
    curves = metadata.get("curves", [])
    tolerance = metadata.get("curve_flatness_source_grid_units")
    require(tolerance == 0.0005, "unexpected source-space subdivision tolerance")
    parents, declared_edges, curve_ids = set(), [], set()
    joins: dict[tuple, list[np.ndarray]] = {}
    maximum_flatness = maximum_sample_error = maximum_join_error = 0.0
    area_difference, area_bound = 0.0, 0.0
    for record in curves:
        parent = np.asarray(record["parent"], dtype=float)
        key = tuple(map(tuple, parent))
        require(key in expected_parents and key not in parents, "unexpected or duplicate curve parent")
        parents.add(key)
        control = np.asarray(record["controls"], dtype=float)
        ts, ids = np.asarray(record["parameters"], dtype=float), np.asarray(record["vertices"], dtype=int)
        require(control.shape == (4, 2) and np.isfinite(control).all()
                and len(ids) == len(ts) and len(ids) >= 2 and ts[0] == 0 and ts[-1] == 1
                and np.all(np.diff(ts) > 0) and np.all(ids >= 0) and np.all(ids < len(parameters)),
                "invalid cubic metadata")
        expected_endpoints = []
        for a, b in zip(parent, np.roll(parent, -1, axis=0)):
            edge = b - a
            offsets = control - a
            signed = edge[0] * offsets[:, 1] - edge[1] * offsets[:, 0]
            require(np.all(signed >= -1e-10), "cubic control leaves convex source parent")
            if raw_mask[tuple(a.astype(int))] != raw_mask[tuple(b.astype(int))]:
                expected_endpoints.append(tuple((a+b)/2))
        require(len(expected_endpoints) == 2 and set(map(tuple, control[[0, 3]])) == set(expected_endpoints),
                "cubic endpoints differ from source-edge midpoints")
        chord = control[3] - control[0]
        length_squared = float(chord @ chord)
        require(length_squared > 0, "zero cubic endpoint chord")
        fractions = (control - control[0]) @ chord / length_squared
        require(np.all(np.diff(fractions) >= -1e-12)
                and fractions[1] <= 1/3 + 1e-10 and fractions[2] >= 2/3 - 1e-10,
                "cubic controls are not chord-monotone")
        values = np.array([cubic_value(control, t) for t in ts])
        error = float(np.max(np.linalg.norm(values - parameters[ids], axis=1)))
        maximum_sample_error = max(maximum_sample_error, error)
        require(error <= 2e-10, "recorded vertex differs from independent cubic evaluation")
        for point, direction in [(control[0], control[1] - control[0]),
                                 (control[3], control[2] - control[3])]:
            length = np.linalg.norm(direction)
            require(length > 1e-12, "undefined cubic endpoint tangent")
            joins.setdefault(tuple(point), []).append(direction / length)
        curve_ids.update(map(int, ids[1:-1]))
        declared_edges.extend(zip(ids[:-1], ids[1:]))
        for i, (start, end) in enumerate(zip(ts[:-1], ts[1:])):
            a, d = values[i], values[i+1]
            dt = end - start
            # Endpoint derivatives determine the exact restricted cubic.
            sub = np.array([a, a + dt * cubic_derivative(control, start)/3,
                            d - dt * cubic_derivative(control, end)/3, d])
            line = d - a
            length = np.linalg.norm(line)
            require(length > 1e-12, "zero subdivision chord")
            sub_fractions = (sub - a) @ line / (length*length)
            require(np.all(np.diff(sub_fractions) >= -2e-8),
                    "restricted cubic is not monotone along its own chord")
            weights = np.clip((sub[1:3] - a) @ line / (length*length), 0, 1)
            flatness = float(np.max(np.linalg.norm(sub[1:3] - (a + weights[:, None]*line), axis=1)))
            maximum_flatness = max(maximum_flatness, flatness)
            require(flatness <= tolerance + 2e-10, "cubic subpiece exceeds its control-hull bound")
            # Translate before integrating to avoid large-coordinate cancellation.
            local_control = sub - a
            curve_area = cubic_signed_area(local_control)
            chord_area = 0.0
            area_difference += abs(curve_area - chord_area)
            area_bound += tolerance * length
    require(parents == expected_parents, "missing source binary contour parent")
    for directions in joins.values():
        require(len(directions) <= 2, "branched declared shoreline tangent")
        if len(directions) == 2:
            error = float(np.linalg.norm(directions[0] + directions[1]))
            maximum_join_error = max(maximum_join_error, error)
            require(error < 2e-8, "shared cubic tangent directions disagree")
    require(area_difference <= area_bound + 1e-8, "cubic area deviation exceeds chord-strip bound")
    # Every pure land/ocean interface is exactly one declared sampled curve.
    count = len(parameters)
    edges = np.sort(faces[:, [[0, 1], [1, 2], [2, 0]]], axis=2)
    edge_keys = edges[:, :, 0] * count + edges[:, :, 1]
    actual = np.intersect1d(edge_keys[codes == 0], edge_keys[codes == 1])
    declared = np.asarray(declared_edges, dtype=np.int64).reshape(-1, 2)
    declared.sort(axis=1)
    declared_keys = declared[:, 0]*count + declared[:, 1]
    require(len(np.unique(declared_keys)) == len(declared_keys)
            and np.array_equal(actual, np.sort(declared_keys)), "mesh interface differs from declared curves")
    return {"curves_checked": len(curves), "curve_vertex_ids": curve_ids,
            "maximum_cubic_sample_error_source_units": maximum_sample_error,
            "maximum_subcurve_control_hull_distance_source_units": maximum_flatness,
            "maximum_join_unit_tangent_error": maximum_join_error,
            "sum_absolute_curve_chord_area_difference_source_units_squared": area_difference,
            "sum_control_hull_area_bounds_source_units_squared": area_bound}


def audit_topology(parameters: np.ndarray, faces: np.ndarray, bounds: list[int]) -> dict:
    """A consistently oriented disk must cover the entire rectangular domain."""
    r0, r1, c0, c1 = bounds
    p = parameters[faces]
    areas2 = ((p[:, 1, 1] - p[:, 0, 1]) * (p[:, 2, 0] - p[:, 0, 0])
              - (p[:, 1, 0] - p[:, 0, 0]) * (p[:, 2, 1] - p[:, 0, 1]))
    require(np.all(areas2 < -1e-12), "reversed or degenerate parameter face")
    area = float(-areas2.sum() / 2)
    expected_area = (r1 - r0) * (c1 - c0)
    require(abs(area - expected_area) < 1e-6, "parameter coverage area differs")
    directed = faces[:, [[0, 1], [1, 2], [2, 0]]].reshape(-1, 2)
    undirected = np.sort(directed, axis=1)
    unique, inverse, counts = np.unique(undirected, axis=0, return_inverse=True, return_counts=True)
    require(np.all((counts == 1) | (counts == 2)), "nonmanifold or duplicate mesh edge")
    winding_sum = np.bincount(inverse, weights=np.where(directed[:, 0] < directed[:, 1], 1, -1))
    require(np.all(winding_sum[counts == 2] == 0), "internal edge has equal incident winding")
    boundary = unique[counts == 1]
    q = parameters[boundary]
    on_edge = ((np.all(q[:, :, 0] == r0, axis=1)) | (np.all(q[:, :, 0] == r1, axis=1))
               | (np.all(q[:, :, 1] == c0, axis=1)) | (np.all(q[:, :, 1] == c1, axis=1)))
    require(np.all(on_edge), "unpaired interior edge or T-junction")
    used = np.unique(faces)
    require(len(used) - len(unique) + len(faces) == 1, "mesh is not a disk by Euler characteristic")
    adjacency: dict[int, list[int]] = {}
    for a, b in boundary:
        adjacency.setdefault(int(a), []).append(int(b))
        adjacency.setdefault(int(b), []).append(int(a))
    require(adjacency and all(len(neighbors) == 2 for neighbors in adjacency.values()), "branched boundary")
    pending, visited = [next(iter(adjacency))], set()
    while pending:
        vertex = pending.pop()
        if vertex not in visited:
            visited.add(vertex)
            pending.extend(adjacency[vertex])
    require(len(visited) == len(adjacency), "multiple boundary loops")
    return {"used_vertices": int(len(used)), "unique_edges": int(len(unique)),
            "boundary_edges": int(len(boundary)), "parameter_area_native_pixels_squared": area,
            "euler_characteristic": 1, "all_internal_edges_oppositely_paired": True}


def verify(identifier: str, tile: str, mask_tile: str, source_dir: Path,
           mask_dir: Path, asset_dir: Path, vertical: pyproj.Transformer) -> dict:
    terrain_path = asset_dir / f"{identifier}.aet"
    terrain, water = terrain_path.read_bytes(), terrain_path.with_suffix(".aew").read_bytes()
    provenance = json.loads(terrain_path.with_suffix(".provenance.json").read_text())
    meta = json.loads(terrain_path.with_suffix(".water.provenance.json").read_text())
    magic, width, height, west, north, dx, dy = struct.unpack("<8sII4d", terrain[:48])
    require(magic == b"AETERR01" and len(terrain) == 48 + width * height * 8, "terrain format")
    require(digest(terrain_path) == provenance["output_sha256"] == meta["terrain_sha256"], "retained terrain changed")
    require(water[:8] == b"AEWATR02" and water[8:48] == terrain[8:48], "refined companion header")
    extra_count, face_count = struct.unpack_from("<II", water, 56)
    offset = 64 + 24 * extra_count
    require(len(water) == offset + 13 * face_count, "refined companion size")
    require(hashlib.sha256(water).hexdigest() == meta["output_sha256"], "refined output hash")
    added = np.frombuffer(water, dtype="<f8", count=3 * extra_count, offset=64).reshape(-1, 3)
    packed = np.frombuffer(water, dtype=np.dtype([("ids", "<u4", (3,)), ("code", "u1")]), offset=offset)
    faces, codes = packed["ids"].astype(np.int64), packed["code"]
    base_count = width * height
    require(np.isfinite(added).all() and np.all(faces < base_count + extra_count)
            and np.all(codes <= 4), "invalid vertex/index/category")
    records = meta["shoreline_refinement"]["added_vertex_records"]
    require(len(records) == extra_count, "vertex record count")
    source_path, mask_path = source_dir / f"{tile}.tif", mask_dir / f"{mask_tile}-WBM.tif"
    require(digest(source_path) == provenance["dem"]["source_sha256"], "DSM source hash")
    require(digest(mask_path) == meta["source"]["sha256"], "mask source hash")
    require(digest(source_dir / "us_nga_egm08_25.tif") == provenance["geoid"]["source_sha256"], "geoid source hash")
    with rasterio.open(source_path) as source, rasterio.open(mask_path) as mask:
        require(source.transform == mask.transform and source.shape == mask.shape
                and source.crs.to_epsg() == mask.crs.to_epsg() == 4326, "source registration")
        require(source.tags().get("AREA_OR_POINT") == mask.tags().get("AREA_OR_POINT") == "Point", "point registration")
        affine = source.transform
        raw_height, raw_mask = source.read(1, masked=True), mask.read(1, masked=True)
        require(not np.ma.getmaskarray(raw_mask).any() and np.isin(raw_mask, [0, 1, 2, 3]).all(), "invalid mask")
        raw_mask = np.asarray(raw_mask)
        lon, lat = np.meshgrid(west + np.arange(width) * dx, north - np.arange(height) * dy)
        column, row = (~affine) * (lon, lat)
        require(np.max(np.abs(column - 0.5 - np.rint(column - 0.5))) < 1e-7
                and np.max(np.abs(row - 0.5 - np.rint(row - 0.5))) < 1e-7, "retained source-post alignment")
        original_r, original_c = np.rint(row - 0.5).astype(int), np.rint(column - 0.5).astype(int)
        parameters = np.zeros((base_count + extra_count, 2))
        parameters[:base_count] = np.stack((original_r.ravel(), original_c.ravel()), axis=1)
        native_ids, contour_ids, center_ids, curved_ids = list(range(base_count)), [], [], []
        used_native = set(map(tuple, parameters[:base_count].astype(int)))
        for index, record in enumerate(records, base_count):
            require(record and record[0] in (0, 1, 2, 3), "unknown vertex recipe")
            if record[0] in (0, 1, 3):
                require(len(record) == (5 if record[0] == 3 else 3), "invalid geographic vertex recipe")
                r, c = record[1:3]
                require(np.isfinite([r, c]).all() and 0 <= r < source.height and 0 <= c < source.width,
                        "geographic recipe outside source")
                parameters[index] = [r, c]
                if record[0] == 0:
                    require(r == int(r) and c == int(c) and (r, c) not in used_native, "duplicate/non-native post")
                    used_native.add((r, c))
                    native_ids.append(index)
                elif record[0] == 1:
                    require(r * 2 == int(r * 2) and c * 2 == int(c * 2)
                            and (r != int(r) or c != int(c)), "crossing is not a native edge midpoint")
                    a, b = (int(np.floor(r)), int(np.floor(c))), (int(np.ceil(r)), int(np.ceil(c)))
                    require({int(raw_mask[a]), int(raw_mask[b])} == {0, 1}, "crossing lacks land/ocean endpoints")
                    contour_ids.append(index)
                else:
                    require(isinstance(record[3], int)
                            and 0 <= record[3] < len(meta["shoreline_refinement"]["curves"])
                            and 0 < record[4] < 1, "invalid cubic vertex recipe")
                    curve = meta["shoreline_refinement"]["curves"][record[3]]
                    require(index in curve["vertices"]
                            and curve["parameters"][curve["vertices"].index(index)] == record[4],
                            "cubic recipe disagrees with curve metadata")
                    curved_ids.append(index)
            else:
                require(len(record) == 4 and all(isinstance(i, int) and 0 <= i < index for i in record[1:]),
                        "invalid Cartesian-centre dependencies")
                parameters[index] = parameters[record[1:]].mean(axis=0)
                center_ids.append(index)
        native_ids = np.array(native_ids, dtype=int)
        contour_ids = np.array(contour_ids, dtype=int)
        native_rc = parameters[native_ids].astype(int)
        source_heights = raw_height[native_rc[:, 0], native_rc[:, 1]]
        require(not np.ma.getmaskarray(source_heights).any() and np.isfinite(source_heights).all(), "missing native DSM height")
        source_heights = np.asarray(source_heights, dtype=float)
        ocean_native = raw_mask[native_rc[:, 0], native_rc[:, 1]] == 1
        require(np.all(source_heights[ocean_native] == 0), "native ocean post is not H=0")
        curved_ids = np.array(curved_ids, dtype=int)
        geographic_ids = np.concatenate((native_ids, contour_ids, curved_ids))
        orthometric = np.concatenate((source_heights, np.zeros(len(contour_ids) + len(curved_ids))))
        geo_r, geo_c = parameters[geographic_ids].T
        geo_lon, geo_lat = affine * (geo_c + 0.5, geo_r + 0.5)
        _, _, h = vertical.transform(geo_lon, geo_lat, orthometric, errcheck=True)
        to_ecef = pyproj.Transformer.from_crs(4979, 4978, always_xy=True)
        expected_xyz = np.zeros((base_count + extra_count, 3))
        expected_xyz[geographic_ids] = np.stack(to_ecef.transform(geo_lon, geo_lat, h, errcheck=True), axis=-1)
        retained_height = np.frombuffer(terrain, dtype="<f8", offset=48)
        retained_error = float(np.max(np.abs(retained_height - h[:base_count])))
        require(retained_error <= 1e-8, "retained H+PROJ N differs")
        for index in center_ids:
            expected_xyz[index] = expected_xyz[records[index - base_count][1:]].mean(axis=0)
        construction_error = float(np.max(np.linalg.norm(added - expected_xyz[base_count:], axis=1), initial=0))
        require(construction_error <= 2e-8, "appended source/contour/centre differs from independent construction")
        actual_xyz = np.vstack((expected_xyz[:base_count], added))
        points = actual_xyz[faces]
        normals = np.cross(points[:, 1] - points[:, 0], points[:, 2] - points[:, 0])
        require(np.all(np.einsum("ij,ij->i", normals, points.mean(axis=1)) > 0), "inverted ECEF presentation winding")
        bounds = [int(original_r.min()), int(original_r.max()), int(original_c.min()), int(original_c.max())]
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
        curve_audit = audit_curves(parameters, faces, codes, meta["shoreline_refinement"],
                                  raw_mask, expected_parents)
        require(curve_audit.pop("curve_vertex_ids") == set(map(int, curved_ids)), "unaccounted cubic sample")
        native_classes = np.full(len(parameters), -1, dtype=int)
        native_classes[native_ids] = raw_mask[native_rc[:, 0], native_rc[:, 1]]
        corner_classes = native_classes[faces]
        known = (corner_classes >= 0) & (codes[:, None] != 4)
        require(np.all((corner_classes == codes[:, None])[known]), "source-post class changed across reconstructed interface")
    return {"patch": identifier, "retained_posts": base_count,
            "added_native_posts": len(native_ids) - base_count,
            "modeled_ocean_crossing_vertices": len(contour_ids), "cartesian_stitch_centres": len(center_ids),
            "modeled_cubic_sample_vertices": len(curved_ids), "curve_audit": curve_audit,
            "native_ocean_posts_checked_H0": int(ocean_native.sum()), "faces_verified": int(face_count),
            "triangle_counts_land_ocean_lake_river_mixed": np.bincount(codes, minlength=5).tolist(),
            "maximum_retained_H_plus_PROJ_N_difference_m": retained_error,
            "maximum_added_vertex_PROJ_construction_difference_m": construction_error,
            "all_ECEF_faces_outward": True,
            "all_face_vertices_inside_declared_source_region": True, "topology": topology,
            "unchanged_original_terrain_sha256": digest(terrain_path)}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-dir", type=Path, default=ROOT / ".cache/terrain")
    parser.add_argument("--mask-dir", type=Path, default=ROOT / ".cache/water")
    parser.add_argument("--asset-dir", type=Path, default=ROOT / "assets/terrain")
    parser.add_argument("--region", choices=[item[0] for item in SOURCES])
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    geoid_path = (args.source_dir / "us_nga_egm08_25.tif").resolve()
    vertical = pyproj.Transformer.from_pipeline(
        "+proj=pipeline +step +proj=unitconvert +xy_in=deg +xy_out=rad "
        f'+step +proj=vgridshift +grids="{geoid_path.as_posix()}" +multiplier=1 '
        "+step +proj=unitconvert +xy_in=rad +xy_out=deg")
    results = [verify(name, tile, mask, args.source_dir, args.mask_dir, args.asset_dir, vertical)
               for name, tile, mask in SOURCES if args.region is None or args.region == name]
    report = json.dumps({"scope": "declared mesh/source agreement, not surveyed shoreline accuracy",
                         "patches": results}, indent=2, allow_nan=False) + "\n"
    if args.report:
        args.report.write_text(report, encoding="utf-8")
    print(report, end="")


if __name__ == "__main__":
    main()
