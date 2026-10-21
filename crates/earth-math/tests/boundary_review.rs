//! Independent checks that WBM categories never become physical elevations.
use earth_math::{
    astro::{AstroInstant, Mat3d, ObserverTracking, SimulationClock},
    guides::{GuideViewport, HorizonProfile},
    rendering::{LocalFrame, RenderMesh, boundary_scene_meshes, grass_blades_with_quality},
    *,
};
use std::collections::BTreeMap;

fn geo(lat: f64, lon: f64, height: f64) -> Geodetic {
    Geodetic::new(lat, lon, height).unwrap()
}
fn close(a: f64, b: f64, tolerance: f64) {
    assert!((a - b).abs() <= tolerance, "{a} != {b}: {}", (a - b).abs());
}
fn bytes() -> Vec<u8> {
    let mut bytes = b"AEBNDR01".to_vec();
    bytes.extend(4_u32.to_le_bytes());
    bytes.extend(2_u32.to_le_bytes());
    for value in [-0.03_f64, 0.01, 0.02, 0.02] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(8_u32.to_le_bytes());
    bytes.extend(6_u32.to_le_bytes());
    for row in [0.0_f64, 3.0] {
        for column in [0.0_f64, 3.0, 6.0, 9.0] {
            bytes.extend(row.to_le_bytes());
            bytes.extend(column.to_le_bytes());
        }
    }
    for (column, codes) in [[0_u8, 1], [2, 3], [4, 0]].into_iter().enumerate() {
        for (indices, code) in [
            ([column, column + 4, column + 5], codes[0]),
            ([column, column + 5, column + 1], codes[1]),
        ] {
            for index in indices {
                bytes.extend((index as u32).to_le_bytes());
            }
            bytes.push(code);
        }
    }
    bytes
}
fn scene() -> TerrainScene {
    TerrainScene::boundary_only(vec![TerrainPatch::from_boundary_bytes(&bytes()).unwrap()]).unwrap()
}
fn empty() -> TerrainScene {
    TerrainScene::boundary_only(vec![]).unwrap()
}

#[test]
fn every_class_and_coverage_edge_has_the_identical_analytic_surface() {
    let scene = scene();
    assert!(scene.is_boundary_only());
    for (column, codes) in [[0_u8, 1], [2, 3], [4, 0]].into_iter().enumerate() {
        for (row, col, code) in [(2.0, 1.0, codes[0]), (1.0, 2.0, codes[1])] {
            let g = geo(
                0.01 - row * 0.02 / 3.0,
                -0.03 + (column as f64 * 3.0 + col) * 0.02 / 3.0,
                123.0,
            );
            let hit = scene.ground(g).unwrap();
            assert_eq!(hit.hit.geodetic, g.with_height(0.0).unwrap());
            assert_eq!(
                hit.hit.position,
                geodetic_to_ecef(g.with_height(0.0).unwrap())
            );
            assert_eq!(hit.hit.normal, EnuBasis::at(g).up);
            assert_eq!(hit.grass_allowed, code == 0);
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
                4 => SurfaceKind::ReferenceEllipsoid,
                _ => SurfaceKind::Terrain(0),
            };
            assert_eq!(hit.kind, kind);
            let ray = Ray {
                origin: geodetic_to_ecef(g),
                direction: -EnuBasis::at(g).up * 7.0,
            };
            let actual = scene.intersect(ray).unwrap();
            let expected = intersect_wgs84(ray).unwrap();
            assert_eq!(actual.hit.position, expected.position);
            assert_eq!(actual.hit.distance_m, expected.distance_m);
            close(actual.hit.distance_m, 123.0, 3e-9);
        }
    }
    for latitude in [
        -90.0,
        -0.010000001,
        -0.01,
        -0.009999999,
        0.0,
        0.01,
        0.010000001,
        90.0,
    ] {
        for longitude in [-180.0, -0.030000001, -0.03, 0.0, 0.03, 0.030000001, 180.0] {
            let g = geo(latitude, longitude, 5.0);
            assert_eq!(scene.ground(g).unwrap().hit.geodetic.height_m, 0.0);
            for heading in [0.0, 47.0, 180.0] {
                for pitch in [-90.0, -0.1, 0.0, 45.0] {
                    let ray = CameraFrame::new(g, heading, pitch).center_ray();
                    assert_eq!(scene.intersects_any(ray), intersect_wgs84(ray).is_some());
                    if let Some(hit) = scene.intersect(ray) {
                        assert_eq!(hit.hit.position, intersect_wgs84(ray).unwrap().position);
                    }
                }
            }
        }
    }
}

