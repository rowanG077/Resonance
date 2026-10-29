//! Character-owned settings and shared battle policies in one editable document.
use crate::{
    battle_action::{ActionPose, TargetPolicy},
    battle_profile as profile,
    battle_voice::{Sound, TimedVoice},
    library::Mappings,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Party {
    pub normal_attacks: String,
    pub martial_attacks: String,
    pub characters: BTreeMap<String, Character>,
    pub technique_strategies: BTreeMap<TechniqueStrategy, Limits>,
    pub entry: profile::Entry,
    pub death_voice_pairs: Vec<[String; 2]>,
    pub elemental_contact_sounds: ContactSounds,
    pub lethal_rescue_names: RescueNames,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Character {
    pub profile: profile::Profile,
    pub body_collider: profile::StandingCollider,
    pub default_strategy: Strategy,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chant_sequence: Vec<(u16, ActionPose)>,
    pub casting_voices: CastingVoices,
    pub contact_sound: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overlimit_voice: Option<Sound>,
    pub item_use_offset: [f32; 3],
}

/// Names follow the selectable strategy choices. Their numeric values are only
/// used at the existing save/menu and prepared-gameplay boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[repr(u8)]
pub enum TechniqueStrategy {
    AtOnce = 1,
    KeepReserve,
    Retain,
    Save,
    DoNotUse,
    Moderation,
    Heal,
    Support,
}

impl TechniqueStrategy {
    const ALL: [Self; 8] = [
        Self::AtOnce,
        Self::KeepReserve,
        Self::Retain,
        Self::Save,
        Self::DoNotUse,
        Self::Moderation,
        Self::Heal,
        Self::Support,
    ];

    fn from_id(id: u8) -> Result<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|value| *value as u8 == id)
            .context("invalid technique strategy")
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[repr(u8)]
pub enum PositionStrategy {
    Frontlines = 1,
    DontPursue,
    HoldPosition,
    LongRangeSkills,
    LongRangeMagic,
    SkillsAndMagic,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Strategy {
    pub target: TargetPolicy,
    pub techniques: TechniqueStrategy,
    pub position: PositionStrategy,
}

impl Strategy {
    fn from_ids([target, techniques, position]: [u8; 3]) -> Result<Self> {
        ensure!(
            (1..=8).contains(&target),
            "invalid party targeting strategy"
        );
        Ok(Self {
            target: target.try_into()?,
            techniques: TechniqueStrategy::from_id(techniques)?,
            position: match position {
                1 => PositionStrategy::Frontlines,
                2 => PositionStrategy::DontPursue,
                3 => PositionStrategy::HoldPosition,
                4 => PositionStrategy::LongRangeSkills,
                5 => PositionStrategy::LongRangeMagic,
                6 => PositionStrategy::SkillsAndMagic,
                _ => anyhow::bail!("invalid party position strategy"),
            },
        })
    }

    fn ids(self) -> Result<[u8; 3]> {
        let target = match self.target {
            TargetPolicy::Nearest => 1,
            TargetPolicy::Farthest => 2,
            TargetPolicy::Leader => 3,
            TargetPolicy::Spread => 4,
            TargetPolicy::Flying => 5,
            TargetPolicy::Casting => 6,
            TargetPolicy::LowestHp => 7,
            TargetPolicy::Protect => 8,
            _ => anyhow::bail!("invalid party targeting strategy"),
        };
        Ok([target, self.techniques as u8, self.position as u8])
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub tp_reserve_percent: u8,
    pub healing_threshold_percent: u8,
}

/// Eight element-specific contact cues; neutral contacts belong to each character.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContactSounds {
    pub water: u8,
    pub wind: u8,
    pub fire: u8,
    pub earth: u8,
    pub lightning: u8,
    pub ice: u8,
    pub light: u8,
    pub darkness: u8,
}

impl From<[u8; 8]> for ContactSounds {
    fn from([water, wind, fire, earth, lightning, ice, light, darkness]: [u8; 8]) -> Self {
        Self {
            water,
            wind,
            fire,
            earth,
            lightning,
            ice,
            light,
            darkness,
        }
    }
}

impl From<ContactSounds> for [u8; 8] {
    fn from(v: ContactSounds) -> Self {
        [
            v.water,
            v.wind,
            v.fire,
            v.earth,
            v.lightning,
            v.ice,
            v.light,
            v.darkness,
        ]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RescueNames {
    pub angel_tear: String,
    pub revive: String,
    pub resurrect: String,
    pub resurrection_ring: String,
    pub reverse_doll: String,
}

impl From<[String; 5]> for RescueNames {
    fn from(
        [
            angel_tear,
            revive,
            resurrect,
            resurrection_ring,
            reverse_doll,
        ]: [String; 5],
    ) -> Self {
        Self {
            angel_tear,
            revive,
            resurrect,
            resurrection_ring,
            reverse_doll,
        }
    }
}

impl From<RescueNames> for [String; 5] {
    fn from(v: RescueNames) -> Self {
        [
            v.angel_tear,
            v.revive,
            v.resurrect,
            v.resurrection_ring,
            v.reverse_doll,
        ]
    }
}

/// One fallback belongs to the caster. Only techniques with a different
/// fallback declare an override; no recursive includes or patch language.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CastingVoices {
    pub fallback: TimedVoice,
    pub default: VoiceSequence,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub techniques: BTreeMap<u16, VoiceSequence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceSequence {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chant: Option<Sound>,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub chant_ticks: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub self_chant: Option<Sound>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release: Option<Sound>,
    /// An explicit {"sound":null,"ticks":0} disables the shared fallback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<TimedVoice>,
}

impl VoiceSequence {
    fn from_sequence(v: profile::VoiceSequence, shared: TimedVoice) -> Self {
        let fallback = TimedVoice {
            sound: v.fallback,
            ticks: v.fallback_ticks,
        };
        Self {
            chant: v.chant,
            chant_ticks: v.chant_ticks,
            self_chant: v.self_chant,
            release: v.release,
            fallback: (fallback != shared).then_some(fallback),
        }
    }

    fn prepare(self, shared: TimedVoice) -> profile::VoiceSequence {
        let fallback = self.fallback.unwrap_or(shared);
        profile::VoiceSequence {
            chant: self.chant,
            chant_ticks: self.chant_ticks,
            self_chant: self.self_chant,
            release: self.release,
            fallback: fallback.sound,
            fallback_ticks: fallback.ticks,
        }
    }
}

impl From<profile::VoicePolicy> for CastingVoices {
    fn from(v: profile::VoicePolicy) -> Self {
        let fallback = TimedVoice {
            sound: v.default.fallback,
            ticks: v.default.fallback_ticks,
        };
        Self {
            fallback,
            default: VoiceSequence::from_sequence(v.default, fallback),
            techniques: v
                .techniques
                .into_iter()
                .map(|(id, sequence)| (id, VoiceSequence::from_sequence(sequence, fallback)))
                .collect(),
        }
    }
}

impl From<CastingVoices> for profile::VoicePolicy {
    fn from(v: CastingVoices) -> Self {
        Self {
            default: v.default.prepare(v.fallback),
            techniques: v
                .techniques
                .into_iter()
                .map(|(id, sequence)| (id, sequence.prepare(v.fallback)))
                .collect(),
        }
    }
}

fn names(mappings: &Mappings) -> Result<[&str; 9]> {
    ensure!(
        mappings.iter().map(|(id, _)| id).eq(1..=9),
        "party rules require character mappings 1 through 9"
    );
    let names = std::array::from_fn(|i| mappings.target(i as u32 + 1).unwrap());
    ensure!(
        names
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            == 9,
        "party rule names must identify distinct characters"
    );
    Ok(names)
}

impl Party {
    pub fn from_table(
        value: profile::Table,
        normal_attacks: String,
        martial_attacks: String,
        mappings: &Mappings,
    ) -> Result<Self> {
        let names = names(mappings)?;
        ensure!(
            value.records.len() == 9 && value.voice_sequences.len() == 9,
            "party rules require nine profiles and casting policies"
        );
        ensure!(
            value.chant_sequences.keys().all(|id| (1..=9).contains(id)),
            "chant sequence refers to an unknown character"
        );
        let mut characters = BTreeMap::new();
        for (i, (profile, casting)) in value
            .records
            .into_iter()
            .zip(value.voice_sequences)
            .enumerate()
        {
            let row = Character {
                profile,
                body_collider: value.body_colliders[i],
                default_strategy: Strategy::from_ids(value.default_strategy[i])?,
                chant_sequence: value
                    .chant_sequences
                    .get(&(i as u8 + 1))
                    .cloned()
                    .unwrap_or_default(),
                casting_voices: casting.into(),
                contact_sound: value.contact_sounds.party[i],
                overlimit_voice: value.overlimit_voices[i],
                item_use_offset: value.item_use_offsets[i],
            };
            ensure!(
                characters.insert(names[i].to_owned(), row).is_none(),
                "duplicate character name"
            );
        }
        let name = |id: u8| -> Result<String> { Ok(mappings.target(u32::from(id))?.to_owned()) };
        Ok(Self {
            normal_attacks,
            martial_attacks,
            characters,
            technique_strategies: TechniqueStrategy::ALL
                .into_iter()
                .map(|strategy| {
                    let i = strategy as usize;
                    (
                        strategy,
                        Limits {
                            tp_reserve_percent: value.companion_policy.tp_limits[i],
                            healing_threshold_percent: value.companion_policy.healing_limits[i],
                        },
                    )
                })
                .collect(),
            entry: value.entry,
            death_voice_pairs: value
                .death_voice_pairs
                .into_iter()
                .map(|[victim, recipient]| Ok([name(victim)?, name(recipient)?]))
                .collect::<Result<_>>()?,
            elemental_contact_sounds: value.contact_sounds.elements.into(),
            lethal_rescue_names: value.lethal_rescue_names.into(),
        })
    }

    pub fn prepare(
        self,
        mappings: &Mappings,
        normal_attacks: Vec<[crate::battle_normal::Attack; 7]>,
        martial_attacks: crate::battle_martial::Catalogue,
    ) -> Result<profile::Table> {
        let names = names(mappings)?;
        let rows = names
            .map(|name| {
                self.characters
                    .get(name)
                    .with_context(|| format!("missing party rules for {name}"))
            })
            .into_iter()
            .collect::<Result<Vec<_>>>()?;
        let mut companion_policy = profile::CompanionPolicy::default();
        for choice in TechniqueStrategy::ALL {
            let limits = self
                .technique_strategies
                .get(&choice)
                .context("missing technique strategy")?;
            ensure!(
                limits.tp_reserve_percent <= 100 && limits.healing_threshold_percent <= 100,
                "invalid companion strategy percentage"
            );
            companion_policy.tp_limits[choice as usize] = limits.tp_reserve_percent;
            companion_policy.healing_limits[choice as usize] = limits.healing_threshold_percent;
        }
        let id = |name: &str| -> Result<u8> {
            names
                .iter()
                .position(|entry| *entry == name)
                .map(|i| i as u8 + 1)
                .with_context(|| format!("unmapped death-voice character {name}"))
        };
        let table = profile::Table {
            normal_attacks,
            martial_attacks,
            body_colliders: std::array::from_fn(|i| rows[i].body_collider),
            default_strategy: rows
                .iter()
                .map(|row| row.default_strategy.ids())
                .collect::<Result<Vec<_>>>()?
                .try_into()
                .unwrap(),
            companion_policy,
            entry: self.entry,
            chant_sequences: rows
                .iter()
                .enumerate()
                .filter(|(_, row)| !row.chant_sequence.is_empty())
                .map(|(i, row)| (i as u8 + 1, row.chant_sequence.clone()))
                .collect(),
            voice_sequences: rows
                .iter()
                .map(|row| row.casting_voices.clone().into())
                .collect(),
            death_voice_pairs: self
                .death_voice_pairs
                .into_iter()
                .map(|[victim, recipient]| Ok([id(&victim)?, id(&recipient)?]))
                .collect::<Result<_>>()?,
            contact_sounds: profile::ContactSounds {
                party: std::array::from_fn(|i| rows[i].contact_sound),
                elements: self.elemental_contact_sounds.into(),
            },
            overlimit_voices: std::array::from_fn(|i| rows[i].overlimit_voice),
            item_use_offsets: std::array::from_fn(|i| rows[i].item_use_offset),
            records: rows.iter().map(|row| row.profile.clone()).collect(),
            lethal_rescue_names: self.lethal_rescue_names.into(),
        };
        table.validate()?;
        Ok(table)
    }
}
