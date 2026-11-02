//! Named actor speech and explicit audio references.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sound {
    Cue(u16),
    Stream(u16),
}

/// Choose one line when its action executes; an absent line is intentional silence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Choice {
    pub sounds: [Option<Sound>; 2],
    pub first_chance: u8,
}

impl Choice {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(self.first_chance <= 100, "voice probability exceeds 100");
        Ok(())
    }

    /// `roll` is in 0..100.
    pub fn select(&self, roll: u8) -> Option<Sound> {
        self.sounds[usize::from(roll >= self.first_chance)]
    }
}

/// Authored scene time is independent of the decoded audio clip's length.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimedVoice {
    pub sound: Option<Sound>,
    pub ticks: u16,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Voices {
    pub hurt: [Option<Sound>; 2],
    pub defeat: Option<Sound>,
    pub critical: Option<Sound>,
    pub guard: Option<Sound>,
    pub ally_defeated: [Option<Sound>; 2],
    pub unison_start: Option<Sound>,
    pub technique_command: Option<Sound>,
    pub item_use: Option<Sound>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acquisition: Option<Sound>,
    pub entry: [Option<Sound>; 3],
    pub major_enemy: Option<Sound>,
    pub outnumbered: Option<Sound>,
    pub weaker_enemies: Option<Sound>,
    pub stronger_enemies: Option<Sound>,
    pub repeated_encounter: Option<Sound>,
    /// Two variations for each ordinary celebration pose, in pose order.
    pub victory: Vec<[Sound; 2]>,
    pub interrupted_cast: Option<Sound>,
    pub escape_request: Option<Sound>,
    pub escape_success: Option<Sound>,
    pub escape_cancel: Option<Sound>,
    pub taunt: Option<Sound>,
    pub contact_recovery: Option<Sound>,
}

impl Voices {
    /// All declared choices, before an encounter selects speakers or variations.
    pub fn sounds(&self) -> impl Iterator<Item = Sound> + '_ {
        let Self {
            hurt,
            defeat,
            critical,
            guard,
            ally_defeated,
            unison_start,
            technique_command,
            item_use,
            acquisition,
            entry,
            major_enemy,
            outnumbered,
            weaker_enemies,
            stronger_enemies,
            repeated_encounter,
            victory,
            interrupted_cast,
            escape_request,
            escape_success,
            escape_cancel,
            taunt,
            contact_recovery,
        } = self;
        [
            *defeat,
            *critical,
            *guard,
            *unison_start,
            *technique_command,
            *item_use,
            *acquisition,
            *major_enemy,
            *outnumbered,
            *weaker_enemies,
            *stronger_enemies,
            *repeated_encounter,
            *interrupted_cast,
            *escape_request,
            *escape_success,
            *escape_cancel,
            *taunt,
            *contact_recovery,
        ]
        .into_iter()
        .flatten()
        .chain(
            hurt.iter()
                .chain(ally_defeated)
                .chain(entry)
                .copied()
                .flatten(),
        )
        .chain(victory.iter().flatten().copied())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declared_choices_keep_recorded_and_streamed_audio_distinct() {
        let voices = Voices {
            hurt: [Some(Sound::Cue(1)), Some(Sound::Stream(1))],
            defeat: Some(Sound::Cue(2)),
            entry: [None, Some(Sound::Stream(3)), Some(Sound::Cue(4))],
            victory: vec![[Sound::Cue(5), Sound::Stream(6)]],
            ..Default::default()
        };
        let sounds: std::collections::BTreeSet<_> = voices.sounds().collect();
        assert_eq!(
            sounds,
            [
                Sound::Cue(1),
                Sound::Cue(2),
                Sound::Cue(4),
                Sound::Cue(5),
                Sound::Stream(1),
                Sound::Stream(3),
                Sound::Stream(6)
            ]
            .into_iter()
            .collect()
        );
    }
}
