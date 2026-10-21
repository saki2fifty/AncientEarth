use ancient_earth::surface::WorldSurface;
use ancient_earth::weather::WeatherState;
use dear_imgui_rs::{TreeNodeFlags, Ui};
use earth_math::{
    Ray, TerrainScene, UnitSystem, astro::AstronomyFrame, atmosphere::AtmosphereProfile,
};

pub(super) fn draw(
    ui: &Ui,
    weather: &mut WeatherState,
    units: UnitSystem,
    observer: earth_math::Observer,
) {
    if !ui.collapsing_header("WEATHER & REFRACTION", TreeNodeFlags::DEFAULT_OPEN) {
        return;
    }
    let mut enabled = weather.enabled;
    if ui.checkbox("Enable weather refraction", &mut enabled) {
        weather.set_enabled(enabled);
    }
    ui.text_wrapped(&weather.status);
    if weather.enabled {
        ui.text_wrapped(format!(
            "Optical display eye-height limit: {} above WGS84. Higher views use geometric rays.",
            units.format_distance(earth_math::atmosphere::gpu::MAX_GPU_EYE_HEIGHT_M)
        ));
    }
    if weather.active_profile().is_some()
        && !earth_math::atmosphere::gpu::display_supported(observer)
    {
        ui.text_wrapped("GEOMETRIC VIEW: this eye altitude is outside the supported refraction display range. Weather data remain available; view, range and L/V guides all use geometric rays.");
    }
    if let Some(error) = &weather.last_error {
        ui.text_wrapped(error);
    }
    if weather.enabled
        && ui.small_button(if weather.reference_snapshot {
            "Use live weather"
        } else {
            "Refresh weather"
        })
    {
        weather.refresh();
    }
    if let Some(metadata) = weather
        .profile
        .as_ref()
        .and_then(|profile| profile.metadata.as_ref())
    {
        ui.text_wrapped(&metadata.model);
        ui.text_wrapped(format!("Valid {} UTC", metadata.valid_time_label()));
        ui.text_wrapped(format!("Run {} UTC", metadata.cycle_time_label()));
        ui.text_wrapped(format!(
            "Source grid {:.2}, {:.2}",
            metadata.latitude_deg, metadata.longitude_deg
        ));
        ui.text_wrapped(metadata.surface_conditions_label(units));
    }
    ui.text_wrapped("Live weather follows your in-app location and real UTC. The Sun/Moon calendar is independent.");
    ui.text_wrapped("NOAA forecast layers are mapped onto the smooth surface and used across the view. Thin local mirage layers are unresolved.");
    ui.text_wrapped(
        "Refraction view uses simplified lighting; decorative grass and its shadows are hidden.",
    );
    ui.text_wrapped("Source: NOAA / NCEP GFS. Modified optical scenario; no NOAA endorsement.");
}

pub(super) fn draw_sight(
    ui: &Ui,
    units: UnitSystem,
    profile: &AtmosphereProfile,
    ray: Ray,
    scene: &TerrainScene,
    astronomy: Option<&AstronomyFrame>,
) {
    let trace = match profile.trace(ray, scene) {
        Ok(trace) => trace,
        Err(error) => {
            ui.text_wrapped(format!("Refracted range unavailable: {error}"));
            return;
        }
    };
    let distances = |path, chord, optical| {
        ui.text(units.format_distance(path));
        ui.text_colored(super::MUTED, units.format_precise_distance(path));
        ui.text_colored(super::MUTED, "Curved geometric path from the eyes");
        ui.text_wrapped(format!(
            "Straight endpoint distance: {}",
            units.format_distance(chord)
        ));
        ui.text_wrapped(format!(
            "Phase optical path: {}",
            units.format_distance(optical)
        ));
    };
    if let Some(celestial) = astronomy.and_then(|frame| trace.nearest_celestial(frame)) {
        distances(
            celestial.geometric_path_m,
            celestial.chord_m,
            celestial.optical_path_m,
        );
        ui.text_wrapped(celestial.hit.body.description());
    } else if let Some(surface) = trace.surface {
        distances(
            trace.geometric_path_m,
            surface.hit.distance_m,
            trace.optical_path_m,
        );
        ui.text_wrapped(WorldSurface::label(surface.kind));
        ui.text_wrapped(format!(
            "Lat {:.7} / Lon {:.7}",
            surface.hit.geodetic.lat_deg, surface.hit.geodetic.lon_deg
        ));
    } else {
        ui.text("No target intersection");
        ui.text_wrapped("Refracted ray leaves the atmosphere toward the sky.");
    }
}
