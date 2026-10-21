//! Kinematic Earth-centered, nonrotating observer tracking. This is not a
//! gravity, orbit, free-fall, or relativistic GCRS trajectory integration.
use super::{AstroInstant, EarthOrientationData, Mat3d, eop, orientation};
use crate::{Geodetic, Observer, TerrainScene, Vec3d, WGS84_A_M, ecef_to_geodetic};
use std::fmt;

const CLEARANCE_TOLERANCE_M: f64 = 1e-6;
const MAX_ENDPOINT_STEP_SECONDS: f64 = 1.0;

/// Exactly the terrestrial orientation used by ephemeris sampling, including
/// the same IAU/IERS approximations and the same modeled EOP fallback.
pub fn terrestrial_rotation(instant: AstroInstant, eop: Option<&EarthOrientationData>) -> Mat3d {
    let eo = eop::resolve(instant, eop);
    orientation::earth_orientation(instant.tt_seconds_j2000(), eo.ut1, eo.xp, eo.yp).0
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackingError {
    InvalidPose,
    InvalidRotation,
    BelowSurface,
    UncertifiedTimeJump,
}
impl fmt::Display for TrackingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidPose => "Nonrotating observer pose is outside the supported coordinate or eye-height domain.",
            Self::InvalidRotation => "Observer tracking requires a finite proper orthonormal terrestrial rotation.",
            Self::BelowSurface => "Earth's rotation would place the observer's feet below the modeled surface. Time change rejected; pause and relock.",
            Self::UncertifiedTimeJump => "A nonrotating time change over 1 second requires clearance outside the global terrain envelope. Fly higher or use smaller steps; time change rejected.",
        })
    }
}
impl std::error::Error for TrackingError {}

/// Held position and full view in geocentric ICRF-oriented axes. The origin
/// translates with Earth. Mode changes and navigation are reconciled at the
/// previously displayed epoch, before applying the requested new rotation.
#[derive(Clone, Debug)]
pub struct ObserverTracking {
    observer: Observer,
    instant: AstroInstant,
    rotation: Mat3d,
    follow_earth: bool,
    eye_icrf_m: Vec3d,
    forward_icrf: Vec3d,
    up_icrf: Vec3d,
}
impl ObserverTracking {
    pub fn new(
        observer: Observer,
        instant: AstroInstant,
        rotation: Mat3d,
    ) -> Result<Self, TrackingError> {
        validate_rotation(rotation)?;
        validate_pose(observer)?;
        let frame = observer.frame();
        let inverse = rotation.transpose();
        Ok(Self {
            observer,
            instant,
            rotation,
            follow_earth: true,
            eye_icrf_m: inverse.transform(frame.origin),
            forward_icrf: inverse.transform(frame.forward),
            up_icrf: inverse.transform(frame.up),
        })
    }
    pub fn instant(&self) -> AstroInstant {
        self.instant
    }
    pub fn observer(&self) -> Observer {
        self.observer
    }
    pub fn follows_earth(&self) -> bool {
        self.follow_earth
    }

    /// Explicitly committed date-and-location navigation supplies its new
    /// epoch and matrix. This operation is transactional on invalid input.
    pub fn reanchor(
        &mut self,
        observer: Observer,
        follow_earth: bool,
        instant: AstroInstant,
        rotation: Mat3d,
    ) -> Result<(), TrackingError> {
        let mut replacement = Self::new(observer, instant, rotation)?;
        replacement.follow_earth = follow_earth;
        *self = replacement;
        Ok(())
    }

