//! Independent kinematic fixtures, including explicit Cartesian rotations.
use earth_math::{astro::*, *};

fn observer(lat: f64, lon: f64, h: f64) -> Observer {
    let mut observer = Observer::new(Geodetic::new(lat, lon, h).unwrap());
    observer.heading_deg = 31.0;
    observer.pitch_deg = -17.0;
    observer.roll_deg = 29.0;
    observer
}
fn epoch() -> AstroInstant {
    SimulationClock::from_calendar("2024-04-08 18:40:00")
        .unwrap()
        .instant()
}
fn vector_close(a: Vec3d, b: Vec3d, tolerance: f64) {
    assert!(
        (a - b).length() <= tolerance,
        "{a:?} != {b:?}, error {}",
        (a - b).length()
    );
}
fn axes_close(a: CameraFrame, b: CameraFrame, tolerance: f64) {
    vector_close(a.forward, b.forward, tolerance);
    vector_close(a.up, b.up, tolerance);
    vector_close(a.right, b.right, tolerance);
}

#[test]
fn full_axes_recover_arbitrary_bank_vertical_sights_and_reject_invalid_transactionally() {
    for lat in [-90.0, -43.2, 0.0, 90.0] {
        for pitch in [-90.0, -27.0, 90.0] {
            let mut expected = observer(lat, 137.0, 1000.0);
            expected.pitch_deg = pitch;
            expected.roll_deg = 113.0;
            let frame = expected.frame();
            let mut actual = observer(lat, 137.0, 1000.0);
            actual.heading_deg = 271.0;
            let feet = actual.feet;
            actual
                .set_view_axes(frame.forward * 3.0, frame.up * 5.0 + frame.forward * 2.0)
                .unwrap();
            axes_close(actual.frame(), frame, 2e-14);
            assert_eq!(actual.feet, feet);
            if pitch.abs() == 90.0 {
                assert_eq!(actual.heading_deg, 271.0);
            }
            for (forward, up) in [
                (Vec3d::ZERO, frame.up),
                (frame.forward, frame.forward),
                (Vec3d::new(f64::NAN, 0.0, 0.0), frame.up),
            ] {
                let before = actual;
                assert!(actual.set_view_axes(forward, up).is_err());
                assert_eq!(actual, before);
            }
        }
    }
}

#[test]
fn unlocked_cartesian_quarter_turn_and_inverse_keep_all_icrf_axes_without_drift() {
    let scene = TerrainScene::new(vec![]);
    let mut actual = observer(22.0, 41.0, 2_000_000.0);
    let initial = actual;
    let initial_frame = actual.frame();
    let mut tracker = ObserverTracking::new(actual, epoch(), Mat3d::identity()).unwrap();
    // Active +90 degrees about Cartesian X, written explicitly rather than
    // obtained from the implementation's rotation constructors.
    let quarter = Mat3d {
        rows: [[1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]],
    };
    let rotate = |v: Vec3d| Vec3d::new(v.x, -v.z, v.y);
    for _ in 0..100 {
        tracker
            .update(
                &mut actual,
                false,
                epoch().shifted(3600.0).unwrap(),
                quarter,
                &scene,
            )
            .unwrap();
        vector_close(actual.eye_position(), rotate(initial_frame.origin), 2e-8);
        vector_close(actual.frame().forward, rotate(initial_frame.forward), 2e-14);
        vector_close(actual.frame().up, rotate(initial_frame.up), 2e-14);
        tracker
            .update(&mut actual, false, epoch(), Mat3d::identity(), &scene)
            .unwrap();
        vector_close(actual.eye_position(), initial_frame.origin, 2e-8);
        axes_close(actual.frame(), initial_frame, 2e-14);
    }
    assert_eq!(actual.eye_height_m, initial.eye_height_m);
    assert_eq!(tracker.instant(), epoch());
    assert!(!tracker.follows_earth());
}

