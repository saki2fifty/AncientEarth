//! Independent observation fixtures: explicit synthetic datum values, analytic
//! Cartesian terrain sections, and a circular equatorial WGS84 section.
use earth_math::*;

fn geo(lat: f64, lon: f64, height: f64) -> Geodetic {
    Geodetic::new(lat, lon, height).unwrap()
}

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        actual.is_finite() && (actual - expected).abs() <= tolerance,
        "actual={actual:.17e}, expected={expected:.17e}, tolerance={tolerance:.17e}"
    );
}

fn synthetic_geoid() -> Vec<f32> {
    vec![
        12.25, 12.25, 12.25, 12.25, // north pole
        -30.5, 0.125, 42.75, 100.0, // equator, -180,-90,0,+90
        -31.75, -31.75, -31.75, -31.75, // south pole
    ]
}

// Construct the format from its specification, without calling a production
// encoder: arrange byte planes, then difference adjacent bytes modulo 256.
fn encode_grid(width: u32, height: u32, samples: &[f32]) -> Vec<u8> {
    let mut bytes = b"AGEGEO01".to_vec();
    bytes.extend(width.to_le_bytes());
    bytes.extend(height.to_le_bytes());
    for value in [
        -180.0,
        90.0,
        360.0 / f64::from(width),
        180.0 / f64::from(height - 1),
    ] {
        bytes.extend(value.to_le_bytes());
    }
    for row in samples.chunks_exact(width as usize) {
        let shuffled: Vec<u8> = (0..4)
            .flat_map(|plane| row.iter().map(move |value| value.to_le_bytes()[plane]))
            .collect();
        bytes.push(shuffled[0]);
        bytes.extend(
            shuffled
                .windows(2)
                .map(|pair| pair[1].wrapping_sub(pair[0])),
        );
    }
    bytes
}

#[test]
fn native_geoid_codec_preserves_signed_float_posts_and_periodic_registration() {
    let bytes = encode_grid(4, 3, &synthetic_geoid());
    let grid = GeoidGrid::from_bytes(&bytes).unwrap();
    assert_eq!(grid.dimensions(), (4, 3));
    for (lon, expected) in [
        (-180.0, -30.5),
        (-90.0, 0.125),
        (0.0, 42.75),
        (90.0, 100.0),
        (180.0, -30.5),
    ] {
        assert_eq!(grid.undulation_m(0.0, lon).unwrap(), expected);
        assert_eq!(grid.undulation_m(90.0, lon).unwrap(), 12.25);
        assert_eq!(grid.undulation_m(-90.0, lon).unwrap(), -31.75);
    }
    // Four equal weights: north-pole pair and equator's 0/+90 degree pair.
    assert_eq!(grid.undulation_m(45.0, 45.0).unwrap(), 41.8125);
    // The final longitude cell interpolates +90 back to -180 across the seam.
    assert_eq!(grid.undulation_m(0.0, 135.0).unwrap(), 34.75);
    close(
        grid.undulation_m(0.0, 179.999_999).unwrap(),
        grid.undulation_m(0.0, -179.999_999).unwrap(),
        2e-6,
    );
}

#[test]
fn invalid_geoid_payloads_and_nonphysical_grid_values_are_rejected() {
    let bytes = encode_grid(4, 3, &synthetic_geoid());
    for length in [0, 7, 47, bytes.len() - 1] {
        assert!(GeoidGrid::from_bytes(&bytes[..length]).is_err());
    }
    let mut corrupt = bytes.clone();
    corrupt[0] = b'X';
    assert!(GeoidGrid::from_bytes(&corrupt).is_err());
    let mut corrupt = bytes.clone();
    corrupt.extend([0]);
    assert!(GeoidGrid::from_bytes(&corrupt).is_err());
    let mut corrupt = bytes;
    corrupt[32..40].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(GeoidGrid::from_bytes(&corrupt).is_err());
    for bad_value in [f32::NAN, f32::INFINITY, 1000.01] {
        let mut samples = synthetic_geoid();
        samples[5] = bad_value;
        assert!(GeoidGrid::from_grid(4, 3, samples).is_err());
    }
    let mut samples = synthetic_geoid();
    samples[0] += 0.5;
    assert!(GeoidGrid::from_grid(4, 3, samples).is_err());
    assert!(GeoidGrid::from_grid(4, 3, vec![0.0; 11]).is_err());
    let grid = GeoidGrid::from_grid(4, 3, synthetic_geoid()).unwrap();
    for (lat, lon) in [
        (90.01, 0.0),
        (0.0, 180.01),
        (f64::NAN, 0.0),
        (0.0, f64::INFINITY),
    ] {
        assert!(grid.undulation_m(lat, lon).is_err());
    }
}

