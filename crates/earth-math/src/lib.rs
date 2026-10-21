//! WGS84 reference-ellipsoid geometry, in metres and double precision.
//!
//! Includes height-free regional categories, native EGM2008 readouts, and
//! optional weather-informed curved optics with explicit profile assumptions.
//! Archived terrain APIs remain for tests. Tides and survey accuracy are absent.
//! See `docs/science/model.md` for conventions, sources and numerical limits.

use std::f64::consts::PI;
use std::fmt;
use std::ops::{Add, Div, Mul, Neg, Sub};

pub mod atmosphere;
pub mod boundary;
mod geodesic;
mod geoid;
pub use geodesic::wgs84_surface_distance;
pub mod guides;
pub mod rendering;
mod terrain;
pub use geoid::{GeoidError, GeoidGrid, VerticalReport};
pub use terrain::{
    PatchTextureView, SkylineMeasurement, SkylineMethod, SurfaceHit, SurfaceKind, TerrainError,
    TerrainPatch, TerrainScene, TextureProjection, WaterKind,
};

pub const WGS84_A_M: f64 = 6_378_137.0;
pub const WGS84_INVERSE_FLATTENING: f64 = 298.257_223_563;
pub const WGS84_F: f64 = 1.0 / WGS84_INVERSE_FLATTENING;
pub const WGS84_B_M: f64 = WGS84_A_M * (1.0 - WGS84_F);
pub const WGS84_E2: f64 = WGS84_F * (2.0 - WGS84_F);
pub const METRES_PER_FOOT: f64 = 0.3048;
pub const METRES_PER_MILE: f64 = 1_609.344;
pub const STATURE_M: f64 = 6.0 * METRES_PER_FOOT;
/// An explicit configurable assumption, not a deduction from six-foot stature.
pub const DEFAULT_EYE_HEIGHT_M: f64 = 1.72;
/// Configurable crouching pose assumption, independent of six-foot stature.
pub const DEFAULT_CROUCH_EYE_HEIGHT_M: f64 = 1.0;
pub const MIN_HEIGHT_M: f64 = -10_000.0;
pub const MAX_HEIGHT_M: f64 = 1_000_000_000.0;
pub const MIN_FOV_DEG: f64 = 0.000_001;
pub const DEFAULT_FOV_DEG: f64 = 60.0;
pub const MIN_SPEED_MPS: f64 = 0.05;
pub const MAX_SPEED_MPS: f64 = 50_000_000.0;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3d {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3d {
    pub const ZERO: Self = Self::new(0.0, 0.0, 0.0);
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }
    pub fn dot(self, rhs: Self) -> f64 {
        self.x.mul_add(rhs.x, self.y.mul_add(rhs.y, self.z * rhs.z))
    }
    pub fn cross(self, rhs: Self) -> Self {
        Self::new(
            self.y * rhs.z - self.z * rhs.y,
            self.z * rhs.x - self.x * rhs.z,
            self.x * rhs.y - self.y * rhs.x,
        )
    }
    pub fn length(self) -> f64 {
        self.x.hypot(self.y).hypot(self.z)
    }
    pub fn normalized(self) -> Option<Self> {
        let n = self.length();
        (n.is_finite() && n > 0.0).then(|| self / n)
    }
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
    pub fn distance(self, rhs: Self) -> f64 {
        (self - rhs).length()
    }
}

impl Add for Vec3d {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y, self.z + rhs.z)
    }
}
impl Sub for Vec3d {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self::new(self.x - rhs.x, self.y - rhs.y, self.z - rhs.z)
    }
}
impl Mul<f64> for Vec3d {
    type Output = Self;
    fn mul(self, rhs: f64) -> Self {
        Self::new(self.x * rhs, self.y * rhs, self.z * rhs)
    }
}
impl Div<f64> for Vec3d {
    type Output = Self;
    fn div(self, rhs: f64) -> Self {
        Self::new(self.x / rhs, self.y / rhs, self.z / rhs)
    }
}
impl Neg for Vec3d {
    type Output = Self;
    fn neg(self) -> Self {
        self * -1.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeometryError {
    NonFinite,
    InvalidDirection,
    Latitude,
    Longitude,
    Height,
    ObserverHeight,
    UnsupportedInterior,
    GeodesicSolution,
}
impl fmt::Display for GeometryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NonFinite => "Coordinates must be finite numbers.",
            Self::InvalidDirection => "The target must define a finite nonzero direction from the eyes.",
            Self::Latitude => "Latitude must be between -90 and 90 degrees.",
            Self::Longitude => "Longitude must be between -180 and 180 degrees.",
            Self::Height => "Ellipsoidal height must be between -10,000 and 1,000,000,000 metres.",
            Self::ObserverHeight => "Feet height must be nonnegative and leave room for the eye height below the 1,000,000,000 metre limit.",
            Self::UnsupportedInterior => "Position is inside the unsupported deep Earth interior.",
            Self::GeodesicSolution => "The reference-ellipsoid geodesic did not return a finite nonnegative distance.",
        })
    }
}
impl std::error::Error for GeometryError {}

