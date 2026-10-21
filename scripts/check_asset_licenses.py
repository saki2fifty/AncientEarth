"""Fail closed on unreviewed, changed, missing or nonredistributable project assets."""
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
APPROVED_LICENSES = {
    "Copernicus-WorldDEM-30", "PDL-1.0", "Public-Domain",
    "NASA-Earth-Observatory-Reuse", "CC-BY-4.0", "MIT",
    # Reviewed NAIF kernel redistribution/modification/commercial-use terms,
    # and Astropy's explicitly BSD-licensed IERS table distribution.
    "NAIF-Kernel-Terms", "BSD-3-Clause",
}


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def validate(root=ROOT):
    manifest = json.loads((root / "licenses/assets.json").read_text(encoding="utf-8"))
    sources = manifest["sources"]
    for name, source in sources.items():
        if (source.get("license") not in APPROVED_LICENSES
                or source.get("reviewed") is not True
                or source.get("redistribution_allowed") is not True
                or source.get("modification_allowed") is not True
                or source.get("commercial_use_allowed") is not True
                or not source.get("terms_url", "").startswith("https://")
                or not source.get("credit")):
            raise ValueError(f"Source requires license review: {name}")
        if "license_file" in source or "license_sha256" in source:
            relative = source.get("license_file", "")
            license_path = (root / relative).resolve()
            if (not relative or not license_path.is_relative_to((root / "licenses").resolve())
                    or not license_path.is_file()
                    or sha256(license_path) != source.get("license_sha256")):
                raise ValueError(f"Missing or changed reviewed license file: {name}")
    actual = {
        path.relative_to(root).as_posix()
        for directory in ("assets", "docs/images")
        for path in (root / directory).rglob("*")
        if path.is_file()
    }
    reviewed = set()
    for asset in manifest["assets"]:
        path = asset["path"]
        if path in reviewed or path not in actual:
            raise ValueError(f"Duplicate, missing or out-of-scope asset: {path}")
        reviewed.add(path)
        if not asset.get("sources") or any(source not in sources for source in asset["sources"]):
            raise ValueError(f"Unknown asset source: {path}")
        if sha256(root / path) != asset["sha256"]:
            raise ValueError(f"Asset changed since its license review: {path}")
        if "provenance" in asset:
            provenance = asset["provenance"]
            if provenance in reviewed or provenance not in actual:
                raise ValueError(f"Missing or duplicate provenance: {provenance}")
            reviewed.add(provenance)
            if sha256(root / provenance) != asset["provenance_sha256"]:
                raise ValueError(f"Provenance changed: {provenance}")
            record = json.loads((root / provenance).read_text(encoding="utf-8"))
            if record["output_sha256"] != asset["sha256"]:
                raise ValueError(f"Provenance/output hash mismatch: {path}")
    if actual != reviewed:
        raise ValueError(f"Unreviewed files: {sorted(actual - reviewed)}")
    for required in ("LICENSE", "NOTICE.md", "THIRD_PARTY_LICENSES.txt", "licenses/software-manifest.json"):
        if not (root / required).is_file():
            raise ValueError(f"Missing distribution notice: {required}")
    notice = (root / "NOTICE.md").read_text(encoding="utf-8")
    for source in sources.values():
        if source["notice_marker"] not in notice:
            raise ValueError(f"Required source attribution missing: {source['credit']}")
    return len(manifest["assets"])


if __name__ == "__main__":
    print(f"Asset license audit passed: {validate()} reviewed assets; no unreviewed files")
