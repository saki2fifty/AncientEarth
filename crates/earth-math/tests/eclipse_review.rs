//! Independent CSPICE/ERFA, numerical-area and published-path eclipse checks.
use earth_math::astro::*;
use earth_math::{EnuBasis, Geodetic, Observer, Vec3d, geodetic_to_ecef};
use std::path::PathBuf;
use std::sync::OnceLock;

#[path = "fixtures/eclipse_gpu.rs"]
mod gpu_reference;
#[path = "fixtures/eclipses.rs"]
mod reference;

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        actual.is_finite() && (actual - expected).abs() <= tolerance,
        "{actual:.17e} != {expected:.17e} within {tolerance:.3e}"
    );
}
fn vector(v: [f64; 3]) -> Vec3d {
    Vec3d::new(v[0], v[1], v[2])
}
fn sources() -> &'static (Ephemeris, EarthOrientationData) {
    static SOURCES: OnceLock<(Ephemeris, EarthOrientationData)> = OnceLock::new();
    SOURCES.get_or_init(|| {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/astronomy");
        (
            Ephemeris::from_bytes(&std::fs::read(root.join("de440s.bsp")).unwrap()).unwrap(),
            EarthOrientationData::from_bytes(&std::fs::read(root.join("finals2000A.all")).unwrap())
                .unwrap(),
        )
    })
}
fn instant(label: &str) -> AstroInstant {
    AstroInstant::from_calendar(CalendarDateTime::parse(label).unwrap()).unwrap()
}
fn frame(label: &str, position: Vec3d) -> AstronomyFrame {
    let (ephemeris, orientation) = sources();
    ephemeris
        .sample(instant(label), Some(orientation), position)
        .unwrap()
}
fn static_field(sun: Vec3d, moon: Vec3d, sr: f64, mr: f64) -> SolarEclipseField {
    SolarEclipseField {
        anchor_ecef_m: Vec3d::ZERO,
        sun_relative_ecef_m: sun,
        moon_relative_ecef_m: moon,
        sun_velocity_ecef_mps: Vec3d::ZERO,
        moon_velocity_ecef_mps: Vec3d::ZERO,
        observer_velocity_ecef_mps: Vec3d::ZERO,
        spin_ecef_radians_per_second: Vec3d::ZERO,
        sun_radius_m: sr,
        moon_radius_m: mr,
    }
}

#[test]
fn topocentric_eclipse_geometry_matches_original_cspice_erfa_and_chord_quadrature() {
    for &(label, geo, ecef, sun, moon, expected) in reference::CASES {
        let position = geodetic_to_ecef(Geodetic::new(geo[0], geo[1], geo[2]).unwrap());
        close(position.distance(vector(ecef)), 0.0, 3e-9);
        let frame = frame(label, position);
        let (actual_sun, actual_moon) = frame.eclipse.apparent_vectors(position).unwrap();
        // Independent ERFA split-date conversion can differ by one 120 ns
        // relative-second ULP; Earth rotation amplifies it to metres at the Sun.
        // The eclipse's angular and fractional comparisons below stay tight.
        close(actual_sun.distance(vector(sun)), 0.0, 2.0);
        close(actual_moon.distance(vector(moon)), 0.0, 0.006);
        // The field at its anchor must also be the actually displayed spheres.
        close(
            (frame.sun.apparent_center_ecef_m - position).distance(actual_sun),
            0.0,
            0.0001,
        );
        close(
            (frame.moon.apparent_center_ecef_m - position).distance(actual_moon),
            0.0,
            0.000001,
        );
        let sample = frame.solar_eclipse(position).unwrap();
        close(sample.sun_angular_radius_rad, expected[0], 1e-12);
        close(sample.moon_angular_radius_rad, expected[1], 1e-12);
        close(sample.separation_rad, expected[2], 2e-11);
        close(sample.obscuration, expected[3], 5e-9);
        close(sample.obscuration + sample.visible_fraction, 1.0, 1e-15);
        assert_eq!(
            frame.quality.earth_rotation,
            EarthRotationQuality::ObservedIers
        );
    }
}

