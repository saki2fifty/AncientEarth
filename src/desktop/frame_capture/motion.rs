//! Opt-in native integration probe. Navigation uses the canonical scene API.
use super::super::Explorer;
use ancient_earth::surface::WorldSurface;
use earth_math::Vec3d;
use serde_json::{Value, json};

#[derive(Clone, Copy)]
enum Stage {
    Lift,
    Traverse,
    Recover,
    Quality,
    Disabled,
    FinalRecovery,
    Complete,
}

pub struct MotionProbe {
    stage: Stage,
    started: f64,
    simulated_stage_seconds: f64,
    initial_origin: Vec3d,
    initial_eye: Vec3d,
    initial_generation: u64,
    requested_generation: u64,
    original_quality: usize,
    ready_since: Option<f64>,
    pending_observed: bool,
    events: Vec<Value>,
}

impl MotionProbe {
    pub fn begin(
        state: &Explorer,
        snapshot: (Vec3d, u64, usize, usize),
        elapsed: f64,
    ) -> Result<Self, &'static str> {
        if !state.grass_enabled || snapshot.2 == 0 {
            return Err("motion QA requires visible grass; use --coast --ground");
        }
        if state.observer.feet.height_m.abs() > 4000.0 {
            return Err("motion QA requires a near-ground preset below 4 km");
        }
        Ok(Self {
            stage: Stage::Lift,
            started: elapsed,
            simulated_stage_seconds: 0.0,
            initial_origin: snapshot.0,
            initial_eye: state.observer.eye_position(),
            initial_generation: snapshot.1,
            requested_generation: snapshot.1,
            original_quality: state.quality,
            ready_since: None,
            pending_observed: false,
            events: Vec::new(),
        })
    }

    /// Called once per application frame, before the next renderer update.
    /// All recorded intervals remain in the enclosing nonstationary capture.
    pub fn step(
        &mut self,
        state: &mut Explorer,
        surface: &WorldSurface,
        snapshot: (Vec3d, u64, usize, usize),
        ready: bool,
        elapsed: f64,
        interval: f64,
    ) -> Result<bool, &'static str> {
        if elapsed - self.started > 180.0 {
            return Err("motion QA did not complete within its 180-second watchdog");
        }
        if snapshot.2 != snapshot.3 {
            return Err("motion QA detected mismatched grass and range-bound counts");
        }
        self.pending_observed |= !ready;
        match self.stage {
            Stage::Lift | Stage::Traverse => {
                let lifting = matches!(self.stage, Stage::Lift);
                let target_seconds = if lifting { 1.0 } else { 6.0 };
                // The production scene API caps each flight interval at 0.25 s.
                // Count precisely the bounded input supplied to that API.
                let step = interval
                    .min(0.25)
                    .min(target_seconds - self.simulated_stage_seconds);
                state.observer.speed_mps = if lifting { 5.0 } else { 20.0 };
                if self.simulated_stage_seconds == 0.0 {
                    state.observer.pitch_deg = 0.0;
                }
                surface.active(state.boundaries_enabled).fly(
                    &mut state.observer,
                    if lifting { 0.0 } else { 1.0 },
                    0.0,
                    if lifting { 1.0 } else { 0.0 },
                    step,
                    false,
                );
                self.simulated_stage_seconds += step;
                if self.simulated_stage_seconds >= target_seconds {
                    self.simulated_stage_seconds = 0.0;
                    self.event(
                        if lifting {
                            "lift_finished"
                        } else {
                            "traverse_finished"
                        },
                        state,
                        snapshot,
                        ready,
                        elapsed,
                    );
                    self.stage = if lifting {
                        Stage::Traverse
                    } else {
                        Stage::Recover
                    };
                }
            }
            Stage::Recover if ready => {
                if snapshot.0 == self.initial_origin
                    || snapshot.1 == self.initial_generation
                    || state.observer.eye_position().distance(self.initial_eye) <= 80.0
                {
                    return Err("motion QA did not exercise an actual origin rebase");
                }
                self.event("movement_recovered", state, snapshot, ready, elapsed);
                self.requested_generation = snapshot.1;
                state.quality = (self.original_quality + 1) % 4;
                self.stage = Stage::Quality;
            }
            Stage::Quality if snapshot.1 != self.requested_generation => {
                self.event(
                    "quality_generation_observed",
                    state,
                    snapshot,
                    ready,
                    elapsed,
                );
                self.requested_generation = snapshot.1;
                state.grass_enabled = false;
                self.stage = Stage::Disabled;
            }
            Stage::Disabled if snapshot.1 != self.requested_generation => {
                if snapshot.2 != 0 || snapshot.3 != 0 {
                    return Err("motion QA detected grass geometry remaining after disable");
                }
                self.event("disabled_geometry_cleared", state, snapshot, ready, elapsed);
                self.requested_generation = snapshot.1;
                state.grass_enabled = true;
                state.quality = self.original_quality;
                self.stage = Stage::FinalRecovery;
            }
            Stage::FinalRecovery if ready && snapshot.1 != self.requested_generation => {
                let since = *self.ready_since.get_or_insert(elapsed);
                if elapsed - since >= 2.0 {
                    if !self.pending_observed || snapshot.2 == 0 {
                        return Err("motion QA did not observe pending work and restored grass");
                    }
                    self.event("final_recovery_stable", state, snapshot, ready, elapsed);
                    self.stage = Stage::Complete;
                }
            }
            Stage::FinalRecovery => self.ready_since = None,
            _ => {}
        }
        Ok(matches!(self.stage, Stage::Complete))
    }

    fn event(
        &mut self,
        label: &str,
        state: &Explorer,
        snapshot: (Vec3d, u64, usize, usize),
        ready: bool,
        elapsed: f64,
    ) {
        let eye = state.observer.eye_position();
        self.events.push(json!({
            "event": label,
            "elapsed_recording_seconds": elapsed - self.started,
            "eye_ecef_metres": [eye.x,eye.y,eye.z],
            "local_origin_ecef_metres": [snapshot.0.x,snapshot.0.y,snapshot.0.z],
            "generation": snapshot.1,
            "blades": snapshot.2,
            "range_bounds": snapshot.3,
            "presentation_ready": ready,
            "quality": state.quality,
            "grass_enabled": state.grass_enabled,
        }));
    }

    pub fn report(&self) -> Value {
        json!({
            "passed": matches!(self.stage, Stage::Complete),
            "pending_work_observed": self.pending_observed,
            "events": self.events,
            "scope": "native movement, rebase, quality/toggle invalidation and recovery; not GPU pixel equality",
        })
    }
}
