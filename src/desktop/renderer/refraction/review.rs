//! Independent audit of packed source-category buffers and local GPU lookup.
use super::*;

fn sample(
    data: &[[f32; 4]],
    region: &Region,
    eye: earth_math::Geodetic,
    normal: [f32; 3],
    geometry: GpuEllipsoid,
) -> bool {
    let angles = geometry.relative_angles(normal);
    let (a, b) = region.uniforms(eye);
    let p = Vec2::new((a.x - angles[0]) * a.z, (a.y + angles[1]) * a.w);
    if p.min_element() < 0.0 || p.x > 3.0 * b.y || p.y > 3.0 * b.x {
        return false;
    }
    let row = ((p.x / 3.0) as usize).min(region.height - 2);
    let column = ((p.y / 3.0) as usize).min(region.width - 2);
    let bin = data[region.offset + row * (region.width - 1) + column];
    for i in 0..bin[1] as usize {
        let id = data[bin[0] as usize + i][0] as usize;
        let t = data[id];
        let v = data[id + 1];
        let points = [
            Vec2::new(t[0], t[1]),
            Vec2::new(t[2], t[3]),
            Vec2::new(v[0], v[1]),
        ];
        let signs: [f32; 3] =
            std::array::from_fn(|j| (points[(j + 1) % 3] - points[j]).perp_dot(p - points[j]));
        let tolerance: [f32; 3] =
            std::array::from_fn(|j| 0.0002 * (points[(j + 1) % 3] - points[j]).length());
        if (0..3).all(|j| signs[j] >= -tolerance[j]) || (0..3).all(|j| signs[j] <= tolerance[j]) {
            return v[2] > 0.5;
        }
    }
    false
}

#[test]
fn packed_boundary_bins_preserve_every_source_face_and_material() {
    let surface = ancient_earth::surface::WorldSurface::bundled().unwrap();
    let scene = surface.active(true);
    let (data, regions) = boundary_data(scene);
    assert_eq!(regions.len(), 4);
    for (patch, region) in scene.patches().iter().zip(&regions) {
        let parameters = patch.boundary_parameters().unwrap();
        let faces: Vec<_> = patch.boundary_triangles().unwrap().collect();
        let mut ids = std::collections::BTreeSet::new();
        for row in 0..region.height - 1 {
            for column in 0..region.width - 1 {
                let bin = data[region.offset + row * (region.width - 1) + column];
                assert_eq!(bin[0].fract(), 0.0);
                assert_eq!(bin[1].fract(), 0.0);
                for entry in &data[bin[0] as usize..(bin[0] + bin[1]) as usize] {
                    assert_eq!(entry[0].fract(), 0.0);
                    let id = entry[0] as usize;
                    assert!(ids.insert(id));
                    let t = data[id];
                    let v = data[id + 1];
                    for p in [[t[0], t[1]], [t[2], t[3]], [v[0], v[1]]] {
                        assert!(
                            p[0] >= 3.0 * row as f32 - 0.0001
                                && p[0] <= 3.0 * (row + 1) as f32 + 0.0001
                        );
                        assert!(
                            p[1] >= 3.0 * column as f32 - 0.0001
                                && p[1] <= 3.0 * (column + 1) as f32 + 0.0001
                        );
                    }
                }
            }
        }
        assert_eq!(ids.len(), faces.len());
        for (id, (indices, code)) in ids.into_iter().zip(faces) {
            let p = indices.map(|i| parameters[i]);
            assert_eq!(
                data[id],
                [
                    p[0][0] as f32,
                    p[0][1] as f32,
                    p[1][0] as f32,
                    p[1][1] as f32
                ]
            );
            assert_eq!(
                data[id + 1],
                [
                    p[2][0] as f32,
                    p[2][1] as f32,
                    u8::from((1..=3).contains(&code)) as f32,
                    0.0
                ]
            );
        }
    }
}

#[test]
fn actual_relative_region_uniforms_match_canonical_interior_categories() {
    use earth_math::{EnuBasis, Geodetic, Observer};
    let surface = ancient_earth::surface::WorldSurface::bundled().unwrap();
    let scene = surface.active(true);
    let (data, regions) = boundary_data(scene);
    let mut checked = [0_usize; 5];
    for (patch, region) in scene.patches().iter().zip(&regions) {
        let [west, south, east, north] = region.bounds;
        let observer =
            Observer::new(Geodetic::new((south + north) * 0.5, (west + east) * 0.5, 6.0).unwrap());
        let eye = observer.eye_geodetic();
        let local = LocalFrame::at(observer);
        let geometry = GpuEllipsoid::at(observer);
        let parameters = patch.boundary_parameters().unwrap();
        for (indices, code) in patch.boundary_triangles().unwrap().step_by(31) {
            let p = indices.map(|i| parameters[i]);
            let center = [
                p.iter().map(|v| v[0]).sum::<f64>() / 3.0,
                p.iter().map(|v| v[1]).sum::<f64>() / 3.0,
            ];
            // Avoid claiming exact categorical-edge agreement at finite f32
            // precision. Here every sample is >0.005 source-grid unit inside.
            let clearance = (0..3)
                .map(|i| {
                    let q = p[(i + 1) % 3];
                    let d = [q[0] - p[i][0], q[1] - p[i][1]];
                    (d[0] * (center[1] - p[i][1]) - d[1] * (center[0] - p[i][0])).abs()
                        / d[0].hypot(d[1])
                })
                .fold(f64::INFINITY, f64::min);
            if clearance < 0.005 {
                continue;
            }
            let location = Geodetic::new(
                north - center[0] * (north - south) / (3.0 * (region.height - 1) as f64),
                west + center[1] * (east - west) / (3.0 * (region.width - 1) as f64),
                0.0,
            )
            .unwrap();
            let normal = local.direction(EnuBasis::at(location).up);
            assert_eq!(
                sample(&data, region, eye, normal, geometry),
                (1..=3).contains(&code)
            );
            assert_eq!(
                matches!(
                    scene.ground(location).unwrap().kind,
                    earth_math::SurfaceKind::Water { .. }
                ),
                (1..=3).contains(&code)
            );
            checked[usize::from(code)] += 1;
        }
    }
    assert!(
        checked[0] > 100 && checked[1] > 100 && checked[2] > 0 && checked[3] > 0 && checked[4] > 0,
        "{checked:?}"
    );
}