#[test]
fn surface_placement_datum_crouch_and_motion_do_not_restore_height_data() {
    let scene = scene();
    let mut observer = Observer::new(geo(0.0, -0.02, 100.0));
    let feet = observer.feet;
    scene.place_on_ground(&mut observer, feet).unwrap();
    assert_eq!(observer.feet.height_m, 0.0);
    let before = observer;
    assert!(
        scene
            .try_teleport(&mut observer, geo(0.0, -0.02, -1e-10))
            .is_err()
    );
    assert_eq!(observer, before);
    let posture = EyePosture {
        crouched: true,
        ..EyePosture::default()
    };
    posture.apply(&mut observer).unwrap();
    assert_eq!(observer.feet, before.feet);
    let datum = GeoidGrid::from_grid(2, 2, vec![-31.0; 4]).unwrap();
    let report = scene.vertical_report(observer, &datum).unwrap();
    close(report.ground_egm2008_height_m, 31.0, 0.0);
    close(report.eyes_above_ground_m, observer.eye_height_m, 1e-12);
    let mut plain = observer;
    observer.heading_deg = 90.0;
    plain.heading_deg = 90.0;
    observer.speed_mps = 1000.0;
    plain.speed_mps = 1000.0;
    for _ in 0..50 {
        scene.fly(&mut observer, 1.0, 0.0, 0.0, 0.1, false);
        empty().fly(&mut plain, 1.0, 0.0, 0.0, 0.1, false);
        assert_eq!(observer, plain);
        assert!(observer.feet.height_m >= -1e-8);
    }
}

