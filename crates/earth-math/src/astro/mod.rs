//! Finite-range scientific astronomy. See docs/science/astronomy.md.
use crate::Vec3d;
use std::fmt;

mod bodies;
mod eclipse;
mod eop;
mod orientation;
#[cfg(test)]
mod orientation_review;
mod reference_frames;
mod spk;
mod time;
pub use bodies::{iau_body_orientation, tdb_minus_tt_seconds};
pub use eclipse::{
    EclipseKind, GpuSolarEclipse, SolarEclipse, SolarEclipseField, disk_obscuration,
};
pub use eop::{EarthOrientationData, EarthRotationQuality};
pub use reference_frames::{ObserverTracking, TrackingError, terrestrial_rotation};
pub use spk::{CartesianState, Ephemeris};
pub use time::{AstroInstant, CalendarDateTime, SimulationClock};

pub const SUN_RADIUS_M: f64 = 695_700_000.0;
pub const MOON_RADIUS_M: f64 = 1_737_400.0;
pub const LIGHT_SPEED_MPS: f64 = 299_792_458.0;
pub const JULIAN_DAY_J2000: f64 = 2_451_545.0;
pub const SECONDS_PER_DAY: f64 = 86_400.0;
pub(crate) const ARCSEC_RAD: f64 = std::f64::consts::PI / (180.0 * 3600.0);

#[derive(Clone, Debug, PartialEq)]
pub enum AstroError {
    InvalidDate,
    UnsupportedDate,
    InvalidStep,
    InvalidKernel,
    MissingBody(i32),
    KernelCoverage,
    InvalidEarthOrientationData,
    InvalidObserver,
}
impl fmt::Display for AstroError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDate => f.write_str("Use a valid YYYY-MM-DD HH:MM:SS calendar date; leap seconds must be published."),
            Self::UnsupportedDate => f.write_str("Astronomy supports calendar years 1850 through 2149."),
            Self::InvalidStep => f.write_str("Time step and playback rate must be finite and stay within the supported calendar."),
            Self::InvalidKernel => f.write_str("Invalid or unsupported JPL SPK Type 2 ephemeris."),
            Self::MissingBody(id) => write!(f, "The ephemeris has no complete center chain for body {id}."),
            Self::KernelCoverage => f.write_str("Requested time is outside the ephemeris segment coverage."),
            Self::InvalidEarthOrientationData => f.write_str("Invalid IERS Earth-orientation records."),
            Self::InvalidObserver => f.write_str("Observer position must be finite."),
        }
    }
}
impl std::error::Error for AstroError {}

