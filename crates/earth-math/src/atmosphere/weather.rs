//! NOAA GFS request planning and strict scientific GRIB ingestion.
use super::*;
use crate::astro::AstroInstant;
use grib::LatLons;
use std::{collections::BTreeMap, io::Cursor};

#[derive(Clone, Debug, PartialEq)]
pub struct GfsRequest {
    pub url: String,
    pub key: String,
    pub latitude_deg: f64,
    pub longitude_deg: f64,
    pub cycle_unix_seconds: i64,
    pub valid_unix_seconds: i64,
    pub forecast_hour: u32,
}
#[derive(Clone, Debug)]
pub struct WeatherMetadata {
    pub source: String,
    pub model: String,
    pub source_url: String,
    pub latitude_deg: f64,
    pub longitude_deg: f64,
    pub cycle_unix_seconds: i64,
    pub valid_unix_seconds: i64,
    pub provider_surface_geopotential_m: f64,
    pub temperature_2m_k: f64,
    pub pressure_surface_pa: f64,
    pub specific_humidity_2m: f64,
    pub source_levels: usize,
    pub extrapolated_refractivity: bool,
}
impl WeatherMetadata {
    /// Friendly source-condition display; unit conversion and rounding do not
    /// change the profile or imply weather measurement precision. Specific
    /// humidity remains mass fraction, never mislabeled as relative humidity.
    pub fn surface_conditions_label(&self, units: UnitSystem) -> String {
        let celsius = self.temperature_2m_k - 273.15;
        let (temperature, symbol) = match units {
            UnitSystem::Metric => (celsius, "°C"),
            UnitSystem::Imperial => (celsius * 1.8 + 32.0, "°F"),
        };
        format!(
            "2m air: {temperature:.1}{symbol}; surface: {:.1} hPa; specific humidity: {:.2} g/kg",
            self.pressure_surface_pa / 100.0,
            self.specific_humidity_2m * 1000.0
        )
    }
    pub fn usable_at(&self, location: Geodetic, now_unix_seconds: f64) -> bool {
        let Ok((lat, lon)) = grid_point(location.lat_deg, location.lon_deg) else {
            return false;
        };
        lat == self.latitude_deg
            && lon == self.longitude_deg
            && now_unix_seconds.is_finite()
            && (now_unix_seconds - self.valid_unix_seconds as f64).abs() <= 10800.0
    }
    pub fn valid_time_label(&self) -> String {
        AstroInstant::from_unix_utc(self.valid_unix_seconds as f64)
            .map(|i| i.calendar_string())
            .unwrap_or_else(|_| "invalid".into())
    }
    pub fn cycle_time_label(&self) -> String {
        AstroInstant::from_unix_utc(self.cycle_unix_seconds as f64)
            .map(|i| i.calendar_string())
            .unwrap_or_else(|_| "invalid".into())
    }
}
#[derive(Clone, Debug)]
pub struct GfsProfile {
    pub profile: AtmosphereProfile,
    pub metadata: WeatherMetadata,
    pub request: GfsRequest,
}

fn grid_point(latitude: f64, longitude: f64) -> Result<(f64, f64), AtmosphereError> {
    Geodetic::new(latitude, longitude, 0.0).map_err(|_| AtmosphereError::InvalidSample)?;
    let lat = (latitude * 4.0).round() / 4.0;
    let lon = ((longitude.rem_euclid(360.0) * 4.0).round() / 4.0).rem_euclid(360.0);
    Ok((lat, if lon > 180.0 { lon - 360.0 } else { lon }))
}
pub fn request_candidates(
    latitude: f64,
    longitude: f64,
    now_unix_seconds: f64,
) -> Result<Vec<GfsRequest>, AtmosphereError> {
    let (lat, lon) = grid_point(latitude, longitude)?;
    let date = AstroInstant::from_unix_utc(now_unix_seconds)
        .map_err(|_| AtmosphereError::InvalidSample)?
        .calendar();
    if date.year < 2020 {
        return Err(AtmosphereError::InvalidSample);
    }
    let valid = (now_unix_seconds / 3600.0).round() as i64 * 3600;
    let latest = ((now_unix_seconds - 21600.0) / 21600.0).floor() as i64 * 21600;
    let mut result = Vec::new();
    for offset in 0..3 {
        let cycle = latest - offset * 21600;
        let date = AstroInstant::from_unix_utc(cycle as f64)
            .map_err(|_| AtmosphereError::InvalidSample)?
            .calendar();
        let forecast = ((valid - cycle) / 3600) as u32;
        let lon360 = lon.rem_euclid(360.0);
        let west = (lon360 - 0.25).max(0.0);
        let east = (lon360 + 0.25).min(359.75);
        let south = (lat - 0.25).max(-90.0);
        let north = (lat + 0.25).min(90.0);
        let day = format!("{:04}{:02}{:02}", date.year, date.month, date.day);
        let url = format!(
            "https://nomads.ncep.noaa.gov/cgi-bin/filter_gfs_0p25.pl?file=gfs.t{:02}z.pgrb2.0p25.f{forecast:03}&all_lev=on&var_TMP=on&var_SPFH=on&var_HGT=on&var_PRES=on&subregion=&leftlon={west:.2}&rightlon={east:.2}&toplat={north:.2}&bottomlat={south:.2}&dir=%2Fgfs.{day}%2F{:02}%2Fatmos",
            date.hour, date.hour
        );
        result.push(GfsRequest {
            url,
            key: format!(
                "gfs-{day}-{:02}-f{forecast:03}-{lat:.2}-{lon:.2}",
                date.hour
            ),
            latitude_deg: lat,
            longitude_deg: lon,
            cycle_unix_seconds: cycle,
            valid_unix_seconds: valid,
            forecast_hour: forecast,
        });
    }
    Ok(result)
}
impl GfsRequest {
    pub fn matches_location(&self, location: Geodetic) -> bool {
        grid_point(location.lat_deg, location.lon_deg)
            .is_ok_and(|p| p == (self.latitude_deg, self.longitude_deg))
    }
}

