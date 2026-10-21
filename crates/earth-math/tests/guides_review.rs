//! Independent analytic ellipsoid and geometric ruler constraints.
use earth_math::{guides::*, *};

fn observer() -> Observer {
    let mut observer = Observer::new(Geodetic::new(0.0, 0.0, 100_000.0).unwrap());
    observer.heading_deg = 90.0;
    observer
}
fn viewport() -> GuideViewport {
    GuideViewport::new([1920.0, 1080.0], [1.0, 1.0]).unwrap()
}
fn close(a: f64, b: f64, tolerance: f64) {
    assert!(
        (a - b).abs() <= tolerance,
        "{a:.15} != {b:.15}; tolerance {tolerance}"
    );
}
fn profile(observer: Observer, lens: Lens, viewport: GuideViewport) -> HorizonProfile {
    HorizonProfile::measure(&TerrainScene::new(vec![]), observer.frame(), lens, viewport).unwrap()
}

#[test]
fn rectangular_horizon_apex_matches_independently_derived_equatorial_tangent_cone() {
    let observer = observer();
    let lens = Lens::default();
    let profile = profile(observer, lens, viewport());
    // With eye=(r,0,0), forward=+Y, up=+X, and image horizontal
    // coefficient u, the WGS84 tangent cone has image v<0 satisfying
    // v²=(r²/a²-1)*(1+a²*u²/b²). Its highest image point is u=0.
    let r = WGS84_A_M + observer.eye_geodetic().height_m;
    let k = ((r / WGS84_A_M).powi(2) - 1.0).sqrt();
    let focal = 540.0 / (lens.vertical_fov_deg.to_radians() * 0.5).tan();
    let expected_row = 540.0 + focal * k;
    close(profile.apex.pixel[1], expected_row, 0.03);
    close(profile.left.pixel[0], 0.0, 0.03);
    close(profile.right.pixel[0], 1920.0, 0.03);
    for point in [profile.apex, profile.left, profile.right] {
        assert!(point.bracket_pixels <= 0.025);
        let u = (point.pixel[0] - 960.0) / focal;
        let v = -(k * k * (1.0 + WGS84_A_M.powi(2) * u * u / WGS84_B_M.powi(2))).sqrt();
        close(point.pixel[1], 540.0 - focal * v, 0.03);
        let p = point.hit.hit.position;
        close(
            (p.x * p.x + p.y * p.y) / WGS84_A_M.powi(2) + p.z * p.z / WGS84_B_M.powi(2),
            1.0,
            2e-14,
        );
    }
    let width = profile.chord_width_m.unwrap();
    assert!(width > 100_000.0 && width < 2.0 * WGS84_A_M);
    close(
        width,
        profile
            .left
            .hit
            .hit
            .position
            .distance(profile.right.hit.hit.position),
        1e-8,
    );
    close(
        profile.surface_width_m.unwrap(),
        wgs84_surface_distance(
            profile.left.hit.hit.geodetic,
            profile.right.hit.hit.geodetic,
        )
        .unwrap(),
        1e-8,
    );
    assert!(profile.surface_width_m.unwrap() > width);
}

#[test]
fn ruler_is_a_camera_depth_plane_gap_and_projects_to_the_two_declared_endpoints() {
    let scene = TerrainScene::new(vec![]);
    for (roll, projection, fov, pitch) in [
        (0.0, Projection::Rectilinear, 70.0, 0.0),
        (31.0, Projection::Rectilinear, 70.0, -10.0),
        (18.0, Projection::EquidistantFisheye, 140.0, -20.0),
        (0.0, Projection::Rectilinear, 5.0, -10.08),
    ] {
        let mut observer = observer();
        observer.roll_deg = roll;
        observer.pitch_deg = pitch;
        let frame = observer.frame();
        let lens = Lens::new(projection, fov);
        let profile = profile(observer, lens, viewport());
        let mut valid = 0;
        for column in [0.1, 0.3, 0.5, 0.7, 0.9] {
            let Some(ruler) = profile.ruler(&scene, column) else {
                continue;
            };
            valid += 1;
            let ground = ruler.ground.hit.hit.position;
            let reference = ruler.reference_ecef_m;
            close((reference - ground).dot(frame.forward), 0.0, 3e-9);
            close(ruler.view_plane_gap_m, reference.distance(ground), 1e-8);
            close(ruler.eye_range_m, frame.origin.distance(ground), 2e-8);
            assert!(ruler.top_pixel[1] <= ruler.ground.pixel[1]);
            assert!(ruler.ground.bracket_pixels <= 0.025);
            for (point, pixel) in [(reference, ruler.top_pixel), (ground, ruler.ground.pixel)] {
                let ndc = lens
                    .project(frame, point - frame.origin, 1920.0 / 1080.0)
                    .unwrap();
                close((ndc[0] + 1.0) * 960.0, pixel[0], 2e-8);
                close((1.0 - ndc[1]) * 540.0, pixel[1], 2e-8);
            }
        }
        assert!(valid > 0, "projection={projection:?} roll={roll}");
    }
}

