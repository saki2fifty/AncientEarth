"""Fetch and stitch GSI aerial imagery using a math-reviewed XYZ mosaic plan."""
from concurrent.futures import ThreadPoolExecutor
from datetime import date
import argparse
import hashlib
import json
from pathlib import Path
import time
from urllib.request import Request, urlopen

from PIL import Image
from imagery_coordinates import mosaic_size, pixel_offset


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def fetch(item):
    url, path = item
    if not path.exists():
        path.parent.mkdir(parents=True, exist_ok=True)
        for attempt in range(3):
            try:
                request = Request(url, headers={"User-Agent": "AncientEarth/0.1 data preparation"})
                with urlopen(request, timeout=45) as response:
                    content = response.read()
                temporary = path.with_suffix(".part")
                temporary.write_bytes(content)
                with Image.open(temporary) as tile:
                    if tile.size != (256, 256):
                        raise ValueError(f"Unexpected GSI tile dimensions: {url}")
                    tile.verify()
                temporary.replace(path)
                break
            except Exception:
                if attempt == 2:
                    raise
                time.sleep(1)
    return {"url": url, "sha256": sha256(path)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("plan", type=Path)
    parser.add_argument("--retrieved", required=True, type=date.fromisoformat)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    plan = json.loads(args.plan.read_text(encoding="utf-8"))
    for patch in plan["patches"]:
        if patch.get("provider", "gsi") != "gsi":
            continue
        if patch["url_template"] != "https://cyberjapandata.gsi.go.jp/xyz/seamlessphoto/{z}/{x}/{y}.jpg":
            raise ValueError("Unreviewed imagery provider")
        xmin, ymin, xmax, ymax = patch["tile_range"]
        size = patch["tile_size_pixels"]
        cache = root / ".cache/aerial" / patch["id"]
        tiles = []
        for y in range(ymin, ymax + 1):
            for x in range(xmin, xmax + 1):
                url = patch["url_template"].format(z=patch["zoom"], x=x, y=y)
                tiles.append((x, y, url, cache / f"{patch['zoom']}-{x}-{y}.jpg"))
        with ThreadPoolExecutor(max_workers=4) as pool:
            sources = list(pool.map(fetch, [(url, path) for _, _, url, path in tiles]))
        mosaic = Image.new("RGB", mosaic_size(xmin, ymin, xmax, ymax, size))
        for x, y, _, path in tiles:
            with Image.open(path) as tile:
                mosaic.paste(tile.convert("RGB"), pixel_offset(x, y, xmin, ymin, size))
        output = root / "assets/textures" / f"{patch['id']}-aerial.jpg"
        mosaic.save(output, quality=95, subsampling=0, optimize=True)
        metadata = {
            "dataset": "GSI seamless aerial photography, zoom 14",
            "source_page": "https://maps.gsi.go.jp/development/ichiran.html#seamlessphoto",
            "terms": "https://www.gsi.go.jp/ENGLISH/page_e30286.html",
            "credit": "Created by editing GSI Tiles (Geospatial Information Authority of Japan)",
            "retrieved_date": args.retrieved.isoformat(),
            "capture_date": "Mixed acquisition dates; the retrieval date is not the photography date",
            "projection": "WebMercator",
            "mosaic_bounds_degrees": patch["mosaic_bounds_degrees"],
            "tile_range_inclusive": patch["tile_range"],
            "zoom": patch["zoom"],
            "output_dimensions_pixels": list(mosaic.size),
            "output_sha256": sha256(output),
            "processing": "Native north-up XYZ tiles stitched without reprojection or resizing; JPEG quality 95",
            "limitations": "Aerial color only; not 3D buildings, current conditions, or a ground survey",
            "tiles": sources,
        }
        output.with_suffix(".provenance.json").write_text(
            json.dumps(metadata, indent=2) + "\n", encoding="utf-8"
        )
        print(f"Prepared {output.name}: {len(tiles)} source tiles", flush=True)


if __name__ == "__main__":
    main()
