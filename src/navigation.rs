//! Renderer-independent application navigation state.
use earth_math::{EyePosture, Geodetic, Lens, Observer, Projection, TerrainScene, UnitSystem};

#[cfg_attr(feature = "desktop", derive(bevy::prelude::Resource))]
pub struct Explorer {
    pub observer: Observer,
    pub posture: EyePosture,
    pub normal_lens: Lens,
    pub telescope_lens: Lens,
    pub telescope: bool,
    pub units: UnitSystem,
    pub show_panel: bool,
    pub show_ui: bool,
    pub grass_enabled: bool,
    pub show_licenses: bool,
    pub show_horizontal_line: bool,
    pub show_vertical_line: bool,
    pub guide_column: f64,
    pub follow_earth: bool,
    pub crouch_locked: bool,
    pub latitude: f64,
    pub longitude: f64,
    pub altitude_m: f64,
    pub quality: usize,
    pub status: String,
    pub looking: bool,
    pub boundaries_enabled: bool,
}

impl Default for Explorer {
    fn default() -> Self {
        let observer = earth_math::boundary::default_observer();
        let location = observer.feet;
        Self {
            observer,
            posture: EyePosture::default(),
            normal_lens: Lens::default(),
            telescope_lens: Lens::new(Projection::Rectilinear, 5.0),
            telescope: false,
            units: UnitSystem::Metric,
            show_panel: true,
            show_ui: true,
            grass_enabled: true,
            show_licenses: false,
            show_horizontal_line: false,
            show_vertical_line: false,
            guide_column: 0.5,
            follow_earth: true,
            crouch_locked: false,
            latitude: location.lat_deg,
            longitude: location.lon_deg,
            altitude_m: location.height_m,
            quality: 1,
            status: "Enter WGS84 coordinates or fly to explore the smooth Earth surface.".into(),
            looking: false,
            boundaries_enabled: true,
        }
    }
}

impl Explorer {
    pub fn lens(&self) -> Lens {
        if self.telescope {
            self.telescope_lens
        } else {
            self.normal_lens
        }
    }

    pub fn visit_coast(&mut self, surface: &TerrainScene) {
        match surface.boundary_observer() {
            Ok(mut observer) => {
                if let Err(error) = self.posture.apply(&mut observer) {
                    self.status = error.to_string();
                    return;
                }
                self.observer = observer;
                self.boundaries_enabled = true;
                self.telescope = false;
                self.normal_lens = Lens::default();
                self.latitude = observer.feet.lat_deg;
                self.longitude = observer.feet.lon_deg;
                self.altitude_m = observer.feet.height_m;
                self.status = "Land and water share the smooth WGS84 surface. Boundaries come from water-mask data only.".into();
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    pub fn update_crouch(&mut self, key_held: bool) {
        let previous = self.posture.crouched;
        self.posture.crouched = self.crouch_locked || key_held;
        if let Err(error) = self.posture.apply(&mut self.observer) {
            self.posture.crouched = previous;
            self.status = error.to_string();
        }
    }

    pub fn set_standing_eye_height(&mut self, metres: f64) {
        let previous = self.posture;
        self.posture.set_standing_height(metres);
        if let Err(error) = self.posture.apply(&mut self.observer) {
            self.posture = previous;
            self.status = error.to_string();
        }
    }

    pub fn set_crouching_eye_height(&mut self, metres: f64) {
        let previous = self.posture;
        self.posture.set_crouching_height(metres);
        if let Err(error) = self.posture.apply(&mut self.observer) {
            self.posture = previous;
            self.status = error.to_string();
        }
    }

    pub fn visit_coordinates(&mut self) {
        self.visit_coordinates_on_surface(&TerrainScene::default());
    }

    pub fn visit_coordinates_on_surface(&mut self, surface: &TerrainScene) {
        match Geodetic::new(self.latitude, self.longitude, self.altitude_m) {
            Ok(location) => {
                let mut observer = self.observer;
                match surface
                    .try_teleport(&mut observer, location)
                    .and_then(|()| self.posture.apply(&mut observer).map_err(Into::into))
                {
                    Ok(()) => {
                        self.observer = observer;
                        self.status =
                            "Arrived at the entered latitude, longitude and feet height.".into();
                    }
                    Err(error) => self.status = error.to_string(),
                }
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    pub fn ground_on_surface(&mut self, surface: &TerrainScene) {
        let location = self.observer.feet;
        match surface.place_on_ground(&mut self.observer, location) {
            Ok(()) => {
                self.latitude = self.observer.feet.lat_deg;
                self.longitude = self.observer.feet.lon_deg;
                self.altitude_m = self.observer.feet.height_m;
                self.status = "Standing on the active ground surface.".into();
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    pub fn visit_coordinate_ground(&mut self, surface: &TerrainScene) {
        match Geodetic::new(self.latitude, self.longitude, 0.0) {
            Ok(location) => match surface.place_on_ground(&mut self.observer, location) {
                Ok(()) => {
                    self.altitude_m = self.observer.feet.height_m;
                    self.status = "At the entered latitude/longitude on the ground surface.".into();
                }
                Err(error) => self.status = error.to_string(),
            },
            Err(error) => self.status = error.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejected_coordinate_entry_does_not_move() {
        let mut state = Explorer::default();
        let before = state.observer;
        state.latitude = 91.0;
        state.visit_coordinates();
        assert_eq!(state.observer, before);
        assert!(state.status.contains("Latitude"));
        state.latitude = 0.0;
        state.altitude_m = -1.0;
        state.visit_coordinates();
        assert_eq!(state.observer, before);
        state.altitude_m = earth_math::MAX_HEIGHT_M;
        state.visit_coordinates();
        assert_eq!(state.observer, before);
    }

    #[test]
    fn valid_coordinate_entry_moves_exactly() {
        let mut state = Explorer {
            latitude: -33.865,
            longitude: 151.2094,
            altitude_m: 250.0,
            ..Default::default()
        };
        state.visit_coordinates();
        assert_eq!(
            state.observer.feet,
            Geodetic::new(-33.865, 151.2094, 250.0).unwrap()
        );
    }

    #[test]
    fn units_and_telescope_selection_preserve_physical_observer() {
        let mut state = Explorer::default();
        let before = state.observer;
        let normal = state.normal_lens;
        state.units = UnitSystem::Imperial;
        state.telescope = true;
        assert_eq!(state.lens().projection, Projection::Rectilinear);
        state.telescope = false;
        assert_eq!(state.lens(), normal);
        assert_eq!(state.observer, before);
    }
}
