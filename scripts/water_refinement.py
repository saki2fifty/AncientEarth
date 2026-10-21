"""Native-mask shoreline reconstruction with globally shared mesh topology.

No downloaded data are modified. Native ocean/land cell edge midpoints are
joined by explicitly modeled, cell-contained cubic curves. Crossing and curve
vertices use H=0; native DEM posts retain their original measured heights.
"""
from __future__ import annotations

import numpy as np

from prepare_terrain import WGS84_A, WGS84_E2, geoid_undulations
from shoreline_curves import FLATNESS_GRID, binary_regions, curve_plan, triangulate_polygon


def ecef(longitude: np.ndarray, latitude: np.ndarray, height: np.ndarray) -> np.ndarray:
    phi, lam = np.radians(latitude), np.radians(longitude)
    n = WGS84_A / np.sqrt(1 - WGS84_E2 * np.sin(phi)**2)
    radius = (n + height) * np.cos(phi)
    return np.stack((radius * np.cos(lam), radius * np.sin(lam),
                     (n * (1 - WGS84_E2) + height) * np.sin(phi)), axis=-1)


def refine(mask: np.ndarray, source_heights: np.ndarray, rows: np.ndarray, columns: np.ndarray,
           affine, original_heights: np.ndarray, geoid, coarse_codes: np.ndarray) -> tuple[np.ndarray, np.ndarray, dict]:
    """Return appended XYZ positions, [a,b,c,class] faces and reproducible records.

    Added-vertex records are [0,row,col] native posts, [1,row,col] modeled
    shoreline midpoints, [2,a,b,c] Cartesian stitching centers, or
    [3,row,col,curve_index,t] sampled ocean-zero curve points. Their global
    IDs follow the original row-major grid, in exactly this record order.
    """
    height, width = original_heights.shape
    if mask.shape != source_heights.shape or coarse_codes.shape != (height - 1, width - 1, 2):
        raise ValueError("Refinement source/grid mismatch")
    if not np.all(np.diff(rows) == 3) or not np.all(np.diff(columns) == 3):
        raise ValueError("Refinement requires the reviewed three-to-one source grid")
    base_count = width * height
    post_ids = {(int(row), int(col)): iy * width + ix
                for iy, row in enumerate(rows) for ix, col in enumerate(columns)}
    parameters = [(float(row), float(col)) for row in rows for col in columns]
    records: list[list] = []
    crossing_ids: dict[tuple[int, int], int] = {}
    faces: list[list[int]] = []
    refined = np.zeros((height - 1, width - 1), dtype=bool)
    split_edges: set[tuple[tuple[int, int], tuple[int, int]]] = set()
    ocean_native: set[tuple[int, int]] = set()
    curve_records = []

    def edge_key(a, b):
        return tuple(sorted((a, b)))

    for iy in range(height - 1):
        for ix in range(width - 1):
            r, c = int(rows[iy]), int(columns[ix])
            values = mask[r:r + 4, c:c + 4]
            if np.any(values == 1) and np.any(values != 1):
                refined[iy, ix] = True
                corners = [(r, c), (r + 3, c), (r + 3, c + 3), (r, c + 3)]
                for a, b in zip(corners, corners[1:] + corners[:1]):
                    split_edges.add(edge_key(a, b))

    regions = []
    for iy, ix in np.argwhere(refined):
        r, c = int(rows[iy]), int(columns[ix])
        for dy in range(3):
            for dx in range(3):
                nw = (r + dy, c + dx)
                corners = [nw, (nw[0] + 1, nw[1]), (nw[0] + 1, nw[1] + 1), (nw[0], nw[1] + 1)]
                values = [int(mask[point]) for point in corners]
                if set(values) == {0, 1}:
                    regions.extend(binary_regions(corners, values))
                else:
                    for triangle in [corners[:3], [corners[0], corners[2], corners[3]]]:
                        if {int(mask[point]) for point in triangle} == {0, 1}:
                            regions.append(triangle)
    plans = curve_plan(regions, mask)

    def add(record: list, parameter: tuple[float, float]) -> int:
        index = base_count + len(records)
        records.append(record)
        parameters.append(parameter)
        return index

    def post(point: tuple[int, int]) -> int:
        r, c = point
        if np.ma.is_masked(source_heights[r, c]) or not np.isfinite(source_heights[r, c]) or source_heights[r, c] == -32767:
            raise ValueError("Missing native height in coastal refinement")
        if int(mask[r, c]) == 1:
            if float(source_heights[r, c]) != 0.0:
                raise ValueError("Native ocean height is nonzero; refusing an unreviewed flattening")
            ocean_native.add(point)
        if point not in post_ids:
            post_ids[point] = add([0, r, c], (float(r), float(c)))
        return post_ids[point]

    def crossing(a: tuple[int, int], b: tuple[int, int]) -> int:
        if {int(mask[a]), int(mask[b])} != {0, 1}:
            raise ValueError("Only binary land/ocean edges have an ocean-zero crossing")
        ia, ib = post(a), post(b)
        key = tuple(sorted((ia, ib)))
        if key not in crossing_ids:
            r, c = (a[0] + b[0]) / 2, (a[1] + b[1]) / 2
            crossing_ids[key] = add([1, r, c], (r, c))
        return crossing_ids[key]

    def emit(a: int, b: int, c: int, code: int) -> None:
        p, q, s = (parameters[index] for index in (a, b, c))
        # (column,row) signed area is negative for the outward NW-SW-SE order.
        twice_area = (q[1] - p[1]) * (s[0] - p[0]) - (q[0] - p[0]) * (s[1] - p[1])
        if len({a, b, c}) != 3 or twice_area >= -1e-12:
            raise ValueError("Degenerate or reversed geographic mesh triangle")
        faces.append([a, b, c, code])

    def fan_polygon(ids: list[int], code: int) -> None:
        for a, b, c in triangulate_polygon(ids, parameters):
            emit(a, b, c, code)

    def binary_region(corners: list[tuple[int, int]]) -> None:
        plan = plans[tuple(corners)]
        edge_crossings = []
        for a, b in zip(corners, corners[1:] + corners[:1]):
            if int(mask[a]) != int(mask[b]):
                edge_crossings.append(crossing(a, b))
        first, last = edge_crossings
        curve_index = len(curve_records)
        curve = [first]
        for t, point in plan["samples"][1:-1]:
            curve.append(add([3, float(point[0]), float(point[1]), curve_index, t], tuple(point)))
        curve.append(last)
        curve_records.append({"parent": [list(point) for point in corners], "controls": plan["controls"].tolist(),
                              "parameters": [float(t) for t, _ in plan["samples"]], "vertices": curve})
        for wanted in (0, 1):
            polygon = []
            previous = corners[-1]
            inside_before = int(mask[previous]) == wanted
            for current in corners:
                inside = int(mask[current]) == wanted
                if inside != inside_before:
                    polygon.append(crossing(previous, current))
                if inside:
                    polygon.append(post(current))
                previous, inside_before = current, inside
            expanded = []
            for a, b in zip(polygon, polygon[1:] + polygon[:1]):
                expanded.append(a)
                if a == first and b == last:
                    expanded.extend(curve[1:-1])
                elif a == last and b == first:
                    expanded.extend(curve[-2:0:-1])
            fan_polygon(expanded, wanted)

    def stitched_triangle(corners: list[tuple[int, int]], boundary: list[int], code: int) -> None:
        originals = [post(point) for point in corners]
        if boundary == originals:
            emit(*originals, code)
            return
        parameter = tuple(sum(parameters[index][axis] for index in originals) / 3 for axis in (0, 1))
        center = add([2, *originals], parameter)
        for a, b in zip(boundary, boundary[1:] + boundary[:1]):
            emit(center, a, b, code)

    def native_triangle(corners: list[tuple[int, int]]) -> None:
        values = [int(mask[point]) for point in corners]
        if set(values) == {0, 1}:
            binary_region(corners)
            return
        # Inland-water mixtures retain source geometry/classification. A shared
        # 0/1 edge still needs its already reconstructed midpoint for watertight
        # topology, but the mixed face is not labeled geoid-level ocean.
        code = values[0] if len(set(values)) == 1 else 4
        boundary = []
        for a, b in zip(corners, corners[1:] + corners[:1]):
            boundary.append(post(a))
            if {int(mask[a]), int(mask[b])} == {0, 1}:
                boundary.append(crossing(a, b))
        stitched_triangle(corners, boundary, code)

    def coarse_edge(a: tuple[int, int], b: tuple[int, int]) -> list[int]:
        if edge_key(a, b) not in split_edges:
            return [post(a)]
        dr, dc = (b[0] - a[0]) // 3, (b[1] - a[1]) // 3
        result = []
        for step in range(3):
            p = (a[0] + dr * step, a[1] + dc * step)
            q = (p[0] + dr, p[1] + dc)
            result.append(post(p))
            if {int(mask[p]), int(mask[q])} == {0, 1}:
                result.append(crossing(p, q))
        return result

    for iy in range(height - 1):
        for ix in range(width - 1):
            r, c = int(rows[iy]), int(columns[ix])
            if refined[iy, ix]:
                for dy in range(3):
                    for dx in range(3):
                        nw = (r + dy, c + dx)
                        ne, sw, se = (nw[0], nw[1] + 1), (nw[0] + 1, nw[1]), (nw[0] + 1, nw[1] + 1)
                        corners = [nw, sw, se, ne]
                        values = [int(mask[point]) for point in corners]
                        if set(values) == {0, 1}:
                            for region in binary_regions(corners, values):
                                binary_region(region)
                        else:
                            native_triangle([nw, sw, se])
                            native_triangle([nw, se, ne])
            else:
                nw, ne, sw, se = (r, c), (r, c + 3), (r + 3, c), (r + 3, c + 3)
                for half, corners in enumerate(([nw, sw, se], [nw, se, ne])):
                    boundary = []
                    for a, b in zip(corners, corners[1:] + corners[:1]):
                        boundary.extend(coarse_edge(a, b))
                    stitched_triangle(corners, boundary, int(coarse_codes[iy, ix, half]))

    cc, rr = np.meshgrid(columns, rows)
    original_lon, original_lat = affine * (cc + 0.5, rr + 0.5)
    original_xyz = ecef(original_lon, original_lat, original_heights).reshape(-1, 3)
    xyz = np.zeros((base_count + len(records), 3), dtype=np.float64)
    xyz[:base_count] = original_xyz
    geographic = [(index, record) for index, record in enumerate(records) if record[0] in (0, 1, 3)]
    if geographic:
        geo_rows = np.array([record[1] for _, record in geographic], dtype=float)
        geo_cols = np.array([record[2] for _, record in geographic], dtype=float)
        lon, lat = affine * (geo_cols + 0.5, geo_rows + 0.5)
        orthometric = np.array([float(source_heights[int(record[1]), int(record[2])])
                               if record[0] == 0 else 0.0 for _, record in geographic])
        height_values = orthometric + geoid_undulations(geoid, lon, lat)
        positions = ecef(lon, lat, height_values)
        for (index, _), position in zip(geographic, positions):
            xyz[base_count + index] = position
    for index, record in enumerate(records):
        if record[0] == 2:
            if max(record[1:]) >= base_count + index:
                raise ValueError("Stitching center must reference existing vertices")
            xyz[base_count + index] = xyz[record[1:]].mean(axis=0)
    if not np.isfinite(xyz).all():
        raise ValueError("Nonfinite reconstructed vertex")
    return xyz[base_count:], np.asarray(faces, dtype=np.uint32), {
        "refined_coarse_cells": int(refined.sum()),
        "added_vertex_records": records,
        "added_vertex_record_format": "Implicit global index original_width*original_height+list_position; [0,native_row,native_column] source post, [1,midpoint_row,midpoint_column] modeled H=0 edge crossing, [2,a,b,c] Cartesian barycenter, [3,row,column,curve_index,t] modeled H=0 cubic sample",
        "added_vertex_counts_native_crossing_center_curve": [sum(record[0] == kind for record in records) for kind in range(4)],
        "checked_native_ocean_posts_including_original": len(ocean_native),
        "curves": curve_records,
        "curve_flatness_source_grid_units": FLATNESS_GRID,
        "contour": "Binary native quad outer-edge midpoints, fixed NW-SE halves only for ambiguous checkerboards; G1 cubic rounding contained in each convex source parent, chord-monotone controls and bounded de Casteljau subdivision. All crossing/curve samples use modeled H=0+EGM2008 N. Inland category mixtures remain unresolved.",
    }
