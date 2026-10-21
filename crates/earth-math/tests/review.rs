//! Independent review fixtures and invariants.
//!
//! These tests intentionally combine published external fixtures, algebraic
//! closed forms, and operational invariants. Round trips alone cannot reveal
//! matching forward/inverse mistakes.

use earth_math::{
    CameraFrame, EnuBasis, Geodetic, Lens, MAX_HEIGHT_M, MIN_HEIGHT_M, Observer, Projection, Ray,
    STATURE_M, UnitSystem, Vec3d, WGS84_A_M, WGS84_B_M, ecef_to_geodetic, geodetic_to_ecef,
    intersect_wgs84,
};

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        actual.is_finite() && (actual - expected).abs() <= tolerance,
        "{actual:.17e} != {expected:.17e}; error {:.17e}, tolerance {tolerance:.17e}",
        (actual - expected).abs()
    );
}

fn vector_close(actual: Vec3d, expected: Vec3d, tolerance: f64) {
    close(actual.x, expected.x, tolerance);
    close(actual.y, expected.y, tolerance);
    close(actual.z, expected.z, tolerance);
}

fn geo(latitude: f64, longitude: f64, height: f64) -> Geodetic {
    Geodetic::new(latitude, longitude, height).unwrap()
}

#[test]
fn aiming_at_an_ecef_target_changes_only_attitude_and_rejects_undefined_directions() {
    for location in [
        geo(0.0, 0.0, 0.0),
        geo(47.5, -122.3, 1234.5),
        geo(-73.0, 179.9, 100_000.0),
        geo(90.0, 38.0, 100.0),
    ] {
        let mut observer = Observer::new(location);
        observer.heading_deg = 123.0;
        observer.pitch_deg = -17.0;
        observer.speed_mps = 91.0;
        observer.set_eye_height(0.93);
        let initial = observer;
        for target in [
            Vec3d::new(125_000_000.0, -310_000_000.0, 20_000_000.0),
            Vec3d::new(-90_000_000_000.0, 70_000_000_000.0, -40_000_000_000.0),
        ] {
            let expected = (target - observer.eye_position()).normalized().unwrap();
            observer.aim_at(target).unwrap();
            vector_close(observer.frame().forward, expected, 3e-15);
            assert_eq!(observer.feet, initial.feet);
            assert_eq!(observer.eye_position(), initial.eye_position());
            assert_eq!(observer.eye_height_m, initial.eye_height_m);
            assert_eq!(observer.speed_mps, initial.speed_mps);
            assert_eq!(
                observer.maximum_feet_height_m(),
                initial.maximum_feet_height_m()
            );
            assert!((0.0..360.0).contains(&observer.heading_deg));
        }
        // Long offsets keep subtraction roundoff below the helper's angular
        // degeneracy threshold, including at an oblique geodetic latitude.
        let up = EnuBasis::at(observer.feet).up;
        for sign in [-1.0, 1.0] {
            observer.heading_deg = 123.0;
            let target = observer.eye_position() + up * (sign * 100_000_000.0);
            observer.aim_at(target).unwrap();
            assert_eq!(observer.heading_deg, 123.0);
            close(observer.pitch_deg, sign * 90.0, 3e-13);
            vector_close(observer.frame().forward, up * sign, 3e-15);
        }
        for invalid in [
            observer.eye_position(),
            Vec3d::new(f64::NAN, 0.0, 0.0),
            Vec3d::new(0.0, f64::INFINITY, 0.0),
            Vec3d::new(0.0, 0.0, f64::NEG_INFINITY),
        ] {
            let before = observer;
            assert_eq!(
                observer.aim_at(invalid),
                Err(earth_math::GeometryError::InvalidDirection)
            );
            assert_eq!(observer, before);
        }
    }
}

