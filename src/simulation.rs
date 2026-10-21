//! Application clock controls and bundled data. All astronomy lives in earth-math.
use earth_math::{
    Observer, TerrainScene,
    astro::{
        AstroError, AstronomyFrame, EarthOrientationData, Ephemeris, ObserverTracking,
        SimulationClock, terrestrial_rotation,
    },
};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg_attr(feature = "desktop", derive(bevy::prelude::Resource))]
pub struct AstronomicalScene {
    pub clock: SimulationClock,
    pub frame: Option<AstronomyFrame>,
    pub error: Option<String>,
    pub calendar_input: String,
    pub step_seconds: f64,
    pub step_count: i32,
    ephemeris: Ephemeris,
    orientation: EarthOrientationData,
    tracking: ObserverTracking,
}

impl AstronomicalScene {
    pub fn bundled(observer: Observer) -> Result<Self, AstroError> {
        let clock = current_clock()?;
        let ephemeris = Ephemeris::from_bytes(include_bytes!("../assets/astronomy/de440s.bsp"))?;
        let orientation = EarthOrientationData::from_bytes(include_bytes!(
            "../assets/astronomy/finals2000A.all"
        ))?;
        let frame =
            ephemeris.sample(clock.instant(), Some(&orientation), observer.eye_position())?;
        let tracking = ObserverTracking::new(observer, clock.instant(), frame.icrf_to_ecef)
            .map_err(|_| AstroError::InvalidObserver)?;
        Ok(Self {
            calendar_input: clock.calendar_string(),
            clock,
            frame: Some(frame),
            error: None,
            step_seconds: 1.0,
            step_count: 1,
            ephemeris,
            orientation,
            tracking,
        })
    }

    pub fn advance(
        &mut self,
        elapsed_seconds: f64,
        explorer: &mut crate::navigation::Explorer,
        surface: &TerrainScene,
    ) {
        if let Err(error) = self.clock.advance(elapsed_seconds) {
            self.error = Some(error.to_string());
        }
        let rotation = terrestrial_rotation(self.clock.instant(), Some(&self.orientation));
        if let Err(error) = self.tracking.update(
            &mut explorer.observer,
            explorer.follow_earth,
            self.clock.instant(),
            rotation,
            surface,
        ) {
            explorer.follow_earth = true;
            self.clock.set_instant(self.tracking.instant());
            self.clock.running = false;
            let rotation = terrestrial_rotation(self.clock.instant(), Some(&self.orientation));
            if self
                .tracking
                .update(
                    &mut explorer.observer,
                    true,
                    self.clock.instant(),
                    rotation,
                    surface,
                )
                .is_err()
            {
                explorer.observer = self.tracking.observer();
                // The saved pose and epoch were already accepted by the tracker.
                let _ =
                    self.tracking
                        .reanchor(explorer.observer, true, self.clock.instant(), rotation);
            }
            self.calendar_input = self.clock.calendar_string();
            self.error = Some(error.to_string());
            explorer.status = error.to_string();
        }
        self.sample(explorer.observer, explorer.follow_earth);
    }

    pub fn sample(&mut self, observer: Observer, follow_earth: bool) {
        match self.ephemeris.sample_with_tracking(
            self.clock.instant(),
            Some(&self.orientation),
            observer.eye_position(),
            follow_earth,
        ) {
            Ok(frame) => self.frame = Some(frame),
            Err(error) => {
                self.frame = None;
                self.clock.running = false;
                self.error = Some(error.to_string());
            }
        }
    }

    /// Establish a new initial pose after command-line setup, before time advances.
    pub fn reset_tracking(&mut self, observer: Observer, follow_earth: bool) -> Result<(), String> {
        self.tracking
            .reanchor(
                observer,
                follow_earth,
                self.clock.instant(),
                terrestrial_rotation(self.clock.instant(), Some(&self.orientation)),
            )
            .map_err(|error| error.to_string())
    }

    pub fn apply_calendar(&mut self) {
        match self.clock.set_calendar(&self.calendar_input) {
            Ok(()) => {
                self.clock.running = false;
                self.error = None;
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    pub fn reset_now(&mut self) {
        match current_clock() {
            Ok(clock) => {
                self.calendar_input = clock.calendar_string();
                self.clock = clock;
                self.error = None;
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    pub fn step(&mut self) {
        self.clock.running = false;
        match self
            .clock
            .step(i64::from(self.step_count), self.step_seconds)
        {
            Ok(()) => {
                self.calendar_input = self.clock.calendar_string();
                self.error = None;
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }
}

fn current_clock() -> Result<SimulationClock, AstroError> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| AstroError::InvalidDate)?
        .as_secs_f64();
    SimulationClock::from_unix_utc(seconds)
}
