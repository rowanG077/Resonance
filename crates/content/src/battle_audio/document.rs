//! Complete named audio catalogues are discovered from mappings. Playback
//! environments and exceptional speech clocks are authored separately from PCM.
use super::{Selection, spatial_controls};
use crate::{
    battle_voice::Sound,
    diagnostics::Diagnostics,
    field_audio::{
        MusicReverbs,
        document::{self as audio, Playback, Resolved, Speech},
    },
    library::Snapshot,
    preparation::JSON_LIMIT,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const PATH: &str = "battle/audio.json";
pub const VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub version: u32,
    pub music: String,
    pub effects: String,
    /// Original archive base whose member IDs are also native battle streams.
    pub streams: Option<u32>,
    pub archive_playback: BTreeMap<u32, Playback>,
    pub voice_playback: BTreeMap<u32, Playback>,
    pub music_reverbs: MusicReverbs,
    pub voice_gains: Vec<f32>,
    pub reverbs: [[f32; 5]; 2],
    pub effect_spatial: [f32; 5],
    pub voice_spatial: [f32; 5],
}

pub struct Catalogue {
    pub definition: Definition,
    pub audio: audio::Definition,
    pub resolved: Resolved,
}

impl Catalogue {
    /// Capture only declarations and mappings. No score, sample or spoken PCM
    /// is read until the encounter has collected its complete audio selection.
    pub fn capture(snapshot: &mut Snapshot, cancelled: &impl Fn() -> bool) -> Result<Self> {
        ensure!(!cancelled(), "battle audio catalogue cancelled");
        let definition: Definition = snapshot.json(PATH, JSON_LIMIT)?;
        ensure!(
            definition.version == VERSION,
            "unsupported named battle audio version"
        );
        spatial_controls(definition.effect_spatial)?;
        spatial_controls(definition.voice_spatial)?;
        let music = snapshot
            .mappings(&definition.music)?
            .iter()
            .map(|(id, _)| id)
            .collect();
        let effects = snapshot
            .mappings(&definition.effects)?
            .iter()
            .map(|(id, _)| id)
            .collect();
        let archives: Vec<_> = snapshot
            .mappings(audio::SPEECH)?
            .iter()
            .map(|(id, _)| id)
            .collect();
        if let Some(base) = definition.streams {
            ensure!(
                archives.contains(&base),
                "battle stream archive {base} is not mapped"
            );
        }
        let mut speech = Vec::new();
        for archive in archives {
            ensure!(!cancelled(), "battle audio catalogue cancelled");
            // Skits have their own media-ID scope under a separate mapping.
            let mapping = snapshot.resolve(audio::SPEECH, archive, Some("mappings.json"))?;
            let mut members = BTreeMap::new();
            for (member, _) in snapshot.mappings(&mapping)?.iter() {
                let member =
                    u16::try_from(member).context("speech member exceeds native domain")?;
                let id = archive | u32::from(member);
                members.insert(
                    member,
                    definition
                        .voice_playback
                        .get(&id)
                        .or_else(|| definition.archive_playback.get(&archive))
                        .cloned()
                        .unwrap_or_default(),
                );
            }
            speech.push(Speech { archive, members });
        }
        let audio = audio::Definition {
            version: audio::VERSION,
            music: vec![audio::Selection {
                mapping: definition.music.clone(),
                ids: music,
            }],
            effects: vec![audio::Selection {
                mapping: definition.effects.clone(),
                ids: effects,
            }],
            speech,
            music_reverbs: definition.music_reverbs.clone(),
            voice_gains: definition.voice_gains.clone(),
            reverbs: Some(definition.reverbs),
        };
        let mut resolved = audio.resolve(snapshot)?;
        for archive in definition.archive_playback.keys() {
            ensure!(
                audio.speech.iter().any(|bank| bank.archive == *archive),
                "speech clock references unknown archive {archive}"
            );
        }
        for id in definition.voice_playback.keys() {
            ensure!(
                resolved.voices.contains_key(id),
                "speech clock references unknown voice {id}"
            );
        }
        if let Some(base) = definition.streams {
            let aliases: Vec<_> = resolved
                .voices
                .iter()
                .filter(|(id, _)| **id & 0xffff_0000 == base)
                .map(|(id, voice)| (*id & 0xffff, voice.clone()))
                .collect();
            for (id, voice) in aliases {
                ensure!(id < 0x8000, "battle stream ID exceeds native domain");
                ensure!(
                    resolved.voices.insert(id, voice).is_none(),
                    "duplicate battle stream {id}"
                );
            }
        }
        ensure!(!cancelled(), "battle audio catalogue cancelled");
        Ok(Self {
            definition,
            audio,
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