#[test]
fn relocking_and_navigation_edits_anchor_at_last_displayed_epoch() {
    let scene = TerrainScene::new(vec![]);
    let first = terrestrial_rotation(epoch(), None);
    let later = terrestrial_rotation(epoch().shifted(7200.0).unwrap(), None);
    let mut actual = observer(-51.0, 173.0, 100_000.0);
    let mut tracker = ObserverTracking::new(actual, epoch(), first).unwrap();
    tracker
        .update(
            &mut actual,
            false,
            epoch().shifted(7200.0).unwrap(),
            later,
            &scene,
        )
        .unwrap();
    let unlocked = actual;
    tracker
        .update(
            &mut actual,
            true,
            epoch().shifted(7200.0).unwrap(),
            later,
            &scene,
        )
        .unwrap();
    assert_eq!(actual, unlocked);
    tracker
        .update(
            &mut actual,
            true,
            epoch().shifted(14400.0).unwrap(),
            terrestrial_rotation(epoch().shifted(14400.0).unwrap(), None),
            &scene,
        )
        .unwrap();
    assert_eq!(actual, unlocked);
    // Teleport + crouch + look are edits made in the currently displayed ECEF
    // frame. Their ICRF anchor must use that epoch, not the newly requested one.
    actual
        .try_teleport(Geodetic::new(63.0, -179.9, 300_000.0).unwrap())
        .unwrap();
    EyePosture {
        crouched: true,
        ..EyePosture::default()
    }
    .apply(&mut actual)
    .unwrap();
    actual.apply_look(7.0, -9.0, Lens::default());
    let before = actual.frame();
    let old_rotation = terrestrial_rotation(tracker.instant(), None);
    let new_epoch = epoch().shifted(20000.0).unwrap();
    let new_rotation = terrestrial_rotation(new_epoch, None);
    tracker
        .update(&mut actual, false, new_epoch, new_rotation, &scene)
        .unwrap();
    vector_close(
        new_rotation.transpose().transform(actual.eye_position()),
        old_rotation.transpose().transform(before.origin),
        3e-8,
    );
    vector_close(
        new_rotation.transpose().transform(actual.frame().up),
        old_rotation.transpose().transform(before.up),
        2e-14,
    );
}

#[test]
fn true_eop_orientation_and_held_pose_agree_across_reverse_time_and_earth_follow_mode() {
    let eop = EarthOrientationData::from_bytes(include_bytes!(
        "../../../assets/astronomy/finals2000A.all"
    ))
    .unwrap();
    let kernel =
        Ephemeris::from_bytes(include_bytes!("../../../assets/astronomy/de440s.bsp")).unwrap();
    let scene = TerrainScene::new(vec![]);
    let start = observer(89.9, -179.0, 1_000_000.0);
    let rotation = terrestrial_rotation(epoch(), Some(&eop));
    let anchor = rotation.transpose().transform(start.eye_position());
    let up = rotation.transpose().transform(start.frame().up);
    let mut actual = start;
    let mut tracker = ObserverTracking::new(actual, epoch(), rotation).unwrap();
    for seconds in [86400.0, -7200.0, 120.0, 0.0] {
        let instant = epoch().shifted(seconds).unwrap();
        let rotation = terrestrial_rotation(instant, Some(&eop));
        tracker
            .update(&mut actual, false, instant, rotation, &scene)
            .unwrap();
        vector_close(
            rotation.transpose().transform(actual.eye_position()),
            anchor,
            3e-8,
        );
        vector_close(rotation.transpose().transform(actual.frame().up), up, 2e-14);
        let sample = kernel
            .sample_with_tracking(instant, Some(&eop), actual.eye_position(), false)
            .unwrap();
        assert_eq!(sample.icrf_to_ecef, rotation);
        let surface = kernel
            .sample(instant, Some(&eop), actual.eye_position())
            .unwrap();
        // Observer aberration changes; the separate co-rotating ground shadow
        // remains exactly the same when toggling at the same position/epoch.
        assert_eq!(
            sample.eclipse.observer_velocity_ecef_mps,
            surface.eclipse.observer_velocity_ecef_mps
        );
        assert_ne!(
            sample.sun.apparent_center_ecef_m,
            surface.sun.apparent_center_ecef_m
        );
        let eclipse = sample.solar_eclipse(actual.eye_position()).unwrap();
        let sun = (sample.sun.apparent_center_ecef_m - actual.eye_position())
            .normalized()
            .unwrap();
        let moon = (sample.moon.apparent_center_ecef_m - actual.eye_position())
            .normalized()
            .unwrap();
        let separation = sun.cross(moon).length().atan2(sun.dot(moon));
        assert!((eclipse.separation_rad - separation).abs() < 2e-14);
    }
}

