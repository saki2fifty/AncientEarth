"""Regression tests for release-blocking license checks; no network or assets required."""
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from check_asset_licenses import validate
from software_licenses import select_license


class LicenseChecks(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        for name in ("assets", "docs/images", "licenses"):
            (self.root / name).mkdir(parents=True)
        for name in ("LICENSE", "THIRD_PARTY_LICENSES.txt", "licenses/software-manifest.json"):
            (self.root / name).write_text("fixture")
        (self.root / "NOTICE.md").write_text("Test source credit")
        (self.root / "assets/map.bin").write_bytes(b"reviewed fixture")
        self.manifest = {
            "sources": {"fixture": {
                "license": "Public-Domain", "reviewed": True,
                "redistribution_allowed": True, "modification_allowed": True,
                "commercial_use_allowed": True, "terms_url": "https://example.org/terms",
                "credit": "Test source credit", "notice_marker": "Test source credit",
            }},
            "assets": [{"path": "assets/map.bin", "sources": ["fixture"],
                        "sha256": hashlib.sha256(b"reviewed fixture").hexdigest()}],
        }
        self.save()

    def save(self):
        (self.root / "licenses/assets.json").write_text(json.dumps(self.manifest))

    def test_reviewed_fixture_passes(self):
        self.assertEqual(validate(self.root), 1)

    def test_new_or_changed_assets_block_distribution(self):
        (self.root / "assets/unreviewed.png").write_bytes(b"new")
        with self.assertRaisesRegex(ValueError, "Unreviewed"):
            validate(self.root)
        (self.root / "assets/unreviewed.png").unlink()
        (self.root / "assets/map.bin").write_bytes(b"changed")
        with self.assertRaisesRegex(ValueError, "changed"):
            validate(self.root)

    def test_noncommercial_or_unknown_license_is_rejected(self):
        self.manifest["sources"]["fixture"]["commercial_use_allowed"] = False
        self.save()
        with self.assertRaisesRegex(ValueError, "license review"):
            validate(self.root)
        self.manifest["sources"]["fixture"]["commercial_use_allowed"] = True
        self.manifest["sources"]["fixture"]["license"] = "Unknown"
        self.save()
        with self.assertRaisesRegex(ValueError, "license review"):
            validate(self.root)

    def test_missing_attribution_blocks_distribution(self):
        (self.root / "NOTICE.md").write_text("Credits removed")
        with self.assertRaisesRegex(ValueError, "attribution missing"):
            validate(self.root)

    def test_original_license_must_be_preserved(self):
        source = self.manifest["sources"]["fixture"]
        source["license_file"] = "licenses/original.txt"
        source["license_sha256"] = hashlib.sha256(b"original terms").hexdigest()
        self.save()
        with self.assertRaisesRegex(ValueError, "reviewed license file"):
            validate(self.root)
        (self.root / "licenses/original.txt").write_bytes(b"original terms")
        self.assertEqual(validate(self.root), 1)
        (self.root / "licenses/original.txt").write_bytes(b"altered terms")
        with self.assertRaisesRegex(ValueError, "reviewed license file"):
            validate(self.root)
        source["license_file"] = "../outside.txt"
        self.save()
        with self.assertRaisesRegex(ValueError, "reviewed license file"):
            validate(self.root)

    def test_license_alternatives_and_obligations(self):
        self.assertEqual(select_license("LGPL-2.1-or-later OR MIT"), "MIT")
        self.assertEqual(select_license("Zlib AND (MIT OR Apache-2.0)"), "Zlib AND MIT")
        with self.assertRaises(ValueError):
            select_license("MIT AND LicenseRef-Unknown")
        with self.assertRaises(ValueError):
            select_license("MIT !!!")


if __name__ == "__main__":
    unittest.main()