// Independent midpoint integration of vertical chords. This does not use the
// circle-sector angles, Heron radicand or antiderivative used by production.
fn chord_area(radius: f64, separation: f64) -> f64 {
    let left = (-1.0_f64).max(separation - radius);
    let right = 1.0_f64.min(separation + radius);
    if left >= right {
        return 0.0;
    }
    let steps = 200_000;
    let width = (right - left) / f64::from(steps);
    (0..steps)
        .map(|i| {
            let x = left + (f64::from(i) + 0.5) * width;
            let a = (1.0 - x * x).max(0.0).sqrt();
            let b = (radius * radius - (x - separation).powi(2)).max(0.0).sqrt();
            2.0 * a.min(b) * width
        })
        .sum::<f64>()
        / std::f64::consts::PI
}

#[test]
fn overlap_agrees_with_independent_chord_integration_and_area_reciprocity() {
    for ratio in [0.3, 0.9, 0.999, 1.0, 1.001, 1.1, 2.0] {
        for fraction in [0.05, 0.3, 0.7, 0.95] {
            let separation = (1.0_f64 - ratio).abs() + fraction * 2.0 * ratio.min(1.0);
            let expected = chord_area(ratio, separation);
            let (_, actual) = disk_obscuration(0.005, 0.005 * ratio, 0.005 * separation).unwrap();
            close(actual, expected, 3e-8);
            let (_, reversed) = disk_obscuration(0.005 * ratio, 0.005, 0.005 * separation).unwrap();
            close(actual, reversed * ratio * ratio, 2e-13);
            let (_, scaled) = disk_obscuration(0.02, 0.02 * ratio, 0.02 * separation).unwrap();
            close(actual, scaled, 2e-14);
        }
    }
    // Unit circles with centre separation one have an independently known area.
    let expected = (2.0 * std::f64::consts::PI / 3.0 - 3.0_f64.sqrt() / 2.0) / std::f64::consts::PI;
    close(
        disk_obscuration(0.01, 0.01, 0.01).unwrap().1,
        expected,
        1e-14,
    );
}

#[test]
fn contact_branches_are_continuous_bounded_and_monotone_including_equal_disks() {
    assert_eq!(
        disk_obscuration(0.005, 0.005, 0.0).unwrap(),
        (EclipseKind::Total, 1.0)
    );
    for ratio in [0.9, 0.999, 0.999999, 1.0, 1.000001, 1.001, 1.1] {
        let inner = (1.0_f64 - ratio).abs();
        let outer = 1.0 + ratio;
        let mut previous = 1.0;
        let mut separations = vec![0.0, inner];
        for edge in [inner, outer] {
            for delta in [-1e-8, -1e-11, 0.0, 1e-11, 1e-8] {
                separations.push((edge + delta).max(0.0));
            }
        }
        for i in 0..1000 {
            separations.push(outer * f64::from(i) / 999.0);
        }
        separations.sort_by(f64::total_cmp);
        for d in separations {
            let (_, covered) = disk_obscuration(0.005, 0.005 * ratio, 0.005 * d).unwrap();
            assert!((0.0..=1.0).contains(&covered));
            assert!(
                covered <= previous + 2e-10,
                "ratio {ratio}, separation {d}: {covered} > {previous}"
            );
            previous = covered;
        }
        close(previous, 0.0, 1e-14);
    }
}

