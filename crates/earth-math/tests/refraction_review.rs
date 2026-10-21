//! Independent atmospheric-optics checks. Published metrology values test the
//! implementation of its index formula, not the accuracy of forecast weather.

use earth_math::atmosphere::{AtmosphereLayer, AtmosphereProfile, phase_index};
use earth_math::{Geodetic, Ray, TerrainScene, Vec3d, WGS84_A_M, intersect_wgs84};

#[path = "fixtures/weather_gfs_reference.rs"]
mod gfs_reference;
#[path = "fixtures/refraction_gpu.rs"]
mod gpu_trace;

fn profile(knots: &[(f64, f64)]) -> AtmosphereProfile {
    AtmosphereProfile::from_layers(
        knots
            .iter()
            .map(|&(height_m, refractivity)| AtmosphereLayer {
                height_m,
                refractivity,
            })
            .collect(),
    )
    .unwrap()
}

fn clear_scene() -> TerrainScene {
    TerrainScene::boundary_only(Vec::new()).unwrap()
}

fn simpson(function: impl Fn(f64) -> f64, low: f64, high: f64) -> f64 {
    // A separate radial quadrature oracle; it does not call the ray integrator.
    let intervals = 4096;
    let step = (high - low) / f64::from(intervals);
    let mut sum = function(low) + function(high);
    for i in 1..intervals {
        sum += if i % 2 == 0 { 2.0 } else { 4.0 } * function(low + f64::from(i) * step);
    }
    sum * step / 3.0
}

#[test]
fn published_nist_phase_index_cases() {
    // NIST Stone/Zimmerman, Table 1, 450 µmol/mol CO2:
    // https://emtoolbox.nist.gov/Wavelength/Documentation.asp
    // Every 633 nm dry-air row in the table; expected n was published rounded
    // to nine decimals. No production formula generates these expected values.
    let dry = [
        (20.0, 101_325.0, 1.000_271_800),
        (20.0, 60_000.0, 1.000_160_924),
        (20.0, 120_000.0, 1.000_321_916),
        (50.0, 100_000.0, 1.000_243_285),
        (5.0, 100_000.0, 1.000_282_756),
        (-40.0, 100_000.0, 1.000_337_580),
    ];
    for (celsius, pressure, expected) in dry {
        let actual = phase_index(celsius + 273.15, pressure, 0.0, 633.0).unwrap();
        assert!(
            (actual - expected).abs() <= 5.1e-10,
            "{celsius}C {pressure}Pa: {actual}"
        );
    }

    // Wet Table 1 cases. Table 2 independently publishes saturation pressures
    // rounded to 1 Pa; NIST A16/A17/A19 convert RH to vapor mole fraction.
    // Convert mole fraction to a mass fraction by counting masses of one mole
    // of mixture. The 1e-9 tolerance includes both published roundings.
    let wet = [
        (50.0, 120_000.0, 1.0, 12_351.0, 1.000_287_924),
        (40.0, 120_000.0, 0.75, 7_384.0, 1.000_299_418),
        (20.0, 100_000.0, 1.0, 2_339.0, 1.000_267_394),
    ];
    for (celsius, pressure, rh, saturation, expected) in wet {
        let enhancement = 1.00062 + 3.14e-8 * pressure + 5.6e-7 * celsius * celsius;
        let vapor_mole_fraction = rh * enhancement * saturation / pressure;
        let vapor_mass = vapor_mole_fraction * 0.018015;
        let dry_mass = (1.0 - vapor_mole_fraction) * (0.0289635 + 1.2011e-8 * 50.0);
        let q = vapor_mass / (vapor_mass + dry_mass);
        let actual = phase_index(celsius + 273.15, pressure, q, 633.0).unwrap();
        assert!(
            (actual - expected).abs() < 1e-9,
            "wet {celsius}C {pressure}Pa: {actual}"
        );
    }
}

#[test]
fn phase_index_domain_vacuum_dispersion_and_moisture_semantics() {
    let valid = [293.15, 101_325.0, 0.01, 550.0];
    for field in 0..4 {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut sample = valid;
            sample[field] = value;
            assert!(phase_index(sample[0], sample[1], sample[2], sample[3]).is_err());
        }
    }
    for sample in [
        [99.9, valid[1], valid[2], valid[3]],
        [400.1, valid[1], valid[2], valid[3]],
        [valid[0], -0.01, valid[2], valid[3]],
        [valid[0], 120_001.0, valid[2], valid[3]],
        [valid[0], valid[1], -0.001, valid[3]],
        [valid[0], valid[1], 0.101, valid[3]],
        [valid[0], valid[1], valid[2], 399.9],
        [valid[0], valid[1], valid[2], 700.1],
    ] {
        assert!(phase_index(sample[0], sample[1], sample[2], sample[3]).is_err());
    }
    for temperature in [100.0, 233.15, 293.15, 400.0] {
        for q in [0.0, 0.01, 0.1] {
            assert_eq!(phase_index(temperature, 0.0, q, 550.0).unwrap(), 1.0);
            for pressure in [1e-6, 10_000.0, 120_000.0] {
                let index = phase_index(temperature, pressure, q, 550.0).unwrap();
                assert!(index.is_finite() && index >= 1.0);
            }
        }
    }
    let dry = phase_index(293.15, 100_000.0, 0.0, 550.0).unwrap();
    let humid = phase_index(293.15, 100_000.0, 0.01, 550.0).unwrap();
    // Moisture replaces heavier dry-air molecules at fixed total pressure.
    assert!(humid < dry);
    assert!(phase_index(293.15, 100_000.0, 0.01, 400.0).unwrap() > humid);
    assert!(phase_index(293.15, 100_000.0, 0.01, 700.0).unwrap() < humid);
    assert!(phase_index(293.15, 80_000.0, 0.01, 550.0).unwrap() < humid);
    assert!(phase_index(303.15, 100_000.0, 0.01, 550.0).unwrap() < humid);
}

