//! Independent GPU presentation geometry checks. These verify geometry and
//! coordinate conventions without treating finite raster meshes as surveys.
use earth_math::rendering::*;
use earth_math::*;
use std::collections::BTreeMap;

fn geo(lat: f64, lon: f64, height: f64) -> Geodetic {
    Geodetic::new(lat, lon, height).unwrap()
}
fn vector(v: [f32; 3]) -> Vec3d {
    Vec3d::new(f64::from(v[0]), f64::from(v[1]), f64::from(v[2]))
}
fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        actual.is_finite() && (actual - expected).abs() <= tolerance,
        "actual={actual}, expected={expected}, tolerance={tolerance}"
    );
}

#[test]
fn floating_origin_preserves_eye_position_millimetres_and_camera_handedness() {
    for (lat, lon) in [(0.0, 0.0), (60.0, 170.0), (-80.0, -179.9), (90.0, 23.0)] {
        let mut observer = Observer::new(geo(lat, lon, 12.0));
        observer.heading_deg = 90.0;
        let frame = LocalFrame::at(observer);
        assert_eq!(frame.position(observer.eye_position()), [0.0; 3]);
        let (origin, forward, up) = frame.camera(observer.frame());
        assert_eq!(origin, [0.0; 3]);
        close(
            (vector(forward) - Vec3d::new(1.0, 0.0, 0.0)).length(),
            0.0,
            1e-7,
        );
        close((vector(up) - Vec3d::new(0.0, 1.0, 0.0)).length(), 0.0, 1e-7);
        let enu = EnuBasis::at(observer.feet);
        let point = observer.eye_position() + enu.east * 0.003 - enu.up * 0.002 - enu.north * 0.004;
        close(
            (vector(frame.position(point)) - Vec3d::new(0.003, -0.002, 0.004)).length(),
            0.0,
            3e-9,
        );
        let east = vector(frame.direction(enu.east));
        let north = vector(frame.direction(enu.north));
        close(
            (east.cross(north) - vector(frame.direction(enu.up))).length(),
            0.0,
            1e-7,
        );
        EyePosture {
            crouched: true,
            ..EyePosture::default()
        }
        .apply(&mut observer)
        .unwrap();
        close(
            vector(frame.position(observer.eye_position())).y,
            -0.72,
            4e-8,
        );
    }
}

fn patch_with_peak(peak: f64) -> TerrainPatch {
    let mut heights = vec![0.0; 81];
    heights[4 * 9 + 4] = peak;
    TerrainPatch::from_grid(9, 9, -0.004, 0.004, 0.001, 0.001, heights).unwrap()
}

#[test]
fn nearby_render_mesh_uses_every_canonical_post_and_identical_triangles() {
    let patch = patch_with_peak(25.0);
    let observer = Observer::new(geo(0.0, 0.0, 100.0));
    let frame = LocalFrame::at(observer);
    let (mesh, stride) = terrain_mesh(&patch, frame, focal_pixels(60.0, 900));
    assert_eq!(stride, 1);
    let (width, height, points) = patch.presentation_grid();
    assert_eq!(mesh.positions.len(), points.len());
    assert_eq!(mesh.indices.len(), 6 * (width - 1) * (height - 1));
    for (point, presented) in points.iter().zip(&mesh.positions) {
        assert_eq!(*presented, frame.position(*point));
    }
    let mut expected = Vec::new();
    for row in 0..height - 1 {
        for col in 0..width - 1 {
            let nw = (row * width + col) as u32;
            let ne = nw + 1;
            let sw = nw + width as u32;
            let se = sw + 1;
            expected.extend([nw, sw, se, nw, se, ne]);
        }
    }
    assert_eq!(mesh.indices, expected);
    for face in mesh.indices.chunks_exact(3) {
        let [a, b, c] = [face[0], face[1], face[2]].map(|i| vector(mesh.positions[i as usize]));
        assert!((b - a).cross(c - a).y > 0.0);
    }
}