#[test]
fn invalid_angles_and_observers_reject_instead_of_reporting_full_illumination() {
    for angles in [
        (0.0, 0.005, 0.0),
        (-1.0, 0.005, 0.0),
        (0.005, f64::NAN, 0.0),
        (0.005, 0.005, f64::INFINITY),
        (0.005, 0.005, -1.0),
        (0.005, 0.005, 4.0),
        (2.0, 0.005, 0.0),
    ] {
        assert!(disk_obscuration(angles.0, angles.1, angles.2).is_err());
    }
    assert_eq!(
        disk_obscuration(1e-320, 0.1, 0.01).unwrap(),
        (EclipseKind::Total, 1.0)
    );
    assert_eq!(
        disk_obscuration(1e-320, 0.1, 0.2).unwrap(),
        (EclipseKind::None, 0.0)
    );
    // Rounded input angles cannot resolve the partial contact at this scale.
    assert!(disk_obscuration(1e-320, 0.1, 0.1).is_err());
    let field = static_field(
        Vec3d::new(1e11, 0.0, 0.0),
        Vec3d::new(3e8, 0.0, 0.0),
        SUN_RADIUS_M,
        MOON_RADIUS_M,
    );
    for position in [
        Vec3d::new(f64::NAN, 0.0, 0.0),
        Vec3d::new(f64::INFINITY, 0.0, 0.0),
        Vec3d::new(1.2e9, 0.0, 0.0),
        field.moon_relative_ecef_m,
        field.moon_relative_ecef_m + Vec3d::new(MOON_RADIUS_M * 0.5, 0.0, 0.0),
    ] {
        assert!(field.sample(position).is_err());
    }
    // Unsupported angular geometry must not be reached at a valid far observer.
    let far = field.sample(Vec3d::new(0.0, 1e9, 0.0)).unwrap();
    assert!(far.obscuration.is_finite());
    assert!((0.0..=1.0).contains(&far.obscuration));
    for invalid in [
        SolarEclipseField {
            sun_radius_m: -1.0,
            ..field
        },
        SolarEclipseField {
            moon_radius_m: f64::NAN,
            ..field
        },
        SolarEclipseField {
            moon_velocity_ecef_mps: Vec3d::new(LIGHT_SPEED_MPS, 0.0, 0.0),
            ..field
        },
        SolarEclipseField {
            sun_relative_ecef_m: Vec3d::new(f64::INFINITY, 0.0, 0.0),
            ..field
        },
        SolarEclipseField {
            observer_velocity_ecef_mps: Vec3d::new(0.0, LIGHT_SPEED_MPS * 2.0, 0.0),
            ..field
        },
    ] {
        assert!(invalid.sample(Vec3d::ZERO).is_err());
    }
}

#[test]
fn static_field_is_rotation_translation_invariant_and_respects_occluder_depth() {
    let field = static_field(
        Vec3d::new(1e11, 0.0, 0.0),
        Vec3d::new(3e8, 1e6, 0.0),
        SUN_RADIUS_M,
        MOON_RADIUS_M,
    );
    let position = Vec3d::new(1e6, 2e6, -3e6);
    let expected = field.sample(position).unwrap();
    let rotate = |v: Vec3d| Vec3d::new(v.z, v.x, v.y);
    let translated = Vec3d::new(-2e6, 1e6, 4e6);
    let mut other = field;
    other.anchor_ecef_m = translated;
    other.sun_relative_ecef_m = rotate(field.sun_relative_ecef_m);
    other.moon_relative_ecef_m = rotate(field.moon_relative_ecef_m);
    let actual = other.sample(rotate(position) + translated).unwrap();
    close(actual.obscuration, expected.obscuration, 1e-14);
    close(actual.separation_rad, expected.separation_rad, 1e-14);
    other = field;
    other.moon_relative_ecef_m = Vec3d::new(2e11, 0.0, 0.0);
    other.moon_radius_m = SUN_RADIUS_M * 3.0;
    assert_eq!(other.sample(Vec3d::ZERO).unwrap().kind, EclipseKind::None);
    // Geometric overlap remains a separate fact from the local horizon.
    let mut below = field;
    below.sun_relative_ecef_m = -below.sun_relative_ecef_m;
    below.moon_relative_ecef_m = -below.moon_relative_ecef_m;
    let night = below.sample(Vec3d::new(6_378_137.0, 0.0, 0.0)).unwrap();
    assert!(night.sun_elevation_deg.unwrap() < -89.0);
}

