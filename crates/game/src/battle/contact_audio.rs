//! Prepare contact sound tables and actor voice choices from the encounter files.
use super::{model::ModelSource, party::Character, voice::Resolver};
use anyhow::{Context, Result, ensure};
use resonance_battle::Sound;

#[derive(Debug, Clone, Default)]
pub struct ContactVoices {
    pub hurt: [Option<Sound>; 2],
    pub defeat: Option<Sound>,
    pub critical: Option<Sound>,
    pub guard: Option<Sound>,
    pub interrupted_cast: Option<Sound>,
    pub stunned: Option<Sound>,
}

#[derive(Debug, Clone)]
pub struct ContactActorAudio {
    pub neutral: Option<Sound>,
    pub voices: ContactVoices,
}

#[derive(Debug, Clone)]
pub struct ContactAudio {
    pub actors: Vec<ContactActorAudio>,
    pub elements: [Option<Sound>; 8],
    pub guard: Option<Sound>,
    pub guard_break: Option<Sound>,
    pub overlimit: Option<Sound>,
}

pub fn prepare(
    resolver: &Resolver<'_>,
    actors: &[ModelSource],
    mut resolve: impl FnMut(Sound) -> Result<Option<Sound>>,
) -> Result<ContactAudio> {
    let source = resolver.party_table();
    let mut prepared = Vec::with_capacity(actors.len());
    for &actor in actors {
        let (neutral, colette) = match actor {
            ModelSource::Party(character) => {
                let index = usize::from(character.checked_sub(1).context("zero character ID")?);
                let neutral = *source
                    .contact_sounds
                    .party
                    .get(index)
                    .context("missing neutral contact cue")?;
                (neutral, character == Character::Colette as u8)
            }
            ModelSource::Enemy(_) => (54, false),
            _ => anyhow::bail!("contact audio requires actor profiles"),
        };
        ensure!(neutral != 0, "missing neutral contact cue");
        let mut select = |field: fn(&resonance_content::battle_voice::Voices) -> Option<Sound>| {
            resolver.select(actor, field, &mut resolve)
        };
        let voices = ContactVoices {
            hurt: [select(|v| v.hurt[0])?, select(|v| v.hurt[1])?],
            defeat: select(|v| v.defeat)?,
            critical: select(|v| v.critical)?,
            guard: select(|v| v.guard)?,
            interrupted_cast: select(|v| v.interrupted_cast)?,
            stunned: if colette {
                resolver.absolute(actor, Some(Sound::Cue(725)), &mut resolve)?
            } else {
                None
            },
        };
        prepared.push(ContactActorAudio {
            neutral: resolve(Sound::Cue(neutral))?,
            voices,
        });
    }
    let elements = source.contact_sounds.elements[1..]
        .iter()
        .map(|&cue| {
            ensure!(cue != 0, "missing elemental contact cue");
            resolve(Sound::Cue(u16::from(cue)))
        })
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .unwrap();
    Ok(ContactAudio {
        actors: prepared,
        elements,
        guard: resolve(Sound::Cue(65))?,
        guard_break: resolve(Sound::Cue(46))?,
        overlimit: resolve(Sound::Cue(66))?,
    })
}
