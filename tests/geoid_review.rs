//! Runtime verification of the actual bundled gzip and Rust native-grid
//! decoder against independent PROJ vgridshift results from the source TIFF.
//! Fixtures are fixed numeric references, not copied from the runtime grid.
use ancient_earth::datum::SeaLevel;
use std::sync::OnceLock;

fn grid() -> &'static SeaLevel {
    static GEOID: OnceLock<SeaLevel> = OnceLock::new();
    GEOID.get_or_init(|| SeaLevel::bundled().expect("bundled global EGM2008 grid"))
}

#[test]
fn bundled_rust_decoder_matches_independent_source_proj_fixtures() {
    let grid = &grid().0;
    assert_eq!(grid.dimensions(), (8640, 4321));
    // PROJ: +proj=vgridshift +grids=us_nga_egm08_25.tif +multiplier=1.
    // The official source is https://cdn.proj.org/us_nga_egm08_25.tif.
    for (latitude, longitude, expected) in [
        (0.0, 0.0, 17.225_084_304_809_57),
        (90.0, 0.0, 14.898_501_396_179_2),
        (-90.0, 0.0, -30.149_969_100_952_15),
        (45.0, -180.0, -6.337_575_912_475_586),
        (45.0, 180.0, -6.337_575_912_475_586),
        (45.0, 179.99, -6.342_778_282_165_483),
        (45.0, -179.99, -6.333_754_158_020_019),
        (29.76328, -95.36327, -28.406_682_277_381_44),
        (29.30135, -94.7977, -27.744_387_136_875_61),
        (35.36, 138.73, 42.238_787_811_279_25),
        (-36.8485, 174.7633, 34.102_596_208_837_88),
    ] {
        let actual = grid.undulation_m(latitude, longitude).unwrap();
        assert!(
            (actual - expected).abs() < 2e-9,
            "lat={latitude}, lon={longitude}: runtime={actual:.15}, PROJ={expected:.15}"
        );
    }
}

#[test]
fn runtime_native_grid_is_continuous_at_seam_and_longitude_independent_at_poles() {
    let grid = &grid().0;
    for longitude in [-180.0, -123.456, 0.0, 47.12, 179.999, 180.0] {
        assert_eq!(
            grid.undulation_m(90.0, longitude).unwrap(),
            14.898_501_396_179_2
        );
        assert_eq!(
            grid.undulation_m(-90.0, longitude).unwrap(),
            -30.149_969_100_952_15
        );
    }
    for latitude in [-89.99, -45.0, 0.0, 45.0, 89.99] {
        assert_eq!(
            grid.undulation_m(latitude, -180.0).unwrap(),
            grid.undulation_m(latitude, 180.0).unwrap()
        );
        let west = grid.undulation_m(latitude, -179.999_999_999).unwrap();
        let east = grid.undulation_m(latitude, 179.999_999_999).unwrap();
        assert!((east - west).abs() < 1e-8);
    }
}
