use earth_math::*;

fn scene() -> TerrainScene {
    TerrainScene::new(vec![
        TerrainPatch::from_bytes(include_bytes!("../../../assets/terrain/fuji.aet")).unwrap(),
        TerrainPatch::from_bytes(include_bytes!("../../../assets/terrain/tokyo.aet")).unwrap(),
        TerrainPatch::from_bytes(include_bytes!("../../../assets/terrain/houston.aet")).unwrap(),
        TerrainPatch::from_bytes(include_bytes!("../../../assets/terrain/galveston.aet")).unwrap(),
    ])
}

#[test]
fn archived_source_patches_load_and_vertical_rays_hit_their_surface() {
    let scene = scene();
    assert_eq!(scene.patches()[0].vertex_count(), 265 * 217);
    assert_eq!(scene.patches()[1].vertex_count(), 265 * 216);
    assert_eq!(scene.patches()[2].vertex_count(), 249 * 217);
    assert_eq!(scene.patches()[3].vertex_count(), 247 * 216);
    // Historical surface fixtures remain test-local after removing all runtime
    // location presets. Sample each imported patch at its geometric center.
    for (index, patch) in scene.patches().iter().enumerate() {
        let [west, south, east, north] = patch.bounds();
        let location = Geodetic::new((south + north) * 0.5, (west + east) * 0.5, 0.0).unwrap();
        let ground = scene.ground(location).unwrap();
        let mut observer = Observer::new(
            ground
                .hit
                .geodetic
                .with_height(ground.hit.geodetic.height_m + 1200.0)
                .unwrap(),
        );
        observer.pitch_deg = -90.0;
        let surface = scene.intersect(observer.frame().center_ray()).unwrap();
        assert_eq!(surface.kind, SurfaceKind::Terrain(index));
        assert!((surface.hit.distance_m - 1201.72).abs() < 2e-7);
    }
    // Sanity bounds, not a survey reference: the summit sample is kilometres
    // above Tokyo's lowland DSM. Exact post/height provenance lives in JSON.
    let fuji = scene
        .ground(Geodetic::new(35.36, 138.73, 0.0).unwrap())
        .unwrap();
    let tokyo = scene
        .ground(Geodetic::new(35.6895, 139.69171, 0.0).unwrap())
        .unwrap();
    assert!(fuji.hit.geodetic.height_m > 3500.0);
    assert!((30.0..150.0).contains(&tokyo.hit.geodetic.height_m));
}

#[test]
fn terrain_render_smoke_uses_both_projections_and_exact_display_aspect() {
    let scene = scene();
    let mut observer = Observer::new(Geodetic::new(35.36, 138.73, 10_000.0).unwrap());
    observer.pitch_deg = -90.0;
    for projection in [Projection::Rectilinear, Projection::EquidistantFisheye] {
        let lens = Lens::new(projection, 60.0);
        let mut surface_pixels = 0;
        for y in 0..32 {
            for x in 0..48 {
                let color = scene.render_pixel(
                    observer.frame(),
                    lens,
                    x,
                    y,
                    48,
                    32,
                    [1439.0, 901.0],
                    None,
                    &[],
                );
                assert_eq!(color[3], 255);
                if color != [9, 17, 30, 255] && color != [4, 7, 14, 255] {
                    surface_pixels += 1;
                }
            }
        }
        assert!(
            surface_pixels > 100,
            "{projection:?} had only {surface_pixels} surface pixels"
        );
    }
}

#[test]
fn web_mercator_local_texture_registration_uses_mercator_not_linear_latitude() {
    let image = TextureView {
        rgba: &[0, 0, 0, 255],
        width: 1,
        height: 1,
    };
    let texture = PatchTextureView {
        texture: image,
        projection: TextureProjection::WebMercator,
        west_deg: -10.0,
        south_deg: 0.0,
        east_deg: 10.0,
        north_deg: 60.0,
    };
    let center = texture.uv(Geodetic::new(30.0, 0.0, 0.0).unwrap()).unwrap();
    // Independent expected Mercator y: ln(tan(pi/4+phi/2)).
    let mercator = |latitude: f64| {
        (std::f64::consts::FRAC_PI_4 + latitude.to_radians() / 2.0)
            .tan()
            .ln()
    };
    let expected = (mercator(60.0) - mercator(30.0)) / mercator(60.0);
    assert!((center[0] - 0.5).abs() < 1e-15);
    assert!((center[1] - expected).abs() < 2e-15);
    assert!((center[1] - 0.5).abs() > 0.05);
    assert_eq!(
        texture.uv(Geodetic::new(60.0, -10.0, 0.0).unwrap()),
        Some([0.0, 0.0])
    );
    assert_eq!(
        texture.uv(Geodetic::new(0.0, 10.0, 0.0).unwrap()),
        Some([1.0, 1.0])
    );
    assert!(
        texture
            .uv(Geodetic {
                lat_deg: f64::NAN,
                lon_deg: 0.0,
                height_m: 0.0
            })
            .is_none()
    );
}
