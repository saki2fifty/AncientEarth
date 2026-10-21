//! Independent calendar, ephemeris, body-orientation and apparent-position
//! fixtures produced by original CSPICE/ERFA, not the production algorithms.
use earth_math::Vec3d;
use earth_math::astro::*;
use std::path::PathBuf;
use std::sync::OnceLock;

#[path = "fixtures/astronomy.rs"]
mod reference;

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        actual.is_finite() && (actual - expected).abs() <= tolerance,
        "actual={actual:.17e}, expected={expected:.17e}, tolerance={tolerance:.17e}"
    );
}
fn vector(values: [f64; 3]) -> Vec3d {
    Vec3d::new(values[0], values[1], values[2])
}
fn asset(name: &str) -> Vec<u8> {
    std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/astronomy")
            .join(name),
    )
    .unwrap()
}
fn ephemeris() -> &'static Ephemeris {
    static EPHEMERIS: OnceLock<Ephemeris> = OnceLock::new();
    EPHEMERIS.get_or_init(|| Ephemeris::from_bytes(&asset("de440s.bsp")).unwrap())
}
fn instant(label: &str) -> AstroInstant {
    AstroInstant::from_calendar(CalendarDateTime::parse(label).unwrap()).unwrap()
}

#[test]
fn original_de440_states_and_derivatives_match_cspice_including_coverage_endpoints() {
    let ephemeris = ephemeris();
    for &(body, epoch, position, velocity) in reference::STATES {
        let state = ephemeris.state(body, epoch).unwrap();
        close(state.position_m.distance(vector(position)), 0.0, 0.0005);
        close(state.velocity_mps.distance(vector(velocity)), 0.0, 1e-8);
    }
    let (start, end) = ephemeris.coverage_tdb_seconds();
    assert!(ephemeris.state(399, start - 1.0).is_err());
    assert!(ephemeris.state(399, end + 1.0).is_err());
    assert!(ephemeris.state(399, f64::NAN).is_err());
    assert!(ephemeris.state(123_456, 0.0).is_err());
}

#[test]
fn lunar_and_solar_body_frames_match_original_naif_text_pck() {
    for &(body, epoch, expected) in reference::BODY_ROTATIONS {
        let actual = iau_body_orientation(body, epoch).unwrap();
        for (row, wanted) in actual.rows.iter().zip(expected) {
            for (value, target) in row.iter().zip(wanted) {
                close(*value, target, 2e-11);
            }
        }
        let identity = actual.mul(actual.transpose());
        for row in 0..3 {
            for col in 0..3 {
                close(
                    identity.rows[row][col],
                    if row == col { 1.0 } else { 0.0 },
                    1e-14,
                );
            }
        }
    }
    assert!(iau_body_orientation(399, 0.0).is_err());
    assert!(iau_body_orientation(301, f64::INFINITY).is_err());
}

#[test]
fn geocentric_fairhead_time_model_matches_all_original_erfa_terms() {
    for &(tt, expected) in reference::TDB_MINUS_TT {
        close(tdb_minus_tt_seconds(tt), expected, 1e-15);
    }
}

#[test]
fn calendar_to_tt_matches_erfa_before_and_after_atomic_utc_and_during_leaps() {
    for &(label, expected) in reference::UTC_TO_TT {
        let value = instant(label);
        close(value.tt_seconds_j2000(), expected, 3e-6);
        let source = CalendarDateTime::parse(label).unwrap();
        let restored = value.calendar();
        assert_eq!(
            (
                restored.year,
                restored.month,
                restored.day,
                restored.hour,
                restored.minute
            ),
            (
                source.year,
                source.month,
                source.day,
                source.hour,
                source.minute
            )
        );
        close(restored.second, source.second, 2e-6);
    }
}

