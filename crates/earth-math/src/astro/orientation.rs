//! Celestial-to-terrestrial orientation, adapted from ERFA 2.0.1.
//!
//! IAU 2006 bias/precession; 77-term IAU 2000B nutation with the 2006
//! obliquity/J2 corrections; IAU 2006 CIO locator; UT1 Earth rotation and
//! IERS polar motion. No observed celestial-pole offsets are applied.
//! The 2000B model's approximately 1 mas performance is documented for
//! 1900–2100, not a bound throughout the application's wider date range.
//!
//! Copyright (C) 2013-2023, NumFOCUS Foundation. All rights reserved.
//! Derived from SOFA via ERFA, not the unmodified SOFA reference library.
//! SPDX-License-Identifier: BSD-3-Clause
//! The complete ERFA license accompanies the coefficient source in
//! `orientation_coefficients.rs` and the distributed third-party notices.
//! Source routines: nut00b, nut06a, pfw06, obl06, fw2m, s06, c2ixys,
//! era00, sp00, pom00, and IERS 2003 fundamental arguments.

use super::Mat3d;
use std::f64::consts::{PI, TAU};

const ARCSEC_TO_RAD: f64 = PI / (180.0 * 3600.0);
const CENTURY_SECONDS: f64 = 36525.0 * 86400.0;

struct NutationTerm {
    arguments: [i8; 5],
    coefficients: [f64; 6],
}
struct CioTerm {
    arguments: [i8; 8],
    sin: f64,
    cos: f64,
}
include!("orientation_coefficients.rs");

fn polynomial(t: f64, coefficients: &[f64]) -> f64 {
    coefficients
        .iter()
        .rev()
        .fold(0.0, |value, coefficient| coefficient + value * t)
}

fn nutation_2006_b(t: f64) -> (f64, f64) {
    // IAU 2000B deliberately uses the linear Delaunay arguments, unlike the
    // full IERS 2003 polynomials used in the CIO locator below.
    let arguments = [
        (485868.249036 + 1717915923.2178 * t) % 1296000.0 * ARCSEC_TO_RAD,
        (1287104.79305 + 129596581.0481 * t) % 1296000.0 * ARCSEC_TO_RAD,
        (335779.526232 + 1739527262.8478 * t) % 1296000.0 * ARCSEC_TO_RAD,
        (1072260.70369 + 1602961601.2090 * t) % 1296000.0 * ARCSEC_TO_RAD,
        (450160.398036 - 6962890.5431 * t) % 1296000.0 * ARCSEC_TO_RAD,
    ];
    let (mut longitude, mut obliquity) = (0.0, 0.0);
    for term in NUTATION_TERMS.iter().rev() {
        let angle = term
            .arguments
            .iter()
            .zip(arguments)
            .map(|(multiplier, argument)| f64::from(*multiplier) * argument)
            .sum::<f64>()
            % TAU;
        let (sin, cos) = angle.sin_cos();
        let [ps, pst, pc, ec, ect, es] = term.coefficients;
        longitude += (ps + pst * t) * sin + pc * cos;
        obliquity += (ec + ect * t) * cos + es * sin;
    }
    // Fixed planetary bias for the rigorous separate-bias/precession method.
    longitude = longitude * (ARCSEC_TO_RAD / 1e7) - 0.135e-3 * ARCSEC_TO_RAD;
    obliquity = obliquity * (ARCSEC_TO_RAD / 1e7) + 0.388e-3 * ARCSEC_TO_RAD;
    // ERFA nut06a's P03 compatibility corrections applied to abridged 2000B.
    let fj2 = -2.7774e-6 * t;
    (
        longitude + longitude * (0.4697e-6 + fj2),
        obliquity + obliquity * fj2,
    )
}

fn bias_precession_nutation(t: f64) -> Mat3d {
    let gamb = polynomial(
        t,
        &[
            -0.052928,
            10.556378,
            0.4932044,
            -0.00031238,
            -0.000002788,
            0.0000000260,
        ],
    ) * ARCSEC_TO_RAD;
    let phib = polynomial(
        t,
        &[
            84381.412819,
            -46.811016,
            0.0511268,
            0.00053289,
            -0.000000440,
            -0.0000000176,
        ],
    ) * ARCSEC_TO_RAD;
    let psib = polynomial(
        t,
        &[
            -0.041775,
            5038.481484,
            1.5584175,
            -0.00018522,
            -0.000026452,
            -0.0000000148,
        ],
    ) * ARCSEC_TO_RAD;
    let epsa = polynomial(
        t,
        &[
            84381.406,
            -46.836769,
            -0.0001831,
            0.00200340,
            -0.000000576,
            -0.0000000434,
        ],
    ) * ARCSEC_TO_RAD;
    let (dpsi, deps) = nutation_2006_b(t);
    // ERFA row rotations pre-multiply the current transformation.
    Mat3d::rotation_x(-(epsa + deps))
        .mul(Mat3d::rotation_z(-(psib + dpsi)))
        .mul(Mat3d::rotation_x(phib))
        .mul(Mat3d::rotation_z(gamb))
}