#[test]
fn radial_ray_integrates_piecewise_linear_index_and_stops_at_actual_top() {
    let atmosphere = profile(&[(0.0, 0.0003), (500.0, 0.0001), (1000.0, 0.0)]);
    let height = 12.3;
    let origin = Vec3d::new(WGS84_A_M + height, 0.0, 0.0);
    let result = atmosphere
        .trace(
            Ray {
                origin,
                direction: Vec3d::new(7.0, 0.0, 0.0),
            },
            &clear_scene(),
        )
        .unwrap();
    assert!(result.surface.is_none());
    let index_start = 1.0003 - height * 0.0002 / 500.0;
    let expected_optical =
        (500.0 - height) * (index_start + 1.0001) / 2.0 + 500.0 * (1.0001 + 1.0) / 2.0;
    assert!(
        (result.geometric_path_m - (1000.0 - height)).abs() < 0.002,
        "geometric={}",
        result.geometric_path_m
    );
    assert!(
        (result.optical_path_m - expected_optical).abs() < 0.002,
        "optical={} expected={expected_optical}",
        result.optical_path_m
    );
    assert!(
        result
            .exit_ray
            .origin
            .distance(Vec3d::new(WGS84_A_M + 1000.0, 0.0, 0.0))
            < 0.002
    );
    assert!(
        result
            .exit_ray
            .direction
            .distance(Vec3d::new(1.0, 0.0, 0.0))
            < 1e-12
    );
}

#[test]
fn isolated_thin_layers_cannot_disappear_between_adaptive_stage_samples() {
    // A radial ray cannot bend. The excess optical path is the exact area
    // under n−1: a triangle of base200m and height0.001 gives0.1m.
    // A single2km midpoint step, even compared with two half steps, samples
    // neither slope of this bump. Knot-aware event limits must find it.
    for offset in [100.0, 437.25, 1100.0] {
        let atmosphere = profile(&[
            (0.0, 0.0),
            (offset, 0.0),
            (offset + 100.0, 0.001),
            (offset + 200.0, 0.0),
            (10000.0, 0.0),
        ]);
        for (height, sign) in [(12.3, 1.0), (9990.0, -1.0)] {
            let ray = Ray {
                origin: Vec3d::new(WGS84_A_M + height, 0.0, 0.0),
                direction: Vec3d::new(sign, 0.0, 0.0),
            };
            let trace = atmosphere.trace(ray, &clear_scene()).unwrap();
            assert!((trace.optical_path_m - trace.geometric_path_m - 0.1).abs() < 0.002);
            assert_eq!(trace.surface.is_some(), sign < 0.0);
        }
    }
    let atmosphere = profile(&[(0.0, 0.0003), (1000.0, 0.0)]);
    assert!(atmosphere.index_gradient(f64::NAN).0.is_nan());
    let (n, slope) = atmosphere.index_gradient(-0.001);
    assert!((n - (1.0003 - slope * 0.001)).abs() < 1e-15);
}

#[test]
fn constant_index_surface_paths_equal_straight_ellipsoid_rays() {
    let atmosphere = profile(&[(0.0, 0.0003), (20_000.0, 0.0003), (30_000.0, 0.0)]);
    let scene = clear_scene();
    for (lat, lon) in [(0.0, 0.0), (43.0, -78.0), (90.0, 17.0), (-89.0, 179.0)] {
        let location = Geodetic::new(lat, lon, 100.0).unwrap();
        let frame = earth_math::CameraFrame::new(location, 36.0, -18.0);
        let ray = Ray {
            origin: frame.origin,
            direction: frame.forward * 3.7,
        };
        let expected = intersect_wgs84(ray).unwrap();
        let actual = atmosphere.trace(ray, &scene).unwrap();
        let surface = actual.surface.unwrap().hit;
        assert!(surface.position.distance(expected.position) < 0.003);
        assert!((actual.geometric_path_m - expected.distance_m).abs() < 0.003);
        assert!((actual.optical_path_m - expected.distance_m * 1.0003).abs() < 0.003);
    }
}

