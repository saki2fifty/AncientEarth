//! Independent irregular-mesh fixtures. A Cartesian pyramid with an inserted
//! edge vertex exposes dropped vertices, obsolete bounds and mismatched edges.
use earth_math::{
    Geodetic, Lens, Observer, Ray, SurfaceKind, TerrainPatch, TerrainScene, Vec3d, WGS84_A_M,
    WGS84_E2, WaterKind,
    astro::{Mat3d, ObserverTracking, SimulationClock, TrackingError},
    guides::{GuideViewport, HorizonProfile},
    rendering::{LocalFrame, RenderMesh, terrain_mesh, terrain_water_mesh},
};
use std::collections::{BTreeSet, HashMap};

type Face = ([usize; 3], u8);
type Fixture = (Vec<u8>, Vec<u8>, Vec<Vec3d>, Vec<Face>);

fn fixture() -> Fixture {
    let mut terrain = b"AETERR01".to_vec();
    terrain.extend(2_u32.to_le_bytes());
    terrain.extend(2_u32.to_le_bytes());
    for value in [-0.002_f64, 0.002, 0.004, 0.004] {
        terrain.extend(value.to_le_bytes());
    }
    let plane = WGS84_A_M + 100.0;
    for latitude in [0.002_f64, -0.002] {
        for longitude in [-0.002_f64, 0.002] {
            let (phi, lambda) = (latitude.to_radians(), longitude.to_radians());
            let n = WGS84_A_M / (1.0 - WGS84_E2 * phi.sin().powi(2)).sqrt();
            let height = plane / (phi.cos() * lambda.cos()) - n;
            terrain.extend(height.to_le_bytes());
        }
    }
    let original = TerrainPatch::from_bytes(&terrain).unwrap();
    let mut vertices = original.presentation_grid().2.to_vec();
    // The high centre exceeds the old grid-post bound by almost two kilometres.
    vertices.push(Vec3d::new(plane + 2000.0, 0.0, 0.0));
    vertices.push((vertices[0] + vertices[2]) * 0.5 + Vec3d::new(200.0, 0.0, 0.0));
    let faces = vec![
        ([0, 5, 4], 0),
        ([5, 2, 4], 1),
        ([2, 3, 4], 2),
        ([3, 1, 4], 3),
        ([1, 0, 4], 4),
    ];
    let hash = terrain.iter().fold(0xcbf29ce484222325_u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    });
    let mut water = b"AEWATR02".to_vec();
    water.extend(&terrain[8..48]);
    water.extend(hash.to_le_bytes());
    water.extend(2_u32.to_le_bytes());
    water.extend((faces.len() as u32).to_le_bytes());
    for v in &vertices[4..] {
        for coordinate in [v.x, v.y, v.z] {
            water.extend(coordinate.to_le_bytes());
        }
    }
    for (indices, class) in &faces {
        for index in indices {
            water.extend((*index as u32).to_le_bytes());
        }
        water.push(*class);
    }
    (terrain, water, vertices, faces)
}

fn mesh_triangles(mesh: &RenderMesh) -> BTreeSet<[[u32; 3]; 3]> {
    mesh.indices
        .chunks_exact(3)
        .map(|ids| {
            let mut points =
                std::array::from_fn(|i| mesh.positions[ids[i] as usize].map(f32::to_bits));
            points.sort_unstable();
            points
        })
        .collect()
}

#[test]
fn irregular_vertices_enter_bvh_ground_bounds_and_classification_without_editing_posts() {
    let (terrain, water, vertices, faces) = fixture();
    let patch = TerrainPatch::from_bytes_with_water(&terrain, &water).unwrap();
    assert_eq!(patch.presentation_grid().2, &vertices[..4]);
    assert_eq!(patch.canonical_vertices(), vertices);
    assert!(patch.requires_canonical_presentation());
    assert_eq!(patch.triangle_count(), 5);
    assert_eq!(patch.water_triangle_counts(), [1; 5]);
    for (ids, code) in &faces {
        let [a, b, c] = ids.map(|i| vertices[i]);
        let point = a * 0.2 + b * 0.3 + c * 0.5;
        let normal = (b - a).cross(c - a).normalized().unwrap();
        for scale in [1e-100, 1.0, 1e100] {
            let ray = Ray {
                origin: point + normal * 10.0,
                direction: -normal * scale,
            };
            let hit = patch.intersect(ray).unwrap();
            let reference = patch.intersect_brute_force(ray).unwrap();
            assert!((hit.position - point).length() < 2e-8);
            assert!((hit.distance_m - 10.0).abs() < 2e-8);
            assert_eq!(hit.position, reference.position);
            let scene = TerrainScene::new(vec![patch.clone()]);
            let tagged = scene.intersect(ray).unwrap();
            let kind = match code {
                1 => SurfaceKind::Water {
                    patch: 0,
                    kind: WaterKind::Ocean,
                },
                2 => SurfaceKind::Water {
                    patch: 0,
                    kind: WaterKind::Lake,
                },
                3 => SurfaceKind::Water {
                    patch: 0,
                    kind: WaterKind::River,
                },
                _ => SurfaceKind::Terrain(0),
            };
            assert_eq!(tagged.kind, kind);
            assert_eq!(tagged.grass_allowed, *code == 0);
        }
    }
    let scene = TerrainScene::new(vec![patch]);
    let ground = scene.ground(Geodetic::new(0.0, 0.0, 0.0).unwrap()).unwrap();
    assert!((ground.hit.position - vertices[4]).length() < 2e-8);
    assert!((ground.hit.geodetic.height_m - 2100.0).abs() < 2e-8);
    let ray = Ray {
        origin: Vec3d::new(WGS84_A_M + 5000.0, 0.0, 0.0),
        direction: Vec3d::new(-1.0, 0.0, 0.0),
    };
    assert!((scene.intersect(ray).unwrap().hit.distance_m - 2900.0).abs() < 2e-8);
}

