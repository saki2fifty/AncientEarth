//! Receive-time solar occultation by smooth spheres. This is geometric angular
//! disk obscuration, not limb-darkened irradiance or a certified contact time.
use super::{AstroError, AstronomyFrame, LIGHT_SPEED_MPS, Mat3d};
use crate::{EnuBasis, Vec3d, ecef_to_geodetic};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EclipseKind {
    None,
    Partial,
    Annular,
    Total,
}
impl EclipseKind {
    pub fn description(self) -> &'static str {
        match self {
            Self::None => "No solar eclipse",
            Self::Partial => "Partial / penumbra",
            Self::Annular => "Annular / antumbra",
            Self::Total => "Total / umbra",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SolarEclipse {
    pub kind: EclipseKind,
    /// Fraction of the angular solar disk's area covered, in [0,1].
    pub obscuration: f64,
    pub visible_fraction: f64,
    pub sun_angular_radius_rad: f64,
    pub moon_angular_radius_rad: f64,
    pub separation_rad: f64,
    /// Elevation above geodetic horizontal, not a refracted/terrain horizon.
    /// Undefined at points without a geodetic surface normal.
    pub sun_elevation_deg: Option<f64>,
}
impl SolarEclipse {
    pub fn summary(self) -> String {
        let elevation = self.sun_elevation_deg.map_or(String::new(), |value| {
            format!("; Sun elevation {value:.2}°")
        });
        format!(
            "{}; geometric obscuration {:.3}%{}",
            self.kind.description(),
            self.obscuration * 100.0,
            elevation
        )
    }
}

/// Circular overlap in an angular plane. Radii are true sphere angular radii;
/// the overlap area is a small-angle disk approximation, explicitly distinct
/// from a spherical-cap solid angle and limb-darkened photometric flux.
pub fn disk_obscuration(
    sun_radius_rad: f64,
    moon_radius_rad: f64,
    separation_rad: f64,
) -> Result<(EclipseKind, f64), AstroError> {
    if ![sun_radius_rad, moon_radius_rad, separation_rad]
        .iter()
        .all(|x| x.is_finite())
        || sun_radius_rad <= 0.0
        || moon_radius_rad <= 0.0
        || sun_radius_rad >= std::f64::consts::FRAC_PI_2
        || moon_radius_rad >= std::f64::consts::FRAC_PI_2
        || !(0.0..=std::f64::consts::PI).contains(&separation_rad)
    {
        return Err(AstroError::InvalidObserver);
    }
    // Classify before normalization, including finite subnormal radii whose
    // ratios would overflow. Difference comparisons avoid adding a tiny radius
    // to a much larger one. Unresolvable partial ratios are rejected below.
    if separation_rad - sun_radius_rad.max(moon_radius_rad) >= sun_radius_rad.min(moon_radius_rad) {
        return Ok((EclipseKind::None, 0.0));
    }
    if moon_radius_rad >= sun_radius_rad && moon_radius_rad - separation_rad >= sun_radius_rad {
        return Ok((EclipseKind::Total, 1.0));
    }
    if moon_radius_rad < sun_radius_rad && sun_radius_rad - separation_rad >= moon_radius_rad {
        let y = moon_radius_rad / sun_radius_rad;
        return Ok((EclipseKind::Annular, y * y));
    }
    let x = separation_rad / sun_radius_rad;
    let y = moon_radius_rad / sun_radius_rad;
    if !x.is_finite() || !y.is_finite() || !(x * x).is_finite() || !(y * y).is_finite() {
        return Err(AstroError::InvalidObserver);
    }
    // Factor the radius difference and use atan2 at both contacts. acos of a
    // rounded near-unit argument is especially poorly conditioned on the GPU.
    let difference = 1.0 - y;
    let chord = 0.5 * (x + difference * (1.0 + y) / x);
    let radicand = (1.0 + y - x) * (x - difference) * (x + difference) * (x + 1.0 + y);
    let height = radicand.max(0.0).sqrt() / (2.0 * x);
    let area = height.atan2(chord) + y * y * height.atan2(x - chord) - x * height;
    Ok((
        EclipseKind::Partial,
        (area / std::f64::consts::PI).clamp(0.0, 1.0),
    ))
}

/// A receive-time spatial field. Vectors are relative to the anchor observer at
/// each body's independently converged retarded epoch, in the common ECEF axes.
/// The emission velocities linearly extend those states for other positions;
/// a fresh ephemeris sample is authoritative at its own observer anchor.
#[derive(Clone, Copy, Debug)]
pub struct SolarEclipseField {
    pub anchor_ecef_m: Vec3d,
    pub sun_relative_ecef_m: Vec3d,
    pub moon_relative_ecef_m: Vec3d,
    pub sun_velocity_ecef_mps: Vec3d,
    pub moon_velocity_ecef_mps: Vec3d,
    pub observer_velocity_ecef_mps: Vec3d,
    pub spin_ecef_radians_per_second: Vec3d,
    pub sun_radius_m: f64,
    pub moon_radius_m: f64,
}
impl SolarEclipseField {
    fn retarded_relative(initial: Vec3d, velocity: Vec3d, offset: Vec3d) -> Vec3d {
        let distance = initial.length();
        let mut delta = 0.0;
        for _ in 0..6 {
            let shift = -offset - velocity * delta;
            // Difference of lengths without subtracting two large distances.
            delta = shift.dot(initial * 2.0 + shift)
                / ((initial + shift).length() + distance)
                / LIGHT_SPEED_MPS;
        }
        initial - offset - velocity * delta
    }

    pub fn apparent_vectors(self, position_ecef_m: Vec3d) -> Result<(Vec3d, Vec3d), AstroError> {
        if ![
            position_ecef_m,
            self.anchor_ecef_m,
            self.sun_relative_ecef_m,
            self.moon_relative_ecef_m,
            self.sun_velocity_ecef_mps,
            self.moon_velocity_ecef_mps,
            self.observer_velocity_ecef_mps,
            self.spin_ecef_radians_per_second,
        ]
        .iter()
        .all(|v| v.is_finite())
            || position_ecef_m.length() > 1.1e9
            || self.anchor_ecef_m.length() > 1.1e9
            || !self.sun_radius_m.is_finite()
            || !self.moon_radius_m.is_finite()
            || self.sun_radius_m <= 0.0
            || self.moon_radius_m <= 0.0
            || self.sun_velocity_ecef_mps.length() >= LIGHT_SPEED_MPS
            || self.moon_velocity_ecef_mps.length() >= LIGHT_SPEED_MPS
        {
            return Err(AstroError::InvalidObserver);
        }
        let offset = position_ecef_m - self.anchor_ecef_m;
        let velocity =
            self.observer_velocity_ecef_mps + self.spin_ecef_radians_per_second.cross(offset);
        if !velocity.is_finite() || velocity.length() >= LIGHT_SPEED_MPS {
            return Err(AstroError::InvalidObserver);
        }
        let apparent = |initial, target_velocity| {
            let relative = Self::retarded_relative(initial, target_velocity, offset);
            let distance = relative.length();
            let direction = relative.normalized().ok_or(AstroError::InvalidObserver)?;
            Ok(super::bodies::aberration(direction, velocity) * distance)
        };
        Ok((
            apparent(self.sun_relative_ecef_m, self.sun_velocity_ecef_mps)?,
            apparent(self.moon_relative_ecef_m, self.moon_velocity_ecef_mps)?,
        ))
    }

    pub fn sample(self, position_ecef_m: Vec3d) -> Result<SolarEclipse, AstroError> {
        let (sun, moon) = self.apparent_vectors(position_ecef_m)?;
        let sun_distance = sun.length();
        let moon_distance = moon.length();
        if sun_distance <= self.sun_radius_m || moon_distance <= self.moon_radius_m {
            return Err(AstroError::InvalidObserver);
        }
        let sun_direction = sun / sun_distance;
        let moon_direction = moon / moon_distance;
        let alpha = (self.sun_radius_m / sun_distance).asin();
        let beta = (self.moon_radius_m / moon_distance).asin();
        let separation = sun_direction
            .cross(moon_direction)
            .length()
            .atan2(sun_direction.dot(moon_direction));
        // With disjoint physical spheres, the nearer lunar center determines
        // which angular silhouette can occult the more distant solar one.
        let (kind, obscuration) = if moon_distance < sun_distance {
            disk_obscuration(alpha, beta, separation)?
        } else {
            (EclipseKind::None, 0.0)
        };
        let elevation = ecef_to_geodetic(position_ecef_m).ok().map(|point| {
            EnuBasis::at(point)
                .up
                .dot(sun_direction)
                .clamp(-1.0, 1.0)
                .asin()
                .to_degrees()
        });
        Ok(SolarEclipse {
            kind,
            obscuration,
            visible_fraction: 1.0 - obscuration,
            sun_angular_radius_rad: alpha,
            moon_angular_radius_rad: beta,
            separation_rad: separation,
            sun_elevation_deg: elevation,
        })
    }

    /// Rotate into a Sun-aligned frame in f64 before quantization. In particular,
    /// lunar transverse coordinates retain metre/submetre f32 resolution instead
    /// of losing it to subtraction of nearly parallel, Earth-scale vectors.
    /// Requires a valid sampled field and a finite orthonormal local frame.
    pub fn gpu(self, local: crate::rendering::LocalFrame) -> GpuSolarEclipse {
        let z = self.sun_relative_ecef_m.normalized().unwrap();
        let reference = if z.z.abs() < 0.9 {
            Vec3d::new(0.0, 0.0, 1.0)
        } else {
            Vec3d::new(1.0, 0.0, 0.0)
        };
        let x = reference.cross(z).normalized().unwrap();
        let y = z.cross(x);
        let axes = [x, y, z];
        let rotation = Mat3d {
            rows: axes.map(|axis| [axis.x, axis.y, axis.z]),
        };
        let transform = |v: Vec3d, w: f64| {
            let a = rotation.transform(v);
            [a.x as f32, a.y as f32, a.z as f32, w as f32]
        };
        GpuSolarEclipse {
            local_to_shadow: axes.map(|axis| {
                [
                    axis.dot(local.basis.east) as f32,
                    axis.dot(local.basis.up) as f32,
                    -axis.dot(local.basis.north) as f32,
                    axis.dot(local.origin - self.anchor_ecef_m) as f32,
                ]
            }),
            sun: transform(self.sun_relative_ecef_m, self.sun_radius_m),
            moon: transform(self.moon_relative_ecef_m, self.moon_radius_m),
            sun_velocity: transform(self.sun_velocity_ecef_mps, 0.0),
            moon_velocity: transform(self.moon_velocity_ecef_mps, 0.0),
            observer_beta: transform(self.observer_velocity_ecef_mps / LIGHT_SPEED_MPS, 0.0),
            spin_over_c: transform(self.spin_ecef_radians_per_second / LIGHT_SPEED_MPS, 0.0),
        }
    }
}

/// Each row transforms a local vertex: dot(row.xyz,position)+row.w.
/// Body vectors and velocities then use these shadow axes. Sun/Moon .w is radius.
#[derive(Clone, Copy, Debug)]
pub struct GpuSolarEclipse {
    pub local_to_shadow: [[f32; 4]; 3],
    pub sun: [f32; 4],
    pub moon: [f32; 4],
    pub sun_velocity: [f32; 4],
    pub moon_velocity: [f32; 4],
    pub observer_beta: [f32; 4],
    pub spin_over_c: [f32; 4],
}

impl AstronomyFrame {
    pub fn solar_eclipse(&self, position_ecef_m: Vec3d) -> Result<SolarEclipse, AstroError> {
        self.observer_eclipse.sample(position_ecef_m)
    }
}
