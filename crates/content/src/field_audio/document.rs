//! Audio selections live beside their field/world. IDs use the library's fixed
//! mappings; speech groups share an optional playback rate, never archive ownership.
use super::mix::{Environment, Mix};
use crate::library::Snapshot;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const MUSIC: &str = "audio/music/mappings.json";
pub const EFFECTS: &str = "audio/mappings.json";
pub const SPEECH: &str = "audio/speech/mappings.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub music: BTreeSet<u32>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub effects: BTreeSet<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub speech: Vec<Speech>,
    /// Override the common field acoustics from audio/mix.json for this place.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::field_audio::reverb::optional"
    )]
    pub reverbs: Option<[[f32; 5]; 2]>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Speech {
    pub ids: BTreeSet<u32>,
    /// Omit to use the current WAV's rate. A fixed rate deliberately preserves
    /// a caller's different clock, independently of the replacement recording.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_rate: Option<u32>,
}

impl Speech {
    pub fn validate(&self) -> Result<()> {
        ensure!(!self.ids.is_empty(), "empty speech selection");
        ensure!(
            self.sample_rate
                .is_none_or(|rate| (8000..=96000).contains(&rate)),
            "invalid speech playback rate"
        );
        Ok(())
    }
}

#[derive(Clone)]
pub struct Voice {
    pub source: super::speech::Source,
    pub sample_rate: Option<u32>,
}

#[derive(Clone)]
pub struct Resolved {
    pub music: BTreeMap<i16, String>,
    pub effects: BTreeMap<i16, String>,
    pub voices: BTreeMap<u32, Voice>,
}

impl Definition {
    pub fn is_empty(&self) -> bool {
        self.music.is_empty()
            && self.effects.is_empty()
            && self.speech.is_empty()
            && self.reverbs.is_none()
    }

    pub fn environment(&self, snapshot: &mut Snapshot) -> Result<Environment> {
        let mix = Mix::read(snapshot)?;
        let reverbs = self.reverbs.unwrap_or(mix.field_reverbs);
        mix.environment(reverbs)
    }

    pub fn resolve(&self, snapshot: &mut Snapshot) -> Result<Resolved> {
        let music = resolve_resources(snapshot, MUSIC, &self.music)?;
        let effects = resolve_resources(snapshot, EFFECTS, &self.effects)?;
        let mut voices = BTreeMap::new();
        let speech = if self.speech.is_empty() {
            BTreeMap::new()
        } else {
            super::speech::read(snapshot)?
        };
        for group in &self.speech {
            group.validate()?;
            for &id in &group.ids {
                let source = speech
                    .get(&id)
                    .with_context(|| format!("unmapped speech {id}"))?
                    .resolve()?;
                ensure!(
                    voices
                        .insert(
                            id,
                            Voice {
                                source,
                                sample_rate: group.sample_rate
                            }
                        )
                        .is_none(),
                    "duplicate speech binding {id}"
                );
            }
        }
        Ok(Resolved {
            music,
            effects,
            voices,
        })
    }
}

pub(crate) fn resolve_resources(
    snapshot: &mut Snapshot,
    mapping: &str,
    ids: &BTreeSet<u32>,
) -> Result<BTreeMap<i16, String>> {
    let mut scores = BTreeMap::new();
    for &id in ids {
        let key = i16::try_from(id).context("audio ID exceeds cue range")?;
        let path = snapshot.mappings(mapping)?.resolve(mapping, id, None)?;
        scores.insert(key, path);
    }
    Ok(scores)
}
