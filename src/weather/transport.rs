//! Bounded HTTPS and an integrity-checked cache for NOAA-produced GRIB subsets.
//! Scientific request selection and decoding live in earth-math.
use ring::digest::{SHA256, digest};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const NOAA_PREFIX: &str = "https://nomads.ncep.noaa.gov/cgi-bin/filter_gfs_0p25.pl?";
const MAX_GRIB_BYTES: u64 = 16 * 1024 * 1024;
const MAX_METADATA_BYTES: u64 = 16 * 1024;

pub(super) struct Payload {
    pub bytes: Vec<u8>,
    pub cached: bool,
    pub fetched_unix: u64,
}

pub(super) fn unix_now() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .map_err(|_| "System clock is before the Unix epoch".to_owned())
}

pub(super) fn cache_directory() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("ANCIENT_EARTH_WEATHER_CACHE") {
        return Some(PathBuf::from(path));
    }
    std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("XDG_CACHE_HOME"))
        .map(|path| PathBuf::from(path).join("AncientEarth").join("weather"))
}

fn sha256(bytes: &[u8]) -> String {
    digest(&SHA256, bytes)
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn bounded_read(path: &Path, limit: u64) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut file = fs::File::open(path).ok()?.take(limit + 1);
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= limit).then_some(bytes)
}

fn cached_payload(directory: &Path, url: &str) -> Option<Payload> {
    let metadata = bounded_read(&directory.join("latest.json"), MAX_METADATA_BYTES)?;
    let record: Value = serde_json::from_slice(&metadata).ok()?;
    if record["schema"] != "ancientearth.weather-cache.v1" || record["source_url"] != url {
        return None;
    }
    let bytes = bounded_read(&directory.join("latest.grib2"), MAX_GRIB_BYTES)?;
    if !bytes.starts_with(b"GRIB") || record["sha256"].as_str()? != sha256(&bytes) {
        return None;
    }
    Some(Payload {
        bytes,
        cached: true,
        fetched_unix: record["fetched_unix"].as_u64()?,
    })
}

pub(super) fn save_cache(directory: &Path, url: &str, payload: &Payload) -> std::io::Result<()> {
    fs::create_dir_all(directory)?;
    // Two fixed files bound disk usage. A partial write or concurrent process
    // cannot be accepted: the reader requires a matching URL and payload hash.
    fs::write(directory.join("latest.grib2"), &payload.bytes)?;
    let record = json!({
        "schema": "ancientearth.weather-cache.v1",
        "source": "NOAA / NCEP Global Forecast System (GFS)",
        "source_url": url,
        "fetched_unix": payload.fetched_unix,
        "sha256": sha256(&payload.bytes),
        "terms_url": "https://www.weather.gov/disclaimer",
        "rights": "NOAA-produced public-domain data; commercial reuse and redistribution permitted. Retain attribution. No NOAA endorsement.",
        "modifications": "NOMADS server subset; original downloaded GRIB bytes retained. Ancient Earth derives its own atmospheric scenario; it is not an official NOAA product."
    });
    fs::write(
        directory.join("latest.json"),
        serde_json::to_vec_pretty(&record)?,
    )
}

pub(super) fn load_or_fetch(
    url: &str,
    directory: Option<&Path>,
    cancelled: &AtomicBool,
) -> Result<Payload, String> {
    if cancelled.load(Ordering::Relaxed) {
        return Err("Weather request cancelled".into());
    }
    if !url.starts_with(NOAA_PREFIX) {
        return Err("Weather request is not the approved NOAA endpoint".into());
    }
    if let Some(payload) = directory.and_then(|directory| cached_payload(directory, url)) {
        return Ok(payload);
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .https_only(true)
        .max_redirects(0)
        .timeout_global(Some(Duration::from_secs(45)))
        .build()
        .into();
    let mut response = agent
        .get(url)
        .header(
            "User-Agent",
            "AncientEarth/0.1 (https://github.com/saki2fifty/AncientEarth)",
        )
        .call()
        .map_err(|error| format!("NOAA download failed: {error}"))?;
    if response.status() != 200 {
        return Err(format!("NOAA returned HTTP {}", response.status()));
    }
    let bytes = response
        .body_mut()
        .with_config()
        .limit(MAX_GRIB_BYTES)
        .read_to_vec()
        .map_err(|error| format!("NOAA response could not be read: {error}"))?;
    if cancelled.load(Ordering::Relaxed) {
        return Err("Weather request cancelled".into());
    }
    if !bytes.starts_with(b"GRIB") {
        return Err("NOAA returned a response without a GRIB header".into());
    }
    let payload = Payload {
        bytes,
        cached: false,
        fetched_unix: unix_now()?,
    };
    // The caller writes the cache only after scientific decoding succeeds.
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_rejects_changed_payload_and_other_request() {
        let directory = std::env::temp_dir().join(format!(
            "ancientearth-weather-cache-test-{}",
            std::process::id()
        ));
        let payload = Payload {
            bytes: b"GRIBcache fixture".to_vec(),
            cached: false,
            fetched_unix: 1,
        };
        let url = format!("{NOAA_PREFIX}fixture=1");
        save_cache(&directory, &url, &payload).unwrap();
        assert_eq!(
            cached_payload(&directory, &url).unwrap().bytes,
            payload.bytes
        );
        assert!(cached_payload(&directory, &format!("{NOAA_PREFIX}fixture=2")).is_none());
        fs::write(directory.join("latest.grib2"), b"GRIBchanged fixture").unwrap();
        assert!(cached_payload(&directory, &url).is_none());
        fs::remove_file(directory.join("latest.grib2")).unwrap();
        fs::remove_file(directory.join("latest.json")).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn cancellation_and_unapproved_host_do_not_start_network() {
        let cancelled = AtomicBool::new(true);
        assert!(load_or_fetch(NOAA_PREFIX, None, &cancelled).is_err());
        assert!(load_or_fetch("https://example.com/", None, &AtomicBool::new(false)).is_err());
    }
}
