//! Action definitions used during battle preparation.
use crate::menu_data::Element;
use serde::{Deserialize, Serialize};

/// A carried weapon binding within one actor's model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WeaponSlot(pub u8);

impl WeaponSlot {
    pub const PRIMARY: [Self; 2] = [Self(0), Self(1)];

    pub const fn is_primary(self) -> bool {
        self.0 < 2
    }
}

impl TryFrom<u16> for WeaponSlot {
    type Error = std::num::TryFromIntError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        u8::try_from(value).map(Self)
    }
}

/// Enemy choices and their resolved contact resources.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EnemyActions {
    pub rows: Vec<EnemyAction>,
    pub policy: EnemyPolicy,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EnemyPolicy {
    pub back_row: Vec<EnemyBackRow>,
    pub walk_override: Option<EnemyWalkOverride>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub behavior: Option<EnemyBehavior>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub items_forbidden: bool,
    /// Requires an encounter event to release the enemy's health hold.
    #[serde(default)]
    pub event_hp_hold: bool,
    pub unsupported_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnemyBehavior {
    /// Leave when either the health threshold or elapsed gameplay time is reached.
    Retreat {
        hp_percent: u8,
        after_ticks: u32,
    },
    /// Switch choices permanently and release turning, pushing and Unison restrictions.
    HealthStage {
        hp_percent: u8,
        initial_station: EnemyStation,
        initial_actions: std::ops::Range<u8>,
        final_actions: std::ops::Range<u8>,
    },
    CarriedAnimationRate {
        slot: WeaponSlot,
        rate: f32,
    },
    /// Break the equipped weapon, then move the carried spare into its slot.
    ReplaceWeapon {
        action: u8,
        hide_at: u16,
        replace_at: u16,
        source: WeaponSlot,
        destination: WeaponSlot,
        disabled_action: u8,
    },
    ActionWeapons {
        /// Model identity and destination attachment slot for each equipped group.
        groups: Vec<Vec<(u8, WeaponSlot)>>,
        actions: Vec<Option<u8>>,
        /// Restore this group outside attacks; absent means the weapons stay hidden.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        idle_group: Option<u8>,
        effect: u16,
        /// Body attachment slots at which equipment changes emit their effect.
        anchors: Vec<u8>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        drop_on_hurt: Option<WeaponDrop>,
    },
    ThreeStations {
        stations: [EnemyStation; 3],
        /// Elapsed action time, including animation blend holds.
        change_at: u16,
    },
    ModelParts {
        bone_scales: Vec<(String, f32)>,
    },
    Transformation {
        reveal_hp_percent: u8,
        revealed_bones: Vec<String>,
        airborne_hp_percent: u8,
        airborne_walk_speed: f32,
        airborne_motions: Vec<(u16, u16)>,
        hover_height: f32,
        initial_actions: std::ops::Range<u8>,
        final_actions: std::ops::Range<u8>,
    },
}

/// Equipment knocked loose by a hit at low health, then left on the ground.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct WeaponDrop {
    pub slot: WeaponSlot,
    pub hp_percent: u8,
    pub upward_speed: f32,
    pub gravity: f32,
    pub pitch: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct EnemyStation {
    pub position: [f32; 3],
    pub heading: f32,
}

/// A walking speed and motion adopted at low health and retained after healing.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct EnemyWalkOverride {
    pub hp_percent: u8,
    pub speed: f32,
    /// Logical walking clip and the replacement clip.
    pub motion_alias: (u16, u16),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct EnemyBackRow {
    pub action: u8,
    pub weight: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnemyRequirements {
    pub difficulty: std::ops::RangeInclusive<u8>,
    pub hp_percent: Option<u8>,
    /// Maximum available enemies, including the actor choosing this action.
    #[serde(default)]
    pub max_enemies: Option<u8>,
    pub priority: bool,
    /// Starting the action consumes its eligibility for the rest of this battle.
    #[serde(default)]
    pub once_per_battle: bool,
    #[serde(default)]
    pub target_guarding: bool,
    #[serde(default)]
    pub followup_only: bool,
    #[serde(default)]
    pub followup_requires_hit: bool,
    #[serde(default)]
    pub predicates: Vec<EnemyPredicate>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnemyPredicate {
    AllyAbsent(u16),
    DoNotRepeat,
    OriginDistance {
        minimum: Option<u16>,
        maximum: Option<u16>,
    },
    OpponentCount {
        minimum: Option<u8>,
        maximum: Option<u8>,
    },
    TargetAttacking,
}

impl Default for EnemyRequirements {
    fn default() -> Self {
        Self {
            difficulty: 0..=2,
            hp_percent: None,
            max_enemies: None,
            priority: false,
            once_per_battle: false,
            target_guarding: false,
            followup_only: false,
            followup_requires_hit: false,
            predicates: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetPolicy {
    Nearest,
    Farthest,
    Leader,
    Spread,
    Flying,
    Casting,
    LowestHp,
    Protect,
    SelfTarget,
    Random,
    NearestIncapacitated,
}

impl TryFrom<u8> for TargetPolicy {
    type Error = anyhow::Error;

    fn try_from(value: u8) -> anyhow::Result<Self> {
        Ok(match value {
            1 => Self::Nearest,
            2 => Self::Farthest,
            3 => Self::Leader,
            4 => Self::Spread,
            5 => Self::Flying,
            6 => Self::Casting,
            7 => Self::LowestHp,
            8 => Self::Protect,
            9 => Self::SelfTarget,
            10 => Self::Random,
            11 => Self::NearestIncapacitated,
            _ => anyhow::bail!("unsupported target policy {value}"),
        })
    }
}

/// Decoded action data, independent of an enemy's catalogue identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnemyExecution {
    Attack(EnemyAttack),
    Physical { technique: u16, attack: EnemyAttack },
    Spell(u16),
    LeaveArena,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpellSlot {
    Primary,
    Secondary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnemyAttack {
    /// Elapsed gameplay ticks, including animation blend holds.
    pub duration: u16,
    /// One follow-up attempt on the same elapsed clock as the attack events.
    #[serde(default)]
    pub chain_at: Option<u16>,
    pub recovery_ticks: u8,
    pub recovery_pose: ActionPose,
    pub armor: u8,
    pub vulnerable: [u16; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact_recovery: Option<EnemyContactRecovery>,
    /// Event times use the same elapsed clock as duration.
    pub events: Vec<(u16, EnemyEvent)>,
}

/// The retreat selected after this enemy's attack makes contact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnemyContactRecovery {
    pub pose: Option<ActionPose>,
    pub movement: [Option<f32>; 2],
    pub braking: Option<f32>,
    pub pass_through_ticks: u8,
    pub voice: Option<crate::battle_voice::Sound>,
    pub expressions: [u8; 4],
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ActionPose {
    pub clip: u16,
    /// Positions in the decoded clip's frame units.
    pub start: f32,
    pub end: Option<f32>,
    pub rate: f32,
    pub blend: u8,
    pub repeat: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContactAttachments {
    Groups(Vec<u8>),
    BodyBone(u16),
    BodyCenter {},
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeleeContact {
    pub duration: u16,
    #[serde(flatten)]
    pub attachments: ContactAttachments,
    pub shape: crate::battle_projectile::HitShape,
    pub radius: f32,
    pub height: f32,
    pub hit: ActionHit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnemyEvent {
    Effect(u16),
    /// A burst at a carried model's current bone position.
    CarriedEffect {
        slot: WeaponSlot,
        bone: u16,
        member: u16,
    },
    /// Shared effect attached to the actor's body center.
    CommonEffect(u16),
    /// Temporary RGB tint, returning to the stage color one unit per update.
    Tint([u8; 3]),
    /// End the actor's participation without dealing damage to it.
    Defeat {
        rewards: bool,
    },
    /// Pause pending action events until contact; a miss still reaches this action age.
    WaitForHit {
        deadline: u16,
    },
    /// Pause pending action events until an unguarded hit overcomes reaction protection.
    WaitForEffectiveHit {
        deadline: u16,
    },
    ReleaseCaptured {
        hitstun: u16,
    },
    CapturedVisibility(bool),
    CapturedForward(f32),
    CapturedVertical(f32),
    Pose(ActionPose),
    /// Earlier partial poses take precedence over later overlapping parts.
    PartialPose {
        first_bone: u16,
        pose: ActionPose,
    },
    PoseKeepingParts(ActionPose),
    CarriedPose {
        slot: WeaponSlot,
        clip: u16,
        rate: f32,
        repeat: bool,
    },
    AnimationRate(f32),
    Contact(MeleeContact),
    /// Update an overlapping row without resetting the current window's victims.
    ContinueContact(MeleeContact),
    Projectile {
        definition: Box<crate::battle_projectile::Projectile>,
        hit: ActionHit,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        feedback: Option<crate::battle_martial::ProjectileFeedback>,
    },
    /// Emit at up to two distinct living opponents, retreating from their ground positions.
    GroundProjectiles {
        definitions: Box<[crate::battle_projectile::Projectile; 2]>,
        hit: ActionHit,
        retreat: f32,
    },
    /// Release a prepared spell from the attack timeline without chanting.
    ReleaseSpell {
        technique: u16,
        slot: SpellSlot,
    },
    UseItem {
        item: u16,
        party: bool,
    },
    Forward(f32),
    Vertical(f32),
    Acceleration(f32),
    Gravity(f32),
    Braking(f32),
    /// Replace world position without changing velocity.
    SetPosition([i16; 3]),
    /// Start at the selected target, retreat along its cached bearing, then set height.
    PositionFromTarget {
        height: i16,
        retreat: i16,
    },
    /// Retreat from the current position along the cached target bearing, then set height.
    OffsetPosition {
        height: i16,
        retreat: i16,
    },
    /// Turn movement from its retained action direction and add degrees to facing.
    TurnMotion(f32),
    /// Align facing and action movement with the current target bearing.
    FaceTarget,
    /// Suppress ordinary hit reactions for this many common updates.
    ReactionArmor(u16),
    SecondaryWind {
        enabled: bool,
        duration: u16,
    },
    /// Apply a temporary percentage increase to the owner.
    StatUp {
        condition: Condition,
        amount: i16,
    },
    /// Apply one random condition from each group to every living opponent.
    AfflictOpponents {
        groups: Vec<Vec<Condition>>,
        dispel: bool,
    },
    Stun,
    WeaponVisibility {
        slot: WeaponSlot,
        visible: bool,
    },
    PassThrough {
        enabled: bool,
        /// Zero lasts until the action ends; otherwise shared actor updates count down.
        duration: u8,
    },
    Trail {
        slot: WeaponSlot,
        duration: u16,
    },
    Sound(u16),
    Voice {
        sound: Option<crate::battle_voice::Sound>,
        announcement: bool,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ActionHit {
    pub kind: crate::battle_projectile::DamageKind,
    /// Additional contact program in the attacking enemy's effect bank.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effect: Option<u16>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub effect_on_guard: bool,
    /// Omit the default blocking sound while retaining the authored contact cue.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub suppress_guard_sound: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub capture: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sound: Option<u16>,
    pub power: Power,
    pub element: HitElement,
    pub arte: bool,
    pub prevents_defeat: bool,
    /// Award the finisher grade bonus when this contact defeats an enemy.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub finisher_grade: bool,
    /// Restore nominal contact damage to the attacker on an unguarded, unsuppressed hit.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hp_drain: bool,
    #[serde(default)]
    pub suppress_reaction: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub force_reaction: bool,
    pub alternate_motion: bool,
    pub knock_down: bool,
    pub launch: bool,
    pub hits_down: bool,
    pub breaks_guard: bool,
    pub unbreakable_guard: bool,
    pub lift_guard: bool,
    pub guard_pressure: u8,
    #[serde(default)]
    pub bypass_guard: bool,
    pub hitstun: u8,
    pub cooldown: u8,
    pub stun_chance: u8,
    pub stagger: u8,
    pub armor_damage: u8,
    pub recoil: u8,
    pub recoil_delay: u8,
    pub condition: Option<HitCondition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnemyAction {
    pub weight: u8,
    /// Saved story flag required when the encounter begins.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_flag: Option<u16>,
    #[serde(default)]
    pub cast_ticks: Option<u16>,
    /// Hit armor installed when this spell begins chanting, including zero.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cast_armor: Option<u8>,
    #[serde(default)]
    pub voices: EnemyActionVoices,
    /// A follow-up selects from the next numbered group.
    #[serde(default)]
    pub chain_group: u8,
    /// Percentage chance; zero disables follow-ups.
    #[serde(default)]
    pub followup_chance: u8,
    pub target_policy: Option<TargetPolicy>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub target_allies: bool,
    pub requirements: EnemyRequirements,
    pub return_to_formation: bool,
    /// Use running motion and speed while returning to formation.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub run_return: bool,
    #[serde(default)]
    pub approach: EnemyApproach,
    pub range: [i16; 2],
    pub approach_range: i16,
    pub approach_minimum: i16,
    pub guard_chance: i8,
    pub tp: u8,
    pub execution: Option<EnemyExecution>,
    pub unsupported_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct EnemyActionVoices {
    pub begin: Option<crate::battle_voice::Sound>,
    pub release: Option<crate::battle_voice::Sound>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnemyApproach {
    #[default]
    Walk,
    Run,
    Custom {
        clip: u16,
        rate: f32,
        speed: f32,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HitElement {
    #[default]
    Inherited,
    Neutral,
    Element(Element),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Power {
    #[default]
    Normal,
    Percent(u16),
    Fixed(u16),
    /// Use current power for this contact, then adjust the owner's power.
    AdjustAfterContact(i16),
}

pub use crate::battle_conditions::Condition;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HitCondition {
    pub condition: Condition,
    pub chance: u8,
    pub value: i8,
}
