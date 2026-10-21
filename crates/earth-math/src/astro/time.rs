//! Calendar/atomic time. UTC drift constants derive from ERFA 2.0.1 dat.c;
//! Copyright NumFOCUS Foundation, BSD-3-Clause, see licenses/ERFA-LICENSE.txt.
use super::{AstroError, JULIAN_DAY_J2000, SECONDS_PER_DAY};

const UNIX_AT_J2000_NOON: f64 = 946_728_000.0;
const TT_MINUS_TAI: f64 = 32.184;
// (year, month, base TAI−UTC, reference MJD, drift seconds/day).
const OFFSETS: &[(i32, u32, f64, f64, f64)] = &[
    (1960, 1, 1.417818, 37300., 0.001296),
    (1961, 1, 1.422818, 37300., 0.001296),
    (1961, 8, 1.372818, 37300., 0.001296),
    (1962, 1, 1.845858, 37665., 0.0011232),
    (1963, 11, 1.945858, 37665., 0.0011232),
    (1964, 1, 3.240130, 38761., 0.001296),
    (1964, 4, 3.340130, 38761., 0.001296),
    (1964, 9, 3.440130, 38761., 0.001296),
    (1965, 1, 3.540130, 38761., 0.001296),
    (1965, 3, 3.640130, 38761., 0.001296),
    (1965, 7, 3.740130, 38761., 0.001296),
    (1965, 9, 3.840130, 38761., 0.001296),
    (1966, 1, 4.313170, 39126., 0.002592),
    (1968, 2, 4.213170, 39126., 0.002592),
    (1972, 1, 10., 0., 0.),
    (1972, 7, 11., 0., 0.),
    (1973, 1, 12., 0., 0.),
    (1974, 1, 13., 0., 0.),
    (1975, 1, 14., 0., 0.),
    (1976, 1, 15., 0., 0.),
    (1977, 1, 16., 0., 0.),
    (1978, 1, 17., 0., 0.),
    (1979, 1, 18., 0., 0.),
    (1980, 1, 19., 0., 0.),
    (1981, 7, 20., 0., 0.),
    (1982, 7, 21., 0., 0.),
    (1983, 7, 22., 0., 0.),
    (1985, 7, 23., 0., 0.),
    (1988, 1, 24., 0., 0.),
    (1990, 1, 25., 0., 0.),
    (1991, 1, 26., 0., 0.),
    (1992, 7, 27., 0., 0.),
    (1993, 7, 28., 0., 0.),
    (1994, 7, 29., 0., 0.),
    (1996, 1, 30., 0., 0.),
    (1997, 7, 31., 0., 0.),
    (1999, 1, 32., 0., 0.),
    (2006, 1, 33., 0., 0.),
    (2009, 1, 34., 0., 0.),
    (2012, 7, 35., 0., 0.),
    (2015, 7, 36., 0., 0.),
    (2017, 1, 37., 0., 0.),
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CalendarDateTime {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: f64,
}
impl CalendarDateTime {
    pub fn parse(text: &str) -> Result<Self, AstroError> {
        let text = text.trim().strip_suffix('Z').unwrap_or(text.trim());
        let (date, time) = text
            .split_once('T')
            .or_else(|| text.split_once(' '))
            .ok_or(AstroError::InvalidDate)?;
        let d: Vec<_> = date.split('-').collect();
        let t: Vec<_> = time.split(':').collect();
        if d.len() != 3 || t.len() != 3 {
            return Err(AstroError::InvalidDate);
        }
        let p = |s: &str| s.parse::<u32>().map_err(|_| AstroError::InvalidDate);
        let value = Self {
            year: d[0].parse().map_err(|_| AstroError::InvalidDate)?,
            month: p(d[1])?,
            day: p(d[2])?,
            hour: p(t[0])?,
            minute: p(t[1])?,
            second: t[2].parse().map_err(|_| AstroError::InvalidDate)?,
        };
        value.validate()?;
        Ok(value)
    }
    pub fn validate(self) -> Result<(), AstroError> {
        if !(1850..=2149).contains(&self.year) {
            return Err(AstroError::UnsupportedDate);
        }
        if !(1..=12).contains(&self.month)
            || self.day == 0
            || self.day > days_in_month(self.year, self.month)
            || self.hour > 23
            || self.minute > 59
            || !self.second.is_finite()
            || !(0.0..61.0).contains(&self.second)
        {
            return Err(AstroError::InvalidDate);
        }
        if self.second >= 60.0 {
            let next = days_from_civil(self.year, self.month, self.day) + 1;
            let legal = self.hour == 23
                && self.minute == 59
                && OFFSETS[15..]
                    .iter()
                    .any(|&(y, m, _, _, _)| days_from_civil(y, m, 1) == next);
            if !legal {
                return Err(AstroError::InvalidDate);
            }
        }
        Ok(())
    }
    pub fn label(self) -> String {
        let displayed = (self.second * 1000.0).floor() / 1000.0;
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:06.3}",
            self.year, self.month, self.day, self.hour, self.minute, displayed
        )
    }
    pub(crate) fn uniform_seconds(self) -> f64 {
        days_from_civil(self.year, self.month, self.day) as f64 * SECONDS_PER_DAY
            + f64::from(self.hour * 3600 + self.minute * 60)
            + self.second
            - UNIX_AT_J2000_NOON
    }
    pub(crate) fn decimal_year(self) -> f64 {
        let day = (days_from_civil(self.year, self.month, self.day)
            - days_from_civil(self.year, 1, 1)) as f64
            + (f64::from(self.hour * 3600 + self.minute * 60) + self.second) / SECONDS_PER_DAY;
        f64::from(self.year) + day / f64::from(if is_leap(self.year) { 366 } else { 365 })
    }
}

