use crate::conditions::Condition;
use crate::{
    Activity, PreparedBattle, ProjectileDefinition, ProjectileFrame, ProjectileId,
    action::{Execution, Sequence, step_sequence},
    contact::Contacts,
    projectile::Projectile,
};
use anyhow::{Context, Result, ensure};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ActorId(pub(crate) u8);

impl ActorId {
    /// Construct a checked public actor handle for host integrations.
    pub fn from_index(index: usize) -> Result<Self> {
        ensure!(
            index <= usize::from(u8::MAX),
            "actor index exceeds actor handle range"
        );
        Ok(Self(index as u8))
    }

    pub fn index(self) -> usize {
        usize::from(self.0)
    }
}

/// Never reused within a battle, including after interruption or completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ActionId(pub(crate) u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Party,
    Enemy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpellSlot {
    Primary,
    Secondary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContactSource {
    Melee {
        actor: ActorId,
        action: ActionId,
    },
    Weapon {
        actor: ActorId,
        slot: u8,
        action: ActionId,
    },
    Projectile(ProjectileId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryKind {
    Hp,
    Tp,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RecoveryTraits {
    /// EX97 Magic Boost: HP recovery chance and magic-damage critical admission.
    pub lucky: bool,
    pub boost: bool,
    pub common: crate::CommonRecoveryTraits,
    pub lethal: crate::LethalRescueTraits,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Actor {
    pub side: Side,
    pub species: u16,
    pub equipment: EquipmentAttributes,
    pub control: crate::Control,
    /// Prepared local controller slot, independent of character identity.
    pub control_slot: u8,
    pub availability: crate::ActorAvailability,
    pub guard: crate::Guard,
    pub hp: i32,
    pub tp: u16,
    pub control_ex_state: crate::ControlExState,
    pub casting_state: crate::CastingState,
    /// Prepared casting action stored for a later release.
    pub stored_spell: Option<crate::ActionKey>,
    pub overlimit: crate::OverLimit,
    pub elements: crate::AttackElements,
    /// Current action power percentage; released attacks retain it at emission.
    pub attack_power: u16,
    pub proficiency: u8,
    /// Fresh movement and action intent sampled from the current input.
    pub input: crate::InputIntent,
    pub conditions: crate::conditions::Conditions,
    pub position: [f32; 3],
    pub heading: f32,
    /// Desired facing. Rendered heading approaches it independently of movement.
    pub facing_direction: [f32; 3],
    /// Profile scale for effects emitted on this actor, independent of body scale.
    pub effect_scale: f32,
    pub secondary_wind: crate::SecondaryWind,
    pub body: crate::Body,
    pub movement: crate::Movement,
    pub reaction: crate::Reaction,
    /// Actor-local stop counter; common timers continue while commands are held.
    pub hit_stop: u8,
    /// Remaining world ticks with this actor's commands, motion, and common timers frozen.
    pub time_stop: u16,
}

impl Actor {
    /// Compute integer percentages before the renderer scales bars.
    pub fn hp_percent(&self) -> i16 {
        (i64::from(self.hp) * 100 / i64::from(self.equipment.max_hp)).clamp(0, 100) as i16
    }

    pub fn tp_percent(&self) -> i16 {
        if self.equipment.max_tp == 0 {
            0
        } else {
            (u32::from(self.tp) * 100 / u32::from(self.equipment.max_tp)) as i16
        }
    }

    pub(crate) fn airborne(&self) -> bool {
        self.position[1] > crate::movement::GROUND_TOLERANCE
    }

    pub(crate) fn needs_landing(&self) -> bool {
        self.airborne() && !self.movement.flying && !self.movement.fixed_height
    }

    pub(crate) fn recovered_tp(&self, percent: i16) -> (u16, i32) {
        let nominal = (i64::from(self.equipment.max_tp) * i64::from(percent.max(0)) / 100)
            .clamp(1, i64::from(i32::MAX)) as i32;
        let tp =
            (i64::from(self.tp) + i64::from(nominal)).min(i64::from(self.equipment.max_tp)) as u16;
        (tp, nominal)
    }

    pub(crate) fn recover_flat_hp(&mut self, amount: i32) {
        let weak = self.conditions.effective().contains(Condition::Weak);
        let cap = if weak {
            self.equipment.max_hp / 2
        } else {
            self.equipment.max_hp
        };
        if self.hp < cap {
            self.hp = self.hp.saturating_add(amount.max(0)).min(cap);
        }
    }

    pub(crate) fn recovered_hp(&self, percent: i32) -> (i32, i32) {
        let mut percent = i64::from(percent.max(0));
        if self.equipment.recovery.boost {
            percent += percent * 20 / 100;
        }
        let nominal =
            (i64::from(self.equipment.max_hp) * percent / 100).clamp(1, i64::from(i32::MAX)) as i32;
        let weak = self.conditions.effective().contains(Condition::Weak);
        let cap = if weak {
            self.equipment.max_hp / 2
        } else {
            self.equipment.max_hp
        };
        let hp = if self.hp < cap {
            self.hp.saturating_add(nominal).min(cap)
        } else {
            self.hp
        };
        (hp, nominal)
    }

    pub(crate) fn recover_flat_tp(&mut self, amount: i32) {
        self.tp = (i64::from(self.tp) + i64::from(amount.max(0)))
            .min(i64::from(self.equipment.max_tp)) as u16;
    }

    pub(crate) fn validate(&self) -> Result<()> {
        self.equipment.validate()?;
        ensure!(
            (0..=self.equipment.max_hp).contains(&self.hp),
            "invalid battle HP"
        );
        ensure!(self.tp <= self.equipment.max_tp, "invalid battle TP");
        ensure!(self.proficiency <= 5, "invalid technique proficiency");
        ensure!(self.control_slot < 4, "invalid local control slot");
        ensure!(
            self.position
                .iter()
                .chain(&self.facing_direction)
                .all(|v| v.is_finite())
                && self.heading.is_finite()
                && self.effect_origin().iter().all(|v| v.is_finite())
                && self.target_center().iter().all(|v| v.is_finite()),
            "invalid battle position"
        );
        ensure!(
            self.effect_scale.is_finite() && self.effect_scale >= 0.,
            "invalid battle effect scale"
        );
        self.movement.validate()?;
        self.reaction.validate()?;
        self.body.validate()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ActionRequest {
    pub actor: ActorId,
    pub action: crate::ActionKey,
    pub target: ActorId,
}

/// Input for one simulation update. A paused menu advances neither action ages nor tasks.
#[derive(Debug, Default)]
pub struct BattleInput {
    pub controllers: Vec<crate::ControlInput>,
    pub unison: Option<crate::UnisonInput>,
    /// Actor-owned executable roots; released children are dispatched internally.
    pub actions: Vec<ActionRequest>,
    pub interrupt: Vec<ActionId>,
    /// Menus and command pages freeze gameplay and feedback together.
    pub paused: bool,
    /// Actors whose struggle buttons were pressed on this update.
    pub stun_struggle: Vec<ActorId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejection {
    BattleEnding,
    Busy,
    Defeated,
    Petrified,
    InsufficientTp,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Cue {
    /// Retire existing effect emitters and particles before a cinematic's first effect.
    ClearEffects,
    /// Retire one actor's spell scene without disturbing other effect resources.
    ClearEffectBank {
        actor: ActorId,
        resource: u32,
    },
    Effect(crate::EffectRequest),
    /// Casting artwork includes its initial emission and first update on admission.
    CastingEffect(crate::EffectRequest),
    /// A cinematic emitter runs while ordinary effects are held; particles keep their own clock.
    EffectDuringPause(crate::EffectRequest),
    /// Scene-owned scenery tint; its timer is independent of the released spell.
    StageTint(crate::StageTint),
    Cutin {
        action: ActionId,
        catalogue: u16,
        duration: u16,
    },
    /// Start on the next scene update, independently of the actor's hit stop.
    ActionEffect {
        action: ActionId,
        request: crate::EffectRequest,
    },
    /// A completed poison tick. Presentation owns its optional feedback.
    PoisonPulse {
        actor: ActorId,
    },
    SelfCured {
        actor: ActorId,
    },
    Defeated {
        actor: ActorId,
    },
    Landed {
        actor: ActorId,
        position: [f32; 3],
    },
    CombatRetired,
    Casting {
        actor: ActorId,
        action: crate::ActionKey,
        phase: crate::CastPhase,
    },
    TechniqueQueued {
        actor: ActorId,
    },
    Jumped {
        actor: ActorId,
        position: [f32; 3],
    },
    GuardReady {
        actor: ActorId,
    },
    Countered {
        actor: ActorId,
    },
    Charged {
        actor: ActorId,
        level: crate::ChargeLevel,
    },
    KnockdownImpact {
        actor: ActorId,
    },
    UnisonReady,
    /// Optional weapon ribbon. Presentation owns sampling, lifetime and fading.
    WeaponTrail {
        actor: ActorId,
        slot: u8,
        duration: u16,
        /// Technique feedback uses the shared bright blade palette.
        highlighted: bool,
    },
    /// Presentation owns the color and lifetime; this cannot change actor opacity.
    ActorFlash {
        actor: ActorId,
        color: [u8; 3],
    },
    /// Presentation owns camera displacement and controller pulse lifetimes.
    Shake {
        duration: u32,
        amplitude: u32,
    },
    OverLimitEntered {
        actor: ActorId,
        position: [f32; 3],
    },
    /// A successful release, emitted after inventory and knowledge are committed.
    ItemReleased {
        user: ActorId,
        target: ActorId,
        effect: crate::item::Effect,
        discovered: bool,
    },
    Paralyzed {
        actor: ActorId,
    },
    Breakfall {
        actor: ActorId,
    },
    ItemNotice {
        actor: ActorId,
        item: u16,
        duration: u16,
    },
    EnemyScanned {
        actor: ActorId,
    },
    Started {
        definition: Option<crate::ActionKey>,
        action: ActionId,
        actor: ActorId,
    },
    Rejected {
        actor: ActorId,
        reason: Rejection,
    },
    Completed {
        action: ActionId,
    },
    Released {
        action: ActionId,
        /// Recoil callbacks have no surviving action task to name as parent.
        parent: Option<ActionId>,
        actor: ActorId,
        slot: SpellSlot,
    },
    Interrupted {
        action: ActionId,
    },
    HammerRevenge {
        projectile: ProjectileId,
    },
    ProjectileStarted {
        projectile: ProjectileId,
        action: ActionId,
    },
    /// A model follows the projectile's contact rebound and later floor bounces.
    ProjectileRebound {
        projectile: ProjectileId,
        first: bool,
    },
    ProjectileExpired {
        projectile: ProjectileId,
    },
    ProjectileClashed {
        projectile: ProjectileId,
        other: ContactSource,
        position: [f32; 3],
    },
    /// Resolved contact data; presentation selects impact artwork and audio.
    Hit {
        source: ContactSource,
        owner: ActorId,
        actor: ActorId,
        position: [f32; 3],
        element: Option<crate::Element>,
        contact_sound: Option<crate::Sound>,
        suppress_guard_sound: bool,
        was_casting: bool,
        overlimit: bool,
        stunned: bool,
        result: crate::HitResult,
    },
    /// The host resolves text from the prepared action. Presentation retains
    /// the independent window lifetime after interruption or completion.
    Notice {
        actor: ActorId,
        action: crate::ActionKey,
        duration: u16,
    },
    /// An action-name window requested by a prepared effect timeline.
    EffectNotice {
        actor: ActorId,
        text: String,
        duration: u16,
    },
    Rescued {
        actor: ActorId,
        kind: crate::RescueKind,
    },
    ExSkillLabel {
        actor: ActorId,
        position: [f32; 3],
    },
    CustomLabel {
        actor: ActorId,
        position: [f32; 3],
        left: String,
        right: String,
        duration: u32,
    },
    ConditionLabel {
        actor: ActorId,
        kind: crate::conditions::ConditionLabel,
        position: [f32; 3],
    },
    /// Contact-time values before defeat/recovery clears the victim's counters.
    Combo {
        actor: ActorId,
        hits: i32,
        damage: i32,
    },
    IncidentalDamage {
        actor: ActorId,
        amount: i32,
    },
    Recovered {
        kind: crate::RecoveryKind,
        actor: ActorId,
        nominal: i32,
        applied: i32,
    },
    Voice {
        actor: ActorId,
        priority: crate::VoicePriority,
        sound: crate::Sound,
        position: [f32; 3],
        centered: bool,
    },
    StopVoice {
        actor: ActorId,
    },
    Sound {
        actor: ActorId,
        sound: crate::Sound,
        position: [f32; 3],
        priority: u8,
    },
    GlobalSound {
        sound: crate::Sound,
        priority: u8,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BattleResult {
    Escaped,
    Victory,
    Defeat,
}

#[derive(Debug, Clone)]
pub struct BattleOutcome {
    pub result: BattleResult,
    pub(crate) completion: Arc<()>,
}

impl PartialEq for BattleOutcome {
    fn eq(&self, other: &Self) -> bool {
        self.result == other.result && Arc::ptr_eq(&self.completion, &other.completion)
    }
}

/// An actor observation, derived when the battle publishes its frame.
#[derive(Debug, Clone, PartialEq)]
pub struct ActorFrame {
    pub state: Actor,
    pub activity: crate::Activity,
}

impl std::ops::Deref for ActorFrame {
    type Target = Actor;

    fn deref(&self) -> &Actor {
        &self.state
    }
}

impl std::ops::DerefMut for ActorFrame {
    fn deref_mut(&mut self) -> &mut Actor {
        &mut self.state
    }
}

/// Timing selected once for an update. New effect holds start on the next update.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BattleClock {
    #[default]
    Running,
    Held,
    /// Gameplay waits while pause-enabled activation effects continue.
    TimedHold,
    Rescue(ActorId),
    /// Acquisition holds actors while the camera frames the caster.
    Learning(ActorId),
    /// Stored-scene framing runs while ordinary gameplay and actor playback wait.
    SpellTransition,
    /// Summon staging holds gameplay while actor poses and cinematic feedback advance.
    SummonTransition,
}

impl BattleClock {
    pub fn paused(self) -> bool {
        self != Self::Running
    }

    pub fn feedback_paused(self) -> bool {
        matches!(self, Self::Held | Self::TimedHold | Self::Rescue(_))
    }
}

/// One coherent state for drawing. No renderer can mutate simulation actors.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BattleFrame {
    pub clock: BattleClock,
    /// Actor clocks held during this update, before contacts could start a new hold.
    pub paused_actors: Vec<ActorId>,
    pub unison_gauge: i16,
    pub unison_available: bool,
    pub unison: Option<crate::UnisonFrame>,
    pub summon: Option<crate::SummonFrame>,
    pub enemy_presentation: Option<crate::EnemyPresentationFrame>,
    pub escape: Option<crate::EscapeFrame>,
    pub item_cooldown: u16,
    pub hourglass_remaining: u16,
    pub scanned_enemies: u16,
    pub update: u64,
    pub targets: Vec<Option<ActorId>>,
    pub target_selector: Option<ActorId>,
    pub actors: Vec<ActorFrame>,
    /// Scene-owned drawing data, filled after native publication.
    pub models: Vec<crate::ModelFrame>,
    /// Encounter-owned models with no actor or particle lifetime.
    pub independent_models: Vec<crate::EffectModelFrame>,
    pub weapons: Vec<crate::WeaponFrame>,
    pub model_requests: Vec<crate::ModelRequest>,
    pub weapon_flights: Vec<crate::WeaponFlightFrame>,
    pub actions: Vec<(ActionId, ActorId, u32)>,
    pub spell_origins: Vec<(ActionId, [f32; 3])>,
    pub projectiles: Vec<ProjectileFrame>,
    pub projectile_shadows: Vec<crate::ProjectileShadowFrame>,
    pub camera: Option<crate::CameraPose>,
    /// An event-owned lens; None selects the ordinary combat projection.
    pub camera_fov_degrees: Option<f32>,
    /// Event cover rendered through the shared battle transition layer.
    pub event_fade: Option<crate::TransitionFrame>,
    pub cues: Vec<Cue>,
    /// Recognition precedes the shared world visit; results still keep stepping.
    pub recognized_result: Option<BattleResult>,
    /// Delivered once. Subsequent calls after an outcome are rejected.
    pub outcome: Option<BattleOutcome>,
}

pub use resonance_content::random::Random;

/// Mutually exclusive native work owned by an actor. Physical movement and
/// released effects have independent lifetimes.
#[derive(Default)]
pub(crate) enum ActorTask {
    #[default]
    None,
    Approach(crate::approach::Approach),
    Returning(crate::steering::ReturnState),
    Escaping,
    Hurt {
        remaining: u32,
    },
    Recovering {
        remaining: u32,
        after_hit: bool,
    },
    Paralysis {
        remaining: u8,
    },
    Stunned {
        remaining: u32,
    },
    Knockdown(crate::knockdown::Recovery),
    Item(crate::item::Use),
    Mobility(crate::mobility::Mobility),
    Taunt {
        remaining: u32,
    },
    Action {
        id: ActionId,
        sequence: Box<Sequence>,
    },
}

impl ActorTask {
    fn activity(&self, actor: &Actor) -> crate::Activity {
        if actor.availability == crate::ActorAvailability::Dead {
            return Activity::Defeated;
        }
        match self {
            Self::None => {
                if actor.guard.active || actor.guard.recovery > 0 {
                    Activity::Guarding
                } else {
                    Activity::Idle
                }
            }
            Self::Approach(_) | Self::Returning(_) => Activity::Approaching,
            Self::Escaping => Activity::Escaping,
            Self::Hurt { .. } | Self::Paralysis { .. } => Activity::Hurt,
            Self::Recovering { .. } => Activity::Recovering,
            Self::Stunned { .. } => Activity::Stunned,
            Self::Knockdown(crate::knockdown::Recovery::Down { .. }) => Activity::KnockedDown,
            Self::Knockdown(crate::knockdown::Recovery::Rising { .. }) => Activity::GettingUp,
            Self::Item(_) => Activity::Item,
            Self::Mobility(
                crate::mobility::Mobility::Jump { .. } | crate::mobility::Mobility::Breakfall,
            ) => Activity::Jumping,
            Self::Mobility(crate::mobility::Mobility::Backstep { .. }) => Activity::Evading,
            Self::Mobility(crate::mobility::Mobility::Landing { .. }) => Activity::Recovering,
            Self::Taunt { .. } => Activity::Taunting,
            Self::Action { sequence, .. } => sequence.activity(),
        }
    }

    pub(crate) fn action(&self) -> Option<(&ActionId, &Sequence)> {
        match self {
            Self::Action { id, sequence } => Some((id, sequence)),
            _ => None,
        }
    }
    pub(crate) fn approach(&self) -> Option<crate::approach::Approach> {
        if let Self::Approach(approach) = self {
            Some(*approach)
        } else {
            None
        }
    }
    pub(crate) fn item(&self) -> Option<crate::item::Use> {
        if let Self::Item(item) = self {
            Some(*item)
        } else {
            None
        }
    }
    pub(crate) fn mobility(&self) -> Option<crate::mobility::Mobility> {
        if let Self::Mobility(mobility) = self {
            Some(*mobility)
        } else {
            None
        }
    }
}

/// State owned by one actor throughout the battle. Public actor values and model
/// poses stay separate so gameplay and animation can borrow them independently.
pub(crate) struct ActorRuntime {
    pub(crate) combo: crate::action_selection::Combo,
    pub(crate) control: Option<crate::control::Controller>,
    pub(crate) target: ActorId,
    pub(crate) enemy_choice: Option<usize>,
    pub(crate) last_executed_enemy_row: Option<usize>,
    pub(crate) enemy_effective_hit: bool,
    pub(crate) body_bypass_ticks: u8,
    pub(crate) used_enemy_choices: Vec<usize>,
    pub(crate) enemy_walk_override: Option<f32>,
    pub(crate) enemy_behavior: crate::enemy_policy::State,
    pub(crate) capture: crate::capture::State,
    task: ActorTask,
    pub(crate) idle_timer: u32,
    pub(crate) companion_policy: Option<crate::CompanionPolicy>,
    pub(crate) recovery: crate::CommonRecoveryState,
    pub(crate) angel_tear_armed: bool,
    pub(crate) support_target: ActorId,
    pub(crate) special_guard_pending: Option<u16>,
}

impl ActorRuntime {
    pub(crate) fn action_mut(&mut self) -> Option<(&ActionId, &mut Sequence)> {
        match &mut self.task {
            ActorTask::Action { id, sequence } => Some((id, sequence)),
            _ => None,
        }
    }

    pub(crate) fn task(&self) -> &ActorTask {
        &self.task
    }
}

pub struct Battle {
    pub(crate) runtime: Vec<ActorRuntime>,
    pub(crate) unison_gauge: i16,
    pub(crate) unison: crate::unison::State,
    pub(crate) summon_entry: crate::summon_entry::State,
    pub(crate) enemy_presentation: crate::enemy_presentation::State,
    pub(crate) defense_system: crate::defense_system::State,
    pub(crate) escape: crate::escape::State,
    pub(crate) items: crate::item::State,
    pub(crate) diagnostics: resonance_content::diagnostics::Diagnostics,
    pub(crate) diagnostic: bool,
    pub(crate) cast_inputs: [crate::casting::CastInput; 4],
    pub(crate) ledger: crate::Ledger,
    pub(crate) weapon_flights: BTreeMap<(ActorId, u8), crate::weapon_flight::Flight>,
    pub(crate) target_selector: Option<ActorId>,
    pub(crate) command_target: Option<(ActorId, ActorId)>,
    pub(crate) prepared: crate::prepare::BattleResources,
    /// Mutable battle-local learning membership. Persistent Party projection
    /// remains game-owned at the world-update boundary.
    pub(crate) learning_members: Vec<crate::learning::TechniqueLearningMember>,
    pub(crate) actors: Vec<Actor>,
    /// Target centers sampled before each actor's movement visit.
    pub(crate) actor_start_centers: Vec<[f32; 3]>,
    pub(crate) volleys: BTreeMap<ActionId, crate::release::Volley>,
    pub(crate) random: Random,
    pub(crate) model_requests: Vec<crate::ModelRequest>,
    pub(crate) models: Option<crate::Models>,
    pub(crate) projectiles: BTreeMap<ProjectileId, Projectile>,
    defer_projectile_anchors: bool,
    pub(crate) camera: Option<crate::camera::Camera>,
    pub(crate) entry_remaining: u8,
    pub(crate) update: u64,
    pub(crate) timed_hold: Option<crate::overlimit::Hold>,
    pub(crate) event_boss: Option<ActorId>,
    pub(crate) clock: BattleClock,
    paused_actors: Vec<ActorId>,
    pub(crate) next_action: u64,
    pub(crate) pending_cues: Vec<Cue>,
    pub(crate) next_projectile: u64,
    pub(crate) terminal: crate::outcome::Terminal,
    pub(crate) ended: bool,
}

/// Equipment-derived values; live action, movement and presentation state stay with the actor.
#[derive(Debug, Clone, PartialEq)]
pub struct EquipmentAttributes {
    pub max_hp: i32,
    pub max_tp: u16,
    pub tp_cost_reduction: bool,
    pub quick_escape: bool,
    pub taunt_enabled: bool,
    pub taunt_guard: bool,
    pub taunt_cancel: bool,
    pub control_ex: crate::ControlExTraits,
    pub quick_turn: bool,
    pub backstep_guard: bool,
    pub casting: crate::CastingTraits,
    pub dagger_reach: bool,
    pub contact: crate::ContactTraits,
    pub normal_combo_limit: u8,
    pub luck: u16,
    pub guaranteed_steal: bool,
    pub stats: crate::CombatStats,
    pub base_element: Option<crate::Element>,
    pub affinities: [crate::Affinity; 9],
    pub damage: crate::DamageTraits,
    pub recovery: RecoveryTraits,
    pub combo_traits: crate::ComboTraits,
    pub normal_guard: bool,
    pub speed_multiplier: f32,
    pub reaction_ex: crate::ContactEx,
    pub stun_ex_bonus: bool,
    pub spell_revenge: bool,
}

impl EquipmentAttributes {
    fn validate(&self) -> Result<()> {
        ensure!(self.max_hp > 0, "invalid battle HP");
        ensure!(self.normal_combo_limit > 0, "invalid normal combo limit");
        ensure!(
            self.damage
                .weapon_species
                .is_none_or(|species| (1..=12).contains(&species)),
            "invalid weapon species bonus"
        );
        ensure!(
            self.speed_multiplier.is_finite() && self.speed_multiplier > 0.,
            "invalid battle movement"
        );
        Ok(())
    }
}
/// One fully prepared row published at the Equip return boundary. Keeping
/// this as a named value makes the model/contact/trail ownership explicit and
/// gives the transaction one authoritative input shape.
pub struct EquipmentReplacement {
    pub actor: ActorId,
    pub attributes: EquipmentAttributes,
    pub conditions: crate::conditions::Conditions,
    pub equipment: Option<[u16; 2]>,
}

enum SequenceStep {
    Continue,
    Transition(ActionTransition),
    Finished,
}

enum ActionTransition {
    Advance,
    Recover { remaining: u32, landed: bool },
    FinishRecovery,
}

impl Battle {
    pub(crate) fn from_prepared(mut prepared: PreparedBattle) -> Self {
        let actors = prepared.actors;
        let actor_count = actors.len();
        let learning_members = prepared.technique_learning_members;
        let enemy_presentation = crate::enemy_presentation::State::new(
            &actors,
            &prepared.resources.enemy_presentations,
            prepared.entry_remaining != 0,
        );
        let entry_remaining = if enemy_presentation.opening() {
            0
        } else {
            prepared.entry_remaining
        };
        Self {
            runtime: prepared
                .resources
                .actor_setup
                .iter_mut()
                .enumerate()
                .map(|(index, setup)| {
                    let actor = &actors[index];
                    let target = prepared.targets[index];
                    ActorRuntime {
                        combo: Default::default(),
                        control: crate::control::Controller::from_setup(
                            ActorId(index as u8),
                            setup,
                            target,
                        ),
                        target,
                        enemy_choice: None,
                        last_executed_enemy_row: None,
                        enemy_effective_hit: false,
                        body_bypass_ticks: 0,
                        used_enemy_choices: Vec::new(),
                        enemy_walk_override: None,
                        enemy_behavior: Default::default(),
                        capture: Default::default(),
                        task: ActorTask::None,
                        idle_timer: 0,
                        companion_policy: setup.companion.as_ref().map(|companion| {
                            crate::CompanionPolicy {
                                choices: companion.initial_policy,
                            }
                        }),
                        recovery: Default::default(),
                        angel_tear_armed: actor.equipment.recovery.lethal.angel_tear,
                        support_target: ActorId(index as u8),
                        special_guard_pending: None,
                    }
                })
                .collect(),
            escape: crate::escape::State::new(prepared.resources.escape.as_ref()),
            items: Default::default(),
            diagnostics: resonance_content::diagnostics::Diagnostics::new(true),
            diagnostic: false,
            ledger: crate::Ledger::new(actor_count, prepared.resources.grade_rank),
            weapon_flights: BTreeMap::new(),
            cast_inputs: Default::default(),
            target_selector: None,
            command_target: None,
            pending_cues: Vec::new(),
            actor_start_centers: actors.iter().map(Actor::effect_origin).collect(),
            actors,
            random: Random::new(prepared.random_seed),
            model_requests: Vec::new(),
            models: None,
            camera: prepared.camera,
            entry_remaining,
            learning_members,
            volleys: BTreeMap::new(),
            projectiles: BTreeMap::new(),
            defer_projectile_anchors: false,
            update: 0,
            unison_gauge: prepared.resources.unison_gauge,
            unison: Default::default(),
            summon_entry: Default::default(),
            enemy_presentation,
            defense_system: Default::default(),
            timed_hold: None,
            event_boss: None,
            clock: BattleClock::Running,
            paused_actors: Vec::new(),
            next_action: 1,
            next_projectile: 1,
            terminal: Default::default(),
            ended: false,
            prepared: prepared.resources,
        }
    }

    pub fn actors(&self) -> &[Actor] {
        &self.actors
    }

    pub fn activity(&self, actor: ActorId) -> crate::Activity {
        self.runtime[actor.index()]
            .task
            .activity(&self.actors[actor.index()])
    }

    pub fn actor_ids(&self) -> impl ExactSizeIterator<Item = ActorId> + '_ {
        (0..self.actors.len()).map(|index| ActorId(index as u8))
    }

    /// Resolve prepared item identities, then publish all actor changes together.
    pub fn replace_equipment_batch(
        &mut self,
        replacements: Vec<EquipmentReplacement>,
    ) -> Result<()> {
        ensure!(
            self.phase() == crate::BattlePhase::Combat,
            "equipment requires combat"
        );
        ensure!(
            !replacements.is_empty(),
            "equipment replacement set is empty"
        );
        let mut ids = BTreeSet::new();
        for replacement in &replacements {
            let id = replacement.actor;
            ensure!(ids.insert(id), "equipment replacement actor is duplicated");
            let actor = self.actor(id)?;
            ensure!(
                actor.side == Side::Party,
                "equipment owner is not a party actor"
            );
            replacement.attributes.validate()?;
        }
        // Validate every native replacement before committing any of them.
        for replacement in replacements {
            let id = replacement.actor;
            let actor = &mut self.actors[id.index()];
            actor.equipment = replacement.attributes;
            actor.conditions = replacement.conditions;
            actor.hp = actor.hp.min(actor.equipment.max_hp);
            actor.tp = actor.tp.min(actor.equipment.max_tp);
            if let Some(items) = replacement.equipment {
                self.model_requests
                    .push(crate::ModelRequest::Equip { actor: id, items });
            }
        }
        Ok(())
    }

    /// Share the startup error policy and diagnostic history with the host.
    /// Standalone battles retain strict validation unless configured explicitly.
    pub fn set_diagnostics(&mut self, diagnostics: resonance_content::diagnostics::Diagnostics) {
        self.diagnostics = diagnostics;
    }

    pub fn diagnostics(&self) -> &resonance_content::diagnostics::Diagnostics {
        &self.diagnostics
    }

    /// A skipped simulation fault makes this run unsuitable for persistent
    /// results. Unrelated host or presentation diagnostics do not set this flag.
    pub fn is_diagnostic(&self) -> bool {
        self.diagnostic
    }

    /// Inspect native state without consuming pending feedback or advancing time.
    pub fn snapshot(&self) -> BattleFrame {
        self.frame(Vec::new(), None)
    }
    /// Publish pending feedback once, after all host mutations.
    pub fn publish(&mut self, mut cues: Vec<Cue>) -> BattleFrame {
        cues.append(&mut self.pending_cues);
        let mut frame = self.frame(cues, None);
        frame.model_requests = std::mem::take(&mut self.model_requests);
        frame
    }

    pub fn command_admission_allowed(&self) -> bool {
        self.phase() == crate::BattlePhase::Combat
            && !self.target_selector_active()
            && !self.unison.active()
            && !self.summon_entry.active()
            && !self.enemy_presentation.holding()
    }

    pub fn random_state(&self) -> u64 {
        self.random.state()
    }

    pub(crate) fn sequences(&self) -> impl Iterator<Item = (&ActionId, &Sequence)> {
        self.runtime
            .iter()
            .filter_map(|runtime| runtime.task.action())
    }

    #[cfg(test)]
    fn sequences_mut(&mut self) -> impl Iterator<Item = (&ActionId, &mut Sequence)> {
        self.runtime.iter_mut().filter_map(ActorRuntime::action_mut)
    }

    pub(crate) fn sequence(&self, id: &ActionId) -> Option<&Sequence> {
        self.sequences()
            .find_map(|(key, sequence)| (key == id).then_some(sequence))
    }

    #[cfg(test)]
    pub(crate) fn sequence_mut(&mut self, id: &ActionId) -> Option<&mut Sequence> {
        self.sequences_mut()
            .find_map(|(key, sequence)| (key == id).then_some(sequence))
    }

    pub(crate) fn take_sequence(&mut self, id: &ActionId) -> Option<Box<Sequence>> {
        if let Some(runtime) = self
            .runtime
            .iter_mut()
            .find(|runtime| runtime.task.action().is_some_and(|(key, _)| key == id))
            && let ActorTask::Action { sequence, .. } = std::mem::take(&mut runtime.task)
        {
            return Some(sequence);
        }
        None
    }

    pub(crate) fn set_task(&mut self, index: usize, task: ActorTask) {
        self.runtime[index].task = task;
    }

    pub(crate) fn put_sequence(&mut self, id: ActionId, sequence: Box<Sequence>) {
        let index = sequence.actor.index();
        debug_assert!(matches!(self.runtime[index].task, ActorTask::None));
        self.set_task(index, ActorTask::Action { id, sequence });
    }

    /// Prepared definition identity of a live actor action.
    pub fn action_definition(&self, handle: ActionId) -> Option<crate::ActionKey> {
        self.sequence(&handle).map(|s| s.action)
    }

    pub fn action_age(&self, handle: ActionId) -> Option<u32> {
        self.sequence(&handle)
            .map(|s| s.age)
            .or_else(|| self.volleys.get(&handle).map(|volley| volley.age))
            .or_else(|| {
                self.runtime
                    .iter()
                    .filter_map(|state| state.task.item())
                    .find(|item| item.action == handle)
                    .map(|item| item.elapsed)
            })
    }

    /// Remaining chant updates for an actor's live cast.
    pub fn casting_remaining(&self, actor: ActorId) -> Option<u32> {
        let ActorTask::Action { sequence, .. } = &self.runtime.get(actor.index())?.task else {
            return None;
        };
        match &sequence.execution {
            Execution::Casting(crate::casting::CastingRun::Running(cast)) => Some(cast.remaining),
            _ => None,
        }
    }

    pub fn action_recovery_remaining(&self, handle: ActionId) -> Option<u32> {
        match self.sequence(&handle)?.execution {
            Execution::Recovering { remaining } => Some(remaining),
            _ => None,
        }
    }

    pub(crate) fn interrupt_actor(&mut self, actor: ActorId, cues: &mut Vec<Cue>) {
        self.interrupt_capture(actor);
        let runtime = &mut self.runtime[actor.index()];
        runtime.combo = Default::default();
        self.actors[actor.index()].input = Default::default();
        self.actors[actor.index()].secondary_wind.set(false, 0);
        match std::mem::take(&mut runtime.task) {
            ActorTask::Item(item) => cues.push(Cue::Interrupted {
                action: item.action,
            }),
            ActorTask::Action { id, sequence } => {
                if matches!(sequence.execution, Execution::LeavingArena) {
                    self.actors[actor.index()].movement.steering.leaving_arena = false;
                }
                if !sequence.unison
                    && let Execution::Casting(crate::casting::CastingRun::Running(cast)) =
                        &sequence.execution
                {
                    cast.save_progress(&mut self.actors[actor.index()], sequence.action);
                }
                cues.push(Cue::Interrupted { action: id });
            }
            _ => {}
        }
        if let Some(control) = &mut runtime.control {
            control.cancel_action();
        }
        self.clear_special_guard(actor);
        self.runtime[actor.index()].special_guard_pending = None;
    }

    pub fn step(&mut self, input: BattleInput) -> Result<BattleFrame> {
        let cues = self.update(input, &mut crate::item::Unavailable)?;
        Ok(self.publish(cues))
    }

    /// A timed battle scene returns actors to their formation and ends their interrupted work.
    /// Headings and displayed model clips remain until the next ordinary actor visit.
    pub fn resume_event_combat(&mut self, formation: &[[f32; 3]]) -> Result<Vec<Cue>> {
        ensure!(
            !self.ended
                && self.phase() == crate::BattlePhase::Combat
                && self.clock == BattleClock::Held
                && formation.len() == self.actors.len()
                && formation.iter().flatten().all(|value| value.is_finite()),
            "invalid battle event return"
        );
        let mut cues = Vec::new();
        for (index, &position) in formation.iter().enumerate() {
            let id = ActorId(index as u8);
            self.interrupt_actor(id, &mut cues);
            self.actors[index].position = position;
            self.actors[index].reaction.recoil = Default::default();
            self.actors[index].movement.airborne_action = false;
            self.enter_idle(id);
        }
        Ok(cues)
    }

    /// Admit the party member already prepared for the encounter's rescue scene.
    pub fn activate_event_party_member(&mut self, id: ActorId) -> Result<()> {
        ensure!(
            !self.ended && self.phase() == crate::BattlePhase::Combat,
            "battle event party activation outside combat"
        );
        let actor = self
            .actors
            .get_mut(id.index())
            .context("missing battle event reserve")?;
        ensure!(
            actor.side == Side::Party && actor.availability == crate::ActorAvailability::Absent,
            "battle event party member is not reserved"
        );
        actor.availability = if actor.hp == 0 {
            crate::ActorAvailability::Dead
        } else if actor.conditions.effective().contains(Condition::Petrified) {
            crate::ActorAvailability::Petrified
        } else {
            crate::ActorAvailability::Active
        };
        Ok(())
    }

    /// Event ownership changes advance elapsed time while combat and actors remain held.
    pub fn advance_event_transition(&mut self) -> Result<()> {
        ensure!(
            !self.ended
                && self.phase() == crate::BattlePhase::Combat
                && self.clock == BattleClock::Held,
            "battle event transition requires held combat"
        );
        self.ledger.advance(false);
        self.update += 1;
        Ok(())
    }

    /// Advance simulation and return its events. Call `publish` after all host mutations.
    pub fn update(
        &mut self,
        input: BattleInput,
        items: &mut dyn crate::item::Provider,
    ) -> Result<Vec<Cue>> {
        ensure!(!self.ended, "battle has ended or faulted");
        // Reject stale/external input before mutating any state.
        self.validate_controls(&input.controllers)?;
        for request in &input.actions {
            self.actor(request.actor)?;
            self.actor(request.target)?;
            ensure!(
                self.prepared.actions.get(request.action).is_some(),
                "unknown battle action {}",
                request.action
            );
            ensure!(
                self.prepared.actor_setup[request.actor.index()]
                    .action_ids()
                    .any(|action| action == request.action),
                "battle action {} is not assigned to actor {}",
                request.action,
                request.actor.index()
            );
        }
        for action in &input.interrupt {
            ensure!(
                self.sequence(action).is_some()
                    || self.volleys.contains_key(action)
                    || self
                        .runtime
                        .iter()
                        .any(|state| state.task.item().is_some_and(|item| item.action == *action)),
                "stale battle action handle"
            );
        }
        for &actor in &input.stun_struggle {
            self.actor(actor)?;
        }
        self.sample_cast_inputs(&input.controllers);
        self.sample_target_holds(&input.controllers);
        self.clock = BattleClock::Held;
        let result = if input.paused {
            Ok(Vec::new())
        } else if self.summon_entry.active() {
            self.clock = BattleClock::SummonTransition;
            self.advance(input, items)
        } else {
            if !self.unison.active() {
                self.admit_target_selector(&input.controllers);
            }
            if self.target_selector.is_some() {
                self.control_target_selector(&input.controllers)
                    .map(|()| Vec::new())
            } else if self.command_target.is_some() {
                self.render_command_target().map(|()| Vec::new())
            } else {
                self.clock = self.timed_hold.map_or(BattleClock::Running, |hold| {
                    if let Some(actor) = hold.actor {
                        BattleClock::Rescue(actor)
                    } else {
                        self.cast_learning_actor()
                            .map_or(BattleClock::TimedHold, BattleClock::Learning)
                    }
                });
                if self.clock == BattleClock::Running
                    && self
                        .volleys
                        .values()
                        .any(crate::release::Volley::transitioning)
                {
                    self.clock = BattleClock::SpellTransition;
                }
                self.advance(input, items)
            }
        };
        if result.is_err() {
            self.invalidate();
        }
        result
    }

    fn advance(
        &mut self,
        input: BattleInput,
        items: &mut dyn crate::item::Provider,
    ) -> Result<Vec<Cue>> {
        let mut cues = Vec::new();
        if self.advance_enemy_presentation(&mut cues)? {
            self.paused_actors.clear();
            self.ledger.advance(false);
            self.update += 1;
            return Ok(cues);
        }
        if !self.clock.feedback_paused() {
            self.summon_entry.tick_flash();
        }
        if self.summon_entry.active() {
            self.paused_actors.clear();
            self.ledger
                .advance(self.phase() == crate::BattlePhase::Combat);
            self.advance_summon_entry(&mut cues)?;
            self.update += 1;
            return Ok(cues);
        }
        if !self.clock.paused() {
            self.step_unison(input.unison, &mut cues)?;
        }
        let entry_tick = if self.entry_remaining != 0 && !self.clock.paused() {
            self.entry_remaining -= 1;
            Some(crate::prepare::ENTRY_RESUME_UPDATE - self.entry_remaining)
        } else {
            None
        };
        let entry_hold = entry_tick.is_some()
            && (1..=crate::prepare::ENTRY_SETTLE_UPDATES).contains(&self.entry_remaining);
        if entry_hold {
            self.clock = BattleClock::Held;
        }
        if !self.clock.paused() && !self.unison.active() {
            self.retarget_selected_opponent()?;
        }
        self.recognize_update();
        self.ledger
            .advance(self.phase() == crate::BattlePhase::Combat);
        if !self.unison.active()
            && self.phase() != crate::BattlePhase::Results
            && !self.terminal.ordinary_camera_finished
        {
            if let Some(camera) = &mut self.camera {
                if let Some(tick) =
                    entry_tick.filter(|tick| *tick <= crate::prepare::ENTRY_PAN_UPDATES)
                {
                    camera.frame_entry(tick);
                } else {
                    camera.step(
                        &self.actors,
                        self.runtime[camera.definition.leader.index()].target,
                        if self.clock.feedback_paused() && !entry_hold
                            || self.entry_remaining == crate::prepare::ENTRY_SETTLE_UPDATES
                        {
                            crate::camera::Update::Held
                        } else if let BattleClock::Learning(actor) = self.clock {
                            crate::camera::Update::Spell(actor)
                        } else if let Some(volley) =
                            self.volleys.values().find(|volley| volley.transitioning())
                        {
                            crate::camera::Update::Spell(volley.actor)
                        } else {
                            crate::camera::Update::Tracking
                        },
                    )?;
                }
            }
        }
        // The combat ledger runs while actors, scenery, and effects wait for framing.
        if !entry_hold {
            if self.unison.holds_world() && !self.clock.paused() {
                self.paused_actors.clear();
            } else if self.clock == BattleClock::SpellTransition {
                self.advance_volleys(&mut cues, true)?;
            } else if self.clock.paused() {
                self.advance_timed_hold();
            } else {
                self.advance_gameplay(input, &mut cues, items)?;
            }
        }
        if self.phase() == crate::BattlePhase::Results {
            if let Some(camera) = &mut self.camera {
                camera.frame_results();
            }
        }
        self.terminal.ordinary_camera_finished |= self.terminal.result.is_some();
        self.update += 1;
        Ok(cues)
    }

    fn advance_gameplay(
        &mut self,
        input: BattleInput,
        cues: &mut Vec<Cue>,
        items: &mut dyn crate::item::Provider,
    ) -> Result<()> {
        for actor in &mut self.actors {
            actor.time_stop = actor.time_stop.saturating_sub(1);
        }
        self.advance_defense_system(cues)?;
        self.advance_escape();
        self.items.cooldown = self.items.cooldown.saturating_sub(1);
        for action in input.interrupt {
            if let Some(index) = self.runtime.iter().position(|state| match &state.task {
                ActorTask::Action { id, .. } => *id == action,
                ActorTask::Item(item) => item.action == action,
                _ => false,
            }) {
                let actor = ActorId(index as u8);
                self.interrupt_actor(actor, cues);
                self.enter_idle(actor);
            } else if self.volleys.remove(&action).is_some() {
                cues.push(Cue::Interrupted { action });
            }
        }
        if !self.unison.staged() {
            for request in input.actions {
                self.start(request, cues)?;
            }
        }
        let mut contacts = Contacts::default();
        let mut transitions = BTreeMap::new();
        let body_positions: Vec<_> = self.actors.iter().map(|actor| actor.position).collect();
        let mut body_steps = vec![0.; self.actors.len()];
        self.defer_projectile_anchors = true;
        let actor_result = self.advance_actors(
            &input.stun_struggle,
            &input.controllers,
            &mut body_steps,
            &mut contacts,
            &mut transitions,
            cues,
            items,
        );
        self.defer_projectile_anchors = false;
        actor_result?;
        // Entry interrupts the old world on its releasing actor's visit. In particular,
        // contacts already collected by earlier actors must not escape into the scene.
        if self.summon_entry.active() {
            self.paused_actors.clear();
            return Ok(());
        }
        let resolve_contacts = self.phase() != crate::BattlePhase::Entry;
        let body_placements = self
            .models
            .as_ref()
            .map(crate::Models::body_placements)
            .unwrap_or_default();
        let defense_arm_positions = self.defense_arm_positions()?;
        self.prepare_contact_models(resolve_contacts, crate::model::UpdatePhase::PoseOnly)?;
        // The introduction preserves the prepared formation.
        if resolve_contacts && !self.unison.staged() {
            self.push_bodies(&body_positions, &body_steps, &body_placements);
        }
        self.attach_defense_arms(defense_arm_positions);
        for index in 0..self.actors.len() {
            self.advance_weapon_flights(ActorId(index as u8), &mut contacts, cues)?;
        }

        let ids: Vec<_> = self.projectiles.keys().rev().copied().collect();
        self.defer_projectile_anchors = true;
        let projectile_result: Result<()> = (|| {
            for &id in &ids {
                if !self.projectiles[&id].pending_launch {
                    self.visit_projectile(id, true, &mut contacts, cues)?;
                }
            }
            Ok(())
        })();
        self.defer_projectile_anchors = false;
        projectile_result?;
        // Refresh placement and late requests; this update's pose clock already advanced.
        self.prepare_contact_models(resolve_contacts, crate::model::UpdatePhase::FinishPose)?;
        if resolve_contacts && !self.unison.staged() {
            for index in 0..self.actors.len() {
                if self.actors[index].time_stop == 0 {
                    self.follow_capture(ActorId(index as u8))?;
                }
            }
        }
        let launches: Vec<_> = self
            .projectiles
            .iter()
            .rev()
            .filter_map(|(&id, projectile)| projectile.pending_launch.then_some(id))
            .collect();
        for id in launches {
            // New actor emissions still move on their authored update. A child emitted
            // during the projectile loop retains the ordinary next-update first visit.
            self.visit_projectile(id, ids.contains(&id), &mut contacts, cues)?;
        }
        if resolve_contacts {
            contacts.resolve(self, cues, &body_placements)?;
            for index in 0..self.actors.len() {
                if !self.actors[index].available() && self.runtime[index].task.action().is_some() {
                    self.interrupt_actor(ActorId(index as u8), cues);
                }
            }
        }
        // Every attack keeps its airborne classification through the landing contacts,
        // including martial successors that no longer own a normal recovery callback.
        for actor in &mut self.actors {
            if !actor.airborne() {
                actor.movement.airborne_action = false;
            }
        }
        // Resolve the current attack before replacing its power, hit history, or recovery.
        for (id, transition) in transitions {
            let Some(mut sequence) = self.take_sequence(&id) else {
                continue;
            };
            match self.commit_action_transition(id, &mut sequence, transition, cues) {
                Ok(true) => {
                    self.put_sequence(id, sequence);
                }
                Ok(false) => {}
                Err(error) => {
                    self.diagnostics.report("battle action transition", error)?;
                    self.diagnostic = true;
                    self.discard_action(id, &sequence, cues);
                }
            }
        }
        if resolve_contacts {
            self.advance_volleys(cues, false)?;
        }
        Ok(())
    }

    fn visit_projectile(
        &mut self,
        id: ProjectileId,
        advance: bool,
        contacts: &mut Contacts,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        if self.projectiles[&id].retiring {
            self.expire_projectile(id, cues);
            return Ok(());
        }
        let result = (|| {
            if self.projectiles[&id].pending_launch {
                let projectile = &self.projectiles[&id];
                let (position, direction) = self.projectile_launch_anchors(
                    &projectile.definition.motion,
                    projectile.frame.owner,
                    projectile.frame.position,
                )?;
                let projectile = self.projectiles.get_mut(&id).unwrap();
                projectile.frame.position = position;
                if let Some(birth) = projectile.finish_launch(&mut self.pending_cues, direction) {
                    self.pending_cues.push(Cue::Effect(birth));
                }
            }
            if !advance {
                return Ok(());
            }
            self.advance_projectile_seal(id, cues)?;
            let projectile = self.projectiles.get_mut(&id).unwrap();
            if projectile.definition.motion.aqua_edge {
                projectile.target_point = self.actors[projectile.frame.target.index()].position;
            } else if projectile.definition.motion.departure.is_some() {
                projectile.target_point = self.actors[projectile.frame.owner.index()].position;
            }
            projectile.step()?;
            if let Some(first) = projectile.take_rebound() {
                if first {
                    self.random.next_u16();
                }
                cues.push(Cue::ProjectileRebound {
                    projectile: id,
                    first,
                });
            }
            contacts.submit(projectile)?;
            for effect in projectile.effects() {
                cues.push(Cue::Effect(effect));
            }
            if let Some((target, definition)) = projectile.take_rebound_projectile() {
                let (action, owner, heading) = (
                    projectile.action,
                    projectile.frame.owner,
                    projectile.frame.heading,
                );
                let position = self.actor(target)?.position;
                self.emit_at(definition, action, owner, target, position, heading)?;
            }
            Ok(())
        })();
        if let Err(error) = result {
            self.diagnostics.report("battle projectile", error)?;
            self.diagnostic = true;
            contacts
                .0
                .retain(|contact| contact.source != ContactSource::Projectile(id));
            self.expire_projectile(id, cues);
        }
        Ok(())
    }

    fn prepare_contact_models(
        &mut self,
        settle: bool,
        phase: crate::model::UpdatePhase,
    ) -> Result<()> {
        if self.models.is_some() {
            let mut frame = self.frame(Vec::new(), None);
            // All geometry visits share one pose clock for this update.
            frame.update += 1;
            frame.model_requests = std::mem::take(&mut self.model_requests);
            self.models
                .as_mut()
                .unwrap()
                .advance_for_contacts(&mut frame, self.clock, settle, phase)?;
            self.model_requests.append(&mut frame.model_requests);
        }
        Ok(())
    }

    /// Volleys run after contacts in release order, independently of their casters.
    pub(crate) fn advance_volleys(
        &mut self,
        cues: &mut Vec<Cue>,
        transitions_only: bool,
    ) -> Result<()> {
        let first_projectile = ProjectileId(self.next_projectile);
        let ids: Vec<_> = self
            .volleys
            .iter()
            .filter(|(_, volley)| !transitions_only || volley.transitioning())
            .map(|(&id, _)| id)
            .collect();
        for id in ids {
            let mut volley = self.volleys.remove(&id).unwrap();
            match volley.step(self, id, cues) {
                Ok(true) => cues.push(Cue::Completed { action: id }),
                Ok(false) => {
                    self.volleys.insert(id, volley);
                }
                Err(error) => {
                    self.diagnostics.report("spell volley", error)?;
                    self.diagnostic = true;
                    self.discard_projectiles(id, cues);
                    cues.push(Cue::Interrupted { action: id });
                }
            }
        }
        // Spell emissions occur after the ordinary projectile pass. Give their
        // newborns the same first motion/effect visit as actor emissions; age zero
        // cannot submit a contact, so contact resolution stays in its usual phase.
        let newborns: Vec<_> = self
            .projectiles
            .range(first_projectile..)
            .rev()
            .map(|(&id, _)| id)
            .collect();
        let mut contacts = Contacts::default();
        for id in newborns {
            self.visit_projectile(id, true, &mut contacts, cues)?;
        }
        Ok(())
    }

    fn advance_actors(
        &mut self,
        struggle: &[ActorId],
        input: &[crate::ControlInput],
        body_steps: &mut [f32],
        contacts: &mut Contacts,
        transitions: &mut BTreeMap<ActionId, ActionTransition>,
        cues: &mut Vec<Cue>,
        items: &mut dyn crate::item::Provider,
    ) -> Result<()> {
        self.paused_actors.clear();
        for index in 0..self.actors.len() {
            if self.summon_entry.active() {
                break;
            }
            let actor = ActorId(index as u8);
            let before = self.actors[index].position;
            if self.actors[index].hit_stop != 0 || self.actors[index].time_stop != 0 {
                self.paused_actors.push(actor);
            }
            if self.actors[index].availability == crate::ActorAvailability::Absent {
                continue;
            }
            self.actor_start_centers[index] = self.actors[index].effect_origin();
            for volley in self
                .volleys
                .values_mut()
                .filter(|volley| volley.actor == actor)
            {
                volley.projectile_target = self.actor_start_centers[volley.target.index()];
            }
            if self.unison.staged() {
                if self.unison.target() == Some(actor) {
                    let owner = &mut self.actors[index];
                    if owner.hit_stop != 0 {
                        owner.hit_stop -= 1;
                    } else {
                        owner
                            .movement
                            .integrate_along(&mut owner.position, owner.reaction.direction);
                        owner.movement.brake(owner.position[1], Activity::Hurt);
                    }
                    let position = self.actors[index].position;
                    body_steps[index] = (position[0] - before[0]).hypot(position[2] - before[2]);
                    self.constrain_actor(index);
                    continue;
                }
                if self.unison.holds_actor(actor) {
                    continue;
                }
            }
            if self.phase() == crate::BattlePhase::Entry {
                self.advance_actor_common(index, cues)?;
                continue;
            }
            if self.actors[index].time_stop != 0 {
                continue;
            }
            if self.is_captured(actor) {
                let hit_stop = self.actors[index].hit_stop;
                self.advance_actor_common(index, cues)?;
                self.actors[index].hit_stop = hit_stop;
                continue;
            }
            if !self.advance_escape_actor(actor, cues)? {
                match self.actors[index].availability {
                    crate::ActorAvailability::Absent => unreachable!(),
                    crate::ActorAvailability::Dead => self.advance_dead(index),
                    crate::ActorAvailability::Petrified => self.advance_petrified(index),
                    crate::ActorAvailability::Active => {
                        if let Some((id, transition)) = self.advance_active_actor(
                            actor,
                            input,
                            struggle.contains(&actor),
                            contacts,
                            cues,
                            items,
                        )? {
                            transitions.insert(id, transition);
                        }
                    }
                }
            }
            if self.summon_entry.active() {
                break;
            }
            // Arena correction is not movement and must not start body separation.
            let position = self.actors[index].position;
            body_steps[index] = (position[0] - before[0]).hypot(position[2] - before[2]);
            self.constrain_actor(index);
            self.publish_landing(index, cues);
            let owner = &mut self.actors[index];
            ensure!(
                owner.position.iter().all(|v| v.is_finite()),
                "battle movement overflow"
            );
            owner.movement.validate()?;
            self.advance_actor_common(index, cues)?;
        }
        Ok(())
    }

    /// Commands and tasks run only for available, unfrozen actors.
    /// Their transition commits after this update's contacts have resolved.
    fn advance_active_actor(
        &mut self,
        actor: ActorId,
        input: &[crate::ControlInput],
        struggling: bool,
        contacts: &mut Contacts,
        cues: &mut Vec<Cue>,
        items: &mut dyn crate::item::Provider,
    ) -> Result<Option<(ActionId, ActionTransition)>> {
        let index = actor.index();
        if self.phase() == crate::BattlePhase::Combat && self.actors[index].hit_stop == 0 {
            self.advance_enemy_transformation(index)?;
        }
        let mut next = None;
        let controls = input
            .iter()
            .find(|input| input.actor == actor)
            .copied()
            .unwrap_or_else(|| crate::ControlInput::neutral(actor));
        if let Some(target) = self.target(actor) {
            self.actors[index].movement.target_direction = crate::distance::planar_direction(
                self.actors[target.index()].position,
                self.actors[index].position,
                self.actors[index].movement.target_direction,
            );
        }
        let unison = self.runtime[index]
            .task
            .action()
            .is_some_and(|(_, sequence)| sequence.unison)
            || self.unison.owns_commands(actor);
        if !unison {
            self.sample_ground_stick(index, controls.stick[0]);
            self.start_pending_item(actor, cues)?;
            let activity = self.activity(actor);
            crate::knockdown::sample_contact_recovery(
                &mut self.actors[index],
                activity,
                self.prepared.actor_setup[index].contact_recovery,
                controls.guard.pressed,
                || self.random.next_u16(),
            );
            self.control_actor(actor, controls, cues)?;
            if self.actors[index].control == crate::Control::Auto
                && matches!(self.activity(actor), Activity::Idle | Activity::Guarding)
                && self.actors[index].hit_stop == 0
            {
                self.consume_auto_special_guard(actor, cues)?;
            }
            self.advance_ai(actor, cues)?;
        }
        if self.advance_enemy_departure(actor, cues)? {
            return Ok(None);
        }
        let task_movement = match self.runtime[index].task {
            ActorTask::Approach(_) => self.update_approach(actor, cues)?,
            ActorTask::Returning(state) => self.update_recovery_return(actor, state)?,
            ActorTask::Hurt { remaining } => self.advance_hurt(actor, remaining, input, cues)?,
            ActorTask::Recovering {
                remaining,
                after_hit,
            } => {
                if remaining == 0 {
                    self.enter_idle(actor);
                } else {
                    self.set_task(
                        index,
                        ActorTask::Recovering {
                            remaining: remaining - 1,
                            after_hit,
                        },
                    );
                }
                false
            }
            ActorTask::Paralysis { remaining } => self.advance_paralysis(actor, remaining)?,
            ActorTask::Stunned { remaining } => {
                self.advance_stun(actor, remaining, struggling, cues)
            }
            ActorTask::Knockdown(recovery) => self.advance_knockdown(actor, recovery, cues)?,
            ActorTask::Mobility(_) => self.update_mobility(actor, cues)?,
            ActorTask::Taunt { remaining } => {
                self.advance_taunt(actor, remaining, controls, cues)?
            }
            ActorTask::Item(item) => {
                self.advance_item(actor, item, items, cues)?;
                false
            }
            ActorTask::Action { .. } | ActorTask::None | ActorTask::Escaping => false,
        };
        let movement_handled =
            task_movement || self.update_control_stop(index)? || self.update_guard(index, cues)?;
        let facing_ready = self.face_active_action(actor);
        if facing_ready
            && let Some((&id, _)) = self.runtime[index].task.action()
            && let Some(transition) = self.advance_sequence(id, contacts, items, cues)?
        {
            next = Some((id, transition));
        }
        if self.summon_entry.active() {
            return Ok(next);
        }
        let activity = self.activity(actor);
        if !movement_handled && facing_ready {
            let actor = &mut self.actors[index];
            if !matches!(
                activity,
                Activity::Stunned | Activity::KnockedDown | Activity::GettingUp
            ) && (actor.hit_stop == 0
                || !matches!(activity, Activity::Action | Activity::Casting { .. }))
            {
                actor.movement.integrate(&mut actor.position);
                if matches!(
                    activity,
                    crate::Activity::Action
                        | crate::Activity::Casting { .. }
                        | crate::Activity::Recovering
                        | crate::Activity::Item
                ) {
                    actor.movement.brake(actor.position[1], activity);
                }
            }
            if activity == crate::Activity::Idle {
                self.advance_hover(index, true)?;
            }
        }
        if !movement_handled
            && facing_ready
            && let Some((id, recovery)) = self.brake_control(index)?
        {
            next = Some((
                id,
                ActionTransition::Recover {
                    remaining: recovery,
                    landed: true,
                },
            ));
        }
        // A policy change applies to movement beginning with the next update.
        self.update_enemy_walk_override(actor);
        Ok(next)
    }

    fn commit_action_transition(
        &mut self,
        id: ActionId,
        sequence: &mut Sequence,
        transition: ActionTransition,
        cues: &mut Vec<Cue>,
    ) -> Result<bool> {
        if matches!(transition, ActionTransition::Recover { landed: true, .. }) {
            self.finish_normal_landing(sequence);
        }
        if matches!(
            &sequence.definition.execution,
            crate::ActionExecution::Attack(_)
        ) && (self.chain_enemy(id, sequence, cues)? || self.chain_normal(id, sequence, cues)?)
        {
            return Ok(false);
        }
        match transition {
            ActionTransition::Recover { remaining, .. } => {
                sequence.recover(self, id, remaining, cues)?;
                self.restore_contact_recovery_expression(sequence);
            }
            ActionTransition::FinishRecovery => {
                if sequence.unison {
                    self.enter_idle(sequence.actor);
                } else {
                    self.complete_ordinary_action(sequence.actor)?;
                }
                cues.push(Cue::Completed { action: id });
                return Ok(false);
            }
            ActionTransition::Advance => {
                if sequence.events_complete() {
                    self.enter_idle(sequence.actor);
                    cues.push(Cue::Completed { action: id });
                    return Ok(false);
                }
                sequence.age += 1;
            }
        }
        Ok(true)
    }

    fn advance_sequence(
        &mut self,
        id: ActionId,
        contacts: &mut Contacts,
        items: &mut dyn crate::item::Provider,
        cues: &mut Vec<Cue>,
    ) -> Result<Option<ActionTransition>> {
        let mut sequence = self.take_sequence(&id).unwrap();
        match self.advance_sequence_inner(id, &mut sequence, contacts, items, cues) {
            Ok(SequenceStep::Finished) => Ok(None),
            Ok(step) => {
                self.put_sequence(id, sequence);
                Ok(match step {
                    SequenceStep::Transition(transition) => Some(transition),
                    _ => None,
                })
            }
            Err(error) => {
                self.diagnostics.report("battle action", error)?;
                self.diagnostic = true;
                // Cancel partially executed work and its pending contacts; never retry it.
                self.discard_action(id, &sequence, cues);
                contacts.0.retain(|contact| match contact.source {
                    ContactSource::Melee { action, .. } | ContactSource::Weapon { action, .. } => {
                        action != id
                    }
                    ContactSource::Projectile(projectile) => {
                        self.projectiles.contains_key(&projectile)
                    }
                });
                Ok(None)
            }
        }
    }

    /// Discard a detached sequence and its gameplay work. Released particles finish independently.
    pub(crate) fn discard_action(
        &mut self,
        id: ActionId,
        sequence: &Sequence,
        cues: &mut Vec<Cue>,
    ) {
        self.enter_idle(sequence.actor);
        let flights: Vec<_> = self
            .weapon_flights
            .iter()
            .filter(|(_, flight)| flight.action == id)
            .map(|(&key, _)| key)
            .collect();
        for (owner, slot) in flights {
            self.retire_weapon_flight(owner, slot);
        }
        self.discard_projectiles(id, cues);
        cues.push(Cue::Interrupted { action: id });
    }

    pub(crate) fn discard_projectiles(&mut self, action: ActionId, cues: &mut Vec<Cue>) {
        let projectiles: Vec<_> = self
            .projectiles
            .iter()
            .filter_map(|(&id, projectile)| (projectile.action == action).then_some(id))
            .collect();
        for projectile in projectiles {
            self.expire_projectile(projectile, cues);
        }
    }

    fn advance_sequence_inner(
        &mut self,
        id: ActionId,
        sequence: &mut Sequence,
        contacts: &mut Contacts,
        items: &mut dyn crate::item::Provider,
        cues: &mut Vec<Cue>,
    ) -> Result<SequenceStep> {
        let actor_action = matches!(
            &sequence.definition.execution,
            crate::ActionExecution::Attack(_)
        );
        if !self.actor(sequence.actor)?.available() {
            self.clear_special_guard(sequence.actor);
            cues.push(Cue::Interrupted { action: id });
            return Ok(SequenceStep::Finished);
        }
        let index = sequence.actor.index();
        if actor_action && sequence.started_at != self.update {
            self.sample_action_combo(sequence);
        }
        if actor_action && self.actors[index].hit_stop != 0 {
            return Ok(SequenceStep::Continue);
        }
        if let Execution::Recovering { remaining } = &mut sequence.execution {
            let actor = &self.actors[index];
            let finished = *remaining == 0 && !actor.needs_landing();
            *remaining = remaining.saturating_sub(1);
            return Ok(if finished {
                SequenceStep::Transition(ActionTransition::FinishRecovery)
            } else {
                SequenceStep::Continue
            });
        }
        let contact_recovery = sequence.contact_recovery_requested();
        let recovery = step_sequence(self, id, sequence, contacts, items, cues)?;
        if !self.actors[index].available() {
            cues.push(Cue::Interrupted { action: id });
            return Ok(SequenceStep::Finished);
        }
        if actor_action && sequence.finished() {
            cues.push(Cue::Interrupted { action: id });
            contacts.0.retain(|contact| {
                !matches!(contact.source, ContactSource::Melee { action, .. } if action == id)
            });
            return Ok(SequenceStep::Finished);
        }
        if contact_recovery {
            if self.chain_enemy(id, sequence, cues)? {
                return Ok(SequenceStep::Finished);
            }
            self.enter_contact_recovery(id, sequence, cues)?;
            contacts.0.retain(|contact| {
                !matches!(contact.source,
                ContactSource::Melee { action, .. } if action == id)
            });
            return Ok(SequenceStep::Continue);
        }
        if actor_action {
            return Ok(SequenceStep::Transition(recovery.map_or(
                ActionTransition::Advance,
                |remaining| ActionTransition::Recover {
                    remaining,
                    landed: false,
                },
            )));
        }
        if let Some(remaining) = recovery {
            return Ok(SequenceStep::Transition(ActionTransition::Recover {
                remaining,
                landed: false,
            }));
        }
        if sequence.finished() {
            self.enter_idle(sequence.actor);
            cues.push(Cue::Completed { action: id });
            return Ok(SequenceStep::Finished);
        }
        sequence.age += 1;
        Ok(SequenceStep::Continue)
    }

    pub(crate) fn frame(&self, cues: Vec<Cue>, outcome: Option<BattleOutcome>) -> BattleFrame {
        BattleFrame {
            clock: self.clock,
            paused_actors: self.paused_actors.clone(),
            unison_gauge: self.unison_gauge,
            unison_available: self.prepared.unison_available,
            unison: self.unison.frame(self.actors.len()),
            summon: self.summon_entry.frame(),
            enemy_presentation: self.enemy_presentation.frame(),
            escape: self.escape_frame(),
            item_cooldown: self.items.cooldown,
            hourglass_remaining: self.hourglass_remaining(),
            scanned_enemies: self.items.revealed,
            update: self.update,
            targets: (0..self.actors.len())
                .map(|index| {
                    let actor = ActorId(index as u8);
                    self.command_target
                        .filter(|(issuer, _)| *issuer == actor)
                        .map(|(_, target)| target)
                        .or_else(|| self.target(actor))
                })
                .collect(),
            target_selector: self
                .target_selector
                .or_else(|| self.command_target.map(|(actor, _)| actor)),
            actors: self
                .actors
                .iter()
                .zip(&self.runtime)
                .map(|(actor, runtime)| ActorFrame {
                    state: actor.clone(),
                    activity: runtime.task.activity(actor),
                })
                .collect(),
            models: Vec::new(),
            independent_models: Vec::new(),
            weapons: Vec::new(),
            model_requests: Vec::new(),
            weapon_flights: self
                .weapon_flights
                .iter()
                .filter(|(_, flight)| !flight.caught)
                .map(|(&(owner, slot), flight)| crate::WeaponFlightFrame {
                    owner,
                    slot,
                    position: flight.position,
                    direction: flight.direction,
                })
                .collect(),
            actions: self
                .sequences()
                .map(|(&id, s)| (id, s.actor, s.age))
                .chain(
                    self.volleys
                        .iter()
                        .map(|(&id, volley)| (id, volley.actor, volley.age)),
                )
                .chain(
                    self.runtime
                        .iter()
                        .enumerate()
                        .filter_map(|(index, state)| {
                            state
                                .task
                                .item()
                                .map(|item| (item.action, ActorId(index as u8), item.elapsed))
                        }),
                )
                .collect(),
            spell_origins: self
                .volleys
                .iter()
                .filter_map(|(&id, volley)| volley.origin.map(|point| (id, point)))
                .collect(),
            cues,
            recognized_result: self.terminal.result,
            projectile_shadows: self
                .projectiles
                .values()
                .filter_map(Projectile::shadow_frame)
                .collect(),
            camera: self
                .enemy_presentation
                .camera()
                .or_else(|| self.summon_entry.camera())
                .or_else(|| self.unison.camera())
                .or_else(|| self.camera.as_ref().map(|camera| camera.frame())),
            camera_fov_degrees: None,
            event_fade: None,
            projectiles: self.projectiles.values().map(|p| p.frame.clone()).collect(),
            outcome,
        }
    }

    pub(crate) fn start(&mut self, request: ActionRequest, cues: &mut Vec<Cue>) -> Result<()> {
        self.start_actor_command(request, cues).map(|_| ())
    }

    pub(crate) fn action_rejection(&self, request: ActionRequest) -> Result<Option<Rejection>> {
        let actor = self.actor(request.actor)?;
        let busy = matches!(
            self.activity(request.actor),
            crate::Activity::Hurt
                | crate::Activity::KnockedDown
                | crate::Activity::GettingUp
                | crate::Activity::Stunned
        ) || self.activity(request.actor) == crate::Activity::Guarding
            && self.prepared.special_guard(request.actor) != Some(request.action)
            || self.runtime[request.actor.index()].task.item().is_some()
            || matches!(
                self.runtime[request.actor.index()].task,
                ActorTask::Recovering { .. }
            )
            || self.runtime[request.actor.index()].task.action().is_some();
        let reason = if self.terminal.result.is_some() {
            Some(Rejection::BattleEnding)
        } else if actor.availability == crate::ActorAvailability::Petrified {
            Some(Rejection::Petrified)
        } else if !actor.available() {
            Some(Rejection::Defeated)
        } else if self.phase() == crate::BattlePhase::Entry
            || busy
            || self.unison.owns_commands(request.actor)
            || actor.time_stop != 0
            || self.clock.paused()
        {
            Some(Rejection::Busy)
        } else {
            self.action_candidate(
                request.actor,
                request.action,
                self.selection_range(request.actor, request.action),
            )
            .err()
        };
        Ok(reason)
    }

    pub(crate) fn start_actor_command(
        &mut self,
        request: ActionRequest,
        cues: &mut Vec<Cue>,
    ) -> Result<bool> {
        if let Some(reason) = self.action_rejection(request)? {
            cues.push(Cue::Rejected {
                actor: request.actor,
                reason,
            });
            return Ok(false);
        }
        self.start_admitted_action(request, cues)?;
        Ok(true)
    }

    /// The Unison scene has selected and placed this actor. Run its prepared arte
    /// without ordinary TP admission, learning rolls or player action chaining.
    pub(crate) fn start_unison_arte(
        &mut self,
        request: ActionRequest,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        self.start_unison_action(request, true, cues)
    }

    pub(crate) fn start_unison_combination(
        &mut self,
        request: ActionRequest,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        self.start_unison_action(request, false, cues)
    }

    fn start_unison_action(
        &mut self,
        request: ActionRequest,
        arte: bool,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        let owner = self.actor(request.actor)?;
        let target = self.actor(request.target)?;
        ensure!(
            self.phase() == crate::BattlePhase::Combat
                && owner.side == Side::Party
                && owner.available()
                && !owner.conditions.effective().contains(Condition::Curse)
                && target.side == Side::Enemy
                && target.available(),
            "invalid Unison arte participants"
        );
        if arte {
            ensure!(
                self.learned_technique(request.actor, request.action)
                    .is_some_and(|row| row.capabilities.target == crate::TechniqueTarget::Enemy),
                "Unison arte is not a learned offensive technique"
            );
            self.record_unison_use(request.actor, request.action)?;
        }
        let (id, mut sequence) =
            self.allocate_sequence(request.action, request.actor, request.target)?;
        sequence.unison = true;
        sequence.combo = None;
        self.cancel_pending_item(request.actor);
        self.interrupt_actor(request.actor, cues);
        self.clear_cast_memory(request.actor);
        self.clear_matching_technique_command(request.actor, request.action);
        self.actors[request.actor.index()].proficiency = 0;
        if arte {
            self.snapshot_technique_proficiency(request.actor, request.action);
        }
        let index = request.actor.index();
        self.runtime[index].target = request.target;
        self.actors[index].guard.active = false;
        self.actors[index].attack_power = 100;
        self.actors[index].movement.locomotion = crate::Locomotion::Action;
        self.put_sequence(id, sequence);
        cues.push(Cue::Started {
            definition: Some(request.action),
            action: id,
            actor: request.actor,
        });
        Ok(())
    }

    /// Commit an action immediately after its admission check.
    pub(crate) fn start_admitted_action(
        &mut self,
        request: ActionRequest,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        let action = request.action;
        let debit = if matches!(
            &self.prepared.actions[action].execution,
            crate::ActionExecution::Attack(_)
        ) {
            self.commit_martial_use(request.actor, action)?;
            self.action_quote(request.actor, action)
        } else {
            0
        };
        // Commit payment before any action commands execute.
        ensure!(
            debit <= u32::from(self.actors[request.actor.index()].tp),
            "admitted action exceeds available TP"
        );
        let (id, mut sequence) = self.allocate_sequence(action, request.actor, request.target)?;
        let index = request.actor.index();
        self.actors[index].tp -= debit as u16;
        {
            self.set_task(index, ActorTask::None);
            let actor = &mut self.actors[index];
            actor.movement.locomotion = crate::Locomotion::Action;
            if matches!(
                &sequence.definition.execution,
                crate::ActionExecution::Casting(_)
            ) {
                actor.casting_state.power_percent = 100;
            }
        }
        {
            let technique = self
                .selection_definition(request.actor, action)
                .map(|row| row.capabilities);
            self.initialize_normal(&mut sequence);
            let combo = &mut self.runtime[index].combo;
            if sequence.normal.is_some() && combo.last.is_some() {
                combo.normal_links += 1;
            }
            combo.record(action, technique);
        }
        match &sequence.definition.execution {
            crate::ActionExecution::LeaveArena => {
                self.begin_enemy_departure(request.actor, request.target)?;
            }
            crate::ActionExecution::Attack(attack) => {
                if sequence.definition.normal.is_some() {
                    self.apply_normal_guard(request.actor, attack.end_at);
                }
                self.clear_matching_technique_command(request.actor, action);
                self.snapshot_technique_proficiency(request.actor, action);
            }
            crate::ActionExecution::Casting(cast) => {
                self.snapshot_technique_proficiency(request.actor, action);
                if !cast.release.recovery.is_empty()
                    && self.actors[request.target.index()].side == self.actors[index].side
                {
                    self.runtime[index].support_target = request.target;
                } else if matches!(
                    self.actors[index].control,
                    crate::Control::Manual | crate::Control::SemiAuto
                ) {
                    self.runtime[index].support_target = request.actor;
                }
            }
        }
        self.put_sequence(id, sequence);
        self.record_enemy_action(request.actor, action);
        cues.push(Cue::Started {
            definition: Some(action),
            action: id,
            actor: request.actor,
        });
        Ok(())
    }

    pub(crate) fn allocate_sequence(
        &mut self,
        definition: crate::ActionKey,
        actor: ActorId,
        target: ActorId,
    ) -> Result<(ActionId, Box<Sequence>)> {
        let sequence = Sequence::new(
            definition,
            &self.prepared.actions[definition],
            actor,
            target,
            self.update,
        );
        let id = self.allocate_action_id()?;
        Ok((id, Box::new(sequence)))
    }

    pub(crate) fn allocate_action_id(&mut self) -> Result<ActionId> {
        let id = ActionId(self.next_action);
        self.next_action = self
            .next_action
            .checked_add(1)
            .context("battle action handle exhausted")?;
        Ok(id)
    }

    pub(crate) fn actor(&self, id: ActorId) -> Result<&Actor> {
        self.actors
            .get(id.index())
            .ok_or_else(|| anyhow::anyhow!("invalid battle actor handle"))
    }

    pub(crate) fn action_quote(&self, actor: ActorId, key: crate::ActionKey) -> u32 {
        let action = &self.prepared.actions[key];
        self.prepared
            .special_guard(actor)
            .filter(|&binding| binding == key && self.special_guard_learned(actor, binding))
            .map_or_else(
                || crate::tp::action_quote(&self.actors[actor.index()], key, action),
                |_| crate::tp::special_guard_debit(&self.actors[actor.index()], action.tp_cost),
            )
    }

    pub(crate) fn special_guard_learned(&self, actor: ActorId, action: crate::ActionKey) -> bool {
        self.prepared
            .technique(actor, action)
            .is_some_and(|technique| {
                self.learning_members
                    .iter()
                    .find(|row| row.actor == actor)
                    .is_some_and(|row| row.member.current().contains(&technique.catalogue))
            })
    }

    pub(crate) fn special_guard_family_available(&self, actor: ActorId) -> bool {
        self.prepared
            .special_guard(actor)
            .is_some_and(|binding| self.action_family_available(actor, binding))
    }

    pub(crate) fn special_guard_admission_allowed(
        &self,
        actor: ActorId,
        action: crate::ActionKey,
    ) -> bool {
        let Some(binding) = self.prepared.special_guard(actor) else {
            return false;
        };
        binding == action
            && self.special_guard_learned(actor, binding)
            && self.special_guard_family_available(actor)
            && self.actors[actor.index()].conditions.arte_queue_allowed()
            && !self.actors[actor.index()].airborne()
    }

    pub(crate) fn special_guard_threat_pending(&self, actor: ActorId) -> bool {
        self.runtime
            .get(actor.index())
            .is_some_and(|state| state.special_guard_pending.is_some())
    }

    pub(crate) fn publish_special_guard_threat(
        &mut self,
        caster: ActorId,
        target: ActorId,
        threat: crate::CastingThreat,
    ) {
        let Some(caster_actor) = self.actors.get(caster.index()) else {
            return;
        };
        let Some(target_actor) = self.actors.get(target.index()) else {
            return;
        };
        let Some(binding) = self.prepared.special_guard(target) else {
            return;
        };
        let Some(technique) = self.prepared.technique(target, binding) else {
            return;
        };
        let guard_catalogue = technique.catalogue;
        if caster_actor.side != crate::Side::Enemy
            || target_actor.side != crate::Side::Party
            || !threat.offensive
            || threat.element == 0
            || usize::from(threat.element) >= target_actor.equipment.affinities.len()
            || !matches!(
                target_actor.equipment.affinities[usize::from(threat.element)],
                crate::Affinity::Normal | crate::Affinity::Weak
            )
            || target_actor.overlimit.is_active()
            || target_actor.control != crate::Control::Auto
            || self.runtime[target.index()].special_guard_pending.is_some()
            || !self.special_guard_learned(target, binding)
            || !self.technique_enabled(target, binding)
        {
            return;
        }
        let can_guard = match self.activity(target) {
            crate::Activity::Idle | crate::Activity::Guarding => true,
            crate::Activity::Action => target_actor.guard.kind == crate::GuardKind::Special,
            crate::Activity::Casting { .. } => self
                .casting_remaining(target)
                .is_some_and(|remaining| remaining > 0),
            _ => false,
        };
        if can_guard {
            self.runtime[target.index()].special_guard_pending = Some(guard_catalogue);
            self.runtime[caster.index()].special_guard_pending = Some(guard_catalogue);
        }
    }

    pub(crate) fn consume_auto_special_guard(
        &mut self,
        actor: ActorId,
        cues: &mut Vec<Cue>,
    ) -> Result<bool> {
        let Some(catalogue) = self.runtime[actor.index()].special_guard_pending.take() else {
            return Ok(false);
        };
        let Some(binding) = self.prepared.special_guard(actor) else {
            return Ok(false);
        };
        let Some(technique) = self.prepared.technique(actor, binding).copied() else {
            return Ok(false);
        };
        if self.actors[actor.index()].control != crate::Control::Auto
            || !self.special_guard_learned(actor, binding)
            || technique.catalogue != catalogue
        {
            return Ok(false);
        }
        self.start_special_guard(actor, binding, cues)
    }

    pub(crate) fn start_special_guard(
        &mut self,
        actor: ActorId,
        action: crate::ActionKey,
        cues: &mut Vec<Cue>,
    ) -> Result<bool> {
        let index = actor.index();
        let request = ActionRequest {
            actor,
            action,
            target: actor,
        };
        if self.actors[index].hit_stop != 0 || self.action_rejection(request)?.is_some() {
            return Ok(false);
        }
        if crate::conditions::paralysis_controller_roll(&self.actors[index], &mut self.random) {
            self.clear_technique_command(actor);
            self.begin_paralysis(actor, cues)?;
            return Ok(true);
        }
        self.actors[index].guard.active = false;
        self.actors[index].guard.recovery = 0;
        self.actors[index].movement.forward = 0.;
        self.actors[index].attack_power = 100;
        self.runtime[index].combo = Default::default();
        self.start_admitted_action(request, cues)?;
        Ok(true)
    }

    pub(crate) fn special_guard_action(
        &self,
        actor: ActorId,
        action: crate::ActionKey,
    ) -> Option<crate::ActionKey> {
        self.prepared
            .special_guard(actor)
            .filter(|&binding| binding == action && self.special_guard_learned(actor, binding))
    }

    pub(crate) fn begin_special_guard(
        &mut self,
        actor: ActorId,
        action: crate::ActionKey,
    ) -> Result<()> {
        self.special_guard_action(actor, action)
            .ok_or_else(|| anyhow::anyhow!("actor has no prepared Special Guard action"))?;
        let guard = &mut self.actors[actor.index()].guard;
        guard.active = true;
        guard.kind = crate::GuardKind::Special;
        guard.pressure = 0;
        self.actors[actor.index()].reaction.direction =
            self.actors[actor.index()].movement.direction;
        Ok(())
    }

    pub(crate) fn clear_special_guard(&mut self, actor: ActorId) {
        {
            let guard = &mut self.actors[actor.index()].guard;
            if guard.kind == crate::GuardKind::Special {
                guard.active = false;
                guard.kind = crate::GuardKind::Normal;
                guard.pressure = 0;
            }
        }
    }

    pub(crate) fn clear_special_guard_pending(&mut self, actor: ActorId) {
        if let Some(state) = self.runtime.get_mut(actor.index()) {
            state.special_guard_pending = None;
        }
    }

    pub(crate) fn expire_projectile(&mut self, id: ProjectileId, cues: &mut Vec<Cue>) {
        self.projectiles.remove(&id);
        cues.push(Cue::ProjectileExpired { projectile: id });
    }

    pub(crate) fn emit(
        &mut self,
        definition: Arc<ProjectileDefinition>,
        action: ActionId,
        owner: ActorId,
        target: ActorId,
        position: [f32; 3],
    ) -> Result<ProjectileId> {
        self.emit_at(
            definition,
            action,
            owner,
            target,
            position,
            self.actor(owner)?.heading,
        )
    }

    pub(crate) fn emit_at(
        &mut self,
        definition: Arc<ProjectileDefinition>,
        action: ActionId,
        owner: ActorId,
        target: ActorId,
        position: [f32; 3],
        heading: f32,
    ) -> Result<ProjectileId> {
        self.emit_aimed(definition, action, owner, target, position, heading, None)
    }

    pub(crate) fn emit_aimed(
        &mut self,
        definition: Arc<ProjectileDefinition>,
        action: ActionId,
        owner: ActorId,
        target: ActorId,
        position: [f32; 3],
        heading: f32,
        aim: Option<[f32; 3]>,
    ) -> Result<ProjectileId> {
        let position = match definition.motion.origin {
            crate::ProjectileOrigin::TargetFloor => {
                let target = self.actor(target)?.position;
                [target[0], 0., target[2]]
            }
            _ => position,
        };
        let deferred = self.defer_projectile_anchors
            && (matches!(definition.motion.origin, crate::ProjectileOrigin::Bone(_))
                || definition.motion.bone_direction.is_some());
        let (position, direction) = if deferred {
            (position, None)
        } else {
            self.projectile_launch_anchors(&definition.motion, owner, position)?
        };
        ensure!(
            position.iter().all(|v| v.is_finite())
                && heading.is_finite()
                && aim.is_none_or(|point| point.iter().all(|value| value.is_finite())),
            "invalid projectile emission transform"
        );
        let id = ProjectileId(self.next_projectile);
        self.next_projectile = self
            .next_projectile
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("battle projectile handle exhausted"))?;
        let frame = ProjectileFrame {
            velocity: [0.; 3],
            id,
            owner,
            target,
            position,
            heading,
            age: 0,
            contact_active: false,
            disarmed: false,
            shadow: None,
        };
        let mut projectile = Projectile::new(definition, action, frame);
        projectile.attack_power = self.actor(owner)?.attack_power;
        projectile.target_point = match aim {
            Some(point) => point,
            None => *self
                .actor_start_centers
                .get(target.index())
                .context("missing projectile target")?,
        };
        if deferred {
            projectile.prepare_velocity(&mut self.random);
            projectile.pending_launch = true;
        } else if let Some(birth) =
            projectile.initialize(&mut self.pending_cues, &mut self.random, direction)
        {
            self.pending_cues.push(Cue::Effect(birth));
        }
        self.projectiles.insert(id, projectile);
        Ok(id)
    }

    fn projectile_launch_anchors(
        &self,
        motion: &crate::ProjectileMotion,
        owner: ActorId,
        position: [f32; 3],
    ) -> Result<([f32; 3], Option<[f32; 3]>)> {
        let anchor = |bone| -> Result<[f32; 3]> {
            let models = self
                .models
                .as_ref()
                .context("projectile launch has no models")?;
            let model = models
                .definition(owner)
                .context("projectile launch has no body")?;
            models.anchor(
                owner,
                crate::MeleeAnchor {
                    resource: model.resource,
                    bone: u16::from(bone),
                },
            )
        };
        let position = match motion.origin {
            crate::ProjectileOrigin::Bone(bone) => anchor(bone)?,
            _ => position,
        };
        let direction = motion
            .bone_direction
            .map(|direction| -> Result<[f32; 3]> {
                let from = anchor(direction.from)?;
                let to = anchor(direction.to)?;
                let delta = std::array::from_fn(|axis| to[axis] - from[axis]);
                let length = crate::distance::length(delta);
                ensure!(
                    length.is_finite() && length > 0.,
                    "invalid projectile launch anchors"
                );
                Ok(delta.map(|value| value / length))
            })
            .transpose()?;
        Ok((position, direction))
    }

    pub(crate) fn recover(&mut self, id: ActorId, percent: i16, cues: &mut Vec<Cue>) -> Result<()> {
        let actor = self.actor(id)?;
        if actor.hp <= 0 && !actor.available() {
            return Ok(());
        }
        self.recover_vitals(id, percent, true, cues)
    }

    pub(crate) fn recover_vitals(
        &mut self,
        id: ActorId,
        percent: i16,
        lucky: bool,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        let actor = self.actor(id)?;
        let (lucky, luck) = (
            lucky && actor.equipment.recovery.lucky,
            actor.equipment.luck,
        );
        let mut value = i32::from(percent.max(0));
        if lucky && self.random.next_u16() % 100 < luck / 20 + 5 {
            value += value / 2;
        }
        let actor = &mut self.actors[id.index()];
        let before = actor.hp;
        let (hp, nominal) = actor.recovered_hp(value);
        actor.hp = hp;
        cues.push(Cue::Recovered {
            kind: crate::RecoveryKind::Hp,
            actor: id,
            nominal,
            applied: actor.hp - before,
        });
        Ok(())
    }
}

#[cfg(test)]
mod melee_tests;

#[cfg(test)]
mod spell_tests;
