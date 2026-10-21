"""Independent imagery registration checks.

Run ``python scripts/test_imagery_coordinates.py`` after installing the optional
``scripts/requirements-terrain.txt`` dependencies. Expected projections use
PROJ independently of the production helper's spherical-Mercator formulas.
"""

from __future__ import annotations

from copy import deepcopy
import hashlib
import json
import math
from pathlib import Path
import unittest

from PIL import Image
from pyproj import Transformer

from imagery_coordinates import naip_export_plan, validate_naip_export, web_mercator_bounds


class ImageryRegistrationReview(unittest.TestCase):
    def setUp(self) -> None:
        self.forward = Transformer.from_crs(4326, 3857, always_xy=True)
        self.inverse = Transformer.from_crs(3857, 4326, always_xy=True)
        self.request = {"west": -94.9, "south": 29.21, "east": -94.69, "north": 29.4}

    def projected(self, bounds: dict) -> tuple[float, float, float, float]:
        xmin, ymin = self.forward.transform(bounds["west"], bounds["south"], errcheck=True)
        xmax, ymax = self.forward.transform(bounds["east"], bounds["north"], errcheck=True)
        return xmin, ymin, xmax, ymax

    def expanded_response(self) -> dict:
        xmin, ymin, xmax, ymax = self.projected(self.request)
        half_span = max(xmax - xmin, ymax - ymin) / 2 + 100.0
        center_x, center_y = (xmin + xmax) / 2, (ymin + ymax) / 2
        return {
            "width": 1024,
            "height": 1024,
            "extent": {
                "xmin": center_x - half_span,
                "ymin": center_y - half_span,
                "xmax": center_x + half_span,
                "ymax": center_y + half_span,
                "spatialReference": {"wkid": 102100, "latestWkid": 3857},
            },
        }

    def test_geographic_to_mercator_agrees_with_independent_proj(self) -> None:
        for bounds in [
            self.request,
            {"west": -95.47, "south": 29.67, "east": -95.26, "north": 29.86},
            {"west": 138.62, "south": 35.27, "east": 138.84, "north": 35.45},
            {"west": -0.1, "south": -80.0, "east": 0.1, "north": 80.0},
        ]:
            result = web_mercator_bounds(bounds)
            for key, expected in zip(("xmin", "ymin", "xmax", "ymax"), self.projected(bounds)):
                self.assertLessEqual(abs(result[key] - expected), 1e-7)

    def test_export_plan_has_explicit_projection_and_bounded_dimensions(self) -> None:
        plan = naip_export_plan(self.request)
        params = plan["export_parameters"]
        self.assertEqual(params["bboxSR"], "3857")
        self.assertEqual(params["imageSR"], "3857")
        self.assertEqual(params["f"], "json")
        self.assertEqual(params["adjustAspectRatio"], "true")
        width, height = plan["requested_dimensions_pixels"]
        self.assertEqual(max(width, height), 4000)
        self.assertTrue(1 <= min(width, height) <= 4000)
        for actual, expected in zip(map(float, params["bbox"].split(",")), self.projected(self.request)):
            self.assertLessEqual(abs(actual - expected), 1e-7)
        for size in [True, 1, 4001, 2.5]:
            with self.assertRaises(ValueError):
                naip_export_plan(self.request, maximum_pixels=size)

    def test_actual_expanded_extent_drives_outer_image_bounds(self) -> None:
        response = self.expanded_response()
        registration = validate_naip_export(response, self.request, (1024, 1024))
        extent = response["extent"]
        west, south = self.inverse.transform(extent["xmin"], extent["ymin"], errcheck=True)
        east, north = self.inverse.transform(extent["xmax"], extent["ymax"], errcheck=True)
        actual = registration["mosaic_bounds_degrees"]
        for key, expected in zip(("west", "south", "east", "north"), (west, south, east, north)):
            self.assertAlmostEqual(actual[key], expected, delta=1e-12)
        self.assertLess(actual["west"], self.request["west"])
        self.assertGreater(actual["east"], self.request["east"])
        self.assertEqual(registration["output_dimensions_pixels"], [1024, 1024])
        root = Path(__file__).resolve().parents[1]
        for patch, latitude, longitude in [
            ("houston", 29.76328, -95.36327),
            ("galveston", 29.30135, -94.7977),
        ]:
            with self.subTest(patch=patch):
                metadata = json.loads(
                    (root / f"assets/textures/{patch}-aerial.provenance.json").read_text(encoding="utf-8")
                )
                terrain = json.loads(
                    (root / f"assets/terrain/{patch}.provenance.json").read_text(encoding="utf-8")
                )
                path = root / f"assets/textures/{patch}-aerial.png"
                self.assertEqual(hashlib.sha256(path.read_bytes()).hexdigest(), metadata["output_sha256"])
                with Image.open(path) as image:
                    actual = validate_naip_export(
                        metadata["export_response"], terrain["sample_bounds_degrees"], image.size,
                    )
                    self.assertEqual(actual["output_dimensions_pixels"], list(image.size))
                    extent = metadata["export_response"]["extent"]
                    west, south = self.inverse.transform(extent["xmin"], extent["ymin"], errcheck=True)
                    east, north = self.inverse.transform(extent["xmax"], extent["ymax"], errcheck=True)
                    for key, expected in zip(
                        ("west", "south", "east", "north"), (west, south, east, north),
                    ):
                        self.assertAlmostEqual(
                            metadata["mosaic_bounds_degrees"][key], expected, delta=1e-12,
                        )
                        self.assertAlmostEqual(actual["mosaic_bounds_degrees"][key], expected, delta=1e-12)
                    # Check imagery coverage at the unchanged GeoNames place
                    # coordinate using independently projected pixel indexing.
                    point_x, point_y = self.forward.transform(longitude, latitude, errcheck=True)
                    column = math.floor(
                        image.width * (point_x - extent["xmin"]) / (extent["xmax"] - extent["xmin"])
                    )
                    row = math.floor(
                        image.height * (extent["ymax"] - point_y) / (extent["ymax"] - extent["ymin"])
                    )
                    self.assertEqual(image.convert("RGBA").getpixel((column, row))[3], 255)

    def test_invalid_response_geometry_is_rejected(self) -> None:
        response = self.expanded_response()
        variants = []
        wrong_crs = deepcopy(response)
        wrong_crs["extent"]["spatialReference"] = {"wkid": 4326}
        variants.append(wrong_crs)
        wrong_dimensions = deepcopy(response)
        wrong_dimensions["width"] = 1023
        variants.append(wrong_dimensions)
        nonfinite = deepcopy(response)
        nonfinite["extent"]["xmin"] = math.nan
        variants.append(nonfinite)
        undercoverage = deepcopy(response)
        undercoverage["extent"]["xmin"] = self.projected(self.request)[0] + 1.0
        variants.append(undercoverage)
        nonsquare = deepcopy(response)
        nonsquare["extent"]["xmax"] += 1000.0
        variants.append(nonsquare)
        variants.append({"error": {"message": "No image available"}})
        for invalid in variants:
            with self.subTest(response=invalid), self.assertRaises(ValueError):
                validate_naip_export(invalid, self.request, (1024, 1024))


if __name__ == "__main__":
    unittest.main()
