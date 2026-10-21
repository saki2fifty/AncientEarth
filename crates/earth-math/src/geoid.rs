//! Native EGM2008 point-grid interpolation; h = H + N, hence H = h - N.
use super::*;

#[derive(Clone, Debug, PartialEq)]
pub enum GeoidError {
    InvalidFormat,
    InvalidGrid,
    InvalidCoordinate(GeometryError),
    Surface(TerrainError),
}
impl fmt::Display for GeoidError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidFormat => "Invalid native geoid binary format.",
            Self::InvalidGrid => "Invalid global geoid point grid.",
            Self::InvalidCoordinate(_) => "Invalid coordinate for geoid lookup.",
            Self::Surface(_) => "Ground beneath this coordinate could not be determined.",
        })
    }
}
impl std::error::Error for GeoidError {}

#[derive(Clone, Debug)]
pub struct GeoidGrid {
    width: usize,
    height: usize,
    values: Vec<f32>,
}
impl GeoidGrid {
    /// A complete periodic longitude grid, north/south pole rows included.
    /// Row zero is +90°, column zero -180°; the final longitude is not duplicated.
    pub fn from_grid(width: usize, height: usize, values: Vec<f32>) -> Result<Self, GeoidError> {
        if width < 2
            || height < 2
            || width.checked_mul(height) != Some(values.len())
            || values.len() > 100_000_000
            || !values.iter().all(|v| v.is_finite() && v.abs() <= 1000.0)
        {
            return Err(GeoidError::InvalidGrid);
        }
        // Geodetic longitude is indeterminate at a pole; the supplied model
        // must therefore provide the same pole value in every column.
        for row in [0, height - 1] {
            if values[row * width..(row + 1) * width]
                .iter()
                .any(|v| *v != values[row * width])
            {
                return Err(GeoidError::InvalidGrid);
            }
        }
        Ok(Self {
            width,
            height,
            values,
        })
    }
    /// Parse the inflated AGEGEO01 lossless byte-plane/delta payload. gzip
    /// decompression belongs to the application I/O layer.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, GeoidError> {
        if bytes.len() < 48 || &bytes[..8] != b"AGEGEO01" {
            return Err(GeoidError::InvalidFormat);
        }
        let u32_at = |i| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap()) as usize;
        let f64_at = |i| f64::from_le_bytes(bytes[i..i + 8].try_into().unwrap());
        let width = u32_at(8);
        let height = u32_at(12);
        let count = width.checked_mul(height).ok_or(GeoidError::InvalidFormat)?;
        if width < 2
            || height < 2
            || count > 100_000_000
            || count.checked_mul(4).and_then(|n| n.checked_add(48)) != Some(bytes.len())
            || f64_at(16) != -180.0
            || f64_at(24) != 90.0
            || (f64_at(32) - 360.0 / width as f64).abs() > 1e-12
            || (f64_at(40) - 180.0 / (height - 1) as f64).abs() > 1e-12
            || !f64_at(32).is_finite()
            || !f64_at(40).is_finite()
        {
            return Err(GeoidError::InvalidFormat);
        }
        let mut values = Vec::with_capacity(count);
        let mut planes = vec![0u8; width * 4];
        for encoded in bytes[48..].chunks_exact(width * 4) {
            let mut previous = 0u8;
            for (output, delta) in planes.iter_mut().zip(encoded) {
                previous = previous.wrapping_add(*delta);
                *output = previous;
            }
            for column in 0..width {
                values.push(f32::from_le_bytes([
                    planes[column],
                    planes[width + column],
                    planes[2 * width + column],
                    planes[3 * width + column],
                ]));
            }
        }
        Self::from_grid(width, height, values)
    }
    pub fn dimensions(&self) -> (usize, usize) {
        (self.width, self.height)
    }
    pub fn undulation_m(&self, latitude_deg: f64, longitude_deg: f64) -> Result<f64, GeoidError> {
        Geodetic::new(latitude_deg, longitude_deg, 0.0).map_err(GeoidError::InvalidCoordinate)?;
        let x = (longitude_deg + 180.0).rem_euclid(360.0) * self.width as f64 / 360.0;
        let y = ((90.0 - latitude_deg) * (self.height - 1) as f64 / 180.0)
            .clamp(0.0, (self.height - 1) as f64);
        let x0 = x.floor() as usize % self.width;
        let x1 = (x0 + 1) % self.width;
        let y0 = y.floor() as usize;
        let y1 = (y0 + 1).min(self.height - 1);
        let tx = x - x.floor();
        let ty = y - y0 as f64;
        let value = |column, row| f64::from(self.values[row * self.width + column]);
        Ok(
            (value(x0, y0) * (1.0 - tx) + value(x1, y0) * tx) * (1.0 - ty)
                + (value(x0, y1) * (1.0 - tx) + value(x1, y1) * tx) * ty,
        )
    }
}

#[derive(Clone, Copy, Debug)]
pub struct VerticalReport {
    pub ground: SurfaceHit,
    pub geoid_position: Geodetic,
    pub geoid_undulation_m: f64,
    pub ground_egm2008_height_m: f64,
    pub feet_egm2008_height_m: f64,
    pub eye_egm2008_height_m: f64,
    pub feet_above_ground_m: f64,
    pub eyes_above_ground_m: f64,
    pub eye_height_m: f64,
}
impl TerrainScene {
    /// Beneath means along the observer's geodetic normal, preserving its
    /// latitude/longitude. EGM2008 is a modeled reference, not instantaneous tide.
    pub fn vertical_report(
        &self,
        observer: Observer,
        geoid: &GeoidGrid,
    ) -> Result<VerticalReport, GeoidError> {
        let ground = self.ground(observer.feet).map_err(GeoidError::Surface)?;
        let n = geoid.undulation_m(observer.feet.lat_deg, observer.feet.lon_deg)?;
        let eye_h = observer.eye_geodetic().height_m;
        Ok(VerticalReport {
            ground,
            geoid_position: observer
                .feet
                .with_height(n)
                .map_err(GeoidError::InvalidCoordinate)?,
            geoid_undulation_m: n,
            ground_egm2008_height_m: ground.hit.geodetic.height_m - n,
            feet_egm2008_height_m: observer.feet.height_m - n,
            eye_egm2008_height_m: eye_h - n,
            feet_above_ground_m: observer.feet.height_m - ground.hit.geodetic.height_m,
            eyes_above_ground_m: eye_h - ground.hit.geodetic.height_m,
            eye_height_m: observer.eye_height_m,
        })
    }
}
