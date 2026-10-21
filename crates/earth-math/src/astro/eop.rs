//! IERS finals2000A daily UT1/polar motion. Interpolate UT1−TAI across leaps.
use super::time::{calendar_from_uniform, delta_t_model, tai_minus_utc};
use super::{ARCSEC_RAD, AstroError, AstroInstant, SECONDS_PER_DAY};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EarthRotationQuality {
    ObservedIers,
    PredictedIers,
    ModeledDeltaT,
}
impl EarthRotationQuality {
    pub fn description(self) -> &'static str {
        match self {
            Self::ObservedIers => "IERS observed Earth rotation; daily interpolation",
            Self::PredictedIers => "IERS predicted Earth rotation",
            Self::ModeledDeltaT => {
                "Modeled Earth rotation outside IERS coverage; polar motion omitted"
            }
        }
    }
}
#[derive(Clone, Copy, Debug)]
struct Record {
    tt: f64,
    ut1_minus_tai: f64,
    xp: f64,
    yp: f64,
    predicted: bool,
}
#[derive(Clone, Debug)]
pub struct EarthOrientationData {
    records: Vec<Record>,
}
pub(crate) struct OrientationSample {
    pub ut1: f64,
    pub xp: f64,
    pub yp: f64,
    pub quality: EarthRotationQuality,
}
impl EarthOrientationData {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, AstroError> {
        let text =
            std::str::from_utf8(bytes).map_err(|_| AstroError::InvalidEarthOrientationData)?;
        let mut records = Vec::new();
        for line in text.lines() {
            if line.len() < 68 || !line.is_ascii() {
                continue;
            }
            let number = |a, b| line.get(a..b)?.trim().parse::<f64>().ok();
            let (Some(mjd), Some(xp), Some(yp), Some(dut1)) = (
                number(7, 15),
                number(18, 27),
                number(37, 46),
                number(58, 68),
            ) else {
                continue;
            };
            if ![mjd, xp, yp, dut1].iter().all(|v| v.is_finite())
                || !(-10_000.0..110_000.0).contains(&mjd)
                || xp.abs() > 5.0
                || yp.abs() > 5.0
                || dut1.abs() > 2.0
            {
                return Err(AstroError::InvalidEarthOrientationData);
            }
            let civil = calendar_from_uniform((mjd - 51544.5) * SECONDS_PER_DAY);
            let tt = AstroInstant::from_calendar(civil)?.tt_seconds_j2000();
            if records.last().is_some_and(|last: &Record| last.tt >= tt) {
                return Err(AstroError::InvalidEarthOrientationData);
            }
            let flags = [line.as_bytes()[16], line.as_bytes()[57]];
            if flags.iter().any(|flag| !matches!(flag, b'I' | b'P')) {
                return Err(AstroError::InvalidEarthOrientationData);
            }
            records.push(Record {
                tt,
                ut1_minus_tai: dut1 - tai_minus_utc(civil),
                xp: xp * ARCSEC_RAD,
                yp: yp * ARCSEC_RAD,
                predicted: flags.contains(&b'P'),
            });
        }
        if records.len() < 2 {
            return Err(AstroError::InvalidEarthOrientationData);
        }
        Ok(Self { records })
    }
    pub fn coverage_calendar(&self) -> (String, String) {
        (
            AstroInstant::from_tt_seconds_j2000(self.records[0].tt)
                .unwrap()
                .calendar_string(),
            AstroInstant::from_tt_seconds_j2000(self.records[self.records.len() - 1].tt)
                .unwrap()
                .calendar_string(),
        )
    }
    fn interpolate(&self, instant: AstroInstant) -> Option<OrientationSample> {
        let tt = instant.tt_seconds_j2000();
        let upper = self.records.partition_point(|r| r.tt <= tt);
        if upper == 0 || tt > self.records.last()?.tt {
            return None;
        }
        let first = self.records[(upper - 1).min(self.records.len() - 2)];
        let next = self.records[upper.min(self.records.len() - 1)];
        if next.tt - first.tt > 2.0 * SECONDS_PER_DAY + 2.0 {
            return None;
        }
        let fraction = (tt - first.tt) / (next.tt - first.tt);
        let lerp = |a, b| a + (b - a) * fraction;
        Some(OrientationSample {
            ut1: tt - 32.184 + lerp(first.ut1_minus_tai, next.ut1_minus_tai),
            xp: lerp(first.xp, next.xp),
            yp: lerp(first.yp, next.yp),
            quality: if first.predicted || next.predicted {
                EarthRotationQuality::PredictedIers
            } else {
                EarthRotationQuality::ObservedIers
            },
        })
    }
}
pub(crate) fn resolve(
    instant: AstroInstant,
    eop: Option<&EarthOrientationData>,
) -> OrientationSample {
    eop.and_then(|data| data.interpolate(instant))
        .unwrap_or_else(|| OrientationSample {
            ut1: instant.tt_seconds_j2000() - delta_t_model(instant.calendar().decimal_year()),
            xp: 0.0,
            yp: 0.0,
            quality: EarthRotationQuality::ModeledDeltaT,
        })
}
