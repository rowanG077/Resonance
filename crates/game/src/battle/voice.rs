//! Resolve actor voice requests before activation, using verified source tables.
use super::model::ModelSource;
use anyhow::{Context, Result, ensure};
use resonance_battle::{SoundBinding, VoiceLine};
use resonance_content::{
    battle_model, battle_profile, battle_voice, diagnostics::Diagnostics, prepared::Files,
};

/// The existing audio loader prepares either a cue program or a spoken stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sound {
    Cue(u16),
    Stream(u16),
}

#[derive(Debug, Clone, Copy)]
pub enum Phase {
    Chant,
    /// A spell with the original self-target chant override (39974).
    SelfChant,
    Fallback,
    Release,
}

/// An original absolute request already carries its stream flag (71E78).
/// Profile bases only determine whether the actor can speak; they do not offset
/// this line or apply the relative enemy-line restriction.
pub fn absolute(
    files: &Files,
    actors: &[ModelSource],
    line: u16,
    mut resolve: impl FnMut(Sound) -> Result<SoundBinding>,
) -> Result<Vec<Option<VoiceLine>>> {
    let Some(table) = files.diagnostics().attempt(
        "battle voice table",
        files.json::<battle_voice::Table>(battle_voice::PATH),
    )?
    else {
        return Ok(vec![None; actors.len()]);
    };
    files
        .diagnostics()
        .attempt("battle voice table", table.validate())?;
    actors
        .iter()
        .map(|&actor| {
            files
                .diagnostics()
                .attempt(
                    "battle voice binding",
                    (|| {
                        let profile = match actor {
                            ModelSource::Party(character) => {
                                super::profile::party_template(files, character)?
                            }
                            ModelSource::Enemy(id) => {
                                files
                                    .json::<battle_model::Enemy>(&battle_model::enemy_path(id))?
                                    .profile
                            }
                            _ => anyhow::bail!("voice binding requires an actor profile"),
                        };
                        if profile.voice_base == 0 {
                            return Ok(None);
                        }
                        bind(&table, line, &mut resolve, files.diagnostics())
                    })(),
                )
                .map(Option::flatten)
        })
        .collect()
}

/// Return one binding per combat actor. Unvoiced profiles and restricted enemy
/// lines retain empty slots, so the binding follows the prepared roster.
pub fn relative(
    files: &Files,
    actors: &[ModelSource],
    line: u16,
    mut resolve: impl FnMut(Sound) -> Result<SoundBinding>,
) -> Result<Vec<Option<VoiceLine>>> {
    let Some(table) = files.diagnostics().attempt(
        "battle voice table",
        files.json::<battle_voice::Table>(battle_voice::PATH),
    )?
    else {
        return Ok(vec![None; actors.len()]);
    };
    files
        .diagnostics()
        .attempt("battle voice table", table.validate())?;
    actors
        .iter()
        .map(|&actor| {
            files
                .diagnostics()
                .attempt(
                    "battle voice binding",
                    (|| {
                        let profile = match actor {
                            ModelSource::Party(character) => {
                                super::profile::party_template(files, character)?
                            }
                            ModelSource::Enemy(_) if line >= 10 => return Ok(None),
                            ModelSource::Enemy(id) => {
                                files
                                    .json::<battle_model::Enemy>(&battle_model::enemy_path(id))?
                                    .profile
                            }
                            _ => anyhow::bail!("voice binding requires an actor profile"),
                        };
                        if profile.voice_base == 0 {
                            return Ok(None);
                        }
                        let mut index = profile.voice_base.wrapping_add(u32::from(line)) as u16;
                        let flags = table
                            .streams
                            .get(usize::from(index / 8))
                            .context("voice line exceeds the storage table")?;
                        if flags & (1 << (index % 8)) != 0 {
                            index |= 0x8000;
                        }
                        bind(&table, index, &mut resolve, files.diagnostics())
                    })(),
                )
                .map(Option::flatten)
        })
        .collect()
}

