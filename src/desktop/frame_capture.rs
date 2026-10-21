//! Raw application frame pacing, deliberately separate from GPU timer queries.
use super::{Explorer, renderer::RenderedEarth};
use ancient_earth::{simulation::AstronomicalScene, surface::WorldSurface};
use bevy::{
    ecs::system::SystemParam, prelude::*, render::renderer::RenderAdapterInfo,
    window::PrimaryWindow,
};
use serde_json::{Value, json};
use std::{io::Write, path::PathBuf};
mod motion;
use motion::MotionProbe;

const WARMUP_FRAMES: u64 = 120;
const WARMUP_SECONDS: f64 = 10.0;
const RECORD_SECONDS: f64 = 20.0;
const PREPARATION_TIMEOUT_SECONDS: f64 = 300.0;

#[derive(Default)]
struct SampleWindow {
    frames: u64,
    started: bool,
    ready_since: Option<f64>,
    incomplete_scene_frames: u64,
    intervals: Vec<f64>,
    recorded_seconds: f64,
}
#[derive(Debug, PartialEq, Eq)]
enum SampleState {
    Warming,
    Started,
    Recording,
    Complete,
}
impl SampleWindow {
    fn observe_scene(
        &mut self,
        elapsed: f64,
        interval: f64,
        presentation_ready: bool,
    ) -> Result<SampleState, &'static str> {
        if !elapsed.is_finite() || elapsed < 0.0 {
            return Err("invalid elapsed time");
        }
        if !self.started {
            if !presentation_ready {
                self.frames = 0;
                self.ready_since = None;
                return Ok(SampleState::Warming);
            }
            let ready_since = *self.ready_since.get_or_insert(elapsed);
            return self.observe(elapsed - ready_since, interval);
        }
        // Never remove expensive intervals once recording begins. A scene
        // change invalidates a stationary baseline but remains in the raw data.
        self.incomplete_scene_frames += u64::from(!presentation_ready);
        self.observe(elapsed, interval)
    }

    fn observe(&mut self, elapsed: f64, interval: f64) -> Result<SampleState, &'static str> {
        self.frames += 1;
        if !elapsed.is_finite() || elapsed < 0.0 {
            return Err("invalid elapsed time");
        }
        if !self.started {
            if self.frames >= WARMUP_FRAMES && elapsed >= WARMUP_SECONDS {
                self.started = true;
                // The just-finished interval began during warmup. Recording
                // begins at this boundary, with the NEXT complete interval.
                return Ok(SampleState::Started);
            }
            return Ok(SampleState::Warming);
        }
        if !interval.is_finite() || interval <= 0.0 {
            return Err("recorded frame interval must be finite and positive");
        }
        self.intervals.push(interval);
        self.recorded_seconds += interval;
        Ok(if self.recorded_seconds >= RECORD_SECONDS {
            SampleState::Complete
        } else {
            SampleState::Recording
        })
    }
}

#[derive(Resource)]
struct FrameCapture {
    output: Result<PathBuf, String>,
    arguments: Vec<String>,
    samples: SampleWindow,
    start_metadata: Option<Value>,
    start_view: Option<Value>,
    view_changed: bool,
    unfocused_frames: u64,
    motion_requested: bool,
    motion: Option<MotionProbe>,
    done: bool,
}

/// Install only when explicitly requested. Root startup owns the other CLI modes.
pub fn install(app: &mut App, args: &[String]) {
    let positions: Vec<_> = args
        .iter()
        .enumerate()
        .filter_map(|(i, arg)| (arg == "--frame-capture").then_some(i))
        .collect();
    let motion_requested = args.iter().any(|arg| arg == "--frame-capture-motion");
    if positions.is_empty() && !motion_requested {
        return;
    }
    let index = positions.first().copied().unwrap_or(0);
    let output = if positions.len() != 1 {
        Err("--frame-capture must occur once".into())
    } else if args.iter().any(|arg| arg == "--screenshot") {
        Err("--frame-capture and --screenshot are separate runs".into())
    } else {
        args.get(index + 1)
            .filter(|value| !value.is_empty() && !value.starts_with("--"))
            .map(PathBuf::from)
            .ok_or_else(|| "--frame-capture requires an output JSON path".into())
    };
    app.insert_resource(FrameCapture {
        output,
        arguments: args.to_vec(),
        samples: SampleWindow::default(),
        start_metadata: None,
        start_view: None,
        view_changed: false,
        unfocused_frames: 0,
        motion_requested,
        motion: None,
        done: false,
    })
    .add_systems(Update, collect);
}

#[derive(SystemParam)]
struct CaptureContext<'w, 's> {
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    explorer: ResMut<'w, Explorer>,
    surface: Res<'w, WorldSurface>,
    astronomy: Res<'w, AstronomicalScene>,
    weather: Res<'w, ancient_earth::weather::WeatherState>,
    rendered: Res<'w, RenderedEarth>,
    adapter: Option<Res<'w, RenderAdapterInfo>>,
}