/// Geodetic latitude, east longitude, and ellipsoidal height (not mean sea level).
/// Construct with `new`; public fields permit convenient UI display.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geodetic {
    pub lat_deg: f64,
    pub lon_deg: f64,
    pub height_m: f64,
}
impl Geodetic {
    pub fn new(lat_deg: f64, lon_deg: f64, height_m: f64) -> Result<Self, GeometryError> {
        if !lat_deg.is_finite() || !lon_deg.is_finite() || !height_m.is_finite() {
            return Err(GeometryError::NonFinite);
        }
        if !(-90.0..=90.0).contains(&lat_deg) {
            return Err(GeometryError::Latitude);
        }
        if !(-180.0..=180.0).contains(&lon_deg) {
            return Err(GeometryError::Longitude);
        }
        if !(MIN_HEIGHT_M..=MAX_HEIGHT_M).contains(&height_m) {
            return Err(GeometryError::Height);
        }
        Ok(Self {
            lat_deg,
            lon_deg,
            height_m,
        })
    }
    pub fn with_height(self, height_m: f64) -> Result<Self, GeometryError> {
        Self::new(self.lat_deg, self.lon_deg, height_m)
    }
}

pub fn geodetic_to_ecef(g: Geodetic) -> Vec3d {
    let (s, c) = g.lat_deg.to_radians().sin_cos();
    let (sl, cl) = g.lon_deg.to_radians().sin_cos();
    let n = WGS84_A_M / (1.0 - WGS84_E2 * s * s).sqrt();
    Vec3d::new(
        (n + g.height_m) * c * cl,
        (n + g.height_m) * c * sl,
        (n * (1.0 - WGS84_E2) + g.height_m) * s,
    )
}

/// Inverse for the documented near-surface/exterior domain, not the ellipsoid evolute.
pub fn ecef_to_geodetic(p: Vec3d) -> Result<Geodetic, GeometryError> {
    let g = inverse_near_surface(p)?;
    if !(MIN_HEIGHT_M - 0.000_01..=MAX_HEIGHT_M + 0.000_01).contains(&g.height_m) {
        return Err(GeometryError::Height);
    }
    Geodetic::new(
        g.lat_deg,
        g.lon_deg,
        g.height_m.clamp(MIN_HEIGHT_M, MAX_HEIGHT_M),
    )
}

// The private flight path may temporarily overshoot the public height interval
// before being clamped. Its inverse is otherwise the same calculation.
fn inverse_near_surface(p: Vec3d) -> Result<Geodetic, GeometryError> {
    if !p.is_finite() {
        return Err(GeometryError::NonFinite);
    }
    if p.length() < WGS84_B_M + MIN_HEIGHT_M - 0.001 {
        return Err(GeometryError::UnsupportedInterior);
    }
    let r = p.x.hypot(p.y);
    let lon = if r == 0.0 {
        0.0
    } else {
        p.y.atan2(p.x).to_degrees()
    };
    let mut lat = p.z.atan2(r * (1.0 - WGS84_E2));
    for _ in 0..20 {
        let s = lat.sin();
        let n = WGS84_A_M / (1.0 - WGS84_E2 * s * s).sqrt();
        let next = (p.z + WGS84_E2 * n * s).atan2(r);
        let change = (next - lat).abs();
        lat = next;
        if change < 2.0 * f64::EPSILON {
            break;
        }
    }
    let (s, c) = lat.sin_cos();
    // Dot the displacement from the ellipsoid with geodetic up. This avoids
    // p/cos(latitude) near poles and z/sin(latitude) near the equator.
    let height = r * c + p.z * s - WGS84_A_M * (1.0 - WGS84_E2 * s * s).sqrt();
    Ok(Geodetic {
        lat_deg: lat.to_degrees(),
        lon_deg: lon,
        height_m: height,
    })
}

#[derive(Clone, Copy, Debug)]
pub struct EnuBasis {
    pub east: Vec3d,
    pub north: Vec3d,
    pub up: Vec3d,
}
impl EnuBasis {
    pub fn at(g: Geodetic) -> Self {
        let (s, c) = g.lat_deg.to_radians().sin_cos();
        let (sl, cl) = g.lon_deg.to_radians().sin_cos();
        Self {
            east: Vec3d::new(-sl, cl, 0.0),
            north: Vec3d::new(-s * cl, -s * sl, c),
            up: Vec3d::new(c * cl, c * sl, s),
        }
    }
}