#[test]
fn screen_guides_keep_one_physical_pixel_at_normal_and_fractional_display_scale() {
    let scene = TerrainScene::new(vec![]);
    let lens = Lens::default();
    let observer = observer();
    let a = profile(observer, lens, viewport());
    let b = profile(
        observer,
        lens,
        GuideViewport::new([1280.0, 720.0], [1.5, 1.5]).unwrap(),
    );
    close(a.apex.pixel[1], b.apex.pixel[1], 1e-10);
    close(a.chord_width_m.unwrap(), b.chord_width_m.unwrap(), 1e-8);
    for profile in [a, b] {
        let h = profile.horizontal_rect();
        let sx = f64::from(profile.viewport.scale[0]);
        let sy = f64::from(profile.viewport.scale[1]);
        close(f64::from(h.max[1] - h.min[1]) * sy, 1.0, 1e-4);
        close(
            f64::from(h.min[1]) * sy,
            (profile.apex.pixel[1]).floor(),
            1e-4,
        );
        let ruler = profile.ruler(&scene, 0.8).unwrap();
        let v = profile.vertical_rect(ruler);
        close(f64::from(v.max[0] - v.min[0]) * sx, 1.0, 1e-4);
        assert!(v.max[1] >= v.min[1]);
    }
}

#[test]
fn absent_boundaries_invalid_viewports_and_fisheye_aperture_do_not_invent_distances() {
    let scene = TerrainScene::new(vec![]);
    for (display, scale) in [
        ([0.0, 1080.0], [1.0, 1.0]),
        ([1920.0, f32::NAN], [1.0, 1.0]),
        ([1920.0, 1080.0], [1.0, -1.0]),
    ] {
        assert!(GuideViewport::new(display, scale).is_none());
    }
    for pitch in [-90.0, 90.0] {
        let mut observer = observer();
        observer.feet.height_m = 2.0;
        observer.pitch_deg = pitch;
        assert!(
            HorizonProfile::measure(&scene, observer.frame(), Lens::default(), viewport())
                .is_none()
        );
    }
    let p = profile(
        observer(),
        Lens::new(Projection::EquidistantFisheye, 140.0),
        viewport(),
    );
    for column in [f64::NAN, -0.1, 1.1, 0.0, 1.0] {
        assert!(p.ruler(&scene, column).is_none());
    }
    assert_eq!(column_from_screen(f32::NAN, 1920.0), None);
    assert_eq!(column_from_screen(960.0, 1920.0), Some(0.5));
    assert_eq!(move_column(0.5, f64::NAN, 1.0), 0.5);
    assert_eq!(move_column(0.5, 1.0, -1.0), 0.5);
}

#[test]
fn finite_terrain_outline_raises_guide_and_off_patch_columns_keep_reference_hits() {
    let mut observer = observer();
    observer.feet.height_m = 100.0;
    let lens = Lens::default();
    let baseline = profile(observer, lens, viewport());
    let patch = TerrainPatch::from_grid(3, 3, 0.24, 0.01, 0.01, 0.01, vec![2000.0; 9]).unwrap();
    let scene = TerrainScene::new(vec![patch]);
    let profile = HorizonProfile::measure(&scene, observer.frame(), lens, viewport()).unwrap();
    assert!(profile.apex.pixel[1] + 10.0 < baseline.apex.pixel[1]);
    assert!(matches!(profile.apex.hit.kind, SurfaceKind::Terrain(0)));
    let near = profile.ruler(&scene, 0.5).unwrap();
    assert!(matches!(near.ground.hit.kind, SurfaceKind::Terrain(0)));
    let far = profile.ruler(&scene, 0.9).unwrap();
    assert_eq!(far.ground.hit.kind, SurfaceKind::ReferenceEllipsoid);
    assert!(far.view_plane_gap_m > near.view_plane_gap_m);
    for point in [profile.apex, near.ground, far.ground] {
        assert!(point.bracket_pixels <= 0.025);
        let ray = Ray::new(
            observer.eye_position(),
            point.hit.hit.position - observer.eye_position(),
        )
        .unwrap();
        let actual = scene.intersect(ray).unwrap();
        assert_eq!(actual.kind, point.hit.kind);
        close(actual.hit.distance_m, point.hit.hit.distance_m, 1e-3);
    }
}
