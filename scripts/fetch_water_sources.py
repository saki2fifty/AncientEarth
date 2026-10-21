"""Retrieve the exact reviewed Copernicus water-mask packages into the dev cache.

Only downloads and verifies bytes. Scientific registration and processing belong
to prepare_water.py. Runtime and Windows packages do not require network access.
"""
from __future__ import annotations

import hashlib
import json
from pathlib import Path
from urllib.request import urlopen


ROOT = Path(__file__).resolve().parents[1]


def main() -> None:
    review = json.loads((ROOT / "licenses/water-sources.json").read_text(encoding="utf-8"))
    cache = ROOT / ".cache/water"
    cache.mkdir(parents=True, exist_ok=True)
    for record in review["files"]:
        target = cache / record["filename"]
        content = target.read_bytes() if target.is_file() else None
        if content is None or hashlib.sha256(content).hexdigest() != record["sha256"]:
            with urlopen(record["url"], timeout=60) as response:
                content = response.read()
            if hashlib.sha256(content).hexdigest() != record["sha256"]:
                raise ValueError(f"Provider bytes changed; new source review required: {record['url']}")
            target.write_bytes(content)
        print(f"Verified: {target.relative_to(ROOT).as_posix()}")


if __name__ == "__main__":
    main()
