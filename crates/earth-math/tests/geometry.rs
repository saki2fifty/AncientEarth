use earth_math::*;

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{actual:.16} != {expected:.16} (tol {tolerance})"
    );
}

#[test]
fn geographiclib_published_fixture() {
    // GeographicLib CartConvert manual; its expected results are rounded to 0.01m.
    let actual = geodetic_to_ecef(Geodetic::new(33.3, 44.4, 6000.0).unwrap());
    close(actual.x, 3_816_209.60, 0.005);
    close(actual.y, 3_737_108.55, 0.005);
    close(actual.z, 3_485_109.57, 0.005);
}

#[test]
fn grid_roundtrip_including_poles_dateline_and_negative_height() {
    for lat in [
        -90.0, -89.999999, -70.0, -45.0, -0.001, 0.0, 23.5, 89.999999, 90.0,
    ] {
        for lon in [-180.0, -145.0, -0.1, 0.0, 45.0, 179.9999, 180.0] {
            for height in [MIN_HEIGHT_M, -500.0, 0.0, 1.72, 1000.0, 1e7, MAX_HEIGHT_M] {
                let source = Geodetic::new(lat, lon, height).unwrap();
                let ecef = geodetic_to_ecef(source);
                let recovered = ecef_to_geodetic(ecef).unwrap();
                close(recovered.lat_deg, lat, 4e-13);
                close(recovered.lon_deg, lon, 4e-13);
                close(recovered.height_m, height, 7e-7);
                assert!(ecef.distance(geodetic_to_ecef(recovered)) < 1e-6);
            }
        }
    }
}

#[test]
fn input_validation_is_explicit() {
    assert_eq!(Geodetic::new(91.0, 0.0, 0.0), Err(GeometryError::Latitude));
    assert_eq!(
        Geodetic::new(0.0, 181.0, 0.0),
        Err(GeometryError::Longitude)
    );
    assert_eq!(
        Geodetic::new(0.0, 0.0, MIN_HEIGHT_M - 1.0),
        Err(GeometryError::Height)
    );
    assert_eq!(
        Geodetic::new(f64::NAN, 0.0, 0.0),
        Err(GeometryError::NonFinite)
    );
    assert_eq!(
        ecef_to_geodetic(Vec3d::ZERO),
        Err(GeometryError::UnsupportedInterior)
    );
    assert!(Ray::new(Vec3d::ZERO, Vec3d::ZERO).is_none());
}

#[test]
fn eye_to_surface_is_eye_height_and_not_stature() {
    close(STATURE_M, 1.8288, 1e-15);
    for lat in [-90.0, -45.0, 0.0, 40.0, 90.0] {
        let ground = Geodetic::new(lat, 35.0, 0.0).unwrap();
        let mut person = Observer::new(ground);
        person.pitch_deg = -90.0;
        let hit = intersect_wgs84(person.frame().center_ray()).unwrap();
        close(hit.distance_m, DEFAULT_EYE_HEIGHT_M, 3e-9);
        close(
            person.eye_position().distance(geodetic_to_ecef(ground)),
            DEFAULT_EYE_HEIGHT_M,
            1e-9,
        );
    }
}

#[test]
fn ray_hit_miss_inside_and_nonunit() {
    let origin = Vec3d::new(WGS84_A_M + 100.0, 0.0, 0.0);
    for scale in [0.001, 1.0, 1000.0] {
        let hit = intersect_wgs84(Ray {
            origin,
            direction: Vec3d::new(-scale, 0.0, 0.0),
        })
        .unwrap();
        close(hit.distance_m, 100.0, 2e-9);
    }
    assert!(
        intersect_wgs84(Ray {
            origin,
            direction: Vec3d::new(1.0, 0.0, 0.0)
        })
        .is_none()
    );
    let inside = intersect_wgs84(Ray {
        origin: Vec3d::ZERO,
        direction: Vec3d::new(0.0, 0.0, -5.0),
    })
    .unwrap();
    close(inside.distance_m, WGS84_B_M, 1e-9);
}