    /// Transactional transport. On failure neither tracker nor observer is
    /// changed. The caller restores `instant()`, pauses, and requests locked
    /// mode. Ordinary <=1 s endpoint checks do NOT certify a swept path.
    pub fn update(
        &mut self,
        observer: &mut Observer,
        follow_earth: bool,
        instant: AstroInstant,
        rotation: Mat3d,
        scene: &TerrainScene,
    ) -> Result<(), TrackingError> {
        validate_rotation(rotation)?;
        validate_pose(*observer)?;
        validate_surface(*observer, scene)?;
        let mut next = self.clone();
        if *observer != self.observer || self.follow_earth != follow_earth {
            // UI, crouch, mouse look, flight and ordinary teleports happened
            // against the OLD displayed terrestrial frame.
            next.reanchor(*observer, follow_earth, self.instant, self.rotation)?;
        }
        let mut candidate = *observer;
        if !follow_earth && instant != self.instant {
            let elapsed = (instant.tt_seconds_j2000() - self.instant.tt_seconds_j2000()).abs();
            if elapsed > MAX_ENDPOINT_STEP_SECONDS
                && !globally_clear(next.eye_icrf_m, candidate.eye_height_m, scene)
            {
                return Err(TrackingError::UncertifiedTimeJump);
            }
            let eye = ecef_to_geodetic(rotation.transform(next.eye_icrf_m))
                .map_err(|_| TrackingError::InvalidPose)?;
            candidate.feet = Geodetic::new(
                eye.lat_deg,
                eye.lon_deg,
                eye.height_m - candidate.eye_height_m,
            )
            .map_err(|_| TrackingError::InvalidPose)?;
            candidate
                .set_view_axes(
                    rotation.transform(next.forward_icrf),
                    rotation.transform(next.up_icrf),
                )
                .map_err(|_| TrackingError::InvalidPose)?;
            validate_pose(candidate)?;
            validate_surface(candidate, scene)?;
        }
        next.observer = candidate;
        next.follow_earth = follow_earth;
        next.instant = instant;
        next.rotation = rotation;
        *observer = candidate;
        *self = next;
        Ok(())
    }
}

fn validate_pose(observer: Observer) -> Result<(), TrackingError> {
    Geodetic::new(
        observer.feet.lat_deg,
        observer.feet.lon_deg,
        observer.feet.height_m,
    )
    .map_err(|_| TrackingError::InvalidPose)?;
    if ![
        observer.heading_deg,
        observer.pitch_deg,
        observer.roll_deg,
        observer.eye_height_m,
        observer.speed_mps,
    ]
    .iter()
    .all(|v| v.is_finite())
        || !(-90.0..=90.0).contains(&observer.pitch_deg)
        || !(0.1..=crate::STATURE_M).contains(&observer.eye_height_m)
        || observer.feet.height_m > observer.maximum_feet_height_m()
    {
        return Err(TrackingError::InvalidPose);
    }
    Ok(())
}
fn validate_surface(observer: Observer, scene: &TerrainScene) -> Result<(), TrackingError> {
    let ground = scene
        .ground(observer.feet)
        .map_err(|_| TrackingError::InvalidPose)?;
    if observer.feet.height_m + CLEARANCE_TOLERANCE_M < ground.hit.geodetic.height_m {
        Err(TrackingError::BelowSurface)
    } else {
        Ok(())
    }
}
fn validate_rotation(rotation: Mat3d) -> Result<(), TrackingError> {
    let [a, b, c] = rotation.rows.map(|r| Vec3d::new(r[0], r[1], r[2]));
    let errors = [
        a.dot(a) - 1.0,
        b.dot(b) - 1.0,
        c.dot(c) - 1.0,
        a.dot(b),
        a.dot(c),
        b.dot(c),
        a.cross(b).dot(c) - 1.0,
    ];
    if errors.iter().all(|e| e.is_finite() && e.abs() <= 1e-10) {
        Ok(())
    } else {
        Err(TrackingError::InvalidRotation)
    }
}
fn globally_clear(eye_icrf_m: Vec3d, eye_height_m: f64, scene: &TerrainScene) -> bool {
    // Norm is convex, so every triangle lies in the ball containing its three
    // vertices. The WGS84 ellipsoid lies in the radius-a ball. For any Earth
    // orientation, |feet| >= |eye| - eye offset by the triangle inequality.
    let radius = scene
        .patches()
        .iter()
        .filter(|_| !scene.is_boundary_only())
        .flat_map(|p| p.canonical_vertices())
        .map(|p| p.length())
        .fold(WGS84_A_M, f64::max);
    eye_icrf_m.length() - eye_height_m > radius + CLEARANCE_TOLERANCE_M
}