#[test]
fn equatorial_bending_matches_independent_snell_invariant_quadrature() {
    // In z=0, WGS84 geodetic height is exactly r−a, so n depends on r only.
    // Rotational symmetry conserves b=n*r*cos(elevation). Independent radial
    // quadrature then gives path length, optical length and polar sweep.
    let atmosphere = profile(&[(0.0, 0.0003), (20_000.0, 0.0)]);
    let surface_radius = WGS84_A_M;
    let eye_radius = surface_radius + 1000.0;
    let n = |radius: f64| 1.0003 - 0.0003 * (radius - surface_radius) / 20_000.0;
    for degrees in [2.0_f64, 10.0, 35.0] {
        let angle = degrees.to_radians();
        let impact = n(eye_radius) * eye_radius * angle.cos();
        let root = |radius: f64| {
            let nr = n(radius) * radius;
            ((nr - impact) * (nr + impact)).sqrt()
        };
        let distance = simpson(|r| n(r) * r / root(r), surface_radius, eye_radius);
        let optical = simpson(|r| n(r).powi(2) * r / root(r), surface_radius, eye_radius);
        let sweep = simpson(|r| impact / (r * root(r)), surface_radius, eye_radius);
        let expected = Vec3d::new(
            surface_radius * sweep.cos(),
            surface_radius * sweep.sin(),
            0.0,
        );
        let actual = atmosphere
            .trace(
                Ray {
                    origin: Vec3d::new(eye_radius, 0.0, 0.0),
                    direction: Vec3d::new(-angle.sin(), angle.cos(), 0.0),
                },
                &clear_scene(),
            )
            .unwrap();
        let hit = actual.surface.unwrap().hit;
        assert!(
            hit.position.distance(expected) < 0.03,
            "{degrees}deg position error={}m",
            hit.position.distance(expected)
        );
        assert!((actual.geometric_path_m - distance).abs() < 0.03);
        assert!((actual.optical_path_m - optical).abs() < 0.03);
        let preserved = 1.0003 * hit.position.cross(actual.exit_ray.direction).length();
        assert!(
            (preserved - impact).abs() < 0.02,
            "{degrees}deg invariant delta={}",
            preserved - impact
        );
        assert!((actual.exit_ray.direction.length() - 1.0).abs() < 1e-12);
    }
}

#[test]
fn real_noaa_column_matches_independent_gdal_values_and_registration() {
    use earth_math::atmosphere::weather::{decode_gfs, request_candidates};
    use gfs_reference::*;
    let request = request_candidates(29.28, -94.81, 1_792_609_200.0)
        .unwrap()
        .remove(0);
    assert_eq!(
        (request.latitude_deg, request.longitude_deg),
        (29.25, -94.75)
    );
    assert_eq!(request.cycle_unix_seconds, 1_792_584_000);
    assert_eq!(request.valid_unix_seconds, 1_792_609_200);
    assert_eq!(request.forecast_hour, 7);
    let decoded = decode_gfs(
        include_bytes!("../../../assets/weather/gfs-reference.grib2"),
        &request,
    )
    .unwrap();
    let metadata = &decoded.metadata;
    assert!((metadata.temperature_2m_k - TEMPERATURE_2M_K).abs() < 0.00004);
    assert!((metadata.pressure_surface_pa - SURFACE_PRESSURE_PA).abs() < 0.01);
    assert!((metadata.specific_humidity_2m - SPECIFIC_HUMIDITY_2M).abs() < 2e-9);
    assert!((metadata.provider_surface_geopotential_m - SURFACE_GEOPOTENTIAL_M).abs() < 1e-6);
    assert!(metadata.extrapolated_refractivity); // Upper levels are outside lab validation.
    assert_eq!(
        metadata.surface_conditions_label(earth_math::UnitSystem::Metric),
        "2m air: 23.6°C; surface: 1019.8 hPa; specific humidity: 14.23 g/kg"
    );
    assert_eq!(
        metadata.surface_conditions_label(earth_math::UnitSystem::Imperial),
        "2m air: 74.5°F; surface: 1019.8 hPa; specific humidity: 14.23 g/kg"
    );
    assert_eq!(decoded.profile.layers().len(), PRESSURE_LEVELS.len() + 3);
    let radius = 6_356_766.0;
    let ground = radius * SURFACE_GEOPOTENTIAL_M / (radius - SURFACE_GEOPOTENTIAL_M);
    for (&(pressure, geopotential, temperature, humidity), actual) in
        PRESSURE_LEVELS.iter().zip(&decoded.profile.layers()[2..])
    {
        let expected_height = radius * geopotential / (radius - geopotential) - ground;
        let expected_index = phase_index(temperature, pressure, humidity, 550.0).unwrap();
        assert!(
            (actual.height_m - expected_height).abs() < 0.012,
            "pressure={pressure} height"
        );
        // This comparison isolates independently decoded source samples and
        // mapping. Published NIST tests above independently check phase_index.
        assert!(
            (actual.refractivity - (expected_index - 1.0)).abs() < 1e-10,
            "pressure={pressure} index"
        );
    }
    assert!(metadata.usable_at(
        Geodetic::new(29.28, -94.81, 1000.0).unwrap(),
        1_792_609_200.0
    ));
    assert!(!metadata.usable_at(Geodetic::new(29.8, -94.81, 0.0).unwrap(), 1_792_609_200.0));
    assert!(!metadata.usable_at(
        Geodetic::new(29.28, -94.81, 0.0).unwrap(),
        1_792_609_200.0 + 10801.0
    ));
}