/// Frozen independently decoded NOAA forecast used only by explicitly selected
/// verification mode. It is never a live-weather fallback.
pub fn reference_request() -> GfsRequest {
    request_candidates(29.25, -94.75, 1_792_609_200.0)
        .expect("fixed valid reference request")
        .remove(0)
}

/// Explicit optical verification pose at the frozen source column. The pitch
/// lies between independently checked straight and refracted skyline angles;
/// this is a diagnostic, not a catalog of real-world locations.
pub fn reference_observer() -> Observer {
    let mut observer = Observer::new(Geodetic::new(29.25, -94.75, 6.0).expect("fixed valid pose"));
    observer.heading_deg = 90.0;
    observer.pitch_deg = -0.085655878886;
    observer
}

pub fn reference_lens() -> Lens {
    Lens::new(Projection::Rectilinear, 0.2)
}

#[derive(Default)]
struct Sample {
    temperature: Option<f64>,
    humidity: Option<f64>,
    height: Option<f64>,
}
fn rejected(message: impl Into<String>) -> AtmosphereError {
    AtmosphereError::InvalidWeather(message.into())
}
pub fn decode_gfs(bytes: &[u8], request: &GfsRequest) -> Result<GfsProfile, AtmosphereError> {
    if request
        .cycle_unix_seconds
        .checked_add(i64::from(request.forecast_hour) * 3600)
        != Some(request.valid_unix_seconds)
    {
        return Err(rejected("request cycle and valid time disagree"));
    }
    if bytes.len() > 8_000_000 || bytes.len() < 16 || &bytes[..4] != b"GRIB" {
        return Err(rejected("not a bounded GRIB2 response"));
    }
    let parsed = grib::from_reader(Cursor::new(bytes)).map_err(|e| rejected(e.to_string()))?;
    let expected = AstroInstant::from_unix_utc(request.cycle_unix_seconds as f64)
        .map_err(|_| AtmosphereError::InvalidSample)?
        .calendar();
    let mut levels = BTreeMap::<u32, Sample>::new();
    let mut surface = Sample::default();
    let mut air = Sample::default();
    let mut pressure = None;
    for (_, message) in parsed.iter() {
        if message.indicator().discipline != 0 {
            continue;
        }
        let product = message.prod_def();
        let Some((category, parameter)) =
            product.parameter_category().zip(product.parameter_number())
        else {
            continue;
        };
        if ![(0, 0), (1, 0), (3, 0), (3, 5)].contains(&(category, parameter)) {
            continue;
        }
        let Some((fixed, _)) = product.fixed_surfaces() else {
            continue;
        };
        if fixed.surface_type != 100
            && fixed.surface_type != 1
            && !(fixed.surface_type == 103 && fixed.value() == 2.0)
        {
            continue;
        }
        if message.identification().centre_id() != 7 || product.prod_tmpl_num() != 0 {
            return Err(rejected("expected instantaneous NOAA/NCEP forecast fields"));
        }
        let reference = message.identification().ref_time_unchecked();
        if i32::from(reference.year) != expected.year
            || u32::from(reference.month) != expected.month
            || u32::from(reference.day) != expected.day
            || u32::from(reference.hour) != expected.hour
            || reference.minute != 0
            || reference.second != 0
            || product.forecast_time()
                != Some(grib::ForecastTime::from_numbers(1, request.forecast_hour))
        {
            return Err(rejected("mixed or unexpected model cycle/forecast time"));
        }
        if message.grid_def().grid_tmpl_num() != 0 || message.grid_def().num_points() > 10000 {
            return Err(rejected("unsupported or oversized source grid"));
        }
        let points: Vec<_> = message
            .latlons()
            .map_err(|e| rejected(e.to_string()))?
            .take(10001)
            .collect();
        if points.len() > 10000 {
            return Err(rejected("unexpected global grid response"));
        }
        let selected = points
            .iter()
            .position(|&(lat, lon)| {
                (f64::from(lat) - request.latitude_deg).abs() < 1e-6
                    && ((f64::from(lon) - request.longitude_deg + 180.0).rem_euclid(360.0) - 180.0)
                        .abs()
                        < 1e-6
            })
            .ok_or_else(|| rejected("requested source grid column is absent"))?;
        let decoder =
            grib::Grib2SubmessageDecoder::from(message).map_err(|e| rejected(e.to_string()))?;
        let value = f64::from(
            decoder
                .dispatch()
                .map_err(|e| rejected(e.to_string()))?
                .nth(selected)
                .ok_or_else(|| rejected("truncated grid values"))?,
        );
        if !value.is_finite() {
            return Err(rejected("missing required source value"));
        }
        if fixed.surface_type == 1 && (category, parameter) == (3, 0) {
            if pressure.replace(value).is_some() {
                return Err(rejected("duplicate surface pressure"));
            }
            continue;
        }
        let sample = if fixed.surface_type == 100 {
            let p = fixed.value();
            if !p.is_finite() || !(1.0..=110000.0).contains(&p) || p.fract() != 0.0 {
                continue;
            }
            levels.entry(p as u32).or_default()
        } else if fixed.surface_type == 103 {
            &mut air
        } else {
            &mut surface
        };
        let slot = match (category, parameter) {
            (0, 0) => &mut sample.temperature,
            (1, 0) => &mut sample.humidity,
            (3, 5) => &mut sample.height,
            _ => continue,
        };
        if slot.replace(value).is_some() {
            return Err(rejected("duplicate required field"));
        }
    }
    let ground = surface
        .height
        .ok_or_else(|| rejected("surface geopotential height missing"))?;
    let temperature = air
        .temperature
        .ok_or_else(|| rejected("2m air temperature missing"))?;
    let humidity = air
        .humidity
        .ok_or_else(|| rejected("2m specific humidity missing"))?;
    let pressure = pressure.ok_or_else(|| rejected("actual surface pressure missing"))?;
    if !(-1000.0..=10000.0).contains(&ground) {
        return Err(rejected("surface height outside provider domain"));
    }
    // Explicit coordinate mapping: convert geopotential heights using the
    // conventional geopotential radius, then subtract the provider ground.
    // This is a transplanted profile, not restored terrain or local observation.
    let geometric = |h: f64| 6_356_766.0 * h / (6_356_766.0 - h);
    let ground_geometric = geometric(ground);
    let virtual_temperature = temperature * (1.0 + 0.6078 * humidity);
    let p2 = pressure * (-9.80665 * 2.0 / (287.05 * virtual_temperature)).exp();
    let mut samples = vec![
        (0.0, temperature, pressure, humidity),
        (2.0, temperature, p2, humidity),
    ];
    for (p, sample) in levels.into_iter().rev() {
        let (Some(t), Some(q), Some(h)) = (sample.temperature, sample.humidity, sample.height)
        else {
            return Err(rejected("incomplete pressure-level triplet"));
        };
        if !(-1000.0..=110000.0).contains(&h) {
            return Err(rejected(
                "pressure-level geopotential outside supported domain",
            ));
        }
        let height = geometric(h) - ground_geometric;
        if !height.is_finite() {
            return Err(rejected("invalid mapped source altitude"));
        }
        if p as f64 >= p2 || height <= 2.01 {
            continue;
        }
        if height > 110000.0 || !height.is_finite() {
            return Err(rejected("pressure level outside altitude mapping"));
        }
        samples.push((height, t, p as f64, q));
    }
    if samples.len() < 12 || samples.last().unwrap().0 < 60000.0 {
        return Err(rejected("insufficient vertical coverage"));
    }
    let source_levels = samples.len();
    let extrapolated = samples
        .iter()
        .any(|&(_, t, p, _)| !(233.15..=373.15).contains(&t) || !(80000.0..=120000.0).contains(&p));
    let mut layers = Vec::new();
    for &(height, t, p, q) in &samples {
        layers.push(AtmosphereLayer {
            height_m: height,
            refractivity: phase_index(t, p, q, 550.0)? - 1.0,
        });
    }
    // A declared high-altitude continuation closes continuously to vacuum;
    // it is not an extra weather observation or validated thermosphere model.
    layers.push(AtmosphereLayer {
        height_m: 120000.0,
        refractivity: 0.0,
    });
    let mut profile = AtmosphereProfile::from_layers(layers)?;
    let metadata = WeatherMetadata {
        source: "NOAA NCEP NOMADS".into(),
        model: "GFS 0.25 degree forecast column".into(),
        source_url: request.url.clone(),
        latitude_deg: request.latitude_deg,
        longitude_deg: request.longitude_deg,
        cycle_unix_seconds: request.cycle_unix_seconds,
        valid_unix_seconds: request.valid_unix_seconds,
        provider_surface_geopotential_m: ground,
        temperature_2m_k: temperature,
        pressure_surface_pa: pressure,
        specific_humidity_2m: humidity,
        source_levels,
        extrapolated_refractivity: extrapolated,
    };
    profile.metadata = Some(metadata.clone());
    Ok(GfsProfile {
        profile,
        metadata,
        request: request.clone(),
    })
}
