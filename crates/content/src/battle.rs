//! Battle formations and resource closure. Monster statistics remain in the shared catalogue.
pub mod action_inventory;
pub mod action_program;
pub mod actions;
pub mod arte_inventory;
pub mod audio;
pub mod chains;
pub mod conditions;
pub mod contact_effects;
pub mod effect_inventory;
pub mod effect_program;
pub mod effects;
pub mod enemy_inventory;
pub mod entrance;
pub mod inventory;
pub mod items;
mod party_models;
pub mod pose;
pub mod projectile_modifiers;
pub mod ui;
pub mod unison;
pub mod victory_group;
pub mod visual;

use crate::{field_preload::File, monster::Monster, validate_asset_path};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const OVERLIMIT_THRESHOLD: u16 = 1_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BattleCatalog {
    pub version: u32,
    pub formations: Vec<Formation>,
    pub enemies: BTreeMap<u8, EnemyData>,
    pub party: BTreeMap<u8, PartyTraits>,
    pub party_layout: PartyLayout,
    pub entrance: entrance::EntranceRecipe,
    pub motion: MotionRules,
    pub contact_effects: contact_effects::ContactEffects,
    /// Level-only techniques that active combatants may learn automatically.
    /// Required levels and ownership remain in SessionData.
    pub active_level_techniques: BTreeSet<u16>,
    pub actions: actions::BattleActions,
    pub effects: effects::BattleEffects,
    pub effect_programs: effect_program::BattleEffectPrograms,
    pub projectile_modifiers: projectile_modifiers::ProjectileModifiers,
    pub visuals: visual::VisualAssets,
    pub audio: audio::BattleAudio,
    pub ui: ui::BattleUi,
    pub items: items::BattleItems,
    pub unison: unison::UnisonData,
    pub victory_groups: victory_group::Groups,
    /// Complete cooked dependencies, excluding this manifest itself.
    pub files: BTreeMap<String, File>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Formation {
    pub id: u16,
    pub arena: u16,
    #[serde(default)]
    pub placement: FormationPlacement,
    pub escape_allowed: bool,
    /// Ordinary party opening line; independent of enemy entrance callbacks.
    #[serde(default = "opening_voice_default")]
    pub opening_voice: bool,
    pub victory_music: bool,
    pub victory_camera: bool,
    pub victory_celebration: bool,
    pub enemies: Vec<EnemySpawn>,
}

fn opening_voice_default() -> bool {
    true
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FormationPlacement {
    #[default]
    Explicit,
    Generated {
        layout: GeneratedEnemyLayout,
    },
}

/// Enemy base-statistics row selector, retained across statistic variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnemyPlacementRow {
    Random,
    Front,
    Middle,
    Back,
}

impl TryFrom<u8> for EnemyPlacementRow {
    type Error = anyhow::Error;

    fn try_from(value: u8) -> Result<Self> {
        [Self::Random, Self::Front, Self::Middle, Self::Back]
            .get(usize::from(value))
            .copied()
            .context("invalid enemy placement row")
    }
}

/// Generated enemy formation coordinates, in front-to-back row order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeneratedEnemyLayout {
    pub row_x: [f32; 3],
    pub member_offset: [f32; 2],
    pub back_row_x_correction: f32,
    pub center_z: f32,
    pub single_z: [f32; 3],
    pub main_z: [f32; 8],
}

