"""GSI/XYZ Web Mercator tile coordinates, with geographic outer-edge bounds.

The spherical Mercator latitude formula is intentional: XYZ tiles use EPSG:3857,
even when the underlying terrain uses WGS84 ellipsoidal geometry.
Reference: https://maps.gsi.go.jp/development/siyou.html
"""

from __future__ import annotations

import math
import json


MAX_LATITUDE_DEG = math.degrees(math.atan(math.sinh(math.pi)))
WEB_MERCATOR_RADIUS_M = 6_378_137.0


def _tile_count(zoom: int) -> int:
    if isinstance(zoom, bool) or not isinstance(zoom, int) or not 0 <= zoom <= 30:
        raise ValueError("zoom must be an integer from 0 to 30")
    return 1 << zoom


def _tile_y(latitude: float, count: int) -> float:
    return count * (1.0 - math.asinh(math.tan(math.radians(latitude))) / math.pi) / 2.0


def tile_range(
    west: float, south: float, east: float, north: float, zoom: int
) -> tuple[int, int, int, int]:
    """Return inclusive xmin, ymin, xmax, ymax intersecting a geographic box.

    The box must not cross the antimeridian. Outer east/south edges are exclusive,
    so a box ending exactly at a tile boundary does not request its neighbor.
    """
    count = _tile_count(zoom)
    if not all(math.isfinite(value) for value in (west, south, east, north)):
        raise ValueError("bounds must be finite")
    if not -180.0 <= west < east <= 180.0:
        raise ValueError("longitude bounds must increase within [-180, 180]")
    if not -MAX_LATITUDE_DEG <= south < north <= MAX_LATITUDE_DEG:
        raise ValueError("latitude bounds must fit the Web Mercator domain")
    xmin = math.floor((west + 180.0) / 360.0 * count)
    xmax = math.ceil((east + 180.0) / 360.0 * count) - 1
    ymin = math.floor(_tile_y(north, count))
    ymax = math.ceil(_tile_y(south, count)) - 1
    return tuple(max(0, min(count - 1, value)) for value in (xmin, ymin, xmax, ymax))


def bounds_for_tiles(
    xmin: int, ymin: int, xmax: int, ymax: int, zoom: int
) -> dict[str, float]:
    """Return west/south/east/north degrees at the mosaic's outer pixel edges."""
    count = _tile_count(zoom)
    if not all(isinstance(value, int) and not isinstance(value, bool)
               for value in (xmin, ymin, xmax, ymax)):
        raise ValueError("tile indices must be integers")
    if not (0 <= xmin <= xmax < count and 0 <= ymin <= ymax < count):
        raise ValueError("inclusive tile indices must fit the zoom level")

    def latitude(y: int) -> float:
        return math.degrees(math.atan(math.sinh(math.pi * (1.0 - 2.0 * y / count))))

    return {
        "west": xmin / count * 360.0 - 180.0,
        "south": latitude(ymax + 1),
        "east": (xmax + 1) / count * 360.0 - 180.0,
        "north": latitude(ymin),
    }


def pixel_offset(
    x: int, y: int, xmin: int, ymin: int, tile_size: int = 256
) -> tuple[int, int]:
    """Upper-left mosaic pixel for a north-up XYZ tile, with no resampling."""
    if not all(isinstance(value, int) and not isinstance(value, bool)
               for value in (x, y, xmin, ymin, tile_size)):
        raise ValueError("tile indices and tile size must be integers")
    if xmin < 0 or ymin < 0 or x < xmin or y < ymin or tile_size <= 0:
        raise ValueError("invalid tile origin, tile index, or tile size")
    return (x - xmin) * tile_size, (y - ymin) * tile_size


def mosaic_size(
    xmin: int, ymin: int, xmax: int, ymax: int, tile_size: int = 256
) -> tuple[int, int]:
    """Width and height in pixels for inclusive XYZ tile indices."""
    if not all(isinstance(value, int) and not isinstance(value, bool)
               for value in (xmin, ymin, xmax, ymax, tile_size)):
        raise ValueError("tile indices and tile size must be integers")
    if not (0 <= xmin <= xmax and 0 <= ymin <= ymax and tile_size > 0):
        raise ValueError("invalid inclusive tile range or tile size")
    return (xmax - xmin + 1) * tile_size, (ymax - ymin + 1) * tile_size


def web_mercator_bounds(bounds: dict[str, float]) -> dict[str, float]:
    """Convert geographic outer bounds to EPSG:3857, using its spherical formula."""
    west, south, east, north = (bounds[key] for key in ("west", "south", "east", "north"))
    # Reuse validation only; the tile range itself is irrelevant to this export.
    tile_range(west, south, east, north, 0)
    radius = WEB_MERCATOR_RADIUS_M
    return {
        "xmin": radius * math.radians(west),
        "ymin": radius * math.asinh(math.tan(math.radians(south))),
        "xmax": radius * math.radians(east),
        "ymax": radius * math.asinh(math.tan(math.radians(north))),
    }