#[test]
fn crouching_moves_only_the_eye_along_geodetic_up_and_changes_actual_range() {
    let mut observer = Observer::new(geo(47.5, -123.4, 0.0));
    observer.pitch_deg = -90.0;
    let standing = observer;
    let posture = EyePosture {
        crouched: true,
        ..EyePosture::default()
    };
    posture.apply(&mut observer).unwrap();
    assert_eq!(observer.feet, standing.feet);
    assert_eq!(observer.heading_deg, standing.heading_deg);
    assert_eq!(observer.pitch_deg, standing.pitch_deg);
    let expected_delta = EnuBasis::at(observer.feet).up * (1.72 - 1.0);
    close(
        (standing.eye_position() - observer.eye_position() - expected_delta).length(),
        0.0,
        2e-9,
    );
    assert_eq!(observer.frame().origin, observer.eye_position());
    let scene = TerrainScene::default();
    close(
        scene
            .intersect(standing.frame().center_ray())
            .unwrap()
            .hit
            .distance_m,
        1.72,
        3e-9,
    );
    close(
        scene
            .intersect(observer.frame().center_ray())
            .unwrap()
            .hit
            .distance_m,
        1.0,
        3e-9,
    );
    let mut restored = observer;
    EyePosture::default().apply(&mut restored).unwrap();
    assert_eq!(restored, standing);
}

#[test]
fn crouched_flight_and_teleport_reserve_standing_ceiling_without_moving_feet() {
    let mut observer = Observer::new(geo(0.0, 0.0, MAX_HEIGHT_M - 2.0));
    let mut posture = EyePosture {
        standing_eye_height_m: 1.8,
        crouching_eye_height_m: 0.5,
        crouched: true,
    };
    posture.apply(&mut observer).unwrap();
    assert_eq!(observer.maximum_feet_height_m(), MAX_HEIGHT_M - 1.8);
    let scene = TerrainScene::default();
    let before = observer;
    assert!(
        scene
            .try_teleport(&mut observer, geo(0.0, 0.0, MAX_HEIGHT_M - 1.0))
            .is_err()
    );
    assert_eq!(observer, before);
    assert!(
        observer
            .try_teleport(geo(0.0, 0.0, MAX_HEIGHT_M - 1.0))
            .is_err()
    );
    assert_eq!(observer, before);
    observer.speed_mps = 1000.0;
    scene.fly(&mut observer, 0.0, 0.0, 1.0, 0.25, false);
    assert_eq!(observer.feet.height_m, MAX_HEIGHT_M - 1.8);
    let fixed_feet = observer.feet;
    posture.crouched = false;
    posture.apply(&mut observer).unwrap();
    assert_eq!(observer.feet, fixed_feet);
    assert_eq!(observer.eye_geodetic().height_m, MAX_HEIGHT_M);
    let before = observer;
    posture.standing_eye_height_m = STATURE_M;
    assert!(posture.apply(&mut observer).is_err());
    assert_eq!(observer, before);
}

#[test]
fn posture_configuration_enforces_stature_and_crouch_never_raises_the_eye() {
    let mut posture = EyePosture::default();
    posture.set_standing_height(500.0);
    posture.set_crouching_height(-10.0);
    assert_eq!(posture.standing_eye_height_m, STATURE_M);
    assert_eq!(posture.crouching_eye_height_m, 0.1);
    posture.set_standing_height(0.6);
    posture.set_crouching_height(1.2);
    posture.crouched = true;
    assert_eq!(posture.effective_height_m(), 0.6);
    let before = posture;
    posture.set_standing_height(f64::NAN);
    assert_eq!(posture, before);
    let mut observer = Observer::new(geo(0.0, 0.0, 0.0));
    let before = observer;
    posture.crouching_eye_height_m = f64::INFINITY;
    assert!(posture.apply(&mut observer).is_err());
    assert_eq!(observer, before);
}

