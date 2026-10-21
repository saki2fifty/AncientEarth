"""Independently decode the licensed GFS reference column with GDAL/Rasterio.

This does not import the Rust GRIB decoder or any production weather formulas.
Install the optional requirements-terrain.txt tools, then run with --check to
reproduce the numerical fixture consumed by the Rust atmospheric review tests.
"""

from __future__ import annotations

import argparse
import hashlib
from pathlib import Path

import rasterio


ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "crates/earth-math/tests/fixtures/weather_gfs_reference.rs"


def reference_text(source: Path) -> str:
    digest = hashlib.sha256(source.read_bytes()).hexdigest()
    expected_hash = "0b5b4658c61b74deae99b2e45cd4391ca120b2bfaee7d7d8b409f146559f3446"
    if digest != expected_hash:
        raise ValueError("unexpected GFS reference payload; review its provenance first")
    fields: dict[tuple[str, str], float] = {}
    with rasterio.Env(GRIB_NORMALIZE_UNITS="NO"):
        with rasterio.open(source) as dataset:
            if (dataset.width, dataset.height, dataset.count) != (3, 3, 164):
                raise ValueError("unexpected source grid")
            longitude, latitude = dataset.xy(1, 1)
            if (longitude, latitude) != (-94.75, 29.25):
                raise ValueError("source post registration changed")
            for band in range(1, dataset.count + 1):
                tags = dataset.tags(band)
                level, element = tags["GRIB_SHORT_NAME"], tags["GRIB_ELEMENT"]
                if not (level.endswith("-ISBL") or level in ("0-SFC", "2-HTGL")):
                    continue
                if (tags["GRIB_REF_TIME"], tags["GRIB_VALID_TIME"], tags["GRIB_FORECAST_SECONDS"]) != (
                    "1792584000", "1792609200", "25200"
                ):
                    raise ValueError("mixed run/forecast time in required fields")
                units = {"TMP": "[K]", "PRES": "[Pa]", "HGT": "[gpm]", "SPFH": "[kg/kg]"}
                if tags["GRIB_UNIT"] != units[element]:
                    raise ValueError("source quantity units changed")
                if "CENTER=7(US-NCEP)" not in tags["GRIB_IDS"] or tags["GRIB_PDS_PDTN"] != "0":
                    raise ValueError("expected instantaneous NOAA/NCEP forecast field")
                key = (level, element)
                if key in fields:
                    raise ValueError("duplicate required field")
                fields[key] = float(dataset.read(band)[1, 1])

    pressures = sorted({int(level.removesuffix("-ISBL")) for level, _ in fields if level.endswith("-ISBL")}, reverse=True)
    lines = [
        "// Generated independently by scripts/verify_weather.py using GDAL/Rasterio.",
        f"// Unmodified NOAA/NCEP GFS payload SHA256: {digest}",
        "// Source run2026-10-21T12:00UTC, valid19:00UTC, post29.25N/94.75W.",
        "// NOAA-produced public-domain data; provenance/terms are in assets/weather.",
        f"pub const SURFACE_PRESSURE_PA: f64 = {fields['0-SFC', 'PRES']!r};",
        f"pub const SURFACE_GEOPOTENTIAL_M: f64 = {fields['0-SFC', 'HGT']!r};",
        f"pub const TEMPERATURE_2M_K: f64 = {fields['2-HTGL', 'TMP']!r};",
        f"pub const SPECIFIC_HUMIDITY_2M: f64 = {fields['2-HTGL', 'SPFH']!r};",
        "// Pressure Pa, geopotential height gpm, temperature K, specific humidity kg/kg.",
        "#[rustfmt::skip]",
        "pub const PRESSURE_LEVELS: &[(f64, f64, f64, f64)] = &[",
    ]
    for pressure in pressures:
        level = f"{pressure}-ISBL"
        values = (float(pressure), fields[level, "HGT"], fields[level, "TMP"], fields[level, "SPFH"])
        lines.append("    (" + ", ".join(repr(value) for value in values) + "),")
    lines.extend([
        "];", "",
    ])
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=ROOT / "assets/weather/gfs-reference.grib2")
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    text = reference_text(args.source)
    if args.check:
        if FIXTURE.read_text(encoding="utf-8") != text:
            raise SystemExit("independent GFS fixture differs; review before regeneration")
    else:
        FIXTURE.write_text(text, encoding="utf-8", newline="\n")
    print("Independent GDAL GFS registration, run/valid time, units and column fixture verified.")


if __name__ == "__main__":
    main()