#[test]
fn published_iogp_epsg_geocentric_fixture() {
    // IOGP Guidance Note 7 Part 2 (September 2019), section 4.1.1, p. 96.
    // https://www.iogp.org/wp-content/uploads/2019/09/373-07-02.pdf
    // Published coordinates are rounded to millimetres; this is not a claim
    // that the fixture itself specifies a nanometre-accurate position.
    let expected = Vec3d::new(3_771_793.968, 140_253.342, 5_124_304.349);
    let location = geo(53.0 + 48.0 / 60.0 + 33.82 / 3600.0, 2.12955, 73.0);
    vector_close(geodetic_to_ecef(location), expected, 0.001);
    let inverse = ecef_to_geodetic(expected).unwrap();
    close(inverse.lat_deg, location.lat_deg, 1.0e-8);
    close(inverse.lon_deg, location.lon_deg, 1.0e-8);
    close(inverse.height_m, 73.0, 0.001);
}

#[test]
fn independent_high_precision_algebraic_fixtures() {
    // Computed independently with Python Decimal, precision=70, using exact
    // defining a=6378137 and f=1/Decimal("298.257223563").
    // At phi=lambda=45 degrees, sin=cos=1/sqrt(2), so:
    // N=a/sqrt(1-e²/2), X=Y=(N+h)/2, Z=(N*(1-e²)+h)/sqrt(2).
    let fixtures = [
        (0.0, 3_194_419.145_060_574, 4_487_348.408_865_92, 3.0e-9),
        (1.72, 3_194_420.005_060_574, 4_487_349.625_089_583, 3.0e-9),
        (
            -10_000.0,
            3_189_419.145_060_574,
            4_480_277.341_054_055,
            3.0e-9,
        ),
        (
            1_000_000_000.0,
            503_194_419.145_060_6,
            711_594_129.595_413_4,
            3.0e-7,
        ),
    ];
    for (height, xy, z, tolerance) in fixtures {
        let expected = Vec3d::new(xy, xy, z);
        vector_close(
            geodetic_to_ecef(geo(45.0, 45.0, height)),
            expected,
            tolerance,
        );
        let inverse = ecef_to_geodetic(expected).unwrap();
        close(inverse.lat_deg, 45.0, 2.0e-14);
        close(inverse.lon_deg, 45.0, 2.0e-14);
        close(inverse.height_m, height, tolerance * 2.0);
    }
}

#[test]
fn geodetic_round_trips_include_negative_heights_poles_and_domain_edges() {
    for lat in [
        -90.0,
        -89.99999999,
        -45.0,
        -0.00000001,
        0.0,
        45.0,
        89.99999999,
        90.0,
    ] {
        for lon in [-180.0, -123.456789, 0.0, 123.456789, 180.0] {
            for h in [MIN_HEIGHT_M, -430.0, 0.0, 1.72, 35_786_000.0, MAX_HEIGHT_M] {
                let original = geo(lat, lon, h);
                let position = geodetic_to_ecef(original);
                let inverse = ecef_to_geodetic(position).unwrap();
                close(inverse.lat_deg, lat, 5.0e-13);
                close(inverse.height_m, h, 5.0e-7);
                // Longitude at a pole is a coordinate convention; compare the
                // physical position rather than imposing one longitude.
                assert!(position.distance(geodetic_to_ecef(inverse)) < 1.0e-6);
            }
        }
    }
}

#[test]
fn invalid_geodetic_and_cartesian_positions_are_rejected() {
    for lat in [f64::NAN, f64::INFINITY, -90.00001, 90.00001] {
        assert!(Geodetic::new(lat, 0.0, 0.0).is_err());
    }
    for lon in [f64::NAN, f64::NEG_INFINITY, -180.00001, 180.00001] {
        assert!(Geodetic::new(0.0, lon, 0.0).is_err());
    }
    for h in [
        f64::NAN,
        f64::INFINITY,
        MIN_HEIGHT_M - 1.0,
        MAX_HEIGHT_M + 1.0,
    ] {
        assert!(Geodetic::new(0.0, 0.0, h).is_err());
    }
    for p in [
        Vec3d::ZERO,
        Vec3d::new(1.0, 2.0, 3.0),
        Vec3d::new(f64::INFINITY, 0.0, 0.0),
    ] {
        assert!(ecef_to_geodetic(p).is_err());
    }
}

