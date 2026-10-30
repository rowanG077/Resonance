//! Actor templates. Session statistics and equipment are applied on loading.
use crate::{battle_conditions::ConditionSet, battle_voice::Sound};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod casting;
pub use casting::Casting;
mod category;
pub use category::{RecoilWeight, Size, Species};
mod expression;
pub use expression::{Blink, Expression, ExpressionTexture, FacialTexture};
mod shadow;
pub use shadow::Shadow;

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Table {
    /// Maintained standing capsules for characters 1..=9, separate from authored MO/DM geometry.
    pub body_colliders: [StandingCollider; 9],
    /// Seven directional normal attacks for each ordinary party character.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub normal_attacks: Vec<[crate::battle_normal::Attack; 7]>,
    /// Complete common-action tracks, with separate character bindings.
    pub martial_attacks: crate::battle_martial::Catalogue,
    /// Default strategy values, transposed from three ten-byte asset columns.
    pub default_strategy: [[u8; 3]; 9],
    pub companion_policy: CompanionPolicy,
    pub entry: Entry,
    /// Character-specific poses, scheduled in elapsed chanting updates.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub chant_sequences: BTreeMap<u8, Vec<(u16, crate::battle_action::ActionPose)>>,
    /// Character slots used to select chanting and release lines.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub voice_sequences: Vec<VoicePolicy>,
    /// Ordered victim/recipient character pairs used by the ordinary death entry.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub death_voice_pairs: Vec<[u8; 2]>,
    pub contact_sounds: ContactSounds,
    /// Over Limit sounds for characters 1..=9.
    pub overlimit_voices: [Option<Sound>; 9],
    /// Item-release sparkle offsets from each character's body center.
    pub item_use_offsets: [[f32; 3]; 9],
    /// Playable character IDs 1..=9.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub records: Vec<Profile>,
    /// Rescue names: Angel Tear, Revive, Resurrect, Ring, Doll.
    pub lethal_rescue_names: [String; 5],
}

impl Table {
    pub fn validate(&self) -> Result<()> {
        for collider in self.body_colliders {
            collider.validate()?;
        }
        Ok(())
    }

    pub fn martial_attack(
        &self,
        catalogue: u16,
        character: u8,
    ) -> Result<&crate::battle_martial::Attack> {
        self.martial_attacks.attack(catalogue, character)
    }
}

/// Fixed actor-local body envelope; posed reception markers remain authoritative for hits.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StandingCollider {
    pub radius: f32,
    pub height: f32,
}

impl StandingCollider {
    pub fn validate(self) -> Result<()> {
        ensure!(
            self.radius.is_finite()
                && self.radius > 0.
                && self.height.is_finite()
                && self.height / 2. >= self.radius,
            "invalid standing body collider"
        );
        Ok(())
    }
}

