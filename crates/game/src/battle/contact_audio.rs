//! Prepare contact sound tables and actor voice choices from the encounter files.
use super::{
    model::ModelSource,
    voice::{self, Sound},
};
use anyhow::{Context, Result, ensure};
use resonance_battle::{ContactActorAudio, ContactAudio, ContactVoices, SoundBinding};
use resonance_content::{battle_model, battle_profile, prepared::Files};

pub fn prepare(
    files: &Files,
    actors: &[ModelSource],
    mut resolve: impl FnMut(Sound) -> Result<SoundBinding>,
) -> Result<ContactAudio> {
    let source: battle_profile::Table = files.json(battle_profile::PARTY_PATH)?;
    let mut prepared = Vec::with_capacity(actors.len());
    for &actor in actors {
        let (profile, neutral, colette) = match actor {
            ModelSource::Party(character) => {
                let index = usize::from(character.checked_sub(1).context("zero character ID")?);
                let neutral = *source
                    .contact_sounds
                    .party
                    .get(index)
                    .context("missing neutral contact cue")?;
                (
                    super::profile::party_template(files, character)?,
                    neutral,
                    character == 2,
                )
            }
            ModelSource::Enemy(id) => (
                files
                    .json::<battle_model::Enemy>(&battle_model::enemy_path(id))?
                    .profile,
                54,
                false,
            ),
            _ => anyhow::bail!("contact audio requires actor profiles"),
        };
        ensure!(neutral != 0, "missing neutral contact cue");
        let mut relative = |line| -> Result<Option<SoundBinding>> {
            Ok(voice::relative(files, &[actor], line, &mut resolve)?[0].map(|line| line.sound))
        };
        let voices = ContactVoices {
            hurt: [relative(3)?, relative(4)?],
            low_hp: if profile.flags & 0x10 != 0 {
                relative(5)?
            } else {
                None
            },
            defeat: relative(6)?,
            critical: relative(8)?,
            guard: relative(9)?,
            arte_guard_break: relative(39)?,
            fifth_hit: relative(40)?,
            interrupted_cast: relative(49)?,
            affinity: [relative(46)?, relative(47)?],
            kill: relative(50)?,
            has_alternate_defeat: profile.death_voice != 0,
            alternate_defeat: if profile.death_voice != 0 {
                voice::absolute(files, &[actor], profile.death_voice, &mut resolve)?[0]
                    .map(|line| line.sound)
            } else {
                None
            },
            stunned: if colette {
                voice::absolute(files, &[actor], 224, &mut resolve)?[0].map(|line| line.sound)
            } else {
                None
            },
            stunned_override: colette,
        };
        prepared.push(ContactActorAudio {
            neutral: resolve(Sound::Cue(neutral))?,
            voiced: profile.voice_base != 0,
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
