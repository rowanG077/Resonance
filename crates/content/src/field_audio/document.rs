//! Editable audio choices. Original numeric namespaces select named resources;
//! playback settings stay here, separate from the PCM files they describe.
use super::MusicReverbs;
use crate::library::{Snapshot, validate_path};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const VERSION: u32 = 1;
pub const SPEECH: &str = "audio/speech/mappings.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub version: u32,
    pub music: Vec<Selection>,
    pub effects: Vec<Selection>,
    pub speech: Vec<Speech>,
    pub music_reverbs: MusicReverbs,
    pub voice_gains: Vec<f32>,
    /// Initial shared studio. Music selectors may change auxiliary A; auxiliary
    /// B remains shared with effects. Standalone programs can retain their own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reverbs: Option<[[f32; 5]; 2]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub mapping: String,
    pub ids: BTreeSet<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Speech {
    /// Native archive base, retaining the upper half of a scenario voice ID.
    pub archive: u32,
    pub members: BTreeMap<u16, Playback>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Playback {
    /// Omit to use the current WAV's rate. Some original callers intentionally
    /// choose a different clock; retain that choice apart from the source file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_rate: Option<u32>,
}

#[derive(Clone)]
pub struct Voice {
    pub path: String,
    pub sample_rate: Option<u32>,
}

#[derive(Clone)]
pub struct Resolved {
    pub music: BTreeMap<i16, String>,
    pub effects: BTreeMap<i16, String>,
    pub voices: BTreeMap<u32, Voice>,
}

impl Definition {
    pub fn program_reverbs(&self, music: Option<i16>) -> Result<Option<[[f32; 5]; 2]>> {
        let Some(mut parameters) = self.reverbs else {
            return Ok(None);
        };
        if let Some(id) = music
            && let Some(selected) = self.music_reverbs.for_song(id)?
        {
            parameters[0] = selected;
        }
        Ok(Some(parameters))
    }

    pub fn resolve(&self, snapshot: &mut Snapshot) -> Result<Resolved> {
        ensure!(
            self.version == VERSION,
            "unsupported editable field audio version"
        );
        self.music_reverbs.validate()?;
        if let Some(reverbs) = self.reverbs {
            for parameters in reverbs {
                super::MusicReverbs::validate_preset(parameters)?;
            }
        }
        super::validate_voice_gains(&self.voice_gains)?;
        let music = resolve_resources(snapshot, &self.music, "music.json")?;
        let effects = resolve_resources(snapshot, &self.effects, "sound.json")?;
        let mut voices = BTreeMap::new();
        for archive in &self.speech {
            ensure!(
                archive.archive != 0 && archive.archive & 0xffff == 0,
                "invalid speech archive base {}",
                archive.archive
            );
            let mapping = snapshot.resolve(SPEECH, archive.archive, Some("mappings.json"))?;
            for (&member, playback) in &archive.members {
                ensure!(
                    playback
                        .sample_rate
                        .is_none_or(|rate| (8000..=96000).contains(&rate)),
                    "invalid speech playback rate"
                );
                let id = archive.archive | u32::from(member);
                // Mapping shape is structural. Individual media files are
                // admitted by the audio loader under its diagnostic policy.
                let path =
                    snapshot
                        .mappings(&mapping)?
                        .resolve(&mapping, u32::from(member), None)?;
                ensure!(
                    voices
                        .insert(
                            id,
                            Voice {
                                path,
                                sample_rate: playback.sample_rate
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

fn resolve_resources(
    snapshot: &mut Snapshot,
    selections: &[Selection],
    document: &str,
) -> Result<BTreeMap<i16, String>> {
    let mut scores = BTreeMap::new();
    for selection in selections {
        validate_path(&selection.mapping)?;
        for &id in &selection.ids {
            let key = i16::try_from(id).context("audio ID exceeds native cue range")?;
            let path = snapshot.mappings(&selection.mapping)?.resolve(
                &selection.mapping,
                id,
                Some(document),
            )?;
            ensure!(
                scores.insert(key, path).is_none(),
                "ambiguous audio ID {id} selected from multiple banks"
            );
        }
    }
    Ok(scores)
}