#[test]
fn standing_colliders_reject_non_finite_or_inverted_capsules() -> Result<()> {
    StandingCollider {
        radius: 24.,
        height: 110.,
    }
    .validate()?;
    StandingCollider {
        radius: 30.,
        height: 60.,
    }
    .validate()?;
    for (radius, height) in [
        (0., 100.),
        (-1., 100.),
        (30., 59.),
        (f32::NAN, 100.),
        (30., f32::INFINITY),
    ] {
        assert!(StandingCollider { radius, height }.validate().is_err());
    }
    Ok(())
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompanionPolicy {
    pub tp_limits: [u8; 9],
    pub healing_limits: [u8; 9],
}

/// Artwork settings for battle entry; gameplay owns the introduction duration.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub replacement_motion_rate: f32,
    /// RGB used by result fades.
    pub fade_color: [u8; 3],
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoicePolicy {
    pub default: VoiceSequence,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub techniques: BTreeMap<u16, VoiceSequence>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceSequence {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chant: Option<Sound>,
    pub chant_ticks: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_chant: Option<Sound>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release: Option<Sound>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback: Option<Sound>,
    pub fallback_ticks: u16,
}

/// Neutral party and elemental contact cue tables.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContactSounds {
    pub party: [u16; 9],
    pub elements: [u8; 8],
}

/// Actor capabilities resolved during asset import. False leaves ordinary behavior enabled.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileTraits {
    #[serde(default)]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub hosts_participants: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub flying: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hover_bobbing: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub turning_disabled: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub inactive_item_target: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub knockdown_immune: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub clear_pending_on_hit: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub show_body_attachments: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub auto_guard_disabled: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub retain_defeated_body: bool,
    #[serde(default)]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub wait_for_defeat_animation: bool,
    #[serde(default)]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub hit_shake_disabled: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub recover_in_air: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub launch_immune: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub immune_to_unison: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub body_motion_disabled: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub push_immovable: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unrestricted_arena: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub enemy_contact_recovery: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub passes_allied_obstacles: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub passable_for_allies: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub suppress_hurt_motion: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub push_obstacle_disabled: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub item_target_excluded: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub secondary_body_entry: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fixed_height: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeaponStyle {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub before_body: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub toon: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub additive: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TargetAnchor {
    Reference(TargetOrigin),
    Joint(HeadAnchor),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetOrigin {
    Head,
    Center,
}

impl Default for TargetAnchor {
    fn default() -> Self {
        Self::Reference(TargetOrigin::Head)
    }
}

impl TargetAnchor {
    /// Center targeting uses the actor's body center; it has no joint offset.
    pub fn joint<'a>(&'a self, head: &'a HeadAnchor) -> Option<&'a HeadAnchor> {
        match self {
            Self::Reference(TargetOrigin::Head) => Some(head),
            Self::Reference(TargetOrigin::Center) => None,
            Self::Joint(anchor) => Some(anchor),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeadAnchor {
    /// Joint name in the selected model's GLB, resolved for each costume.
    pub bone: String,
    /// Actor-local offset, scaled and turned independently of the head pose.
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub offset: [f32; 3],
}

/// A participant carried on its formation leader until that leader releases it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostedBody {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub release_on_hurt: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub release_below_half_health: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub release_on_defeat: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub launch_on_release: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub walk_speed: f32,
    pub run_speed: f32,
    pub turn_ticks: u8,
    /// Idle delay; entry uses only the random variation.
    pub idle_ticks: u8,
    #[serde(
        default = "idle_variation_default",
        skip_serializing_if = "idle_variation_is_default"
    )]
    pub idle_variation: u8,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    #[serde(with = "crate::battle_motion::byte")]
    pub initial_motion: u8,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    #[serde(with = "crate::battle_motion::byte")]
    pub initial_motion_override: u8,
    /// Initial primary body alpha, separate from material texture alpha.
    #[serde(
        default = "crate::defaults::opaque",
        skip_serializing_if = "crate::defaults::is_opaque"
    )]
    pub body_alpha: u8,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expressions: Vec<Expression>,
    /// Styles for every attachment slot, including those enabled by equipment.
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub weapon_styles: [WeaponStyle; 8],
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub initial_conditions: ConditionSet,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub intrinsic_conditions: ConditionSet,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub immunities: ConditionSet,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub recoil_weight: RecoilWeight,
    /// Actor species category used by weapon damage bonuses.
    pub species: Species,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub stun_resistance: u8,
    #[serde(
        default = "stagger_threshold_default",
        skip_serializing_if = "stagger_threshold_is_default"
    )]
    pub stagger_threshold: u8,
    #[serde(
        default = "stagger_ticks_default",
        skip_serializing_if = "stagger_ticks_is_default"
    )]
    pub stagger_ticks: u8,
    #[serde(
        default = "guard_reduction_default",
        skip_serializing_if = "guard_reduction_is_default"
    )]
    pub guard_reduction: u8,
    #[serde(
        default = "guard_pressure_limit_default",
        skip_serializing_if = "guard_pressure_limit_is_default"
    )]
    pub guard_pressure_limit: u32,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub traits: ProfileTraits,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hosted_body: Option<HostedBody>,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub armor: u8,
    pub center_offset: [f32; 3],
    /// Omitted targeting follows the head, including its actor-local offset.
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub target_anchor: TargetAnchor,
    pub head_anchor: HeadAnchor,
    #[serde(
        default = "crate::defaults::unit",
        skip_serializing_if = "crate::defaults::is_unit"
    )]
    pub model_scale: f32,
    #[serde(
        default = "shadow::default",
        skip_serializing_if = "shadow::is_default"
    )]
    pub shadow: Option<Shadow>,
    #[serde(
        default = "crate::defaults::unit",
        skip_serializing_if = "crate::defaults::is_unit"
    )]
    pub effect_scale: f32,
    /// Independent recurring effects attached to the actor's root.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub passive_effects: Vec<RecurringEffect>,
    /// Recurring effect while the actor rests between actions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_effect: Option<RecurringEffect>,
    /// Recurring effect while approaching a target or returning to formation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub movement_effect: Option<RecurringEffect>,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub size: Size,
    /// Named speech; absence denotes an unvoiced actor.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voices: Option<crate::battle_voice::Voices>,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    #[serde(with = "crate::battle_motion::byte")]
    pub death_motion: u8,
    /// Bone sampled once for the activation burst.
    pub overlimit_bone: String,
    #[serde(
        default = "crate::defaults::unit",
        skip_serializing_if = "crate::defaults::is_unit"
    )]
    pub overlimit_scale: f32,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub overlimit_gain: u8,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub initial_overlimit: i16,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub ground_offset: f32,
    /// Casting defaults are shared by the format; each actor only stores overrides.
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub casting: Casting,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RecurringEffect {
    pub program: u16,
    pub interval: u16,
}

fn idle_variation_default() -> u8 {
    30
}
fn idle_variation_is_default(value: &u8) -> bool {
    *value == 30
}

fn stagger_threshold_default() -> u8 {
    100
}
fn stagger_threshold_is_default(value: &u8) -> bool {
    *value == 100
}

fn stagger_ticks_default() -> u8 {
    60
}
fn stagger_ticks_is_default(value: &u8) -> bool {
    *value == 60
}

fn guard_reduction_default() -> u8 {
    75
}
fn guard_reduction_is_default(value: &u8) -> bool {
    *value == 75
}

fn guard_pressure_limit_default() -> u32 {
    10
}
fn guard_pressure_limit_is_default(value: &u32) -> bool {
    *value == 10
}
