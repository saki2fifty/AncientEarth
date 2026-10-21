"""Regenerate independent eclipse vectors with CSPICE and ERFA.

No network access and no AncientEarth calculation routines are used. Inputs
are the unchanged bundled DE440s and IERS files. Results are original computed
reference numbers, not a redistributed NASA table or image.

CSPICE spkcvo supplies converged reception light time for a supplied observer
state. ERFA supplies time conversion, ITRS rotation and relativistic aberration.
The final angular-disk overlap is independently integrated by vertical chords,
not evaluated with the production closed-form circle-overlap expression.
"""
from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import math
from pathlib import Path
import warnings

import erfa
import numpy as np
import spiceypy as spice

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "crates/earth-math/tests/fixtures/eclipses.rs"
CASES = [
    ("2024-04-08 18:40:00", 31.72, -97.37, 0.0),
    ("2024-04-08 17:40:00", 31.72, -97.37, 0.0),
    ("2024-04-08 16:00:00", 31.72, -97.37, 0.0),
    ("2024-04-08 18:42:00", 32.78306, -96.80667, 0.0),
    ("2024-04-08 18:40:00", 29.76328, -95.36327, 0.0),
    ("2024-04-08 18:40:00", 31.72, -97.37, 1_000_000.0),
    ("2023-10-14 16:36:00", 35.55, -107.15166666666667, 0.0),
    ("2023-10-14 16:36:00", 35.08449, -106.65114, 0.0),
    ("2023-10-14 16:00:00", 35.55, -107.15166666666667, 0.0),
    ("2023-10-14 14:00:00", 35.55, -107.15166666666667, 0.0),
]


def chord_overlap(radius: float, separation: float) -> float:
    """Independent area quadrature in units of the solar angular radius."""
    if separation >= 1.0 + radius:
        return 0.0
    if separation + 1.0 <= radius:
        return 1.0
    if separation + radius <= 1.0:
        return radius * radius
    left, right = max(-1.0, separation - radius), min(1.0, separation + radius)
    # Separate the crossing of the two upper circular arcs. Substitution
    # x=mid+half*sin(theta) removes square-root endpoint singularities.
    crossing = (1.0 + separation * separation - radius * radius) / (2 * separation)

    def integrate(start: float, end: float, lunar: bool) -> float:
        if start >= end:
            return 0.0
        mid, half = (start + end) / 2, (end - start) / 2

        def f(theta: float) -> float:
            x = mid + half * math.sin(theta)
            square = radius * radius - (x - separation) ** 2 if lunar else 1 - x*x
            return 2 * math.sqrt(max(0.0, square)) * half * math.cos(theta)

        def adaptive(a, b, fa, fm, fb, whole, budget, depth):
            m = (a+b)/2
            fl, fr = f((a+m)/2), f((m+b)/2)
            l = (m-a)*(fa+4*fl+fm)/6
            r = (b-m)*(fm+4*fr+fb)/6
            if depth == 0 or abs(l+r-whole) <= 15*budget:
                return l+r+(l+r-whole)/15
            return (adaptive(a,m,fa,fl,fm,l,budget/2,depth-1)
                    + adaptive(m,b,fm,fr,fb,r,budget/2,depth-1))

        a, b = -math.pi/2, math.pi/2
        fa, fm, fb = f(a), f(0), f(b)
        return adaptive(a,b,fa,fm,fb,(b-a)*(fa+4*fm+fb)/6,1e-13,24)

    return (integrate(left, min(right, crossing), True)
            + integrate(max(left, crossing), right, False)) / math.pi


def rotation(tt_seconds: float, ut1_seconds: float, xp: float, yp: float):
    t = tt_seconds / (36525*86400)
    dpsi, deps = erfa.nut00b(2451545.0, tt_seconds/86400)
    factor = -2.7774e-6*t
    *_, rbpn = erfa.pn06(2451545.0, tt_seconds/86400,
                        dpsi*(1+0.4697e-6+factor), deps*(1+factor))
    x, y = erfa.bpn2xy(rbpn)
    s = erfa.s06(2451545.0, tt_seconds/86400, x, y)
    return erfa.c2tcio(
        erfa.c2ixys(x,y,s),
        erfa.era00(2451545.0, ut1_seconds/86400),
        erfa.pom00(xp,yp,erfa.sp00(2451545.0, tt_seconds/86400)))


