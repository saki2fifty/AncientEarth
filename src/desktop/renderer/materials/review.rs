//! Independent checks of the actual shader-coordinate uniforms.
use super::GroundSettings;
use bevy::prelude::*;
use earth_math::{EnuBasis, Geodetic, Observer, Vec3d, rendering::LocalFrame};

#[test]
fn water_phase_preserves_smooth_finite_animation_and_sum_identity() {
    for time in [-10000.0, -100.25, -0.001, 0.0, 0.001, 100.25, 10000.0] {
        let phase = super::water_phase(time);
        for (actual, expected) in phase.to_array().into_iter().zip([
            (0.33_f64 * time).sin(),
            (0.33_f64 * time).cos(),
            (0.61_f64 * time).sin(),
            (0.61_f64 * time).cos(),
        ]) {
            assert!((f64::from(actual) - expected).abs() < 4e-8);
        }
        // The third shader ripple uses the sum of the independently reduced
        // phases; compare to a separately evaluated single angular frequency.
        let sum_sin = phase.x * phase.w + phase.y * phase.z;
        let sum_cos = phase.y * phase.w - phase.x * phase.z;
        assert!((f64::from(sum_sin) - (0.94_f64 * time).sin()).abs() < 1.5e-7);
        assert!((f64::from(sum_cos) - (0.94_f64 * time).cos()).abs() < 1.5e-7);
    }
    for time in [-f64::MAX, -1e100, 1e100, f64::MAX, f64::INFINITY, f64::NAN] {
        let p = super::water_phase(time);
        assert!(p.is_finite());
        assert!((p.x * p.x + p.y * p.y - 1.0).abs() < 2e-7);
        assert!((p.z * p.z + p.w * p.w - 1.0).abs() < 2e-7);
    }
    assert_eq!(super::water_phase(f64::NAN), super::water_phase(0.0));
}

fn shader_coordinates(frame: LocalFrame, point: Vec3d) -> Vec3 {
    let settings = GroundSettings::at(frame);
    let basis = Mat3::from_cols(
        settings.east.truncate(),
        settings.up.truncate(),
        settings.south.truncate(),
    );
    settings.origin.truncate() + basis * Vec3::from_array(frame.position(point))
}

fn periodic_error(actual: f32, reference: f64) -> f64 {
    let difference = f64::from(actual) - reference;
    (difference + 2048.0).rem_euclid(4096.0) - 2048.0
}

#[test]
fn material_coordinates_follow_fixed_ecef_positions_across_camera_rebases() {
    for (lat, lon) in [(0.0, 0.0), (45.0, 120.0), (-70.0, -179.999), (89.9, 25.0)] {
        let location = Geodetic::new(lat, lon, 12.0).unwrap();
        let base = Observer::new(location);
        let basis = EnuBasis::at(location);
        let point = base.eye_position() + basis.east * 37.125 + basis.north * 18.75;
        for height in [12.0, 212.0, 1012.0] {
            let observer = Observer::new(Geodetic::new(lat + 0.0005, lon, height).unwrap());
            let frame = LocalFrame::at(observer);
            let coordinate = shader_coordinates(frame, point);
            for (actual, reference) in coordinate
                .to_array()
                .into_iter()
                .zip([point.x, point.y, point.z])
            {
                assert!(
                    periodic_error(actual, reference).abs() < 0.001,
                    "procedural coordinate moved beyond nearby f32 tolerance"
                );
            }
            let settings = GroundSettings::at(frame);
            let gpu_basis = Mat3::from_cols(
                settings.east.truncate(),
                settings.up.truncate(),
                settings.south.truncate(),
            );
            assert!((gpu_basis.determinant() - 1.0).abs() < 3e-7);
            let identity = gpu_basis.transpose() * gpu_basis;
            for (actual, expected) in identity
                .to_cols_array()
                .into_iter()
                .zip(Mat3::IDENTITY.to_cols_array())
            {
                assert!((actual - expected).abs() < 3e-7);
            }
        }
    }
}

#[test]
fn material_wrapping_preserves_negative_and_period_boundary_positions() {
    let basis = EnuBasis::at(Geodetic::new(31.0, -95.0, 0.0).unwrap());
    for epsilon in [-0.001, -0.00001, 0.0, 0.00001, 0.001] {
        let point = Vec3d::new(-4096.0 + epsilon, 4096.0 - epsilon, epsilon);
        for displacement in [
            Vec3d::ZERO,
            Vec3d::new(7.0, -23.0, 18.0),
            Vec3d::new(4096.0, -4096.0, 4096.0),
        ] {
            let frame = LocalFrame {
                origin: point + displacement,
                basis,
            };
            let coordinate = shader_coordinates(frame, point);
            for (actual, reference) in coordinate
                .to_array()
                .into_iter()
                .zip([point.x, point.y, point.z])
            {
                assert!(periodic_error(actual, reference).abs() < 0.002);
            }
        }
    }
}

#[test]
fn grass_culling_box_contains_all_wind_extremes_and_root_relative_fades() {
    use earth_math::rendering::{GrassBlade, grass_deform_vertex, grass_render_mesh};
    let observer = Observer::new(Geodetic::new(45.0, 120.0, 10.0).unwrap());
    let mut frame = LocalFrame::at(observer);
    let basis = frame.basis;
    let blade = GrassBlade {
        base: observer.eye_position() - basis.up * 1.72,
        up: basis.up,
        side: basis.east,
        wind: basis.north,
        lean: basis.north * 0.13 + basis.east * 0.04,
        height_m: 0.19,
        width_m: 0.019,
        phase: 1.3,
        gust_phase: 2.7,
        cutoff_m: 18.0,
        color: [0.1, 0.4, 0.02, 1.0],
    };
    for rebase in [0.0, 20_000.0] {
        frame.origin = observer.eye_position() + basis.east * rebase;
        let upload = grass_render_mesh(std::slice::from_ref(&blade), frame);
        let mut deformed = Vec::new();
        // The four coefficients are linear in each weight, so the 16 corners
        // cover every extreme of that wind hypercube. Every fade is a convex
        // combination with the root; also exercise intermediate GPU arithmetic.
        for corner in 0..16 {
            let weights =
                std::array::from_fn(|axis| if corner & (1 << axis) == 0 { -1.0 } else { 1.0 });
            for index in 0..upload.mesh.positions.len() {
                let root = upload.roots[index];
                for distance in [root[3] - 3.0, root[3] - 1.5, root[3]] {
                    deformed.push(grass_deform_vertex(
                        upload.mesh.positions[index],
                        [
                            upload.wind_a[index],
                            upload.wind_b[index],
                            upload.wind_c[index],
                            upload.wind_d[index],
                        ],
                        root,
                        weights,
                        [root[0], root[1] + distance, root[2]],
                    ));
                }
            }
        }
        let (_, bounds) = super::grass_mesh(upload);
        let low = Vec3::from(bounds.center - bounds.half_extents);
        let high = Vec3::from(bounds.center + bounds.half_extents);
        for point in deformed {
            let point = Vec3::from_array(point);
            assert!(point.cmpge(low).all() && point.cmple(high).all());
        }
    }
}
