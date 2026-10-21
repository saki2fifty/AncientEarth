"""Independent analytic mask patterns and topology regressions for refinement."""
from __future__ import annotations

import unittest
import copy

import numpy as np
from affine import Affine
from rasterio.io import MemoryFile

from verify_shoreline import audit_topology, audit_curves, cubic_signed_area
from water_refinement import refine


class ShorelineGeometryReview(unittest.TestCase):
    def construct(self, mask, invalid_ocean_height=False):
        row, col = np.indices(mask.shape)
        heights = 12.0 + row / 3 + col / 7
        heights = np.where(mask == 1, 0.0, heights)
        heights = np.where(mask == 2, 5.0, heights)
        if invalid_ocean_height:
            heights[mask == 1] = 2.0
        original_heights = heights[::3, ::3] - 27.0
        indices = np.arange(0, mask.shape[0], 3)
        codes = np.empty((len(indices) - 1, len(indices) - 1, 2), dtype=np.uint8)
        for iy, r in enumerate(indices[:-1]):
            for ix, c in enumerate(indices[:-1]):
                values = mask[r:r + 4, c:c + 4]
                codes[iy, ix] = values.min() if values.min() == values.max() else 4
        step = 1 / 3600
        affine = Affine(step, 0, -95 - step / 2, 0, -step, 29 + step / 2)
        with MemoryFile() as memory:
            with memory.open(driver="GTiff", width=16, height=16, count=1,
                             dtype="float64", transform=affine, crs="EPSG:4979") as writer:
                writer.write(np.full((1, 16, 16), -27.0))
            with memory.open() as geoid:
                added, faces, meta = refine(mask, heights, indices, indices, affine,
                                            original_heights, geoid, codes)
        parameters = [tuple(pair) for pair in np.stack(np.meshgrid(indices, indices, indexing="ij"), axis=-1).reshape(-1, 2)]
        for record in meta["added_vertex_records"]:
            if record[0] in (0, 1, 3):
                parameters.append(tuple(record[1:3]))
            else:
                parameters.append(tuple(np.asarray(parameters)[record[1:]].mean(axis=0)))
        parameters = np.asarray(parameters, dtype=float)
        audit_topology(parameters, faces[:, :3], [0, 6, 0, 6])
        return added, faces, parameters, meta

    def area(self, faces, parameters, code):
        points = parameters[faces[faces[:, 3] == code, :3]]
        return float(np.abs((points[:, 1, 1] - points[:, 0, 1]) * (points[:, 2, 0] - points[:, 0, 0])
                            - (points[:, 1, 0] - points[:, 0, 0]) * (points[:, 2, 1] - points[:, 0, 1])).sum() / 2)

    def test_analytic_single_post_islands_channels_and_fixed_diagonal_cases(self):
        for background in [0, 1]:
            mask = np.full((7, 7), background, dtype=np.uint8)
            added, faces, parameters, _ = self.construct(mask)
            self.assertEqual(len(added), 0)
            self.assertEqual(len(faces), 8)
            self.assertEqual(self.area(faces, parameters, background), 36)
        for background, isolated in [(0, 1), (1, 0)]:
            mask = np.full((7, 7), background, dtype=np.uint8)
            mask[2, 2] = isolated
            _, faces, parameters, _ = self.construct(mask)
            # Four symmetric cubic arcs join half-edge crossings. Their handle
            # length h=sqrt(.5)/3 gives the independently integrated loop area
            # 1/2 + 6h/5 - 3h^2/5. Polygon chords lie inside this convex loop.
            h = np.sqrt(0.5) / 3
            exact_area = 0.5 + 1.2*h - 0.6*h*h
            difference = exact_area - self.area(faces, parameters, isolated)
            self.assertGreaterEqual(difference, -1e-12)
            perimeter_upper = 4 * (2*h + np.sqrt(2)*(0.5-h))
            self.assertLessEqual(difference, 0.0005*perimeter_upper)
        mask = np.zeros((7, 7), dtype=np.uint8)
        mask[:, 2] = 1
        _, faces, parameters, _ = self.construct(mask)
        self.assertAlmostEqual(self.area(faces, parameters, 1), 6, places=12)
        mask = (np.indices((7, 7)).sum(axis=0) % 2).astype(np.uint8)
        _, faces, parameters, _ = self.construct(mask)
        self.assertAlmostEqual(self.area(faces, parameters, 1), 18, places=12)
        mask = np.zeros((7, 7), dtype=np.uint8)
        mask[0, 0] = 1
        _, faces, parameters, _ = self.construct(mask)
        self.assertAlmostEqual(self.area(faces, parameters, 1), 0.125, places=12)

    def test_refined_coarse_transition_and_inland_mixture_have_shared_edges(self):
        mask = np.zeros((7, 7), dtype=np.uint8)
        mask[2, 2] = 1
        added, faces, parameters, meta = self.construct(mask)
        self.assertGreater(meta["added_vertex_counts_native_crossing_center_curve"][2], 0)
        self.assertTrue(np.isfinite(added).all())
        self.assertEqual(len(set(map(tuple, parameters[9:][np.array([r[0] == 1 for r in meta["added_vertex_records"]])]))),
                         meta["added_vertex_counts_native_crossing_center_curve"][1])
        mask[1, 2] = 2
        _, faces, parameters, _ = self.construct(mask)
        self.assertTrue(np.any(faces[:, 3] == 4))
        self.assertGreater(self.area(faces, parameters, 1), 0)
        with self.assertRaises(ValueError):
            self.construct(mask, invalid_ocean_height=True)

    def test_auditor_rejects_missing_duplicate_or_reversed_face(self):
        mask = np.zeros((7, 7), dtype=np.uint8)
        mask[2, 2] = 1
        _, faces, parameters, _ = self.construct(mask)
        for invalid in [faces[:-1, :3], np.vstack([faces[:, :3], faces[:1, :3]])]:
            with self.assertRaises(ValueError):
                audit_topology(parameters, invalid, [0, 6, 0, 6])
        reversed_face = faces[:, :3].copy()
        reversed_face[0] = reversed_face[0, ::-1]
        with self.assertRaises(ValueError):
            audit_topology(parameters, reversed_face, [0, 6, 0, 6])

    def test_curves_have_verified_bounds_tangents_areas_and_rotation_symmetry(self):
        mask = np.zeros((7, 7), dtype=np.uint8)
        mask[2, 2] = 1
        _, faces, parameters, meta = self.construct(mask)
        expected = {((r, c), (r+1, c), (r+1, c+1), (r, c+1))
                    for r in [1, 2] for c in [1, 2]}
        report = audit_curves(parameters, faces[:, :3], faces[:, 3], meta, mask, expected)
        self.assertEqual(report["curves_checked"], 4)
        self.assertLessEqual(report["maximum_subcurve_control_hull_distance_source_units"], 0.0005+1e-12)
        # Check the independent polynomial integral itself against a straight
        # chord and a known parabolic arc y=x^2 from x=0 to1: signed area1/6.
        parabola = np.array([[0., 0.], [1/3, 0.], [2/3, 1/3], [1., 1.]])
        self.assertAlmostEqual(cubic_signed_area(parabola), 1/6, places=14)
        for curve_index, component, delta in [(0, 1, 2.0), (0, 2, -2.0)]:
            bad = copy.deepcopy(meta)
            bad["curves"][curve_index]["controls"][component][0] += delta
            with self.assertRaises(ValueError):
                audit_curves(parameters, faces[:, :3], faces[:, 3], bad, mask, expected)
        stepped = np.zeros((7, 7), dtype=np.uint8)
        for row in range(7):
            stepped[row, max(1, min(5, row)): ] = 1
        areas = []
        for reflected in [stepped, stepped[:, ::-1]]:
            for turn in range(4):
                _, rotated_faces, rotated_parameters, _ = self.construct(np.rot90(reflected, turn))
                areas.append(self.area(rotated_faces, rotated_parameters, 1))
        self.assertLess(max(areas)-min(areas), 1e-10)


if __name__ == "__main__":
    unittest.main()
