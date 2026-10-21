//! Integration tests for clock/tracker transactions; kinematics are separately
//! verified against explicit matrices in earth-math's reference_frames_review.
use ancient_earth::{navigation::Explorer, simulation::AstronomicalScene};
use earth_math::*;

#[test]
fn unsafe_calendar_change_restores_accepted_pose_epoch_and_display_then_relocks() {
    let scene = TerrainScene::new(vec![]);
    let mut explorer = Explorer {
        observer: Observer::new(Geodetic::new(45.0, 12.0, 10.0).unwrap()),
        ..Explorer::default()
    };
    explorer.observer.roll_deg = 37.0;
    let initial = explorer.observer;
    let mut astronomy = AstronomicalScene::bundled(initial).unwrap();
    astronomy.calendar_input = "2024-04-08 18:40:00".into();
    astronomy.apply_calendar();
    astronomy.reset_tracking(initial, true).unwrap();
    astronomy.sample(initial, true);
    let accepted = astronomy.clock.instant();
    explorer.follow_earth = false;
    astronomy.advance(0.0, &mut explorer, &scene);
    assert!(!explorer.follow_earth);
    assert_eq!(explorer.observer, initial);
    astronomy.calendar_input = "2024-04-09 18:40:00".into();
    astronomy.apply_calendar();
    astronomy.advance(0.0, &mut explorer, &scene);
    assert!(explorer.follow_earth);
    assert!(!astronomy.clock.running);
    assert_eq!(astronomy.clock.instant(), accepted);
    assert_eq!(astronomy.calendar_input, accepted.calendar_string());
    assert_eq!(explorer.observer, initial);
    assert_eq!(astronomy.frame.unwrap().tt_jd, accepted.tt_jd());
    assert!(astronomy.error.as_ref().unwrap().contains("clearance"));
    // Recovery is usable: ordinary Earth-following date changes still work.
    astronomy.step_count = -1;
    astronomy.step_seconds = 60.0;
    astronomy.step();
    astronomy.advance(0.0, &mut explorer, &scene);
    assert_eq!(astronomy.clock.instant(), accepted.shifted(-60.0).unwrap());
    assert_eq!(explorer.observer, initial);
    // A malformed external edit must restore the last accepted pose instead
    // of committing an invalid eye while changing the recovery epoch.
    explorer.observer.roll_deg = f64::NAN;
    astronomy.advance(0.0, &mut explorer, &scene);
    assert_eq!(explorer.observer, initial);
    assert!(explorer.follow_earth);
    assert!(astronomy.frame.is_some());
}

#[test]
fn combined_date_and_pose_commit_reanchors_unlocked_observer() {
    let scene = TerrainScene::new(vec![]);
    let mut explorer = Explorer::default();
    let mut astronomy = AstronomicalScene::bundled(explorer.observer).unwrap();
    explorer.follow_earth = false;
    explorer.crouch_locked = true;
    explorer.update_crouch(false);
    // A generic explicit destination and calendar edit must be committed as a
    // new anchor, not transported from the old epoch as if the observer waited.
    explorer.observer = Observer::new(Geodetic::new(23.0, 47.0, 8_000_000.0).unwrap());
    explorer.observer.heading_deg = 113.0;
    explorer.observer.pitch_deg = -15.0;
    explorer.observer.roll_deg = 31.0;
    explorer.posture.apply(&mut explorer.observer).unwrap();
    astronomy.calendar_input = "2024-01-15 12:00:00".into();
    astronomy.apply_calendar();
    astronomy.reset_tracking(explorer.observer, false).unwrap();
    astronomy.sample(explorer.observer, false);
    assert!(!explorer.follow_earth);
    assert!(explorer.posture.crouched);
    assert_eq!(
        explorer.observer.eye_height_m,
        explorer.posture.effective_height_m()
    );
    let committed = explorer.observer;
    let instant = astronomy.clock.instant();
    let rotation = astronomy.frame.unwrap().icrf_to_ecef;
    astronomy.advance(0.0, &mut explorer, &scene);
    assert_eq!(explorer.observer, committed);
    assert_eq!(astronomy.clock.instant(), instant);
    assert_eq!(astronomy.frame.unwrap().icrf_to_ecef, rotation);
    astronomy.step_count = 1;
    astronomy.step_seconds = 3600.0;
    astronomy.step();
    astronomy.advance(0.0, &mut explorer, &scene);
    let new_rotation = astronomy.frame.unwrap().icrf_to_ecef;
    let held_eye = rotation.transpose().transform(committed.eye_position());
    let transported = new_rotation
        .transpose()
        .transform(explorer.observer.eye_position());
    assert!((transported - held_eye).length() < 5e-8);
    let held_up = rotation.transpose().transform(committed.frame().up);
    assert!(
        (new_rotation
            .transpose()
            .transform(explorer.observer.frame().up)
            - held_up)
            .length()
            < 2e-14
    );
    assert!(!explorer.follow_earth);
    assert!(astronomy.error.is_none());
}
