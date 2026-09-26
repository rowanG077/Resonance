//! Ordinary encounter entry. The caller supplies the independent battle clock;
//! preparation never advances the retained field's random stream or VM.
use anyhow::{Context, Result, ensure};
use resonance_battle::{ActorId, CameraDefinition, CameraPose, EntryCamera, Playback};
use resonance_content::{
    animation::Motion,
    battle_profile::{Entry, PARTY_PATH, Profile, Table},
    prepared::Files,
};
use std::collections::BTreeMap;

/// A missing legacy save field cannot select a source branch or draw count.
/// This optional presentation feature is skipped as one diagnosed item.
pub(super) fn previous_formation(
    diagnostics: &resonance_content::diagnostics::Diagnostics,
    formation_flags: u8,
    previous: Option<u16>,
) -> Result<Option<u16>> {
    if formation_flags & 0x40 != 0 {
        return Ok(None);
    }
    diagnostics.attempt(
        "battle entry voice history",
        previous.context("previous formation is unknown in this legacy save; entry voice selection is unavailable"),
    )
}

#[derive(Debug, Clone, Copy)]
pub struct Random(resonance_battle::Random);

impl Random {
    /// 5C38 calls 4DEB4(0): raw OSGetTime converted to integer milliseconds,
    /// then masked to 16 bits. This is neither save play-time nor field RNG.
    pub fn from_elapsed_millis(milliseconds: u64) -> Self {
        Self::from_state((milliseconds & 0xffff) as u32)
    }

    /// A captured primary seed/state can include draws made before preparation.
    pub fn from_state(state: u32) -> Self {
        Self(resonance_battle::Random::from_state(state))
    }

    pub fn from_timebase(ticks: u64, bus_clock: u32) -> Result<Self> {
        let divisor = (bus_clock >> 2) / 1000;
        ensure!(divisor != 0, "invalid battle timebase clock");
        Ok(Self::from_elapsed_millis(ticks / u64::from(divisor)))
    }

    pub fn state(self) -> u32 {
        self.0.state()
    }
    pub fn next_u16(&mut self) -> u16 {
        self.0.next()
    }
}

pub struct Initial {
    pub playback: Playback,
    /// Native 52AA8 actor+1ca clock, drawn after primary motion setup.
    pub fidget_ticks: u16,
}

/// Ordinary actors use 52AA8 in party roster order, then enemy roster
/// order. Consume each actor's birth effects between these calls, when present.
/// Missing native clips and unsupported multi-body entry fail before activation.
pub fn initial_playback(
    random: &mut Random,
    entry: &Entry,
    profile: &Profile,
    motions: &BTreeMap<u16, Motion>,
    hp: i32,
    max_hp: i32,
) -> Result<Initial> {
    ensure!(
        max_hp > 0 && hp >= 0 && hp <= max_hp,
        "invalid entry actor health"
    );
    ensure!(
        profile.body_flags & 0x400 == 0,
        "secondary body entry is not prepared"
    );
    ensure!(
        entry.replacement_motion_rate.is_finite() && entry.replacement_motion_rate > 0.,
        "invalid entry motion rate"
    );
    let clip = u16::from(if profile.initial_motion_override != 0 {
        profile.initial_motion_override
    } else {
        profile.initial_motion
    });
    let motion = motions
        .get(&clip)
        .context("initial actor motion is not prepared")?;
    let duration = motion.duration_frames;
    ensure!(
        duration.is_finite() && (1. ..32768.).contains(&duration),
        "invalid native entry motion duration"
    );
    let phase = random.next_u16() % duration as i16 as u16;
    let mut playback = Playback {
        clip,
        frame: f32::from(phase),
        // 52AA8 retains 8006EB68's native half-frame rate (8035B8AC).
        rate: 0.5,
        repeat: true,
    };
    let replacement = if hp == 0 {
        Some((7, false))
    } else if (i64::from(hp) * 100) / i64::from(max_hp) < 25 {
        Some((26, true))
    } else {
        motions.contains_key(&24).then_some((24, false))
    };
    if let Some((clip, repeat)) = replacement {
        ensure!(
            motions.contains_key(&clip),
            "entry replacement motion is not prepared"
        );
        let frame = if hp == 0 {
            let duration = motions[&clip].duration_frames;
            ensure!(
                duration.is_finite() && (0. ..32768.).contains(&duration),
                "invalid KO entry motion duration"
            );
            f32::from(duration as i16)
        } else {
            0.
        };
        playback = Playback {
            clip,
            frame,
            rate: entry.replacement_motion_rate,
            repeat,
        };
    }
    Ok(Initial {
        playback,
        fidget_ticks: random.next_u16() % 180,
    })
}

/// Original main dispatch9 selects a route theme before the map/world defaults.
/// (The partial decompilation labels this block case11.)
/// `world_selection` is persistent script word 4 (VM byte offset 0x50).
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

pub fn camera(
    files: &Files,
    leader: ActorId,
    target: ActorId,
    stage_pitch: f32,
    adaptive: bool,
) -> Result<(CameraDefinition, EntryCamera)> {
    let table: Table = files.json(PARTY_PATH)?;
    let entry = table.entry;
    Ok((
        CameraDefinition {
            leader,
            target,
            stage_pitch,
            adaptive,
            initial: CameraPose {
                eye: entry.bootstrap_eye,
                focus: entry.bootstrap_focus,
                pitch: 0.,
                yaw: entry.initial_yaw,
                radius: entry.radius,
            },
        },
        EntryCamera {
            initial_yaw: entry.initial_yaw,
            radius: entry.radius,
            focus_x: entry.focus_x,
            focus_speed_scale: entry.focus_speed_scale,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_legacy_history_is_diagnosed_without_fabricating_a_selector() -> Result<()> {
        use resonance_content::diagnostics::Diagnostics;
        let tolerant = Diagnostics::default();
        assert_eq!(previous_formation(&tolerant, 0, None)?, None);
        assert_eq!(tolerant.entries().len(), 1);
        let strict = Diagnostics::new(true);
        assert!(previous_formation(&strict, 0, None).is_err());
        let suppressed = Diagnostics::new(true);
        assert_eq!(previous_formation(&suppressed, 0x40, None)?, None);
        assert!(!suppressed.has_errors());
        assert_eq!(previous_formation(&suppressed, 0, Some(0))?, Some(0));
        Ok(())
    }
    #[test]
    fn entry_seed_uses_independent_timebase_and_music_preserves_precedence() -> Result<()> {
        assert_eq!(Random::from_elapsed_millis(0x12345).state(), 0x2345);
        assert_eq!(
            Random::from_timebase(40500 * 0x12345, 162000000)?.state(),
            0x2345
        );
        assert!(Random::from_timebase(1, 3999).is_err());
        let mut random = Random::from_state(0);
        assert_eq!(random.next_u16(), 0x12);
        assert_eq!(random.state(), 0x12d687);
        let mut setup = resonance_events::battle::Setup {
            encounter: 0,
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