def naip_export_plan(bounds: dict[str, float], maximum_pixels: int = 4000) -> dict:
    """Plan a bounded native-USGS image export; actual returned extent is authoritative."""
    if isinstance(maximum_pixels, bool) or not isinstance(maximum_pixels, int) or not 2 <= maximum_pixels <= 4000:
        raise ValueError("maximum_pixels must be an integer between 2 and 4000")
    extent = web_mercator_bounds(bounds)
    dx, dy = extent["xmax"] - extent["xmin"], extent["ymax"] - extent["ymin"]
    pixel_size = max(dx, dy) / maximum_pixels
    width = max(1, min(maximum_pixels, round(dx / pixel_size)))
    height = max(1, min(maximum_pixels, round(dy / pixel_size)))
    return {
        "provider": "usgs_naip",
        "service_url": "https://imagery.nationalmap.gov/arcgis/rest/services/USGSNAIPImagery/ImageServer",
        "requested_bounds_degrees": bounds,
        "requested_extent_3857": extent,
        "requested_dimensions_pixels": [width, height],
        "export_parameters": {
            "f": "json",
            "bbox": ",".join(format(extent[key], ".17g") for key in ("xmin", "ymin", "xmax", "ymax")),
            "bboxSR": "3857",
            "imageSR": "3857",
            "size": f"{width},{height}",
            "format": "png",
            "pixelType": "U8",
            "interpolation": "RSP_BilinearInterpolation",
            "renderingRule": json.dumps({"rasterFunction": "NaturalColor"}, separators=(",", ":")),
            "adjustAspectRatio": "true",
            "returnSquarePixels": "true",
        },
        "registration": "Use validated JSON response extent and dimensions, not the request extent",
        "nodata": "Preserve returned PNG alpha; transparent local pixels use global imagery fallback",
    }


def validate_naip_export(
    response: dict, requested_bounds: dict[str, float], image_size: tuple[int, int]
) -> dict:
    """Validate export geometry and map actual outer pixel edges back to degrees.

    This checks raster registration, not whether NAIP covers every pixel. Missing
    source coverage is represented by image alpha, which the caller must retain.
    """
    if "error" in response:
        raise ValueError(f"NAIP export failed: {response['error']}")
    width, height = response.get("width"), response.get("height")
    if (isinstance(width, bool) or isinstance(height, bool)
            or not isinstance(width, int) or not isinstance(height, int)
            or not 1 <= width <= 4000 or not 1 <= height <= 4000
            or tuple(image_size) != (width, height)):
        raise ValueError("Returned image and metadata dimensions disagree or exceed service limits")
    extent = response.get("extent", {})
    spatial = extent.get("spatialReference", {})
    if spatial.get("latestWkid", spatial.get("wkid")) not in (3857, 102100, 102113):
        raise ValueError("Expected an explicit EPSG:3857-compatible returned extent")
    values = [extent.get(key) for key in ("xmin", "ymin", "xmax", "ymax")]
    if any(isinstance(value, bool) or not isinstance(value, (int, float))
           or not math.isfinite(value) for value in values):
        raise ValueError("Nonfinite or missing exported geographic registration")
    xmin, ymin, xmax, ymax = values
    world_edge = WEB_MERCATOR_RADIUS_M * math.pi
    if not (-world_edge <= xmin < xmax <= world_edge and -world_edge <= ymin < ymax <= world_edge):
        raise ValueError("Returned bounds are outside the supported Web Mercator domain")
    requested = web_mercator_bounds(requested_bounds)
    tolerance = 1e-6  # Floating-point comparison allowance in projected metres.
    if (xmin > requested["xmin"] + tolerance or ymin > requested["ymin"] + tolerance
            or xmax < requested["xmax"] - tolerance or ymax < requested["ymax"] - tolerance):
        raise ValueError("Returned raster extent does not cover the requested terrain bounds")
    pixel_x, pixel_y = (xmax - xmin) / width, (ymax - ymin) / height
    # The request explicitly requires square projected pixels; tolerate only
    # service metadata rounding, not an assumed aspect correction.
    if not math.isclose(pixel_x, pixel_y, rel_tol=1e-8, abs_tol=1e-6):
        raise ValueError("Returned export does not have the requested square projected pixels")
    radius = WEB_MERCATOR_RADIUS_M
    bounds = {
        "west": math.degrees(xmin / radius),
        "south": math.degrees(math.atan(math.sinh(ymin / radius))),
        "east": math.degrees(xmax / radius),
        "north": math.degrees(math.atan(math.sinh(ymax / radius))),
    }
    return {
        "projection": "WebMercator",
        "mosaic_bounds_degrees": bounds,
        "actual_extent_3857": {key: float(extent[key]) for key in ("xmin", "ymin", "xmax", "ymax")},
        "output_dimensions_pixels": [width, height],
        "projected_pixel_size_metres": [pixel_x, pixel_y],
        "bounds_semantics": "Actual service-returned outer pixel edges; north at image top",
    }