fn view_configuration(window: &Window, context: &CaptureContext) -> Value {
    let state = &context.explorer;
    let eye = state.observer.eye_position();
    let lens = state.lens();
    json!({
        "physical_size": [window.physical_width(), window.physical_height()],
        "scale_factor": window.scale_factor(),
        "present_mode": format!("{:?}", window.present_mode),
        "projection": format!("{:?}", lens.projection),
        "vertical_fov_degrees": lens.vertical_fov_deg,
        "eye_ecef_metres": [eye.x, eye.y, eye.z],
        "heading_degrees": state.observer.heading_deg,
        "pitch_degrees": state.observer.pitch_deg,
        "roll_degrees": state.observer.roll_deg,
        "follow_earth": state.follow_earth,
        "horizontal_guide": state.show_horizontal_line,
        "vertical_guide": state.show_vertical_line,
        "guide_column": state.guide_column,
        "boundaries_enabled": state.boundaries_enabled,
        "grass_enabled": state.grass_enabled,
        "show_ui": state.show_ui,
        "show_panel": state.show_panel,
        "quality": state.quality,
        "renderer": context.rendered.mode,
        "weather_enabled": context.weather.enabled,
        "weather_refraction_rendered": context.weather.view_profile(state.observer).is_some(),
        "weather_profile_generation": context.weather.generation,
        "weather_reference_snapshot": context.weather.reference_snapshot,
        "weather_valid_unix_seconds": context.weather.active_profile().and_then(|p| p.metadata.as_ref()).map(|m| m.valid_unix_seconds),
        "weather_cycle_unix_seconds": context.weather.active_profile().and_then(|p| p.metadata.as_ref()).map(|m| m.cycle_unix_seconds),
        "presentation_ready": context.rendered.presentation_ready,
    })
}

