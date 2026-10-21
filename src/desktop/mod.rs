mod controls;
mod frame_capture;
mod renderer;
mod ui;

use ancient_earth::datum::SeaLevel;
use ancient_earth::navigation::Explorer;
use ancient_earth::simulation::AstronomicalScene;
use ancient_earth::surface::WorldSurface;
use ancient_earth::weather::WeatherState;
use bevy::prelude::*;
use dear_imgui_bevy::prelude::*;
use earth_math::{Lens, Projection, UnitSystem};

#[derive(Resource)]
pub struct AutomatedView(pub bool);

#[derive(Resource)]
struct SmokeTest {
    output: Option<String>,
    frames: u32,
    requested: bool,
}

pub fn run() -> AppExit {
    let mut explorer = Explorer::default();
    let surface = WorldSurface::bundled().expect("valid bundled land/water boundaries");
    let sea_level = SeaLevel::bundled().expect("valid bundled EGM2008 geoid");
    let args: Vec<String> = std::env::args().collect();
    let mut weather = WeatherState::default();
    if args.iter().any(|arg| arg == "--weather-reference") {
        if args.iter().any(|arg| arg == "--weather") {
            eprintln!(
                "Choose --weather for live data or --weather-reference for the fixed verification snapshot"
            );
            return AppExit::error();
        }
        if let Err(error) = weather.load_reference() {
            eprintln!("Could not load the fixed weather verification snapshot: {error}");
            return AppExit::error();
        }
    } else if args.iter().any(|arg| arg == "--weather") {
        weather.set_enabled(true);
    }
    if args.iter().any(|arg| arg == "--coast") {
        explorer.visit_coast(surface.active(true));
    }
    if args.iter().any(|arg| arg == "--show-licenses") {
        explorer.show_licenses = true;
    }
    if args.iter().any(|arg| arg == "--reference") {
        explorer.boundaries_enabled = false;
    }
    if args.iter().any(|arg| arg == "--ground") {
        explorer.ground_on_surface(surface.active(explorer.boundaries_enabled));
        explorer.observer.pitch_deg = -30.0;
    }
    if args.iter().any(|arg| arg == "--imperial") {
        explorer.units = UnitSystem::Imperial;
    }
    if args.iter().any(|arg| arg == "--crouch") {
        explorer.crouch_locked = true;
        explorer.update_crouch(false);
    }
    if args.iter().any(|arg| arg == "--line") {
        explorer.show_horizontal_line = true;
    }
    if args.iter().any(|arg| arg == "--vertical-guide") {
        explorer.show_horizontal_line = true;
        explorer.show_vertical_line = true;
    }
    if let Some(pair) = args.windows(2).find(|pair| pair[0] == "--guide-column") {
        match pair[1].parse::<f64>() {
            Ok(column) if column.is_finite() && (0.0..=1.0).contains(&column) => {
                explorer.guide_column = column;
            }
            _ => {
                eprintln!("--guide-column requires a fraction from 0 to 1");
                return AppExit::error();
            }
        }
    }
    if args.iter().any(|arg| arg == "--earth-unlocked") {
        explorer.follow_earth = false;
    }
    if let Some(pair) = args.windows(2).find(|pair| pair[0] == "--guide-demo") {
        use earth_math::guides::{GuideDemo, demo_observer};
        let demo = match pair[1].as_str() {
            "horizon" => GuideDemo::HighAltitude,
            "rolled" => GuideDemo::Rolled,
            "sky" => GuideDemo::SkyOnly,
            _ => {
                eprintln!("--guide-demo requires horizon, rolled or sky");
                return AppExit::error();
            }
        };
        explorer.observer = demo_observer(demo);
        explorer.show_horizontal_line = true;
        explorer.show_panel = false;
    }
    if args.iter().any(|arg| arg == "--clean-view") {
        explorer.show_ui = false;
    }
    if args.iter().any(|arg| arg == "--no-grass") {
        explorer.grass_enabled = false;
    }
    if let Some(pair) = args.windows(2).find(|pair| pair[0] == "--quality") {
        explorer.quality = match pair[1].as_str() {
            "performance" => 0,
            "balanced" => 1,
            "fine" => 2,
            "high" => 3,
            _ => {
                eprintln!("--quality requires performance, balanced, fine or high");
                std::process::exit(2);
            }
        };
    }
    let mut astronomy = AstronomicalScene::bundled(explorer.observer)
        .expect("valid bundled astronomy data and supported system date");
    if let Some(pair) = args.windows(2).find(|pair| pair[0] == "--datetime") {
        astronomy.calendar_input = pair[1].clone();
        astronomy.apply_calendar();
        astronomy.sample(explorer.observer, explorer.follow_earth);
    }
    if args.iter().any(|arg| arg == "--paused") {
        astronomy.clock.running = false;
    }
    if args.iter().any(|arg| arg == "--fisheye") {
        explorer.normal_lens = Lens::new(Projection::EquidistantFisheye, 140.0);
    }
    if args.iter().any(|arg| arg == "--telescope") {
        explorer.telescope = true;
    }
    if args.iter().any(|arg| arg == "--weather-horizon-check") {
        explorer.observer = earth_math::atmosphere::weather::reference_observer();
        explorer.normal_lens = earth_math::atmosphere::weather::reference_lens();
        explorer.telescope = false;
        explorer.grass_enabled = false;
    }
    if let Some(frame) = astronomy.frame {
        if args.iter().any(|arg| arg == "--look-sun") {
            explorer
                .observer
                .aim_at(frame.sun.apparent_center_ecef_m)
                .expect("valid Sun direction");
        }
        if args.iter().any(|arg| arg == "--look-moon") {
            explorer
                .observer
                .aim_at(frame.moon.apparent_center_ecef_m)
                .expect("valid Moon direction");
        }
    }
    if let Err(error) = astronomy.reset_tracking(explorer.observer, explorer.follow_earth) {
        eprintln!("Could not establish observer frame: {error}");
        return AppExit::error();
    }
    astronomy.sample(explorer.observer, explorer.follow_earth);
    let output = args
        .windows(2)
        .find(|pair| pair[0] == "--screenshot")
        .map(|pair| pair[1].clone());
    // Exercise physical-pixel overlays without modifying the host display.
    let test_hidpi = output.is_some() && args.iter().any(|arg| arg == "--test-hidpi");
    let capture_fullscreen =
        output.is_some() && args.iter().any(|arg| arg == "--capture-fullscreen");
    let automated_view = output.is_some()
        || args
            .iter()
            .any(|arg| arg == "--frame-capture" || arg == "--frame-capture-motion");
    if let Some(path) = output
        .as_ref()
        .and_then(|path| std::path::Path::new(path).parent())
        && !path.as_os_str().is_empty()
    {
        std::fs::create_dir_all(path).expect("create screenshot output directory");
    }
    let mut app = App::new();
    app.insert_resource(ClearColor(Color::srgb(0.015, 0.025, 0.05)))
        .insert_resource(surface)
        .insert_resource(sea_level)
        .insert_resource(explorer)
        .insert_resource(astronomy)
        .insert_resource(weather)
        .insert_resource(AutomatedView(automated_view))
        .insert_resource(SmokeTest {
            output,
            frames: 0,
            requested: false,
        })
        .add_plugins(
            DefaultPlugins
                .set(bevy::render::RenderPlugin {
                    // Finish each required pipeline before presenting its first frame.
                    // Otherwise initial and newly selected views can remain partially blank.
                    synchronous_pipeline_compilation: true,
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Ancient Earth | WGS84 Observatory".into(),
                        mode: if capture_fullscreen {
                            bevy::window::WindowMode::BorderlessFullscreen(
                                bevy::window::MonitorSelection::Current,
                            )
                        } else {
                            bevy::window::WindowMode::Windowed
                        },
                        resolution: if test_hidpi {
                            bevy::window::WindowResolution::new(1920, 1080)
                                .with_scale_factor_override(1.5)
                        } else {
                            (1920, 1080).into()
                        },
                        present_mode: bevy::window::PresentMode::AutoVsync,
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_systems(Startup, (renderer::setup, ui::configure_style))
        .add_systems(Update, (controls::fullscreen, screenshot_test));
    renderer::install(&mut app);
    frame_capture::install(&mut app, &args);
    app.try_install_imgui(ImguiPlugin::new(
        ImguiPluginConfig::default().with_docking(false),
    ))
    .expect("valid Dear ImGui configuration");
    let pass = app.imgui_primary_pass().expect("primary Dear ImGui pass");
    app.add_imgui_systems(
        &pass,
        (
            pass.system(ui::draw_controls),
            pass.system(controls::fly),
            pass.system(advance_astronomy),
            pass.system(update_weather),
            pass.system(renderer::render),
            pass.system(ui::draw_overlay),
        )
            .chain(),
    )
    .expect("register the ordered UI, controls, astronomy, renderer and HUD");
    app.run()
}

fn advance_astronomy(
    _frame: ImguiFrame<'_>,
    time: Res<Time<Real>>,
    mut state: ResMut<Explorer>,
    surface: Res<WorldSurface>,
    mut astronomy: ResMut<AstronomicalScene>,
) {
    let active = surface.active(state.boundaries_enabled);
    astronomy.advance(time.delta_secs_f64(), &mut state, active);
}

fn update_weather(
    _frame: ImguiFrame<'_>,
    state: Res<Explorer>,
    mut weather: ResMut<WeatherState>,
    mut previous: Local<Option<(String, Option<String>)>>,
) {
    weather.tick(state.observer.feet);
    let changed = previous
        .as_ref()
        .is_none_or(|(status, error)| status != &weather.status || error != &weather.last_error);
    if changed {
        info!("Weather: {}", weather.status);
        if let Some(error) = &weather.last_error {
            warn!("Weather: {error}");
        }
        *previous = Some((weather.status.clone(), weather.last_error.clone()));
    }
}

fn screenshot_test(
    mut commands: Commands,
    mut smoke: ResMut<SmokeTest>,
    time: Res<Time<Real>>,
    rendered: Res<renderer::RenderedEarth>,
    weather: Res<WeatherState>,
) {
    if smoke.output.is_none() {
        return;
    }
    smoke.frames += 1;
    // Allow startup, asset extraction and layout to settle before native capture.
    if smoke.frames >= 120
        && time.elapsed_secs() >= 8.0
        && rendered.presentation_ready
        && (!weather.enabled || weather.active_profile().is_some())
        && !smoke.requested
    {
        smoke.requested = true;
        use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(smoke.output.clone().unwrap()))
            .observe(
                |_: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
                    exit.write(AppExit::Success);
                },
            );
    }
    let live_weather = weather.enabled && !weather.reference_snapshot;
    let preparation_timeout = if live_weather { 240.0 } else { 120.0 };
    let completion_timeout = if live_weather { 300.0 } else { 180.0 };
    if (time.elapsed_secs() >= preparation_timeout && !smoke.requested)
        || time.elapsed_secs() >= completion_timeout
    {
        error!(
            "Screenshot smoke test timed out after {} frames (scene ready: {}; weather: {}; weather error: {:?})",
            smoke.frames, rendered.presentation_ready, weather.status, weather.last_error
        );
        commands.queue(|world: &mut World| {
            world.write_message(AppExit::error());
        });
    }
}