#[test]
fn weather_planning_wraps_grid_and_rejects_wrong_source_identity() {
    use earth_math::atmosphere::weather::{decode_gfs, request_candidates};
    let now = 1_792_609_200.0;
    for latitude in [-90.0, -89.99, 0.0, 89.99, 90.0] {
        for longitude in [-180.0, -0.01, 0.0, 179.99, 180.0] {
            let requests = request_candidates(latitude, longitude, now).unwrap();
            assert_eq!(requests.len(), 3);
            for request in requests {
                assert!((-90.0..=90.0).contains(&request.latitude_deg));
                assert!((-180.0..=180.0).contains(&request.longitude_deg));
                assert_eq!(
                    request.valid_unix_seconds,
                    request.cycle_unix_seconds + i64::from(request.forecast_hour) * 3600
                );
                assert!(request.matches_location(Geodetic::new(latitude, longitude, 0.0).unwrap()));
            }
        }
    }
    assert!(request_candidates(f64::NAN, 0.0, now).is_err());
    assert!(request_candidates(0.0, 181.0, now).is_err());
    assert!(request_candidates(0.0, 0.0, f64::INFINITY).is_err());
    let request = request_candidates(29.28, -94.81, now).unwrap().remove(0);
    let bytes = include_bytes!("../../../assets/weather/gfs-reference.grib2");
    let mut wrong_cycle = request.clone();
    wrong_cycle.cycle_unix_seconds -= 21600;
    assert!(decode_gfs(bytes, &wrong_cycle).is_err());
    let mut wrong_time = request.clone();
    wrong_time.valid_unix_seconds += 3600;
    assert!(decode_gfs(bytes, &wrong_time).is_err());
    let mut absent_column = request.clone();
    absent_column.latitude_deg = 45.0;
    assert!(decode_gfs(bytes, &absent_column).is_err());
    for length in [0, 15, bytes.len() / 2, bytes.len() - 1] {
        assert!(
            decode_gfs(&bytes[..length], &request).is_err(),
            "accepted truncation {length}"
        );
    }
}

#[test]
fn profile_and_ray_domains_fail_closed_and_vacuum_handles_far_entry() {
    for knots in [
        vec![],
        vec![(0.0, 0.0)],
        vec![(1.0, 0.0003), (1000.0, 0.0)],
        vec![(0.0, 0.0003), (1000.0, 0.0001)],
        vec![(0.0, f64::NAN), (1000.0, 0.0)],
        vec![(0.0, 0.0003), (0.0, 0.0)],
        vec![(0.0, 0.0021), (1000.0, 0.0)],
        vec![(0.0, 0.0), (100.0, -0.0001), (1000.0, 0.0)],
    ] {
        let layers = knots
            .into_iter()
            .map(|(height_m, refractivity)| AtmosphereLayer {
                height_m,
                refractivity,
            })
            .collect();
        assert!(AtmosphereProfile::from_layers(layers).is_err());
    }
    let vacuum = profile(&[(0.0, 0.0), (120_000.0, 0.0)]);
    let scene = clear_scene();
    for origin in [
        Vec3d::ZERO,
        Vec3d::new(f64::NAN, 0.0, 0.0),
        Vec3d::new(WGS84_A_M - 1.0, 0.0, 0.0),
    ] {
        assert!(
            vacuum
                .trace(
                    Ray {
                        origin,
                        direction: Vec3d::new(1.0, 0.0, 0.0)
                    },
                    &scene
                )
                .is_err()
        );
    }
    assert!(
        vacuum
            .trace(
                Ray {
                    origin: Vec3d::new(WGS84_A_M + 100.0, 0.0, 0.0),
                    direction: Vec3d::ZERO
                },
                &scene
            )
            .is_err()
    );
    for height in [200_000.0, 8_000_000.0, 1_000_000_000.0] {
        for latitude in [0.0, 37.0, 90.0] {
            let eye = Geodetic::new(latitude, 19.0, height).unwrap();
            let frame = earth_math::CameraFrame::new(eye, 0.0, -90.0);
            let expected = intersect_wgs84(frame.center_ray()).unwrap();
            let actual = vacuum.trace(frame.center_ray(), &scene).unwrap();
            assert!(
                actual
                    .surface
                    .unwrap()
                    .hit
                    .position
                    .distance(expected.position)
                    < 0.005
            );
            assert!((actual.geometric_path_m - expected.distance_m).abs() < 0.005);
            assert!((actual.optical_path_m - actual.geometric_path_m).abs() < 1e-6);
        }
    }
}