#[test]
fn vertical_report_uses_h_minus_n_and_ground_normal_clearance() {
    let grid = GeoidGrid::from_grid(2, 2, vec![-30.0; 4]).unwrap();
    let scene = TerrainScene::default();
    let mut observer = Observer::new(geo(38.0, -110.0, 100.0));
    let standing = scene.vertical_report(observer, &grid).unwrap();
    close(standing.ground_egm2008_height_m, 30.0, 2e-8);
    assert_eq!(standing.feet_egm2008_height_m, 130.0);
    close(standing.eye_egm2008_height_m, 131.72, 1e-13);
    close(standing.feet_above_ground_m, 100.0, 2e-8);
    close(standing.eyes_above_ground_m, 101.72, 2e-8);
    assert_eq!(standing.geoid_position, geo(38.0, -110.0, -30.0));
    EyePosture {
        crouched: true,
        ..EyePosture::default()
    }
    .apply(&mut observer)
    .unwrap();
    let crouched = scene.vertical_report(observer, &grid).unwrap();
    assert_eq!(
        crouched.feet_egm2008_height_m,
        standing.feet_egm2008_height_m
    );
    assert_eq!(
        crouched.ground_egm2008_height_m,
        standing.ground_egm2008_height_m
    );
    close(
        standing.eye_egm2008_height_m - crouched.eye_egm2008_height_m,
        0.72,
        1e-13,
    );
}

#[test]
fn horizontal_guide_is_one_framebuffer_row_under_fractional_dpi() {
    for (display, scale) in [
        ([1440.0, 900.0], [1.0, 1.0]),
        ([960.0, 600.0], [1.5, 1.5]),
        ([1536.0, 864.0], [1.25, 1.25]),
        ([1280.0, 720.0], [2.0, 2.0]),
        ([701.25, 503.25], [1.25, 1.5]),
        ([1920.0, 1081.0], [1.0, 1.0]),
    ] {
        let line = physical_horizontal_line(display, scale).unwrap();
        let physical_height = f64::from(display[1]) * f64::from(scale[1]);
        let expected_row = (physical_height.floor() / 2.0).floor();
        assert_eq!(line.min[0], 0.0);
        assert_eq!(line.max[0], display[0]);
        close(
            f64::from(line.min[1]) * f64::from(scale[1]),
            expected_row,
            0.0001,
        );
        close(
            f64::from(line.max[1]) * f64::from(scale[1]),
            expected_row + 1.0,
            0.0001,
        );
    }
    assert!(physical_horizontal_line([1440.0, 0.0], [1.0, 1.0]).is_none());
    assert!(physical_horizontal_line([f32::NAN, 900.0], [1.0, 1.0]).is_none());
    assert!(physical_horizontal_line([1440.0, 900.0], [1.0, 0.0]).is_none());
    assert!(physical_horizontal_line([1440.0, 0.1], [1.0, 1.0]).is_none());
}

#[test]
fn panel_bounds_reserve_footer_and_margins_even_after_dpi_resize() {
    assert_eq!(
        constrained_panel_size([960.0, 600.0], [342.0, 748.0], [18.0, 18.0]),
        [342.0, 564.0]
    );
    assert_eq!(
        page_panel_size([960.0, 600.0], [342.0, 748.0], [18.0, 18.0], 116.0),
        [342.0, 430.0]
    );
    for display in [[1440.0, 900.0], [960.0, 600.0], [640.0, 360.0]] {
        let size = page_panel_size(display, [370.0, 800.0], [18.0, 18.0], 116.0);
        let panel_bottom = 18.0 + size[1];
        let footer_top = display[1] - 18.0 - 116.0;
        assert!(panel_bottom <= footer_top - 18.0);
        assert!(size[0] + 36.0 <= display[0]);
    }
    assert_eq!(
        page_panel_size([0.0, 0.0], [370.0, 800.0], [18.0, 18.0], 116.0),
        [1.0, 1.0]
    );
    assert_eq!(
        page_panel_size([960.0, 600.0], [370.0, 800.0], [18.0, 18.0], f32::NAN),
        [370.0, 1.0]
    );
}