#[test]
fn lod_selection_checks_interior_peaks_and_keeps_source_boundary_posts() {
    let flat = patch_with_peak(0.0);
    let rough = patch_with_peak(20_000.0);
    let eye = Vec3d::new(WGS84_A_M + 1_000_000.0, 0.0, 0.0);
    let (w, h, p) = flat.presentation_grid();
    assert!(patch_stride(p, w, h, eye, 1000.0) > 1);
    let (w, h, p) = rough.presentation_grid();
    assert_eq!(patch_stride(p, w, h, eye, 1000.0), 1);
    let frame = LocalFrame::at(Observer::new(geo(0.0, 0.0, 100_000_000.0)));
    let (mesh, stride) = terrain_mesh(&flat, frame, 100.0);
    assert!(stride > 1);
    let (_, _, posts) = flat.presentation_grid();
    for index in [0, 8, 72, 80] {
        assert!(mesh.positions.contains(&frame.position(posts[index])));
    }
}

#[test]
fn adaptive_reference_ellipsoid_has_outward_winding_and_no_unpaired_edges() {
    let scene = TerrainScene::default();
    let frame = LocalFrame::at(Observer::new(geo(0.0, 0.0, 12_000_000.0)));
    let mesh = reference_mesh(&scene, frame, 50.0);
    assert!(!mesh.indices.is_empty());
    // Weld independently by physical position, including the periodic date
    // line and coincident polar posts. Local f32 repeats are bit-identical
    // except for sub-nanometre sin(pi) remnants at the date line.
    let key = |index: u32| -> [i64; 3] {
        mesh.positions[index as usize].map(|v| (f64::from(v) * 10_000.0).round() as i64)
    };
    let mut edges: BTreeMap<([i64; 3], [i64; 3]), usize> = BTreeMap::new();
    let mut tested = 0;
    for face in mesh.indices.chunks_exact(3) {
        let k = [key(face[0]), key(face[1]), key(face[2])];
        if k[0] == k[1] || k[1] == k[2] || k[2] == k[0] {
            continue; // Collapsed polar cell sides carry no surface area.
        }
        let p = [face[0], face[1], face[2]].map(|i| vector(mesh.positions[i as usize]));
        let normal = (p[1] - p[0]).cross(p[2] - p[0]);
        let outward = [face[0], face[1], face[2]]
            .map(|i| vector(mesh.normals[i as usize]))
            .into_iter()
            .fold(Vec3d::ZERO, |sum, n| sum + n);
        assert!(normal.dot(outward) > 0.0, "inward reference face");
        for (a, b) in [(k[0], k[1]), (k[1], k[2]), (k[2], k[0])] {
            *edges
                .entry(if a < b { (a, b) } else { (b, a) })
                .or_default() += 1;
        }
        tested += 1;
    }
    assert!(tested > 100);
    assert!(
        edges.values().all(|count| *count == 2),
        "reference LOD produced an unpaired or overlapping edge"
    );
}

#[test]
fn decorative_grass_bases_follow_the_canonical_slope_and_stay_fixed_during_wind() {
    // Prescribed Cartesian plane X - 0.3Y = a+20, solved independently at
    // each geodetic post. Every canonical triangle lies on this plane.
    let mut heights = Vec::new();
    for row in 0..3 {
        for col in 0..3 {
            let phi = (0.001 - f64::from(row) * 0.001).to_radians();
            let lambda = (-0.001 + f64::from(col) * 0.001).to_radians();
            let n = WGS84_A_M / (1.0 - WGS84_E2 * phi.sin().powi(2)).sqrt();
            heights
                .push((WGS84_A_M + 20.0) / (phi.cos() * (lambda.cos() - 0.3 * lambda.sin())) - n);
        }
    }
    let patch = TerrainPatch::from_grid(3, 3, -0.001, 0.001, 0.001, 0.001, heights).unwrap();
    let scene = TerrainScene::new(vec![patch]);
    let mut observer = Observer::new(geo(0.0, 0.0, 0.0));
    scene
        .place_on_ground(&mut observer, geo(0.0, 0.0, 0.0))
        .unwrap();
    let blades = grass_blades(&scene, observer);
    assert!(blades.len() > 1000);
    for blade in &blades {
        close(blade.base.x - 0.3 * blade.base.y, WGS84_A_M + 20.0, 3e-8);
        close(blade.up.length(), 1.0, 1e-14);
        assert!((0.045..=0.19).contains(&blade.height_m));
        if blade.height_m < 0.075 {
            assert!((4.0..=6.0).contains(&blade.cutoff_m));
            assert!(blade.base.distance(observer.eye_position()) <= blade.cutoff_m + 5.0);
        }
        if blade.cutoff_m < 6.0 {
            assert!((0.045..=0.09).contains(&blade.height_m));
            assert!((0.014..=0.024).contains(&blade.width_m));
        }
    }
    assert!(blades.iter().any(|blade| blade.height_m < 0.075));
    assert!(blades.iter().any(|blade| blade.height_m > 0.09));
    let frame = LocalFrame::at(observer);
    let first = grass_mesh(&blades, frame, 0.0);
    let later = grass_mesh(&blades, frame, 0.75);
    assert_eq!(first.indices, later.indices);
    for (index, blade) in blades.iter().enumerate().take(100) {
        let center = (vector(first.positions[GRASS_VERTEX_COUNT * index])
            + vector(first.positions[GRASS_VERTEX_COUNT * index + 1]))
            * 0.5;
        close(
            center.distance(vector(frame.position(blade.base))),
            0.0,
            2e-6,
        );
        assert_eq!(
            first.positions[GRASS_VERTEX_COUNT * index],
            later.positions[GRASS_VERTEX_COUNT * index]
        );
        assert_eq!(
            first.positions[GRASS_VERTEX_COUNT * index + 1],
            later.positions[GRASS_VERTEX_COUNT * index + 1]
        );
    }
    assert!(
        first
            .positions
            .chunks_exact(GRASS_VERTEX_COUNT)
            .zip(later.positions.chunks_exact(GRASS_VERTEX_COUNT))
            .any(|(a, b)| a[8] != b[8])
    );
}

