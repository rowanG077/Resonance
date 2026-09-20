//! Authored combat actions. All times are fixed combat ticks, not render frames.
pub mod acid_rain;
pub mod air_thrust;
pub mod aqua_edge;
mod enemy_spell;
pub use enemy_spell::{DirectSpell, EnemySpell, EnemySpellBinding};
pub mod absolute;
pub mod earth_bite;
pub mod earth_field;
pub mod fire_field;
pub mod freeze_lancer;
pub mod ground_pulse;
pub mod ground_summon;
pub mod ice_tornado;
pub mod icicle;
pub mod lance;
pub mod lightning;
pub mod meteor_storm;
pub mod nurse;
pub mod orb;
pub mod prism;
pub mod ray;
pub mod spiral_flare;
pub mod spread;
pub mod stone_blast;
pub mod summon;
pub mod thunder_arrow;
pub mod water;
pub mod wind_blade;
pub mod wind_field;
use super::effects::{EffectBank, EffectId};
pub use air_thrust::AirThrustRecipe;
use anyhow::{Result, ensure};
pub use aqua_edge::AquaEdgeRecipe;
pub use fire_field::{FireFieldPulse, FireFieldRecipe};
use serde::{Deserialize, Serialize};
pub use spread::SpreadRecipe;
use std::collections::BTreeSet;
pub use wind_blade::WindBladeRecipe;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BattleActions {
    pub party: Vec<PartyActions>,
    pub enemies: Vec<EnemyActions>,
    pub projectiles: Vec<ProjectileMotion>,
    pub techniques: Vec<TechniqueAction>,
    pub chains: Option<super::chains::MartialChains>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TechniqueAction {
    pub technique: u16,
    pub native_id: u16,
    pub properties: TechniqueProperties,
    pub program: TechniqueProgram,
}

impl TechniqueAction {
    /// Native entry dispatch shared by source admission and battle execution.
    /// This selects an implementation phase, independently of model costumes.
    pub fn martial_entry_variant(&self, character: u8) -> Option<u8> {
        if !(1..=9).contains(&character)
            || !matches!(self.program, TechniqueProgram::Martial { .. })
        {
            return None;
        }
        Some(match self.native_id {
            34 => character - 1,
            1 | 2 | 4 | 11 | 12 => match character {
                1 => 0,
                9 => 2,
                _ => 1,
            },
            10 if matches!(character, 1 | 6 | 9) => match character {
                1 => 0,
                6 => 1,
                _ => 2,
            },
            81..=87 => u8::from(character == 9),
            6 if character == 1 => 0,
            20 | 22 => 0,
            37 | 39 | 40..=44 if character == 2 => 0,
            63..=71 if character == 5 => 0,
            3
            | 5
            | 7..=9
            | 13
            | 14
            | 15
            | 16
            | 17..=19
            | 21
            | 23..=28
            | 35
            | 36
            | 38
            | 45
            | 46
            | 48
            | 62
            | 78
            | 95..=101
            | 103..=105
            | 108
            | 109
            | 111..=118
            | 120
            | 124..=126
            | 133
            | 137 => 0,
            _ => return None,
        })
    }

    /// Every phase this source-listed owner can select, including dynamic entry.
    /// Character ownership still comes from the original per-character arte list.
    pub fn martial_phases(&self, character: u8) -> Result<impl Iterator<Item = &TechniquePhase>> {
        let selected = self
            .martial_entry_variant(character)
            .ok_or_else(|| anyhow::anyhow!("unsupported martial technique dispatch"))?;
        let TechniqueProgram::Martial { variants } = &self.program else {
            unreachable!()
        };
        let base = variants
            .iter()
            .find(|phase| phase.variant == selected)
            .ok_or_else(|| anyhow::anyhow!("missing selected martial phase"))?;
        let hammer = self.native_id == 40;
        if hammer {
            ensure!(
                (0..3).all(
                    |variant| variants.iter().any(|phase| phase.variant == variant
                        && matches!(
                            phase.callback,
                            Some(MartialCallback::HammerVolley {
                                pattern: HammerPattern::Single,
                                ..
                            })
                        ))
                ),
                "missing dynamic single-hammer phase"
            );
        }
        let dynamic = !matches!(self.native_id, 4 | 40)
            && !matches!(
                base.callback,
                Some(MartialCallback::Beast { .. } | MartialCallback::MagicGuard { .. })
            );
        Ok(variants.iter().filter(move |phase| {
            phase.variant == selected
                || hammer && phase.variant < 3
                || dynamic
                    && phase
                        .callback
                        .and_then(MartialCallback::elemental_entry)
                        .is_some_and(|entry| entry.character == character)
        }))
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TechniqueProperties {
    pub menu_target: TechniqueMenuTarget,
    pub magic: bool,
    /// Eligible for casting-time reductions, independently of its spell category.
    pub cast_time_reducible: bool,
    pub ranged: bool,
    pub damage: bool,
    pub hp_recovery: bool,
    pub condition_change: bool,
    pub incapacitated_target: bool,
    pub physical_condition_recovery: bool,
    pub magical_condition_recovery: bool,
    pub utility: bool,
    pub target_condition_mask: u64,
    pub target_preference: TechniqueTargetPreference,
    pub tactic: TechniqueTactic,
    pub recovery_ticks: u16,
    pub approach: TechniqueApproach,
    pub chain: TechniqueChain,
}

/// Command menus distinguish the selected recipient from the attack-line target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TechniqueMenuTarget {
    Ally,
    Enemy,
    User,
    Unavailable,
}

/// Automatic single/group searches have overlapping eligibility. This is a
/// tactical preference, not the spell's actual target or area implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TechniqueTargetPreference {
    Individual,
    Either,
    Group,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TechniqueTactic {
    Any,
    Weakened,
    Guarding,
    Special,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TechniqueChain {
    None,
    First,
    Second,
    Third,
    Finisher,
    Ground,
    Launch,
    Aerial,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TechniqueApproach {
    pub maximum: f32,
    pub ai_minimum: f32,
    /// Close-range artes shorten their reach with the relevant Kratos/Zelos weapon class.
    pub short_weapon_penalty: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TechniqueProgram {
    Martial {
        variants: Vec<TechniquePhase>,
    },
    FireBall {
        tp: u8,
        cast_time_adjustment: i16,
        voices: CastingVoices,
        casting: AnimationProgram,
        cast_commands: Vec<TimedCommand>,
        loop_cast_commands: bool,
        cast_pulse: u8,
        release: AnimationCommand,
        lifetime: u16,
        rule: HitRule,
        effect: u8,
        height_scale: f32,
        height_offset: f32,
        emissions: Vec<SpellProjectile>,
    },
    Lightning {
        casters: Vec<CastRecipe>,
        recipe: lightning::LightningRecipe,
    },
    Icicle {
        casters: Vec<CastRecipe>,
        recipe: icicle::IcicleRecipe,
    },
    StoneBlast {
        casters: Vec<CastRecipe>,
        recipe: stone_blast::StoneBlastRecipe,
    },
    WindBlade {
        casters: Vec<CastRecipe>,
        recipe: WindBladeRecipe,
    },
    AirThrust {
        casters: Vec<CastRecipe>,
        resume: AnimationCommand,
        recipe: AirThrustRecipe,
    },
    WindField {
        casters: Vec<CastRecipe>,
        resume: AnimationCommand,
        recipe: wind_field::WindFieldRecipe,
    },
    AquaEdge {
        casters: Vec<CastRecipe>,
        recipe: AquaEdgeRecipe,
    },
    Water {
        casters: Vec<CastRecipe>,
        resume: AnimationCommand,
        recipe: water::WaterRecipe,
    },
    Spread {
        casters: Vec<CastRecipe>,
        resume: AnimationCommand,
        recipe: SpreadRecipe,
    },
    IceTornado {
        casters: Vec<CastRecipe>,
        resume: AnimationCommand,
        recipe: ice_tornado::IceTornadoRecipe,
    },
    FreezeLancer {
        casters: Vec<CastRecipe>,
        resume: AnimationCommand,
        recipe: freeze_lancer::FreezeLancerRecipe,
    },
    ThunderArrow {
        casters: Vec<CastRecipe>,
        resume: AnimationCommand,
        recipe: thunder_arrow::ThunderArrowRecipe,
    },
    EarthField {
        casters: Vec<CastRecipe>,
        resume: AnimationCommand,
        recipe: earth_field::EarthFieldRecipe,
    },
    FireField {
        casters: Vec<CastRecipe>,
        resume: AnimationCommand,
        recipe: FireFieldRecipe,
    },
    GroundSummon {
        casters: Vec<CastRecipe>,
        resume: AnimationCommand,
        recipe: ground_summon::GroundSummonRecipe,
    },
    Summon {
        casters: Vec<CastRecipe>,
        resume: AnimationCommand,
        recipe: summon::SummonRecipe,
    },
    Orb {
        casters: Vec<CastRecipe>,
        resume: AnimationCommand,
        recipe: orb::OrbRecipe,
    },
    Absolute {
        casters: Vec<CastRecipe>,
        resume: AnimationCommand,
        recipe: absolute::AbsoluteRecipe,
    },
    EarthBite {
        casters: Vec<CastRecipe>,
        resume: AnimationCommand,
        recipe: earth_bite::EarthBiteRecipe,
    },
    MeteorStorm {
        casters: Vec<CastRecipe>,
        resume: AnimationCommand,
        recipe: meteor_storm::MeteorStormRecipe,
    },
    SpiralFlare {
        casters: Vec<CastRecipe>,
        resume: AnimationCommand,
        recipe: spiral_flare::SpiralFlareRecipe,
    },
    GroundPulse {
        casters: Vec<CastRecipe>,
        resume: AnimationCommand,
        recipe: ground_pulse::GroundPulseRecipe,
    },
    PrismSword {
        casters: Vec<CastRecipe>,
        resume: AnimationCommand,
        recipe: prism::PrismRecipe,
    },
    Ray {
        casters: Vec<CastRecipe>,
        resume: AnimationCommand,
        recipe: ray::RayRecipe,
    },
    Lance {
        casters: Vec<CastRecipe>,
        resume: Option<AnimationCommand>,
        recipe: lance::LanceRecipe,
    },
    AcidRain {
        recipe: acid_rain::AcidRainRecipe,
    },
    Nurse {
        casters: Vec<CastRecipe>,
        resume: AnimationCommand,
        recipe: nurse::NurseRecipe,
    },
    RecoverySpell {
        casters: Vec<CastRecipe>,
        lifetime: u16,
        pulses: Vec<RecoveryPulse>,
    },
    /// Recovery occurs in initialization after an authored scene prelude.
    StoredRecoverySpell {
        casters: Vec<CastRecipe>,
        resume: AnimationCommand,
        lifetime: u16,
        percent: u16,
        effect: EffectId,
        presentation: StoredSpellPresentation,
    },
}

impl TechniqueProgram {
    pub const fn is_summon(&self) -> bool {
        matches!(self, Self::Summon { .. } | Self::GroundSummon { .. })
    }
    pub fn spell_rule(&self) -> Option<(EffectBank, HitRule)> {
        match self {
            Self::FireBall { rule, .. } => Some((EffectBank::Techniques, *rule)),
            Self::Lightning { recipe, .. } => recipe
                .pulses
                .first()
                .map(|pulse| (recipe.kind.bank(), pulse.rule)),
            Self::Orb { recipe, .. } => Some((recipe.kind.effect(1).bank, recipe.pulses[0].rule)),
            Self::Lance { recipe, .. } => Some((recipe.kind.effect(1).bank, recipe.rules[0])),
            Self::Ray { recipe, .. } => Some((ray::RayRecipe::effect(1).bank, recipe.rule)),
            Self::Absolute { recipe, .. } => {
                Some((absolute::AbsoluteRecipe::effect(1).bank, recipe.rules[0]))
            }
            Self::EarthBite { recipe, .. } => Some((
                earth_bite::EarthBiteRecipe::effect(1).bank,
                recipe.pulses[0].rule,
            )),
            Self::MeteorStorm { recipe, .. } => {
                Some((meteor_storm::MeteorStormRecipe::effect(1).bank, recipe.rule))
            }
            Self::SpiralFlare { recipe, .. } => {
                Some((spiral_flare::SpiralFlareRecipe::effect(1).bank, recipe.rule))
            }
            Self::GroundPulse { recipe, .. } => Some((recipe.kind.effect(1).bank, recipe.rule)),
            Self::PrismSword { recipe, .. } => {
                Some((prism::PrismRecipe::effect(1).bank, recipe.rules[0]))
            }
            Self::Icicle { recipe, .. } => Some((EffectBank::Techniques, recipe.pulses[0].rule)),
            Self::StoneBlast { recipe, .. } => Some((EffectBank::Techniques, recipe.rule)),
            Self::WindBlade { recipe, .. } => Some((EffectBank::Techniques, recipe.rule)),
            Self::AirThrust { recipe, .. } => Some((EffectBank::Magic(9), recipe.rule)),
            Self::WindField { recipe, .. } => Some((recipe.kind.effect(1).bank, recipe.rule)),
            Self::AquaEdge { recipe, .. } => Some((EffectBank::Techniques, recipe.rule)),
            Self::Spread { recipe, .. } => Some((EffectBank::Magic(1), recipe.rule)),
            Self::Water { recipe, .. } => Some((recipe.kind.effect(1).bank, recipe.rule)),
            Self::GroundSummon { recipe, .. } => {
                recipe.pulses.first().map(|p| (p.projectile.bank, p.rule))
            }
            Self::Summon { recipe, .. } => Some((recipe.kind.effect(1).bank, recipe.rule)),
            Self::IceTornado { recipe, .. } => Some((EffectBank::Magic(21), recipe.rule)),
            Self::FreezeLancer { recipe, .. } => Some((EffectBank::Magic(22), recipe.rule)),
            Self::ThunderArrow { recipe, .. } => Some((EffectBank::Magic(27), recipe.rule)),
            Self::EarthField { recipe, .. } => recipe
                .pulses
                .first()
                .map(|pulse| (pulse.projectile.bank, pulse.rule)),
            Self::FireField { recipe, .. } => recipe
                .pulses
                .first()
                .map(|pulse| (pulse.projectile.bank, pulse.rule)),
            _ => None,
        }
    }
    /// Every damage rule contributes resources, including later pulses with different effects.
    pub fn spell_rules(&self) -> impl Iterator<Item = (EffectBank, HitRule)> {
        let pulses = match self {
            Self::FireField { recipe, .. } => recipe.pulses.as_slice(),
            _ => &[],
        };
        let earth = match self {
            Self::GroundSummon { recipe, .. } => recipe.pulses.as_slice(),
            Self::EarthField { recipe, .. } => recipe.pulses.as_slice(),
            Self::EarthBite { recipe, .. } => recipe.pulses.as_slice(),
            _ => &[],
        };
        let lightning = match self {
            Self::Lightning { recipe, .. } => recipe.pulses.as_slice(),
            _ => &[],
        };
        let ice = match self {
            Self::Icicle { recipe, .. } => recipe.pulses.as_slice(),
            _ => &[],
        };
        let (orb_bank, orb) = match self {
            Self::Orb { recipe, .. } => (recipe.kind.effect(1).bank, recipe.pulses.as_slice()),
            _ => (EffectBank::Techniques, &[][..]),
        };
        let (rules_bank, rules) = match self {
            Self::Absolute { recipe, .. } => (
                absolute::AbsoluteRecipe::effect(1).bank,
                recipe.rules.as_slice(),
            ),
            Self::PrismSword { recipe, .. } => {
                (prism::PrismRecipe::effect(1).bank, recipe.rules.as_slice())
            }
            Self::Lance { recipe, .. } => (recipe.kind.effect(1).bank, recipe.rules.as_slice()),
            _ => (EffectBank::Techniques, &[][..]),
        };
        self.spell_rule()
            .filter(|_| {
                pulses.is_empty()
                    && earth.is_empty()
                    && lightning.is_empty()
                    && ice.is_empty()
                    && orb.is_empty()
                    && rules.is_empty()
            })
            .into_iter()
            .chain(
                pulses
                    .iter()
                    .map(|pulse| (pulse.projectile.bank, pulse.rule)),
            )
            .chain(
                earth
                    .iter()
                    .map(|pulse| (pulse.projectile.bank, pulse.rule)),
            )
            .chain(
                lightning
                    .iter()
                    .map(|pulse| (pulse.projectile.bank, pulse.rule)),
            )
            .chain(ice.iter().map(|pulse| (EffectBank::Techniques, pulse.rule)))
            .chain(orb.iter().map(move |pulse| (orb_bank, pulse.rule)))
            .chain(rules.iter().map(move |rule| (rules_bank, *rule)))
    }
    pub fn stored(&self) -> Option<StoredSpellSettings> {
        if let Self::Lance { recipe, resume, .. } = self {
            return Some(StoredSpellSettings {
                party_resume: *resume,
                lifetime: recipe.lifetime,
                presentation: recipe.presentation,
            });
        }
        if let Self::AcidRain { recipe } = self {
            return Some(StoredSpellSettings {
                party_resume: None,
                lifetime: recipe.lifetime,
                presentation: recipe.presentation,
            });
        }
        let (resume, lifetime, presentation) = match self {
            Self::StoredRecoverySpell {
                resume,
                lifetime,
                presentation,
                ..
            } => Some((*resume, *lifetime, *presentation)),
            Self::Absolute { resume, recipe, .. } => {
                Some((*resume, recipe.lifetime, recipe.presentation))
            }
            Self::EarthBite { resume, recipe, .. } => {
                Some((*resume, recipe.lifetime, recipe.presentation))
            }
            Self::MeteorStorm { resume, recipe, .. } => {
                Some((*resume, recipe.lifetime, recipe.presentation))
            }
            Self::SpiralFlare { resume, recipe, .. } => {
                Some((*resume, recipe.lifetime, recipe.presentation))
            }
            Self::GroundPulse { resume, recipe, .. } => {
                Some((*resume, recipe.lifetime, recipe.presentation))
            }
            Self::PrismSword { resume, recipe, .. } => {
                Some((*resume, recipe.lifetime, recipe.presentation))
            }
            Self::Ray { resume, recipe, .. } => {
                Some((*resume, recipe.lifetime, recipe.presentation))
            }
            Self::Orb { resume, recipe, .. } => {
                Some((*resume, recipe.lifetime, recipe.presentation))
            }
            Self::Nurse { resume, recipe, .. } => {
                Some((*resume, recipe.lifetime, recipe.presentation))
            }
            Self::Lightning { recipe, .. } => recipe
                .stored
                .as_ref()
                .map(|stored| (stored.resume, recipe.lifetime, stored.presentation)),
            Self::IceTornado { resume, recipe, .. } => {
                Some((*resume, recipe.lifetime, recipe.presentation))
            }
            Self::FreezeLancer { resume, recipe, .. } => {
                Some((*resume, recipe.lifetime, recipe.presentation))
            }
            Self::ThunderArrow { resume, recipe, .. } => {
                Some((*resume, recipe.lifetime, recipe.presentation))
            }
            Self::EarthField { resume, recipe, .. } => {
                Some((*resume, recipe.lifetime, recipe.presentation))
            }
            Self::FireField { resume, recipe, .. } => {
                Some((*resume, recipe.lifetime, recipe.presentation))
            }
            Self::WindField { resume, recipe, .. } => {
                Some((*resume, recipe.lifetime, recipe.presentation))
            }
            Self::AirThrust { resume, recipe, .. } => {
                Some((*resume, recipe.lifetime, recipe.presentation))
            }
            Self::GroundSummon { resume, recipe, .. } => {
                Some((*resume, recipe.lifetime, recipe.presentation))
            }
            Self::Summon { resume, recipe, .. } => {
                Some((*resume, recipe.kind.lifetime(), recipe.presentation))
            }
            Self::Water { resume, recipe, .. } => {
                Some((*resume, recipe.lifetime, recipe.presentation))
            }
            Self::Spread { resume, recipe, .. } => {
                Some((*resume, recipe.lifetime, recipe.presentation))
            }
            _ => None,
        }?;
        Some(StoredSpellSettings {
            party_resume: Some(resume),
            lifetime,
            presentation,
        })
    }
}

/// Scene settings are shared; enemy concluding motions belong to each casting binding.
#[derive(Debug, Clone, Copy)]
pub struct StoredSpellSettings {
    pub party_resume: Option<AnimationCommand>,
    pub lifetime: u16,
    pub presentation: StoredSpellPresentation,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct StoredSpellPresentation {
    pub color: [u8; 4],
    pub camera_distance: f32,
    /// Minimum camera elevation in degrees.
    pub camera_elevation: f32,
}

/// Caster-specific motion and voice bindings are separate from a spell's result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CastRecipe {
    pub character: u8,
    pub tp: u8,
    pub time_adjustment: i16,
    pub voices: CastingVoices,
    pub self_voices: CastingVoices,
    pub animations: AnimationProgram,
    pub commands: Vec<TimedCommand>,
    pub loop_commands: bool,
    pub pulse: u8,
    /// An absent original motion slot preserves the current pose.
    pub release: Option<AnimationCommand>,
    pub release_effect: u8,
    /// Optional pose near the end of the caster's recovery countdown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery_pose: Option<AnimationCommand>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RecoveryPulse {
    pub tick: u16,
    pub percent: u16,
    pub effect: EffectId,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CastingVoices {
    pub begin: u16,
    pub begin_remaining: u16,
    pub release: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TechniquePhase {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alternate: Option<MartialAlternate>,
    pub variant: u8,
    pub recovery_ticks: u16,
    pub buffer_until: u16,
    pub combo_at: u16,
    pub effect: Option<u16>,
    pub action: Action,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub callback: Option<MartialCallback>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
}

/// Native emissions run before authored commands and retain their own damage rule.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MartialCallback {
    Steal {
        recipe: StealRecipe,
    },
    /// Entry-only delay until the shared successful-use threshold is reached.
    ComboDelay {
        until_uses: u16,
        ticks: u16,
    },
    /// A timed defensive arte; the ordinary action clock owns its lifetime.
    MagicGuard {
        contact_effects: [u8; 9],
        contact_colors: [[u8; 3]; 9],
    },
    /// Beast family resolves the first hit's right-shoulder anchor at entry.
    Beast {
        /// One RNG draw selects the first voice: low two bits zero choose index0.
        opening_voices: Option<[u16; 2]>,
    },
    Seal {
        recipe: SealRecipe,
    },
    FierceDemonFang {
        pulses: ContactVolley,
        projectile: EffectId,
        alternate: VolleySchedule,
        rules: [HitRule; 2],
    },
    ContactArea {
        tick: u16,
        origin_height: f32,
        contact: InlineContactRecipe,
        rule: HitRule,
        followup: MartialEffect,
    },
    ContactProjectile {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        entry: Option<ElementalEntry>,
        tick: u16,
        projectile: EffectId,
        rule: HitRule,
        origin_height: f32,
        overrides: [super::projectile_modifiers::ProjectileOverride; 2],
        reaction: Option<u8>,
        followup: Option<MartialEffect>,
    },
    TigerBlade {
        minimum_uses: u16,
        element: crate::menu_data::Element,
        tick: u16,
        projectile: EffectId,
        rule: HitRule,
        origin_height: f32,
        overrides: [super::projectile_modifiers::ProjectileOverride; 3],
    },
    HammerVolley {
        first_tick: u16,
        interval: u16,
        count: u8,
        projectile: EffectId,
        rules: [HitRule; 3],
        birth_effects: [u8; 3],
        #[serde(default)]
        pattern: HammerPattern,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        overrides: Option<[super::projectile_modifiers::ProjectileOverride; 3]>,
    },
    ProjectileVolley {
        first_tick: u16,
        interval: u16,
        count: u8,
        step_degrees: f32,
        projectile: EffectId,
        rule: HitRule,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct StealRecipe {
    /// The first hit after the failure path's terminator, resumed only on success.
    pub success_hit: u16,
    pub projectile: EffectId,
    pub rule: HitRule,
    pub overrides: [super::projectile_modifiers::ProjectileOverride; 4],
}

/// One native contact gate plus the Pinion variant's independently retained flight.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SealRecipe {
    pub tick: u16,
    pub level: SealLevel,
    pub impact: SealImpact,
    pub projectile: Option<SealProjectile>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SealLevel {
    Basic,
    Pinion,
    Absolute,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SealProjectile {
    pub tick: u16,
    pub projectile: EffectId,
    pub rule: HitRule,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SealImpact {
    pub stat: super::conditions::StatDebuff,
    pub effect: EffectId,
    pub amount: i16,
    pub duration: i16,
    pub tint: [u8; 3],
    pub notice: super::ui::NoticeKind,
    pub hold: u16,
}

impl SealRecipe {
    pub fn validate(self, duration: u16) -> Result<()> {
        ensure!(
            self.tick < duration
                && self.impact.amount == -10
                && self.impact.duration > 0
                && self.impact.hold > 0
                && self.impact.effect.bank == EffectBank::Techniques
                && matches!(
                    (self.impact.stat, self.impact.notice),
                    (
                        super::conditions::StatDebuff::DefenseDown,
                        super::ui::NoticeKind::DefenseDown
                    ) | (
                        super::conditions::StatDebuff::AccuracyDown,
                        super::ui::NoticeKind::AccuracyDown
                    ) | (
                        super::conditions::StatDebuff::EvasionDown,
                        super::ui::NoticeKind::EvasionDown
                    )
                ),
            "invalid seal callback"
        );
        ensure!(
            matches!(self.level, SealLevel::Pinion) == self.projectile.is_some(),
            "seal Pinion callback missing its projectile"
        );
        if let Some(shot) = self.projectile {
            ensure!(
                shot.tick > self.tick
                    && shot.tick < duration
                    && shot.projectile.bank == EffectBank::Techniques,
                "invalid seal flight"
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HammerPattern {
    #[default]
    Repeated,
    Single,
    Rain {
        opening: HammerOpening,
        ring: HammerRing,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct HammerOpening {
    pub tick: u16,
    pub projectile: EffectId,
    pub rule: HitRule,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct HammerRing {
    pub count: u8,
    pub spawn_offset: [f32; 3],
    pub angle_step: f32,
    pub radians_per_degree: f32,
    pub speed: f32,
    pub speed_jitter: f32,
    pub lift: f32,
    pub lift_jitter: f32,
    /// Each velocity component draws its own signed integer remainder.
    pub jitter_range: u16,
}

impl HammerPattern {
    pub fn opening(self) -> Option<HammerOpening> {
        match self {
            Self::Rain { opening, .. } => Some(opening),
            _ => None,
        }
    }

    fn validate(self, first_tick: u16) -> Result<()> {
        if let Self::Rain { opening, ring } = self {
            ensure!(
                opening.tick < first_tick
                    && opening.projectile.bank == EffectBank::Techniques
                    && ring.count != 0
                    && ring.jitter_range != 0
                    && ring
                        .spawn_offset
                        .iter()
                        .chain([
                            &ring.angle_step,
                            &ring.radians_per_degree,
                            &ring.speed,
                            &ring.speed_jitter,
                            &ring.lift,
                            &ring.lift_jitter,
                        ])
                        .all(|value| value.is_finite()),
                "invalid hammer rain recipe"
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MartialAlternate {
    pub minimum_uses: u16,
    pub element: crate::menu_data::Element,
    pub effect: EffectId,
    pub caption: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct VolleySchedule {
    pub first_tick: u16,
    pub interval: u16,
    pub count: u8,
}

impl VolleySchedule {
    pub fn shot(self, tick: u16) -> Option<u16> {
        let elapsed = tick.checked_sub(self.first_tick)?;
        (self.interval != 0
            && elapsed % self.interval == 0
            && elapsed / self.interval < u16::from(self.count))
        .then(|| elapsed / self.interval)
    }
    fn validate(self, duration: u16) -> Result<()> {
        ensure!(
            self.interval != 0
                && self.count != 0
                && u32::from(self.first_tick)
                    + u32::from(self.interval) * u32::from(self.count - 1)
                    < u32::from(duration),
            "invalid native volley schedule"
        );
        Ok(())
    }
}

/// Independent short-lived contact volumes; these have no projectile-table row.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ContactVolley {
    pub schedule: VolleySchedule,
    pub lifetime: u16,
    pub velocity: [f32; 3],
    pub offset: [f32; 3],
    pub forward_step: f32,
    pub radius: f32,
    pub reactions: [u8; 2],
}

impl ContactVolley {
    pub fn recipe(self, shot: u16) -> super::effects::ProjectileRecipe {
        InlineContactRecipe {
            lifetime: self.lifetime,
            velocity: self.velocity,
            offset: [
                self.offset[0],
                self.offset[1],
                self.forward_step.mul_add(f32::from(shot), self.offset[2]),
            ],
            shape: HitShape {
                radius: self.radius,
                height: self.radius,
                inner_radius: 0.,
                kind: HitShapeKind::Box,
                damage_kind: 0,
                hit_class: 1,
                reaction: self.reactions[usize::from(shot / 2)],
            },
        }
        .recipe()
    }
}

/// Short-lived native contact volumes share the projectile pool without a table identity.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct InlineContactRecipe {
    pub lifetime: u16,
    pub velocity: [f32; 3],
    pub offset: [f32; 3],
    pub shape: HitShape,
}

impl InlineContactRecipe {
    pub fn recipe(self) -> super::effects::ProjectileRecipe {
        use super::effects::*;
        ProjectileRecipe {
            id: None,
            lifetime: self.lifetime,
            movement: ProjectileMovement::Ballistic {
                velocity: self.velocity,
                acceleration: [0.; 3],
                steering: None,
            },
            behavior: ProjectileBehavior::default(),
            velocity_jitter: [0.; 3],
            spawn_offset: self.offset,
            hit_offset: [0.; 3],
            shape: self.shape,
            knockback: KnockbackDirection::Velocity,
            active: None,
            persist_after_hit: true,
            clashable: false,
            birth_bank: Some(EffectBank::Common),
            spawn_effect: None,
            trail_effect: None,
            trail_interval: 1,
            ground_effect: None,
            shadow: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct MartialEffect {
    pub effect: EffectId,
    pub scale: f32,
}

impl MartialEffect {
    fn validate(self) -> Result<()> {
        ensure!(
            self.effect.bank == EffectBank::Techniques && self.scale.is_finite() && self.scale > 0.,
            "invalid martial followup effect"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ElementalEntry {
    pub character: u8,
    pub minimum_uses: u16,
    pub element: crate::menu_data::Element,
}

impl MartialCallback {
    /// A character-specific elemental phase selected once at accepted entry.
    pub fn elemental_entry(self) -> Option<ElementalEntry> {
        match self {
            Self::ContactProjectile { entry, .. } => entry,
            Self::TigerBlade {
                minimum_uses,
                element,
                ..
            } => Some(ElementalEntry {
                character: 1,
                minimum_uses,
                element,
            }),
            _ => None,
        }
    }

    pub fn projectile(self) -> Option<EffectId> {
        match self {
            Self::Steal { recipe } => Some(recipe.projectile),
            Self::ComboDelay { .. }
            | Self::MagicGuard { .. }
            | Self::Beast { .. }
            | Self::ContactArea { .. } => None,
            Self::ProjectileVolley { projectile, .. }
            | Self::FierceDemonFang { projectile, .. }
            | Self::ContactProjectile { projectile, .. }
            | Self::TigerBlade { projectile, .. }
            | Self::HammerVolley { projectile, .. } => Some(projectile),
            Self::Seal { recipe } => recipe.projectile.map(|shot| shot.projectile),
        }
    }

    pub fn projectiles(self) -> impl Iterator<Item = EffectId> {
        self.projectile().into_iter().chain(match self {
            Self::HammerVolley { pattern, .. } => {
                pattern.opening().map(|opening| opening.projectile)
            }
            _ => None,
        })
    }

    pub fn rules(&self) -> impl Iterator<Item = &HitRule> {
        let (rules, opening): (&[HitRule], _) = match self {
            Self::Steal { recipe } => (std::slice::from_ref(&recipe.rule), None),
            Self::ComboDelay { .. } | Self::MagicGuard { .. } | Self::Beast { .. } => (&[], None),
            Self::Seal { recipe } => (&[], recipe.projectile.as_ref().map(|shot| &shot.rule)),
            Self::ProjectileVolley { rule, .. }
            | Self::ContactProjectile { rule, .. }
            | Self::ContactArea { rule, .. }
            | Self::TigerBlade { rule, .. } => (std::slice::from_ref(rule), None),
            Self::FierceDemonFang { rules, .. } => (rules.as_slice(), None),
            Self::HammerVolley { rules, pattern, .. } => (
                rules.as_slice(),
                match pattern {
                    HammerPattern::Rain { opening, .. } => Some(&opening.rule),
                    _ => None,
                },
            ),
        };
        rules.iter().chain(opening)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SpellProjectile {
    pub tick: u16,
    pub effect: u8,
    pub sound: u16,
    pub velocity: [f32; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartyActions {
    pub character: u8,
    /// Selection order: neutral, up, down, horizontal, finisher, air, air-down.
    pub normal: Vec<NormalAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalAction {
    pub selection: u8,
    pub combo: ComboWindow,
    pub recovery: Recovery,
    pub effect: Option<u16>,
    pub reach: u16,
    pub airborne_reach: u16,
    pub action: Action,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ComboWindow {
    pub first: u16,
    pub second: Option<u16>,
    pub buffer_until: u8,
    pub allowed_directions: u8,
    pub fallback: Option<u8>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Recovery {
    pub duration: u16,
    pub animation: Option<u8>,
    pub rate: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnemyActions {
    /// Only explicit zero entries in the enemy CAB authorize keeping the live movement pose.
    #[serde(default)]
    pub absent_movement_motions: BTreeSet<EnemyMovementMotion>,
    #[serde(default)]
    pub casting: std::collections::BTreeMap<u8, EnemyCastRecipe>,
    pub monster: u8,
    /// Original metadata required by 298C0/30C4C/2F788 locomotion.
    #[serde(default)]
    pub locomotion: Option<EnemyLocomotion>,
    pub actions: Vec<EnemyAction>,
    #[serde(default)]
    pub policy: Option<EnemyPolicy>,
    #[serde(default)]
    pub native_tp: std::collections::BTreeMap<u16, u16>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct EnemyLocomotion {
    pub turn_divisor: u8,
    pub unrestricted_range: bool,
    pub bobs: bool,
    pub effect: Option<EnemyLocomotionEffect>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct EnemyLocomotionEffect {
    pub period: u8,
    pub id: u8,
}

/// Common movement requests with a source-defined null-resource return path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnemyMovementMotion {
    Walk,
    Run,
    Stop,
}

impl EnemyMovementMotion {
    pub const fn clip(self) -> u8 {
        match self {
            Self::Walk => 1,
            Self::Run => 19,
            Self::Stop => 18,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnemyPolicy {
    /// Older cooked catalogues must be recooked before native enemy controllers run.
    #[serde(default)]
    pub native: Option<EnemyNativePolicy>,
    pub ordinary_actions: u8,
    pub back_row: Vec<EnemyBackRow>,
    pub overlimit_first_action: bool,
    pub occupies_front_row: bool,
    pub close_distance: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EnemyNativePolicy {
    Ordinary,
    /// Initialize the first carried model's existing animation controller once.
    CarriedAnimationRate {
        rate: f32,
    },
    /// Split guardian bodies initialize their bone scales and carried-part visibility once.
    GuardianParts {
        wing_scale: f32,
        defeat_ticks: u16,
    },
    ThreeStations {
        stations: [EnemyStation; 3],
    },
    BossPresentation {
        opening: BossVoice,
        defeat_ticks: u16,
    },
    DefeatPresentation {
        defeat_ticks: u16,
    },
    ItemlessDuel {
        defeat_ticks: u16,
    },
    Unsupported {
        id: u8,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EnemyStation {
    pub position: [f32; 3],
    pub heading: f32,
}

/// Scene lengths are authored ticks, independent of playback settings or decoded clip length.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BossVoice {
    pub id: u16,
    pub ticks: u16,
}

impl EnemyNativePolicy {
    pub fn validate(self) -> Result<()> {
        match self {
            Self::Ordinary => Ok(()),
            Self::CarriedAnimationRate { rate } => {
                ensure!(
                    rate.is_finite() && rate > 0.,
                    "invalid carried animation rate"
                );
                Ok(())
            }
            Self::GuardianParts {
                wing_scale,
                defeat_ticks,
            } => {
                ensure!(
                    wing_scale.is_finite() && wing_scale > 0.,
                    "invalid guardian wing scale"
                );
                Self::DefeatPresentation { defeat_ticks }.validate()
            }
            Self::ThreeStations { stations } => {
                ensure!(
                    stations
                        .iter()
                        .all(|station| station.position.iter().all(|v| v.is_finite())
                            && station.heading.is_finite()),
                    "invalid native enemy stations"
                );
                Ok(())
            }
            Self::BossPresentation {
                opening,
                defeat_ticks,
            } => {
                ensure!(
                    opening.ticks <= i16::MAX as u16 - 30 && defeat_ticks <= i16::MAX as u16 - 60,
                    "invalid boss scene duration"
                );
                Ok(())
            }
            Self::DefeatPresentation { defeat_ticks } | Self::ItemlessDuel { defeat_ticks } => {
                ensure!(
                    defeat_ticks <= i16::MAX as u16 - 60,
                    "invalid defeat scene duration"
                );
                Ok(())
            }
            Self::Unsupported { id } => {
                ensure!(
                    (1..=27).contains(&id) && ![5, 11, 13, 20, 25, 27].contains(&id),
                    "invalid unsupported enemy policy"
                );
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct EnemyBackRow {
    pub action: u8,
    pub weight: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnemyAction {
    pub id: u8,
    #[serde(default)]
    pub approach: Option<EnemyApproach>,
    #[serde(default)]
    pub contact_recovery: Option<EnemyContactRecovery>,
    pub selection: EnemySelection,
    pub combo_at: u16,
    pub followup_group: u8,
    pub followup_chance: u8,
    pub stagger_threshold: u8,
    pub guard_chance: u8,
    pub vulnerable: [u16; 2],
    pub resource_decrement: u8,
    pub technique: Option<u16>,
    pub effect: Option<u8>,
    pub recovery: Recovery,
    pub action: Action,
}

/// Enemy chanting is an entry controller, independent of the ordinary attack tracks.
/// Source motion-table slots used by enemy casting, independently of action clips.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnemyCastingMotion {
    Chant,
    Conclusion,
    StoredRelease,
}

impl EnemyCastingMotion {
    pub const fn clip(self) -> u16 {
        match self {
            Self::Chant => 11,
            Self::Conclusion => 12,
            Self::StoredRelease => 13,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnemyCastRecipe {
    pub duration: u16,
    pub tp: u8,
    pub voices: CastingVoices,
    pub animation: AnimationCommand,
    pub loop_start: u8,
    pub commands: Vec<TimedCommand>,
    pub loop_commands: bool,
    pub pulse: u8,
    pub release: AnimationCommand,
    pub resume: AnimationCommand,
    /// Loop operand for the concluding motion, independent of its opening frame.
    pub resume_loop_start: u8,
    pub release_effect: u8,
    /// Automatic ordinary casting enters the release pose before its countdown ends.
    #[serde(default)]
    pub early_release: Option<CastingConclusion>,
    /// Explicit zero source CAB pointers preserve the currently bound motion.
    #[serde(default)]
    pub absent_motions: BTreeSet<EnemyCastingMotion>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CastingConclusion {
    pub animation: AnimationCommand,
    /// Source loop operand, scaled by the animation rate independently of its start.
    pub loop_start: u8,
}

impl EnemyActions {
    pub fn native_spells(&self) -> BTreeSet<EnemySpellBinding> {
        self.actions
            .iter()
            .flat_map(|action| {
                action
                    .technique
                    .into_iter()
                    .map(EnemySpellBinding::Chant)
                    .chain(
                        action
                            .action
                            .commands
                            .iter()
                            .filter_map(|step| match step.command {
                                ActionCommand::CastNative { native_id, .. } => {
                                    Some(EnemySpellBinding::Direct(native_id))
                                }
                                _ => None,
                            }),
                    )
            })
            .collect()
    }
}

/// A contact recovery executes its entry commands once, without a second clock.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnemyContactRecovery {
    pub animation: Option<AnimationCommand>,
    pub commands: Vec<ActionCommand>,
    pub texture_layers: [u8; 4],
}

impl EnemyContactRecovery {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.animation.is_none_or(|command| matches!(command,
            AnimationCommand::Play { blend: 4, start: 0, end: None, layer: 8,
                looping: false, mirror: false, resource: -1, rate, .. } if rate == 0.5)),
            "invalid enemy contact recovery motion"
        );
        for command in &self.commands {
            ensure!(
                match command {
                    ActionCommand::ForwardSpeed(v)
                    | ActionCommand::VerticalSpeed(v)
                    | ActionCommand::ForwardAcceleration(v)
                    | ActionCommand::Gravity(v)
                    | ActionCommand::ForwardDeceleration(v) => v.is_finite(),
                    ActionCommand::BodyPush { .. } | ActionCommand::Voice { .. } => true,
                    _ => false,
                },
                "unsupported enemy contact recovery command"
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EnemyApproach {
    Walk,
    Run,
    Custom { clip: u8, rate: f32, speed: f32 },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct EnemySelection {
    pub weight: i8,
    pub target_policy: u8,
    pub requirements: u32,
    pub target_state: u16,
    pub range: [i16; 2],
    pub approach_range: i16,
    pub approach_minimum: i16,
    pub required_story_flag: Option<u16>,
    pub required_monster: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    pub duration: u16,
    pub tp: u8,
    pub animations: AnimationProgram,
    pub commands: Vec<TimedCommand>,
    pub loop_commands: bool,
    pub hits: Vec<HitWindow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnimationStep {
    pub trigger: AnimationTrigger,
    pub command: AnimationCommand,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum AnimationTrigger {
    Tick(i16),
    Finished,
    Grounded,
    LoopAfterFinished(u8),
}

/// Initialization and loop targets bind a motion without dispatching its opcode.
/// Subsequent instruction addresses use a signed, wrapping eight-bit cursor.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnimationProgram {
    pub initial: Option<AnimationCommand>,
    #[serde(deserialize_with = "animation_addresses")]
    pub instructions: std::collections::BTreeMap<i16, AnimationInstruction>,
    #[serde(deserialize_with = "animation_addresses")]
    pub loop_targets: std::collections::BTreeMap<i16, AnimationBinding>,
}

// Tagged recipes buffer JSON object keys as strings before decoding the program.
fn animation_addresses<'de, D, T>(d: D) -> Result<std::collections::BTreeMap<i16, T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    std::collections::BTreeMap::<String, T>::deserialize(d)?
        .into_iter()
        .map(|(key, value)| {
            key.parse()
                .map(|key| (key, value))
                .map_err(serde::de::Error::custom)
        })
        .collect()
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct AnimationBinding {
    pub time: i16,
    pub command: AnimationCommand,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum AnimationInstruction {
    Step(AnimationStep),
    End,
    Stalled,
}

impl AnimationProgram {
    pub fn commands(&self) -> impl Iterator<Item = AnimationCommand> + '_ {
        self.initial
            .into_iter()
            .chain(
                self.instructions
                    .values()
                    .filter_map(|instruction| match instruction {
                        AnimationInstruction::Step(step) => Some(step.command),
                        _ => None,
                    }),
            )
            .chain(self.loop_targets.values().map(|bind| bind.command))
    }

    pub fn is_empty(&self) -> bool {
        self.initial.is_none()
    }
}

/// Construct a short, sequential program authored in Rust.
impl FromIterator<AnimationStep> for AnimationProgram {
    fn from_iter<T: IntoIterator<Item = AnimationStep>>(steps: T) -> Self {
        let steps: Vec<_> = steps.into_iter().collect();
        assert!(steps.len() <= 128, "sequential animation cursor overflow");
        let loop_targets = steps
            .iter()
            .filter_map(|step| {
                let AnimationCommand::Loop { step: target } = step.command else {
                    return None;
                };
                let step = &steps[usize::from(target)];
                let AnimationTrigger::Tick(time) = step.trigger else {
                    panic!("authored loop target requires a time");
                };
                Some((
                    i16::from(target),
                    AnimationBinding {
                        time,
                        command: step.command,
                    },
                ))
            })
            .collect();
        let end = i16::from(steps.len() as i8);
        let mut instructions: std::collections::BTreeMap<_, _> = steps
            .iter()
            .cloned()
            .enumerate()
            .skip(1)
            .map(|(index, step)| (index as i16, AnimationInstruction::Step(step)))
            .collect();
        if !steps.is_empty() {
            instructions.insert(end, AnimationInstruction::End);
        }
        Self {
            initial: steps.first().map(|step| step.command),
            instructions,
            loop_targets,
        }
    }
}

impl From<Vec<AnimationStep>> for AnimationProgram {
    fn from(steps: Vec<AnimationStep>) -> Self {
        steps.into_iter().collect()
    }
}

impl<const N: usize> From<[AnimationStep; N]> for AnimationProgram {
    fn from(steps: [AnimationStep; N]) -> Self {
        steps.into_iter().collect()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AnimationCommand {
    Loop {
        step: u8,
    },
    Play {
        clip: u8,
        blend: u8,
        start: u8,
        end: Option<u8>,
        layer: u8,
        looping: bool,
        mirror: bool,
        resource: i8,
        rate: f32,
    },
    Rate {
        rate: f32,
    },
    Texture {
        layers: [u8; 2],
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimedCommand {
    pub tick: u16,
    pub command: ActionCommand,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ActionCommand {
    /// Reserved source command; its operands have no runtime consumer.
    Noop,
    /// End the actor's participation using its ordinary defeat presentation.
    Defeat {
        rewards: bool,
    },
    AmbientColor([u8; 3]),
    /// Scale subsequent melee hits and projectiles spawned by this actor.
    DamagePower(u16),
    /// Apply a signed percentage of this actor's maximum HP or TP.
    Recover {
        resource: RecoveryResource,
        percent: i16,
    },
    CommonEffect {
        id: u8,
        origin: EffectOrigin,
    },
    /// Common impact effect, scene tint and a sixty-tick cinematic pause.
    ImpactFlash,
    CastNative {
        native_id: u16,
        slot: super::action_program::CastSlot,
    },
    TextureLayers([u8; 4]),
    TextureVariant(u8),
    /// Persistent model-controller slot; the scale clock is independent of animation.
    BoneScale {
        slot: u8,
        bone: u16,
        duration: i16,
        scale: f32,
    },
    ForwardSpeed(f32),
    VerticalSpeed(f32),
    /// Set a captured victim's release velocity; ignored without a victim.
    HeldForwardSpeed(f32),
    HeldVerticalSpeed(f32),
    /// Show or hide a captured victim; ignored without a victim.
    HeldVisibility(bool),
    ReleaseHeld {
        hitstun: i16,
    },
    /// Adjust the completion deadline without rewinding authored tracks.
    ExtendAction(i16),
    ForwardAcceleration(f32),
    ForwardDeceleration(f32),
    BodyPush {
        enabled: bool,
        restore_after: u8,
    },
    /// Suppress ordinary hit reactions without blocking damage.
    Poise {
        enabled: bool,
        duration: i16,
    },
    /// Temporary world-space wind on hair and clothing chains.
    SecondaryWind {
        enabled: bool,
        duration: i16,
    },
    Gravity(f32),
    /// Immediately move along the retained movement direction, in world units.
    AdvancePosition(f32),
    /// Absolute world coordinates in source units.
    SetPosition([i16; 3]),
    /// Place relative to the current target, using the cached target bearing.
    PositionFromTarget {
        height: i16,
        retreat: i16,
    },
    /// Retreat along the cached target bearing, then replace world height.
    OffsetPosition {
        height: i16,
        retreat: i16,
    },
    Reverse,
    /// Rotate the retained base, copy it to movement, and add radians to visible facing.
    TurnHeading(f32),
    /// Add radians to facing; rotate movement from its retained base direction.
    TurnMotion(f32),
    /// Replace both retained directions and visible facing with the cached target bearing.
    FaceTargetDirection,
    AttachmentVisibility {
        slot: u8,
        visible: bool,
    },
    AttachmentTrail {
        slot: u16,
        ticks: u16,
    },
    Voice {
        id: u16,
        priority: u8,
    },
    RandomVoice {
        first: u16,
        second: u16,
        priority: u8,
        first_percent: u16,
    },
    Sound(u16),
    PlayerCameraMotion {
        duration: i16,
        amount: f32,
    },
    CameraBounds {
        duration: i16,
        distance: f32,
        elevation: f32,
    },
    ApplyConditions {
        conditions: super::conditions::ScriptConditions,
        /// Ailments ignore strength; it remains authored signed data.
        strength: i16,
    },
    WaitHit,
    WaitActionResult,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryResource {
    Hp,
    Tp,
}

/// A command snapshots either the live model root or the sampled body anchor.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectOrigin {
    Root,
    Body,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HitWindow {
    pub start: u16,
    pub emission: HitEmission,
    pub shape: HitShape,
    pub rule: HitRule,
    pub projectile_modifier: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HitEmission {
    Contact {
        duration: u8,
        attachment: HitAttachment,
    },
    Effect {
        effect: u8,
        bone: Option<u8>,
    },
    Projectile {
        motion: u8,
        slot: u8,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum HitAttachment {
    Groups(Vec<u8>),
    Center,
    BodyBone(u8),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct HitShape {
    pub radius: f32,
    pub height: f32,
    pub kind: HitShapeKind,
    pub inner_radius: f32,
    pub damage_kind: u8,
    pub hit_class: u8,
    pub reaction: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HitShapeKind {
    Box,
    Cylinder,
    GroundCircle,
    Ring,
    Sphere,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HitRule {
    pub flags: u16,
    pub element: HitElement,
    pub hitstun: u8,
    pub contact_cooldown: u8,
    pub stun_chance: u8,
    pub stagger: u8,
    pub guard_pressure: u8,
    pub conditions: u32,
    pub condition_chance: u8,
    pub power_mode: u8,
    pub power: u16,
    pub sound: u16,
    pub stagger_resistance: u8,
    pub knockback_delay: u8,
    pub impact_effect: u8,
    pub condition_parameter: i8,
    pub impact_bank: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "element", rename_all = "snake_case")]
pub enum HitElement {
    Inherit,
    Neutral,
    Element(crate::menu_data::Element),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ProjectileMotion {
    pub lifetime: i16,
    pub forward_speed: f32,
    pub return_speed: f32,
    pub vertical_speed: f32,
}

impl BattleActions {
    pub fn chain_effects(&self) -> BTreeSet<EffectId> {
        self.techniques
            .iter()
            .filter_map(|arte| {
                if !matches!(arte.program, TechniqueProgram::Martial { .. }) {
                    return None;
                }
                let id = match arte.properties.chain {
                    TechniqueChain::Second | TechniqueChain::Launch => 21,
                    TechniqueChain::Third | TechniqueChain::Aerial => 22,
                    _ => return None,
                };
                Some(EffectId {
                    bank: EffectBank::Common,
                    id,
                })
            })
            .collect()
    }
    pub fn validate(&self) -> Result<()> {
        if let Some(chains) = &self.chains {
            chains.validate()?;
        }
        ensure!(
            self.party
                .iter()
                .map(|p| p.character)
                .collect::<BTreeSet<_>>()
                .len()
                == self.party.len()
                && self.party.iter().all(|p| (1..=9).contains(&p.character)),
            "duplicate or invalid party action owner"
        );
        ensure!(
            self.enemies
                .iter()
                .map(|p| p.monster)
                .collect::<BTreeSet<_>>()
                .len()
                == self.enemies.len()
                && self
                    .enemies
                    .iter()
                    .all(|p| usize::from(p.monster) < crate::monster::MONSTER_COUNT),
            "duplicate or invalid enemy action owner"
        );
        for party in &self.party {
            ensure!(
                party.normal.len() == 7,
                "incomplete normal attack selections"
            );
            for (i, normal) in party.normal.iter().enumerate() {
                ensure!(
                    normal.selection as usize == i
                        && normal.combo.fallback.is_none_or(|v| v < 7)
                        && normal.recovery.rate.is_finite(),
                    "invalid normal attack selection"
                );
            }
        }
        for enemy in &self.enemies {
            if let Some(native) = enemy.policy.as_ref().and_then(|policy| policy.native) {
                native.validate()?;
                ensure!(
                    !matches!(native, EnemyNativePolicy::ThreeStations { .. })
                        || enemy.actions.iter().any(|action| action.id == 2),
                    "native enemy station action is missing"
                );
            }
            for (&id, cast) in &enemy.casting {
                ensure!(enemy.actions.iter().any(|action| action.id == id && action.technique.and_then(EnemySpell::from_native).is_some())
                    && cast.duration > 0 && matches!(cast.pulse, 3..=5)
                    && [cast.animation, cast.release, cast.resume].iter().all(|command|
                        matches!(command, AnimationCommand::Play { rate, .. } if rate.is_finite() && *rate > 0.)),
                    "invalid enemy casting recipe");
                if let Some(early) = cast.early_release {
                    ensure!(
                        enemy.actions.iter().any(|action| action.id == id
                            && action
                                .technique
                                .and_then(EnemySpell::from_native)
                                .is_some_and(|spell| !spell.stored()))
                            && matches!(early.animation, AnimationCommand::Play {
                            clip: 12, start: 4, end: None, layer: 8, mirror: false,
                            resource: -1, rate, ..
                        } if rate.is_finite() && rate > 0.),
                        "invalid early casting conclusion recipe"
                    );
                }
            }
            for action in &enemy.actions {
                if let Some(recovery) = &action.contact_recovery {
                    recovery.validate()?;
                }
            }
        }
        for technique in &self.techniques {
            let casters = match &technique.program {
                TechniqueProgram::Martial { variants } => {
                    for phase in variants {
                        if let Some(alternate) = &phase.alternate {
                            ensure!(
                                matches!(
                                    phase.callback,
                                    Some(MartialCallback::FierceDemonFang { .. })
                                ) && alternate.minimum_uses > 0
                                    && !alternate.caption.is_empty()
                                    && alternate.effect.bank == EffectBank::Techniques,
                                "invalid elemental martial entry"
                            );
                        }
                        if let Some(callback) = phase.callback {
                            let (first_tick, interval, count) = match callback {
                                MartialCallback::Steal { recipe } => {
                                    ensure!(
                                        matches!(technique.native_id, 43 | 44)
                                            && phase.variant == 0
                                            && recipe.success_hit > 0
                                            && usize::from(recipe.success_hit)
                                                < phase.action.hits.len(),
                                        "invalid steal continuation"
                                    );
                                    for operation in recipe.overrides {
                                        operation.validate()?;
                                    }
                                    (40, 1, 1)
                                }
                                MartialCallback::ComboDelay { until_uses, ticks } => {
                                    ensure!(
                                        technique.native_id == 14
                                            && until_uses == 200
                                            && ticks == 20,
                                        "invalid combo delay"
                                    );
                                    (0, 1, 1)
                                }
                                MartialCallback::MagicGuard {
                                    contact_effects, ..
                                } => {
                                    ensure!(
                                        technique.native_id == 34
                                            && phase.variant < 9
                                            && phase.action.duration == 60
                                            && phase.recovery_ticks == 10
                                            && phase.action.hits.is_empty()
                                            && phase.effect
                                                == Some(if phase.variant == 2 { 2 } else { 1 })
                                            && contact_effects
                                                == [1, 30, 31, 29, 32, 33, 34, 49, 0],
                                        "invalid Magic Guard entry"
                                    );
                                    (0, 1, 1)
                                }
                                MartialCallback::Beast { opening_voices } => {
                                    ensure!(
                                        phase.variant == 0
                                            && matches!(
                                                (technique.native_id, opening_voices),
                                                (20, Some([111, 0x804e])) | (22, None)
                                            )
                                            && matches!(
                                                phase.action.hits.first().map(|hit| &hit.emission),
                                                Some(HitEmission::Contact {
                                                    attachment: HitAttachment::BodyBone(_),
                                                    ..
                                                })
                                            )
                                            && matches!(
                                                phase.action.commands.first(),
                                                Some(TimedCommand {
                                                    tick: 0,
                                                    command: ActionCommand::Voice { .. }
                                                })
                                            ),
                                        "invalid Beast entry"
                                    );
                                    (0, 1, 1)
                                }
                                MartialCallback::Seal { recipe } => {
                                    recipe.validate(phase.action.duration)?;
                                    (recipe.tick, 1, 1)
                                }
                                MartialCallback::FierceDemonFang {
                                    pulses, alternate, ..
                                } => {
                                    ensure!(
                                        phase.alternate.is_some()
                                            && pulses.schedule.count == 3
                                            && pulses.lifetime > 0
                                            && pulses.forward_step.is_finite(),
                                        "invalid contact volley"
                                    );
                                    alternate.validate(phase.action.duration)?;
                                    pulses.schedule.validate(phase.action.duration)?;
                                    pulses.recipe(0).validate()?;
                                    pulses
                                        .recipe(u16::from(pulses.schedule.count - 1))
                                        .validate()?;
                                    (
                                        pulses.schedule.first_tick,
                                        pulses.schedule.interval,
                                        pulses.schedule.count,
                                    )
                                }
                                MartialCallback::ContactArea {
                                    tick,
                                    origin_height,
                                    contact,
                                    followup,
                                    ..
                                } => {
                                    ensure!(
                                        origin_height.is_finite() && contact.lifetime > 0,
                                        "invalid contact area"
                                    );
                                    contact.recipe().validate()?;
                                    followup.validate()?;
                                    (tick, 1, 1)
                                }
                                MartialCallback::ContactProjectile {
                                    entry,
                                    tick,
                                    origin_height,
                                    overrides,
                                    reaction,
                                    followup,
                                    ..
                                } => {
                                    ensure!(
                                        origin_height.is_finite(),
                                        "invalid contact projectile height"
                                    );
                                    if let Some(entry) = entry {
                                        ensure!(
                                            (1..=9).contains(&entry.character)
                                                && entry.minimum_uses > 0,
                                            "invalid elemental phase entry"
                                        );
                                    }
                                    for operation in overrides {
                                        operation.validate()?;
                                    }
                                    if let Some(value) = reaction {
                                        super::projectile_modifiers::ProjectileOverride::Reaction {
                                            value,
                                        }
                                        .validate()?;
                                    }
                                    if let Some(effect) = followup {
                                        effect.validate()?;
                                    }
                                    (tick, 1, 1)
                                }
                                MartialCallback::TigerBlade {
                                    tick,
                                    minimum_uses,
                                    origin_height,
                                    overrides,
                                    ..
                                } => {
                                    ensure!(
                                        minimum_uses != 0 && origin_height.is_finite(),
                                        "invalid elemental martial entry"
                                    );
                                    for operation in overrides {
                                        operation.validate()?;
                                    }
                                    (tick, 1, 1)
                                }
                                MartialCallback::ProjectileVolley {
                                    first_tick,
                                    interval,
                                    count,
                                    step_degrees,
                                    ..
                                } => {
                                    ensure!(
                                        step_degrees.is_finite(),
                                        "non-finite martial volley spread"
                                    );
                                    (first_tick, interval, count)
                                }
                                MartialCallback::HammerVolley {
                                    first_tick,
                                    interval,
                                    count,
                                    birth_effects,
                                    overrides,
                                    pattern,
                                    ..
                                } => {
                                    ensure!(
                                        birth_effects.iter().all(|&id| id != 0),
                                        "missing hammer birth effect"
                                    );
                                    pattern.validate(first_tick)?;
                                    for operation in overrides.into_iter().flatten() {
                                        operation.validate()?;
                                    }
                                    (first_tick, interval, count)
                                }
                            };
                            ensure!(
                                interval != 0
                                    && count != 0
                                    && u32::from(first_tick)
                                        + u32::from(interval) * u32::from(count - 1)
                                        < u32::from(phase.action.duration)
                                    && callback
                                        .projectiles()
                                        .all(|id| id.bank == EffectBank::Techniques),
                                "invalid martial projectile volley"
                            );
                        }
                    }
                    continue;
                }
                TechniqueProgram::Lightning { casters, recipe } => {
                    recipe.validate()?;
                    ensure!(
                        technique.native_id == recipe.kind.native(),
                        "invalid lightning native runner"
                    );
                    casters
                }
                TechniqueProgram::Icicle { casters, recipe } => {
                    recipe.validate()?;
                    ensure!(technique.native_id == 220, "invalid Icicle runner");
                    casters
                }
                TechniqueProgram::StoneBlast { casters, recipe } => {
                    recipe.validate()?;
                    ensure!(technique.native_id == 212, "invalid Stone Blast runner");
                    casters
                }
                TechniqueProgram::WindBlade { casters, recipe } => {
                    recipe.validate()?;
                    ensure!(technique.native_id == 208, "invalid Wind Blade runner");
                    casters
                }
                TechniqueProgram::IceTornado {
                    casters,
                    resume,
                    recipe,
                } => {
                    recipe.validate()?;
                    ensure!(
                        technique.native_id == 221
                            && matches!(resume, AnimationCommand::Play {clip:12,rate,..} if rate.is_finite() && *rate > 0.),
                        "invalid Ice Tornado runner"
                    );
                    casters
                }
                TechniqueProgram::FreezeLancer {
                    casters,
                    resume,
                    recipe,
                } => {
                    recipe.validate()?;
                    ensure!(
                        technique.native_id == 222
                            && matches!(resume, AnimationCommand::Play {clip:12,rate,..} if rate.is_finite() && *rate > 0.),
                        "invalid Freeze Lancer runner"
                    );
                    casters
                }
                TechniqueProgram::ThunderArrow {
                    casters,
                    resume,
                    recipe,
                } => {
                    recipe.validate()?;
                    ensure!(
                        technique.native_id == 227
                            && matches!(resume, AnimationCommand::Play {clip:12,rate,..} if rate.is_finite() && *rate > 0.),
                        "invalid Thunder Arrow runner"
                    );
                    casters
                }
                TechniqueProgram::EarthField {
                    casters,
                    resume,
                    recipe,
                } => {
                    recipe.validate()?;
                    ensure!(
                        technique.native_id == recipe.kind as u16
                            && matches!(resume, AnimationCommand::Play {clip:12,rate,..} if rate.is_finite() && *rate > 0.),
                        "invalid stored Earth runner"
                    );
                    casters
                }
                TechniqueProgram::FireField {
                    casters,
                    resume,
                    recipe,
                } => {
                    recipe.validate()?;
                    ensure!(
                        technique.native_id == recipe.native_id
                            && matches!(resume, AnimationCommand::Play {clip:12,rate,..} if rate.is_finite() && *rate > 0.),
                        "invalid fire field runner"
                    );
                    casters
                }
                TechniqueProgram::WindField {
                    casters,
                    resume,
                    recipe,
                } => {
                    recipe.validate()?;
                    ensure!(
                        technique.native_id == recipe.kind as u16
                            && matches!(resume, AnimationCommand::Play {clip:12,rate,..} if rate.is_finite() && *rate > 0.),
                        "invalid stored Wind runner"
                    );
                    casters
                }
                TechniqueProgram::AirThrust {
                    casters,
                    resume,
                    recipe,
                } => {
                    recipe.validate()?;
                    ensure!(
                        technique.native_id == 209
                            && matches!(resume, AnimationCommand::Play {clip:12,rate,..} if rate.is_finite() && *rate > 0.),
                        "invalid Air Thrust runner"
                    );
                    casters
                }
                TechniqueProgram::AquaEdge { casters, recipe } => {
                    recipe.validate()?;
                    ensure!(
                        technique.native_id == 200 && recipe.lifetime == 150,
                        "invalid Aqua Edge runner"
                    );
                    casters
                }
                TechniqueProgram::Water {
                    casters,
                    resume,
                    recipe,
                } => {
                    recipe.validate()?;
                    ensure!(
                        technique.native_id == recipe.kind as u16
                            && matches!(resume, AnimationCommand::Play {clip:12,rate,..} if rate.is_finite() && *rate > 0.),
                        "invalid stored water runner"
                    );
                    casters
                }
                TechniqueProgram::Spread {
                    casters,
                    resume,
                    recipe,
                } => {
                    recipe.validate()?;
                    ensure!(
                        technique.native_id == 201
                            && recipe.lifetime == 170
                            && recipe.projectile_tick == 45
                            && recipe.projectile
                                == EffectId {
                                    bank: EffectBank::Magic(1),
                                    id: 1
                                }
                            && recipe.effect
                                == EffectId {
                                    bank: EffectBank::Magic(1),
                                    id: 1
                                }
                            && matches!(resume,AnimationCommand::Play {clip:12,rate,..} if rate.is_finite() && *rate > 0.)
                            && recipe.presentation.color[3] == 255,
                        "invalid Spread runner"
                    );
                    casters
                }
                TechniqueProgram::GroundSummon {
                    casters,
                    resume,
                    recipe,
                } => {
                    recipe.validate()?;
                    ensure!(
                        technique.native_id == recipe.kind.native()
                            && technique.technique == recipe.kind.menu()
                            && casters.len() == 1
                            && casters[0].character == 5
                            && casters[0].release.is_none()
                            && matches!(resume, AnimationCommand::Play {clip:12,rate,..} if rate.is_finite() && *rate > 0.),
                        "invalid ground summon caster binding"
                    );
                    casters
                }
                TechniqueProgram::Summon {
                    casters,
                    resume,
                    recipe,
                } => {
                    recipe.validate()?;
                    ensure!(
                        technique.native_id == recipe.kind.native()
                            && casters.len() == 1
                            && casters[0].character == 5
                            && matches!(resume, AnimationCommand::Play {clip:12,rate,..} if rate.is_finite() && *rate > 0.),
                        "invalid summon caster binding"
                    );
                    casters
                }
                TechniqueProgram::Orb {
                    casters,
                    resume,
                    recipe,
                } => {
                    recipe.validate()?;
                    ensure!(
                        technique.native_id == recipe.kind as u16
                            && matches!(resume, AnimationCommand::Play {clip:12,rate,..} if rate.is_finite() && *rate > 0.),
                        "invalid orb spell runner"
                    );
                    // The event-only Dark Sphere template has no recovered party chant binding.
                    if casters.is_empty() && recipe.kind == orb::OrbSpell::DarkSphere {
                        continue;
                    }
                    casters
                }
                TechniqueProgram::Ray {
                    casters,
                    resume,
                    recipe,
                } => {
                    recipe.validate()?;
                    ensure!(
                        technique.native_id == 252
                            && technique.technique == 114
                            && casters.len() == 1
                            && casters[0].character == 4
                            && matches!(resume, AnimationCommand::Play {clip:12,rate,..} if rate.is_finite() && *rate > 0.),
                        "invalid Ray caster or native/menu binding"
                    );
                    casters
                }
                TechniqueProgram::Absolute {
                    casters,
                    resume,
                    recipe,
                } => {
                    recipe.validate()?;
                    ensure!(
                        technique.native_id == 230
                            && technique.technique == 92
                            && casters.len() == 1
                            && casters[0].character == 3
                            && matches!(resume, AnimationCommand::Play {clip:12,rate,..} if rate.is_finite() && *rate > 0.),
                        "invalid Absolute caster or native/menu binding"
                    );
                    casters
                }
                TechniqueProgram::EarthBite {
                    casters,
                    resume,
                    recipe,
                } => {
                    recipe.validate()?;
                    ensure!(
                        technique.native_id == 231
                            && technique.technique == 93
                            && casters.len() == 1
                            && casters[0].character == 3
                            && matches!(resume, AnimationCommand::Play {clip:12,rate,..} if rate.is_finite() && *rate > 0.),
                        "invalid EarthBite caster or native/menu binding"
                    );
                    casters
                }
                TechniqueProgram::MeteorStorm {
                    casters,
                    resume,
                    recipe,
                } => {
                    recipe.validate()?;
                    ensure!(
                        technique.native_id == 233
                            && technique.technique == 95
                            && casters.len() == 1
                            && casters[0].character == 3
                            && matches!(resume, AnimationCommand::Play {clip:12,rate,..} if rate.is_finite() && *rate > 0.),
                        "invalid MeteorStorm caster or native/menu binding"
                    );
                    casters
                }
                TechniqueProgram::SpiralFlare {
                    casters,
                    resume,
                    recipe,
                } => {
                    recipe.validate()?;
                    ensure!(
                        technique.native_id == 226
                            && technique.technique == 88
                            && casters.len() == 1
                            && casters[0].character == 3
                            && matches!(resume, AnimationCommand::Play {clip:12,rate,..} if rate.is_finite() && *rate > 0.),
                        "invalid Spiral Flare caster or native/menu binding"
                    );
                    casters
                }
                TechniqueProgram::GroundPulse {
                    casters,
                    resume,
                    recipe,
                } => {
                    recipe.validate()?;
                    ensure!(
                        technique.native_id == recipe.kind as u16
                            && technique.technique == recipe.kind.menu()
                            && casters.len() == 1
                            && casters[0].character == 3
                            && matches!(resume, AnimationCommand::Play {clip:12,rate,..} if rate.is_finite() && *rate > 0.),
                        "invalid ground pulse caster or native/menu binding"
                    );
                    casters
                }
                TechniqueProgram::PrismSword {
                    casters,
                    resume,
                    recipe,
                } => {
                    recipe.validate()?;
                    ensure!(
                        technique.native_id == 232
                            && technique.technique == 94
                            && casters.len() == 1
                            && casters[0].character == 3
                            && matches!(resume, AnimationCommand::Play {clip:12,rate,..} if rate.is_finite() && *rate > 0.),
                        "invalid Prism Sword caster or native/menu binding"
                    );
                    casters
                }
                TechniqueProgram::Lance {
                    casters,
                    resume,
                    recipe,
                } => {
                    recipe.validate()?;
                    ensure!(
                        technique.native_id == recipe.kind as u16
                            && technique.technique == recipe.kind.menu(),
                        "invalid lance native/menu binding"
                    );
                    if recipe.kind == lance::LanceSpell::BloodyLance {
                        ensure!(
                            casters.is_empty() && resume.is_none(),
                            "unresolved event-only Bloody Lance party entry"
                        );
                        continue;
                    }
                    ensure!(
                        casters.len() == 1
                            && casters[0].character == 4
                            && matches!(resume, Some(AnimationCommand::Play {clip:12,rate,..}) if rate.is_finite() && *rate > 0.),
                        "invalid Holy Lance caster binding"
                    );
                    casters
                }
                TechniqueProgram::AcidRain { recipe } => {
                    recipe.validate()?;
                    ensure!(
                        technique.native_id == 265,
                        "invalid Acid Rain native binding"
                    );
                    continue;
                }
                TechniqueProgram::Nurse {
                    casters,
                    resume,
                    recipe,
                } => {
                    recipe.validate()?;
                    ensure!(
                        technique.native_id == 237
                            && casters.len() == 1
                            && casters[0].character == 4
                            && matches!(resume, AnimationCommand::Play {clip:12,rate,..} if rate.is_finite() && *rate > 0.),
                        "invalid Nurse caster binding"
                    );
                    casters
                }
                TechniqueProgram::StoredRecoverySpell {
                    casters,
                    resume,
                    lifetime,
                    percent,
                    effect,
                    presentation,
                } => {
                    ensure!(
                        *lifetime > 45
                            && (1..=100).contains(percent)
                            && technique.native_id >= 200
                            && effect.bank == EffectBank::Magic(technique.native_id - 200)
                            && matches!(resume, AnimationCommand::Play { clip: 12, rate, .. } if rate.is_finite() && *rate > 0.)
                            && presentation.color[3] == 255
                            && presentation.camera_distance.is_finite()
                            && presentation.camera_distance >= 0.
                            && presentation.camera_elevation.is_finite()
                            && (0. ..90.).contains(&presentation.camera_elevation),
                        "invalid stored recovery spell presentation or result"
                    );
                    casters
                }
                TechniqueProgram::RecoverySpell {
                    casters,
                    lifetime,
                    pulses,
                } => {
                    ensure!(
                        *lifetime != 0
                            && !pulses.is_empty()
                            && pulses
                                .iter()
                                .all(|pulse| pulse.tick <= *lifetime && pulse.percent <= 100)
                            && pulses.windows(2).all(|pair| pair[0].tick <= pair[1].tick),
                        "invalid recovery spell schedule or caster bindings"
                    );
                    casters
                }
                _ => continue,
            };
            ensure!(
                !casters.is_empty()
                    && casters
                        .iter()
                        .map(|cast| cast.character)
                        .collect::<BTreeSet<_>>()
                        .len()
                        == casters.len(),
                "invalid recovery spell caster bindings"
            );
            for cast in casters {
                ensure!(
                    (1..=9).contains(&cast.character)
                        && !cast.animations.is_empty()
                        && matches!(cast.pulse, 3..=5)
                        && matches!(cast.release_effect, 7..=9)
                        && cast
                            .commands
                            .windows(2)
                            .all(|pair| pair[0].tick <= pair[1].tick)
                        && cast.release.map_or(
                            technique.program.is_summon(),
                            |command| matches!(command, AnimationCommand::Play { rate, .. } if rate.is_finite() && rate > 0.),
                        ),
                    "invalid recovery spell cast recipe"
                );
                ensure!(
                    cast.recovery_pose.is_none_or(|command| matches!(
                        command,
                        AnimationCommand::Play {
                            clip: 1..=127,
                            blend: 4,
                            start: 0,
                            end: None,
                            layer: 8,
                            looping: false,
                            mirror: false,
                            resource: -1,
                            rate: 0.5,
                        }
                    )),
                    "invalid spell recovery pose"
                );
            }
        }
        for action in self.all_actions() {
            action.validate(self.projectiles.len())?;
        }
        Ok(())
    }

    fn owned_actions(&self) -> impl Iterator<Item = (EffectBank, &Action)> {
        self.party
            .iter()
            .flat_map(|p| p.normal.iter().map(|n| (EffectBank::Techniques, &n.action)))
            .chain(self.enemies.iter().flat_map(|e| {
                e.actions
                    .iter()
                    .map(|a| (EffectBank::Enemy(e.monster), &a.action))
            }))
            .chain(
                self.techniques
                    .iter()
                    .flat_map(|t| match &t.program {
                        TechniqueProgram::Martial { variants } => variants.as_slice(),
                        _ => &[],
                    })
                    .map(|v| (EffectBank::Techniques, &v.action)),
            )
    }

    pub fn all_actions(&self) -> impl Iterator<Item = &Action> {
        self.owned_actions().map(|(_, action)| action)
    }

    pub fn impact_programs(&self) -> Result<BTreeSet<EffectId>> {
        let mut effects = BTreeSet::new();
        for (owner, action) in self.owned_actions() {
            effects.extend(action.impact_programs(owner)?);
        }
        for technique in &self.techniques {
            if let TechniqueProgram::Martial { variants } = &technique.program {
                for phase in variants {
                    if let Some(callback) = &phase.callback {
                        for rule in callback.rules() {
                            effects.extend(rule.impact_program(EffectBank::Techniques)?);
                        }
                    }
                }
            }
            for (bank, rule) in technique.program.spell_rules() {
                effects.extend(match bank {
                    EffectBank::Magic(package) => {
                        rule.impact_program_from(EffectBank::Techniques, Some(package))?
                    }
                    _ => rule.impact_program(bank)?,
                });
            }
        }
        Ok(effects)
    }

    pub fn audio_ids(&self) -> (BTreeSet<u16>, BTreeSet<u16>) {
        let (mut sounds, mut voices) = (BTreeSet::new(), BTreeSet::new());
        for action in self.all_actions() {
            let (action_sounds, action_voices) = action.audio_ids();
            sounds.extend(action_sounds);
            voices.extend(action_voices);
        }
        for technique in &self.techniques {
            if let TechniqueProgram::Martial { variants } = &technique.program {
                for phase in variants {
                    if let Some(callback) = &phase.callback {
                        if matches!(callback, MartialCallback::Steal { .. }) {
                            voices.extend([0x80b3, 0x80b4]);
                        }
                        if let MartialCallback::Beast {
                            opening_voices: Some(ids),
                        } = callback
                        {
                            voices.extend(ids);
                        }
                        for rule in callback.rules() {
                            sounds.extend((rule.sound != 0).then_some(rule.sound));
                        }
                    }
                }
            }
            if let TechniqueProgram::FireBall {
                emissions,
                voices: casting,
                rule,
                ..
            } = &technique.program
            {
                sounds.extend(emissions.iter().map(|e| e.sound));
                sounds.extend((rule.sound != 0).then_some(rule.sound));
                voices.extend([casting.begin, casting.release]);
            }
            if matches!(
                technique.program,
                TechniqueProgram::StoredRecoverySpell { .. }
                    | TechniqueProgram::AcidRain { .. }
                    | TechniqueProgram::Nurse { .. }
                    | TechniqueProgram::Spread { .. }
                    | TechniqueProgram::Water { .. }
                    | TechniqueProgram::AirThrust { .. }
                    | TechniqueProgram::WindField { .. }
                    | TechniqueProgram::EarthField { .. }
                    | TechniqueProgram::IceTornado { .. }
                    | TechniqueProgram::ThunderArrow { .. }
                    | TechniqueProgram::Orb { .. }
                    | TechniqueProgram::Lance { .. }
                    | TechniqueProgram::Ray { .. }
                    | TechniqueProgram::PrismSword { .. }
                    | TechniqueProgram::GroundPulse { .. }
                    | TechniqueProgram::Absolute { .. }
                    | TechniqueProgram::EarthBite { .. }
                    | TechniqueProgram::MeteorStorm { .. }
                    | TechniqueProgram::SpiralFlare { .. }
                    | TechniqueProgram::FreezeLancer { .. }
                    | TechniqueProgram::FireField { .. }
            ) {
                sounds.insert(122);
            }
            if let TechniqueProgram::FreezeLancer { recipe, .. } = &technique.program {
                sounds.insert(recipe.sound);
            }
            for (_, rule) in technique.program.spell_rules() {
                sounds.extend((rule.sound != 0).then_some(rule.sound));
            }
            if let TechniqueProgram::RecoverySpell { casters, .. }
            | TechniqueProgram::StoredRecoverySpell { casters, .. }
            | TechniqueProgram::Nurse { casters, .. }
            | TechniqueProgram::Lightning { casters, .. }
            | TechniqueProgram::Icicle { casters, .. }
            | TechniqueProgram::StoneBlast { casters, .. }
            | TechniqueProgram::WindBlade { casters, .. }
            | TechniqueProgram::AirThrust { casters, .. }
            | TechniqueProgram::WindField { casters, .. }
            | TechniqueProgram::AquaEdge { casters, .. }
            | TechniqueProgram::Spread { casters, .. }
            | TechniqueProgram::Water { casters, .. }
            | TechniqueProgram::EarthField { casters, .. }
            | TechniqueProgram::IceTornado { casters, .. }
            | TechniqueProgram::ThunderArrow { casters, .. }
            | TechniqueProgram::Orb { casters, .. }
            | TechniqueProgram::Lance { casters, .. }
            | TechniqueProgram::Ray { casters, .. }
            | TechniqueProgram::PrismSword { casters, .. }
            | TechniqueProgram::GroundPulse { casters, .. }
            | TechniqueProgram::Absolute { casters, .. }
            | TechniqueProgram::EarthBite { casters, .. }
            | TechniqueProgram::MeteorStorm { casters, .. }
            | TechniqueProgram::SpiralFlare { casters, .. }
            | TechniqueProgram::FreezeLancer { casters, .. }
            | TechniqueProgram::FireField { casters, .. }
            | TechniqueProgram::GroundSummon { casters, .. }
            | TechniqueProgram::Summon { casters, .. } = &technique.program
            {
                sounds.extend([109, 122, 123]);
                for cast in casters {
                    voices.extend([
                        cast.voices.begin,
                        cast.voices.release,
                        cast.self_voices.begin,
                    ]);
                    for command in &cast.commands {
                        match command.command {
                            ActionCommand::Sound(id) => {
                                sounds.insert(id);
                            }
                            ActionCommand::Voice { id, .. } => {
                                voices.insert(id);
                            }
                            ActionCommand::RandomVoice { first, second, .. } => {
                                voices.extend([first, second]);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        for technique in &self.techniques {
            match &technique.program {
                TechniqueProgram::Summon { recipe, .. } => {
                    voices.insert(recipe.kind.voice());
                }
                TechniqueProgram::GroundSummon { recipe, .. } => {
                    voices.extend(recipe.voices().map(|(_, voice)| voice));
                    sounds.extend(recipe.sound.map(|sound| sound.id));
                }
                _ => {}
            }
        }
        for enemy in &self.enemies {
            if !enemy.native_spells().is_empty() {
                sounds.extend([109, 123]);
            }
            for cast in enemy.casting.values() {
                voices.extend(
                    [cast.voices.begin, cast.voices.release]
                        .into_iter()
                        .filter(|id| *id != 0),
                );
                for step in &cast.commands {
                    match step.command {
                        ActionCommand::Voice { id, .. } => {
                            voices.insert(id);
                        }
                        ActionCommand::Sound(id) => {
                            sounds.insert(id);
                        }
                        ActionCommand::RandomVoice { first, second, .. } => {
                            voices.extend([first, second]);
                        }
                        _ => {}
                    }
                }
            }
            if let Some(EnemyNativePolicy::BossPresentation { opening: voice, .. }) =
                enemy.policy.as_ref().and_then(|policy| policy.native)
                && voice.id != 0
            {
                voices.insert(voice.id);
            }
        }
        for recovery in self
            .enemies
            .iter()
            .flat_map(|enemy| &enemy.actions)
            .filter_map(|action| action.contact_recovery.as_ref())
        {
            for command in &recovery.commands {
                if let ActionCommand::Voice { id, .. } = command {
                    voices.insert(*id);
                }
            }
        }
        (sounds, voices)
    }
}

impl Action {
    pub fn validate(&self, projectile_motions: usize) -> Result<()> {
        ensure!(!self.animations.is_empty(), "action has no animation");
        // Commands execute serially, so their thresholds need not be sorted.
        for step in &self.commands {
            if let ActionCommand::ApplyConditions { conditions, .. } = step.command {
                conditions.validate()?;
            }
            if let ActionCommand::CameraBounds {
                distance,
                elevation,
                ..
            } = step.command
            {
                ensure!(
                    distance.is_finite() && elevation.is_finite(),
                    "invalid camera bounds"
                );
            }
            if let ActionCommand::BoneScale { slot, scale, .. } = step.command {
                ensure!(slot < 8 && scale.is_finite(), "invalid bone scale command");
            }
            if let ActionCommand::AdvancePosition(value)
            | ActionCommand::TurnHeading(value)
            | ActionCommand::TurnMotion(value) = step.command
            {
                ensure!(value.is_finite(), "invalid action displacement or turn");
            }
        }
        for hit in &self.hits {
            ensure!(
                hit.shape.radius.is_finite()
                    && hit.shape.height.is_finite()
                    && hit.shape.radius >= 0.
                    && hit.shape.height >= 0.,
                "invalid hit shape"
            );
            if let HitEmission::Projectile { motion, .. } = hit.emission {
                ensure!(
                    usize::from(motion) < projectile_motions,
                    "missing projectile motion"
                );
            }
        }
        for animation in self.animations.commands() {
            if let AnimationCommand::Play { rate, .. } | AnimationCommand::Rate { rate } = animation
            {
                ensure!(rate.is_finite(), "invalid animation rate");
            }
        }
        Ok(())
    }

    pub fn impact_programs(&self, owner: EffectBank) -> Result<BTreeSet<EffectId>> {
        let mut programs = BTreeSet::new();
        for hit in &self.hits {
            programs.extend(hit.rule.impact_program(owner)?);
        }
        Ok(programs)
    }

    pub fn audio_ids(&self) -> (BTreeSet<u16>, BTreeSet<u16>) {
        let mut sounds = self
            .hits
            .iter()
            .map(|hit| hit.rule.sound)
            .filter(|&id| id != 0)
            .collect::<BTreeSet<_>>();
        let mut voices = BTreeSet::new();
        for command in &self.commands {
            match command.command {
                ActionCommand::Sound(id) => {
                    sounds.insert(id);
                }
                ActionCommand::Voice { id, .. } => {
                    voices.insert(id);
                }
                ActionCommand::RandomVoice { first, second, .. } => {
                    voices.extend([first, second]);
                }
                _ => {}
            }
        }
        (sounds, voices)
    }
}

impl HitRule {
    /// Bank zero follows the attacker; shared arte and enemy overrides are explicit.
    pub fn impact_program(self, owner: EffectBank) -> Result<Option<EffectId>> {
        self.impact_program_from(owner, None)
    }

    /// A spell's package stays with its flight even after the caster starts another action.
    /// It only resolves bank six; bank zero continues to follow the attacker's identity.
    pub fn impact_program_from(
        self,
        owner: EffectBank,
        magic: Option<u16>,
    ) -> Result<Option<EffectId>> {
        if self.impact_effect == 0 {
            return Ok(None);
        }
        let bank = match (self.impact_bank, owner, magic) {
            (0, EffectBank::Techniques | EffectBank::Enemy(_), _) => owner,
            (1, _, _) => EffectBank::Techniques,
            (2, EffectBank::Enemy(_), _) => owner,
            (6, _, Some(package)) => EffectBank::Magic(package),
            _ => anyhow::bail!(
                "invalid impact effect bank {} for {owner:?}",
                self.impact_bank
            ),
        };
        Ok(Some(EffectId {
            bank,
            id: self.impact_effect,
        }))
    }
}

#[cfg(test)]
mod martial_selection_tests {
    use super::*;

    #[test]
    fn tagged_technique_preserves_signed_and_shifted_animation_addresses() {
        let mut original = technique(43);
        let TechniqueProgram::Martial { variants } = &mut original.program else {
            unreachable!()
        };
        variants[0].action.animations = AnimationProgram {
            initial: None,
            instructions: [
                (-128, AnimationInstruction::Stalled),
                (4, AnimationInstruction::End),
            ]
            .into(),
            loop_targets: [(
                255,
                AnimationBinding {
                    time: -2,
                    command: AnimationCommand::Play {
                        clip: 55,
                        blend: 4,
                        start: 0,
                        end: None,
                        layer: 8,
                        looping: false,
                        mirror: false,
                        resource: -1,
                        rate: 1.,
                    },
                },
            )]
            .into(),
        };
        let bytes = serde_json::to_vec(&original).unwrap();
        let decoded: TechniqueAction = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(serde_json::to_vec(&decoded).unwrap(), bytes);
    }

    fn technique(native_id: u16) -> TechniqueAction {
        TechniqueAction {
            technique: 1,
            native_id,
            properties: TechniqueProperties {
                menu_target: TechniqueMenuTarget::Enemy,
                magic: false,
                cast_time_reducible: false,
                ranged: false,
                damage: true,
                hp_recovery: false,
                condition_change: false,
                incapacitated_target: false,
                physical_condition_recovery: false,
                magical_condition_recovery: false,
                utility: false,
                target_condition_mask: 0,
                target_preference: TechniqueTargetPreference::Individual,
                tactic: TechniqueTactic::Any,
                recovery_ticks: 1,
                approach: TechniqueApproach {
                    maximum: 0.,
                    ai_minimum: 0.,
                    short_weapon_penalty: false,
                },
                chain: TechniqueChain::None,
            },
            program: TechniqueProgram::Martial {
                variants: (0..3)
                    .map(|variant| TechniquePhase {
                        alternate: None,
                        variant,
                        recovery_ticks: 1,
                        buffer_until: 0,
                        combo_at: 0,
                        effect: None,
                        callback: None,
                        caption: None,
                        action: Action {
                            duration: 1,
                            tp: 0,
                            animations: Default::default(),
                            commands: vec![],
                            loop_commands: false,
                            hits: vec![],
                        },
                    })
                    .collect(),
            },
        }
    }

    #[test]
    fn martial_owner_dispatch_selects_only_the_applicable_body_program() {
        let shared = technique(1);
        for (owner, phase) in [(1, 0), (6, 1), (9, 2)] {
            assert_eq!(shared.martial_entry_variant(owner), Some(phase));
            assert_eq!(
                shared
                    .martial_phases(owner)
                    .unwrap()
                    .map(|phase| phase.variant)
                    .collect::<Vec<_>>(),
                [phase]
            );
        }
        let guard = technique(34);
        for owner in 1..=9 {
            assert_eq!(guard.martial_entry_variant(owner), Some(owner - 1));
        }
        assert!(guard.martial_phases(9).is_err());
        assert_eq!(shared.martial_entry_variant(0), None);
        assert_eq!(shared.martial_entry_variant(10), None);
        assert_eq!(technique(u16::MAX).martial_entry_variant(1), None);
        assert_eq!(technique(10).martial_entry_variant(2), None);
        assert_eq!(technique(81).martial_entry_variant(6), Some(0));
        assert_eq!(technique(81).martial_entry_variant(9), Some(1));
        assert!(technique(40).martial_phases(2).is_err());
    }
}
