//! Independent classification, geometry and presentation checks. These fixtures
//! attach deliberately different labels to known Cartesian triangles; labels
//! must never move the geometry or change the nearest intersection.
use earth_math::{
    EnuBasis, Geodetic, Observer, Ray, SurfaceKind, TerrainPatch, TerrainScene, WaterKind,
    rendering::{LocalFrame, RenderMesh, grass_blades_with_quality, terrain_water_mesh},
};
use std::collections::BTreeSet;

fn fixture(width: u32, height: u32, elevation: f64, codes: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let mut terrain = b"AETERR01".to_vec();
    terrain.extend(width.to_le_bytes());
    terrain.extend(height.to_le_bytes());
    for value in [-95.005_f64, 29.005, 0.002, 0.002] {
        terrain.extend(value.to_le_bytes());
    }
    for _ in 0..width * height {
        terrain.extend(elevation.to_le_bytes());
    }
    let checksum = terrain.iter().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    });
    let mut water = b"AEWATR01".to_vec();
    water.extend(&terrain[8..48]);
    water.extend(checksum.to_le_bytes());
    water.extend(codes);
    (terrain, water)
}

fn expected_kind(code: u8) -> SurfaceKind {
    match code {
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
    }
}

#[test]
fn labels_survive_bvh_reordering_without_changing_any_triangle_or_range() {
    let codes: Vec<u8> = (0..60).map(|i| (i % 5) as u8).collect();
    let (terrain, water) = fixture(7, 6, 120.0, &codes);
    let original = TerrainPatch::from_bytes(&terrain).unwrap();
    let tagged = TerrainPatch::from_bytes_with_water(&terrain, &water).unwrap();
    assert_eq!(tagged.water_triangle_counts(), [12; 5]);
    assert_eq!(original.presentation_grid().2, tagged.presentation_grid().2);
    let vertices = original.presentation_grid().2.to_vec();
    let plain = TerrainScene::new(vec![original]);
    let scene = TerrainScene::new(vec![tagged]);
    for row in 0..5 {
        for col in 0..6 {
            let nw = row * 7 + col;
            for (half, indices) in [[nw, nw + 7, nw + 8], [nw, nw + 8, nw + 1]]
                .into_iter()
                .enumerate()
            {
                let [a, b, c] = indices.map(|i| vertices[i]);
                let point = a * 0.2 + b * 0.3 + c * 0.5;
                let normal = (b - a).cross(c - a).normalized().unwrap();
                for scale in [1e-100, 1.0, 1e100] {
                    let ray = Ray {
                        origin: point + normal * 50.0,
                        direction: -normal * scale,
                    };
                    let before = plain.intersect(ray).unwrap();
                    let after = scene.intersect(ray).unwrap();
                    assert_eq!(before.hit.position, after.hit.position);
                    assert_eq!(before.hit.normal, after.hit.normal);
                    assert_eq!(before.hit.distance_m, after.hit.distance_m);
                    assert!((after.hit.distance_m - 50.0).abs() < 2e-8);
                    let code = codes[2 * (row * 6 + col) + half];
                    assert_eq!(after.kind, expected_kind(code));
                    assert_eq!(after.grass_allowed, code == 0);
                    assert!(scene.intersects_any(ray));
                    let ground = scene.ground(after.hit.geodetic).unwrap();
                    assert_eq!(ground.kind, after.kind);
                    assert_eq!(ground.grass_allowed, after.grass_allowed);
                }
            }
        }
    }
}

#[test]
fn companion_rejects_mismatched_heights_registration_categories_and_lengths() {
    let (terrain, water) = fixture(2, 2, -25.0, &[1, 1]);
    assert!(TerrainPatch::from_bytes_with_water(&terrain, &water).is_ok());
    for length in [0, 7, 48, 55, 56, 57] {
        assert!(TerrainPatch::from_bytes_with_water(&terrain, &water[..length]).is_err());
    }
    let mut trailing = water.clone();
    trailing.push(0);
    assert!(TerrainPatch::from_bytes_with_water(&terrain, &trailing).is_err());
    for offset in [0, 8, 16, 48] {
        let mut corrupt = water.clone();
        corrupt[offset] ^= 1;
        assert!(TerrainPatch::from_bytes_with_water(&terrain, &corrupt).is_err());
    }
    let mut invalid = water.clone();
    invalid[56] = 5;
    assert!(TerrainPatch::from_bytes_with_water(&terrain, &invalid).is_err());
    let (other_heights, _) = fixture(2, 2, -24.0, &[1, 1]);
    assert!(TerrainPatch::from_bytes_with_water(&other_heights, &water).is_err());
}