#[test]
fn irregular_material_and_generic_meshes_keep_shared_edges_and_every_inserted_triangle() {
    let (terrain, water, vertices, faces) = fixture();
    let patch = TerrainPatch::from_bytes_with_water(&terrain, &water).unwrap();
    let mut edge_counts = HashMap::new();
    for (ids, _) in &faces {
        for [a, b] in [[ids[0], ids[1]], [ids[1], ids[2]], [ids[2], ids[0]]] {
            *edge_counts.entry([a.min(b), a.max(b)]).or_insert(0) += 1;
        }
    }
    for (edge, count) in edge_counts {
        assert_eq!(count, if edge.contains(&4) { 2 } else { 1 });
    }
    for height in [3000.0, 100_000.0, 8_000_000.0] {
        let local = LocalFrame::at(Observer::new(Geodetic::new(0.0, 0.0, height).unwrap()));
        let (land, water, stride) = terrain_water_mesh(&patch, local, 1000.0);
        let (generic, generic_stride) = terrain_mesh(&patch, local, 1000.0);
        assert_eq!((stride, generic_stride), (1, 1));
        let dry = mesh_triangles(&land);
        let wet = mesh_triangles(&water);
        assert!(dry.is_disjoint(&wet));
        let union: BTreeSet<_> = dry.union(&wet).copied().collect();
        assert_eq!(union.len(), faces.len());
        assert_eq!(mesh_triangles(&generic), union);
        for (ids, class) in &faces {
            let mut key = ids.map(|i| local.position(vertices[i]).map(f32::to_bits));
            key.sort_unstable();
            assert!(if (1..=3).contains(class) {
                wet.contains(&key)
            } else {
                dry.contains(&key)
            });
        }
    }
}

#[test]
fn irregular_companion_rejects_truncation_bad_indices_nonfinite_and_degenerate_geometry() {
    let (terrain, water, _, _) = fixture();
    for length in [0, 7, 48, 55, 56, 63, 64, 111, water.len() - 1] {
        assert!(TerrainPatch::from_bytes_with_water(&terrain, &water[..length]).is_err());
    }
    let mut extra = water.clone();
    extra.push(0);
    assert!(TerrainPatch::from_bytes_with_water(&terrain, &extra).is_err());
    for replacement in [f64::NAN, f64::INFINITY, 1e20] {
        let mut bad = water.clone();
        bad[64..72].copy_from_slice(&replacement.to_le_bytes());
        assert!(TerrainPatch::from_bytes_with_water(&terrain, &bad).is_err());
    }
    let triangle = 64 + 2 * 24;
    let mut bad_index = water.clone();
    bad_index[triangle..triangle + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(TerrainPatch::from_bytes_with_water(&terrain, &bad_index).is_err());
    let mut repeated = water.clone();
    repeated[triangle + 4..triangle + 8].copy_from_slice(&0_u32.to_le_bytes());
    assert!(TerrainPatch::from_bytes_with_water(&terrain, &repeated).is_err());
    let mut category = water;
    category[triangle + 12] = 5;
    assert!(TerrainPatch::from_bytes_with_water(&terrain, &category).is_err());
}

#[test]
fn inserted_peak_controls_nonrotating_clearance_and_visible_outline() {
    let (terrain, water, vertices, _) = fixture();
    let scene = TerrainScene::new(vec![
        TerrainPatch::from_bytes_with_water(&terrain, &water).unwrap(),
    ]);
    let plain = TerrainScene::new(vec![TerrainPatch::from_bytes(&terrain).unwrap()]);
    let epoch = SimulationClock::from_calendar("2024-04-08 18:40:00")
        .unwrap()
        .instant();
    let mut observer = Observer::new(Geodetic::new(0.0, -1.0, 1000.0).unwrap());
    let original = observer;
    let mut tracker = ObserverTracking::new(observer, epoch, Mat3d::identity()).unwrap();
    // The unchanged grid is below the eye; the inserted peak alone prevents
    // certification of a long nonrotating time jump through the global shell.
    assert_eq!(
        tracker.update(
            &mut observer,
            false,
            epoch.shifted(2.0).unwrap(),
            Mat3d::identity(),
            &scene,
        ),
        Err(TrackingError::UncertifiedTimeJump)
    );
    assert_eq!(observer, original);
    assert_eq!(tracker.instant(), epoch);
    tracker
        .update(
            &mut observer,
            false,
            epoch.shifted(2.0).unwrap(),
            Mat3d::identity(),
            &plain,
        )
        .unwrap();

    observer.aim_at(vertices[4]).unwrap();
    let viewport = GuideViewport::new([1440.0, 900.0], [1.0, 1.0]).unwrap();
    let profile =
        HorizonProfile::measure(&scene, observer.frame(), Lens::default(), viewport).unwrap();
    // This narrow peak lies between the coarse contour columns. Its actual
    // inserted vertex projects to the exact image centre after aim_at.
    assert!((profile.apex.pixel[1] - 450.0).abs() <= 0.03);
    assert!(profile.apex.hit.hit.geodetic.height_m > 2000.0);
    assert!(profile.apex.bracket_pixels <= 0.025);
    let old_profile =
        HorizonProfile::measure(&plain, observer.frame(), Lens::default(), viewport).unwrap();
    assert!(old_profile.apex.pixel[1] > profile.apex.pixel[1] + 5.0);
}
