"""Height-free categorical boundary topology from WBM source posts only.

The authoritative surface is the analytic WGS84 ellipsoid. This module
constructs only its class partition and finite presentation topology.
"""
from __future__ import annotations
import numpy as np
from shoreline_curves import FLATNESS_GRID, binary_regions, curve_plan, triangulate_polygon


def boundary_mesh(mask: np.ndarray, rows: np.ndarray, columns: np.ndarray,
                  coarse_codes: np.ndarray) -> tuple[np.ndarray, np.ndarray, dict]:
    """Return source [row,column], [a,b,c,class] faces and reproducible records.

    Added-vertex records are [0,row,col] native posts, [1,row,col] edge
    midpoints, [2,a,b,c] source-parameter stitching centers, or
    [3,row,col,curve_index,t] sampled curve points. Their global
    IDs follow the original row-major grid, in exactly this record order.
    """
    if (not isinstance(mask, np.ndarray) or mask.ndim != 2
            or not np.issubdtype(mask.dtype, np.integer)
            or not np.isin(mask, [0, 1, 2, 3]).all()):
        raise ValueError("WBM requires a two-dimensional integer categorical array")
    for indices, limit in [(rows, mask.shape[0]), (columns, mask.shape[1])]:
        if (not isinstance(indices, np.ndarray) or indices.ndim != 1 or len(indices) < 2
                or not np.issubdtype(indices.dtype, np.integer)
                or np.any(indices < 0) or np.any(indices >= limit)
                or not np.all(np.diff(indices) == 3)):
            raise ValueError("Source indices must be in-bounds integer posts spaced by three")
    height, width = len(rows), len(columns)
    if not isinstance(coarse_codes, np.ndarray) or not np.issubdtype(coarse_codes.dtype, np.integer):
        raise ValueError("Coarse categories require an integer array")
    if coarse_codes.shape != (height - 1, width - 1, 2):
        raise ValueError("Refinement source/grid mismatch")
    stencils = np.lib.stride_tricks.sliding_window_view(mask, (4, 4))[
        rows[:-1, None], columns[None, :-1]]
    minimum, maximum = stencils.min(axis=(2, 3)), stencils.max(axis=(2, 3))
    expected_codes = np.repeat(np.where(minimum == maximum, minimum, 4)[:, :, None], 2, axis=2)
    if not np.array_equal(coarse_codes, expected_codes):
        raise ValueError("Coarse categories differ from their complete native WBM stencils")
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
        if int(mask[r, c]) == 1:
            ocean_native.add(point)
        if point not in post_ids:
            post_ids[point] = add([0, r, c], (float(r), float(c)))
        return post_ids[point]

    def crossing(a: tuple[int, int], b: tuple[int, int]) -> int:
        if {int(mask[a]), int(mask[b])} != {0, 1}:
            raise ValueError("Only binary land/ocean edges have a reconstructed crossing")
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
        # Inland-water mixtures retain source-parameter classification. A shared
        # 0/1 edge still needs its already reconstructed midpoint for watertight
        # topology, but the mixed face remains explicitly unresolved.
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

    return np.asarray(parameters, dtype=np.float64), np.asarray(faces, dtype=np.uint32), {
        "refined_coarse_cells": int(refined.sum()),
        "added_vertex_records": records,
        "added_vertex_record_format": "Implicit global index original_width*original_height+list_position; [0,native_row,native_column] source post, [1,midpoint_row,midpoint_column] source-edge crossing, [2,a,b,c] source-parameter barycenter, [3,row,column,curve_index,t] cubic sample",
        "added_vertex_counts_native_crossing_center_curve": [sum(record[0] == kind for record in records) for kind in range(4)],
        "native_ocean_posts_including_original": len(ocean_native),
        "curves": curve_records,
        "curve_flatness_source_grid_units": FLATNESS_GRID,
        "contour": "Binary native quad outer-edge midpoints, fixed NW-SE halves only for ambiguous checkerboards; G1 cubic rounding contained in each convex source parent, chord-monotone controls and bounded de Casteljau subdivision. All geographic parameters are projected to WGS84 h=0 for presentation; no heights are read. Inland category mixtures remain unresolved.",
    }
