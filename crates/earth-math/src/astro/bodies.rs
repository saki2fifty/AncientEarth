use super::*;
include!("tdb_coefficients.rs");

/// Geocentric Fairhead/Bretagnon TDB−TT model, ERFA2.0.1 dtdb with u=v=0.
/// The epoch remains split into J2000 and relative seconds.
/// The caller supplies an epoch in the supported1850–2149 interval.
pub fn tdb_minus_tt_seconds(tt_seconds_j2000: f64) -> f64 {
    let t = tt_seconds_j2000 / (365250.0 * SECONDS_PER_DAY);
    let sum = |start: usize, end: usize| {
        FAIRHEAD[start..end]
            .iter()
            .rev()
            .map(|c| c[0] * (c[1] * t + c[2]).sin())
            .sum::<f64>()
    };
    let fairhead =
        ((((sum(784, 787) * t + sum(764, 784)) * t + sum(679, 764)) * t + sum(474, 679)) * t)
            + sum(0, 474);
    fairhead
        + 0.00065e-6 * (6069.776754 * t + 4.021194).sin()
        + 0.00033e-6 * (213.299095 * t + 5.543132).sin()
        - 0.00196e-6 * (6208.294251 * t + 5.696701).sin()
        - 0.00173e-6 * (74.781599 * t + 2.435900).sin()
        + 0.03638e-6 * t * t
}

/// IAU body axes to ICRF, from the unchanged NAIF pck00011.tpc constants.
/// This is a text-PCK lunar orientation, not DE440 binary-PCK libration.
pub fn iau_body_orientation(body: i32, tdb_seconds_j2000: f64) -> Result<Mat3d, AstroError> {
    if !tdb_seconds_j2000.is_finite() || tdb_seconds_j2000.abs() > 6_000_000_000.0 {
        return Err(AstroError::InvalidDate);
    }
    let d = tdb_seconds_j2000 / SECONDS_PER_DAY;
    let t = d / 36525.0;
    let (ra, dec, w) = match body {
        10 => (286.13, 63.87, 84.176 + 14.18440 * d),
        301 => {
            let arguments = [
                (125.045, -1935.5364525000),
                (250.089, -3871.0729050000),
                (260.008, 475_263.332_872_5),
                (176.625, 487_269.629_985),
                (357.529, 35999.0509575000),
                (311.589, 964_468.499_31),
                (134.963, 477_198.869_325),
                (276.617, 12006.3007650000),
                (34.226, 63863.5132425000),
                (15.134, -5806.6093575000),
                (119.743, 131.8406400000),
                (239.961, 6003.1503825000),
                (25.053, 473_327.796_42),
            ];
            let ra_terms = [
                -3.8787, -0.1204, 0.0700, -0.0172, 0.0, 0.0072, 0.0, 0.0, 0.0, -0.0052, 0.0, 0.0,
                0.0043,
            ];
            let dec_terms = [
                1.5419, 0.0239, -0.0278, 0.0068, 0.0, -0.0029, 0.0009, 0.0, 0.0, 0.0008, 0.0, 0.0,
                -0.0009,
            ];
            let w_terms = [
                3.5610, 0.1208, -0.0642, 0.0158, 0.0252, -0.0066, -0.0047, -0.0046, 0.0028, 0.0052,
                0.0040, 0.0019, -0.0044,
            ];
            let mut ra = 269.9949 + 0.0031 * t;
            let mut dec = 66.5392 + 0.0130 * t;
            let mut w = 38.3213 + 13.17635815 * d - 1.4e-12 * d * d;
            for i in 0..13 {
                let angle = (arguments[i].0 + arguments[i].1 * t).to_radians();
                ra += ra_terms[i] * angle.sin();
                dec += dec_terms[i] * angle.cos();
                w += w_terms[i] * angle.sin();
            }
            (ra, dec, w)
        }
        _ => return Err(AstroError::MissingBody(body)),
    };
    Ok(
        Mat3d::rotation_z(w.to_radians().rem_euclid(std::f64::consts::TAU))
            .mul(Mat3d::rotation_x((90.0 - dec).to_radians()))
            .mul(Mat3d::rotation_z((ra + 90.0).to_radians()))
            .transpose(),
    )
}

/// Special-relativistic aberration of an incoming source direction.
/// Solar/planetary gravitational light bending is not included.
pub(super) fn aberration(direction: Vec3d, observer_velocity: Vec3d) -> Vec3d {
    let beta = observer_velocity / LIGHT_SPEED_MPS;
    let inverse_gamma = (1.0 - beta.dot(beta)).sqrt();
    let dot = direction.dot(beta);
    let shifted =
        (direction * inverse_gamma + beta * (1.0 + dot / (1.0 + inverse_gamma))) / (1.0 + dot);
    shifted.normalized().unwrap_or(direction)
}