#[test]
fn leap_seconds_occupy_distinct_si_seconds_in_both_clock_directions() {
    let mut clock = SimulationClock::from_calendar("2016-12-31 23:59:59").unwrap();
    clock.step(1, 1.0).unwrap();
    assert_eq!(clock.calendar_string(), "2016-12-31 23:59:60.000");
    clock.step(1, 1.0).unwrap();
    assert_eq!(clock.calendar_string(), "2017-01-01 00:00:00.000");
    clock.step(-1, 1.0).unwrap();
    assert_eq!(clock.calendar_string(), "2016-12-31 23:59:60.000");
    clock.step(-1, 1.0).unwrap();
    assert_eq!(clock.calendar_string(), "2016-12-31 23:59:59.000");
    assert_eq!(instant("2000-02-29 12:00:00").calendar().day, 29);
    for label in [
        "1900-02-29 12:00:00",
        "2100-02-29 12:00:00",
        "2016-12-30 23:59:60",
        "2017-12-31 23:59:60",
        "2024-01-01 00:00:NaN",
    ] {
        assert!(CalendarDateTime::parse(label).is_err(), "{label}");
    }
    // Formatting must not round an ordinary minute's last fraction into a
    // nonexistent leap second, nor an actual leap second to second61.
    for label in ["2024-01-01 12:34:59.9999", "2016-12-31 23:59:60.9999"] {
        assert!(CalendarDateTime::parse(&instant(label).calendar_string()).is_ok());
    }
}

#[test]
fn clock_bounds_and_invalid_numeric_epochs_reject_without_panics_or_partial_changes() {
    for value in [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::MAX,
        -f64::MAX,
        1e100,
        -1e100,
    ] {
        assert!(AstroInstant::from_unix_utc(value).is_err());
        assert!(AstroInstant::from_tt_seconds_j2000(value).is_err());
    }
    let mut clock = SimulationClock::from_calendar("2149-12-31 23:59:59").unwrap();
    let before = clock.instant();
    assert!(clock.step(1, 2.0).is_err());
    assert_eq!(clock.instant(), before);
    assert!(clock.set_calendar("2024-02-30 12:00:00").is_err());
    assert_eq!(clock.instant(), before);
    assert!(clock.step(i64::MAX, 1.0).is_err());
    for width in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(clock.step(1, width).is_err());
        assert_eq!(clock.instant(), before);
    }
    clock.running = true;
    clock.rate = 2.0;
    assert!(clock.advance(1.0).is_err());
    assert!(!clock.running);
    assert_eq!(clock.instant(), before);
    let mut backwards = SimulationClock::from_calendar("1850-01-01 00:00:00").unwrap();
    assert!(backwards.step(-1, 1.0).is_err());
}

fn synthetic_sky() -> AstronomyFrame {
    let mut frame = ephemeris()
        .sample(instant("2000-01-01 12:00:00"), None, Vec3d::ZERO)
        .unwrap();
    frame.sun.apparent_center_ecef_m = Vec3d::new(10.0, 0.0, 0.0);
    frame.sun.radius_m = 2.0;
    frame.moon.apparent_center_ecef_m = Vec3d::new(6.0, 10.0, 0.0);
    frame.moon.radius_m = 1.0;
    frame
}

#[test]
fn celestial_sphere_range_is_normalized_handles_inside_surface_and_tangency() {
    let frame = synthetic_sky();
    for scale in [1e-200, 1.0, 7.0, 1e200] {
        let hit = frame
            .nearest_celestial(
                earth_math::Ray {
                    origin: Vec3d::ZERO,
                    direction: Vec3d::new(scale, 0.0, 0.0),
                },
                None,
            )
            .unwrap();
        assert_eq!(hit.body, CelestialKind::Sun);
        close(hit.distance_m, 8.0, 2e-14);
        close(hit.position_ecef_m.x, 8.0, 2e-14);
        close(hit.normal_ecef.x, -1.0, 1e-14);
    }
    for (origin, expected) in [
        (Vec3d::new(10.0, 0.0, 0.0), 2.0),
        (Vec3d::new(8.0, 0.0, 0.0), 0.0),
        (Vec3d::new(0.0, 2.0, 0.0), 10.0),
    ] {
        let hit = frame
            .nearest_celestial(
                earth_math::Ray {
                    origin,
                    direction: Vec3d::new(1.0, 0.0, 0.0),
                },
                None,
            )
            .unwrap();
        close(hit.distance_m, expected, 2e-14);
        close(hit.normal_ecef.length(), 1.0, 1e-14);
    }
    for ray in [
        earth_math::Ray {
            origin: Vec3d::new(0.0, 2.0 + 1e-9, 0.0),
            direction: Vec3d::new(1.0, 0.0, 0.0),
        },
        earth_math::Ray {
            origin: Vec3d::ZERO,
            direction: Vec3d::new(-1.0, 0.0, 0.0),
        },
        earth_math::Ray {
            origin: Vec3d::ZERO,
            direction: Vec3d::ZERO,
        },
    ] {
        assert!(frame.nearest_celestial(ray, None).is_none());
    }
}