#[test]
fn turning_ray_can_avoid_ground_and_plane_hit_matches_radial_oracle() {
    let inverted = profile(&[(0.0, 0.0001), (100.0, 0.0011), (120_000.0, 0.0)]);
    let angle = 0.1_f64.to_radians();
    let ray = Ray {
        origin: Vec3d::new(WGS84_A_M + 2.0, 0.0, 0.0),
        direction: Vec3d::new(-angle.sin(), angle.cos(), 0.0),
    };
    assert!(intersect_wgs84(ray).is_some());
    // n(r)*r increases strictly in the lower100m, and b>n(a)*a.
    // Conservation of impact parameter therefore forbids reaching the ground.
    let impact = 1.00012 * (WGS84_A_M + 2.0) * angle.cos();
    assert!(impact > 1.0001 * WGS84_A_M);
    let turned = inverted.trace(ray, &clear_scene()).unwrap();
    assert!(turned.surface.is_none());
    assert!(
        earth_math::ecef_to_geodetic(turned.exit_ray.origin)
            .unwrap()
            .height_m
            > 119_999.99
    );
    assert!(turned.optical_path_m > turned.geometric_path_m);

    let atmosphere = profile(&[(0.0, 0.0003), (20_000.0, 0.0)]);
    let r0 = WGS84_A_M + 1000.0;
    let angle = 10.0_f64.to_radians();
    let n = |r: f64| 1.0003 - 0.0003 * (r - WGS84_A_M) / 20_000.0;
    let impact = n(r0) * r0 * angle.cos();
    let sweep = |r| {
        simpson(
            |s| {
                let nr = n(s) * s;
                impact / (s * ((nr - impact) * (nr + impact)).sqrt())
            },
            r,
            r0,
        )
    };
    let radius = WGS84_A_M + 0.0005; // Plane just before the ground event.
    let phi = sweep(radius);
    let expected = Vec3d::new(radius * phi.cos(), radius * phi.sin(), 0.0);
    let actual = atmosphere
        .trace_to_plane(
            Ray {
                origin: Vec3d::new(r0, 0.0, 0.0),
                direction: Vec3d::new(-angle.sin(), angle.cos(), 0.0),
            },
            &clear_scene(),
            expected,
            Vec3d::new(0.0, 1.0, 0.0),
        )
        .unwrap()
        .unwrap();
    assert!(
        actual.distance(expected) < 0.03,
        "plane error={}m",
        actual.distance(expected)
    );
    assert!((actual.y - expected.y).abs() < 1e-5);
}

#[test]
fn refracted_guides_keep_constant_index_geometry_and_unresolved_rays_unavailable() {
    use earth_math::{
        CameraFrame, Lens, Projection,
        guides::{GuideViewport, HorizonProfile},
    };
    let scene = clear_scene();
    let vacuum = profile(&[(0.0, 0.0), (120_000.0, 0.0)]);
    let frame = CameraFrame::new(Geodetic::new(0.0, 0.0, 1000.0).unwrap(), 90.0, -1.0);
    let viewport = GuideViewport::new([640.0, 360.0], [1.5, 1.5]).unwrap();
    for lens in [
        Lens::default(),
        Lens::new(Projection::EquidistantFisheye, 140.0),
    ] {
        let straight = HorizonProfile::measure(&scene, frame, lens, viewport).unwrap();
        let optical =
            HorizonProfile::measure_refracted(&scene, frame, lens, viewport, &vacuum).unwrap();
        assert!((straight.apex.pixel[1] - optical.apex.pixel[1]).abs() < 0.03);
        for endpoint in [optical.apex, optical.left, optical.right] {
            assert!(endpoint.bracket_pixels <= 0.025);
            assert!(
                earth_math::ecef_to_geodetic(endpoint.hit.hit.position)
                    .unwrap()
                    .height_m
                    .abs()
                    < 1e-7
            );
        }
        let ruler = optical.ruler_refracted(&scene, 0.25, &vacuum).unwrap();
        let default_ruler = optical.ruler(&scene, 0.25).unwrap();
        assert!(
            default_ruler
                .reference_ecef_m
                .distance(ruler.reference_ecef_m)
                < 1e-9
        );
        let differing = profile(&[(0.0, 0.0003), (120000.0, 0.0)]);
        assert!(optical.ruler_refracted(&scene, 0.25, &differing).is_none());
        assert!(
            (ruler.reference_ecef_m - ruler.ground.hit.hit.position)
                .dot(frame.forward)
                .abs()
                < 1e-5
        );
        assert!(
            (ruler
                .reference_ecef_m
                .distance(ruler.ground.hit.hit.position)
                - ruler.view_plane_gap_m)
                .abs()
                < 1e-8
        );
        let rect = optical.horizontal_rect();
        assert!(((rect.max[1] - rect.min[1]) * 1.5 - 1.0).abs() < 5e-5);
    }
    // The optical solver intentionally rejects legacy terrain scenes; the
    // contour must not reinterpret these unresolved samples as ordinary sky.
    assert!(
        HorizonProfile::measure_refracted(
            &TerrainScene::new(Vec::new()),
            frame,
            Lens::default(),
            viewport,
            &vacuum
        )
        .is_none()
    );
}

