"""Independently regenerate orientation fixtures using Python ERFA.

The Rust implementation is reviewed separately from this reference call path:
nut00b + published P03 correction -> pn06 -> s06/c2ixys -> ERA/pom00/c2tcio.
Requires pyerfa (development only), never an application dependency.
"""
from pathlib import Path
import argparse
import erfa

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "crates/earth-math/src/astro/orientation_review.rs"
CASES = [
    (0.0, -64.184, 0.0, 0.0),
    (-4_733_640_000.25, -4_733_640_007.25, 1.2e-6, -2.1e-6),
    (-3_155_760_000.125, -3_155_760_002.125, -0.8e-6, 0.4e-6),
    (845_856_123.456, 845_856_054.333, 0.7e-6, 1.7e-6),
    (3_155_760_000.75, 3_155_759_798.75, -0.4e-6, -1.3e-6),
    (4_733_640_000.5, 4_733_639_700.5, 0.0, 0.0),
]


def generate() -> str:
    rows = []
    for tt, ut1, xp, yp in CASES:
        centuries = tt / (36525.0 * 86400.0)
        longitude, obliquity = erfa.nut00b(2451545.0, tt / 86400.0)
        factor = -2.7774e-6 * centuries
        longitude *= 1.0 + 0.4697e-6 + factor
        obliquity *= 1.0 + factor
        *_, rbpn = erfa.pn06(2451545.0, tt / 86400.0, longitude, obliquity)
        x, y = erfa.bpn2xy(rbpn)
        s = erfa.s06(2451545.0, tt / 86400.0, x, y)
        c2i = erfa.c2ixys(x, y, s)
        era = erfa.era00(2451545.0, ut1 / 86400.0)
        sp = erfa.sp00(2451545.0, tt / 86400.0)
        polar = erfa.pom00(xp, yp, sp)
        matrix = erfa.c2tcio(c2i, era, polar)
        coefficients = repr(matrix.tolist())
        rows.append(f"({tt!r},{ut1!r},{xp!r},{yp!r},{float(era)!r},{coefficients})")
    text = """// Independently generated with Python ERFA; see scripts/verify_astronomy_orientation.py.
use super::{Vec3d, orientation::earth_orientation};
#[test]
fn orientation_matches_independent_erfa_reference_and_preserves_geometry() {
    #[rustfmt::skip]
    let cases = [
"""
    text += ",\n".join(rows) + "\n];\n"
    text += """
    for (tt, ut1, xp, yp, expected_era, expected_matrix) in cases {
        let (matrix, era) = earth_orientation(tt, ut1, xp, yp);
        assert!(
            (era - expected_era).abs() < 2e-12,
            "ERA {tt}: {era} vs {expected_era}"
        );
        for (row, expected) in matrix.rows.iter().zip(expected_matrix) {
            for (actual, reference) in row.iter().zip(expected) {
                assert!(
                    (actual - reference).abs() < 2e-12,
                    "matrix {tt}: {actual} vs {reference}"
                );
            }
        }
        let vector = Vec3d::new(6378137.0, -1234567.0, 4567890.0);
        let rotated = matrix.transform(vector);
        let recovered = matrix.transpose().transform(rotated);
        assert!((vector - recovered).length() < 1e-8);
        let x = matrix.transform(Vec3d::new(1.0, 0.0, 0.0));
        let y = matrix.transform(Vec3d::new(0.0, 1.0, 0.0));
        let z = matrix.transform(Vec3d::new(0.0, 0.0, 1.0));
        assert!((x.cross(y) - z).length() < 1e-14);
    }
}
"""
    return text


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    result = generate()
    if args.check:
        if OUTPUT.read_text(encoding="utf-8") != result:
            raise SystemExit("Orientation fixtures differ from independent ERFA result")
    else:
        OUTPUT.write_text(result, encoding="utf-8", newline="\n")
    print(f"Verified {len(CASES)} complete Earth-orientation reference matrices")
