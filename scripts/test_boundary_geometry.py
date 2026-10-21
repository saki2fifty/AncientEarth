"""Independent height-free categorical reconstruction regressions."""
from __future__ import annotations
import unittest
import numpy as np
from boundary_geometry import boundary_mesh
from verify_shoreline import audit_topology, audit_curves


def construct(mask):
    indices = np.arange(0, mask.shape[0], 3)
    codes = np.empty((len(indices)-1, len(indices)-1, 2), dtype=np.uint8)
    for iy, row in enumerate(indices[:-1]):
        for ix, column in enumerate(indices[:-1]):
            values = mask[row:row+4, column:column+4]
            codes[iy, ix] = values.min() if values.min() == values.max() else 4
    original = mask.copy()
    parameters, faces, metadata = boundary_mesh(mask, indices, indices, codes)
    np.testing.assert_array_equal(mask, original)
    audit_topology(parameters, faces[:, :3], [0, 6, 0, 6])
    return parameters, faces, metadata


def area(parameters, faces, code):
    p = parameters[faces[faces[:, 3] == code, :3]]
    return float(np.abs((p[:, 1, 0]-p[:, 0, 0])*(p[:, 2, 1]-p[:, 0, 1])
                        -(p[:, 1, 1]-p[:, 0, 1])*(p[:, 2, 0]-p[:, 0, 0])).sum()/2)


class BoundaryGeometryReview(unittest.TestCase):
    def test_uniform_and_analytic_curved_island_areas_without_height_input(self):
        for code in range(4):
            p, f, m = construct(np.full((7, 7), code, dtype=np.uint8))
            self.assertEqual(len(p), 9)
            self.assertEqual(len(f), 8)
            self.assertEqual(area(p, f, code), 36)
            self.assertEqual(m["curves"], [])
        for outside, inside in [(0, 1), (1, 0)]:
            mask = np.full((7, 7), outside, dtype=np.uint8)
            mask[2, 2] = inside
            p, f, m = construct(mask)
            h = np.sqrt(0.5)/3
            exact = 0.5 + 1.2*h - 0.6*h*h
            perimeter_bound = 4*(2*h + np.sqrt(2)*(0.5-h))
            difference = exact-area(p, f, inside)
            self.assertGreaterEqual(difference, -1e-12)
            self.assertLessEqual(difference, 0.0005*perimeter_bound)
            expected = {((r,c),(r+1,c),(r+1,c+1),(r,c+1))
                        for r in [1,2] for c in [1,2]}
            audit_curves(p, f[:, :3], f[:, 3], m, mask, expected)

    def test_shared_transition_topology_and_geographic_stitch_centres(self):
        mask = np.zeros((7, 7), dtype=np.uint8)
        mask[2, 2], mask[1, 2] = 1, 2
        p, f, m = construct(mask)
        self.assertIn(4, f[:, 3])
        self.assertGreater(area(p, f, 1), 0)
        centers = 0
        for index, record in enumerate(m["added_vertex_records"], 9):
            if record[0] == 2:
                centers += 1
                np.testing.assert_allclose(p[index], p[record[1:]].mean(axis=0), rtol=0, atol=1e-15)
        self.assertGreater(centers, 0)
        for invalid in [f[:-1, :3], np.vstack([f[:, :3], f[:1, :3]])]:
            with self.assertRaises(ValueError):
                audit_topology(p, invalid, [0, 6, 0, 6])

    def test_channel_checkerboard_and_axis_symmetry(self):
        mask = np.zeros((7, 7), dtype=np.uint8)
        mask[:, 2] = 1
        p, f, _ = construct(mask)
        self.assertAlmostEqual(area(p, f, 1), 6, places=12)
        checker = (np.indices((7, 7)).sum(axis=0) % 2).astype(np.uint8)
        p, f, _ = construct(checker)
        self.assertAlmostEqual(area(p, f, 1), 18, places=12)
        for row in range(7):
            mask[row] = np.arange(7) >= max(1, min(5, row))
        areas = []
        for reflected in [mask, mask[:, ::-1]]:
            for turn in range(4):
                p, f, _ = construct(np.rot90(reflected, turn))
                areas.append(area(p, f, 1))
        self.assertLess(max(areas)-min(areas), 1e-10)

    def test_invalid_source_registration_or_categories_fail_closed(self):
        mask = np.zeros((7, 7), dtype=np.uint8)
        rows = np.array([0, 3, 6])
        codes = np.zeros((2, 2, 2), dtype=np.uint8)
        for bad in [np.array([]), np.array([0]), np.array([-3, 0, 3]),
                    np.array([0, 3, 9]), np.array([0, 2, 6]),
                    np.array([0.0, 3.0, float("nan")]), np.array([[0, 3, 6]])]:
            with self.assertRaises(ValueError):
                boundary_mesh(mask, bad, rows, codes)
        for bad in [np.zeros((7, 7, 1), dtype=np.uint8), np.full((7, 7), 5),
                    np.full((7, 7), np.nan), np.zeros((7, 7), dtype=bool)]:
            with self.assertRaises(ValueError):
                boundary_mesh(bad, rows, rows, codes)
        with self.assertRaises(ValueError):
            boundary_mesh(mask, rows, rows, np.ones_like(codes))
        mask[1, 1] = 1  # An unretained source post must still affect refinement.
        with self.assertRaises(ValueError):
            boundary_mesh(mask, rows, rows, codes)


if __name__ == "__main__":
    unittest.main()
