"""Download public-domain USGS/USDA NAIP exports from a math-reviewed plan.

Geometry and registration are implemented in imagery_coordinates.py. No API
key, commercial basemap or runtime imagery service is used by the app.
"""
import argparse
from datetime import date
import hashlib
import json
from pathlib import Path
import time
from urllib.parse import urlencode, urlparse
from urllib.request import Request, urlopen

from PIL import Image
from imagery_coordinates import validate_naip_export

ROOT = Path(__file__).resolve().parents[1]
SERVICE = "https://imagery.nationalmap.gov/arcgis/rest/services/USGSNAIPImagery/ImageServer"
SOURCE = "https://www.usgs.gov/centers/eros/science/usgs-eros-archive-aerial-photography-national-agriculture-imagery-program-naip"


def download(url):
    if urlparse(url).scheme != "https" or urlparse(url).hostname != "imagery.nationalmap.gov":
        raise ValueError("Only the reviewed official USGS imagery host is allowed")
    for attempt in range(3):
        try:
            request = Request(url, headers={"User-Agent": "AncientEarth/0.1 open-data preparation"})
            with urlopen(request, timeout=120) as response:
                if urlparse(response.url).hostname != "imagery.nationalmap.gov":
                    raise ValueError("Unexpected imagery download host")
                return response.read()
        except Exception:
            if attempt == 2:
                raise
            time.sleep(2)


def request_json(operation, parameters):
    url = SERVICE + operation + "?" + urlencode(parameters)
    value = json.loads(download(url))
    if "error" in value:
        raise ValueError(f"USGS service error: {value['error']}")
    return value, url


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, allow_nan=False) + "\n", encoding="utf-8")


def prepare(patch, retrieved):
    if patch["service_url"] != SERVICE or patch["provider"] != "usgs_naip":
        raise ValueError("Unreviewed imagery source")
    cache = ROOT / ".cache/naip" / patch["id"]
    cache.mkdir(parents=True, exist_ok=True)
    service, service_url = request_json("", {"f": "json"})
    if "public domain" not in service["description"].lower():
        raise ValueError("USGS no longer identifies this service's NAIP data as public domain; review required")
    query = {
        "f": "json", "where": "Category = 1 AND State = 'TX'",
        "geometry": patch["export_parameters"]["bbox"],
        "geometryType": "esriGeometryEnvelope", "inSR": "3857",
        "spatialRel": "esriSpatialRelIntersects", "returnGeometry": "false",
        "outFields": "*", "orderByFields": "OBJECTID", "resultRecordCount": "50",
    }
    features, query_urls = [], []
    while True:
        query["resultOffset"] = str(len(features))
        response, url = request_json("/query", query)
        batch = response["features"]
        features.extend(item["attributes"] for item in batch)
        query_urls.append(url)
        if not response.get("exceededTransferLimit"):
            break
        if not batch:
            raise ValueError("Incomplete source catalog pagination")
    if not features:
        raise ValueError(f"No public-domain NAIP source footprints for {patch['id']}")
    year = max(item["Year"] for item in features)
    selected = [item for item in features if item["Year"] == year]
    if len(selected) > service["maxMosaicImageCount"]:
        raise ValueError("Source count exceeds service export limit; split the requested region")
    parameters = dict(patch["export_parameters"])
    parameters["mosaicRule"] = json.dumps({
        "mosaicMethod": "esriMosaicLockRaster",
        "lockRasterIds": sorted(item["OBJECTID"] for item in selected),
        "mosaicOperation": "MT_FIRST",
    }, separators=(",", ":"))
    print(f"{patch['id']}: exporting {year} NAIP from {len(selected)} recorded source footprints", flush=True)
    export, export_url = request_json("/exportImage", parameters)
    image_bytes = download(export["href"])
    cached_image = cache / "export.png"
    cached_image.write_bytes(image_bytes)
    with Image.open(cached_image) as image:
        if image.format != "PNG":
            raise ValueError("Expected PNG to preserve missing-imagery coverage")
        image.load()
        registration = validate_naip_export(export, patch["requested_bounds_degrees"], image.size)
        has_transparency = "A" in image.getbands() or "transparency" in image.info
        if not has_transparency:
            raise ValueError("Export must explicitly preserve an alpha channel for coverage")
        alpha_extrema = image.convert("RGBA").getchannel("A").getextrema()
        if alpha_extrema[1] == 0:
            raise ValueError("USGS returned a wholly empty image")
    output = ROOT / "assets/textures" / f"{patch['id']}-aerial.png"
    output.write_bytes(image_bytes)
    metadata = {
        "dataset": "USDA National Agriculture Imagery Program (NAIP), served by USGS",
        "provider": "usgs_naip", "source_page": SOURCE,
        "license": "Public domain", "license_evidence": SERVICE,
        "credit": service["copyrightText"],
        "retrieved_date": retrieved.isoformat(), "source_year": year,
        "capture_date": "See acquisition_date in recorded source footprints; retrieval date is not capture date",
        **registration,
        "output_sha256": hashlib.sha256(image_bytes).hexdigest(),
        "processing": "USGS natural-color export in EPSG:3857 with bilinear service resampling; original returned PNG bytes retained",
        "nodata": "PNG alpha preserved; missing local imagery falls back to the bundled NASA global image",
        "alpha_extrema": list(alpha_extrema),
        "limitations": "Finite-resolution aerial color, not 3D buildings or current conditions. Output sampling is coarser than source NAIP.",
        "service_url": service_url, "query_urls": query_urls,
        "export_url": export_url, "export_response": export,
        "source_footprints": selected,
    }
    write_json(output.with_suffix(".provenance.json"), metadata)
    write_json(cache / "service.json", service)
    write_json(cache / "catalog.json", features)
    print(f"Prepared {output.name}: {registration['output_dimensions_pixels']}, alpha {alpha_extrema}", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("plan", type=Path)
    parser.add_argument("--retrieved", required=True, type=date.fromisoformat)
    parser.add_argument("--patch", choices=["houston", "galveston"], action="append")
    args = parser.parse_args()
    plan = json.loads(args.plan.read_text(encoding="utf-8"))
    for patch in plan["patches"]:
        if patch.get("provider") == "usgs_naip" and (not args.patch or patch["id"] in args.patch):
            prepare(patch, args.retrieved)


if __name__ == "__main__":
    main()