#[test]
fn spatial_light_time_field_matches_fresh_topocentric_samples_across_earth() {
    let label = "2024-04-08 18:40:00";
    for anchor in [
        Geodetic::new(31.72, -97.37, 0.0).unwrap(),
        Geodetic::new(31.72, -97.37, 1e9).unwrap(),
    ] {
        let field = frame(label, geodetic_to_ecef(anchor)).eclipse;
        let mut max_angle = 0.0_f64;
        let mut max_fraction = 0.0_f64;
        for latitude in [-80.0, -45.0, 0.0, 30.0, 60.0, 89.0] {
            for longitude in [-170.0, -100.0, -95.0, 0.0, 100.0] {
                let position = geodetic_to_ecef(Geodetic::new(latitude, longitude, 0.0).unwrap());
                let approximate = field.sample(position).unwrap();
                let fresh = frame(label, position).solar_eclipse(position).unwrap();
                max_angle =
                    max_angle.max((approximate.separation_rad - fresh.separation_rad).abs());
                max_fraction =
                    max_fraction.max((approximate.obscuration - fresh.obscuration).abs());
                close(approximate.separation_rad, fresh.separation_rad, 3e-8);
                close(approximate.obscuration, fresh.obscuration, 5e-6);
            }
        }
        println!(
            "anchor height {} m: max separation error {max_angle:e} rad, max obscuration error {max_fraction:e}",
            anchor.height_m
        );
    }
}

fn central_duration(label: &str, geo: Geodetic, central_kind: EclipseKind) -> f64 {
    let position = geodetic_to_ecef(geo);
    let (ephemeris, eop) = sources();
    let center = instant(label).tt_seconds_j2000();
    let central = |offset| {
        ephemeris
            .sample(
                AstroInstant::from_tt_seconds_j2000(center + offset).unwrap(),
                Some(eop),
                position,
            )
            .unwrap()
            .solar_eclipse(position)
            .unwrap()
            .kind
            == central_kind
    };
    assert!(central(0.0));
    let boundary = |sign| {
        let (mut inside, mut outside) = (0.0, 400.0);
        assert!(!central(sign * outside));
        for _ in 0..30 {
            let middle = (inside + outside) * 0.5;
            if central(sign * middle) {
                inside = middle;
            } else {
                outside = middle;
            }
        }
        (inside + outside) * 0.5
    };
    boundary(-1.0) + boundary(1.0)
}

#[test]
fn historical_central_paths_agree_with_nasa_with_explicit_model_tolerance() {
    // NASA GSFC WGS84 path tables (0.1 arcminute coordinates). Their VSOP87/
    // ELP2000-85 ephemerides, assumed delta-T and distinct penumbral/umbral
    // lunar radii differ from DE440/current IERS/single mean-radius sphere.
    // Five seconds is a comparison tolerance, not certified contact accuracy.
    let total = central_duration(
        "2024-04-08 18:40:00",
        Geodetic::new(31.72, -97.37, 0.0).unwrap(),
        EclipseKind::Total,
    );
    close(total, 263.7, 5.0);
    let annular = central_duration(
        "2023-10-14 16:36:00",
        Geodetic::new(35.55, -107.15166666666667, 0.0).unwrap(),
        EclipseKind::Annular,
    );
    close(annular, 290.6, 5.0);
    println!("NASA central-path comparison: total {total:.6} s, annular {annular:.6} s");
}