def fixture_rows(kernel: Path, eop_path: Path):
    spice.kclear()
    spice.furnsh(str(kernel.resolve()))
    eop = []
    for row in eop_path.read_text(encoding="ascii").splitlines():
        try:
            eop.append([float(row[7:15]), float(row[18:27]),
                        float(row[37:46]), float(row[58:68])])
        except ValueError:
            continue
    daily = np.array(eop)
    results = []
    for label, latitude, longitude, height in CASES:
        date = dt.datetime.strptime(label, "%Y-%m-%d %H:%M:%S")
        with warnings.catch_warnings():
            warnings.simplefilter("ignore", erfa.ErfaWarning)
            utc1,utc2 = erfa.dtf2d("UTC",date.year,date.month,date.day,
                                  date.hour,date.minute,float(date.second))
            tt1,tt2 = erfa.taitt(*erfa.utctai(utc1,utc2))
        tt = ((tt1-2451545.0)+tt2)*86400
        mjd = (utc1-2400000.5)+utc2
        xp,yp,dut1 = [float(np.interp(mjd,daily[:,0],daily[:,i])) for i in (1,2,3)]
        ut1 = ((utc1-2451545.0)+utc2)*86400+dut1
        c2t = rotation(tt,ut1,math.radians(xp/3600),math.radians(yp/3600))
        tdb = tt+erfa.dtdb(2451545.0,tt/86400,0.0,0.0,0.0,0.0)
        ecef = erfa.gd2gc(1,math.radians(longitude),math.radians(latitude),height)
        spin = np.cross([0,0,2*math.pi*1.0027378119113546/86400],ecef)
        obssta = np.concatenate([c2t.T@ecef,c2t.T@spin])/1000
        earth,_ = spice.spkez(399,tdb,"J2000","NONE",0)
        beta = (earth[3:]+obssta[3:])/299792.458
        bm1 = math.sqrt(1-float(beta@beta))
        vectors = []
        for name in ("SUN","MOON"):
            state,_ = spice.spkcvo(name,tdb,"J2000","OBSERVER","CN",
                                  obssta,tdb,"EARTH","J2000")
            distance = float(np.linalg.norm(state[:3]))
            apparent = erfa.ab(state[:3]/distance,beta,1e100,bm1)*distance*1000
            vectors.append(c2t@apparent)
        sun,moon = vectors
        ds,dm = float(np.linalg.norm(sun)),float(np.linalg.norm(moon))
        us,um = sun/ds,moon/dm
        alpha,beta_angle = math.asin(695700000/ds),math.asin(1737400/dm)
        separation = math.atan2(float(np.linalg.norm(np.cross(us,um))),float(us@um))
        obscuration = chord_overlap(beta_angle/alpha,separation/alpha)
        results.append((label,[latitude,longitude,height],ecef,sun,moon,
                        [alpha,beta_angle,separation,obscuration]))
    spice.kclear()
    return results


def generate(kernel: Path, eop: Path) -> str:
    rows = fixture_rows(kernel,eop)
    vector = lambda values: "["+", ".join(repr(float(v)) for v in values)+"]"
    lines = [
        "// Original numerical references generated by scripts/verify_eclipses.py.",
        f"// CSPICE/spiceypy {spice.__version__}, ERFA/pyerfa {erfa.__version__}.",
        "// Reception CN light time, observer aberration, independent chord-area quadrature.",
        f"// DE440s SHA256: {hashlib.sha256(kernel.read_bytes()).hexdigest()}",
        f"// IERS SHA256: {hashlib.sha256(eop.read_bytes()).hexdigest()}",
        "#[rustfmt::skip]",
        "pub type EclipseFixture = (&'static str, [f64; 3], [f64; 3], [f64; 3], [f64; 3], [f64; 4]);",
        "#[rustfmt::skip]",
        "pub const CASES: &[EclipseFixture] = &[",
    ]
    for label,geo,ecef,sun,moon,angles in rows:
        lines.append(f'    ("{label}", {vector(geo)}, {vector(ecef)}, {vector(sun)}, {vector(moon)}, {vector(angles)}),')
    return "\n".join(lines+["];",""])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--kernel",type=Path,default=ROOT/"assets/astronomy/de440s.bsp")
    parser.add_argument("--eop",type=Path,default=ROOT/"assets/astronomy/finals2000A.all")
    parser.add_argument("--output",type=Path,default=OUTPUT)
    parser.add_argument("--check",action="store_true")
    args = parser.parse_args()
    reference = generate(args.kernel,args.eop)
    if args.check:
        if args.output.read_text(encoding="utf-8") != reference:
            raise SystemExit("Independent eclipse fixtures differ; inspect inputs and reference versions")
    else:
        args.output.write_text(reference,encoding="utf-8",newline="\n")
    print(f"Verified {len(CASES)} independent topocentric eclipse reference cases")


if __name__ == "__main__":
    main()
