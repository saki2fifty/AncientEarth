"""Reproduce independent CSPICE/ERFA fixtures for the Rust astronomy review.

Uses the original DE440s SPK and NAIF pck00011.tpc, never AncientEarth's
SPK reader, calendar routines, or coefficient tables. Optional reference
dependencies are spiceypy, pyerfa, and numpy. No network access is performed.
"""
from __future__ import annotations

import argparse
import hashlib
from pathlib import Path
import warnings

import erfa
import numpy as np
import spiceypy as spice

ROOT = Path(__file__).resolve().parents[1]


def literal(value: float) -> str:
    return repr(float(value))


def vector(values) -> str:
    return "[" + ", ".join(literal(value) for value in values) + "]"


def fixtures(kernel: Path, pck: Path) -> str:
    spice.kclear()
    spice.furnsh(str(kernel.resolve()))
    spice.furnsh(str(pck.resolve()))
    coverage = list(spice.spkcov(str(kernel.resolve()), 399))
    assert len(coverage) == 2
    epochs = [coverage[0], -4_700_000_000.0, -2_000_000_000.0, -1.0, 0.0,
              1.0, 123_456_789.1234, 1_000_000_000.0, 4_700_000_000.0, coverage[1]]
    lines = [
        "// Generated independently by scripts/verify_astronomy.py.",
        f"// CSPICE via spiceypy {spice.__version__}; ERFA via pyerfa {erfa.__version__}.",
        "// Numerical results from original NAIF/ERFA references; no production algorithms used.",
        f"// DE440s SHA256: {hashlib.sha256(kernel.read_bytes()).hexdigest()}",
        f"// pck00011 SHA256: {hashlib.sha256(pck.read_bytes()).hexdigest()}",
        "#[rustfmt::skip]",
        "pub const STATES: &[(i32, f64, [f64; 3], [f64; 3])] = &[",
    ]
    for epoch in epochs:
        for body in [3, 10, 301, 399]:
            state, _ = spice.spkez(body, epoch, "J2000", "NONE", 0)
            lines.append(f"    ({body}, {literal(epoch)}, {vector(state[:3] * 1000)}, {vector(state[3:] * 1000)}),")
    lines += ["];", "", "#[rustfmt::skip]",
              "pub const BODY_ROTATIONS: &[(i32, f64, [[f64; 3]; 3])] = &["]
    for epoch in [-4_700_000_000.0, 0.0, 123_456_789.1234, 4_700_000_000.0]:
        for body, frame in [(10, "IAU_SUN"), (301, "IAU_MOON")]:
            rotation = spice.pxform(frame, "J2000", epoch)
            matrix = "[" + ", ".join(vector(row) for row in rotation) + "]"
            lines.append(f"    ({body}, {literal(epoch)}, {matrix}),")
    lines += ["];", "", "#[rustfmt::skip]", "pub const TDB_MINUS_TT: &[(f64, f64)] = &["]
    for tt in epochs:
        offset = erfa.dtdb(2451545.0, tt / 86400.0, 0.0, 0.0, 0.0, 0.0)
        lines.append(f"    ({literal(tt)}, {literal(offset)}),")
    lines += ["];", "", "#[rustfmt::skip]", "pub const UTC_TO_TT: &[(&str, f64)] = &["]
    dates = [
        (1960, 1, 1, 0, 0, 0.0), (1961, 8, 1, 12, 0, 0.0),
        (1965, 4, 17, 15, 31, 42.125), (1972, 1, 1, 0, 0, 0.0),
        (1972, 6, 30, 23, 59, 59.0), (1972, 6, 30, 23, 59, 60.0),
        (1972, 7, 1, 0, 0, 0.0), (2000, 1, 1, 12, 0, 0.0),
        (2016, 12, 31, 23, 59, 59.0), (2016, 12, 31, 23, 59, 60.0),
        (2016, 12, 31, 23, 59, 60.5), (2017, 1, 1, 0, 0, 0.0),
        (2024, 2, 29, 18, 12, 37.125), (2026, 10, 20, 12, 0, 0.0),
    ]
    with warnings.catch_warnings():
        warnings.simplefilter("ignore", erfa.ErfaWarning)  # Reference leap table update warning.
        for year, month, day, hour, minute, second in dates:
            utc = erfa.dtf2d("UTC", year, month, day, hour, minute, second)
            tt1, tt2 = erfa.taitt(*erfa.utctai(*utc))
            seconds = ((tt1 - 2451545.0) + tt2) * 86400.0
            label = f"{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:06.3f}"
            lines.append(f'    ("{label}", {literal(seconds)}),')
    lines += ["];", "",
              "pub type BodySample = (f64, i32, [f64; 3], [f64; 3], f64);",
              "#[rustfmt::skip]",
              "pub const SAMPLE_BODIES: &[BodySample] = &["]
    for tt in [0.0, 123_456_789.1234, 765_871_269.184, 795_290_469.184]:
        tdb = tt + erfa.dtdb(2451545.0, tt / 86400.0, 0.0, 0.0, 0.0, 0.0)
        earth_state, _ = spice.spkez(399, tdb, "J2000", "NONE", 0)
        beta = earth_state[3:] / 299792.458
        bm1 = np.sqrt(1.0 - beta @ beta)
        for body in [10, 301]:
            geometric, _ = spice.spkez(body, tdb, "J2000", "NONE", 399)
            retarded, light_time = spice.spkez(body, tdb, "J2000", "CN", 399)
            distance = np.linalg.norm(retarded[:3])
            # ERFA aberration with solar-potential contribution made negligible.
            # CSPICE CN supplies independent converged light time. CSPICE's
            # stellar-aberration approximation is not substituted for relativity.
            apparent = erfa.ab(retarded[:3] / distance, beta, 1e100, bm1) * distance
            lines.append(
                f"    ({literal(tt)}, {body}, {vector(geometric[:3] * 1000)}, "
                f"{vector(apparent * 1000)}, {literal(light_time)}),"
            )
    lines += ["];", ""]
    spice.kclear()
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--kernel", type=Path, default=ROOT / "assets/astronomy/de440s.bsp")
    parser.add_argument("--pck", type=Path, default=ROOT / ".cache/astronomy/pck00011.tpc")
    parser.add_argument("--output", type=Path, default=ROOT / "crates/earth-math/tests/fixtures/astronomy.rs")
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    reference = fixtures(args.kernel, args.pck)
    if args.check:
        if args.output.read_text(encoding="utf-8") != reference:
            raise SystemExit("CSPICE/ERFA fixtures differ: inspect reference versions and inputs")
        print("Independent CSPICE/ERFA fixtures reproduced exactly")
    else:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(reference, encoding="utf-8", newline="\n")
        print(f"Wrote independent CSPICE/ERFA fixtures to {args.output}")


if __name__ == "__main__":
    main()