#[test]
fn optical_skyline_matches_equatorial_tangent_and_celestial_path_composes_at_exit() {
    use earth_math::{
        Observer,
        astro::{AstroInstant, CalendarDateTime, CelestialKind, Ephemeris},
    };
    let scene = clear_scene();
    let vacuum = profile(&[(0.0, 0.0), (120000.0, 0.0)]);
    let mut observer = Observer::new(Geodetic::new(0.0, 0.0, 1000.0).unwrap());
    observer.heading_deg = 90.0;
    let skyline = vacuum.skyline(&scene, observer).unwrap();
    let radius = observer.eye_geodetic().height_m + WGS84_A_M;
    let expected_angle = -(WGS84_A_M / radius).acos().to_degrees();
    assert!((skyline.elevation_deg - expected_angle).abs() < skyline.angular_bracket_deg * 1.1);
    assert!(skyline.angular_bracket_deg <= 5.0 / 2.0_f64.powi(22));
    // Near tangency, a sub-microdegree angular bracket does NOT imply a
    // millimetre range bound. The hit must remain on the actual ellipsoid.
    assert!(
        earth_math::ecef_to_geodetic(skyline.hit.hit.position)
            .unwrap()
            .height_m
            .abs()
            < 1e-7
    );
    assert!(
        vacuum
            .skyline(&TerrainScene::new(Vec::new()), observer)
            .is_none()
    );

    let atmosphere = profile(&[(0.0, 0.0003), (1000.0, 0.0)]);
    let origin = Vec3d::new(WGS84_A_M + 12.3, 0.0, 0.0);
    let trace = atmosphere
        .trace(
            Ray {
                origin,
                direction: Vec3d::new(1.0, 0.0, 0.0),
            },
            &scene,
        )
        .unwrap();
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/astronomy/de440s.bsp");
    let ephemeris = Ephemeris::from_bytes(&std::fs::read(path).unwrap()).unwrap();
    let instant =
        AstroInstant::from_calendar(CalendarDateTime::parse("2000-01-01 12:00:00").unwrap())
            .unwrap();
    let mut frame = ephemeris.sample(instant, None, origin).unwrap();
    frame.sun.apparent_center_ecef_m = trace.exit_ray.origin + Vec3d::new(10000.0, 0.0, 0.0);
    frame.sun.radius_m = 100.0;
    frame.moon.apparent_center_ecef_m = trace.exit_ray.origin + Vec3d::new(5000.0, 1000.0, 0.0);
    frame.moon.radius_m = 1.0;
    let hit = trace.nearest_celestial(&frame).unwrap();
    assert_eq!(hit.hit.body, CelestialKind::Sun);
    assert!((hit.geometric_path_m - trace.geometric_path_m - 9900.0).abs() < 1e-6);
    assert!((hit.optical_path_m - trace.optical_path_m - 9900.0).abs() < 1e-6);
    assert!((hit.chord_m - origin.distance(hit.hit.position_ecef_m)).abs() < 1e-9);
    let ground = atmosphere
        .trace(
            Ray {
                origin,
                direction: Vec3d::new(-1.0, 0.0, 0.0),
            },
            &scene,
        )
        .unwrap();
    assert!(ground.nearest_celestial(&frame).is_none());
    // A foreground body on the initial vacuum segment must win even when
    // Earth terminates the later atmospheric portion of this same ray.
    let space_origin = Vec3d::new(WGS84_A_M + 1_000_000.0, 0.0, 0.0);
    let inbound = atmosphere
        .trace(
            Ray {
                origin: space_origin,
                direction: Vec3d::new(-1.0, 0.0, 0.0),
            },
            &scene,
        )
        .unwrap();
    assert!(inbound.surface.is_some());
    frame.moon.apparent_center_ecef_m = space_origin - Vec3d::new(500000.0, 0.0, 0.0);
    frame.moon.radius_m = 100.0;
    frame.sun.apparent_center_ecef_m = Vec3d::new(-1e9, 0.0, 0.0);
    let foreground = inbound.nearest_celestial(&frame).unwrap();
    assert_eq!(foreground.hit.body, CelestialKind::Moon);
    assert!((foreground.geometric_path_m - 499900.0).abs() < 1e-6);
    assert_eq!(foreground.geometric_path_m, foreground.optical_path_m);
    assert_eq!(foreground.geometric_path_m, foreground.chord_m);
    // Native curved-celestial diagnostic: a source-ephemeris pose, rather
    // than an additional independent ephemeris accuracy fixture.
    let diagnostic = earth_math::atmosphere::weather::reference_observer();
    let eop_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/astronomy/finals2000A.all");
    let eop =
        earth_math::astro::EarthOrientationData::from_bytes(&std::fs::read(eop_path).unwrap())
            .unwrap();
    let moon_time =
        AstroInstant::from_calendar(CalendarDateTime::parse("2026-10-21 02:00:00").unwrap())
            .unwrap();
    let sky = ephemeris
        .sample(moon_time, Some(&eop), diagnostic.frame().origin)
        .unwrap();
    let direction = (sky.moon.apparent_center_ecef_m - diagnostic.frame().origin)
        .normalized()
        .unwrap();
    let elevation = direction
        .dot(earth_math::EnuBasis::at(diagnostic.eye_geodetic()).up)
        .asin()
        .to_degrees();
    assert!(elevation > 10.0);
    eprintln!(
        "reference optical diagnostic Moon2026-10-21 02UTC apparent geometric elevation={elevation:.9}deg"
    );
}