impl Ephemeris {
    pub fn sample(
        &self,
        instant: AstroInstant,
        eop: Option<&EarthOrientationData>,
        observer_ecef_m: Vec3d,
    ) -> Result<AstronomyFrame, AstroError> {
        self.sample_with_tracking(instant, eop, observer_ecef_m, true)
    }
    /// A nonrotating observer translates with Earth and has zero held
    /// geocentric velocity. Navigation is not a relativistic velocity boost.
    /// Ground eclipse shading retains its independent co-rotating velocity.
    pub fn sample_with_tracking(
        &self,
        instant: AstroInstant,
        eop: Option<&EarthOrientationData>,
        observer_ecef_m: Vec3d,
        follow_earth: bool,
    ) -> Result<AstronomyFrame, AstroError> {
        if !observer_ecef_m.is_finite() || observer_ecef_m.length() > 1.1e9 {
            return Err(AstroError::InvalidObserver);
        }
        let tt = instant.tt_seconds_j2000();
        let tdb = tt + tdb_minus_tt_seconds(tt);
        let eo = eop::resolve(instant, eop);
        let (rotation, era) = orientation::earth_orientation(tt, eo.ut1, eo.xp, eo.yp);
        let earth = self.state(399, tdb)?;
        let observer_icrf = rotation.transpose().transform(observer_ecef_m);
        let observer_position = earth.position_m + observer_icrf;
        // Earth rotation velocity; tiny precession/polar-motion derivatives
        // are omitted. Artificial flight velocity is not a physical boost.
        let angular_velocity = std::f64::consts::TAU * 1.0027378119113546 / SECONDS_PER_DAY;
        let spin_velocity = Vec3d::new(
            -observer_ecef_m.y * angular_velocity,
            observer_ecef_m.x * angular_velocity,
            0.0,
        );
        let corotating_velocity =
            earth.velocity_mps + rotation.transpose().transform(spin_velocity);
        let observer_velocity = if follow_earth {
            corotating_velocity
        } else {
            earth.velocity_mps
        };
        let build = |id, radius| -> Result<CelestialBody, AstroError> {
            let simultaneous = self.state(id, tdb)?.position_m;
            let mut light_time = (simultaneous - observer_position).length() / LIGHT_SPEED_MPS;
            let mut retarded = simultaneous;
            for _ in 0..12 {
                retarded = self.state(id, tdb - light_time)?.position_m;
                let next = (retarded - observer_position).length() / LIGHT_SPEED_MPS;
                if (next - light_time).abs() < 1e-10 {
                    light_time = next;
                    break;
                }
                light_time = next;
            }
            let relative = retarded - observer_position;
            let distance = relative.length();
            let direction = relative.normalized().ok_or(AstroError::InvalidObserver)?;
            let apparent = aberration(direction, observer_velocity) * distance;
            let orientation = iau_body_orientation(id, tdb - light_time)?;
            Ok(CelestialBody {
                center_ecef_m: rotation.transform(simultaneous - earth.position_m),
                apparent_center_ecef_m: observer_ecef_m + rotation.transform(apparent),
                radius_m: radius,
                body_to_ecef: rotation.mul(orientation),
                light_time_seconds: light_time,
            })
        };
        let sun = build(10, SUN_RADIUS_M)?;
        let moon = build(301, MOON_RADIUS_M)?;
        let sun_emission = self.state(10, tdb - sun.light_time_seconds)?;
        let moon_emission_state = self.state(301, tdb - moon.light_time_seconds)?;
        let eclipse = SolarEclipseField {
            anchor_ecef_m: observer_ecef_m,
            sun_relative_ecef_m: rotation.transform(sun_emission.position_m - observer_position),
            moon_relative_ecef_m: rotation
                .transform(moon_emission_state.position_m - observer_position),
            sun_velocity_ecef_mps: rotation.transform(sun_emission.velocity_mps),
            moon_velocity_ecef_mps: rotation.transform(moon_emission_state.velocity_mps),
            observer_velocity_ecef_mps: rotation.transform(corotating_velocity),
            spin_ecef_radians_per_second: Vec3d::new(0.0, 0.0, angular_velocity),
            sun_radius_m: SUN_RADIUS_M,
            moon_radius_m: MOON_RADIUS_M,
        };
        let observer_eclipse = if follow_earth {
            eclipse
        } else {
            SolarEclipseField {
                observer_velocity_ecef_mps: rotation.transform(observer_velocity),
                spin_ecef_radians_per_second: Vec3d::ZERO,
                ..eclipse
            }
        };
        // Illumination at the lunar emission event, with sunlight retarded to
        // that event. This is geometric phase without photometric opposition effects.
        let lunar_epoch = tdb - moon.light_time_seconds;
        let moon_emission = self.state(301, lunar_epoch)?.position_m;
        let mut sun_epoch = lunar_epoch;
        let mut moon_to_sun = Vec3d::ZERO;
        for _ in 0..8 {
            moon_to_sun = self.state(10, sun_epoch)?.position_m - moon_emission;
            sun_epoch = lunar_epoch - moon_to_sun.length() / LIGHT_SPEED_MPS;
        }
        let sun_direction = moon_to_sun
            .normalized()
            .ok_or(AstroError::InvalidObserver)?;
        let observer_direction = (observer_position - moon_emission)
            .normalized()
            .ok_or(AstroError::InvalidObserver)?;
        let illuminated_fraction =
            ((1.0 + sun_direction.dot(observer_direction)) * 0.5).clamp(0.0, 1.0);
        let date = instant.calendar();
        Ok(AstronomyFrame {
            sun,
            moon,
            icrf_to_ecef: rotation,
            earth_rotation_angle_rad: era,
            tt_jd: instant.tt_jd(),
            tdb_jd: JULIAN_DAY_J2000 + tdb / SECONDS_PER_DAY,
            ut1_jd: JULIAN_DAY_J2000 + eo.ut1 / SECONDS_PER_DAY,
            quality: AstroQuality {
                earth_rotation: eo.quality,
                historical_ut1_calendar: date.year < 1960,
                future_leap_seconds_assumed: date.year > 2026,
                outside_nutation_reference_interval: date.year < 1900 || date.year > 2100,
            },
            moon_illuminated_fraction: illuminated_fraction,
            moon_to_sun_direction_ecef: rotation.transform(sun_direction),
            eclipse,
            observer_eclipse,
        })
    }
}