/// Heading is clockwise from north; positive pitch looks up. The full triad
/// may include roll about forward; `new` retains the zero-roll convention.
#[derive(Clone, Copy, Debug)]
pub struct CameraFrame {
    pub origin: Vec3d,
    pub forward: Vec3d,
    pub right: Vec3d,
    pub up: Vec3d,
}
impl CameraFrame {
    pub fn new(eye: Geodetic, heading_deg: f64, pitch_deg: f64) -> Self {
        let enu = EnuBasis::at(eye);
        let (sh, ch) = heading_deg.to_radians().sin_cos();
        let (sp, cp) = pitch_deg.to_radians().sin_cos();
        let right = enu.east * ch - enu.north * sh;
        let forward = (enu.north * ch + enu.east * sh) * cp + enu.up * sp;
        Self {
            origin: geodetic_to_ecef(eye),
            forward,
            right,
            up: right.cross(forward),
        }
    }
    pub fn center_ray(self) -> Ray {
        Ray {
            origin: self.origin,
            direction: self.forward,
        }
    }
    /// Positive roll is right-handed about forward, in degrees.
    pub fn with_roll(eye: Geodetic, heading_deg: f64, pitch_deg: f64, roll_deg: f64) -> Self {
        let mut frame = Self::new(eye, heading_deg, pitch_deg);
        let (s, c) = roll_deg.to_radians().sin_cos();
        let right = frame.right * c - frame.up * s;
        frame.up = frame.up * c + frame.right * s;
        frame.right = right;
        frame
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Ray {
    pub origin: Vec3d,
    /// May be nonunit; intersection reports Euclidean metres, not a ray parameter.
    pub direction: Vec3d,
}
impl Ray {
    pub fn new(origin: Vec3d, direction: Vec3d) -> Option<Self> {
        origin.is_finite().then_some(())?;
        Some(Self {
            origin,
            direction: direction.normalized()?,
        })
    }
}
#[derive(Clone, Copy, Debug)]
pub struct RayHit {
    pub distance_m: f64,
    pub position: Vec3d,
    pub normal: Vec3d,
    pub geodetic: Geodetic,
}

/// Nearest nonnegative intersection with the WGS84 reference ellipsoid.
/// A tangent inside floating-point uncertainty is treated as touching.
pub fn intersect_wgs84(ray: Ray) -> Option<RayHit> {
    let (direction, distances) = wgs84_distances(ray)?;
    let distance = distances
        .into_iter()
        .filter(|t| *t >= 0.0)
        .min_by(f64::total_cmp)?;
    wgs84_hit(ray.origin, direction, distance)
}

// Both roots are required when a regional mesh replaces part of the ellipsoid:
// a masked near root does not necessarily mask the far root.
fn wgs84_distances(ray: Ray) -> Option<(Vec3d, [f64; 2])> {
    if !ray.origin.is_finite() {
        return None;
    }
    let direction = ray.direction.normalized()?;
    let o = Vec3d::new(
        ray.origin.x / WGS84_A_M,
        ray.origin.y / WGS84_A_M,
        ray.origin.z / WGS84_B_M,
    );
    let d = Vec3d::new(
        direction.x / WGS84_A_M,
        direction.y / WGS84_A_M,
        direction.z / WGS84_B_M,
    );
    let scale = d.length();
    let u = d / scale;
    let b = o.dot(u);
    // Cross-product form avoids subtracting b² and c for distant observers.
    let cross = o.cross(u);
    let cross2 = cross.dot(cross);
    let mut discriminant = 1.0 - cross2;
    let tolerance = 32.0 * f64::EPSILON * cross2.max(1.0);
    if discriminant < -tolerance {
        return None;
    }
    discriminant = discriminant.max(0.0);
    let norm = o.length();
    let c = (norm - 1.0) * (norm + 1.0);
    let q = -b - discriminant.sqrt().copysign(b);
    let (t0, t1) = if q == 0.0 { (-b, -b) } else { (q, c / q) };
    Some((direction, [t0.min(t1) / scale, t0.max(t1) / scale]))
}

fn wgs84_hit(origin: Vec3d, direction: Vec3d, distance_m: f64) -> Option<RayHit> {
    let position = origin + direction * distance_m;
    let normal = Vec3d::new(
        position.x / (WGS84_A_M * WGS84_A_M),
        position.y / (WGS84_A_M * WGS84_A_M),
        position.z / (WGS84_B_M * WGS84_B_M),
    )
    .normalized()?;
    let geodetic = Geodetic {
        lat_deg: normal.z.atan2(normal.x.hypot(normal.y)).to_degrees(),
        lon_deg: position.y.atan2(position.x).to_degrees(),
        height_m: 0.0,
    };
    Some(RayHit {
        distance_m,
        position,
        normal,
        geodetic,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Projection {
    Rectilinear,
    EquidistantFisheye,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lens {
    pub projection: Projection,
    pub vertical_fov_deg: f64,
}
impl Default for Lens {
    fn default() -> Self {
        Self {
            projection: Projection::Rectilinear,
            vertical_fov_deg: DEFAULT_FOV_DEG,
        }
    }
}
impl Lens {
    pub fn new(projection: Projection, vertical_fov_deg: f64) -> Self {
        let maximum = match projection {
            Projection::Rectilinear => 150.0,
            Projection::EquidistantFisheye => 180.0,
        };
        Self {
            projection,
            vertical_fov_deg: if vertical_fov_deg.is_finite() {
                vertical_fov_deg.clamp(MIN_FOV_DEG, maximum)
            } else {
                DEFAULT_FOV_DEG
            },
        }
    }
    pub fn zoom(&mut self, wheel_lines: f64) {
        if wheel_lines.is_finite() {
            *self = Self::new(
                self.projection,
                self.vertical_fov_deg * (-wheel_lines.clamp(-100.0, 100.0) * 0.15).exp(),
            );
        }
    }
    pub fn magnification(self) -> f64 {
        let fov = Self::new(self.projection, self.vertical_fov_deg).vertical_fov_deg;
        match self.projection {
            Projection::Rectilinear => {
                (DEFAULT_FOV_DEG.to_radians() * 0.5).tan() / (fov.to_radians() * 0.5).tan()
            }
            Projection::EquidistantFisheye => DEFAULT_FOV_DEG / fov,
        }
    }
    /// NDC coordinates: x right and y up. Fisheye has a circular aperture whose
    /// vertical diameter is the image height; pixels outside it have no ray.
    pub fn ray(self, frame: CameraFrame, ndc_x: f64, ndc_y: f64, aspect: f64) -> Option<Vec3d> {
        if !ndc_x.is_finite() || !ndc_y.is_finite() || !aspect.is_finite() || aspect <= 0.0 {
            return None;
        }
        let lens = Self::new(self.projection, self.vertical_fov_deg);
        let half = lens.vertical_fov_deg.to_radians() * 0.5;
        let x = ndc_x * aspect;
        let y = ndc_y;
        let local = match lens.projection {
            Projection::Rectilinear => Vec3d::new(x * half.tan(), y * half.tan(), 1.0),
            Projection::EquidistantFisheye => {
                let radius = x.hypot(y);
                if radius > 1.0 {
                    return None;
                }
                if radius == 0.0 {
                    return Some(frame.forward);
                }
                let theta = radius * half;
                Vec3d::new(
                    x / radius * theta.sin(),
                    y / radius * theta.sin(),
                    theta.cos(),
                )
            }
        };
        (frame.right * local.x + frame.up * local.y + frame.forward * local.z).normalized()
    }
    /// Direction-to-NDC inverse. Returns None outside the current aperture/view.
    pub fn project(self, frame: CameraFrame, direction: Vec3d, aspect: f64) -> Option<[f64; 2]> {
        if !aspect.is_finite() || aspect <= 0.0 {
            return None;
        }
        let d = direction.normalized()?;
        let x = d.dot(frame.right);
        let y = d.dot(frame.up);
        let z = d.dot(frame.forward);
        let half = Self::new(self.projection, self.vertical_fov_deg)
            .vertical_fov_deg
            .to_radians()
            * 0.5;
        let result = match self.projection {
            Projection::Rectilinear => {
                if z <= 0.0 {
                    return None;
                }
                [x / (z * half.tan() * aspect), y / (z * half.tan())]
            }
            Projection::EquidistantFisheye => {
                let radius = x.hypot(y);
                let theta = radius.atan2(z);
                if theta > half + 8.0 * f64::EPSILON {
                    return None;
                }
                if radius <= f64::MIN_POSITIVE {
                    [0.0, 0.0]
                } else {
                    [
                        theta / half * x / radius / aspect,
                        theta / half * y / radius,
                    ]
                }
            }
        };
        (result[0].abs() <= 1.0 && result[1].abs() <= 1.0).then_some(result)
    }
    pub fn pixel_ray(
        self,
        frame: CameraFrame,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> Option<Ray> {
        self.pixel_ray_for_view(frame, x, y, width, height, [width as f32, height as f32])
    }
    /// Pixel sampling at a reduced raster resolution with the exact eventual
    /// display aspect, keeping projected overlays aligned after resizing.
    pub fn pixel_ray_for_view(
        self,
        frame: CameraFrame,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        display_size: [f32; 2],
    ) -> Option<Ray> {
        if width == 0 || height == 0 || x >= width || y >= height {
            return None;
        }
        if !display_size.iter().all(|v| v.is_finite() && *v > 0.0) {
            return None;
        }
        let dx = 2.0 * (x as f64 + 0.5) / width as f64 - 1.0;
        let dy = 1.0 - 2.0 * (y as f64 + 0.5) / height as f64;
        Ray::new(
            frame.origin,
            self.ray(
                frame,
                dx,
                dy,
                f64::from(display_size[0]) / f64::from(display_size[1]),
            )?,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum UnitSystem {
    #[default]
    Metric,
    Imperial,
}
impl UnitSystem {
    pub fn length_from_metres(self, value: f64) -> f64 {
        match self {
            Self::Metric => value,
            Self::Imperial => value / METRES_PER_FOOT,
        }
    }
    pub fn length_to_metres(self, value: f64) -> f64 {
        match self {
            Self::Metric => value,
            Self::Imperial => value * METRES_PER_FOOT,
        }
    }
    pub fn short_unit(self) -> &'static str {
        match self {
            Self::Metric => "m",
            Self::Imperial => "ft",
        }
    }
    pub fn format_distance(self, metres: f64) -> String {
        match self {
            Self::Metric if metres.abs() >= 1000.0 => format!("{:.3} km", metres / 1000.0),
            Self::Metric => format!("{metres:.3} m"),
            Self::Imperial if metres.abs() >= METRES_PER_MILE => {
                format!("{:.3} mi", metres / METRES_PER_MILE)
            }
            Self::Imperial => format!("{:.3} ft", metres / METRES_PER_FOOT),
        }
    }
    /// Unscaled numerical readout rounded to six decimal places. The displayed
    /// decimal places express formatting, not physical accuracy or uncertainty.
    pub fn format_precise_distance(self, metres: f64) -> String {
        format!(
            "{:.6} {}",
            self.length_from_metres(metres),
            self.short_unit()
        )
    }
    pub fn format_speed(self, metres_per_second: f64) -> String {
        match self {
            Self::Metric => format!("{metres_per_second:.2} m/s"),
            Self::Imperial => format!("{:.2} ft/s", metres_per_second / METRES_PER_FOOT),
        }
    }
}

pub fn adjust_speed(speed_mps: f64, wheel_lines: f64) -> f64 {
    if !wheel_lines.is_finite() || !speed_mps.is_finite() {
        return MIN_SPEED_MPS;
    }
    (speed_mps * (wheel_lines.clamp(-100.0, 100.0) * 0.25).exp())
        .clamp(MIN_SPEED_MPS, MAX_SPEED_MPS)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EyePosture {
    pub standing_eye_height_m: f64,
    pub crouching_eye_height_m: f64,
    pub crouched: bool,
}
impl Default for EyePosture {
    fn default() -> Self {
        Self {
            standing_eye_height_m: DEFAULT_EYE_HEIGHT_M,
            crouching_eye_height_m: DEFAULT_CROUCH_EYE_HEIGHT_M,
            crouched: false,
        }
    }
}
impl EyePosture {
    pub fn set_standing_height(&mut self, metres: f64) {
        if metres.is_finite() {
            self.standing_eye_height_m = metres.clamp(0.1, STATURE_M);
        }
    }
    pub fn set_crouching_height(&mut self, metres: f64) {
        if metres.is_finite() {
            self.crouching_eye_height_m = metres.clamp(0.1, STATURE_M);
        }
    }
    pub fn effective_height_m(self) -> f64 {
        if self.crouched {
            self.crouching_eye_height_m.min(self.standing_eye_height_m)
        } else {
            self.standing_eye_height_m
        }
    }
    /// Changes only eye configuration. Reject unsupported headroom rather than
    /// moving the feet. Standing headroom remains reserved while crouched.
    pub fn apply(self, observer: &mut Observer) -> Result<(), GeometryError> {
        if ![self.standing_eye_height_m, self.crouching_eye_height_m]
            .iter()
            .all(|h| h.is_finite() && (0.1..=STATURE_M).contains(h))
            || observer.feet.height_m > MAX_HEIGHT_M - self.standing_eye_height_m
        {
            return Err(GeometryError::ObserverHeight);
        }
        observer.eye_height_m = self.effective_height_m();
        observer.reserved_eye_height_m = self.standing_eye_height_m;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Observer {
    /// Feet ellipsoidal height; scene navigation permits valid negative terrain.
    pub feet: Geodetic,
    pub heading_deg: f64,
    pub pitch_deg: f64,
    /// Right-handed rotation about the forward sight direction.
    pub roll_deg: f64,
    pub eye_height_m: f64,
    reserved_eye_height_m: f64,
    pub speed_mps: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenRect {
    pub min: [f32; 2],
    pub max: [f32; 2],
}

/// A filled, non-antialiased center row exactly one framebuffer pixel high.
/// Coordinates are logical display units; this is an overlay, not the skyline.
pub fn physical_horizontal_line(
    display: [f32; 2],
    framebuffer_scale: [f32; 2],
) -> Option<ScreenRect> {
    if !display
        .into_iter()
        .chain(framebuffer_scale)
        .all(|v| v.is_finite() && v > 0.0)
    {
        return None;
    }
    let physical_height = f64::from(display[1]) * f64::from(framebuffer_scale[1]);
    if !(1.0..=16_777_216.0).contains(&physical_height) {
        return None;
    }
    let row = (physical_height.floor() * 0.5).floor();
    Some(ScreenRect {
        min: [0.0, (row / f64::from(framebuffer_scale[1])) as f32],
        max: [
            display[0],
            ((row + 1.0) / f64::from(framebuffer_scale[1])) as f32,
        ],
    })
}

/// Layout only: fit a requested panel inside logical viewport margins.
pub fn constrained_panel_size(display: [f32; 2], desired: [f32; 2], margin: [f32; 2]) -> [f32; 2] {
    std::array::from_fn(|axis| {
        if ![display[axis], desired[axis], margin[axis]]
            .iter()
            .all(|value| value.is_finite())
        {
            return 1.0;
        }
        desired[axis]
            .max(1.0)
            .min((display[axis] - 2.0 * margin[axis].max(0.0)).max(1.0))
    })
}

/// Layout only: fit a panel above a bottom-anchored footer, retaining one
/// vertical margin at the top, between panels, and below the footer.
pub fn page_panel_size(
    display: [f32; 2],
    desired: [f32; 2],
    margin: [f32; 2],
    footer_height: f32,
) -> [f32; 2] {
    let available = [
        display[0],
        if footer_height.is_finite() {
            display[1] - footer_height.max(0.0) - margin[1].max(0.0)
        } else {
            f32::NAN
        },
    ];
    constrained_panel_size(available, desired, margin)
}
impl Observer {
    /// Preserve a full ECEF orientation, including bank relative to local ENU.
    /// Supplied up is orthogonalized against forward. Parallel/invalid inputs
    /// are rejected without changing the observer. Heading at a vertical sight
    /// direction is retained; roll absorbs that coordinate choice.
    pub fn set_view_axes(&mut self, forward: Vec3d, up: Vec3d) -> Result<(), GeometryError> {
        let forward = forward
            .normalized()
            .ok_or(GeometryError::InvalidDirection)?;
        let right = forward
            .cross(up)
            .normalized()
            .ok_or(GeometryError::InvalidDirection)?;
        let up = right.cross(forward);
        let enu = EnuBasis::at(self.feet);
        let east = forward.dot(enu.east);
        let north = forward.dot(enu.north);
        let vertical = forward.dot(enu.up);
        let horizontal = east.hypot(north);
        let heading = if horizontal > 64.0 * f64::EPSILON {
            east.atan2(north).to_degrees().rem_euclid(360.0)
        } else {
            self.heading_deg
        };
        let pitch = vertical.atan2(horizontal).to_degrees();
        let base = CameraFrame::new(self.eye_geodetic(), heading, pitch);
        let roll = up.dot(base.right).atan2(up.dot(base.up)).to_degrees();
        self.heading_deg = heading;
        self.pitch_deg = pitch;
        self.roll_deg = roll;
        Ok(())
    }
    /// Point the eye ray toward an ECEF target without moving the feet.
    /// A vertical target has no unique heading; retain the previous heading.
    pub fn aim_at(&mut self, position_ecef_m: Vec3d) -> Result<(), GeometryError> {
        let direction = (position_ecef_m - self.eye_position())
            .normalized()
            .ok_or(GeometryError::InvalidDirection)?;
        let enu = EnuBasis::at(self.feet);
        let east = direction.dot(enu.east);
        let north = direction.dot(enu.north);
        let up = direction.dot(enu.up);
        let horizontal = east.hypot(north);
        if horizontal > 64.0 * f64::EPSILON {
            self.heading_deg = east.atan2(north).to_degrees().rem_euclid(360.0);
        }
        self.pitch_deg = up.atan2(horizontal).to_degrees();
        Ok(())
    }
    pub fn new(feet: Geodetic) -> Self {
        Self {
            feet: Geodetic {
                height_m: feet
                    .height_m
                    .clamp(0.0, MAX_HEIGHT_M - DEFAULT_EYE_HEIGHT_M),
                ..feet
            },
            heading_deg: 0.0,
            pitch_deg: 0.0,
            roll_deg: 0.0,
            eye_height_m: DEFAULT_EYE_HEIGHT_M,
            reserved_eye_height_m: DEFAULT_EYE_HEIGHT_M,
            speed_mps: 1_000.0,
        }
    }
    pub fn eye_geodetic(self) -> Geodetic {
        Geodetic {
            height_m: self.feet.height_m + self.eye_height_m,
            ..self.feet
        }
    }
    pub fn eye_position(self) -> Vec3d {
        geodetic_to_ecef(self.eye_geodetic())
    }
    pub fn frame(self) -> CameraFrame {
        CameraFrame::with_roll(
            self.eye_geodetic(),
            self.heading_deg,
            self.pitch_deg,
            self.roll_deg,
        )
    }
    pub fn set_eye_height(&mut self, height_m: f64) {
        if height_m.is_finite() {
            self.eye_height_m = height_m.clamp(0.1, STATURE_M);
            self.reserved_eye_height_m = self.eye_height_m;
            self.feet.height_m = self.feet.height_m.min(MAX_HEIGHT_M - self.eye_height_m);
        }
    }
    /// Standing headroom is retained during crouched flight.
    pub fn maximum_feet_height_m(self) -> f64 {
        MAX_HEIGHT_M - self.reserved_eye_height_m.max(self.eye_height_m)
    }
    pub fn teleport(&mut self, location: Geodetic) {
        self.feet = Geodetic {
            height_m: location.height_m.clamp(0.0, self.maximum_feet_height_m()),
            ..location
        };
    }
    /// Exact user-coordinate navigation: invalid heights are rejected, never
    /// silently adjusted. Presets may use `teleport` with its documented clamp.
    pub fn try_teleport(&mut self, location: Geodetic) -> Result<(), GeometryError> {
        Geodetic::new(location.lat_deg, location.lon_deg, location.height_m)?;
        if !(0.0..=self.maximum_feet_height_m()).contains(&location.height_m) {
            return Err(GeometryError::ObserverHeight);
        }
        self.feet = location;
        Ok(())
    }
    pub fn ground(&mut self) {
        self.feet.height_m = 0.0;
        self.pitch_deg = 0.0;
        self.speed_mps = 10.0;
    }
    /// Overview is a nonphysical teleport, not an orbital mechanics simulation.
    pub fn overview(&mut self) {
        self.feet.height_m = 12_000_000.0;
        self.pitch_deg = -90.0;
        self.speed_mps = 500_000.0;
    }
    pub fn apply_look(&mut self, mouse_dx: f64, mouse_dy: f64, lens: Lens) {
        if !mouse_dx.is_finite() || !mouse_dy.is_finite() {
            return;
        }
        let sensitivity = 0.12 * lens.vertical_fov_deg / DEFAULT_FOV_DEG;
        self.heading_deg = (self.heading_deg + mouse_dx * sensitivity).rem_euclid(360.0);
        self.pitch_deg = (self.pitch_deg - mouse_dy * sensitivity).clamp(-90.0, 90.0);
    }
    /// Input axes in [-1,1]: forward, right, geodetic up. Combined speed is
    /// normalized, including diagonals. Straight ECEF flight with surface stop.
    pub fn fly(&mut self, forward: f64, right: f64, up: f64, elapsed_seconds: f64, boost: bool) {
        if ![forward, right, up, elapsed_seconds]
            .iter()
            .all(|v| v.is_finite())
        {
            return;
        }
        if elapsed_seconds <= 0.0 {
            return;
        }
        let frame = self.frame();
        let enu = EnuBasis::at(self.feet);
        let Some(direction) = (frame.forward * forward.clamp(-1.0, 1.0)
            + frame.right * right.clamp(-1.0, 1.0)
            + enu.up * up.clamp(-1.0, 1.0))
        .normalized() else {
            return;
        };
        // Geodetic-to-Cartesian rounding can put a surface origin a few nm
        // inside the ellipsoid. Prevent choosing its far-side exit as a step.
        if self.feet.height_m <= 0.000_01 && direction.dot(enu.up) < -1e-14 {
            return;
        }
        let distance = self.speed_mps.clamp(MIN_SPEED_MPS, MAX_SPEED_MPS)
            * elapsed_seconds.min(0.25)
            * if boost { 8.0 } else { 1.0 };
        let origin = geodetic_to_ecef(self.feet);
        let mut actual_distance = distance;
        if let Some(hit) = intersect_wgs84(Ray { origin, direction }) {
            let moving_into_surface = direction.dot(enu.up) < 0.0;
            if hit.distance_m <= distance && (hit.distance_m > 0.000_01 || moving_into_surface) {
                actual_distance = (hit.distance_m - 0.000_01).max(0.0);
            }
        }
        if let Ok(mut location) = inverse_near_surface(origin + direction * actual_distance) {
            location.height_m = location.height_m.clamp(0.0, self.maximum_feet_height_m());
            self.feet = location;
            // Preserve the complete ECEF view across local ENU changes.
            let _ = self.set_view_axes(frame.forward, frame.up);
        }
    }
}

/// Read-only RGBA8 equirectangular texture, north at top and -180° at left.
#[derive(Clone, Copy)]
pub struct TextureView<'a> {
    pub rgba: &'a [u8],
    pub width: u32,
    pub height: u32,
}
pub fn texture_uv(g: Geodetic) -> [f64; 2] {
    [
        (g.lon_deg + 180.0).rem_euclid(360.0) / 360.0,
        (90.0 - g.lat_deg) / 180.0,
    ]
}
impl TextureView<'_> {
    pub fn sample(self, g: Geodetic) -> [u8; 3] {
        if self.width == 0 || self.height == 0 {
            return [45, 101, 116];
        }
        let uv = texture_uv(g);
        let x = (uv[0] * self.width as f64).floor() as usize % self.width as usize;
        let y = (uv[1] * self.height as f64)
            .floor()
            .clamp(0.0, self.height as f64 - 1.0) as usize;
        let offset = (y * self.width as usize + x) * 4;
        self.rgba
            .get(offset..offset + 3)
            .map_or([45, 101, 116], |p| [p[0], p[1], p[2]])
    }
}

/// Analytic surface rendering. Color texture is cartographic illustration only.
/// Pixel sampling never affects the separate centre-dot measurement ray.
pub fn render_pixel(
    frame: CameraFrame,
    lens: Lens,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    texture: Option<TextureView<'_>>,
) -> [u8; 4] {
    render_pixel_for_view(
        frame,
        lens,
        x,
        y,
        width,
        height,
        [width as f32, height as f32],
        texture,
    )
}

/// See `render_pixel`, with display dimensions separate from sampling resolution.
#[allow(clippy::too_many_arguments)]
pub fn render_pixel_for_view(
    frame: CameraFrame,
    lens: Lens,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    display_size: [f32; 2],
    texture: Option<TextureView<'_>>,
) -> [u8; 4] {
    let Some(ray) = lens.pixel_ray_for_view(frame, x, y, width, height, display_size) else {
        return [4, 7, 14, 255];
    };
    let Some(hit) = intersect_wgs84(ray) else {
        let enu_up = frame
            .origin
            .normalized()
            .unwrap_or(Vec3d::new(0.0, 0.0, 1.0));
        let horizon = (1.0 - ray.direction.dot(enu_up).abs()).powi(8);
        return [
            (5.0 + 12.0 * horizon) as u8,
            (9.0 + 20.0 * horizon) as u8,
            (19.0 + 34.0 * horizon) as u8,
            255,
        ];
    };
    let color = texture.map_or([42, 100, 112], |tex| tex.sample(hit.geodetic));
    // Camera-relative fill lighting is a visualization choice, not solar physics.
    let light = 0.55 + 0.45 * hit.normal.dot(-ray.direction).max(0.0);
    let lat_grid = (hit.geodetic.lat_deg / 10.0)
        .rem_euclid(1.0)
        .min(1.0 - (hit.geodetic.lat_deg / 10.0).rem_euclid(1.0));
    let lon_grid = (hit.geodetic.lon_deg / 10.0)
        .rem_euclid(1.0)
        .min(1.0 - (hit.geodetic.lon_deg / 10.0).rem_euclid(1.0));
    let grid = if lat_grid < 0.004 || lon_grid < 0.004 {
        16.0
    } else {
        0.0
    };
    [
        (color[0] as f64 * light + grid).min(255.0) as u8,
        (color[1] as f64 * light + grid).min(255.0) as u8,
        (color[2] as f64 * light + grid).min(255.0) as u8,
        255,
    ]
}

/// Great-circle angular separation is provided only for coarse geographic
/// lookup; it is explicitly not an ellipsoidal geodesic distance.
pub fn angular_separation_degrees(a: Geodetic, b: Geodetic) -> f64 {
    let ua = EnuBasis::at(a).up;
    let ub = EnuBasis::at(b).up;
    ua.cross(ub).length().atan2(ua.dot(ub)).to_degrees()
}

pub fn pixels_to_wheel_lines(pixels: f64) -> f64 {
    pixels / 40.0
}
pub fn elapsed_seconds_f64(seconds: f32) -> f64 {
    f64::from(seconds)
}
pub fn degrees_to_radians(degrees: f64) -> f64 {
    degrees * PI / 180.0
}

/// Aspect-preserving render dimensions bounded to keep minimized/very tall
/// windows cheap. The final image is stretched to the window by the app.
pub fn render_dimensions(window_width: f32, window_height: f32, max_width: u32) -> (u32, u32) {
    if !window_width.is_finite()
        || !window_height.is_finite()
        || window_width <= 0.0
        || window_height <= 0.0
    {
        return (1, 1);
    }
    let cap = max_width.clamp(1, 4096) as f64;
    let width = f64::from(window_width);
    let height = f64::from(window_height);
    let scale = (cap / width).min(cap / height).min(1.0);
    (
        (width * scale).round().max(1.0) as u32,
        (height * scale).round().max(1.0) as u32,
    )
}
pub fn pixel_coordinates(index: usize, width: u32) -> (u32, u32) {
    if width == 0 {
        return (0, 0);
    }
    (
        (index % width as usize) as u32,
        (index / width as usize) as u32,
    )
}
pub fn screen_center(size: [f32; 2]) -> [f32; 2] {
    [size[0] * 0.5, size[1] * 0.5]
}
/// UI placement only: anchor 0 selects the near margin and 1 the far margin.
/// This layout arithmetic has no geodetic or optical meaning.
pub fn panel_anchor(
    display: [f32; 2],
    size: [f32; 2],
    anchor: [f32; 2],
    margin: [f32; 2],
) -> [f32; 2] {
    std::array::from_fn(|axis| {
        let a = anchor[axis].clamp(0.0, 1.0);
        ((display[axis] - size[axis] - margin[axis]) * a + margin[axis] * (1.0 - a)).max(0.0)
    })
}
/// Circular hit target in one shared UI coordinate system, used by draggable markers.
pub fn marker_hit(mouse: [f32; 2], marker: [f32; 2], radius: f32) -> bool {
    radius.is_finite()
        && radius >= 0.0
        && (mouse[0] - marker[0]).hypot(mouse[1] - marker[1]) <= radius
}
pub mod astro;
