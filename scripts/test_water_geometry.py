"""Independent adversarial checks of categorical classification and enclosure.

Run with the optional pinned terrain dependencies. PROJ supplies an independent
Cartesian/geographic transformation for the deliberate bowed-edge fixture.
"""
from __future__ import annotations

import unittest

import numpy as np
from pyproj import Transformer

from prepare_water import assert_mask_enclosure, conservative_codes


class WaterGeometryReview(unittest.TestCase):
    def test_categories_are_discrete_and_one_wrong_native_post_excludes_both_halves(self):
        for category in range(4):
            source = np.full((7, 7), category, dtype=np.uint8)
            baseline = source.copy()
            codes = conservative_codes(source, np.array([0, 3, 6]), np.array([0, 3, 6]))
            np.testing.assert_array_equal(codes, np.full((2, 2, 2), category))
            np.testing.assert_array_equal(source, baseline)
        # This post is not a retained corner. Looking only at mesh vertices
        # would miss its land classification and incorrectly flood the quad.
        source = np.ones((7, 7), dtype=np.uint8)
        source[1, 2] = 0
        result = conservative_codes(source, np.array([0, 3, 6]), np.array([0, 3, 6]))
        np.testing.assert_array_equal(result[0, 0], [4, 4])
        np.testing.assert_array_equal(result[1, 1], [1, 1])
        source[4:, 4:] = 2
        result = conservative_codes(source, np.array([0, 3, 6]), np.array([0, 3, 6]))
        np.testing.assert_array_equal(result[1, 1], [4, 4])

    def test_enclosure_accepts_native_scale_but_rejects_a_real_cartesian_latitude_bow(self):
        native = 1 / 3600
        for north in [1.0, 29.3, 35.7, 80.0]:
            lon, lat = np.meshgrid([120.0, 120.0 + 3 * native], [north, north - 3 * native])
            heights = np.array([[-100.0, 2000.0], [9000.0, 10.0]])
            report = assert_mask_enclosure(heights, lon, lat, native)
            self.assertGreater(report["minimum_south_F_metres"], 15)
            self.assertLess(report["maximum_north_affine_F_metres"], -15)
        lon, lat = np.meshgrid([-1.0, 1.0], [35.0, 34.999])
        xyz = np.array(Transformer.from_crs(4979, 4978, always_xy=True).transform(
            lon[0], lat[0], [0.0, 0.0], errcheck=True))
        midpoint = xyz.mean(axis=1)
        _, actual_lat, _ = Transformer.from_crs(4978, 4979, always_xy=True).transform(
            *midpoint, errcheck=True)
        self.assertGreater(actual_lat, lat[0, 0] + native / 2)
        with self.assertRaises(ValueError):
            assert_mask_enclosure(np.zeros((2, 2)), lon, lat, native)

    def test_enclosure_fails_closed_for_invalid_domains_and_nonrectilinear_registration(self):
        native = 1 / 3600
        longitude, latitude = np.meshgrid([-95.0, -95.0 + 3 * native], [29.0, 29.0 - 3 * native])
        height = np.zeros((2, 2))
        for value in [float("nan"), float("inf"), -1.0, 0.0, 1.0]:
            with self.assertRaises(ValueError):
                assert_mask_enclosure(height, longitude, latitude, value)
        for index in range(3):
            arrays = [height.copy(), longitude.copy(), latitude.copy()]
            arrays[index][0, 0] = float("nan")
            with self.assertRaises(ValueError):
                assert_mask_enclosure(*arrays, native)
        for bad_lon, bad_lat in [
            (longitude[::-1], latitude[::-1]),
            (longitude[:, ::-1], latitude),
            (longitude + np.array([[0, 0], [0.01, 0]]), latitude),
            (longitude, -latitude),
            (longitude, latitude + 60),
        ]:
            with self.assertRaises(ValueError):
                assert_mask_enclosure(height, bad_lon, bad_lat, native)
        for bad_height in [np.zeros((1, 2)), np.zeros((0, 0)), np.zeros((3, 2)),
                           np.full((2, 2), -10001), np.full((2, 2), 100001)]:
            with self.assertRaises(ValueError):
                assert_mask_enclosure(bad_height, longitude, latitude, native)


if __name__ == "__main__":
    unittest.main()