#[test]
fn equatorial_east_skyline_matches_the_exact_circle_tangent_and_crouch_change() {
    let scene = TerrainScene::default();
    for height in [0.0, 100.0, 100_000.0, 10_000_000.0] {
        let mut observer = Observer::new(geo(0.0, 0.0, height));
        observer.heading_deg = 90.0;
        let mut standing_range = 0.0;
        for crouched in [false, true] {
            EyePosture {
                crouched,
                ..EyePosture::default()
            }
            .apply(&mut observer)
            .unwrap();
            let h = height + observer.eye_height_m;
            // This particular WGS84 section is exactly a circle of radius a.
            let expected_range = (h * (2.0 * WGS84_A_M + h)).sqrt();
            let expected_angle = -expected_range.atan2(WGS84_A_M);
            let skyline = scene.skyline(observer).unwrap();
            assert_eq!(skyline.method, SkylineMethod::ReferenceEllipsoidTangent);
            assert_eq!(skyline.hit.kind, SurfaceKind::ReferenceEllipsoid);
            close(skyline.elevation_deg.to_radians(), expected_angle, 5e-12);
            // Tangency is ill-conditioned in floating point: O(a*sqrt(eps))
            // positional uncertainty is distinct from source/model accuracy.
            close(skyline.hit.hit.distance_m, expected_range, 0.25);
            assert_eq!(skyline.angular_search_step_deg, 0.0);
            if crouched {
                assert!(skyline.hit.hit.distance_m < standing_range);
            } else {
                standing_range = skyline.hit.hit.distance_m;
            }
            observer.pitch_deg = 73.0;
            let tilted = scene.skyline(observer).unwrap();
            assert_eq!(tilted.elevation_deg, skyline.elevation_deg);
            assert_eq!(tilted.hit.hit.distance_m, skyline.hit.hit.distance_m);
        }
    }
}

#[test]
fn oblique_wgs84_tangents_match_independent_70_digit_plane_circle_fixtures() {
    // Independent construction using mpmath 1.3.0 at 70 decimal digits:
    // map X=A*Y, A=diag(a,a,b), to the unit sphere. The heading plane has
    // unit normal M=A*S/|A*S| and offset d=S.P/|A*S| in Y coordinates.
    // Its circle has center C=d*M and rho²=1-d². With q=A^-1*P-C,
    // tangency points are C+rho²*q/|q|²
    // +/- rho*sqrt(|q|²-rho²)*(M cross q)/|q|².
    // Map back by A and select the tangent in the forward half-plane.
    // This avoids the production direction-quadratic construction.
    let scene = TerrainScene::default();
    for (lat, lon, heading, feet_height, distance, elevation) in [
        (
            45.0,
            30.0,
            37.0,
            1200.0,
            123_795.371_525_590_69,
            -1.112_369_663_473_930_4,
        ),
        (
            70.0,
            -120.0,
            143.0,
            10.0,
            12_242.187_737_597_585,
            -0.109_703_466_453_625_85,
        ),
        (
            -33.0,
            151.0,
            271.0,
            100_000.0,
            1_134_386.962_656_798_4,
            -10.075_557_779_126_951,
        ),
        (
            90.0,
            0.0,
            55.0,
            0.0,
            4_691.972_396_678_699,
            -0.042_007_381_087_137_75,
        ),
    ] {
        let mut observer = Observer::new(geo(lat, lon, feet_height));
        observer.heading_deg = heading;
        let skyline = scene.skyline(observer).unwrap();
        assert_eq!(skyline.method, SkylineMethod::ReferenceEllipsoidTangent);
        close(skyline.elevation_deg, elevation, 1e-9);
        close(skyline.hit.hit.distance_m, distance, 0.25);
    }
}

fn ridge(west: f64, height: f64) -> TerrainPatch {
    TerrainPatch::from_grid(2, 2, west, 0.01, 0.005, 0.02, vec![height; 4]).unwrap()
}

