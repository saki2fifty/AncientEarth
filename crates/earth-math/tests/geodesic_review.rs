//! WGS84 inverse-geodesic checks. Generic fixtures were independently obtained
//! with PROJ's pyproj.Geod(a=6378137,rf=298.257223563), whose Karney algorithm is
//! related to this dependency; analytic arcs and quadrature are separate checks.
use earth_math::*;

fn geo(lat: f64, lon: f64, h: f64) -> Geodetic {
    Geodetic::new(lat, lon, h).unwrap()
}
fn close(a: f64, b: f64, tolerance: f64) {
    assert!(
        (a - b).abs() <= tolerance,
        "{a:.15} != {b:.15}; tolerance {tolerance}"
    );
}

#[test]
fn inverse_distance_matches_proj_near_antipodes_dateline_and_generic_routes() {
    for (a, b, expected) in [
        ((-41.32, 174.81), (40.96, -5.50), 19_959_679.267_353_82),
        ((0.0, 0.0), (0.0001, 179.9999), 20_003_920.400_274_97),
        ((38.1, 23.7), (-52.3, -167.4), 18_209_264.337_895_58),
        ((12.0, 179.9), (12.0, -179.9), 21_780.529_739_401_212),
    ] {
        let a = geo(a.0, a.1, 0.0);
        let b = geo(b.0, b.1, 0.0);
        close(wgs84_surface_distance(a, b).unwrap(), expected, 1e-6);
        close(wgs84_surface_distance(b, a).unwrap(), expected, 1e-6);
    }
}

#[test]
fn equatorial_arcs_and_polar_antipodal_meridians_match_independent_integrals() {
    for degrees in [0.0_f64, 0.0001, 1.0, 90.0, 170.0] {
        close(
            wgs84_surface_distance(geo(0.0, 0.0, 0.0), geo(0.0, degrees, 0.0)).unwrap(),
            WGS84_A_M * degrees.to_radians(),
            1e-7,
        );
    }
    // Integrate ds/dphi=a(1-e²)/(1-e²sin²phi)^(3/2) with composite
    // Simpson quadrature. This does not call any geodesic implementation.
    let f = 1.0 / 298.257223563;
    let e2 = f * (2.0 - f);
    let n = 16384;
    let step = std::f64::consts::FRAC_PI_2 / f64::from(n);
    let mut sum = 0.0;
    for i in 0..=n {
        let phi = f64::from(i) * step;
        let curvature = WGS84_A_M * (1.0 - e2) / (1.0 - e2 * phi.sin().powi(2)).powf(1.5);
        let weight = if i == 0 || i == n {
            1.0
        } else if i % 2 == 0 {
            2.0
        } else {
            4.0
        };
        sum += weight * curvature;
    }
    let half_meridian = 2.0 * step * sum / 3.0;
    close(
        wgs84_surface_distance(geo(90.0, 10.0, 0.0), geo(-90.0, -73.0, 0.0)).unwrap(),
        half_meridian,
        2e-6,
    );
    close(
        wgs84_surface_distance(geo(0.0, 0.0, 0.0), geo(0.0, 180.0, 0.0)).unwrap(),
        half_meridian,
        2e-6,
    );
    assert_eq!(
        wgs84_surface_distance(geo(90.0, 10.0, 0.0), geo(90.0, -73.0, 0.0)).unwrap(),
        0.0
    );
}

#[test]
fn surface_distance_ignores_valid_heights_but_rejects_invalid_coordinates() {
    let a = geo(0.0, 0.0, 0.0);
    let b = geo(0.0, 30.0, 0.0);
    let surface = wgs84_surface_distance(a, b).unwrap();
    let chord = geodetic_to_ecef(a).distance(geodetic_to_ecef(b));
    assert!(surface > chord);
    assert_eq!(
        surface,
        wgs84_surface_distance(geo(0.0, 0.0, -9000.0), geo(0.0, 30.0, 1e9)).unwrap()
    );
    assert_eq!(wgs84_surface_distance(a, a).unwrap(), 0.0);
    for invalid in [
        Geodetic { lat_deg: 91.0, ..a },
        Geodetic {
            lon_deg: f64::NAN,
            ..a
        },
        Geodetic {
            height_m: f64::INFINITY,
            ..a
        },
    ] {
        assert!(wgs84_surface_distance(invalid, b).is_err());
        assert!(wgs84_surface_distance(a, invalid).is_err());
    }
}