fn collect(
    mut capture: ResMut<FrameCapture>,
    time: Res<Time<Real>>,
    mut context: CaptureContext,
    mut exit: MessageWriter<AppExit>,
) {
    if capture.done {
        return;
    }
    if let Err(message) = &capture.output {
        error!("Frame capture: {message}");
        capture.done = true;
        exit.write(AppExit::error());
        return;
    }
    if !capture.samples.started && time.elapsed_secs_f64() >= PREPARATION_TIMEOUT_SECONDS {
        error!("Frame capture preparation/warmup exceeded the documented 300-second watchdog");
        capture.done = true;
        exit.write(AppExit::error());
        return;
    }
    let Ok(window) = context.windows.single() else {
        error!("Frame capture requires one primary window");
        capture.done = true;
        exit.write(AppExit::error());
        return;
    };
    let ready = context.rendered.presentation_ready
        && (!context.weather.enabled || context.weather.active_profile().is_some());
    let state =
        match capture
            .samples
            .observe_scene(time.elapsed_secs_f64(), time.delta_secs_f64(), ready)
        {
            Ok(state) => state,
            Err(message) => {
                error!("Frame capture: {message}");
                capture.done = true;
                exit.write(AppExit::error());
                return;
            }
        };
    if state == SampleState::Warming {
        return;
    }
    let view = view_configuration(window, &context);
    if state == SampleState::Started {
        if capture.motion_requested {
            match MotionProbe::begin(
                &context.explorer,
                context.rendered.grass_qa_snapshot(),
                time.elapsed_secs_f64(),
            ) {
                Ok(probe) => capture.motion = Some(probe),
                Err(message) => {
                    error!("{message}");
                    capture.done = true;
                    exit.write(AppExit::error());
                    return;
                }
            }
        }
        capture.start_view = Some(view.clone());
        capture.start_metadata = Some(json!({
            "build_profile": if cfg!(debug_assertions) { "debug" } else { "release" },
            "capture_mode": if capture.motion_requested { "nonstationary-motion-qa" } else { "stationary" },
            "arguments": capture.arguments,
            "view": view,
            "clock_calendar": context.astronomy.clock.calendar_string(),
            "clock_running": context.astronomy.clock.running,
            "clock_rate": context.astronomy.clock.rate,
            "warmup_frames_observed": capture.samples.frames,
            "warmup_elapsed_seconds": time.elapsed_secs_f64()
                - capture.samples.ready_since.expect("ready before recording"),
            "application_elapsed_seconds_at_recording": time.elapsed_secs_f64(),
            "adapter": context.adapter.as_ref().map(|info| json!({
                "name": info.name,
                "backend": format!("{:?}", info.backend),
                "device_type": format!("{:?}", info.device_type),
                "driver": info.driver,
                "driver_info": info.driver_info,
            })),
        }));
        info!("Frame capture warmup complete; recording application intervals");
        return;
    }
    capture.view_changed |=
        capture.start_view.as_ref() != Some(&view) || !context.rendered.presentation_ready;
    capture.unfocused_frames += u64::from(!window.focused);
    let motion_complete = if let Some(probe) = &mut capture.motion {
        match probe.step(
            &mut context.explorer,
            &context.surface,
            context.rendered.grass_qa_snapshot(),
            context.rendered.presentation_ready,
            time.elapsed_secs_f64(),
            time.delta_secs_f64(),
        ) {
            Ok(complete) => complete,
            Err(message) => {
                error!("{message}");
                capture.done = true;
                exit.write(AppExit::error());
                return;
            }
        }
    } else {
        true
    };
    if state != SampleState::Complete || !motion_complete {
        return;
    }
    let report = json!({
        "format": "ancient-earth-frame-capture-v1",
        "measurement": "raw application frame intervals, not GPU execution time",
        "warmup_minimum_frames": WARMUP_FRAMES,
        "warmup_minimum_seconds": WARMUP_SECONDS,
        "requested_recording_seconds": RECORD_SECONDS,
        "preparation_watchdog_seconds": PREPARATION_TIMEOUT_SECONDS,
        "metadata": capture.start_metadata,
        "final_view": view,
        "final_clock_calendar": context.astronomy.clock.calendar_string(),
        "view_changed_during_capture": capture.view_changed,
        "incomplete_scene_frames": capture.samples.incomplete_scene_frames,
        "unfocused_recorded_frames": capture.unfocused_frames,
        "sample_count": capture.samples.intervals.len(),
        "recorded_duration_seconds": capture.samples.recorded_seconds,
        "interval_seconds": capture.samples.intervals,
        "motion_qa": capture.motion.as_ref().map(MotionProbe::report),
    });
    let output = capture.output.as_ref().expect("validated output path");
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        if let Some(parent) = output.parent().filter(|path| !path.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::io::BufWriter::new(std::fs::File::create(output)?);
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
        file.flush()?;
        Ok(())
    })();
    match result {
        Ok(()) => {
            info!("Saved raw frame capture to {}", output.display());
            exit.write(AppExit::Success);
        }
        Err(error) => {
            error!(
                "Could not write frame capture {}: {error}",
                output.display()
            );
            exit.write(AppExit::error());
        }
    }
    capture.done = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_requires_both_warmup_thresholds_and_excludes_boundary_interval() {
        let mut samples = SampleWindow::default();
        for _ in 0..119 {
            assert_eq!(samples.observe(20.0, 3.0), Ok(SampleState::Warming));
        }
        assert_eq!(samples.observe(20.1, 3.0), Ok(SampleState::Started));
        assert!(samples.intervals.is_empty());
        assert_eq!(samples.observe(20.12, 0.02), Ok(SampleState::Recording));
        assert_eq!(samples.intervals, [0.02]);

        let mut fast = SampleWindow::default();
        for _ in 0..150 {
            assert_eq!(fast.observe(5.0, 0.01), Ok(SampleState::Warming));
        }
        assert_eq!(fast.observe(10.0, 5.0), Ok(SampleState::Started));
        assert!(fast.intervals.is_empty());
    }

    #[test]
    fn capture_keeps_complete_intervals_and_rejects_invalid_recorded_values() {
        let mut samples = SampleWindow {
            started: true,
            ..default()
        };
        assert_eq!(samples.observe(11.0, 7.0), Ok(SampleState::Recording));
        assert_eq!(samples.observe(18.0, 7.0), Ok(SampleState::Recording));
        assert_eq!(samples.observe(25.0, 7.0), Ok(SampleState::Complete));
        assert_eq!(samples.intervals, [7.0, 7.0, 7.0]);
        assert_eq!(samples.recorded_seconds, 21.0);
        for invalid in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(samples.observe(26.0, invalid).is_err());
            assert_eq!(samples.intervals.len(), 3);
        }
    }

    #[test]
    fn preparation_resets_warmup_but_never_drops_intervals_after_recording_starts() {
        let mut samples = SampleWindow::default();
        for _ in 0..200 {
            assert_eq!(
                samples.observe_scene(500.0, 3.0, false),
                Ok(SampleState::Warming)
            );
        }
        for index in 0..120 {
            assert_eq!(
                samples.observe_scene(501.0 + f64::from(index) * 0.01, 0.01, true),
                Ok(SampleState::Warming)
            );
        }
        assert_eq!(
            samples.observe_scene(510.0, 0.01, false),
            Ok(SampleState::Warming)
        );
        assert_eq!(samples.frames, 0);
        for index in 0..120 {
            assert_eq!(
                samples.observe_scene(511.0 + f64::from(index) * 0.01, 0.01, true),
                Ok(SampleState::Warming)
            );
        }
        assert_eq!(
            samples.observe_scene(521.0, 8.81, true),
            Ok(SampleState::Started)
        );
        assert!(samples.intervals.is_empty());
        assert_eq!(
            samples.observe_scene(526.0, 5.0, false),
            Ok(SampleState::Recording)
        );
        assert_eq!(
            samples.observe_scene(526.02, 0.02, true),
            Ok(SampleState::Recording)
        );
        assert_eq!(samples.intervals, [5.0, 0.02]);
        assert_eq!(samples.incomplete_scene_frames, 1);
    }
}
