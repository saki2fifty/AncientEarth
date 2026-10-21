//! Height-free WBM presentation and classification on the exact WGS84 surface.
use crate::{Geodetic, Observer};

/// A mathematical equatorial overview, unrelated to any named location.
pub fn default_observer() -> Observer {
    let mut observer = Observer::new(Geodetic::new(0.0, 0.0, 8_000_000.0).unwrap());
    observer.pitch_deg = -90.0;
    observer.speed_mps = 100_000.0;
    observer
}