#[test]
fn gpu_ellipsoid_height_normals_and_relative_angles_have_explicit_finite_case_limits() {
    use earth_math::{
        EnuBasis, Observer, atmosphere::gpu::GpuEllipsoid, geodetic_to_ecef, rendering::LocalFrame,
    };
    // These limits include both coefficient and local-position f32 rounding.
    // They are regression tolerances for this finite grid, not universal bounds
    // on the accumulated curved ray or on geographical source accuracy.
    for (eye_height, height_tolerance) in [
        (1.72_f64, 0.1),
        (1000.0, 0.1),
        (100_000.0, 0.1),
        (1_000_000.0, 0.4),
        (8_000_000.0, 3.0),
        (10_000_000.0, 3.5),
    ] {
        for latitude in [0.0_f64, 29.28, 67.0, 89.9] {
            let observer =
                Observer::new(Geodetic::new(latitude, -94.81, eye_height - 1.72).unwrap());
            let local = LocalFrame::at(observer);
            let gpu = GpuEllipsoid::at(observer);
            for dlat in [-15.0, -3.0, 0.0, 3.0, 15.0] {
                for dlon in [-15.0, -3.0, 0.0, 3.0, 15.0] {
                    for height in [0.0, 2.0, 1000.0, 120_000.0] {
                        let target = Geodetic::new(
                            (latitude + dlat).clamp(-89.99, 89.99),
                            -94.81 + dlon,
                            height,
                        )
                        .unwrap();
                        let (actual_height, actual_normal) =
                            gpu.height_normal(local.position(geodetic_to_ecef(target)));
                        assert!((f64::from(actual_height) - height).abs() < height_tolerance);
                        let expected_normal = local.direction(EnuBasis::at(target).up);
                        let norm_error = actual_normal
                            .into_iter()
                            .zip(expected_normal)
                            .map(|(a, b)| f64::from(a - b).powi(2))
                            .sum::<f64>()
                            .sqrt();
                        assert!(norm_error < 2e-7);
                        let angles = gpu.relative_angles(actual_normal);
                        let latitude_error =
                            f64::from(angles[0]) - (target.lat_deg - latitude).to_radians();
                        let longitude_error = (f64::from(angles[1]) - dlon.to_radians())
                            * target.lat_deg.to_radians().cos();
                        assert!(latitude_error.hypot(longitude_error) < 3e-7);
                    }
                }
            }
        }
    }
}

