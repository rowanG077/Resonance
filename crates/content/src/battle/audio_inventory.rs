//! Original audio ownership and routing, without decoded PCM or execution claims.
#[path = "audio_audit.rs"]
mod audit;
use anyhow::{Context, Result, ensure};
pub use audit::{AudioAudit, AudioContext, AudioRequirement, AudioResolution};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioInventory {
    pub version: u32,
    pub banks: BTreeMap<u16, SoundBank>,
    pub party: BTreeMap<u8, NativeVoices>,
    pub enemies: BTreeMap<u8, EnemyAudio>,
    pub streams: Vec<StreamSource>,
    pub music: BTreeMap<u16, MusicSource>,
    pub sources: BTreeMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> AudioInventory {
        AudioInventory {
            version: 1,
            banks: [(
                10,
                SoundBank {
                    path: "files/S/voice.snd".into(),
                    samples: BTreeSet::new(),
                    programs: BTreeMap::new(),
                    sounds: [
                        (
                            502,
                            SoundSource {
                                object: 1,
                                key: 60,
                                volume: 127,
                                pan: 64,
                                programs: BTreeSet::new(),
                                empty: false,
                                unresolved: None,
                                silent_samples: BTreeSet::new(),
                            },
                        ),
                        (
                            503,
                            SoundSource {
                                object: 2,
                                key: 60,
                                volume: 127,
                                pan: 64,
                                programs: BTreeSet::new(),
                                empty: true,
                                unresolved: None,
                                silent_samples: BTreeSet::new(),
                            },
                        ),
                    ]
                    .into(),
                },
            )]
            .into(),
            party: [(
                1,
                NativeVoices::Voiced {
                    base: 1,
                    ids: vec![1, 0x8001, 2],
                },
            )]
            .into(),
            enemies: [(
                36,
                EnemyAudio {
                    voices: NativeVoices::Voiceless,
                    embedded_bank: None,
                },
            )]
            .into(),
            streams: (0..2)
                .map(|id| StreamSource {
                    name: format!("{id}.adx"),
                    sample_rate: 22050,
                    frames: id * 22050,
                    channels: 1,
                    encoding: 3,
                    version: 4,
                    source_ticks: 60,
                    sha256: "0".repeat(64),
                })
                .collect(),
            music: BTreeMap::new(),
            sources: BTreeMap::new(),
        }
    }

    #[test]
    fn request_matrix_distinguishes_voice_namespaces_and_authored_silence() {
        let inventory = fixture();
        for (request, route) in [
            (
                AudioRequest::Voice { id: 1 },
                AudioRoute::Sound { bank: 10, id: 502 },
            ),
            (
                AudioRequest::Voice { id: 0x8001 },
                AudioRoute::Stream { member: 1 },
            ),
            (
                AudioRequest::NativeVoice {
                    actor: AudioActor::Party(1),
                    offset: 1,
                },
                AudioRoute::Stream { member: 1 },
            ),
            (
                AudioRequest::Voice { id: 0 },
                AudioRoute::Silence {
                    reason: SourceSilence::EmptyRequest,
                },
            ),
            (
                AudioRequest::Voice { id: 0x8000 },
                AudioRoute::Silence {
                    reason: SourceSilence::EmptyStream,
                },
            ),
            (
                AudioRequest::Voice { id: 2 },
                AudioRoute::Silence {
                    reason: SourceSilence::EmptyInstrument,
                },
            ),
            (
                AudioRequest::ActorVoice {
                    actor: AudioActor::Enemy(36),
                    id: 0xffff,
                },
                AudioRoute::Silence {
                    reason: SourceSilence::VoicelessActor,
                },
            ),
        ] {
            assert_eq!(inventory.resolve(request).unwrap(), route, "{request:?}");
        }
        for request in [
            AudioRequest::Sound { id: 504 },
            AudioRequest::Voice { id: 0x8002 },
            AudioRequest::NativeVoice {
                actor: AudioActor::Party(1),
                offset: 3,
            },
            AudioRequest::ActorVoice {
                actor: AudioActor::Enemy(37),
                id: 1,
            },
        ] {
            assert!(inventory.resolve(request).is_err(), "{request:?}");
        }
        let mut duplicate = inventory.clone();
        duplicate.banks.insert(11, duplicate.banks[&10].clone());
        assert!(duplicate.resolve(AudioRequest::Sound { id: 502 }).is_err());
        let mut placeholder = inventory.clone();
        placeholder
            .banks
            .get_mut(&10)
            .unwrap()
            .sounds
            .get_mut(&502)
            .unwrap()
            .silent_samples
            .insert(1096);
        assert_eq!(
            placeholder.resolve(AudioRequest::Voice { id: 1 }).unwrap(),
            AudioRoute::Silence {
                reason: SourceSilence::SilentSample
            }
        );
        placeholder
            .banks
            .get_mut(&10)
            .unwrap()
            .sounds
            .get_mut(&502)
            .unwrap()
            .unresolved = Some("missing source pool".into());
        assert!(placeholder.resolve(AudioRequest::Voice { id: 1 }).is_err());
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoundBank {
    pub path: String,
    pub sounds: BTreeMap<u16, SoundSource>,
    pub programs: BTreeMap<u16, ProgramSource>,
    pub samples: BTreeSet<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoundSource {
    pub object: u16,
    pub key: u8,
    pub volume: u8,
    pub pan: u8,
    pub programs: BTreeSet<u16>,
    /// Only empty instrument mappings or an immediate End are classified here.
    pub empty: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unresolved: Option<String>,
    /// Decoded zero-valued samples in a program with no branch or other generator.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub silent_samples: BTreeSet<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramSource {
    pub instructions: u32,
    pub opcodes: BTreeMap<u8, u32>,
    pub samples: BTreeSet<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NativeVoices {
    Voiceless,
    Voiced { base: u16, ids: Vec<u16> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnemyAudio {
    pub voices: NativeVoices,
    pub embedded_bank: Option<EmbeddedBank>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddedBank {
    pub group: u16,
    pub tables_match_standalone: bool,
    pub samples_match_standalone: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamSource {
    pub name: String,
    pub sample_rate: u32,
    pub frames: u32,
    pub channels: u8,
    pub encoding: u8,
    pub version: u8,
    pub source_ticks: u16,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MusicSource {
    pub path: String,
    pub reverb_preset: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum AudioActor {
    Party(u8),
    Enemy(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AudioRequest {
    Sound { id: u16 },
    Voice { id: u16 },
    ActorVoice { actor: AudioActor, id: u16 },
    NativeVoice { actor: AudioActor, offset: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceSilence {
    EmptyRequest,
    VoicelessActor,
    EmptyInstrument,
    EmptyStream,
    SilentSample,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AudioRoute {
    Sound { bank: u16, id: u16 },
    Stream { member: u16 },
    Silence { reason: SourceSilence },
}

impl AudioInventory {
    pub const VERSION: u32 = 1;

    pub fn resolve(&self, request: AudioRequest) -> Result<AudioRoute> {
        let silence = |reason| Ok(AudioRoute::Silence { reason });
        match request {
            AudioRequest::Sound { id: 0 } | AudioRequest::Voice { id: 0 } => {
                silence(SourceSilence::EmptyRequest)
            }
            AudioRequest::NativeVoice { actor, offset } => {
                let voices = self.native_voices(actor)?;
                match voices {
                    NativeVoices::Voiceless => silence(SourceSilence::VoicelessActor),
                    NativeVoices::Voiced { ids, .. } => self.resolve(AudioRequest::Voice {
                        id: *ids
                            .get(usize::from(offset))
                            .context("native voice offset exceeds its family")?,
                    }),
                }
            }
            AudioRequest::ActorVoice { actor, id } => {
                if matches!(self.native_voices(actor)?, NativeVoices::Voiceless) {
                    silence(SourceSilence::VoicelessActor)
                } else {
                    self.resolve(AudioRequest::Voice { id })
                }
            }
            AudioRequest::Voice { id } if id & 0x8000 != 0 => {
                let member = id & 0x7fff;
                let stream = self
                    .streams
                    .get(usize::from(member))
                    .context("voice stream is absent from AFS")?;
                if stream.frames == 0 {
                    silence(SourceSilence::EmptyStream)
                } else {
                    Ok(AudioRoute::Stream { member })
                }
            }
            AudioRequest::Voice { id } => self.resolve(AudioRequest::Sound {
                id: id.checked_add(501).context("actor voice sound overflow")?,
            }),
            AudioRequest::Sound { id } => {
                let mut owners = self
                    .banks
                    .iter()
                    .filter_map(|(&group, bank)| bank.sounds.get(&id).map(|sound| (group, sound)));
                let (bank, sound) = owners
                    .next()
                    .with_context(|| format!("unknown public sound {id}"))?;
                ensure!(
                    owners.next().is_none(),
                    "ambiguous public sound {id}; bank context is required"
                );
                ensure!(
                    sound.unresolved.is_none(),
                    "sound {id} instrument unresolved: {}",
                    sound.unresolved.as_deref().unwrap_or_default()
                );
                if sound.empty {
                    silence(SourceSilence::EmptyInstrument)
                } else if !sound.silent_samples.is_empty() {
                    silence(SourceSilence::SilentSample)
                } else {
                    Ok(AudioRoute::Sound { bank, id })
                }
            }
        }
    }

    pub fn native_voices(&self, actor: AudioActor) -> Result<&NativeVoices> {
        match actor {
            AudioActor::Party(id) => self.party.get(&id),
            AudioActor::Enemy(id) => self.enemies.get(&id).map(|enemy| &enemy.voices),
        }
        .context("unknown audio actor")
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == Self::VERSION
                && self.party.len() == 9
                && self.enemies.len() == 251
                && !self.banks.is_empty()
                && self.streams.len() <= 0x8000,
            "incomplete battle audio inventory"
        );
        for (&group, bank) in &self.banks {
            crate::validate_asset_path(&bank.path)?;
            ensure!(
                self.sources.contains_key(&bank.path),
                "unhashed sound bank {group}"
            );
            for (&id, sound) in &bank.sounds {
                ensure!(
                    sound.key < 128
                        && sound.volume < 128
                        && sound.pan < 128
                        && sound
                            .programs
                            .iter()
                            .all(|id| bank.programs.contains_key(id)),
                    "invalid sound source {id}"
                );
                ensure!(
                    !sound.empty || sound.unresolved.is_none(),
                    "unresolved sound marked silent"
                );
                if sound.unresolved.is_none() {
                    self.resolve(AudioRequest::Sound { id })?;
                }
            }
            for program in bank.programs.values() {
                ensure!(
                    program.instructions > 0
                        && program.opcodes.values().sum::<u32>() == program.instructions,
                    "incomplete instrument instruction inventory"
                );
            }
        }
        for stream in &self.streams {
            ensure!(
                !stream.name.is_empty()
                    && stream.name.len() <= 32
                    && !stream.name.contains(['/', '\\'])
                    && (8000..=96000).contains(&stream.sample_rate)
                    && (1..=2).contains(&stream.channels)
                    && stream.frames <= 32_000_000
                    && stream.sha256.len() == 64,
                "invalid battle ADX source"
            );
        }
        for voices in self
            .party
            .values()
            .chain(self.enemies.values().map(|enemy| &enemy.voices))
        {
            if let NativeVoices::Voiced { base, ids } = voices {
                ensure!(
                    *base > 0 && (1..=128).contains(&ids.len()),
                    "invalid actor voice family"
                );
                for (offset, &id) in ids.iter().enumerate() {
                    ensure!(
                        id & 0x7fff == *base + offset as u16,
                        "native voice family is not contiguous"
                    );
                    // Some standalone family descriptors omit their pool. The
                    // family identity remains source data; routing reports the
                    // instrument gap instead of claiming those voices silent.
                    let unresolved = id & 0x8000 == 0
                        && self.banks.values().any(|bank| {
                            bank.sounds
                                .get(&(id + 501))
                                .is_some_and(|sound| sound.unresolved.is_some())
                        });
                    if !unresolved {
                        self.resolve(AudioRequest::Voice { id })?;
                    }
                }
            }
        }
        for enemy in self.enemies.values() {
            if let Some(bank) = &enemy.embedded_bank {
                ensure!(
                    self.banks.contains_key(&bank.group),
                    "embedded enemy sound bank is missing"
                );
            }
        }
        for source in self.music.values() {
            crate::validate_asset_path(&source.path)?;
            ensure!(
                self.sources.contains_key(&source.path),
                "unhashed battle music"
            );
        }
        for (path, hash) in &self.sources {
            crate::validate_asset_path(path)?;
            ensure!(
                hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()),
                "invalid source digest"
            );
        }
        Ok(())
    }
}
