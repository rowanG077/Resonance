//! Ordinary encounter entry. The caller supplies the independent battle clock;
//! preparation never advances the retained field's random stream or VM.
use super::model::MotionRole;
use anyhow::{Context, Result, ensure};
use resonance_battle::{Playback, Random};
use resonance_content::{
    animation::Motion,
    battle_profile::{Entry, Profile},
};
use std::collections::BTreeMap;

pub fn initial_playback(
    random: &mut Random,
    entry: &Entry,
    profile: &Profile,
    motions: &BTreeMap<u16, Motion>,
    hp: i32,
    max_hp: i32,
    petrified: bool,
) -> Result<Playback> {
    ensure!(
        max_hp > 0 && hp >= 0 && hp <= max_hp,
        "invalid entry actor health"
    );
    ensure!(
        !profile.traits.secondary_body_entry,
        "secondary body entry is not prepared"
    );
    ensure!(
        entry.replacement_motion_rate.is_finite() && entry.replacement_motion_rate > 0.,
        "invalid entry motion rate"
    );
    let idle = u16::from(if profile.initial_motion_override != 0 {
        profile.initial_motion_override
    } else {
        profile.initial_motion
    });
    let (clip, repeat, randomized) = if hp == 0 {
        (MotionRole::Defeated.id(), false, false)
    } else if petrified {
        (idle, true, false)
    } else if i64::from(hp) * 4 < i64::from(max_hp) {
        (MotionRole::WeakIdle.id(), true, false)
    } else if motions.contains_key(&MotionRole::Entry.id()) {
        (MotionRole::Entry.id(), false, false)
    } else {
        (idle, true, true)
    };
    let duration = motions
        .get(&clip)
        .context("entry motion is not prepared")?
        .duration_frames;
    ensure!(
        duration.is_finite() && duration > 0.,
        "invalid entry motion duration"
    );
    Ok(Playback {
        clip,
        frame: if hp == 0 {
            duration
        } else if randomized {
            f32::from(random.next_u16()) % duration
        } else {
            0.
        },
        rate: if randomized {
            0.5
        } else {
            entry.replacement_motion_rate
        },
        repeat,
    })
}

/// An encounter override takes precedence over the map and world themes.
pub fn music(setup: &resonance_events::battle::Setup, map: u32, world_selection: i32) -> u16 {
    setup.music.unwrap_or({
        if map == 0x20c {
            92
        } else {
            match world_selection {
                1 => 86,
                2 => 92,
                _ => 85,
            }
        }
    })
}

pub(super) fn camera_leader(controls: impl Iterator<Item = resonance_battle::Control>) -> usize {
    controls
        .enumerate()
        .find_map(|(index, control)| (control != resonance_battle::Control::Auto).then_some(index))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_owner_keeps_formation_priority_and_all_auto_fallback() {
        use resonance_battle::Control::{Auto, Manual, SemiAuto};
        for (controls, expected) in [
            ([Manual, SemiAuto, Auto, Manual], 0),
            ([SemiAuto, Manual, Auto, Auto], 0),
            ([Auto, SemiAuto, Manual, Auto], 1),
            ([Auto, Manual, SemiAuto, Auto], 1),
            ([Auto, Auto, SemiAuto, Manual], 2),
            ([Auto, Auto, Auto, Manual], 3),
            ([Auto, Auto, Auto, Auto], 0),
        ] {
            assert_eq!(camera_leader(controls.into_iter()), expected);
        }
        assert_eq!(camera_leader([Auto].into_iter()), 0);
    }
    #[test]
    fn music_selection_preserves_precedence() -> Result<()> {
        let mut setup = resonance_events::battle::Setup {
            route: [0; 5],
            encounter: resonance_events::battle::Encounter::Formation(0),
            arena: 0,
            defeat: resonance_events::battle::DefeatPolicy::GameOver,
            music: None,
        };
        assert_eq!(
            [
                music(&setup, 1, 0),
                music(&setup, 1, 1),
                music(&setup, 1, 2)
            ],
            [85, 86, 92]
        );
        assert_eq!(music(&setup, 0x20c, 1), 92);
        setup.music = Some(12);
        assert_eq!(music(&setup, 0x20c, 2), 12);
        Ok(())
    }
}