#[test]
fn enu_and_camera_handedness_match_geodetic_convention() {
    // ESA Navipedia ENU basis: east cross north = up.
    // https://gssc.esa.int/navipedia/index.php/Transformations_between_ECEF_and_ENU_coordinates
    for lat in [-90.0, -48.0, 0.0, 55.0, 90.0] {
        for lon in [-180.0, -78.0, 0.0, 132.0, 180.0] {
            let point = geo(lat, lon, 1.72);
            let enu = EnuBasis::at(point);
            vector_close(enu.east.cross(enu.north), enu.up, 4.0e-16);
            let north = CameraFrame::new(point, 0.0, 0.0);
            vector_close(north.forward, enu.north, 0.0);
            vector_close(north.right, enu.east, 0.0);
            let east = CameraFrame::new(point, 90.0, 0.0);
            vector_close(east.forward, enu.east, 2.0e-16);
            vector_close(east.right, -enu.north, 2.0e-16);
            for heading in [-720.0, -17.0, 0.0, 90.0, 360.0, 1078.0] {
                for pitch in [-90.0, -35.0, 0.0, 37.0, 90.0] {
                    let frame = CameraFrame::new(point, heading, pitch);
                    close(frame.forward.length(), 1.0, 5.0e-16);
                    close(frame.right.length(), 1.0, 5.0e-16);
                    close(frame.up.length(), 1.0, 8.0e-16);
                    close(frame.forward.dot(frame.right), 0.0, 5.0e-16);
                    close(frame.forward.dot(frame.up), 0.0, 5.0e-16);
                    close(frame.up.dot(frame.right), 0.0, 5.0e-16);
                    vector_close(frame.right.cross(frame.forward), frame.up, 0.0);
                }
            }
        }
    }
}

#[test]
fn ray_range_matches_independent_ellipsoid_axis_intersection() {
    // Independent 70-digit Decimal oracle:
    // Xhit=a*sqrt(1-(100000/a)^2-(200000/b)^2).
    let expected_range = 625_804.991_145_995;
    for magnitude in [1.0e-250, 1.0e-150, 0.01, 1.0, 10.0, 1.0e150, 1.0e250] {
        let hit = intersect_wgs84(Ray {
            origin: Vec3d::new(7_000_000.0, 100_000.0, 200_000.0),
            direction: Vec3d::new(-magnitude, 0.0, 0.0),
        })
        .unwrap();
        close(hit.distance_m, expected_range, 5.0e-9);
        close(hit.position.y, 100_000.0, 0.0);
        close(hit.position.z, 200_000.0, 0.0);
        close(hit.normal.length(), 1.0, 3.0e-16);
    }
}

#[test]
fn ray_distinguishes_grazing_hit_and_miss_and_preserves_tangency() {
    // The axis-aligned tangent is exactly representable: x=a, direction +Y.
    // This avoids defining an "exact tangent" using a rounded dip angle.
    let tangent = intersect_wgs84(Ray {
        origin: Vec3d::new(WGS84_A_M, -10_000.0, 0.0),
        direction: Vec3d::new(0.0, 17.0, 0.0),
    })
    .unwrap();
    close(tangent.distance_m, 10_000.0, 0.001);
    for offset in [0.000001, 0.001, 1.0] {
        assert!(
            intersect_wgs84(Ray {
                origin: Vec3d::new(WGS84_A_M + offset, -10_000.0, 0.0),
                direction: Vec3d::new(0.0, 1.0, 0.0),
            })
            .is_none()
        );
        assert!(
            intersect_wgs84(Ray {
                origin: Vec3d::new(WGS84_A_M - offset, -10_000.0, 0.0),
                direction: Vec3d::new(0.0, 1.0, 0.0),
            })
            .is_some()
        );
    }
}

#[test]
fn rays_starting_inside_and_on_surface_have_correct_forward_roots() {
    let inside = intersect_wgs84(Ray {
        origin: Vec3d::ZERO,
        direction: Vec3d::new(0.0, 0.0, -3.0),
    })
    .unwrap();
    close(inside.distance_m, WGS84_B_M, 1.0e-9);
    for sign in [-1.0, 1.0] {
        let surface = intersect_wgs84(Ray {
            origin: Vec3d::new(WGS84_A_M, 0.0, 0.0),
            direction: Vec3d::new(sign, 0.0, 0.0),
        })
        .unwrap();
        close(surface.distance_m, 0.0, 0.0);
    }
    for direction in [Vec3d::ZERO, Vec3d::new(f64::NAN, 0.0, 0.0)] {
        assert!(
            intersect_wgs84(Ray {
                origin: Vec3d::ZERO,
                direction
            })
            .is_none()
        );
    }
}