#[test]
fn scalar_gpu_curved_rays_match_cpu_on_explicit_weather_cases_or_report_unresolved() {
    use earth_math::{
        CameraFrame, Observer,
        atmosphere::weather::{decode_gfs, request_candidates},
        rendering::LocalFrame,
    };
    let request = request_candidates(29.28, -94.81, 1792609200.0)
        .unwrap()
        .remove(0);
    let weather = decode_gfs(
        include_bytes!("../../../assets/weather/gfs-reference.grib2"),
        &request,
    )
    .unwrap();
    let scene = clear_scene();
    let mut maximum_ground = 0.0_f64;
    let mut maximum_path = 0.0_f64;
    let mut maximum_direction = 0.0_f64;
    let mut resolved = 0;
    let mut unresolved = 0;
    for height in [0.0, 98.28, 998.28] {
        let observer = Observer::new(Geodetic::new(29.28, -94.81, height).unwrap());
        let local = LocalFrame::at(observer);
        let gpu = gpu_trace::Model::new(observer, &weather.profile);
        for heading in [0.0, 90.0, 237.0] {
            for pitch in [-90.0, -15.0, -1.0, 0.0, 10.0, 90.0] {
                let frame = CameraFrame::new(observer.eye_geodetic(), heading, pitch);
                let cpu = weather.profile.trace(frame.center_ray(), &scene).unwrap();
                let display = gpu.trace(local.direction(frame.forward));
                if display.status == 2 {
                    unresolved += 1;
                    continue;
                }
                resolved += 1;
                assert_eq!(
                    display.status == 1,
                    cpu.surface.is_some(),
                    "height={height} heading={heading} pitch={pitch}"
                );
                let expected = local.position(cpu.exit_ray.origin);
                let error = display
                    .position
                    .into_iter()
                    .zip(expected)
                    .map(|(a, b)| f64::from(a - b).powi(2))
                    .sum::<f64>()
                    .sqrt();
                let expected_direction = local.direction(cpu.exit_ray.direction);
                let direction_error = display
                    .direction
                    .into_iter()
                    .zip(expected_direction)
                    .map(|(a, b)| f64::from(a - b).powi(2))
                    .sum::<f64>()
                    .sqrt();
                maximum_direction = maximum_direction.max(direction_error);
                maximum_path =
                    maximum_path.max((f64::from(display.path) - cpu.geometric_path_m).abs());
                if display.status == 1 {
                    maximum_ground = maximum_ground.max(error);
                }
                assert!(
                    error < 10.0,
                    "position error={error} height={height} heading={heading} pitch={pitch}"
                );
                assert!(direction_error < 4e-6);
            }
        }
    }
    assert_eq!((resolved, unresolved), (54, 0));
    assert!(maximum_ground < 0.5);
    assert!(maximum_path < 10.0);
    eprintln!(
        "scalar GPU weather: resolved={resolved} unresolved={unresolved}; max ground displacement={maximum_ground:.9}m; path difference={maximum_path:.9}m; direction norm difference={maximum_direction:.9}"
    );
    let mut observer = Observer::new(Geodetic::new(29.25, -94.75, 6.0).unwrap());
    observer.heading_deg = 90.0;
    let curved = weather.profile.skyline(&scene, observer).unwrap();
    let straight = scene.skyline(observer).unwrap();
    let diagnostic = earth_math::atmosphere::weather::reference_observer();
    let diagnostic_lens = earth_math::atmosphere::weather::reference_lens();
    assert!((diagnostic.eye_geodetic().height_m - 7.72).abs() < 1e-12);
    assert_eq!(
        diagnostic_lens.projection,
        earth_math::Projection::Rectilinear
    );
    for elevation in [curved.elevation_deg, straight.elevation_deg] {
        assert!((elevation - diagnostic.pitch_deg).abs() < diagnostic_lens.vertical_fov_deg * 0.5);
    }
    eprintln!(
        "reference NOAA skyline at29.25,-94.75 feet6m east: curved path={:.9}m elevation={:.12}deg; straight range={:.9}m elevation={:.12}deg",
        curved.geometric_path_m,
        curved.elevation_deg,
        straight.hit.hit.distance_m,
        straight.elevation_deg
    );
    let mut elevated_max_position = 0.0_f64;
    let mut elevated_max_path = 0.0_f64;
    for height in [10000.0, 20000.0] {
        for heading in [0.0, 90.0, 237.0] {
            let mut high = Observer::new(Geodetic::new(29.25, -94.75, height - 1.72).unwrap());
            high.heading_deg = heading;
            assert!(earth_math::atmosphere::gpu::display_supported(high));
            let skyline = weather.profile.skyline(&scene, high).unwrap();
            let local = LocalFrame::at(high);
            let gpu = gpu_trace::Model::new(high, &weather.profile);
            for pitch in [
                skyline.elevation_deg - 0.01,
                skyline.elevation_deg + 0.01,
                -90.0,
            ] {
                let ray = CameraFrame::new(high.eye_geodetic(), heading, pitch).center_ray();
                let cpu = weather.profile.trace(ray, &scene).unwrap();
                let display = gpu.trace(local.direction(ray.direction));
                assert_ne!(
                    display.status, 2,
                    "unresolved height{height} heading{heading} pitch{pitch}"
                );
                assert_eq!(display.status == 1, cpu.surface.is_some());
                let expected = local.position(cpu.exit_ray.origin);
                let error = display
                    .position
                    .into_iter()
                    .zip(expected)
                    .map(|(a, b)| f64::from(a - b).powi(2))
                    .sum::<f64>()
                    .sqrt();
                elevated_max_position = elevated_max_position.max(error);
                elevated_max_path =
                    elevated_max_path.max((f64::from(display.path) - cpu.geometric_path_m).abs());
                assert!(error < 25.0, "positionerror{error}");
            }
        }
    }
    eprintln!(
        "18 scalarGPU elevated NOAA rays10/20km: maxendpointdifference={elevated_max_position:.9}m; maxpathdifference={elevated_max_path:.9}m"
    );
    let just_above = Observer::new(Geodetic::new(29.25, -94.75, 20000.001 - 1.72).unwrap());
    assert!(elevated_max_path < 40.0);
    assert!(!earth_math::atmosphere::gpu::display_supported(just_above));
    assert!(!earth_math::atmosphere::gpu::display_supported(
        Observer::new(Geodetic::new(29.25, -94.75, 20000.0).unwrap())
    ));
    // Far-above-budget observers must produce a diagnostic, not a false sky.
    let far = Observer::new(Geodetic::new(29.28, -94.81, 10000001.0).unwrap());
    assert_eq!(
        gpu_trace::Model::new(far, &weather.profile)
            .trace([0.0, -1.0, 0.0])
            .status,
        2
    );
}
