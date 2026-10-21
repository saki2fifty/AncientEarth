"""Declared, source-cell-contained shoreline interpolation; no surveyed accuracy.

Native binary cells supply fixed edge midpoints. The shared tangents round the
polygonal contour without changing any source posts or its cell connectivity.
All coordinates and the subdivision tolerance are in source row/column units.
"""
from __future__ import annotations

import numpy as np

FLATNESS_GRID = 0.0005


def cross(a, b):
    return float(a[0] * b[1] - a[1] * b[0])


def binary_regions(corners, values):
    """Ordinary binary quad; fixed NW-SE halves only for checkerboard topology."""
    if len(corners) != 4 or set(values) != {0, 1}:
        raise ValueError("Binary native quad required")
    transitions = sum(a != b for a, b in zip(values, values[1:] + values[:1]))
    return [corners] if transitions == 2 else [corners[:3], [corners[0], corners[2], corners[3]]]


def curve_plan(regions, mask):
    """Return each parent's cubic controls and certified chord subdivisions.

    Controls lie in the convex native parent and project monotonically along
    the endpoint chord. Shared tangent directions give G1 joins where two
    binary regions meet; open ends use their own chord direction.
    """
    endpoints = []
    adjacent = {}
    for corners in regions:
        points = []
        for a, b in zip(corners, corners[1:] + corners[:1]):
            if int(mask[a]) != int(mask[b]):
                p = tuple((np.asarray(a, dtype=float) + b) * 0.5)
                points.append(p)
        if len(points) != 2:
            raise ValueError("Each declared contour parent needs two crossings")
        p, q = points
        endpoints.append((p, q))
        adjacent.setdefault(p, []).append(q)
        adjacent.setdefault(q, []).append(p)

    def tangent(point, toward):
        chord = np.asarray(toward) - point
        neighbors = adjacent[point]
        if len(neighbors) == 2:
            a, b = (np.asarray(v) - point for v in neighbors)
            direction = a / np.linalg.norm(a) - b / np.linalg.norm(b)
            if np.dot(direction, chord) < 0:
                direction = -direction
        else:
            direction = chord
        length = np.linalg.norm(direction)
        if length <= 1e-12 or np.dot(direction, chord) <= 1e-12:
            raise ValueError("Degenerate or reversed shared shoreline tangent")
        return direction / length

    def handle(parent, point, direction, chord):
        length = np.linalg.norm(chord)
        limit = min(length / 3, length * length / (3 * np.dot(direction, chord)))
        for a, b in zip(parent, parent[1:] + parent[:1]):
            edge = np.asarray(b) - a
            clearance = cross(edge, point - a)
            rate = cross(edge, direction)
            if clearance < -1e-10:
                raise ValueError("Shoreline endpoint leaves its native parent")
            if rate < -1e-12:
                limit = min(limit, max(0.0, clearance) / -rate * 0.95)
        if limit <= 1e-10:
            raise ValueError("Shared tangent cannot enter its native parent")
        return point + direction * limit

    def subdivide(control, t0, t1, depth=0):
        a, b, c, d = control
        chord = d - a
        length = np.linalg.norm(chord)
        if length <= 1e-12:
            raise ValueError("Degenerate shoreline chord")
        def distance(point):
            weight = np.clip(np.dot(point - a, chord) / (length * length), 0.0, 1.0)
            return np.linalg.norm(point - (a + chord * weight))
        deviation = max(distance(b), distance(c))
        if deviation <= FLATNESS_GRID:
            return [(t1, d)]
        if depth >= 20:
            raise ValueError("Shoreline subdivision exceeded its bounded depth")
        ab, bc, cd = (a + b) / 2, (b + c) / 2, (c + d) / 2
        abc, bcd = (ab + bc) / 2, (bc + cd) / 2
        midpoint = (abc + bcd) / 2
        tm = (t0 + t1) / 2
        return (subdivide(np.asarray([a, ab, abc, midpoint]), t0, tm, depth + 1)
                + subdivide(np.asarray([midpoint, bcd, cd, d]), tm, t1, depth + 1))

    result = {}
    for corners, (p, q) in zip(regions, endpoints):
        start, end = np.asarray(p), np.asarray(q)
        control = np.asarray([start, handle(corners, start, tangent(p, q), end - start),
                              handle(corners, end, tangent(q, p), start - end), end])
        samples = [(0.0, start)] + subdivide(control, 0.0, 1.0)
        result[tuple(corners)] = {"controls": control, "samples": samples}
    return result


def triangulate_polygon(ids, parameters):
    """Ear clipping for a simple CCW row/column polygon, retaining every vertex."""
    remaining = list(ids)
    result = []
    while len(remaining) > 3:
        found = False
        for i, current in enumerate(remaining):
            before, after = remaining[i - 1], remaining[(i + 1) % len(remaining)]
            a, b, c = (np.asarray(parameters[v]) for v in (before, current, after))
            if cross(b - a, c - a) <= 1e-12:
                continue
            blocked = False
            for v in remaining:
                if v in (before, current, after):
                    continue
                p = np.asarray(parameters[v])
                if min(cross(b - a, p - a), cross(c - b, p - b), cross(a - c, p - c)) >= -1e-12:
                    blocked = True
                    break
            if not blocked:
                result.append((before, current, after))
                remaining.pop(i)
                found = True
                break
        if not found:
            raise ValueError("Cannot triangulate the constrained shoreline polygon")
    result.append(tuple(remaining))
    return result