#[test]
fn celestial_range_selects_nearest_body_and_respects_earth_and_grass_occlusion() {
    let mut frame = synthetic_sky();
    let ray = earth_math::Ray {
        origin: Vec3d::ZERO,
        direction: Vec3d::new(1.0, 0.0, 0.0),
    };
    frame.moon.apparent_center_ecef_m = Vec3d::new(6.0, 0.0, 0.0);
    let moon = frame.nearest_celestial(ray, None).unwrap();
    assert_eq!(moon.body, CelestialKind::Moon);
    close(moon.distance_m, 5.0, 2e-14);
    for nearer_surface in [0.0, 4.99, 5.0] {
        assert!(frame.nearest_celestial(ray, Some(nearer_surface)).is_none());
    }
    assert_eq!(
        frame.nearest_celestial(ray, Some(5.01)).unwrap().body,
        CelestialKind::Moon
    );
    frame.moon.apparent_center_ecef_m.x = 20.0;
    assert_eq!(
        frame.nearest_celestial(ray, None).unwrap().body,
        CelestialKind::Sun
    );
    // Solar-system scales exercise cancellation at the smaller forward root.
    frame.sun.apparent_center_ecef_m.x = 149_597_870_700.0;
    frame.sun.radius_m = SUN_RADIUS_M;
    frame.moon.apparent_center_ecef_m.y = 1e12;
    let solar = frame.nearest_celestial(ray, None).unwrap();
    close(solar.distance_m, 149_597_870_700.0 - SUN_RADIUS_M, 0.0001);
}

#[test]
fn light_time_and_relativistic_apparent_centres_match_cspice_plus_erfa() {
    let ephemeris = ephemeris();
    for &(tt, id, geometric, apparent, light_time) in reference::SAMPLE_BODIES {
        let frame = ephemeris
            .sample(
                AstroInstant::from_tt_seconds_j2000(tt).unwrap(),
                None,
                Vec3d::ZERO,
            )
            .unwrap();
        let body = if id == 10 { frame.sun } else { frame.moon };
        let inverse = frame.icrf_to_ecef.transpose();
        close(
            inverse
                .transform(body.center_ecef_m)
                .distance(vector(geometric)),
            0.0,
            0.001,
        );
        close(
            inverse
                .transform(body.apparent_center_ecef_m)
                .distance(vector(apparent)),
            0.0,
            0.001,
        );
        close(body.light_time_seconds, light_time, 1e-9);
        close(
            body.apparent_center_ecef_m.length() / LIGHT_SPEED_MPS,
            body.light_time_seconds,
            1e-9,
        );
        assert_eq!(
            body.radius_m,
            if id == 10 {
                SUN_RADIUS_M
            } else {
                MOON_RADIUS_M
            }
        );
        assert!((0.0..=1.0).contains(&frame.moon_illuminated_fraction));
        close(frame.moon_to_sun_direction_ecef.length(), 1.0, 1e-14);
    }
}

#[test]
fn observer_parallax_and_quality_flags_preserve_supported_model_limits() {
    let ephemeris = ephemeris();
    let eop = EarthOrientationData::from_bytes(&asset("finals2000A.all")).unwrap();
    let date = instant("2024-04-08 18:00:00");
    let center = ephemeris.sample(date, Some(&eop), Vec3d::ZERO).unwrap();
    let site = Vec3d::new(6_378_137.0, 0.0, 0.0);
    let topocentric = ephemeris.sample(date, Some(&eop), site).unwrap();
    assert_eq!(
        center.quality.earth_rotation,
        EarthRotationQuality::ObservedIers
    );
    assert_eq!(center.sun.center_ecef_m, topocentric.sun.center_ecef_m);
    assert_eq!(center.moon.center_ecef_m, topocentric.moon.center_ecef_m);
    let central_direction = center.moon.apparent_center_ecef_m.normalized().unwrap();
    let topocentric_direction = (topocentric.moon.apparent_center_ecef_m - site)
        .normalized()
        .unwrap();
    assert!((central_direction - topocentric_direction).length() > 0.001);
    assert!(center.moon_illuminated_fraction < 0.001); // Solar-eclipse new Moon.
    let historical = ephemeris
        .sample(instant("1850-01-01 00:00:00"), Some(&eop), Vec3d::ZERO)
        .unwrap();
    assert!(historical.quality.historical_ut1_calendar);
    assert!(historical.quality.outside_nutation_reference_interval);
    assert_eq!(
        historical.quality.earth_rotation,
        EarthRotationQuality::ModeledDeltaT
    );
    let future = ephemeris
        .sample(instant("2149-12-31 23:59:59"), Some(&eop), Vec3d::ZERO)
        .unwrap();
    assert!(future.quality.future_leap_seconds_assumed);
    assert!(future.quality.outside_nutation_reference_interval);
    assert_eq!(
        future.quality.earth_rotation,
        EarthRotationQuality::ModeledDeltaT
    );
    assert!(
        ephemeris
            .sample(date, None, Vec3d::new(f64::NAN, 0.0, 0.0))
            .is_err()
    );
}