impl GeneratedEnemyLayout {
    pub fn validate(&self, rows: &[EnemyPlacementRow]) -> Result<()> {
        ensure!(
            (1..=8).contains(&rows.len()),
            "invalid generated enemy count"
        );
        ensure!(
            self.row_x
                .iter()
                .chain(&self.member_offset)
                .chain(&self.single_z)
                .chain(&self.main_z)
                .chain([&self.back_row_x_correction, &self.center_z])
                .all(|v| v.is_finite()),
            "invalid generated enemy layout"
        );
        let mut counts = [0_usize; 3];
        let mut random = 0_usize;
        for row in rows {
            match row {
                EnemyPlacementRow::Random => random += 1,
                EnemyPlacementRow::Front => counts[0] += 1,
                EnemyPlacementRow::Middle => counts[1] += 1,
                EnemyPlacementRow::Back => counts[2] += 1,
            }
        }
        // The original pointer groups start four entries apart, but a row can
        // extend into an empty following group without changing any placement.
        for pair in counts.windows(2) {
            let needed = 5_usize.saturating_sub(pair[0]);
            ensure!(
                random < needed || (pair[1] == 0 && random == needed),
                "generated enemy rows can overwrite another occupied row"
            );
        }
        // The seventh back-row pointer overwrites the first main-row z value.
        ensure!(
            counts[2] + random <= 6,
            "generated enemy back row can overwrite placement coordinates"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EnemySpawn {
    pub monster: u8,
    /// Index into Monster.statistics and EnemyData.variants; zero is the base record.
    pub variant: u8,
    /// Vertical row of the body's optional appearance atlas, independent of statistics.
    #[serde(default)]
    pub texture_variant: u8,
    /// Whether this encounter reveals the monster's name in target selection.
    #[serde(default)]
    pub name_visible: bool,
    /// Horizontal world coordinates, x and z. The model supplies vertical placement.
    pub position: [f32; 2],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnemyData {
    pub family: Family,
    pub immune_to_unison: bool,
    /// Encounter concealment uses the source name's width, without changing the bestiary.
    pub concealed_name: String,
    /// Selects the warning line for an unusually dangerous enemy at entry.
    pub opening_warning: bool,
    pub death: DeathStyle,
    pub target_policy: EnemyTargetPolicy,
    /// Required by generated formations; absent in older explicit-only catalogues.
    #[serde(default)]
    pub placement_row: Option<EnemyPlacementRow>,
    /// Excluded when cycling manually through enemy targets.
    pub manual_target_excluded: bool,
    pub flying: bool,
    pub idle_delay: u8,
    pub idle_jitter: u8,
    pub finish_waits_for_animation: bool,
    pub reaction: ReactionTraits,
    pub defense: DefenseTraits,
    pub stun: StunTraits,
    pub conditions: conditions::ConditionTraits,
    pub overlimit_rate: u8,
    pub overlimit_initial: u16,
    /// Authored skeleton index; indices outside the model select its root bone.
    pub overlimit_bone: u8,
    pub overlimit_scale: f32,
    /// Both item slots compare their thresholds against the same roll in 0..100.
    /// Item IDs, EXP and gald are defined by the corresponding Monster record.
    pub drop_chances: [u8; 2],
    pub steal_chance: u8,
    pub grade: i16,
    pub variants: Vec<EnemyTraits>,
    pub guard: GuardTraits,
    pub hit_radius_scale: f32,
    pub effect_scale: f32,
    pub ground_offset: f32,
    pub effect_offset: [f32; 3],
    pub movement: Movement,
    pub physical_affinity: ElementAffinity,
    /// Ordered as menu_data::Element::ALL.
    pub affinities: [ElementAffinity; 8],
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct EnemyTraits {
    pub level: u8,
    pub thrust: u16,
    pub intelligence: u16,
    pub accuracy: u16,
    pub evasion: u16,
    pub luck: u8,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct GuardTraits {
    pub reduction_percent: u8,
    pub pressure_limit: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NormalFacing {
    Target,
    Fixed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PartyTraits {
    pub family: Family,
    pub immune_to_unison: bool,
    pub death: DeathStyle,
    pub combo_limit: u8,
    pub idle_delay: u8,
    pub idle_jitter: u8,
    pub finish_waits_for_animation: bool,
    pub reaction: ReactionTraits,
    pub defense: DefenseTraits,
    pub stun: StunTraits,
    pub conditions: conditions::ConditionTraits,
    pub guard_reduction_percent: u8,
    pub hit_radius_scale: f32,
    pub effect_scale: f32,
    pub overlimit_scale: f32,
    pub casting_base: i16,
    /// Ordinary automatic casts overlap this concluding pose with the chant countdown.
    #[serde(default)]
    pub casting_conclusion: Option<actions::CastingConclusion>,
    pub effect_offset: [f32; 3],
    pub movement: Movement,
    /// Missing metadata must be recovered before admitting a prepared normal attack.
    #[serde(default)]
    pub normal_facing: Option<NormalFacing>,
    /// Authored half-turn duration for automatic approach steering.
    #[serde(default)]
    pub turn_divisor: Option<u8>,
}

/// Numeric creature family, shared by actor metadata and weapon damage traits.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Family(pub u16);

impl Family {
    pub fn from_weapon_trait(effect: u8) -> Option<Self> {
        (97..=108)
            .contains(&effect)
            .then(|| Self(u16::from(effect - 96)))
    }
}

#[test]
fn weapon_family_traits_preserve_original_numeric_categories() {
    for (effect, family) in [(97, 1), (99, 3), (102, 6), (106, 10), (108, 12)] {
        assert_eq!(Family::from_weapon_trait(effect), Some(Family(family)));
    }
    for effect in [0, 96, 109, u8::MAX] {
        assert_eq!(Family::from_weapon_trait(effect), None);
    }
}

/// Defeat presentation is authored independently of combat availability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DeathStyle {
    Fade,
    Collapse {
        clip: u8,
    },
    /// Enemy remains visible in its default collapse pose and gradually darkens.
    Corpse,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Movement {
    pub walk_speed: f32,
    pub run_speed: f32,
    pub gravity: f32,
    /// Older catalogs lack body separation metadata; callers must require it explicitly.
    #[serde(default)]
    pub body: Option<BodyTraits>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct BodyTraits {
    pub resting_height: f32,
    pub freeze_height: bool,
    pub immovable: bool,
    pub excluded: bool,
    pub pairing: BodyPairing,
    pub ignores_boundary: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BodyPairing {
    Independent,
    Primary,
    Secondary,
    Combined,
}

impl BodyPairing {
    pub fn excludes(self, other: Self) -> bool {
        use BodyPairing::*;
        matches!(
            (self, other),
            (Primary | Combined, Secondary | Combined) | (Secondary | Combined, Primary | Combined)
        )
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct ReactionTraits {
    pub weight: ReactionWeight,
    pub restrain_launch: bool,
    pub unlaunchable: bool,
    pub no_knockback: bool,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReactionWeight {
    #[default]
    Standard,
    Light,
    Heavy,
    Grounded,
}

impl TryFrom<u8> for ReactionWeight {
    type Error = anyhow::Error;
    fn try_from(value: u8) -> Result<Self> {
        match value {
            0 => Ok(Self::Standard),
            1 => Ok(Self::Light),
            2 => Ok(Self::Heavy),
            3 => Ok(Self::Grounded),
            _ => anyhow::bail!("unknown actor reaction weight {value}"),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct DefenseTraits {
    pub poise: u8,
    pub stun_resistance: u8,
    pub stagger_threshold: u8,
    pub stagger_ticks: u8,
    pub auto_guard_disabled: bool,
    pub fixed_one_damage: bool,
    pub quarter_damage: bool,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct StunTraits {
    pub motion: StunMotion,
    pub immune: bool,
    pub half_duration: bool,
    pub head_bone: u16,
    pub offset: [f32; 3],
    pub face: [u8; 4],
    pub idle_face: [u8; 4],
}

/// A null authored stun slot leaves the actor's current motion advancing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StunMotion {
    #[default]
    Loop,
    KeepCurrent,
}

impl StunTraits {
    pub fn validate(&self, visual: &visual::ModelVisuals) -> Result<()> {
        ensure!(
            self.offset.iter().all(|v| v.is_finite())
                && usize::from(self.head_bone) < visual.rig.skeleton.bones.len(),
            "invalid stun attachment"
        );
        ensure!(
            (self.motion == StunMotion::Loop) == visual.rig.motions.contains_key(&21),
            "stun motion declaration does not match its resource"
        );
        if !self.immune {
            // Recovery requests get-up only from the knocked-down motion.
            ensure!(
                !visual.rig.motions.contains_key(&7) || visual.rig.motions.contains_key(&9),
                "missing stun get-up motion"
            );
            ensure!(
                visual
                    .texture_layers
                    .iter()
                    .enumerate()
                    .all(|(index, layer)| self.face[index] < layer.frames
                        && self.idle_face[index] < layer.frames),
                "missing stun face frame"
            );
        }
        Ok(())
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnemyTargetPolicy {
    Nearest = 1,
    Farthest = 2,
    Same = 3,
    Spread = 4,
    AimHigh = 5,
    BlockMagic = 6,
    Reduce = 7,
    ProtectFriend = 8,
    SelfTarget = 9,
    Random = 10,
    NearestIncapacitated = 11,
}

impl TryFrom<u8> for EnemyTargetPolicy {
    type Error = anyhow::Error;
    fn try_from(value: u8) -> Result<Self> {
        match value {
            1 => Ok(Self::Nearest),
            2 => Ok(Self::Farthest),
            3 => Ok(Self::Same),
            4 => Ok(Self::Spread),
            5 => Ok(Self::AimHigh),
            6 => Ok(Self::BlockMagic),
            7 => Ok(Self::Reduce),
            8 => Ok(Self::ProtectFriend),
            9 => Ok(Self::SelfTarget),
            10 => Ok(Self::Random),
            11 => Ok(Self::NearestIncapacitated),
            _ => anyhow::bail!("unsupported enemy target policy {value}"),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct MotionRules {
    pub acceleration: f32,
    pub action_drag: f32,
    pub drag_deadzone: f32,
    pub stop_epsilon: f64,
    /// Authored horizontal and vertical impulse pairs, indexed by reaction ID.
    pub reactions: [[f32; 2]; 19],
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ElementAffinity {
    Neutral,
    Weak,
    Resist,
    Absorb,
    Immune,
}

impl TryFrom<u8> for ElementAffinity {
    type Error = anyhow::Error;
    fn try_from(value: u8) -> Result<Self> {
        [
            Self::Neutral,
            Self::Weak,
            Self::Resist,
            Self::Absorb,
            Self::Immune,
        ]
        .get(usize::from(value))
        .copied()
        .context("invalid battle element affinity")
    }
}

/// Party strategy chooses one of three rows; members keep roster order within each row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartyLayout {
    pub default_rows: [u8; 9],
    pub row_x: [f32; 3],
    pub member_offset: [f32; 2],
    pub center_z: f32,
    pub single_z: [f32; 3],
    pub leader_z: [f32; 4],
}

impl BattleCatalog {
    /// Signed animation programs require battle metadata to be prepared again.
    pub const VERSION: u32 = 4;
    pub const PATH: &str = "battle/catalog.json";

    pub fn formation(&self, id: u16, arena: u16) -> Result<&Formation> {
        find_formation(&self.formations, id, arena)
    }

    pub fn validate(&self, monsters: &[Monster]) -> Result<()> {
        ensure!(
            self.version == Self::VERSION,
            "unsupported battle catalogue"
        );
        self.effects.validate()?;
        self.entrance.validate()?;
        self.effect_programs.validate()?;
        ensure!(
            self.contact_effects.elemental[0] == 0,
            "invalid neutral contact event"
        );
        for effect in self.contact_effects.programs() {
            ensure!(
                self.effect_programs
                    .programs
                    .iter()
                    .any(|program| program.id == effect),
                "missing shared contact effect {effect:?}"
            );
        }
        validate_encounters(&self.formations)?;
        for formation in &self.formations {
            ensure!(
                (1..=8).contains(&formation.enemies.len())
                    && self.visuals.arenas.contains_key(&formation.arena),
                "invalid battle enemy count or missing arena"
            );
            for spawn in &formation.enemies {
                ensure!(
                    spawn.position.iter().all(|v| v.is_finite()),
                    "invalid battle enemy position"
                );
                let enemy = self
                    .enemies
                    .get(&spawn.monster)
                    .context("missing enemy data")?;
                ensure!(
                    usize::from(spawn.variant) < enemy.variants.len()
                        && self.visuals.enemies.contains_key(&spawn.monster),
                    "unknown battle enemy variant or missing model"
                );
                ensure!(
                    self.visuals.enemies[&spawn.monster]
                        .variant_texture
                        .as_ref()
                        .is_none_or(|layer| spawn.texture_variant < layer.frames),
                    "enemy texture variant exceeds its body atlas"
                );
            }
            if let FormationPlacement::Generated { layout } = &formation.placement {
                let rows = formation
                    .enemies
                    .iter()
                    .map(|spawn| {
                        self.enemies[&spawn.monster]
                            .placement_row
                            .context("missing generated enemy placement row")
                    })
                    .collect::<Result<Vec<_>>>()?;
                layout.validate(&rows)?;
            }
        }
        for (&id, enemy) in &self.enemies {
            ensure!(
                matches!(enemy.death, DeathStyle::Fade | DeathStyle::Corpse),
                "custom enemy collapse shading is not implemented"
            );
            if enemy.death == DeathStyle::Corpse {
                ensure!(
                    self.visuals
                        .enemies
                        .get(&id)
                        .is_some_and(|v| v.rig.motions.contains_key(&7)),
                    "missing enemy collapse motion {id}"
                );
                ensure!(
                    self.formations
                        .iter()
                        .filter(|f| f.enemies.iter().any(|s| s.monster == id))
                        .all(|f| self.visuals.arenas[&f.arena].actor_ambient.is_some()),
                    "missing persistent enemy lighting {id}"
                );
            }
            ensure!(
                self.visuals
                    .enemies
                    .get(&id)
                    .is_some_and(|v| v.rig.motions.contains_key(&3)),
                "missing enemy defeat motion {id}"
            );
            let monster = monsters
                .iter()
                .find(|m| m.id == id)
                .context("missing monster record")?;
            ensure!(
                !enemy.variants.is_empty()
                    && enemy.variants.len() == monster.statistics.len()
                    && enemy.guard.reduction_percent <= 100
                    && enemy.hit_radius_scale.is_finite()
                    && enemy.hit_radius_scale > 0.
                    && enemy.effect_scale.is_finite()
                    && enemy.effect_scale > 0.
                    && enemy.ground_offset.is_finite()
                    && enemy.effect_offset.iter().all(|v| v.is_finite()),
                "battle and monster variants differ for monster {id}"
            );
            // A meter with no gain and a sub-threshold initial value cannot
            // activate; its unused aura scale may remain zero in the source.
            ensure!(
                enemy.overlimit_scale.is_finite()
                    && enemy.overlimit_scale >= 0.
                    && (enemy.overlimit_rate == 0 && enemy.overlimit_initial < OVERLIMIT_THRESHOLD
                        || enemy.overlimit_scale > 0.),
                "invalid overlimit scale for monster {id}"
            );
            ensure!(
                self.files.contains_key(&format!("monsters/{id:03}.json")),
                "missing battle monster dependency"
            );
            ensure!(
                self.ui.enemy_icons.contains_key(&id),
                "missing battle enemy icon {id}"
            );
        }
        for (&character, traits) in &self.party {
            ensure!(
                traits.normal_facing.is_some(),
                "missing normal facing for character {character}"
            );
            ensure!(
                traits.normal_facing == Some(NormalFacing::Fixed)
                    || traits.turn_divisor.is_some_and(|divisor| divisor != 0),
                "missing normal turn divisor for character {character}"
            );
        }
        for conditions in self
            .enemies
            .values()
            .map(|e| e.conditions)
            .chain(self.party.values().map(|p| p.conditions))
        {
            conditions.validate()?;
        }
        for movement in self
            .enemies
            .values()
            .map(|e| e.movement)
            .chain(self.party.values().map(|p| p.movement))
        {
            ensure!(
                [movement.walk_speed, movement.run_speed]
                    .iter()
                    .all(|v| v.is_finite() && *v > 0.),
                "invalid battle movement speed"
            );
            if let Some(body) = movement.body {
                ensure!(
                    body.resting_height.is_finite(),
                    "invalid body resting height"
                );
            }
            ensure!(
                movement.gravity.is_finite() && movement.gravity <= 0.,
                "invalid battle gravity"
            );
        }
        ensure!(
            [
                self.motion.acceleration,
                self.motion.action_drag,
                self.motion.drag_deadzone
            ]
            .iter()
            .all(|v| v.is_finite() && *v > 0.)
                && self.motion.stop_epsilon.is_finite()
                && self.motion.stop_epsilon > 0.,
            "invalid battle motion rules"
        );
        ensure!(
            self.motion
                .reactions
                .iter()
                .flatten()
                .all(|v| v.is_finite()),
            "invalid battle reaction impulses"
        );
        for (&id, traits) in &self.party {
            let DeathStyle::Collapse { clip } = traits.death else {
                anyhow::bail!("disappearing party defeat is not implemented");
            };
            let variants = self
                .visuals
                .party
                .get(&id)
                .context("missing party visual inventory")?;
            variants.validate(id)?;
            for (costume, model) in variants.concrete() {
                ensure!(
                    [3, u16::from(clip)].iter().all(|clip| model
                        .visual
                        .rig
                        .motions
                        .contains_key(clip)),
                    "missing party defeat motion {id}:{costume:?}"
                );
                let mut stun = traits.stun;
                stun.head_bone = model.head_bone;
                stun.validate(&model.visual)
                    .with_context(|| format!("party stun {id}:{costume:?}"))?;
            }
            ensure!(
                self.visuals.party.contains_key(&id)
                    && (1..=9).contains(&id)
                    && traits.combo_limit > 0
                    && traits.guard_reduction_percent <= 100
                    && traits.casting_base >= 0
                    && traits.casting_conclusion.is_some_and(|conclusion| matches!(
                        conclusion.animation, actions::AnimationCommand::Play {
                            clip: 12, start: 4, end: None, layer: 8, mirror: false,
                            resource: -1, rate, ..
                        } if rate.is_finite() && rate > 0.))
                    && traits.effect_offset.iter().all(|v| v.is_finite())
                    && traits.hit_radius_scale.is_finite()
                    && traits.hit_radius_scale > 0.,
                "invalid battle party traits"
            );
            ensure!(
                traits.effect_scale.is_finite()
                    && traits.effect_scale > 0.
                    && traits.overlimit_scale.is_finite()
                    && traits.overlimit_scale > 0.,
                "invalid battle party traits"
            );
        }
        for (id, enemy) in &self.enemies {
            enemy
                .stun
                .validate(&self.visuals.enemies[id])
                .with_context(|| format!("enemy stun {id}"))?;
        }
        ensure!(
            self.party.keys().eq(self.visuals.party.keys()),
            "party trait and visual owners differ"
        );
        let layout = &self.party_layout;
        ensure!(
            layout.default_rows.iter().all(|&r| r < 3)
                && layout
                    .row_x
                    .iter()
                    .chain(&layout.member_offset)
                    .chain(&layout.single_z)
                    .chain(&layout.leader_z)
                    .chain([&layout.center_z])
                    .all(|v| v.is_finite()),
            "invalid battle party layout"
        );
        self.actions.validate()?;
        if self
            .actions
            .techniques
            .iter()
            .any(|arte| matches!(arte.program, actions::TechniqueProgram::Martial { .. }))
        {
            let chains = self
                .actions
                .chains
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("missing martial chain rules"))?;
            ensure!(
                self.actions
                    .techniques
                    .iter()
                    .filter(|arte| matches!(
                        arte.program,
                        actions::TechniqueProgram::Martial { .. }
                    ))
                    .all(|arte| chains.artes.contains_key(&arte.technique)),
                "missing martial chain arte rules"
            );
        }
        for effect in self
            .actions
            .impact_programs()?
            .into_iter()
            .chain(self.actions.chain_effects())
        {
            ensure!(
                self.effect_programs.programs.iter().any(|p| p.id == effect),
                "missing authored impact effect {effect:?}"
            );
        }
        ensure!(
            self.actions
                .party
                .iter()
                .map(|p| p.character)
                .collect::<BTreeSet<_>>()
                == self.party.keys().copied().collect(),
            "party traits and action owners differ"
        );
        ensure!(
            self.actions
                .enemies
                .iter()
                .map(|e| e.monster)
                .collect::<BTreeSet<_>>()
                == self.enemies.keys().copied().collect(),
            "enemy traits and action owners differ"
        );
        self.projectile_modifiers.validate()?;
        ensure!(
            self.active_level_techniques
                .iter()
                .all(|&id| id > 0 && id <= u16::from(u8::MAX)),
            "invalid active-member level technique"
        );
        self.visuals.validate()?;
        self.validate_party_models()?;
        self.effect_programs
            .validate_models(&self.visuals.effect_models)?;
        for recipe in &self.effects.projectiles {
            if matches!(recipe.behavior.aim, effects::ProjectileAim::Bones { .. })
                && let Some(effects::EffectId {
                    bank: effects::EffectBank::Enemy(owner),
                    ..
                }) = recipe.id
            {
                ensure!(
                    self.visuals.enemies.contains_key(&owner),
                    "missing projectile bone-aim owner model {owner}"
                );
            }
        }
        self.audio.validate()?;
        ensure!(
            self.audio
                .audio
                .sounds
                .contains_key(&(entrance::SHATTER_SOUND as i16)),
            "battle entrance sound is not prepared"
        );
        for sound in self.actions.audio_ids().0 {
            ensure!(
                i16::try_from(sound).is_ok_and(|id| self.audio.audio.sounds.contains_key(&id)),
                "missing action or impact sound {sound}"
            );
        }
        self.ui.validate()?;
        self.items.validate()?;
        self.validate_unison()?;
        self.victory_groups.validate()?;
        for path in self
            .visuals
            .scenes()
            .flat_map(|s| std::iter::once(&s.mesh).chain(&s.textures))
            .map(String::as_str)
            .chain([
                self.visuals.toon_ramp.as_str(),
                self.visuals.shadow_texture.as_str(),
            ])
            .chain(self.visuals.trail_textures().map(|t| t.path.as_str()))
            .chain(self.effect_programs.assets())
            .chain(self.ui.assets())
            .chain(
                self.audio
                    .audio
                    .music
                    .values()
                    .chain(self.audio.audio.sounds.values())
                    .map(|a| a.path.as_str()),
            )
            .chain(
                self.audio
                    .audio
                    .voices
                    .values()
                    .map(|v| v.asset.path.as_str()),
            )
        {
            ensure!(
                self.files.contains_key(path),
                "missing battle dependency {path}"
            );
        }
        for (path, file) in &self.files {
            validate_asset_path(path)?;
            ensure!(
                path != Self::PATH
                    && file.sha256.len() == 64
                    && file.sha256.bytes().all(|b| b.is_ascii_hexdigit())
                    && !file.roles.is_empty(),
                "invalid battle dependency {path}"
            );
        }
        Ok(())
    }

    fn validate_pow_weapons(&self) -> Result<()> {
        use actions::ActionCommand;
        use effects::{EffectBank, EffectId};
        let party = self.party.keys().copied().collect::<Vec<_>>();
        for (kind, data) in &self.unison.pow {
            if !kind.selected(&party) {
                continue;
            }
            let slots = if *kind == unison::PowWeapon::Blade {
                2
            } else {
                1
            };
            let weapon = self
                .visuals
                .pow_weapons
                .get(kind)
                .context("missing Pow carried resource")?;
            ensure!(
                (0..slots).all(|slot| weapon.slots.contains_key(&slot)
                    && weapon.rigs.get(&slot).is_some_and(|rig| rig
                        .attack_groups
                        .get(&0)
                        .is_some_and(|bones| !bones.is_empty()))
                    && weapon.trails.contains_key(&slot)),
                "missing Pow carried instance"
            );
            for &id in kind.effects() {
                self.effect_programs
                    .program(EffectId {
                        bank: EffectBank::Magic(kind.native() - 200),
                        id,
                    })
                    .with_context(|| format!("missing Pow effect {id}"))?;
            }
            ensure!(
                self.audio.audio.sounds.contains_key(&126),
                "missing Pow end sound"
            );
            for (role, phase) in data.phases.iter().enumerate() {
                let character = kind.characters()[role];
                if !self.party.contains_key(&character) {
                    continue;
                }
                let variants = self
                    .visuals
                    .party
                    .get(&character)
                    .context("missing Pow actor model")?;
                for (costume, model) in variants.concrete() {
                    let visual = &model.visual;
                    for layers in phase.action.commands.iter().filter_map(|step| {
                        if let ActionCommand::TextureLayers(layers) = step.command {
                            Some(layers)
                        } else {
                            None
                        }
                    }) {
                        ensure!(
                            visual
                                .texture_layers
                                .iter()
                                .zip(layers)
                                .all(|(layer, frame)| frame < layer.frames),
                            "missing Pow texture frame ({costume:?})"
                        );
                    }
                    if character != 2 {
                        ensure!(
                            (0..slots).all(|slot| visual.rig.weapon_bones.contains_key(&slot)),
                            "missing Pow carried anchor ({costume:?})"
                        );
                        ensure!(
                            data.end_bones.iter().all(|name| visual
                                .rig
                                .skeleton
                                .bones
                                .iter()
                                .any(|bone| &bone.name == name)),
                            "missing Pow end attachment ({costume:?})"
                        );
                    }
                }
                let (sounds, voices) = phase.action.audio_ids();
                for sound in sounds {
                    ensure!(
                        i16::try_from(sound).is_ok_and(|id| self
                            .audio
                            .audio
                            .sounds
                            .contains_key(&id)),
                        "missing Pow action sound {sound}"
                    );
                }
                for voice in voices.into_iter().filter(|&id| id != 0) {
                    ensure!(
                        self.audio.voice_cues.contains_key(&voice),
                        "missing Pow action voice {voice}"
                    );
                }
            }
        }
        Ok(())
    }

    fn validate_thrusts(&self) -> Result<()> {
        use effects::{EffectBank, EffectId};
        let party = self.party.keys().copied().collect::<Vec<_>>();
        for (kind, program) in &self.unison.thrusts {
            if !kind.selected(&party) {
                continue;
            }
            for id in 0..=if *kind == unison::Thrust::Dark { 2 } else { 1 } {
                self.effect_programs
                    .program(EffectId {
                        bank: EffectBank::Magic(kind.native() - 200),
                        id,
                    })
                    .context("missing combined thrust effect")?;
            }
            for &voice in kind.voices() {
                ensure!(
                    self.audio.voice_cues.contains_key(&voice),
                    "missing combined thrust entry voice"
                );
            }
            for character in &party {
                for role in 0..2 {
                    let Some(phase) = kind.phase(role, *character) else {
                        continue;
                    };
                    let phase = &program.phases[usize::from(phase)];
                    let (sounds, voices) = phase.action.audio_ids();
                    for sound in sounds {
                        ensure!(
                            i16::try_from(sound).is_ok_and(|id| self
                                .audio
                                .audio
                                .sounds
                                .contains_key(&id)),
                            "missing combined thrust action sound"
                        );
                    }
                    for voice in voices.into_iter().filter(|&voice| voice != 0) {
                        ensure!(
                            self.audio.voice_cues.contains_key(&voice),
                            "missing combined thrust action voice"
                        );
                    }
                }
            }
        }
        Ok(())
    }

    /// Called during catalogue preparation and retained at the native launch boundary.
    pub fn validate_strike(&self, kind: unison::Strike) -> Result<()> {
        use actions::ActionCommand;
        use effects::EffectBank;
        self.unison
            .validate_strike_motions(kind, &self.visuals.party)?;
        let program = self
            .unison
            .strikes
            .get(&kind)
            .context("missing combined strike program")?;
        for id in kind.programs() {
            self.effect_programs
                .program(kind.effect(id))
                .context("missing combined strike effect")?;
        }
        self.effects
            .projectile(kind.effect(1))
            .context("missing combined strike projectile")?;
        for &character in self.party.keys() {
            let Some(role) = kind.phase(usize::from(character != 1), character) else {
                continue;
            };
            let variants = self
                .visuals
                .party
                .get(&character)
                .context("missing combined strike actor model")?;
            let phase = &program.phases[usize::from(role)];
            for (costume, model) in variants.concrete() {
                let visual = &model.visual;
                for step in &phase.action.commands {
                    if let ActionCommand::TextureLayers(layers) = step.command {
                        ensure!(
                            visual
                                .texture_layers
                                .iter()
                                .zip(layers)
                                .all(|(layer, frame)| frame < layer.frames),
                            "missing combined strike texture frame ({costume:?})"
                        );
                    }
                }
            }
            let (mut sounds, voices) = phase.action.audio_ids();
            if program.projectile_rule.sound != 0 {
                sounds.insert(program.projectile_rule.sound);
            }
            for sound in sounds {
                ensure!(
                    i16::try_from(sound).is_ok_and(|id| self.audio.audio.sounds.contains_key(&id)),
                    "missing combined strike sound {sound}"
                );
            }
            for voice in voices.into_iter().filter(|&voice| voice != 0) {
                ensure!(
                    self.audio.voice_cues.contains_key(&voice),
                    "missing combined strike voice {voice}"
                );
            }
            for rule in phase
                .action
                .hits
                .iter()
                .map(|hit| hit.rule)
                .chain([program.projectile_rule])
            {
                if let Some(effect) =
                    rule.impact_program_from(EffectBank::Techniques, Some(kind.native() - 200))?
                {
                    self.effect_programs
                        .program(effect)
                        .context("missing combined strike impact effect")?;
                }
            }
        }
        Ok(())
    }

    fn validate_unison(&self) -> Result<()> {
        use actions::{ActionCommand, HitEmission};
        use audio::{AudioActor, ResolvedVoice};
        use effects::{EffectBank, EffectId};

        self.unison.validate_party_motions(&self.visuals.party)?;
        self.validate_pow_weapons()?;
        self.validate_thrusts()?;
        let party = self.party.keys().copied().collect::<Vec<_>>();
        for kind in unison::Strike::ALL {
            if kind.selected(&party) {
                self.validate_strike(kind)?;
            }
        }
        for kind in unison::CombinedPair::ALL {
            if kind.selected(&party) && self.unison.pair_program(kind).is_some() {
                self.validate_combined_pair(kind)?;
            }
        }
        for id in [13, 14] {
            self.effect_programs
                .program(EffectId {
                    bank: EffectBank::Common,
                    id,
                })
                .with_context(|| format!("missing Unison effect {id}"))?;
        }
        for id in [73, 77] {
            ensure!(
                self.audio.audio.sounds.contains_key(&id),
                "missing Unison sound {id}"
            );
        }
        for &character in self.party.keys() {
            let opener = &self.unison.opener[usize::from(character - 1)];
            let variants = self
                .visuals
                .party
                .get(&character)
                .context("missing Unison actor model")?;
            for (costume, model) in variants.concrete() {
                let visual = &model.visual;
                opener.action.validate(self.actions.projectiles.len())?;
                for frames in std::iter::once(&opener.texture_layers).chain(
                    opener.action.commands.iter().filter_map(|command| {
                        if let ActionCommand::TextureLayers(ref layers) = command.command {
                            Some(layers)
                        } else {
                            None
                        }
                    }),
                ) {
                    ensure!(
                        visual
                            .texture_layers
                            .iter()
                            .zip(frames)
                            .all(|(layer, &frame)| frame < layer.frames),
                        "missing Unison texture frame {character} ({costume:?})"
                    );
                }
            }
            for hit in &opener.action.hits {
                if let HitEmission::Effect { effect, .. } = hit.emission {
                    self.effects
                        .projectile(EffectId {
                            bank: EffectBank::Techniques,
                            id: effect,
                        })
                        .context("missing Unison projectile")?;
                }
                ensure!(
                    hit.projectile_modifier.is_none(),
                    "unsupported Unison projectile modifier"
                );
            }
            for effect in opener.action.impact_programs(EffectBank::Techniques)? {
                self.effect_programs
                    .program(effect)
                    .with_context(|| format!("missing Unison impact effect {effect:?}"))?;
            }
            let actor = AudioActor::Party(character);
            let profile = self.audio.voice_profile(actor)?;
            let (sounds, mut voices) = opener.action.audio_ids();
            if let ResolvedVoice::Cue(id) = profile.relative(actor, 12)? {
                voices.insert(id);
            }
            for sound in sounds {
                ensure!(
                    i16::try_from(sound).is_ok_and(|id| self.audio.audio.sounds.contains_key(&id)),
                    "missing Unison action sound {sound}"
                );
            }
            for voice in voices.into_iter().filter(|&id| id != 0) {
                ensure!(
                    self.audio.voice_cues.contains_key(&voice),
                    "missing Unison action voice {voice}"
                );
            }
        }
        Ok(())
    }
}

fn find_formation(formations: &[Formation], id: u16, arena: u16) -> Result<&Formation> {
    formations
        .iter()
        .find(|f| (f.id, f.arena) == (id, arena))
        .with_context(|| format!("uncooked battle formation {id} in arena {arena}"))
}

fn validate_encounters(formations: &[Formation]) -> Result<()> {
    ensure!(!formations.is_empty(), "empty battle catalogue");
    let mut ids = BTreeSet::new();
    for formation in formations {
        ensure!(
            ids.insert((formation.id, formation.arena)),
            "duplicate battle formation {} in arena {}",
            formation.id,
            formation.arena
        );
    }
    Ok(())
}

#[test]
fn encounter_identity_includes_the_arena() {
    let mut formations = [13, 14]
        .map(|arena| Formation {
            id: 1,
            arena,
            placement: FormationPlacement::Explicit,
            escape_allowed: true,
            opening_voice: true,
            victory_music: true,
            victory_camera: true,
            victory_celebration: true,
            enemies: Vec::new(),
        })
        .to_vec();
    validate_encounters(&formations).unwrap();
    for (index, arena) in [13, 14].into_iter().enumerate() {
        assert!(std::ptr::eq(
            find_formation(&formations, 1, arena).unwrap(),
            &formations[index]
        ));
    }
    assert!(find_formation(&formations, 1, 15).is_err());
    assert!(find_formation(&formations, 2, 13).is_err());
    formations.push(formations[0].clone());
    let error = validate_encounters(&formations).unwrap_err();
    assert_eq!(
        error.to_string(),
        "duplicate battle formation 1 in arena 13"
    );
}

#[test]
fn old_explicit_formations_keep_placement_and_opening_defaults() {
    let formation: Formation = serde_json::from_str(
        r#"{"id":1,"arena":13,"escape_allowed":true,"victory_music":true,"victory_camera":true,"victory_celebration":true,"enemies":[]}"#,
    ).unwrap();
    assert_eq!(formation.placement, FormationPlacement::Explicit);
    assert!(formation.opening_voice);
}
