//! Complete named audio catalogues are discovered from mappings. Playback
//! environments and exceptional speech clocks are authored separately from PCM.
use super::{Selection, spatial_controls};
use crate::{
    battle_voice::Sound,
    diagnostics::Diagnostics,
    field_audio::{
        document::{self as audio, Resolved, Speech, Voice},
        mix::{Environment, Mix},
    },
    library::Snapshot,
    preparation::JSON_LIMIT,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const PATH: &str = "battle/audio.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    /// Only voices with a fixed clock need an entry; the rest use WAV rates.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub speech_rates: Vec<Speech>,
    /// Override the common battle acoustics from audio/mix.json.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::field_audio::reverb::optional"
    )]
    pub reverbs: Option<[[f32; 5]; 2]>,
    pub effect_spatial: [f32; 5],
    pub voice_spatial: [f32; 5],
}

pub struct Catalogue {
    pub definition: Definition,
    pub audio: Environment,
    pub resolved: Resolved,
}

impl Catalogue {
    /// Capture only declarations and mappings. No score, sample or spoken PCM
    /// is read until the encounter has collected its complete audio selection.
    pub fn capture(snapshot: &mut Snapshot, cancelled: &impl Fn() -> bool) -> Result<Self> {
        ensure!(!cancelled(), "battle audio catalogue cancelled");
        let definition: Definition = snapshot.json(PATH, JSON_LIMIT)?;
        spatial_controls(definition.effect_spatial)?;
        spatial_controls(definition.voice_spatial)?;
        let music = snapshot
            .mappings(audio::MUSIC)?
            .iter()
            .map(|(id, _)| id)
            .collect();
        let effects = snapshot
            .mappings(audio::EFFECTS)?
            .iter()
            .map(|(id, _)| id)
            .collect();
        let music = audio::resolve_resources(snapshot, audio::MUSIC, &music)?;
        let effects = audio::resolve_resources(snapshot, audio::EFFECTS, &effects)?;
        let mut voices = BTreeMap::new();
        for (id, source) in crate::field_audio::speech::read(snapshot)? {
            ensure!(!cancelled(), "battle audio catalogue cancelled");
            voices.insert(
                id,
                Voice {
                    source: source.resolve()?,
                    sample_rate: None,
                },
            );
        }
        let mut overridden = BTreeSet::new();
        for group in &definition.speech_rates {
            group.validate()?;
            for &id in &group.ids {
                ensure!(overridden.insert(id), "duplicate speech clock {id}");
                voices
                    .get_mut(&id)
                    .with_context(|| format!("speech clock references unknown voice {id}"))?
                    .sample_rate = group.sample_rate;
            }
        }
        let resolved = Resolved {
            music,
            effects,
            voices,
        };
        ensure!(!cancelled(), "battle audio catalogue cancelled");
        let mix = Mix::read(snapshot)?;
        let reverbs = definition.reverbs.unwrap_or(mix.battle_reverbs);
        Ok(Self {
            definition,
            audio: mix.environment(reverbs)?,
            resolved,
        })
    }

    pub fn collect(
        &self,
        selected: &mut Selection,
        diagnostics: &Diagnostics,
        sound: Sound,
    ) -> Result<Option<Sound>> {
        diagnostics.attempt(
            "battle audio binding",
            (|| {
                match sound {
                    Sound::Cue(id) => {
                        ensure!(
                            i16::try_from(id)
                                .is_ok_and(|id| self.resolved.effects.contains_key(&id)),
                            "unprepared battle sound {id}"
                        );
                        selected.sounds.insert(id);
                    }
                    Sound::Stream(id) => {
                        ensure!(
                            self.resolved.voices.contains_key(&u32::from(id)),
                            "unprepared battle stream {id}"
                        );
                        selected.streams.insert(id);
                    }
                }
                Ok(sound)
            })(),
        )
    }

    pub fn select(&self, selected: &Selection, diagnostics: &Diagnostics) -> Result<Resolved> {
        let scores = |kind: &str,
                      ids: &BTreeSet<u16>,
                      source: &BTreeMap<i16, String>|
         -> Result<BTreeMap<i16, String>> {
            let mut result = BTreeMap::new();
            for &id in ids {
                if let Some((id, path)) = diagnostics.attempt(
                    "battle audio binding",
                    (|| {
                        let key = i16::try_from(id)?;
                        let path = source
                            .get(&key)
                            .with_context(|| format!("unprepared battle {kind} {id}"))?;
                        Ok((key, path.clone()))
                    })(),
                )? {
                    result.insert(id, path);
                }
            }
            Ok(result)
        };
        let mut voices = BTreeMap::new();
        for id in selected
            .voices
            .iter()
            .copied()
            .chain(selected.streams.iter().map(|&id| u32::from(id)))
        {
            if let Some(voice) = diagnostics.attempt(
                "battle audio binding",
                self.resolved
                    .voices
                    .get(&id)
                    .cloned()
                    .with_context(|| format!("unprepared battle voice {id}")),
            )? {
                voices.insert(id, voice);
            }
        }
        Ok(Resolved {
            music: scores("music", &selected.music, &self.resolved.music)?,
            effects: scores("sound", &selected.sounds, &self.resolved.effects)?,
            voices,
        })
    }
}