#[test]
fn negative_height_water_is_visible_and_navigation_uses_the_same_surface() {
    let (terrain, water) = fixture(2, 2, -25.0, &[1, 1]);
    let patch = TerrainPatch::from_bytes_with_water(&terrain, &water).unwrap();
    let scene = TerrainScene::new(vec![patch]);
    let location = Geodetic::new(29.004, -95.004, 20.0).unwrap();
    let mut observer = Observer::new(location);
    scene.place_on_ground(&mut observer, location).unwrap();
    let frame = observer.frame();
    let down = -EnuBasis::at(observer.feet).up;
    let hit = scene
        .intersect(Ray {
            origin: frame.origin,
            direction: down,
        })
        .unwrap();
    assert_eq!(hit.kind, expected_kind(1));
    assert!(!hit.grass_allowed);
    assert!(hit.hit.geodetic.height_m < -24.99);
    assert!((hit.hit.distance_m - observer.eye_height_m).abs() < 2e-8);
    assert!((scene.ground(observer.feet).unwrap().hit.position - hit.hit.position).length() < 2e-8);
    assert!(
        scene
            .try_teleport(
                &mut observer,
                Geodetic::new(29.004, -95.004, -30.0).unwrap()
            )
            .is_err()
    );
}

fn triangle_keys(mesh: &RenderMesh) -> BTreeSet<[[u32; 3]; 3]> {
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
fn material_partition_has_each_canonical_triangle_exactly_once_at_all_distances() {
    let codes: Vec<u8> = (0..24).map(|i| (i % 5) as u8).collect();
    let (terrain, water) = fixture(5, 4, -25.0, &codes);
    let patch = TerrainPatch::from_bytes_with_water(&terrain, &water).unwrap();
    for altitude in [10.0, 100_000.0, 8_000_000.0] {
        let observer = Observer::new(Geodetic::new(29.002, -95.001, altitude).unwrap());
        let local = LocalFrame::at(observer);
        let (land, water, stride) = terrain_water_mesh(&patch, local, 1000.0);
        assert_eq!(stride, 1);
        let land_keys = triangle_keys(&land);
        let water_keys = triangle_keys(&water);
        assert!(land_keys.is_disjoint(&water_keys));
        assert_eq!(land_keys.len() + water_keys.len(), patch.triangle_count());
        for (vertices, kind) in patch.presentation_triangles() {
            let mut key = vertices.map(|v| local.position(v).map(f32::to_bits));
            key.sort_unstable();
            assert!(if kind.is_some() {
                water_keys.contains(&key)
            } else {
                land_keys.contains(&key)
            });
        }
    }
}

#[test]
fn grass_rejects_ocean_lake_river_and_mixed_shores_without_flooding_negative_land() {
    let position = Geodetic::new(29.004, -95.004, -25.0).unwrap();
    for code in 0..=4 {
        let (terrain, water) = fixture(2, 2, -25.0, &[code; 2]);
        let scene = TerrainScene::new(vec![
            TerrainPatch::from_bytes_with_water(&terrain, &water).unwrap(),
        ]);
        let mut observer = Observer::new(position);
        scene.place_on_ground(&mut observer, position).unwrap();
        let blades = grass_blades_with_quality(&scene, observer, 0);
        assert_eq!(blades.is_empty(), code != 0);
        for blade in blades.iter().step_by(101) {
            let foot = earth_math::ecef_to_geodetic(blade.base).unwrap();
            assert!(scene.ground(foot).unwrap().grass_allowed);
        }
    }
}

#[test]
fn archived_classifications_retain_the_independently_verified_face_counts() {
    let inputs: [(&[u8], &[u8]); 4] = [
        (
            include_bytes!("../../../assets/terrain/fuji.aet"),
            include_bytes!("../../../assets/terrain/fuji.aew"),
        ),
        (
            include_bytes!("../../../assets/terrain/tokyo.aet"),
            include_bytes!("../../../assets/terrain/tokyo.aew"),
        ),
        (
            include_bytes!("../../../assets/terrain/houston.aet"),
            include_bytes!("../../../assets/terrain/houston.aew"),
        ),
        (
            include_bytes!("../../../assets/terrain/galveston.aet"),
            include_bytes!("../../../assets/terrain/galveston.aew"),
        ),
    ];
    let scene = TerrainScene::new(
        inputs
            .map(|(terrain, water)| TerrainPatch::from_bytes_with_water(terrain, water).unwrap())
            .into(),
    );
    // Counts independently reconstructed from original WBM stencils and native
    // clipped faces by scripts/verify_shoreline.py, not from rendered color.
    for (patch, counts) in scene.patches().iter().zip([
        [114028, 0, 0, 0, 20],
        [163703, 59304, 6, 270, 1951],
        [104852, 0, 4, 110, 2170],
        [170758, 237483, 929, 154, 3492],
    ]) {
        assert_eq!(patch.water_triangle_counts(), counts);
    }
}
