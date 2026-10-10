//! Resolve actor voice requests before activation, using named actor sounds.
use super::model::ModelSource;
use anyhow::{Context, Result};
use resonance_content::{battle_profile, battle_voice, diagnostics::Diagnostics, prepared::Files};
use std::collections::BTreeMap;

pub use resonance_content::battle_voice::Sound;

#[derive(Debug, Clone, Copy)]
pub enum Phase {
    Chant,
    SelfChant,
    Fallback,
    Release,
}

/// Select applicable entry lines once; activation only chooses a live speaker and variation.
pub(crate) fn entry_lines(
    voices: &battle_voice::Voices,
    repeated: bool,
    major_enemy: bool,
    party_count: usize,
    enemy_count: usize,
    level_difference: i32,
) -> &[Option<Sound>] {
    let line = if repeated {
        &voices.repeated_encounter
    } else if major_enemy {
        &voices.major_enemy
    } else if enemy_count >= (party_count + 2).min(5) {
        &voices.outnumbered
    } else if level_difference <= -3 {
        &voices.stronger_enemies
    } else if level_difference >= 3 {
        &voices.weaker_enemies
    } else {
        return &voices.entry;
    };
    std::slice::from_ref(line)
}

/// Voice requests use the encounter's selected profiles.
pub struct Resolver<'a> {
    party: &'a battle_profile::Table,
    enemies: BTreeMap<u8, &'a battle_profile::Profile>,
    diagnostics: Diagnostics,
}

impl<'a> Resolver<'a> {
    pub fn new(
        files: &Files,
        party: &'a battle_profile::Table,
        enemies: impl IntoIterator<Item = (u8, &'a battle_profile::Profile)>,
    ) -> Self {
        Self {
            party,
            enemies: enemies.into_iter().collect(),
            diagnostics: files.diagnostics().clone(),
        }
    }

    pub fn party_table(&self) -> &battle_profile::Table {
        self.party
    }

    pub fn profile(&self, actor: ModelSource) -> Result<&battle_profile::Profile> {
        match actor {
            ModelSource::Party(character) => self
                .party
                .records
                .get(usize::from(
                    character.checked_sub(1).context("zero character ID")?,
                ))
                .context("missing character voice profile"),
            ModelSource::Enemy(id) => self
                .enemies
                .get(&id)
                .copied()
                .context("missing enemy voice profile"),
            _ => anyhow::bail!("voice binding requires an actor profile"),
        }
    }

    pub fn absolute(
        &self,
        actor: ModelSource,
        line: Option<Sound>,
        resolve: impl FnMut(Sound) -> Result<Option<Sound>>,
    ) -> Result<Option<Sound>> {
        self.select(actor, |_| line, resolve)
    }

    /// An unvoiced actor or absent choice produces no request.
    pub fn select(
        &self,
        actor: ModelSource,
        select: impl FnOnce(&battle_voice::Voices) -> Option<Sound>,
        resolve: impl FnMut(Sound) -> Result<Option<Sound>>,
    ) -> Result<Option<Sound>> {
        self.bind(
            || Ok(self.profile(actor)?.voices.as_ref().and_then(select)),
            resolve,
        )
    }

    /// Enemy actions carry their own voices; these bindings select party spells.
    pub fn technique(
        &self,
        actor: ModelSource,
        technique: u16,
        phase: Phase,
        resolve: impl FnMut(Sound) -> Result<Option<Sound>>,
    ) -> Result<Option<Sound>> {
        self.bind(
            || {
                let character = match actor {
                    ModelSource::Party(character) => character,
                    ModelSource::Enemy(_) => return Ok(None),
                    _ => anyhow::bail!("voice binding requires an actor profile"),
                };
                if self.profile(actor)?.voices.is_none() {
                    return Ok(None);
                }
                let policy = self
                    .party
                    .voice_sequences
                    .get(usize::from(character - 1))
                    .context("missing character voice policy")?;
                let selected = policy.techniques.get(&technique).unwrap_or(&policy.default);
                Ok(match phase {
                    Phase::Chant => selected.chant,
                    Phase::SelfChant => selected.self_chant,
                    Phase::Release => selected.release,
                    Phase::Fallback => selected.fallback,
                })
            },
            resolve,
        )
    }

    fn bind(
        &self,
        select: impl FnOnce() -> Result<Option<Sound>>,
        mut resolve: impl FnMut(Sound) -> Result<Option<Sound>>,
    ) -> Result<Option<Sound>> {
        let Some(sound) = self
            .diagnostics
            .attempt("battle voice selection", select())?
            .flatten()
        else {
            return Ok(None);
        };
        resolve(sound)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_voices_prioritize_repeated_major_numerous_and_strength() {
        let voices = battle_voice::Voices {
            repeated_encounter: Some(Sound::Cue(1)),
            major_enemy: Some(Sound::Cue(2)),
            outnumbered: Some(Sound::Cue(3)),
            stronger_enemies: Some(Sound::Cue(4)),
            weaker_enemies: Some(Sound::Cue(5)),
            entry: [Some(Sound::Cue(6)); 3],
            ..Default::default()
        };
        for (repeated, major, enemies, difference, expected) in [
            (true, true, 5, -3, &voices.repeated_encounter),
            (false, true, 5, -3, &voices.major_enemy),
            (false, false, 5, -3, &voices.outnumbered),
            (false, false, 1, -100, &voices.stronger_enemies),
            (false, false, 1, 100, &voices.weaker_enemies),
        ] {
            assert_eq!(
                entry_lines(&voices, repeated, major, 3, enemies, difference),
                std::slice::from_ref(expected)
            );
        }
        assert_eq!(entry_lines(&voices, false, false, 3, 1, 0), &voices.entry);
    }
}
