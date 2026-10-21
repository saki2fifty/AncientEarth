//! Offline geoid loading. Datum interpolation belongs to `earth-math`.
use earth_math::GeoidGrid;
use flate2::read::GzDecoder;
use std::io::Read;

#[cfg_attr(feature = "desktop", derive(bevy::prelude::Resource))]
pub struct SeaLevel(pub GeoidGrid);

impl SeaLevel {
    pub fn bundled() -> Result<Self, Box<dyn std::error::Error>> {
        let compressed = include_bytes!("../assets/geoid/egm2008-native.aeg.gz");
        let mut bytes = Vec::new();
        GzDecoder::new(compressed.as_slice()).read_to_end(&mut bytes)?;
        Ok(Self(GeoidGrid::from_bytes(&bytes)?))
    }
}