#[test]
fn downward_range_is_eye_height_across_the_globe() {
    for lat in [-90.0, -78.0, -45.0, 0.0, 31.0, 67.0, 90.0] {
        for lon in [-180.0, -103.0, 0.0, 73.0, 180.0] {
            for height in [0.001, 1.72, 1_000.0, 1_000_000.0] {
                let eye = geo(lat, lon, height);
                let hit = intersect_wgs84(CameraFrame::new(eye, 83.0, -90.0).center_ray()).unwrap();
                close(hit.distance_m, height, 5.0e-9);
            }
        }
    }
}

#[test]
fn projections_round_trip_and_share_exact_center_direction() {
    let frame = CameraFrame::new(geo(47.4, -122.3, 1700.0), 137.0, -41.0);
    for projection in [Projection::Rectilinear, Projection::EquidistantFisheye] {
        for fov in [0.001, 30.0, 60.0, 150.0] {
            let lens = Lens::new(projection, fov);
            vector_close(
                lens.ray(frame, 0.0, 0.0, 1.7).unwrap(),
                frame.forward,
                3.0e-16,
            );
            for aspect in [0.5, 1.0, 1.7, 4.0] {
                for x in [-0.8, -0.1, 0.0, 0.3, 0.8] {
                    for y in [-0.8, -0.2, 0.0, 0.5, 0.8] {
                        let Some(direction) = lens.ray(frame, x, y, aspect) else {
                            continue;
                        };
                        let pixel = lens.project(frame, direction, aspect).unwrap();
                        close(pixel[0], x, 1.0e-10);
                        close(pixel[1], y, 1.0e-10);
                    }
                }
            }
        }
    }
    let fish = Lens::new(Projection::EquidistantFisheye, 180.0);
    assert!(fish.ray(frame, 1.0, 1.0, 1.0).is_none());
    vector_close(fish.ray(frame, 0.0, 1.0, 1.0).unwrap(), frame.up, 3.0e-16);
    assert!(fish.project(frame, -frame.forward, 1.0).is_none());
}

#[test]
fn rectilinear_projection_preserves_straight_lines() {
    let frame = CameraFrame::new(geo(0.0, 0.0, 10.0), 0.0, 0.0);
    let lens = Lens::new(Projection::Rectilinear, 90.0);
    // All these 3D directions have the same local y/z ratio; their image
    // points must occupy a horizontal line without radial lens distortion.
    for x in [-0.8, -0.2, 0.0, 0.4, 0.8] {
        let ndc = lens
            .project(frame, frame.forward + frame.right * x + frame.up * 0.3, 1.0)
            .unwrap();
        close(ndc[0], x, 3.0e-16);
        close(ndc[1], 0.3, 3.0e-16);
    }
}

#[test]
fn observer_eyes_and_units_preserve_physical_distances() {
    // NIST: international foot = 0.3048 metres exactly.
    // https://www.nist.gov/pml/us-surveyfoot
    close(STATURE_M, 1.8288, 3.0e-16);
    close(UnitSystem::Imperial.length_to_metres(6.0), 1.8288, 3.0e-16);
    close(
        UnitSystem::Imperial.length_from_metres(1609.344),
        5280.0,
        1.0e-12,
    );
    for lat in [-90.0, -51.0, 0.0, 48.0, 90.0] {
        let observer = Observer::new(geo(lat, 137.0, 0.0));
        close(
            observer
                .eye_position()
                .distance(geodetic_to_ecef(observer.feet)),
            observer.eye_height_m,
            2.0e-9,
        );
    }
}