#[test]
fn lens_center_and_inverse_for_both_projection_models() {
    let frame = CameraFrame::new(Geodetic::new(52.0, -1.0, 500.0).unwrap(), 135.0, -12.0);
    for projection in [Projection::Rectilinear, Projection::EquidistantFisheye] {
        for fov in [MIN_FOV_DEG, 10.0, 60.0, 150.0] {
            let lens = Lens::new(projection, fov);
            let center = lens.ray(frame, 0.0, 0.0, 16.0 / 9.0).unwrap();
            assert!(center.distance(frame.forward) < 1e-15);
            for (x, y) in [(-0.4, -0.3), (0.2, 0.7), (0.0, 0.0), (0.35, -0.5)] {
                let direction = lens.ray(frame, x, y, 16.0 / 9.0).unwrap();
                let result = lens.project(frame, direction, 16.0 / 9.0).unwrap();
                close(result[0], x, if fov < 0.01 { 4e-8 } else { 1e-13 });
                close(result[1], y, if fov < 0.01 { 4e-8 } else { 1e-13 });
            }
        }
    }
}

#[test]
fn unit_conversions_and_zoom_are_bounded() {
    close(UnitSystem::Imperial.length_to_metres(6.0), STATURE_M, 1e-15);
    close(
        UnitSystem::Imperial.length_from_metres(METRES_PER_MILE),
        5280.0,
        1e-12,
    );
    let mut lens = Lens::default();
    for _ in 0..100 {
        lens.zoom(100.0);
    }
    close(lens.vertical_fov_deg, MIN_FOV_DEG, 0.0);
    for _ in 0..100 {
        lens.zoom(-100.0);
    }
    close(lens.vertical_fov_deg, 150.0, 0.0);
    close(adjust_speed(10.0, 1e9), MAX_SPEED_MPS, 0.0);
}

#[test]
fn precise_distance_readout_preserves_unscaled_units() {
    assert_eq!(
        UnitSystem::Metric.format_precise_distance(12_345.123_456_4),
        "12345.123456 m"
    );
    assert_eq!(
        UnitSystem::Metric.format_precise_distance(12_345.123_456_6),
        "12345.123457 m"
    );
    assert_eq!(
        UnitSystem::Imperial.format_precise_distance(METRES_PER_MILE),
        "5280.000000 ft"
    );
    assert_eq!(
        UnitSystem::Imperial.format_precise_distance(STATURE_M),
        "6.000000 ft"
    );
    assert_eq!(
        UnitSystem::Metric.format_precise_distance(0.0),
        "0.000000 m"
    );
}

#[test]
fn flight_diagonal_normalization_and_surface_collision() {
    let position = Geodetic::new(0.0, 0.0, 1000.0).unwrap();
    let mut cardinal = Observer::new(position);
    let mut diagonal = cardinal;
    cardinal.speed_mps = 100.0;
    diagonal.speed_mps = 100.0;
    cardinal.fly(1.0, 0.0, 0.0, 0.1, false);
    diagonal.fly(1.0, 1.0, 0.0, 0.1, false);
    close(
        geodetic_to_ecef(position).distance(geodetic_to_ecef(cardinal.feet)),
        10.0,
        2e-9,
    );
    close(
        geodetic_to_ecef(position).distance(geodetic_to_ecef(diagonal.feet)),
        10.0,
        2e-9,
    );
    let mut falling = Observer::new(position);
    falling.speed_mps = 50_000_000.0;
    falling.fly(0.0, 0.0, -1.0, 0.25, true);
    assert!(falling.feet.height_m >= 0.0 && falling.feet.height_m < 0.001);
    let before = falling.feet;
    falling.fly(0.0, 0.0, -1.0, 0.25, true);
    assert!(geodetic_to_ecef(before).distance(geodetic_to_ecef(falling.feet)) < 0.001);
}

#[test]
fn texture_georeference_and_pixel_center_are_consistent() {
    assert_eq!(
        texture_uv(Geodetic::new(90.0, -180.0, 0.0).unwrap()),
        [0.0, 0.0]
    );
    assert_eq!(
        texture_uv(Geodetic::new(-90.0, 180.0, 0.0).unwrap()),
        [0.0, 1.0]
    );
    assert_eq!(
        texture_uv(Geodetic::new(0.0, 0.0, 0.0).unwrap()),
        [0.5, 0.5]
    );
    let frame = CameraFrame::new(Geodetic::new(0.0, 0.0, 100.0).unwrap(), 0.0, 0.0);
    let ray = Lens::default().pixel_ray(frame, 2, 2, 5, 5).unwrap();
    assert!(ray.direction.distance(frame.forward) < 1e-15);
}
