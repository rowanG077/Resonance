use super::*;
use resonance_content::{animation::Motion, battle_profile::Entry};
use std::collections::BTreeMap;

fn motion(duration_frames: f32) -> Motion {
    Motion {
        duration_frames,
        tracks: vec![],
    }
}

#[test]
fn entry_chooses_health_pose_or_an_idle_phase() -> Result<()> {
    let entry = Entry {
        replacement_motion_rate: 0.5,
        ..Default::default()
    };
    let mut profile = profile::profile();
    let mut motions = BTreeMap::from([
        (0, motion(31.)),
        (7, motion(19.75)),
        (24, motion(15.)),
        (26, motion(22.)),
    ]);
    for (hp, petrified, clip, frame, repeat) in [
        (100, false, 24, 0., false),
        (24, false, 26, 0., true),
        (0, false, 7, 19.75, false),
        (1, true, 0, 0., true),
        (0, true, 7, 19.75, false),
    ] {
        let initial = battle::entry::initial_playback(
            &mut resonance_battle::Random::new(1234),
            &entry,
            &profile,
            &motions,
            hp,
            100,
            petrified,
        )?;
        assert_eq!(
            (initial.clip, initial.frame, initial.repeat),
            (clip, frame, repeat)
        );
        assert_eq!(initial.rate, 0.5);
    }
    motions.remove(&24);
    for (override_clip, expected_clip, duration) in [(0, 0, 31.), (26, 26, 22.)] {
        profile.initial_motion_override = override_clip;
        let initial = battle::entry::initial_playback(
            &mut resonance_battle::Random::new(1234),
            &entry,
            &profile,
            &motions,
            100,
            100,
            false,
        )?;
        assert_eq!(initial.clip, expected_clip);
        assert!(initial.repeat && (0. ..duration).contains(&initial.frame));
    }
    motions.remove(&26);
    assert!(
        battle::entry::initial_playback(
            &mut resonance_battle::Random::new(1),
            &entry,
            &profile,
            &motions,
            100,
            100,
            false
        )
        .is_err()
    );
    Ok(())
}