#[test]
fn terrain_skyline_is_the_visible_angular_extremum_not_the_farthest_ridge() {
    let scene = TerrainScene::new(vec![ridge(0.005, 200.0), ridge(0.05, 400.0)]);
    let mut observer = Observer::new(geo(0.0, 0.0, 100.0));
    observer.heading_deg = 90.0;
    // The equatorial plane cuts the near ridge's west edge halfway between
    // its +/-0.01 degree latitude endpoints. Compute that ECEF point directly.
    let phi = 0.01_f64.to_radians();
    let lambda = 0.005_f64.to_radians();
    let n = WGS84_A_M / (1.0 - WGS84_E2 * phi.sin().powi(2)).sqrt();
    let x = (n + 200.0) * phi.cos() * lambda.cos();
    let y = (n + 200.0) * phi.cos() * lambda.sin();
    let dx = x - (WGS84_A_M + 101.72);
    let expected_range = dx.hypot(y);
    let expected_angle = dx.atan2(y);
    let skyline = scene.skyline(observer).unwrap();
    assert_eq!(skyline.hit.kind, SurfaceKind::Terrain(0));
    let measured_angle = skyline.elevation_deg.to_radians();
    let (expected_hit_x, expected_hit_y, expected_distance) = match skyline.method {
        SkylineMethod::TerrainSectionExtremum => {
            close(measured_angle, expected_angle, 3e-12);
            (x, y, expected_range)
        }
        SkylineMethod::NumericalBoundaryEstimate => {
            // A heading-plane ray can round outside the exact west edge.
            // Require the explicitly disclosed 1e-9-radian downward probe,
            // then solve that ray's intersection with the Cartesian plane
            // independently instead of loosening the distance assertions.
            assert_eq!(skyline.angular_search_step_deg, 0.0);
            close(skyline.angular_bracket_deg.to_radians(), 1e-9, 1e-20);
            close(measured_angle, expected_angle - 1e-9, 3e-12);
            let east_lambda = 0.010_f64.to_radians();
            let edge_x = (n + 200.0) * phi.cos() * east_lambda.cos() - x;
            let edge_y = (n + 200.0) * phi.cos() * east_lambda.sin() - y;
            let distance = (dx * edge_y - y * edge_x)
                / (measured_angle.sin() * edge_y - measured_angle.cos() * edge_x);
            (
                WGS84_A_M + 101.72 + distance * measured_angle.sin(),
                distance * measured_angle.cos(),
                distance,
            )
        }
        method => panic!("unexpected ridge method: {method:?}"),
    };
    close(skyline.hit.hit.distance_m, expected_distance, 2e-6);
    close(skyline.hit.hit.position.x, expected_hit_x, 2e-8);
    close(skyline.hit.hit.position.y, expected_hit_y, 2e-6);
    close(skyline.hit.hit.position.z, 0.0, 1e-8);
}

#[test]
fn below_ellipsoid_coastal_skyline_is_explicitly_qualified_and_hits_canonical_surface() {
    let patch =
        TerrainPatch::from_bytes(include_bytes!("../../../assets/terrain/galveston.aet")).unwrap();
    let scene = TerrainScene::new(vec![patch]);
    let mut observer = Observer::new(geo(29.30135, -94.7977, 0.0));
    scene
        .place_on_ground(&mut observer, geo(29.30135, -94.7977, 0.0))
        .unwrap();
    observer.heading_deg = 45.0;
    assert!(observer.eye_geodetic().height_m < 0.0);
    let skyline = scene.skyline(observer).unwrap();
    assert_eq!(skyline.method, SkylineMethod::NumericalBoundaryEstimate);
    assert!(skyline.angular_search_step_deg > 0.0);
    assert!(skyline.angular_bracket_deg <= skyline.angular_search_step_deg);
    let enu = EnuBasis::at(observer.feet);
    let h = observer.heading_deg.to_radians();
    let angle = skyline.elevation_deg.to_radians();
    let direction = (enu.north * h.cos() + enu.east * h.sin()) * angle.cos() + enu.up * angle.sin();
    let visible = scene
        .intersect(Ray {
            origin: observer.eye_position(),
            direction,
        })
        .unwrap();
    assert_eq!(visible.kind, skyline.hit.kind);
    close(visible.hit.distance_m, skyline.hit.hit.distance_m, 1e-5);
    assert!(visible.hit.distance_m.is_finite() && visible.hit.distance_m > 0.0);
    // A negative ellipsoidal terrain elevation can still lie above EGM2008.
    let grid = GeoidGrid::from_grid(2, 2, vec![-27.744_387; 4]).unwrap();
    let vertical = scene.vertical_report(observer, &grid).unwrap();
    assert!(vertical.ground_egm2008_height_m > 0.0);
    close(vertical.feet_above_ground_m, 0.0, 1e-8);
    close(vertical.eyes_above_ground_m, 1.72, 1e-8);
}
