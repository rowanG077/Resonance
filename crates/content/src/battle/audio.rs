//! Battle music and actor voices resolved to the shared audio asset formats.
#[path = "audio_inventory.rs"]
pub mod inventory;
use crate::field_audio::FieldAudio;
use anyhow::{Context, Result, ensure};
pub use inventory::{AudioActor, AudioInventory, NativeVoices};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BattleMusic {
    Sylvarant,
    Tethealla,
    Finale,
    Guardian,
    Victory,
    Defeat,
    /// An authored override may select music outside the ordinary battle set.
    Track(i16),
}

impl BattleMusic {
    pub const ALL: [Self; 6] = [
        Self::Sylvarant,
        Self::Tethealla,
        Self::Finale,
        Self::Guardian,
        Self::Victory,
        Self::Defeat,
    ];

    pub const fn id(self) -> i16 {
        match self {
            Self::Sylvarant => 85,
            Self::Tethealla => 86,
            Self::Finale => 92,
            Self::Guardian => 105,
            Self::Victory => 95,
            Self::Defeat => 96,
            Self::Track(id) => id,
        }
    }

    pub fn from_id(id: i16) -> Option<Self> {
        (id > 0).then(|| {
            Self::ALL
                .into_iter()
                .find(|music| music.id() == id)
                .unwrap_or(Self::Track(id))
        })
    }
}