#[test]
fn grass_lattice_is_world_fixed_across_northward_camera_motion() {
    let scene = TerrainScene::default();
    let first = grass_blades(&scene, Observer::new(geo(45.0, 120.0, 0.0)));
    let moved = grass_blades(&scene, Observer::new(geo(45.0001, 120.0, 0.0)));
    let key = |blade: &GrassBlade| {
        [
            blade.base.x.to_bits(),
            blade.base.y.to_bits(),
            blade.base.z.to_bits(),
        ]
    };
    let fixed: std::collections::BTreeSet<_> = first.iter().map(key).collect();
    let common = moved
        .iter()
        .filter(|blade| fixed.contains(&key(blade)))
        .count();
    assert!(
        common > 500,
        "only {common} world anchors survived an eleven-metre move"
    );
}

#[test]
fn grass_range_matches_piecewise_planar_wind_surface_and_nonunit_rays() {
    let blade = GrassBlade {
        base: Vec3d::new(WGS84_A_M + 1.0, 0.0, 0.0),
        up: Vec3d::new(1.0, 0.0, 0.0),
        side: Vec3d::new(0.0, 1.0, 0.0),
        wind: Vec3d::new(0.0, 0.0, 1.0),
        lean: Vec3d::ZERO,
        height_m: 1.0,
        width_m: 0.1,
        phase: 0.0,
        gust_phase: 0.0,
        cutoff_m: 12.0,
        color: [0.0, 0.5, 0.0, 1.0],
    };
    for time in [0.0, 0.75] {
        let wind = grass_wind(time);
        let tip_z = f64::from(wind[0]) * 0.14 + f64::from(wind[2]) * 0.039;
        for height in [0.125_f64, 0.625, 0.875] {
            // The centerline samples a quadratic at quarter-height knots.
            // Each actual triangle interpolates linearly between those knots.
            let low = (height * 4.0).floor() / 4.0;
            let high = low + 0.25;
            let quadratic = low * low + (height - low) * 4.0 * (high * high - low * low);
            let z = quadratic * tip_z;
            let y = quadratic * f64::from(wind[2]) * 0.021;
            for scale in [1e-200, 1.0, 7.0, 1e200] {
                let ray = Ray {
                    origin: Vec3d::new(WGS84_A_M + 1.0 + height, y, 1.0),
                    direction: Vec3d::new(0.0, 0.0, -scale),
                };
                let hit = grass_intersection(std::slice::from_ref(&blade), time, ray).unwrap();
                close(hit.distance_m, 1.0 - z, 2e-9);
                close(hit.position.z, z, 2e-9);
                close(hit.normal.length(), 1.0, 1e-14);
            }
        }
    }
    let front = Ray {
        origin: Vec3d::new(WGS84_A_M + 1.25, 0.0, 1.0),
        direction: Vec3d::new(0.0, 0.0, -1.0),
    };
    let mut closer = blade.clone();
    closer.base.z = 0.4;
    for blades in [[blade.clone(), closer.clone()], [closer, blade.clone()]] {
        close(
            grass_intersection(&blades, 0.0, front).unwrap().distance_m,
            0.6,
            1e-14,
        );
    }
    let back = Ray {
        origin: Vec3d::new(WGS84_A_M + 1.25, 0.0, -1.0),
        direction: Vec3d::new(0.0, 0.0, 1.0),
    };
    close(
        grass_intersection(std::slice::from_ref(&blade), 0.0, back)
            .unwrap()
            .distance_m,
        1.0,
        1e-14,
    );
    for miss in [
        Ray {
            origin: Vec3d::new(WGS84_A_M + 1.25, 0.06, 1.0),
            ..front
        },
        Ray {
            direction: Vec3d::new(0.0, 1.0, 0.0),
            ..front
        },
        Ray {
            direction: Vec3d::ZERO,
            ..front
        },
    ] {
        assert!(grass_intersection(std::slice::from_ref(&blade), 0.0, miss).is_none());
    }
}

