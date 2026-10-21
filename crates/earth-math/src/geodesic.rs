//! Shortest reference-ellipsoid paths, distinct from 3D chords and mesh paths.
use crate::{Geodetic, GeometryError, WGS84_A_M, WGS84_F};
use geographiclib_rs::{Geodesic, InverseGeodesic};
use std::sync::OnceLock;

/// Shortest WGS84 geodesic in metres between the two geographic coordinates.
/// Heights are validated but projected to h=0: this is neither a terrain-
/// following route nor the arc length of a sampled visible skyline. Karney's
/// inverse algorithm handles coincident, polar and antipodal endpoints.
pub fn wgs84_surface_distance(from: Geodetic, to: Geodetic) -> Result<f64, GeometryError> {
    Geodetic::new(from.lat_deg, from.lon_deg, from.height_m)?;
    Geodetic::new(to.lat_deg, to.lon_deg, to.height_m)?;
    static MODEL: OnceLock<Geodesic> = OnceLock::new();
    let model = MODEL.get_or_init(|| Geodesic::new(WGS84_A_M, WGS84_F));
    let distance: f64 = model.inverse(from.lat_deg, from.lon_deg, to.lat_deg, to.lon_deg);
    if distance.is_finite() && distance >= 0.0 {
        Ok(distance)
    } else {
        Err(GeometryError::GeodesicSolution)
    }
}