#[test]
fn skyline_guides_and_nonrotating_transport_ignore_material_boundaries() {
    let scene = scene();
    let plain = empty();
    let mut observer = Observer::new(geo(0.0, 0.0, 1000.0));
    observer.heading_deg = 90.0;
    observer.pitch_deg = -1.0;
    observer.roll_deg = 17.0;
    let actual = scene.skyline(observer).unwrap();
    let expected = plain.skyline(observer).unwrap();
    assert_eq!(actual.method, SkylineMethod::ReferenceEllipsoidTangent);
    close(actual.hit.hit.distance_m, expected.hit.hit.distance_m, 0.0);
    close(actual.elevation_deg, expected.elevation_deg, 0.0);
    let viewport = GuideViewport::new([960.0, 540.0], [1.5, 1.5]).unwrap();
    for projection in [Projection::Rectilinear, Projection::EquidistantFisheye] {
        let lens = Lens::new(projection, 100.0);
        let a = HorizonProfile::measure(&scene, observer.frame(), lens, viewport).unwrap();
        let b = HorizonProfile::measure(&plain, observer.frame(), lens, viewport).unwrap();
        assert_eq!(a.apex.pixel, b.apex.pixel);
        assert_eq!(a.chord_width_m, b.chord_width_m);
        assert_eq!(a.surface_width_m, b.surface_width_m);
    }
    let epoch: AstroInstant = SimulationClock::from_calendar("2024-01-15 12:00:00")
        .unwrap()
        .instant();
    observer.feet.height_m = 1_000_000.0;
    let mut other = observer;
    let mut a = ObserverTracking::new(observer, epoch, Mat3d::identity()).unwrap();
    let mut b = ObserverTracking::new(other, epoch, Mat3d::identity()).unwrap();
    let rotation = Mat3d {
        rows: [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
    };
    a.update(
        &mut observer,
        false,
        epoch.shifted(86400.0).unwrap(),
        rotation,
        &scene,
    )
    .unwrap();
    b.update(
        &mut other,
        false,
        epoch.shifted(86400.0).unwrap(),
        rotation,
        &plain,
    )
    .unwrap();
    assert_eq!(observer, other);
}

#[test]
fn malformed_height_free_bytes_and_legacy_patch_mixing_fail_closed() {
    let good = bytes();
    for length in [0, 7, 55, 56, good.len() - 1] {
        assert!(TerrainPatch::from_boundary_bytes(&good[..length]).is_err());
    }
    for (offset, value) in [(56, f64::NAN), (56, -1.0), (64, 10.0)] {
        let mut bad = good.clone();
        bad[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        assert!(TerrainPatch::from_boundary_bytes(&bad).is_err());
    }
    let mut bad = good.clone();
    bad[56 + 8 * 16 + 12] = 9;
    assert!(TerrainPatch::from_boundary_bytes(&bad).is_err());
    let mut bad = good.clone();
    bad[56 + 8 * 16..56 + 8 * 16 + 4].copy_from_slice(&99_u32.to_le_bytes());
    assert!(TerrainPatch::from_boundary_bytes(&bad).is_err());
    let legacy = TerrainPatch::from_grid(2, 2, -0.01, 0.01, 0.02, 0.02, vec![1000.0; 4]).unwrap();
    assert!(TerrainScene::boundary_only(vec![legacy]).is_err());
}

fn world(local: LocalFrame, point: [f32; 3]) -> Vec3d {
    local.origin + local.basis.east * f64::from(point[0]) + local.basis.up * f64::from(point[1])
        - local.basis.north * f64::from(point[2])
}
type Edge = [[u32; 3]; 2];
fn edges(mesh: &RenderMesh, output: &mut BTreeMap<Edge, (usize, i32)>) {
    for face in mesh.indices.chunks_exact(3) {
        for (a, b) in [(face[0], face[1]), (face[1], face[2]), (face[2], face[0])] {
            let a = mesh.positions[a as usize].map(f32::to_bits);
            let b = mesh.positions[b as usize].map(f32::to_bits);
            let (key, sign) = if a < b { ([a, b], 1) } else { ([b, a], -1) };
            let entry = output.entry(key).or_default();
            entry.0 += 1;
            entry.1 += sign;
        }
    }
}

#[test]
fn joint_display_edges_are_exactly_shared_and_material_area_is_preserved() {
    let scene = scene();
    for height in [100.0, 1_000_000.0] {
        let local = LocalFrame::at(Observer::new(geo(0.0, 0.0, height)));
        let mesh = boundary_scene_meshes(&scene, local, 1200.0);
        let mut regional = BTreeMap::new();
        let mut reference = BTreeMap::new();
        edges(&mesh.reference, &mut reference);
        let region = &mesh.regions[0];
        assert_eq!(region.stride, 1);
        edges(&region.land, &mut regional);
        edges(&region.water, &mut regional);
        let mut border_count = 0;
        for (key, (count, sign)) in &regional {
            if *count == 1 {
                border_count += 1;
                assert_eq!(
                    reference.get(key),
                    Some(&(1, -sign)),
                    "unshared regional edge {key:?}"
                );
            } else {
                assert_eq!((*count, *sign), (2, 0));
            }
        }
        assert!(border_count >= 8);
        // Recover parameter areas only for this small, non-wrapping fixture.
        // The tolerance includes the actual f32 display quantization.
        for material in [&region.land, &region.water] {
            let positions: Vec<_> = material
                .positions
                .iter()
                .map(|&p| ecef_to_geodetic(world(local, p)).unwrap())
                .collect();
            for (point, normal) in positions.iter().zip(&material.normals) {
                assert!(point.height_m.abs() <= if height < 1000.0 { 0.001 } else { 0.07 });
                let expected = local.direction(EnuBasis::at(*point).up);
                for axis in 0..3 {
                    close(f64::from(normal[axis]), f64::from(expected[axis]), 3e-7);
                }
            }
            let mut area = 0.0;
            for face in material.indices.chunks_exact(3) {
                let [a, b, c] = [face[0], face[1], face[2]].map(|i| positions[i as usize]);
                let signed = (b.lon_deg - a.lon_deg) * (c.lat_deg - a.lat_deg)
                    - (b.lat_deg - a.lat_deg) * (c.lon_deg - a.lon_deg);
                assert!(signed > 0.0);
                area += signed / 2.0;
            }
            close(area, 0.0006, 2e-7);
        }
        assert!(
            region
                .land
                .colors
                .iter()
                .all(|color| *color == region.land.colors[0])
        );
        assert_eq!(region.land.colors[0], mesh.reference.colors[0]);
    }
}

#[test]
fn grass_has_analytic_roots_only_on_explicit_land() {
    let scene = scene();
    for (row, col, allowed) in [
        (2.0, 1.0, true),
        (1.0, 2.0, false),
        (2.0, 4.0, false),
        (1.0, 5.0, false),
        (2.0, 7.0, false),
        (0.0, 12.0, false),
    ] {
        let observer = Observer::new(geo(0.01 - row * 0.02 / 3.0, -0.03 + col * 0.02 / 3.0, 0.0));
        let blades = grass_blades_with_quality(&scene, observer, 0);
        assert_eq!(!blades.is_empty(), allowed);
        for blade in &blades {
            let g = ecef_to_geodetic(blade.base).unwrap();
            // Inverse height subtracts Earth-scale quantities; also check the
            // defining surface equation directly, independently of that inverse.
            close(g.height_m, 0.0, 8e-9);
            let p = blade.base;
            close(
                (p.x * p.x + p.y * p.y) / WGS84_A_M.powi(2) + p.z * p.z / WGS84_B_M.powi(2),
                1.0,
                16.0 * f64::EPSILON,
            );
            assert!(scene.ground(g).unwrap().grass_allowed);
        }
    }
}

#[test]
fn bundled_files_contain_no_height_component_and_generic_coast_pose_is_valid() {
    let sources: [&[u8]; 4] = [
        include_bytes!("../../../assets/boundaries/region-01.aeb"),
        include_bytes!("../../../assets/boundaries/region-02.aeb"),
        include_bytes!("../../../assets/boundaries/region-03.aeb"),
        include_bytes!("../../../assets/boundaries/region-04.aeb"),
    ];
    let patches: Vec<_> = sources
        .into_iter()
        .map(|bytes| TerrainPatch::from_boundary_bytes(bytes).unwrap())
        .collect();
    // Counts independently reconstructed against source WBM and curve/topology
    // contracts by scripts/verify_boundaries.py.
    for (patch, counts) in patches.iter().zip([
        [114028, 0, 0, 0, 20],
        [163703, 59304, 6, 270, 1951],
        [104852, 0, 4, 110, 2170],
        [170758, 237483, 929, 154, 3492],
    ]) {
        assert_eq!(patch.water_triangle_counts(), counts);
        for (index, &point) in patch.canonical_vertices().iter().enumerate() {
            let g = patch.boundary_geodetic(index).unwrap();
            assert_eq!(g.height_m, 0.0);
            assert_eq!(point, geodetic_to_ecef(g));
            close(
                point.x * point.x / WGS84_A_M.powi(2)
                    + point.y * point.y / WGS84_A_M.powi(2)
                    + point.z * point.z / WGS84_B_M.powi(2),
                1.0,
                7e-16,
            );
        }
    }
    let scene = TerrainScene::boundary_only(patches).unwrap();
    let observer = scene.boundary_observer().unwrap();
    close(observer.feet.height_m, 6.0, 0.0);
    let ground = scene.ground(observer.feet).unwrap();
    assert!(ground.grass_allowed);
    assert!(matches!(ground.kind, SurfaceKind::Terrain(_)));
    assert!((-10.0..0.0).contains(&observer.pitch_deg));
    let target = scene.intersect(observer.frame().center_ray()).unwrap();
    assert!(target.hit.distance_m.is_finite() && target.hit.distance_m > 0.0);
    assert_eq!(target.hit.geodetic.height_m, 0.0);
}
