use super::{AutomatedView, Explorer};
use ancient_earth::surface::WorldSurface;
use bevy::{
    input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel},
    prelude::*,
    window::{
        CursorGrabMode, CursorOptions, MonitorSelection, PrimaryWindow, WindowMode, WindowPosition,
        WindowResolution,
    },
};
use dear_imgui_bevy::ImguiFrame;
use dear_imgui_bevy::input::ImguiInputCapture;
use earth_math::{adjust_speed, pixels_to_wheel_lines};

#[derive(Default)]
pub struct WindowedState(Option<(WindowResolution, WindowPosition)>);

fn toggle_window_mode(window: &mut Window, saved: &mut WindowedState) {
    if window.mode == WindowMode::Windowed {
        saved.0 = Some((window.resolution.clone(), window.position));
        window.mode = WindowMode::BorderlessFullscreen(MonitorSelection::Current);
    } else {
        window.mode = WindowMode::Windowed;
        if let Some((resolution, position)) = saved.0.take() {
            window.resolution = resolution;
            window.position = position;
        }
    }
}

pub fn fullscreen(
    keys: Res<ButtonInput<KeyCode>>,
    automated: Res<AutomatedView>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut saved: Local<WindowedState>,
) {
    if automated.0 {
        return;
    }
    if let Ok(mut window) = windows.single_mut()
        && window.focused
        && keys.just_pressed(KeyCode::F5)
    {
        toggle_window_mode(&mut window, &mut saved);
    }
}

fn axis(keys: &ButtonInput<KeyCode>, positive: KeyCode, negative: KeyCode) -> f64 {
    match (keys.pressed(positive), keys.pressed(negative)) {
        (true, false) => 1.0,
        (false, true) => -1.0,
        _ => 0.0,
    }
}

fn keyboard_shortcuts(state: &mut Explorer, keys: &ButtonInput<KeyCode>, enabled: bool) {
    state.update_crouch(enabled && keys.pressed(KeyCode::KeyC));
    if !enabled {
        return;
    }
    if keys.just_pressed(KeyCode::Tab) {
        state.show_panel = !state.show_panel;
    }
    if keys.just_pressed(KeyCode::F2) {
        state.show_ui = !state.show_ui;
    }
    if keys.just_pressed(KeyCode::KeyT) {
        state.telescope = !state.telescope;
    }
    if keys.just_pressed(KeyCode::KeyL) {
        state.show_horizontal_line = !state.show_horizontal_line;
    }
    if state.show_horizontal_line && keys.just_pressed(KeyCode::KeyV) {
        state.show_vertical_line = !state.show_vertical_line;
    }
    if keys.just_pressed(KeyCode::F9) {
        state.follow_earth = !state.follow_earth;
    }
}

