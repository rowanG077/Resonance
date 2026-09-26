use super::*;
use resonance_content::{animation::Motion, battle_profile::Entry};

#[test]
fn entry_motion_consumes_discarded_phase_before_replacement_and_fidget() -> Result<()> {
    let entry = Entry {
        replacement_motion_rate: 0.5,
        ..Default::default()
    };
    let mut profile = profile::profile();
    let mut motions = std::collections::BTreeMap::from([
        (
            0,
            Motion {
                duration_frames: 31.,
                tracks: vec![],
            },
        ),
        (
            24,
            Motion {
                duration_frames: 15.,
                tracks: vec![],
            },
        ),
        (
            26,
            Motion {
                duration_frames: 22.,
                tracks: vec![],
            },
        ),
    ]);
    let mut random = battle::entry::Random::from_state(1234);
    let mut expected = random;
    let phase = expected.next_u16() % 31;
    let fidget = expected.next_u16() % 180;
    let initial =
        battle::entry::initial_playback(&mut random, &entry, &profile, &motions, 100, 100)?;
    assert_eq!(
        (
            initial.playback.clip,
            initial.playback.frame,
            initial.playback.rate,
            initial.playback.repeat
        ),
        (24, 0., 0.5, false)
    );
    assert_eq!(initial.fidget_ticks, fidget);
    assert_eq!(random.state(), expected.state());
    motions.remove(&24);
    random = battle::entry::Random::from_state(1234);
    let initial =
        battle::entry::initial_playback(&mut random, &entry, &profile, &motions, 25, 100)?;
    assert_eq!(
        (
            initial.playback.clip,
            initial.playback.frame,
            initial.playback.rate,
            initial.playback.repeat
        ),
        (0, f32::from(phase), 0.5, true)
    );
    random = battle::entry::Random::from_state(1234);
    let initial =
        battle::entry::initial_playback(&mut random, &entry, &profile, &motions, 24, 100)?;
    assert_eq!(
        (
            initial.playback.clip,
            initial.playback.frame,
            initial.playback.rate,
            initial.playback.repeat
        ),
        (26, 0., 0.5, true)
    );
    assert_eq!(random.state(), expected.state());
    profile.initial_motion_override = 26;
    random = battle::entry::Random::from_state(1234);
    let initial =
        battle::entry::initial_playback(&mut random, &entry, &profile, &motions, 100, 100)?;
    assert_eq!(initial.playback.clip, 26);
    motions.insert(
        7,
        Motion {
            duration_frames: 19.75,
            tracks: vec![],
        },
    );
    random = battle::entry::Random::from_state(1234);
    let initial = battle::entry::initial_playback(&mut random, &entry, &profile, &motions, 0, 100)?;
    assert_eq!(
        (
            initial.playback.clip,
            initial.playback.frame,
            initial.playback.rate,
            initial.playback.repeat
        ),
        (7, 19., 0.5, false)
    );
    assert_eq!(random.state(), expected.state());
    profile.body_flags |= 0x400;
    assert!(
        battle::entry::initial_playback(&mut random, &entry, &profile, &motions, 100, 100).is_err()
    );
    Ok(())
}