#[test]
fn music_preserves_named_result_fixtures_and_authored_track_ids() {
    for (json, id) in [
        (r#""sylvarant""#, 85),
        (r#""victory""#, 95),
        (r#""defeat""#, 96),
        (r#""guardian""#, 105),
        (r#"{"track":87}"#, 87),
    ] {
        let music: BattleMusic = serde_json::from_str(json).unwrap();
        assert_eq!(music.id(), id);
        assert_eq!(serde_json::to_string(&music).unwrap(), json);
        assert_eq!(BattleMusic::from_id(id), Some(music));
    }
    assert_eq!(BattleMusic::from_id(0), None);
    assert_eq!(BattleMusic::from_id(-1), None);
}

/// Native menu, result, item and hit reactions; action-stream cues keep their
/// authored numeric IDs in the action catalogue.
#[repr(i16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BattleCue {
    Navigate = 1,
    Confirm = 2,
    Cancel = 3,
    Error = 4,
    CookingSuccess = 6,
    CookingFailure = 7,
    Page = 38,
    SlashHit = 46,
    StrikeHit = 54,
    GuardHit = 65,
    OverlimitHit = 66,
    Landing = 72,
    ItemUsed = 76,
    Overlimit = 77,
    LevelUp = 80,
    Recovery = 104,
    CastStart = 109,
    Stun = 117,
    ExclusiveSpellRelease = 122,
    SpellRelease = 123,
}

impl BattleCue {
    pub const ALL: [Self; 20] = [
        Self::Navigate,
        Self::Confirm,
        Self::Cancel,
        Self::Error,
        Self::CookingSuccess,
        Self::CookingFailure,
        Self::Page,
        Self::SlashHit,
        Self::StrikeHit,
        Self::GuardHit,
        Self::OverlimitHit,
        Self::Landing,
        Self::ItemUsed,
        Self::Overlimit,
        Self::LevelUp,
        Self::Recovery,
        Self::CastStart,
        Self::Stun,
        Self::ExclusiveSpellRelease,
        Self::SpellRelease,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum VoiceCue {
    Sound(i16),
    Stream(u32),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BattleAudio {
    pub version: u32,
    pub audio: FieldAudio,
    /// Keys are actor-action voice IDs, distinct from public sound IDs.
    pub voice_cues: BTreeMap<u16, VoiceCue>,
    pub voice_profiles: VoiceProfiles,
    pub impact_sounds: ImpactSoundTable,
    /// Stereo gains for CRI's 31 discrete pan positions, from left to right.
    pub voice_pan: Vec<[f32; 2]>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VoiceProfiles {
    pub party: BTreeMap<u8, VoiceProfile>,
    pub enemies: BTreeMap<u8, VoiceProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceProfile {
    pub voices: NativeVoices,
    /// Explicit voice ID; zero selects the ordinary death voice.
    pub death_override: u16,
    pub low_hp: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceSilence {
    VoicelessActor,
    EnemyOffset,
    DisabledActor,
    EmptyRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ResolvedVoice {
    Cue(u16),
    Silence(VoiceSilence),
}

/// Relative lines selected once when the ordinary battle entrance is prepared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum OpeningLine {
    Ready = 16,
    ReadyAlternate = 17,
    ReadyThird = 18,
    DangerousEnemy = 19,
    Outnumbered = 20,
    EasierEnemies = 21,
    HarderEnemies = 22,
    RepeatedEncounter = 23,
}
impl OpeningLine {
    pub const ORDINARY: [Self; 3] = [Self::Ready, Self::ReadyAlternate, Self::ReadyThird];
    pub const ALL: [Self; 8] = [
        Self::Ready,
        Self::ReadyAlternate,
        Self::ReadyThird,
        Self::DangerousEnemy,
        Self::Outnumbered,
        Self::EasierEnemies,
        Self::HarderEnemies,
        Self::RepeatedEncounter,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceQueue {
    Primary,
    AfterCurrent,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoicePosition {
    #[default]
    Actor,
    Center,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VoiceFeedback {
    Request {
        voice: ResolvedVoice,
        threshold: u8,
        priority: u8,
        queue: VoiceQueue,
        #[serde(default)]
        position: VoicePosition,
    },
    StopActive {
        priority_ceiling: u8,
    },
}

impl VoiceProfile {
    pub fn relative(&self, actor: AudioActor, offset: u8) -> Result<ResolvedVoice> {
        if matches!(actor, AudioActor::Enemy(_)) && offset >= 10 {
            return Ok(ResolvedVoice::Silence(VoiceSilence::EnemyOffset));
        }
        match &self.voices {
            NativeVoices::Voiceless => Ok(ResolvedVoice::Silence(VoiceSilence::VoicelessActor)),
            NativeVoices::Voiced { ids, .. } => Ok(ResolvedVoice::Cue(
                *ids.get(usize::from(offset))
                    .context("uncooked relative actor voice")?,
            )),
        }
    }

    pub fn explicit(&self, id: u16) -> ResolvedVoice {
        if id == 0 {
            ResolvedVoice::Silence(VoiceSilence::EmptyRequest)
        } else if matches!(self.voices, NativeVoices::Voiceless) {
            ResolvedVoice::Silence(VoiceSilence::VoicelessActor)
        } else {
            ResolvedVoice::Cue(id)
        }
    }

    pub fn cues(&self) -> impl Iterator<Item = u16> + '_ {
        let ids = match &self.voices {
            NativeVoices::Voiceless => &[][..],
            NativeVoices::Voiced { ids, .. } => ids.as_slice(),
        };
        ids.iter()
            .copied()
            .chain((!ids.is_empty() && self.death_override != 0).then_some(self.death_override))
    }
}

impl VoiceProfiles {
    pub fn get(&self, actor: AudioActor) -> Result<&VoiceProfile> {
        match actor {
            AudioActor::Party(id) => self.party.get(&id),
            AudioActor::Enemy(id) => self.enemies.get(&id),
        }
        .context("uncooked actor voice profile")
    }

    pub fn cues(&self) -> impl Iterator<Item = u16> + '_ {
        self.party
            .values()
            .chain(self.enemies.values())
            .flat_map(VoiceProfile::cues)
            .chain(self.party.contains_key(&2).then_some(224))
    }
}

#[test]
fn voice_profiles_distinguish_silence_missing_offsets_and_explicit_ids() {
    let profile = VoiceProfile {
        voices: NativeVoices::Voiced {
            base: 1,
            ids: vec![0x8001, 2],
        },
        death_override: 117,
        low_hp: false,
    };
    assert_eq!(
        profile.relative(AudioActor::Party(1), 0).unwrap(),
        ResolvedVoice::Cue(0x8001)
    );
    assert_eq!(profile.explicit(1), ResolvedVoice::Cue(1));
    assert!(profile.relative(AudioActor::Party(1), 2).is_err());
    assert_eq!(
        profile.relative(AudioActor::Enemy(6), 10).unwrap(),
        ResolvedVoice::Silence(VoiceSilence::EnemyOffset)
    );
    let silent = VoiceProfile {
        voices: NativeVoices::Voiceless,
        ..profile
    };
    assert_eq!(
        silent.explicit(117),
        ResolvedVoice::Silence(VoiceSilence::VoicelessActor)
    );
    assert_eq!(
        silent.relative(AudioActor::Enemy(36), 3).unwrap(),
        ResolvedVoice::Silence(VoiceSilence::VoicelessActor)
    );
    assert!(VoiceProfiles::default().get(AudioActor::Enemy(36)).is_err());
}

/// Default contact sounds; an authored hit sound or guard response takes precedence.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ImpactSoundTable {
    /// Character IDs 1–9, independent of the current party formation.
    pub party: [u16; 9],
    /// Original element selectors 0–10; selector zero falls back to the attacker.
    pub elements: [u16; 11],
}

impl ImpactSoundTable {
    pub fn sounds(&self) -> impl Iterator<Item = u16> + '_ {
        self.party
            .iter()
            .chain(&self.elements)
            .copied()
            .filter(|&id| id != 0)
    }
}

impl BattleAudio {
    pub const VERSION: u32 = 3;

    pub fn validate(&self) -> Result<()> {
        ensure!(self.version == Self::VERSION, "unsupported battle audio");
        self.audio.validate()?;
        ensure!(
            self.voice_pan.len() == 31
                && self
                    .voice_pan
                    .iter()
                    .flatten()
                    .all(|gain| { gain.is_finite() && (0.0..=1.0).contains(gain) }),
            "invalid battle voice pan table"
        );
        for music in BattleMusic::ALL {
            ensure!(
                self.audio.music.contains_key(&music.id()),
                "missing {music:?} music"
            );
        }
        for cue in BattleCue::ALL {
            ensure!(
                self.audio.sounds.contains_key(&(cue as i16)),
                "missing {cue:?} cue"
            );
        }
        for sound in self.impact_sounds.sounds() {
            ensure!(
                i16::try_from(sound).is_ok_and(|id| self.audio.sounds.contains_key(&id)),
                "missing impact sound {sound}"
            );
        }
        for &id in self.voice_cues.keys() {
            self.validate_voice(id)?;
        }
        ensure!(
            self.voice_profiles
                .party
                .keys()
                .all(|id| (1..=9).contains(id))
                && self.voice_profiles.enemies.keys().all(|id| *id < 251),
            "invalid actor voice profile identity"
        );
        for profile in self
            .voice_profiles
            .party
            .values()
            .chain(self.voice_profiles.enemies.values())
        {
            if let NativeVoices::Voiced { base, ids } = &profile.voices {
                ensure!(
                    *base > 0
                        && *base < 0x8000
                        && (1..=128).contains(&ids.len())
                        && ids.iter().enumerate().all(|(offset, id)| {
                            usize::from(id & 0x7fff) == usize::from(*base) + offset
                        }),
                    "invalid native voice family"
                );
            }
        }
        // Prepare every branch, even when this encounter chooses only one line.
        for (&character, profile) in &self.voice_profiles.party {
            for line in OpeningLine::ALL {
                if let ResolvedVoice::Cue(id) =
                    profile.relative(AudioActor::Party(character), line as u8)?
                {
                    self.validate_voice(id)?;
                }
            }
        }
        ensure!(
            self.voice_profiles
                .cues()
                .all(|id| self.voice_cues.contains_key(&id)),
            "incomplete actor voice profile dependencies"
        );
        Ok(())
    }

    /// Resolve an authored actor voice through its declared stream or sound cue.
    /// The high bit selects an AFS stream; unflagged IDs use sound ID + 501.
    pub fn validate_voice(&self, id: u16) -> Result<()> {
        ensure!(id != 0, "zero is an empty actor voice request");
        let cue = self
            .voice_cues
            .get(&id)
            .with_context(|| format!("missing actor voice cue {id}"))?;
        match *cue {
            VoiceCue::Sound(sound) => ensure!(
                id < 0x8000
                    && i32::from(sound) == i32::from(id) + 501
                    && self.audio.sounds.contains_key(&sound),
                "missing or invalid actor sound voice {id}"
            ),
            VoiceCue::Stream(stream) => ensure!(
                id & 0x8000 != 0
                    && stream == u32::from(id & 0x7fff)
                    && self.audio.voices.contains_key(&stream),
                "missing or invalid actor stream voice {id}"
            ),
        }
        Ok(())
    }

    pub fn voice_profile(&self, actor: AudioActor) -> Result<&VoiceProfile> {
        self.voice_profiles.get(actor)
    }

    pub fn actor_voices(&self, character: u8) -> Result<&[u16]> {
        match &self.voice_profile(AudioActor::Party(character))?.voices {
            NativeVoices::Voiced { ids, .. } => Ok(ids),
            NativeVoices::Voiceless => anyhow::bail!("actor has no voice family"),
        }
    }
}

#[cfg(test)]
mod voice_dependency_tests {
    use super::*;
    use crate::field_audio::{Asset, Voice};

    fn audio() -> BattleAudio {
        BattleAudio {
            version: BattleAudio::VERSION,
            audio: FieldAudio {
                version: FieldAudio::VERSION,
                music: Default::default(),
                sounds: BTreeMap::from([(
                    2046,
                    Asset {
                        path: "audio/voice-sound.json".into(),
                        sha256: "0".repeat(64),
                    },
                )]),
                voices: BTreeMap::from([(
                    1558,
                    Voice {
                        asset: Asset {
                            path: "audio/battle-voices/00000616.wav".into(),
                            sha256: "0".repeat(64),
                        },
                        frames: 35653,
                        sample_rate: 22050,
                        source_sample_rate: 22050,
                        channels: 1,
                        source_name: "btl_44_08_04_a.adx".into(),
                        source_sha256: "0".repeat(64),
                    },
                )]),
                voice_gains: vec![],
                recipe: serde_json::Value::Null,
            },
            voice_cues: BTreeMap::from([
                (34326, VoiceCue::Stream(1558)),
                (1545, VoiceCue::Sound(2046)),
            ]),
            voice_profiles: Default::default(),
            impact_sounds: ImpactSoundTable {
                party: [0; 9],
                elements: [0; 11],
            },
            voice_pan: vec![],
        }
    }

    #[test]
    fn recovery_voice_8616_uses_stream_0616_and_unflagged_voices_use_sound_ids() {
        // Companion 104/action 3 requests 0x8616; its AFS index is 0x0616.
        let audio = audio();
        assert!(!audio.audio.voices.contains_key(&34326));
        assert!(!audio.audio.voices.contains_key(&1545));
        audio.validate_voice(34326).unwrap();
        audio.validate_voice(1545).unwrap();
    }

    #[test]
    fn voice_dependencies_reject_absent_cues_missing_assets_and_wrong_namespaces() {
        let valid = audio();
        for id in [0, 1558, 2046] {
            assert!(valid.validate_voice(id).is_err());
        }
        let mut missing_cue = valid.clone();
        missing_cue.voice_cues.remove(&34326);
        assert!(missing_cue.validate_voice(34326).is_err());
        let mut missing_stream = valid.clone();
        let stream = missing_stream.audio.voices.remove(&1558).unwrap();
        // An accidental resource under the raw authored ID must not satisfy it.
        missing_stream.audio.voices.insert(34326, stream);
        assert!(missing_stream.validate_voice(34326).is_err());
        let mut missing_sound = valid.clone();
        missing_sound.audio.sounds.remove(&2046);
        assert!(missing_sound.validate_voice(1545).is_err());
        for (id, cue) in [
            (34326, VoiceCue::Stream(34326)),
            (34326, VoiceCue::Sound(2046)),
            (1545, VoiceCue::Stream(1558)),
            (1545, VoiceCue::Sound(1545)),
        ] {
            let mut wrong = valid.clone();
            wrong.voice_cues.insert(id, cue);
            assert!(wrong.validate_voice(id).is_err());
        }
    }
}