#[test]
fn rejection_keeps_pose_epoch_mode_and_anchor_and_invalid_rotations_are_rejected() {
    let scene = TerrainScene::new(vec![]);
    let mut actual = observer(45.0, 12.0, 10.0);
    let start = actual;
    let mut tracker = ObserverTracking::new(actual, epoch(), Mat3d::identity()).unwrap();
    assert_eq!(
        tracker.update(
            &mut actual,
            false,
            epoch().shifted(2.0).unwrap(),
            Mat3d::identity(),
            &scene
        ),
        Err(TrackingError::UncertifiedTimeJump)
    );
    assert_eq!(actual, start);
    assert_eq!(tracker.instant(), epoch());
    assert!(tracker.follows_earth());
    for rows in [
        [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]],
        [[1.0, 1e-4, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        [[f64::NAN, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
    ] {
        let invalid = Mat3d { rows };
        assert_eq!(
            tracker.update(&mut actual, true, epoch(), invalid, &scene),
            Err(TrackingError::InvalidRotation)
        );
        assert_eq!(
            tracker.reanchor(actual, false, epoch(), invalid),
            Err(TrackingError::InvalidRotation)
        );
        assert_eq!(actual, start);
        assert_eq!(tracker.observer(), start);
        assert!(tracker.follows_earth());
    }
    // Exact endpoint of the documented short-step policy is permitted.
    tracker
        .update(
            &mut actual,
            false,
            epoch().shifted(1.0).unwrap(),
            Mat3d::identity(),
            &scene,
        )
        .unwrap();
}

#[test]
fn rotating_surface_endpoint_and_global_terrain_envelope_block_unsafe_moves() {
    let patch = TerrainPatch::from_grid(3, 3, 9.99, 0.01, 0.01, 0.01, vec![1000.0; 9]).unwrap();
    let scene = TerrainScene::new(vec![patch]);
    let mut actual = observer(0.0, 0.0, 100.0);
    let start = actual;
    let mut tracker = ObserverTracking::new(actual, epoch(), Mat3d::identity()).unwrap();
    let angle = 10_f64.to_radians();
    let (s, c) = angle.sin_cos();
    let turn = Mat3d {
        rows: [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]],
    };
    assert_eq!(
        tracker.update(
            &mut actual,
            false,
            epoch().shifted(0.5).unwrap(),
            turn,
            &scene
        ),
        Err(TrackingError::BelowSurface)
    );
    assert_eq!(actual, start);
    assert_eq!(tracker.instant(), epoch());
    // A different high terrain tile still constrains the all-orientation
    // time-jump proof, even when the requested endpoint itself is clear.
    assert_eq!(
        tracker.update(
            &mut actual,
            false,
            epoch().shifted(10.0).unwrap(),
            Mat3d::identity(),
            &scene
        ),
        Err(TrackingError::UncertifiedTimeJump)
    );
    actual.feet.height_m = 1500.0;
    tracker
        .update(
            &mut actual,
            false,
            epoch().shifted(10.0).unwrap(),
            turn,
            &scene,
        )
        .unwrap();
    assert!((actual.feet.lon_deg - 10.0).abs() < 1e-12);
}

#[test]
fn roll_is_preserved_by_both_flight_paths_and_crouching_keeps_feet() {
    let scene = TerrainScene::new(vec![]);
    let mut direct = observer(89.99, 170.0, 200_000.0);
    direct.speed_mps = 100_000.0;
    let original = direct.frame();
    let mut terrain = direct;
    direct.fly(1.0, 0.3, 0.0, 0.25, false);
    scene.fly(&mut terrain, 1.0, 0.3, 0.0, 0.25, false);
    axes_close(direct.frame(), original, 2e-14);
    axes_close(terrain.frame(), original, 2e-14);
    vector_close(direct.eye_position(), terrain.eye_position(), 2e-8);
    let feet = direct.feet;
    EyePosture {
        crouched: true,
        ..EyePosture::default()
    }
    .apply(&mut direct)
    .unwrap();
    assert_eq!(direct.feet, feet);
    axes_close(direct.frame(), original, 2e-14);
}

#[test]
fn coordinate_ceiling_rejects_transport_without_clamping_while_locked_pose_is_unchanged() {
    let scene = TerrainScene::new(vec![]);
    let mut actual = observer(0.0, 0.0, MAX_HEIGHT_M - 100.0);
    let initial = actual;
    let mut tracker = ObserverTracking::new(actual, epoch(), Mat3d::identity()).unwrap();
    // Map equatorial radius to the polar axis: geodetic height increases by
    // a-b, so this valid starting eye would exceed the supported height limit.
    let turn = Mat3d {
        rows: [[0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]],
    };
    assert_eq!(
        tracker.update(
            &mut actual,
            false,
            epoch().shifted(100.0).unwrap(),
            turn,
            &scene
        ),
        Err(TrackingError::InvalidPose)
    );
    assert_eq!(actual, initial);
    assert_eq!(tracker.instant(), epoch());
    tracker
        .update(
            &mut actual,
            true,
            epoch().shifted(86400.0 * 365.0).unwrap(),
            turn,
            &scene,
        )
        .unwrap();
    assert_eq!(actual, initial);
}