/// Row-major matrix acting on column vectors. Rotations use ERFA's passive
/// convention; matrix multiplication composes rightmost transformation first.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat3d {
    pub rows: [[f64; 3]; 3],
}
impl Mat3d {
    pub const fn identity() -> Self {
        Self {
            rows: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
        }
    }
    pub fn transform(self, v: Vec3d) -> Vec3d {
        let r = self.rows;
        Vec3d::new(
            r[0][0] * v.x + r[0][1] * v.y + r[0][2] * v.z,
            r[1][0] * v.x + r[1][1] * v.y + r[1][2] * v.z,
            r[2][0] * v.x + r[2][1] * v.y + r[2][2] * v.z,
        )
    }
    pub fn transpose(self) -> Self {
        Self {
            rows: std::array::from_fn(|i| std::array::from_fn(|j| self.rows[j][i])),
        }
    }
    #[allow(clippy::should_implement_trait)]
    pub fn mul(self, rhs: Self) -> Self {
        Self {
            rows: std::array::from_fn(|i| {
                std::array::from_fn(|j| (0..3).map(|k| self.rows[i][k] * rhs.rows[k][j]).sum())
            }),
        }
    }
    pub fn rotation_x(a: f64) -> Self {
        let (s, c) = a.sin_cos();
        Self {
            rows: [[1., 0., 0.], [0., c, s], [0., -s, c]],
        }
    }
    pub fn rotation_y(a: f64) -> Self {
        let (s, c) = a.sin_cos();
        Self {
            rows: [[c, 0., -s], [0., 1., 0.], [s, 0., c]],
        }
    }
    pub fn rotation_z(a: f64) -> Self {
        let (s, c) = a.sin_cos();
        Self {
            rows: [[c, s, 0.], [-s, c, 0.], [0., 0., 1.]],
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CelestialBody {
    /// Simultaneous geometric center relative to Earth's center.
    pub center_ecef_m: Vec3d,
    /// Retarded center after light time and observer stellar aberration.
    pub apparent_center_ecef_m: Vec3d,
    pub radius_m: f64,
    /// IAU text-PCK body orientation at its retarded emission epoch.
    pub body_to_ecef: Mat3d,
    pub light_time_seconds: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct AstroQuality {
    pub earth_rotation: EarthRotationQuality,
    pub historical_ut1_calendar: bool,
    pub future_leap_seconds_assumed: bool,
    pub outside_nutation_reference_interval: bool,
}
impl AstroQuality {
    pub fn description(self) -> String {
        let mut result = self.earth_rotation.description().to_string();
        if self.historical_ut1_calendar {
            result.push_str("; pre-1960 calendar is modeled UT1, not UTC");
        }
        if self.future_leap_seconds_assumed {
            result.push_str("; future leap seconds unknown, last published offset held");
        }
        if self.outside_nutation_reference_interval {
            result.push_str("; outside 1900–2100 abridged-nutation accuracy interval");
        }
        result
    }
}

#[derive(Clone, Copy, Debug)]
pub struct AstronomyFrame {
    pub sun: CelestialBody,
    pub moon: CelestialBody,
    pub icrf_to_ecef: Mat3d,
    pub earth_rotation_angle_rad: f64,
    /// Display/reference convenience. Computation retains J2000-relative seconds.
    pub tt_jd: f64,
    pub tdb_jd: f64,
    pub ut1_jd: f64,
    pub quality: AstroQuality,
    pub moon_illuminated_fraction: f64,
    /// Unit direction from Moon toward Sun in the common ECEF axes.
    pub moon_to_sun_direction_ecef: Vec3d,
    pub eclipse: SolarEclipseField,
    /// Apparent-disk field for the observer's selected motion convention.
    /// `eclipse` remains the co-rotating surface field used by ground shading.
    pub observer_eclipse: SolarEclipseField,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CelestialKind {
    Sun,
    Moon,
}
impl CelestialKind {
    pub fn description(self) -> &'static str {
        match self {
            Self::Sun => "Sun / apparent nominal sphere",
            Self::Moon => "Moon / apparent mean-radius sphere",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CelestialHit {
    pub body: CelestialKind,
    pub distance_m: f64,
    pub position_ecef_m: Vec3d,
    pub normal_ecef: Vec3d,
}

impl AstronomyFrame {
    /// Nearest apparent spherical Sun/Moon surface, only when it is nearer
    /// than an optional canonical Earth hit. Numerical tangencies use the same
    /// dimensionless roundoff band as the reference-ellipsoid ray solver.
    /// This measures the displayed apparent sphere, not a surface-specific
    /// light-time solution or lunar topographic range.
    pub fn nearest_celestial(
        &self,
        ray: crate::Ray,
        terrain_distance: Option<f64>,
    ) -> Option<CelestialHit> {
        if !ray.origin.is_finite() {
            return None;
        }
        let direction = ray.direction.normalized()?;
        let mut nearest = terrain_distance
            .filter(|value| value.is_finite() && *value >= 0.0)
            .unwrap_or(f64::INFINITY);
        let mut selected = None;
        for (body, state) in [
            (CelestialKind::Sun, self.sun),
            (CelestialKind::Moon, self.moon),
        ] {
            let radius = state.radius_m;
            if !radius.is_finite() || radius <= 0.0 || !state.apparent_center_ecef_m.is_finite() {
                continue;
            }
            let origin = (ray.origin - state.apparent_center_ecef_m) / radius;
            let norm = origin.length();
            let b = origin.dot(direction);
            let c = (norm - 1.0) * (norm + 1.0);
            let cross_squared = origin.cross(direction).dot(origin.cross(direction));
            let discriminant = 1.0 - cross_squared;
            let tolerance = 32.0 * f64::EPSILON * cross_squared.max(1.0);
            if !discriminant.is_finite() || discriminant < -tolerance {
                continue;
            }
            let q = -b - discriminant.max(0.0).sqrt().copysign(b);
            let roots = if q == 0.0 { [-b, -b] } else { [q, c / q] };
            for root in roots {
                let distance = root * radius;
                if distance.is_finite() && distance >= 0.0 && distance < nearest {
                    let position = ray.origin + direction * distance;
                    let normal = (position - state.apparent_center_ecef_m).normalized()?;
                    nearest = distance;
                    selected = Some(CelestialHit {
                        body,
                        distance_m: distance,
                        position_ecef_m: position,
                        normal_ecef: normal,
                    });
                }
            }
        }
        selected
    }
}