fn fundamental_arguments(t: f64) -> [f64; 8] {
    let arcseconds = [
        polynomial(
            t,
            &[
                485868.249036,
                1717915923.2178,
                31.8792,
                0.051635,
                -0.00024470,
            ],
        ),
        polynomial(
            t,
            &[
                1287104.793048,
                129596581.0481,
                -0.5532,
                0.000136,
                -0.00001149,
            ],
        ),
        polynomial(
            t,
            &[
                335779.526232,
                1739527262.8478,
                -12.7512,
                -0.001037,
                0.00000417,
            ],
        ),
        polynomial(
            t,
            &[
                1072260.703692,
                1602961601.2090,
                -6.3706,
                0.006593,
                -0.00003169,
            ],
        ),
        polynomial(
            t,
            &[450160.398036, -6962890.5431, 7.4722, 0.007702, -0.00005939],
        ),
    ];
    [
        arcseconds[0] % 1296000.0 * ARCSEC_TO_RAD,
        arcseconds[1] % 1296000.0 * ARCSEC_TO_RAD,
        arcseconds[2] % 1296000.0 * ARCSEC_TO_RAD,
        arcseconds[3] % 1296000.0 * ARCSEC_TO_RAD,
        arcseconds[4] % 1296000.0 * ARCSEC_TO_RAD,
        (3.176146697 + 1021.3285546211 * t) % TAU,
        (1.753470314 + 628.3075849991 * t) % TAU,
        (0.024381750 + 0.00000538691 * t) * t,
    ]
}

fn cio_locator(t: f64, x: f64, y: f64) -> f64 {
    let arguments = fundamental_arguments(t);
    let mut coefficients = [
        94.00e-6,
        3808.65e-6,
        -122.68e-6,
        -72574.11e-6,
        27.98e-6,
        15.62e-6,
    ];
    for (order, terms) in [
        CIO_ORDER_0.as_slice(),
        CIO_ORDER_1.as_slice(),
        CIO_ORDER_2.as_slice(),
        CIO_ORDER_3.as_slice(),
        CIO_ORDER_4.as_slice(),
    ]
    .into_iter()
    .enumerate()
    {
        for term in terms.iter().rev() {
            let angle = term
                .arguments
                .iter()
                .zip(arguments)
                .map(|(multiplier, argument)| f64::from(*multiplier) * argument)
                .sum::<f64>();
            let (sin, cos) = angle.sin_cos();
            coefficients[order] += term.sin * sin + term.cos * cos;
        }
    }
    polynomial(t, &coefficients) * ARCSEC_TO_RAD - x * y / 2.0
}

/// ICRF-aligned celestial axes to ITRS, and Earth Rotation Angle in radians.
///
/// Both epoch arguments are seconds since JD2451545.0 on their distinct
/// named time scales. Polar motion is in radians, with IERS xp/yp signs.
/// The caller validates the finite input and supported date interval.
pub(super) fn earth_orientation(
    tt_seconds_j2000: f64,
    ut1_seconds_j2000: f64,
    xp_rad: f64,
    yp_rad: f64,
) -> (Mat3d, f64) {
    let t = tt_seconds_j2000 / CENTURY_SECONDS;
    let rbpn = bias_precession_nutation(t);
    let (x, y) = (rbpn.rows[2][0], rbpn.rows[2][1]);
    let s = cio_locator(t, x, y);
    let r2 = x * x + y * y;
    let e = if r2 > 0.0 { y.atan2(x) } else { 0.0 };
    let d = (r2 / (1.0 - r2)).sqrt().atan();
    let celestial_to_intermediate = Mat3d::rotation_z(-(e + s))
        .mul(Mat3d::rotation_y(d))
        .mul(Mat3d::rotation_z(e));

    // Fractional-day separation avoids taking a huge rotation modulo2pi.
    // J2000's Julian date is integral, so its fractional contribution is zero.
    let days = ut1_seconds_j2000 / 86400.0;
    let era = (TAU * (days % 1.0 + 0.7790572732640 + 0.00273781191135448 * days)).rem_euclid(TAU);
    let sp = -47e-6 * t * ARCSEC_TO_RAD;
    let polar_motion = Mat3d::rotation_x(-yp_rad)
        .mul(Mat3d::rotation_y(-xp_rad))
        .mul(Mat3d::rotation_z(sp));
    (
        polar_motion
            .mul(Mat3d::rotation_z(era))
            .mul(celestial_to_intermediate),
        era,
    )
}