#[test]
fn historical_observer_fixtures_classify_at_the_actual_eyes() {
    // Historical validation fixtures only, not application destinations.
    // Coordinates retain their original GeoNames provenance:
    // https://www.geonames.org/4684888/ ; /5454711/ ; /4699066/ .
    // The published representative points are not surveyed observing stations.
    for (calendar, latitude, longitude, expected) in [
        (
            "2024-04-08 18:42:00",
            32.78306,
            -96.80667,
            EclipseKind::Total,
        ),
        (
            "2023-10-14 16:37:00",
            35.08449,
            -106.65114,
            EclipseKind::Annular,
        ),
        (
            "2024-04-08 18:40:00",
            29.76328,
            -95.36327,
            EclipseKind::Partial,
        ),
        (
            "2024-04-09 18:40:00",
            29.76328,
            -95.36327,
            EclipseKind::None,
        ),
    ] {
        let observer = Observer::new(Geodetic::new(latitude, longitude, 0.0).unwrap());
        let position = observer.eye_position();
        let sample = frame(calendar, position).solar_eclipse(position).unwrap();
        assert_eq!(sample.kind, expected, "{calendar}");
        assert!(sample.sun_elevation_deg.unwrap() > 25.0);
    }
}

#[test]
fn shader_overlap_does_not_flash_or_erase_thin_annuli_at_contact() {
    let mut max_error = 0.0_f64;
    for radius in [0.9_f32, 0.95, 0.999, 0.9999, 1.0, 1.0001, 1.001, 1.05, 1.1] {
        let inner = (1.0 - radius).abs();
        let outer = 1.0 + radius;
        let mut points = vec![0.0, inner, outer];
        for edge in [inner, outer] {
            for delta in [-1e-6, -1e-7, -1e-8, 0.0, 1e-8, 1e-7, 1e-6] {
                points.push((edge + delta).max(0.0));
            }
        }
        for i in 0..1000 {
            points.push(outer * i as f32 / 999.0);
        }
        points.sort_by(f32::total_cmp);
        let mut previous = 0.0;
        for distance in points {
            let reference = 1.0
                - disk_obscuration(
                    0.005,
                    0.005 * f64::from(radius),
                    0.005 * f64::from(distance),
                )
                .unwrap()
                .1;
            let actual = gpu_reference::visible_disk(distance, radius);
            let error = (f64::from(actual) - reference).abs();
            max_error = max_error.max(error);
            close(f64::from(actual), reference, 8e-7);
            assert!(actual + 8e-7 >= previous);
            previous = actual;
        }
    }
    println!("f32 normalized-disk sweep max visibility error {max_error:e}");
}

#[test]
fn quantized_shader_field_tracks_fresh_ground_geometry_across_rebases_and_orbits() {
    use earth_math::rendering::LocalFrame;
    for label in ["2024-04-08 18:40:00", "2023-10-14 16:36:00"] {
        for height in [0.0, 8_000_000.0, 1e9] {
            let geo = Geodetic::new(31.72, -97.37, height).unwrap();
            let anchor = geodetic_to_ecef(geo);
            let field = frame(label, anchor).eclipse;
            let mut max_error = 0.0_f64;
            for rebase in [0.0, 15_000.0] {
                let local = LocalFrame {
                    origin: anchor + EnuBasis::at(geo).east * rebase,
                    basis: EnuBasis::at(geo),
                };
                let gpu = field.gpu(local);
                for latitude in [20.0, 29.76328, 31.72, 32.0, 35.55, 40.0, 50.0] {
                    for longitude in [
                        -115.0,
                        -107.15166666666667,
                        -100.0,
                        -97.37,
                        -95.36327,
                        -90.0,
                    ] {
                        let point =
                            geodetic_to_ecef(Geodetic::new(latitude, longitude, 0.0).unwrap());
                        let reference = frame(label, point).solar_eclipse(point).unwrap();
                        let actual = gpu_reference::visibility(gpu, local.position(point));
                        assert!((0.0..=1.0).contains(&actual));
                        let error = (f64::from(actual) - reference.visible_fraction).abs();
                        max_error = max_error.max(error);
                        close(f64::from(actual), reference.visible_fraction, 1e-4);
                    }
                }
            }
            println!(
                "{label}, anchor {height} m: quantized field max fraction error {max_error:e}"
            );
        }
    }
}