#[test]
fn downward_flight_cannot_tunnel_through_surface_at_high_speed() {
    for lat in [-89.0, -78.0, -45.0, -13.0, 0.0, 31.0, 67.0, 89.0] {
        for lon in [-179.0, -103.0, 0.0, 73.0, 179.0] {
            let mut observer = Observer::new(geo(lat, lon, 0.0));
            let original = geodetic_to_ecef(observer.feet);
            observer.pitch_deg = -90.0;
            observer.speed_mps = 50_000_000.0;
            observer.fly(1.0, 0.0, 0.0, 0.25, true);
            assert!(
                original.distance(geodetic_to_ecef(observer.feet)) < 0.001,
                "Downward flight moved through the surface at ({lat}, {lon}) to {:?}",
                observer.feet
            );
        }
    }
}

#[test]
fn projection_magnification_uses_the_selected_lens_mapping() {
    for projection in [Projection::Rectilinear, Projection::EquidistantFisheye] {
        close(Lens::new(projection, 60.0).magnification(), 1.0, 0.0);
    }
    close(
        Lens::new(Projection::EquidistantFisheye, 180.0).magnification(),
        1.0 / 3.0,
        0.0,
    );
    close(
        Lens::new(Projection::EquidistantFisheye, 30.0).magnification(),
        2.0,
        0.0,
    );
    // Rectilinear central image scale is cot(FOV/2). At FOV 90 degrees
    // relative to 60 degrees, magnification is 1/sqrt(3).
    close(
        Lens::new(Projection::Rectilinear, 90.0).magnification(),
        1.0 / 3.0_f64.sqrt(),
        3.0e-16,
    );
}

#[test]
fn telescope_precision_remains_bounded_at_minimum_supported_fov() {
    let frame = CameraFrame::new(geo(47.4, -122.3, 1700.0), 137.0, -41.0);
    for projection in [Projection::Rectilinear, Projection::EquidistantFisheye] {
        let lens = Lens::new(projection, earth_math::MIN_FOV_DEG);
        for (x, y) in [(0.0, 0.0), (-0.1, 0.2), (0.3, 0.4)] {
            let direction = lens.ray(frame, x, y, 1.7).unwrap();
            let projected = lens.project(frame, direction, 1.7).unwrap();
            // Angular roundoff is amplified by the finite lens zoom. This is
            // under 0.0001 pixel on a 4K viewport, not infinite resolution.
            close(projected[0], x, 4.0e-8);
            close(projected[1], y, 4.0e-8);
        }
    }
}

#[test]
fn observer_height_limits_keep_eyes_in_domain_without_freezing_outward_flight() {
    for (latitude, longitude) in [(0.0, 0.0), (47.4, -122.3), (-90.0, 45.0)] {
        let mut observer = Observer::new(geo(latitude, longitude, MAX_HEIGHT_M));
        close(observer.eye_geodetic().height_m, MAX_HEIGHT_M, 0.0);
        observer.set_eye_height(STATURE_M);
        close(observer.eye_geodetic().height_m, MAX_HEIGHT_M, 0.0);
        assert!(ecef_to_geodetic(observer.eye_position()).is_ok());

        observer.teleport(geo(latitude, longitude, MAX_HEIGHT_M - 100.0));
        let initial_height = observer.feet.height_m;
        observer.speed_mps = 50_000_000.0;
        observer.fly(0.0, 0.0, 1.0, 0.25, true);
        assert!(observer.feet.height_m > initial_height);
        close(observer.eye_geodetic().height_m, MAX_HEIGHT_M, 0.0);
        close(observer.feet.lat_deg, latitude, 3.0e-13);
    }
}

#[test]
fn exact_coordinate_navigation_rejects_instead_of_silently_adjusting() {
    let mut observer = Observer::new(geo(17.25, -123.75, 100.0));
    let initial = observer.feet;
    for invalid in [
        geo(10.0, 20.0, -1.0),
        geo(10.0, 20.0, MAX_HEIGHT_M),
        Geodetic {
            lat_deg: 100.0,
            lon_deg: 0.0,
            height_m: 0.0,
        },
        Geodetic {
            lat_deg: 0.0,
            lon_deg: f64::NAN,
            height_m: 0.0,
        },
    ] {
        assert!(observer.try_teleport(invalid).is_err());
        assert_eq!(observer.feet, initial);
    }
    let exact = geo(-77.123456789, 179.987654321, 1234.567890123);
    observer.try_teleport(exact).unwrap();
    assert_eq!(observer.feet, exact);
    let upper_limit = geo(89.0, -179.0, MAX_HEIGHT_M - observer.eye_height_m);
    observer.try_teleport(upper_limit).unwrap();
    assert_eq!(observer.feet, upper_limit);
}