/// Continuous TT seconds from JD2451545.0 TT. Arithmetic does not step on UTC
/// labels, so a published leap second occupies its own SI-second interval.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AstroInstant {
    tt_seconds: f64,
}
impl AstroInstant {
    pub fn from_calendar(value: CalendarDateTime) -> Result<Self, AstroError> {
        value.validate()?;
        let civil = value.uniform_seconds();
        let tt = if value.year < 1960 {
            civil + delta_t_model(value.decimal_year())
        } else {
            civil + tai_minus_utc(value) + TT_MINUS_TAI
        };
        Ok(Self { tt_seconds: tt })
    }
    pub fn from_unix_utc(seconds: f64) -> Result<Self, AstroError> {
        if !seconds.is_finite() {
            return Err(AstroError::InvalidDate);
        }
        if seconds.abs() > 10_000_000_000.0 {
            return Err(AstroError::UnsupportedDate);
        }
        Self::from_calendar(calendar_from_uniform(seconds - UNIX_AT_J2000_NOON))
    }
    pub fn from_tt_seconds_j2000(seconds: f64) -> Result<Self, AstroError> {
        if !seconds.is_finite() {
            return Err(AstroError::InvalidDate);
        }
        if seconds.abs() > 6_000_000_000.0 {
            return Err(AstroError::UnsupportedDate);
        }
        let result = Self {
            tt_seconds: seconds,
        };
        result.calendar().validate()?;
        Ok(result)
    }
    pub fn tt_seconds_j2000(self) -> f64 {
        self.tt_seconds
    }
    pub fn tt_jd(self) -> f64 {
        JULIAN_DAY_J2000 + self.tt_seconds / SECONDS_PER_DAY
    }
    pub fn calendar(self) -> CalendarDateTime {
        // Published positive leap seconds are disjoint intervals in TT.
        for &(year, month, offset, _, _) in &OFFSETS[15..] {
            let next =
                (days_from_civil(year, month, 1) as f64) * SECONDS_PER_DAY - UNIX_AT_J2000_NOON;
            let threshold = next + offset + TT_MINUS_TAI;
            if self.tt_seconds >= threshold - 1.0 && self.tt_seconds < threshold {
                let mut previous = calendar_from_uniform(next - 1.0);
                previous.second = 60.0 + self.tt_seconds - (threshold - 1.0);
                return previous;
            }
        }
        let mut uniform = self.tt_seconds - 69.184;
        for _ in 0..8 {
            let date = calendar_from_uniform(uniform);
            let correction = if date.year < 1960 {
                delta_t_model(date.decimal_year())
            } else {
                tai_minus_utc(date) + TT_MINUS_TAI
            };
            let updated = self.tt_seconds - correction;
            if updated == uniform {
                break;
            }
            uniform = updated;
        }
        calendar_from_uniform(uniform)
    }
    pub fn calendar_string(self) -> String {
        self.calendar().label()
    }
    pub fn shifted(self, seconds: f64) -> Result<Self, AstroError> {
        if !seconds.is_finite() {
            return Err(AstroError::InvalidStep);
        }
        Self::from_tt_seconds_j2000(self.tt_seconds + seconds).map_err(|_| AstroError::InvalidStep)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SimulationClock {
    instant: AstroInstant,
    pub running: bool,
    pub rate: f64,
}
impl SimulationClock {
    pub fn from_unix_utc(seconds: f64) -> Result<Self, AstroError> {
        Ok(Self {
            instant: AstroInstant::from_unix_utc(seconds)?,
            running: true,
            rate: 1.0,
        })
    }
    pub fn from_calendar(text: &str) -> Result<Self, AstroError> {
        Ok(Self {
            instant: AstroInstant::from_calendar(CalendarDateTime::parse(text)?)?,
            running: false,
            rate: 1.0,
        })
    }
    pub fn set_calendar(&mut self, text: &str) -> Result<(), AstroError> {
        self.instant = AstroInstant::from_calendar(CalendarDateTime::parse(text)?)?;
        Ok(())
    }
    pub fn calendar_string(&self) -> String {
        self.instant.calendar_string()
    }
    pub fn instant(&self) -> AstroInstant {
        self.instant
    }
    /// Restore an already validated continuous instant without a calendar
    /// formatting round-trip; playback settings are unchanged.
    pub fn set_instant(&mut self, instant: AstroInstant) {
        self.instant = instant;
    }
    pub fn advance(&mut self, real_seconds: f64) -> Result<(), AstroError> {
        if !real_seconds.is_finite() || real_seconds < 0.0 || !self.rate.is_finite() {
            return Err(AstroError::InvalidStep);
        }
        if self.running {
            match self.instant.shifted(real_seconds * self.rate) {
                Ok(value) => self.instant = value,
                Err(error) => {
                    self.running = false;
                    return Err(error);
                }
            }
        }
        Ok(())
    }
    pub fn step(&mut self, count: i64, step_seconds: f64) -> Result<(), AstroError> {
        if count.unsigned_abs() > 1_000_000_000 || !step_seconds.is_finite() || step_seconds <= 0.0
        {
            return Err(AstroError::InvalidStep);
        }
        self.instant = self.instant.shifted(count as f64 * step_seconds)?;
        Ok(())
    }
}

pub(crate) fn tai_minus_utc(date: CalendarDateTime) -> f64 {
    let index = OFFSETS
        .iter()
        .rposition(|&(y, m, _, _, _)| (date.year, date.month) >= (y, m));
    let Some(index) = index else {
        return 0.0;
    };
    let (_, _, base, reference, drift) = OFFSETS[index];
    let mjd = 51544.5 + date.uniform_seconds() / SECONDS_PER_DAY;
    base + (mjd - reference) * drift
}
pub(crate) fn is_leap(y: i32) -> bool {
    y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
}
fn days_in_month(y: i32, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap(y) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}
// Gregorian calendar integer arithmetic; the era has exactly146097 days.
pub(crate) fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let y = i64::from(year) - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = i64::from(month) + if month > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468
}
pub(crate) fn calendar_from_uniform(seconds: f64) -> CalendarDateTime {
    let unix = seconds + UNIX_AT_J2000_NOON;
    let days = (unix / SECONDS_PER_DAY).floor() as i64;
    let remainder = unix - days as f64 * SECONDS_PER_DAY;
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    let hour = (remainder / 3600.0).floor() as u32;
    let minute = ((remainder - f64::from(hour) * 3600.0) / 60.0).floor() as u32;
    CalendarDateTime {
        year: year as i32,
        month: month as u32,
        day: day as u32,
        hour,
        minute,
        second: remainder - f64::from(hour * 3600 + minute * 60),
    }
}

/// Espenak/Meeus historical/predictive ΔT polynomials, seconds, NASA eclipse
/// reference. This is explicitly a model, never measured future Earth rotation.
pub fn delta_t_model(year: f64) -> f64 {
    let t;
    if year < 1860.0 {
        t = year - 1800.0;
        13.72 - 0.332447 * t + 0.0068612 * t.powi(2) + 0.0041116 * t.powi(3)
            - 0.00037436 * t.powi(4)
            + 0.0000121272 * t.powi(5)
            - 0.0000001699 * t.powi(6)
            + 0.000000000875 * t.powi(7)
    } else if year < 1900.0 {
        t = year - 1860.0;
        7.62 + 0.5737 * t - 0.251754 * t.powi(2) + 0.01680668 * t.powi(3) - 0.0004473624 * t.powi(4)
            + t.powi(5) / 233174.0
    } else if year < 1920.0 {
        t = year - 1900.0;
        -2.79 + 1.494119 * t - 0.0598939 * t.powi(2) + 0.0061966 * t.powi(3) - 0.000197 * t.powi(4)
    } else if year < 1941.0 {
        t = year - 1920.0;
        21.20 + 0.84493 * t - 0.076100 * t.powi(2) + 0.0020936 * t.powi(3)
    } else if year < 1961.0 {
        t = year - 1950.0;
        29.07 + 0.407 * t - t.powi(2) / 233.0 + t.powi(3) / 2547.0
    } else if year < 1986.0 {
        t = year - 1975.0;
        45.45 + 1.067 * t - t.powi(2) / 260.0 - t.powi(3) / 718.0
    } else if year < 2005.0 {
        t = year - 2000.0;
        63.86 + 0.3345 * t - 0.060374 * t.powi(2)
            + 0.0017275 * t.powi(3)
            + 0.000651814 * t.powi(4)
            + 0.00002373599 * t.powi(5)
    } else if year < 2050.0 {
        t = year - 2000.0;
        62.92 + 0.32217 * t + 0.005589 * t.powi(2)
    } else {
        t = (year - 1820.0) / 100.0;
        -20.0 + 32.0 * t * t - 0.5628 * (2150.0 - year)
    }
}
