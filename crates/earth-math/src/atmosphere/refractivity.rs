//! Visible-light phase refractivity of moist air, following the NIST Ciddor
//! calculation (Appendix A21–A41), with a fixed 450 µmol/mol CO₂ composition.
//!
//! Source: <https://emtoolbox.nist.gov/Wavelength/Documentation.asp>.
//! The numerical acceptance domain below includes atmospheric extrapolation;
//! it is deliberately NOT a statement of experimental accuracy throughout it.

use super::AtmosphereError;

// Preserve the constants used by the referenced formulation rather than
// substituting newer gas/molar-mass constants into a fitted model.
const GAS_CONSTANT: f64 = 8.314_472;
const WATER_MOLAR_MASS: f64 = 0.018_015;
const DRY_MOLAR_MASS: f64 = 0.028_963_5 + 1.2011e-8 * (450.0 - 400.0);
const STANDARD_DRY_COMPRESSIBILITY: f64 = 0.999_592_211_5;
const STANDARD_VAPOR_DENSITY: f64 = 0.009_859_38;

/// Ciddor phase index at a vacuum wavelength in nanometres.
///
/// `specific_humidity` is mass of water vapor divided by total moist-air mass
/// (kg/kg), not relative humidity or the dry-air mixing ratio. The conversion
/// to molecular fraction uses the same molar masses as the density equations.
/// No vapor-pressure enhancement is applied again to this mass fraction.
///
/// Numeric support is 100–400 K, 0–120000 Pa, 0–0.1 kg/kg, and 400–700 nm.
/// The cited metrology validation range is much narrower (notably temperature
/// and pressure); the caller must identify extrapolated atmospheric samples.
/// This describes a gas mixture, not droplets, aerosols, dispersion of a pulse,
/// or a measured present-day CO₂ concentration. Zero pressure gives vacuum.
pub fn phase_index(
    temperature_k: f64,
    pressure_pa: f64,
    specific_humidity: f64,
    wavelength_nm: f64,
) -> Result<f64, AtmosphereError> {
    if !temperature_k.is_finite()
        || !(100.0..=400.0).contains(&temperature_k)
        || !pressure_pa.is_finite()
        || !(0.0..=120_000.0).contains(&pressure_pa)
        || !specific_humidity.is_finite()
        || !(0.0..=0.1).contains(&specific_humidity)
        || !wavelength_nm.is_finite()
        || !(400.0..=700.0).contains(&wavelength_nm)
    {
        return Err(AtmosphereError::InvalidSample);
    }
    if pressure_pa == 0.0 {
        return Ok(1.0);
    }

    let water_moles = specific_humidity / WATER_MOLAR_MASS;
    let dry_moles = (1.0 - specific_humidity) / DRY_MOLAR_MASS;
    let vapor_fraction = water_moles / (water_moles + dry_moles);
    let celsius = temperature_k - 273.15;
    let pressure_over_temperature = pressure_pa / temperature_k;
    let vapor_squared = vapor_fraction * vapor_fraction;
    // NIST A37: compressibility of the actual moist mixture.
    let compressibility = 1.0
        - pressure_over_temperature
            * (1.58123e-6 - 2.9331e-8 * celsius
                + 1.1043e-10 * celsius * celsius
                + (5.707e-6 - 2.051e-8 * celsius) * vapor_fraction
                + (1.9898e-4 - 2.376e-6 * celsius) * vapor_squared)
        + pressure_over_temperature
            * pressure_over_temperature
            * (1.83e-11 - 0.765e-8 * vapor_squared);
    if !compressibility.is_finite() || compressibility <= 0.0 {
        return Err(AtmosphereError::InvalidSample);
    }

    let wave_number_squared = (1000.0 / wavelength_nm).powi(2);
    let dry_refractivity = 1e-8
        * (5_792_105.0 / (238.0185 - wave_number_squared)
            + 167_917.0 / (57.362 - wave_number_squared));
    let vapor_refractivity = 1.022e-8
        * (295.235
            + wave_number_squared
                * (2.6422 + wave_number_squared * (-0.03238 + 0.004028 * wave_number_squared)));
    let density_factor = pressure_pa / (compressibility * GAS_CONSTANT * temperature_k);
    let dry_density = (1.0 - vapor_fraction) * DRY_MOLAR_MASS * density_factor;
    let vapor_density = vapor_fraction * WATER_MOLAR_MASS * density_factor;
    let standard_dry_density =
        101_325.0 * DRY_MOLAR_MASS / (STANDARD_DRY_COMPRESSIBILITY * GAS_CONSTANT * 288.15);
    let index = 1.0
        + dry_density / standard_dry_density * dry_refractivity
        + vapor_density / STANDARD_VAPOR_DENSITY * vapor_refractivity;
    if index.is_finite() && index >= 1.0 {
        Ok(index)
    } else {
        Err(AtmosphereError::InvalidSample)
    }
}