#[test]
fn reduced_raster_rays_align_with_full_display_overlays_after_resize() {
    let frame = CameraFrame::new(geo(47.4, -122.3, 1700.0), 137.0, -41.0);
    for display in [
        [1439.0_f32, 901.0_f32],
        [1440.0, 901.0],
        [769.0, 1421.0],
        [3840.0, 2160.0],
    ] {
        let (width, height) = earth_math::render_dimensions(display[0], display[1], 800);
        for projection in [Projection::Rectilinear, Projection::EquidistantFisheye] {
            let lens = Lens::new(projection, 100.0);
            for (x, y) in [
                (width / 2, height / 2),
                (width * 3 / 5, height * 2 / 5),
                (width * 2 / 5, height * 3 / 5),
            ] {
                let ray = lens
                    .pixel_ray_for_view(frame, x, y, width, height, display)
                    .unwrap();
                let ndc = lens
                    .project(
                        frame,
                        ray.direction,
                        f64::from(display[0]) / f64::from(display[1]),
                    )
                    .unwrap();
                let screen = [
                    (ndc[0] + 1.0) * 0.5 * f64::from(display[0]),
                    (1.0 - ndc[1]) * 0.5 * f64::from(display[1]),
                ];
                close(
                    screen[0],
                    (f64::from(x) + 0.5) / f64::from(width) * f64::from(display[0]),
                    0.0002,
                );
                close(
                    screen[1],
                    (f64::from(y) + 0.5) / f64::from(height) * f64::from(display[1]),
                    0.0002,
                );
            }
            let center_pixel = lens
                .pixel_ray_for_view(frame, 400, 250, 801, 501, display)
                .unwrap();
            vector_close(
                center_pixel.direction,
                frame.center_ray().direction,
                3.0e-16,
            );
        }
    }
}

#[test]
fn continuous_forward_flight_crosses_both_poles_without_reversal() {
    for (latitude, heading) in [(89.999, 0.0), (-89.999, 180.0)] {
        let mut observer = Observer::new(geo(latitude, 77.0, 100_000.0));
        observer.heading_deg = heading;
        observer.speed_mps = 10_000.0;
        let initial_position = geodetic_to_ecef(observer.feet);
        let initial_direction = observer.frame().forward;
        for step in 1..=5 {
            observer.fly(1.0, 0.0, 0.0, 0.1, false);
            let expected = initial_position + initial_direction * (f64::from(step) * 1000.0);
            assert!(
                geodetic_to_ecef(observer.feet).distance(expected) < 1.0e-7,
                "Forward movement reversed or deviated at pole crossing: {:?}",
                observer.feet
            );
            vector_close(observer.frame().forward, initial_direction, 3.0e-14);
        }
    }
}

#[test]
fn free_flight_preserves_world_sight_and_straight_ecef_motion() {
    let mut observer = Observer::new(geo(40.0, -75.0, 1_000_000.0));
    observer.heading_deg = 37.0;
    observer.pitch_deg = 12.0;
    observer.speed_mps = 1_000_000.0;
    let initial_position = geodetic_to_ecef(observer.feet);
    let initial_direction = observer.frame().forward;
    for step in 1..=20 {
        observer.fly(1.0, 0.0, 0.0, 0.1, false);
        let expected = initial_position + initial_direction * (f64::from(step) * 100_000.0);
        assert!(geodetic_to_ecef(observer.feet).distance(expected) < 5.0e-7);
        vector_close(observer.frame().forward, initial_direction, 3.0e-14);
    }
    let mut vertical = Observer::new(geo(0.0, 0.0, 100.0));
    vertical.heading_deg = 83.0;
    vertical.pitch_deg = 90.0;
    vertical.fly(1.0, 0.0, 0.0, 0.1, false);
    close(vertical.heading_deg, 83.0, 0.0);
}