#[test]
fn grass_anchors_wrap_at_the_date_line_and_remain_unique_at_both_poles() {
    let scene = TerrainScene::default();
    let keys = |blades: &[GrassBlade]| -> std::collections::BTreeSet<[u64; 3]> {
        blades
            .iter()
            .map(|blade| {
                assert!(blade.base.is_finite());
                [
                    blade.base.x.to_bits(),
                    blade.base.y.to_bits(),
                    blade.base.z.to_bits(),
                ]
            })
            .collect()
    };
    let west = grass_blades_with_quality(&scene, Observer::new(geo(0.0, 179.999999, 0.0)), 0);
    let east = grass_blades_with_quality(&scene, Observer::new(geo(0.0, -179.999999, 0.0)), 0);
    let west_keys = keys(&west);
    let east_keys = keys(&east);
    assert_eq!(west_keys.len(), west.len(), "duplicate date-line anchors");
    assert_eq!(east_keys.len(), east.len(), "duplicate date-line anchors");
    assert!(west_keys.intersection(&east_keys).count() > west.len() * 9 / 10);
    assert!(west.iter().any(|b| b.base.y > 0.0));
    assert!(west.iter().any(|b| b.base.y < 0.0));
    for latitude in [-90.0, 90.0] {
        let first = grass_blades_with_quality(&scene, Observer::new(geo(latitude, 0.0, 0.0)), 0);
        let rotated =
            grass_blades_with_quality(&scene, Observer::new(geo(latitude, 123.0, 0.0)), 0);
        let first_keys = keys(&first);
        assert!(first.len() > 1000);
        assert_eq!(first_keys.len(), first.len(), "duplicate polar anchors");
        assert_eq!(first_keys, keys(&rotated), "longitude moved a polar anchor");
    }
}

#[test]
fn grass_wind_stays_bounded_and_distance_fade_has_smooth_endpoints() {
    for time in [0.0, -0.125, 1.0, -1e12, 1e12, f64::MIN, f64::MAX] {
        let weights = grass_wind(time);
        assert!(weights.iter().all(|v| v.is_finite() && v.abs() <= 1.0));
        for pair in weights.chunks_exact(2) {
            close(
                f64::from(pair[0]).powi(2) + f64::from(pair[1]).powi(2),
                1.0,
                9e-8,
            );
        }
    }
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(grass_wind(invalid), grass_wind(0.0));
    }
    for (frequency, offset) in [(1.35, 0), (3.8, 2)] {
        let time = 0.375;
        let first = grass_wind(time);
        let next = grass_wind(time + std::f64::consts::TAU / frequency);
        assert_eq!(&first[offset..offset + 2], &next[offset..offset + 2]);
    }
    assert_eq!(grass_fade(0.0, 12.0), 1.0);
    assert_eq!(grass_fade(9.0, 12.0), 1.0);
    assert_eq!(grass_fade(10.5, 12.0), 0.5);
    assert_eq!(grass_fade(12.0, 12.0), 0.0);
    assert_eq!(grass_fade(15.0, 12.0), 0.0);
    let fades: Vec<_> = (0..=300)
        .map(|index| grass_fade(9.0 + index as f32 * 0.01, 12.0))
        .collect();
    assert!(fades.windows(2).all(|pair| pair[0] >= pair[1]));
    // A small finite difference at each endpoint is quadratic, rather than
    // the O(epsilon) change of a visibly abrupt linear transition.
    assert!(1.0 - grass_fade(9.001, 12.0) < 1e-6);
    assert!(grass_fade(11.999, 12.0) < 1e-6);
}