/// Party spell voices selected by native technique ID. Enemy action voices have
/// their own action records and do not participate in a party spell binding.
pub fn technique(
    files: &Files,
    actors: &[ModelSource],
    technique: u16,
    phase: Phase,
    mut resolve: impl FnMut(Sound) -> Result<SoundBinding>,
) -> Result<Vec<Option<VoiceLine>>> {
    let Some(durations) = files.diagnostics().attempt(
        "battle voice table",
        files.json::<battle_voice::Table>(battle_voice::PATH),
    )?
    else {
        return Ok(vec![None; actors.len()]);
    };
    files
        .diagnostics()
        .attempt("battle voice table", durations.validate())?;
    let Some(table) = files.diagnostics().attempt(
        "battle party voice table",
        files.json::<battle_profile::Table>(battle_profile::PARTY_PATH),
    )?
    else {
        return Ok(vec![None; actors.len()]);
    };
    files.diagnostics().attempt(
        "battle party voice table",
        (|| {
            ensure!(
                table.records.len() == 11 && table.voice_sequences.len() == 10,
                "invalid party voice table"
            );
            Ok(())
        })(),
    )?;
    actors
        .iter()
        .map(|&actor| {
            files
                .diagnostics()
                .attempt(
                    "battle voice binding",
                    (|| {
                        let character = match actor {
                            ModelSource::Party(character) => character,
                            ModelSource::Enemy(_) => return Ok(None),
                            _ => anyhow::bail!("voice binding requires an actor profile"),
                        };
                        let index =
                            usize::from(character.checked_sub(1).context("zero character ID")?);
                        let voices = table
                            .voice_sequences
                            .get(index)
                            .context("missing character voices")?;
                        let base = table
                            .records
                            .get(index)
                            .context("missing character voice profile")?
                            .voice_base;
                        if base == 0 {
                            return Ok(None);
                        }
                        // The original scan does not stop on a match; later rows take precedence.
                        let row = voices.iter().rev().find(|row| row.technique == technique);
                        let value = match phase {
                            Phase::Chant | Phase::SelfChant => {
                                let value = if matches!(phase, Phase::SelfChant) && character != 2 {
                                    0
                                } else {
                                    row.map_or(0, |row| row.chant)
                                };
                                if value == 0 {
                                    base.wrapping_add(7) as u16 | 0x8000
                                } else {
                                    value
                                }
                            }
                            Phase::Release => row.map_or(0, |row| row.release),
                            Phase::Fallback => match (character, technique) {
                                (2, 268) => 0x80e8,
                                (2, 269) => 0x80e7,
                                _ => base.wrapping_add(7) as u16,
                            },
                        };
                        bind(&durations, value, &mut resolve, files.diagnostics())
                    })(),
                )
                .map(Option::flatten)
        })
        .collect()
}

fn sound(line: u16) -> Option<Sound> {
    match line {
        0 => None,
        line if line & 0x8000 != 0 => Some(Sound::Stream(line & 0x7fff)),
        line => Some(Sound::Cue(line + 501)),
    }
}

fn bind(
    table: &battle_voice::Table,
    line: u16,
    resolve: &mut impl FnMut(Sound) -> Result<SoundBinding>,
    diagnostics: &Diagnostics,
) -> Result<Option<VoiceLine>> {
    sound(line)
        .map(|sound| {
            let duration = diagnostics
                .attempt(
                    "battle voice duration",
                    table
                        .durations
                        .get(usize::from(line & 0x7fff))
                        .copied()
                        .with_context(|| format!("missing voice duration {}", line & 0x7fff)),
                )?
                .unwrap_or(0);
            Ok(VoiceLine {
                sound: resolve(sound)?,
                duration,
            })
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_duration_is_diagnosed_and_zero_only_in_tolerant_mode() {
        let table = battle_voice::Table {
            source_sha256: "a".repeat(64),
            streams: vec![0],
            durations: vec![10],
        };
        for paranoid in [false, true] {
            let diagnostics = Diagnostics::new(paranoid);
            let result = bind(
                &table,
                7,
                &mut |sound| match sound {
                    Sound::Cue(index) => Ok(SoundBinding { resource: 0, index }),
                    Sound::Stream(index) => Ok(SoundBinding { resource: 1, index }),
                },
                &diagnostics,
            );
            if paranoid {
                assert!(result.is_err());
            } else {
                let voice = result.unwrap().unwrap();
                assert_eq!(voice.duration, 0);
                assert_eq!(
                    voice.sound,
                    SoundBinding {
                        resource: 0,
                        index: 508
                    }
                );
            }
            assert_eq!(diagnostics.entries().len(), 1);
        }
    }
}