#[allow(clippy::too_many_arguments)] // Bevy injects independent system parameters.
pub fn fly(
    _frame: ImguiFrame<'_>,
    automated: Res<AutomatedView>,
    mut state: ResMut<Explorer>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    capture: Res<ImguiInputCapture>,
    surface: Res<WorldSurface>,
    mut motion: MessageReader<MouseMotion>,
    mut wheel: MessageReader<MouseWheel>,
    mut windows: Query<(&Window, &mut CursorOptions), With<PrimaryWindow>>,
) {
    if automated.0 {
        return;
    }
    let Ok((window, mut cursor)) = windows.single_mut() else {
        return;
    };
    let capture = capture.primary();
    if keys.just_pressed(KeyCode::Escape) || !window.focused {
        state.looking = false;
    } else if buttons.just_pressed(MouseButton::Right) && !capture.wants_pointer_input() {
        state.looking = true;
    }
    if !buttons.pressed(MouseButton::Right) {
        state.looking = false;
    }
    cursor.grab_mode = if state.looking {
        CursorGrabMode::Confined
    } else {
        CursorGrabMode::None
    };
    cursor.visible = !state.looking;
    let lens = state.lens();
    for event in motion.read() {
        if state.looking && window.focused {
            state
                .observer
                .apply_look(f64::from(event.delta.x), f64::from(event.delta.y), lens);
        }
    }
    for event in wheel.read() {
        if window.focused && (!capture.wants_pointer_input() || state.looking) {
            let lines = match event.unit {
                MouseScrollUnit::Line => f64::from(event.y),
                MouseScrollUnit::Pixel => pixels_to_wheel_lines(f64::from(event.y)),
            };
            if state.telescope {
                state.telescope_lens.zoom(lines);
            } else {
                state.observer.speed_mps = adjust_speed(state.observer.speed_mps, lines);
            }
        }
    }
    let keyboard_enabled = window.focused && !capture.wants_keyboard_input();
    keyboard_shortcuts(&mut state, &keys, keyboard_enabled);
    if !keyboard_enabled {
        return;
    }
    if state.show_horizontal_line && state.show_vertical_line {
        state.guide_column = earth_math::guides::move_column(
            state.guide_column,
            axis(&keys, KeyCode::ArrowRight, KeyCode::ArrowLeft),
            time.delta_secs_f64(),
        );
    }
    if keys.just_pressed(KeyCode::KeyG) {
        let scene = surface.active(state.boundaries_enabled);
        state.ground_on_surface(scene);
    }
    if keys.just_pressed(KeyCode::KeyO) {
        state.observer.overview();
    }
    let up = if keys.pressed(KeyCode::Space) {
        1.0
    } else {
        axis(&keys, KeyCode::KeyE, KeyCode::KeyQ)
    };
    let scene = surface.active(state.boundaries_enabled);
    scene.fly(
        &mut state.observer,
        axis(&keys, KeyCode::KeyW, KeyCode::KeyS),
        axis(&keys, KeyCode::KeyD, KeyCode::KeyA),
        up,
        time.delta_secs_f64(),
        keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fullscreen_restores_window_size_position_and_dpi() {
        let mut window = Window {
            resolution: WindowResolution::new(1920, 1080).with_scale_factor_override(1.5),
            position: WindowPosition::At(IVec2::new(75, 120)),
            ..default()
        };
        let original = window.resolution.clone();
        let mut saved = WindowedState::default();
        toggle_window_mode(&mut window, &mut saved);
        assert_eq!(
            window.mode,
            WindowMode::BorderlessFullscreen(MonitorSelection::Current)
        );
        window.resolution.set_physical_resolution(3840, 2160);
        toggle_window_mode(&mut window, &mut saved);
        assert_eq!(window.mode, WindowMode::Windowed);
        assert_eq!(window.resolution, original);
        assert_eq!(window.position, WindowPosition::At(IVec2::new(75, 120)));
    }

    #[test]
    fn crouch_releases_on_key_release_or_keyboard_capture_and_keeps_gui_lock() {
        let mut state = Explorer::default();
        let mut keys = ButtonInput::default();
        keys.press(KeyCode::KeyC);
        keyboard_shortcuts(&mut state, &keys, true);
        assert!(state.posture.crouched);
        keyboard_shortcuts(&mut state, &keys, false);
        assert!(!state.posture.crouched);
        keyboard_shortcuts(&mut state, &keys, true);
        keys.release(KeyCode::KeyC);
        keyboard_shortcuts(&mut state, &keys, true);
        assert!(!state.posture.crouched);
        state.crouch_locked = true;
        keyboard_shortcuts(&mut state, &keys, false);
        assert!(state.posture.crouched);
    }

    #[test]
    fn line_toggle_ignores_typing_and_only_toggles_on_press() {
        let mut state = Explorer::default();
        let mut keys = ButtonInput::default();
        keys.press(KeyCode::KeyL);
        keyboard_shortcuts(&mut state, &keys, false);
        assert!(!state.show_horizontal_line);
        keyboard_shortcuts(&mut state, &keys, true);
        assert!(state.show_horizontal_line);
        keys.clear();
        keyboard_shortcuts(&mut state, &keys, true);
        assert!(state.show_horizontal_line);
        keys.release(KeyCode::KeyL);
        keys.press(KeyCode::KeyL);
        keyboard_shortcuts(&mut state, &keys, true);
        assert!(!state.show_horizontal_line);
    }

    #[test]
    fn vertical_guide_requires_horizontal_and_survives_hiding_it() {
        let mut state = Explorer::default();
        let mut keys = ButtonInput::default();
        keys.press(KeyCode::KeyV);
        keyboard_shortcuts(&mut state, &keys, true);
        assert!(!state.show_vertical_line);
        state.show_horizontal_line = true;
        keyboard_shortcuts(&mut state, &keys, false);
        assert!(!state.show_vertical_line);
        keyboard_shortcuts(&mut state, &keys, true);
        assert!(state.show_vertical_line);
        keys.clear();
        keys.press(KeyCode::KeyL);
        keyboard_shortcuts(&mut state, &keys, true);
        assert!(!state.show_horizontal_line);
        assert!(state.show_vertical_line);
        keys.clear();
        keyboard_shortcuts(&mut state, &keys, true);
        assert!(state.show_vertical_line);
    }

    #[test]
    fn clean_view_preserves_independent_guide_controls() {
        let mut state = Explorer::default();
        let mut keys = ButtonInput::default();
        keys.press(KeyCode::F2);
        keyboard_shortcuts(&mut state, &keys, true);
        assert!(!state.show_ui);
        keys.clear();
        keys.press(KeyCode::KeyL);
        keyboard_shortcuts(&mut state, &keys, true);
        assert!(state.show_horizontal_line);
        keys.clear();
        keys.press(KeyCode::KeyV);
        keyboard_shortcuts(&mut state, &keys, true);
        assert!(state.show_vertical_line);
        keys.clear();
        keys.release(KeyCode::KeyV);
        keys.press(KeyCode::KeyV);
        keyboard_shortcuts(&mut state, &keys, true);
        assert!(!state.show_vertical_line);
        keys.clear();
        keys.release(KeyCode::KeyL);
        keys.press(KeyCode::KeyL);
        keyboard_shortcuts(&mut state, &keys, true);
        assert!(!state.show_horizontal_line);
        assert!(!state.show_ui);
        keys.clear();
        keys.release(KeyCode::F2);
        keys.press(KeyCode::F2);
        keyboard_shortcuts(&mut state, &keys, true);
        assert!(state.show_ui);
        assert!(!state.show_horizontal_line);
        assert!(!state.show_vertical_line);
    }

    #[test]
    fn earth_follow_toggle_respects_keyboard_capture_and_key_edges() {
        let mut state = Explorer::default();
        let mut keys = ButtonInput::default();
        keys.press(KeyCode::F9);
        keyboard_shortcuts(&mut state, &keys, false);
        assert!(state.follow_earth);
        keyboard_shortcuts(&mut state, &keys, true);
        assert!(!state.follow_earth);
        keys.clear();
        keyboard_shortcuts(&mut state, &keys, true);
        assert!(!state.follow_earth);
        keys.release(KeyCode::F9);
        keys.press(KeyCode::F9);
        keyboard_shortcuts(&mut state, &keys, true);
        assert!(state.follow_earth);
    }
}