fn eop_row(mjd: f64, dut1: f64, predicted: bool) -> String {
    let mut row = vec![b' '; 188];
    let fields = [
        (7, format!("{mjd:8.2}")),
        (18, format!("{:9.6}", 0.1)),
        (37, format!("{:9.6}", -0.2)),
        (58, format!("{dut1:10.7}")),
    ];
    for (offset, field) in fields {
        row[offset..offset + field.len()].copy_from_slice(field.as_bytes());
    }
    row[16] = b'I';
    row[57] = if predicted { b'P' } else { b'I' };
    String::from_utf8(row).unwrap()
}

#[test]
fn iers_interpolation_keeps_ut1_continuous_across_a_published_leap_second() {
    // At these two midnights the UTC offset increases from36s to37s.
    // Deliberately hold UT1−TAI = −36.4s, while UT1−UTC jumps by+1s.
    let data = format!(
        "{}\n{}\n",
        eop_row(57753.0, -0.4, false),
        eop_row(57754.0, 0.6, false)
    );
    let eop = EarthOrientationData::from_bytes(data.as_bytes()).unwrap();
    let mut epochs = Vec::new();
    for date in [
        "2016-12-31 12:00:00",
        "2016-12-31 23:59:59",
        "2016-12-31 23:59:60",
        "2017-01-01 00:00:00",
    ] {
        let tt = instant(date);
        let frame = ephemeris().sample(tt, Some(&eop), Vec3d::ZERO).unwrap();
        assert_eq!(
            frame.quality.earth_rotation,
            EarthRotationQuality::ObservedIers
        );
        let reported_ut1 = (frame.ut1_jd - 2_451_545.0) * 86400.0;
        close(reported_ut1, tt.tt_seconds_j2000() - 68.584, 3e-5);
        epochs.push(reported_ut1);
    }
    close(epochs[2] - epochs[1], 1.0, 5e-5);
    close(epochs[3] - epochs[2], 1.0, 5e-5);
    let predicted = format!(
        "{}\n{}\n",
        eop_row(57753.0, -0.4, false),
        eop_row(57754.0, 0.6, true)
    );
    let predicted = EarthOrientationData::from_bytes(predicted.as_bytes()).unwrap();
    let frame = ephemeris()
        .sample(
            instant("2016-12-31 12:00:00"),
            Some(&predicted),
            Vec3d::ZERO,
        )
        .unwrap();
    assert_eq!(
        frame.quality.earth_rotation,
        EarthRotationQuality::PredictedIers
    );
}

#[test]
fn malformed_kernel_and_orientation_data_are_rejected_explicitly() {
    let original = asset("de440s.bsp");
    for size in [0, 7, 95, 1024] {
        assert!(Ephemeris::from_bytes(&original[..size]).is_err());
    }
    let mut altered = original;
    altered[88..96].copy_from_slice(b"BIG-IEEE");
    assert!(Ephemeris::from_bytes(&altered).is_err());
    for data in [
        "not an EOP table".to_string(),
        format!(
            "{}\n{}\n",
            eop_row(57754.0, 0.6, false),
            eop_row(57753.0, -0.4, false)
        ),
        format!(
            "{}\n{}\n",
            eop_row(57753.0, -0.4, false),
            eop_row(57754.0, 3.0, false)
        ),
    ] {
        assert!(EarthOrientationData::from_bytes(data.as_bytes()).is_err());
    }
    assert!(iau_body_orientation(301, f64::MAX).is_err());
}