fn fixture_blade() -> GrassBlade {
    GrassBlade {
        base: Vec3d::new(WGS84_A_M + 1.0, 0.0, 0.0),
        up: Vec3d::new(1.0, 0.0, 0.0),
        side: Vec3d::new(0.0, 1.0, 0.0),
        wind: Vec3d::new(0.0, 0.0, 1.0),
        lean: Vec3d::new(0.0, 0.0, 0.2),
        height_m: 1.0,
        width_m: 0.1,
        phase: 0.0,
        gust_phase: 0.0,
        cutoff_m: 12.0,
        color: [0.0, 0.5, 0.0, 1.0],
    }
}

#[test]
fn grass_upload_has_fixed_roots_tapered_knots_and_expected_wind_coefficients() {
    let blade = fixture_blade();
    let local = LocalFrame::at(Observer::new(geo(0.0, 0.0, 0.0)));
    let upload = grass_render_mesh(std::slice::from_ref(&blade), local);
    assert_eq!(upload.mesh.positions.len(), 9);
    assert_eq!(upload.mesh.indices.len(), 21);
    let root = vector(local.position(blade.base));
    for i in [0, 1] {
        for coefficients in [
            &upload.wind_a,
            &upload.wind_b,
            &upload.wind_c,
            &upload.wind_d,
        ] {
            assert_eq!(coefficients[i], [0.0; 3]);
        }
    }
    for (left, right, stage) in [(0, 1, 0.0_f64), (2, 3, 0.25), (4, 5, 0.5), (6, 7, 0.75)] {
        let center =
            (vector(upload.mesh.positions[left]) + vector(upload.mesh.positions[right])) * 0.5;
        // At the equator local axes are ECEF Y, X and -Z respectively.
        let expected = root + Vec3d::new(0.0, stage, -0.2 * stage * stage);
        close(center.distance(expected), 0.0, 8e-8);
        close(
            vector(upload.mesh.positions[right]).distance(vector(upload.mesh.positions[left])),
            0.1 * (1.0 - stage) * (1.0 + 0.5 * stage),
            1e-8,
        );
        close(vector(upload.wind_a[left]).z, -0.14 * stage * stage, 1e-8);
        close(vector(upload.wind_c[left]).x, 0.021 * stage * stage, 1e-8);
        close(vector(upload.wind_c[left]).z, -0.039 * stage * stage, 1e-8);
        assert_eq!(upload.wind_b[left], [0.0; 3]);
        assert_eq!(upload.wind_d[left], [0.0; 3]);
    }
    close(
        vector(upload.mesh.positions[8]).distance(root + Vec3d::new(0.0, 1.0, -0.2)),
        0.0,
        8e-8,
    );
    for normal in upload.mesh.normals {
        close(vector(normal).length(), 1.0, 1e-7);
    }
}

fn local_to_world(frame: LocalFrame, p: [f32; 3]) -> Vec3d {
    frame.origin + frame.basis.east * f64::from(p[0]) + frame.basis.up * f64::from(p[1])
        - frame.basis.north * f64::from(p[2])
}

/// Independent plane/half-space oracle, avoiding the production barycentric
/// determinant formula and deliberately performing no blade broad-phase cull.
fn triangle_range(ray: Ray, triangle: [Vec3d; 3]) -> Option<f64> {
    let direction = ray.direction.normalized()?;
    let [a, b, c] = triangle;
    let normal = (b - a).cross(c - a).normalized()?;
    let denominator = normal.dot(direction);
    if denominator.abs() < 1e-12 {
        return None;
    }
    let distance = (a - ray.origin).dot(normal) / denominator;
    if distance < 0.0 {
        return None;
    }
    let point = ray.origin + direction * distance;
    for (start, end) in [(a, b), (b, c), (c, a)] {
        if (end - start).cross(point - start).dot(normal) < -1e-10 {
            return None;
        }
    }
    Some(distance)
}

