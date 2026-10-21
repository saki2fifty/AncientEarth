"""Prepare NASA's global color map using the math-reviewed full-globe resampling."""
import argparse
import hashlib
import json
from pathlib import Path

from PIL import Image

SOURCE_URL = (
    "https://assets.science.nasa.gov/content/dam/science/esd/eo/images/"
    "bmng/bmng-base/july/world.200407.3x21600x10800.jpg"
)


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("--retrieved", required=True, help="Actual download date, YYYY-MM-DD")
    args = parser.parse_args()
    output = Path(__file__).resolve().parents[1] / "assets/textures/blue-marble.jpg"
    # This known NASA source exceeds Pillow's generic image-size warning threshold.
    # Inspect its dimensions before decoding; do not use this for arbitrary input.
    Image.MAX_IMAGE_PIXELS = 240_000_000
    with Image.open(args.source) as source:
        if source.size != (21600, 10800):
            raise ValueError("Expected the documented 21600 x 10800 full-globe NASA image")
        source.convert("RGB").resize((10800, 5400), Image.Resampling.LANCZOS).save(
            output, quality=90, subsampling=0, optimize=True
        )
    metadata = {
        "dataset": "NASA Blue Marble: Next Generation, July 2004 base map",
        "source_page": "https://science.nasa.gov/earth/earth-observatory/blue-marble-next-generation/base-map/",
        "source_url": SOURCE_URL,
        "credit": "NASA Earth Observatory",
        "retrieved_date": args.retrieved,
        "source_sha256": sha256(args.source),
        "source_dimensions_pixels": [21600, 10800],
        "output_dimensions_pixels": [10800, 5400],
        "output_sha256": sha256(output),
        "geographic_bounds_west_south_east_north": [-180, -90, 180, 90],
        "processing": "Full image resized with Pillow LANCZOS; global bounds and north-at-top registration preserved",
        "limitations": "Historical monthly color composite; not current imagery, elevation, or ground-level detail",
        "math_review": "Full-globe resampling dimensions and preserved registration specified by the math agent",
    }
    output.with_suffix(".provenance.json").write_text(
        json.dumps(metadata, indent=2) + "\n", encoding="utf-8"
    )
    print(f"Prepared {output}")


if __name__ == "__main__":
    main()
