"""Convert the Natural Earth georeferenced TIFF to the app's illustrative texture.

Usage: python scripts/prepare_texture.py [.cache/NE1_50M_SR_W.zip]
Requires Pillow. This is raster data conversion; it creates no map geometry.
"""

import hashlib
import io
import json
from pathlib import Path
import sys
import zipfile

from PIL import Image


ROOT = Path(__file__).resolve().parents[1]
SOURCE = Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / ".cache/NE1_50M_SR_W.zip"
OUTPUT = ROOT / "assets/textures"
SIZE = (4096, 2048)


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def main() -> None:
    archive_bytes = SOURCE.read_bytes()
    with zipfile.ZipFile(io.BytesIO(archive_bytes)) as archive:
        tif_name = next(name for name in archive.namelist() if name.lower().endswith(".tif"))
        tfw_name = next(name for name in archive.namelist() if name.lower().endswith(".tfw"))
        image_bytes = archive.read(tif_name)
        worldfile_bytes = archive.read(tfw_name)
    # World-file coordinates refer to pixel centres, not outer image boundaries.
    dx, rotate_y, rotate_x, dy, left_center, top_center = map(
        float, worldfile_bytes.decode("ascii").split()
    )
    with Image.open(io.BytesIO(image_bytes)) as image:
        width, height = image.size
        bounds = [
            left_center - dx / 2,
            top_center + (height - 0.5) * dy,
            left_center + (width - 0.5) * dx,
            top_center - dy / 2,
        ]
        if rotate_x != 0 or rotate_y != 0 or dx <= 0 or dy >= 0:
            raise ValueError("Expected unrotated equirectangular image, north at top.")
        if any(abs(actual - expected) > 1e-8 for actual, expected in zip(bounds, [-180, -90, 180, 90])):
            raise ValueError(f"Unexpected georeferenced coverage: {bounds}")
        if width != height * 2:
            raise ValueError("Expected a 2:1 whole-world geographic grid.")
        OUTPUT.mkdir(parents=True, exist_ok=True)
        image.convert("RGB").resize(SIZE, Image.Resampling.LANCZOS).save(
            OUTPUT / "earth.jpg", quality=90, subsampling=0, optimize=True
        )
    metadata = {
        "dataset": "Natural Earth I, 1:50 million, shaded relief and water",
        "version_on_source_page": "3.2.0",
        "source_page": "https://www.naturalearthdata.com/downloads/50m-raster-data/50m-natural-earth-1/",
        "source_download": "https://naturalearth.s3.amazonaws.com/50m_raster/NE1_50M_SR_W.zip",
        "license": "Public domain",
        "license_url": "https://www.naturalearthdata.com/about/terms-of-use/",
        "retrieved_date": "2026-10-20",
        "source_archive_sha256": sha256(archive_bytes),
        "source_tiff_sha256": sha256(image_bytes),
        "source_world_file_sha256": sha256(worldfile_bytes),
        "source_world_file": [dx, rotate_y, rotate_x, dy, left_center, top_center],
        "source_dimensions_pixels": [width, height],
        "verified_bounds_lon_lat_degrees": bounds,
        "output_dimensions_pixels": list(SIZE),
        "output_file": "earth.jpg",
        "output_sha256": sha256((OUTPUT / "earth.jpg").read_bytes()),
        "resampling": "Pillow LANCZOS; full outer geographic bounds preserved",
        "mapping": "u=(longitude+180)/360 wrapped; v=(90-latitude)/180; north at top",
        "limitations": "Illustrative finite-resolution shaded-relief color map; not elevation, terrain geometry, satellite ground truth, or a precision survey."
    }
    (OUTPUT / "earth.provenance.json").write_text(json.dumps(metadata, indent=2) + "\n", encoding="utf-8")
    print(f"Verified world bounds and wrote {OUTPUT / 'earth.jpg'} ({SIZE[0]}x{SIZE[1]}).")


if __name__ == "__main__":
    main()
