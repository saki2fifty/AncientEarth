//! NAIF DAF/SPK Type2, J2000 frame, little-endian IEEE754.
use super::{AstroError, Vec3d};

#[derive(Clone, Copy, Debug)]
pub struct CartesianState {
    pub position_m: Vec3d,
    pub velocity_mps: Vec3d,
}
impl CartesianState {
    fn zero() -> Self {
        Self {
            position_m: Vec3d::ZERO,
            velocity_mps: Vec3d::ZERO,
        }
    }
    fn plus(self, rhs: Self) -> Self {
        Self {
            position_m: self.position_m + rhs.position_m,
            velocity_mps: self.velocity_mps + rhs.velocity_mps,
        }
    }
}
#[derive(Clone, Debug)]
struct Segment {
    start: f64,
    end: f64,
    target: i32,
    center: i32,
    address: usize,
    initial: f64,
    interval: f64,
    record_size: usize,
    records: usize,
}
#[derive(Clone, Debug)]
pub struct Ephemeris {
    bytes: Vec<u8>,
    segments: Vec<Segment>,
}
impl Ephemeris {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, AstroError> {
        if bytes.len() < 1024
            || &bytes[..8] != b"DAF/SPK "
            || &bytes[88..96] != b"LTL-IEEE"
            || i32_at(bytes, 8)? != 2
            || i32_at(bytes, 12)? != 6
        {
            return Err(AstroError::InvalidKernel);
        }
        let mut record =
            usize::try_from(i32_at(bytes, 76)?).map_err(|_| AstroError::InvalidKernel)?;
        let mut visited = std::collections::HashSet::new();
        let mut segments = Vec::new();
        while record != 0 {
            if !visited.insert(record) || visited.len() > 4096 {
                return Err(AstroError::InvalidKernel);
            }
            let offset = record
                .checked_sub(1)
                .and_then(|r| r.checked_mul(1024))
                .ok_or(AstroError::InvalidKernel)?;
            if offset.checked_add(1024).is_none_or(|end| end > bytes.len()) {
                return Err(AstroError::InvalidKernel);
            }
            let next = integer_f64(f64_at(bytes, offset)?)?;
            let count = integer_f64(f64_at(bytes, offset + 16)?)?;
            if count > 25 {
                return Err(AstroError::InvalidKernel);
            }
            for n in 0..count {
                let p = offset + 24 + n * 40;
                let start = f64_at(bytes, p)?;
                let end = f64_at(bytes, p + 8)?;
                let target = i32_at(bytes, p + 16)?;
                let center = i32_at(bytes, p + 20)?;
                let frame = i32_at(bytes, p + 24)?;
                let kind = i32_at(bytes, p + 28)?;
                let first = usize::try_from(i32_at(bytes, p + 32)?)
                    .map_err(|_| AstroError::InvalidKernel)?;
                let last = usize::try_from(i32_at(bytes, p + 36)?)
                    .map_err(|_| AstroError::InvalidKernel)?;
                if frame != 1
                    || kind != 2
                    || first == 0
                    || last < first + 3
                    || last > bytes.len() / 8
                    || !start.is_finite()
                    || !end.is_finite()
                    || end <= start
                {
                    return Err(AstroError::InvalidKernel);
                }
                let at = |address| f64_at(bytes, (address - 1) * 8);
                let initial = at(last - 3)?;
                let interval = at(last - 2)?;
                let record_size = integer_f64(at(last - 1)?)?;
                let records = integer_f64(at(last)?)?;
                if !initial.is_finite()
                    || !interval.is_finite()
                    || interval <= 0.0
                    || records == 0
                    || record_size < 8
                    || (record_size - 2) % 3 != 0
                    || record_size > 197
                    || records
                        .checked_mul(record_size)
                        .and_then(|v| v.checked_add(4))
                        != Some(last - first + 1)
                    || initial > start
                    || initial + interval * (records as f64) < end
                {
                    return Err(AstroError::InvalidKernel);
                }
                segments.push(Segment {
                    start,
                    end,
                    target,
                    center,
                    address: first,
                    initial,
                    interval,
                    record_size,
                    records,
                });
            }
            record = next;
        }
        if segments.is_empty() {
            return Err(AstroError::InvalidKernel);
        }
        Ok(Self {
            bytes: bytes.to_vec(),
            segments,
        })
    }
    /// State relative to the solar-system barycenter in ICRF/J2000 axes.
    /// The epoch is TDB seconds since JD2451545.0, never UTC seconds.
    pub fn state(&self, target: i32, tdb_seconds_j2000: f64) -> Result<CartesianState, AstroError> {
        if !tdb_seconds_j2000.is_finite() {
            return Err(AstroError::KernelCoverage);
        }
        let mut body = target;
        let mut state = CartesianState::zero();
        let mut visited = Vec::new();
        while body != 0 {
            if visited.contains(&body) || visited.len() >= 16 {
                return Err(AstroError::InvalidKernel);
            }
            visited.push(body);
            let Some(segment) = self.segments.iter().rev().find(|s| {
                s.target == body && tdb_seconds_j2000 >= s.start && tdb_seconds_j2000 <= s.end
            }) else {
                return Err(if self.segments.iter().any(|s| s.target == body) {
                    AstroError::KernelCoverage
                } else {
                    AstroError::MissingBody(body)
                });
            };
            state = state.plus(self.evaluate(segment, tdb_seconds_j2000)?);
            body = segment.center;
        }
        Ok(state)
    }
    pub fn coverage_tdb_seconds(&self) -> (f64, f64) {
        (
            self.segments
                .iter()
                .map(|s| s.start)
                .fold(f64::INFINITY, f64::min),
            self.segments
                .iter()
                .map(|s| s.end)
                .fold(f64::NEG_INFINITY, f64::max),
        )
    }
    fn evaluate(&self, segment: &Segment, time: f64) -> Result<CartesianState, AstroError> {
        let index = (((time - segment.initial) / segment.interval).floor() as usize)
            .min(segment.records - 1);
        let first = segment.address + index * segment.record_size;
        let at = |i| f64_at(&self.bytes, (first + i - 1) * 8);
        let midpoint = at(0)?;
        let radius = at(1)?;
        if !midpoint.is_finite() || !radius.is_finite() || radius <= 0.0 {
            return Err(AstroError::InvalidKernel);
        }
        let x = (time - midpoint) / radius;
        if x.abs() > 1.0 + 1e-12 {
            return Err(AstroError::InvalidKernel);
        }
        let count = (segment.record_size - 2) / 3;
        let mut p = [0.0; 3];
        let mut v = [0.0; 3];
        for axis in 0..3 {
            let mut previous = 1.0;
            let mut current = x;
            let mut d_previous = 0.0;
            let mut d_current = 1.0;
            p[axis] = at(2 + axis * count)?;
            for degree in 1..count {
                let coefficient = at(2 + axis * count + degree)?;
                p[axis] += coefficient * current;
                v[axis] += coefficient * d_current;
                let next = 2.0 * x * current - previous;
                let d_next = 2.0 * current + 2.0 * x * d_current - d_previous;
                previous = current;
                current = next;
                d_previous = d_current;
                d_current = d_next;
            }
            p[axis] *= 1000.0;
            v[axis] *= 1000.0 / radius;
        }
        let result = CartesianState {
            position_m: Vec3d::new(p[0], p[1], p[2]),
            velocity_mps: Vec3d::new(v[0], v[1], v[2]),
        };
        if !result.position_m.is_finite() || !result.velocity_mps.is_finite() {
            return Err(AstroError::InvalidKernel);
        }
        Ok(result)
    }
}
fn integer_f64(value: f64) -> Result<usize, AstroError> {
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 || value > usize::MAX as f64 {
        Err(AstroError::InvalidKernel)
    } else {
        Ok(value as usize)
    }
}
fn f64_at(bytes: &[u8], offset: usize) -> Result<f64, AstroError> {
    Ok(f64::from_le_bytes(
        bytes
            .get(offset..offset.checked_add(8).ok_or(AstroError::InvalidKernel)?)
            .ok_or(AstroError::InvalidKernel)?
            .try_into()
            .map_err(|_| AstroError::InvalidKernel)?,
    ))
}
fn i32_at(bytes: &[u8], offset: usize) -> Result<i32, AstroError> {
    Ok(i32::from_le_bytes(
        bytes
            .get(offset..offset.checked_add(4).ok_or(AstroError::InvalidKernel)?)
            .ok_or(AstroError::InvalidKernel)?
            .try_into()
            .map_err(|_| AstroError::InvalidKernel)?,
    ))
}