#[test]
fn grass_rendered_range_matches_uploaded_triangles_across_wind_fade_and_rebases() {
    let blade = fixture_blade();
    let mut farther = blade.clone();
    farther.base.z -= 0.7;
    let blades = [farther, blade];
    for rebase in [0.0, 250.0] {
        let mut local = LocalFrame::at(Observer::new(geo(0.0, 0.0, 0.0)));
        local.origin = local.origin + local.basis.east * rebase;
        let upload = grass_render_mesh(&blades, local);
        let bounds = grass_ray_bounds(&blades, local);
        for eye_distance in [2.0, 10.5, 13.0] {
            let eye = Vec3d::new(WGS84_A_M + 1.5, 0.0, eye_distance);
            for weights in [
                [0.0, 1.0, 0.0, 1.0],
                [1.0; 4],
                [-1.0; 4],
                [1.0, -1.0, -1.0, 1.0],
                grass_wind(0.75),
            ] {
                let points: Vec<_> = upload
                    .mesh
                    .positions
                    .iter()
                    .enumerate()
                    .map(|(i, rest)| {
                        local_to_world(
                            local,
                            grass_deform_vertex(
                                *rest,
                                [
                                    upload.wind_a[i],
                                    upload.wind_b[i],
                                    upload.wind_c[i],
                                    upload.wind_d[i],
                                ],
                                upload.roots[i],
                                weights,
                                local.position(eye),
                            ),
                        )
                    })
                    .collect();
                // Aim into the uppermost actual triangle of the nearer blade.
                let start = GRASS_VERTEX_COUNT;
                let target = (points[start + 6] + points[start + 7] + points[start + 8]) / 3.0;
                for scale in [1e-100, 1.0, 1e100] {
                    let ray = Ray {
                        origin: eye,
                        direction: (target - eye) * scale,
                    };
                    let expected = upload
                        .mesh
                        .indices
                        .chunks_exact(3)
                        .filter_map(|face| {
                            triangle_range(
                                ray,
                                [
                                    points[face[0] as usize],
                                    points[face[1] as usize],
                                    points[face[2] as usize],
                                ],
                            )
                        })
                        .min_by(f64::total_cmp);
                    let actual = grass_intersection_rendered(&blades, local, weights, ray);
                    let cached = grass_intersection_cached(&blades, local, &bounds, weights, ray);
                    assert_eq!(
                        actual.as_ref().map(|hit| hit.distance_m),
                        cached.as_ref().map(|hit| hit.distance_m)
                    );
                    if eye_distance < 12.0 {
                        assert!(expected.is_some(), "fixture must strike visible geometry");
                    } else {
                        assert!(expected.is_none(), "fully faded blades have no area");
                    }
                    match (actual, expected) {
                        (Some(hit), Some(expected)) => close(hit.distance_m, expected, 3e-8),
                        (None, None) => {}
                        mismatch => panic!("rendered blade/oracle mismatch: {mismatch:?}"),
                    }
                }
            }
        }
    }
}

#[test]
fn grass_placement_cache_preserves_ground_queries_and_clears_between_scenes() {
    let baseline = TerrainScene::default();
    let raised = TerrainScene::new(vec![
        TerrainPatch::from_grid(3, 3, -0.001, 0.001, 0.001, 0.001, vec![20.0; 9]).unwrap(),
    ]);
    let mut cache = GrassPlacementCache::default();
    let initial = Observer::new(geo(0.0, 0.0, 0.0));
    assert!(!cache.rebuild(&baseline, initial, 0).is_empty());
    for (scene, observer) in [
        (&baseline, Observer::new(geo(0.00002, 0.0, 0.0))),
        (&raised, Observer::new(geo(0.00002, 0.0, 20.0))),
    ] {
        if observer.feet.height_m > 0.0 {
            cache.clear();
        }
        let retained = cache.rebuild(scene, observer, 0);
        let fresh = grass_blades_with_quality(scene, observer, 0);
        assert_eq!(retained.len(), fresh.len());
        for (a, b) in retained.iter().zip(&fresh) {
            for (actual, expected) in [
                (a.base, b.base),
                (a.up, b.up),
                (a.side, b.side),
                (a.wind, b.wind),
                (a.lean, b.lean),
            ] {
                assert_eq!(actual, expected);
            }
            assert_eq!(a.height_m, b.height_m);
            assert_eq!(a.width_m, b.width_m);
            assert_eq!(a.phase, b.phase);
            assert_eq!(a.gust_phase, b.gust_phase);
            assert_eq!(a.cutoff_m, b.cutoff_m);
            assert_eq!(a.color, b.color);
            if observer.feet.height_m > 0.0 {
                assert!(ecef_to_geodetic(a.base).unwrap().height_m > 19.99);
            }
        }
    }
}
