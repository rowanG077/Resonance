use crate::EnemyChoiceId;
use crate::WeaponSlot;
use crate::conditions::Condition;
use crate::{
    Activity, PreparedBattle, ProjectileDefinition, ProjectileFrame, ProjectileId,
    action::{Execution, Sequence, step_sequence},
    contact::Contacts,
    projectile::{Projectile, Rebound},
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

/// Membership for the full actor-handle range, independent of formation size.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ActorSet([u64; (u8::MAX as usize + 1) / u64::BITS as usize]);

impl ActorSet {
    pub fn contains(&self, actor: ActorId) -> bool {
        self.0[actor.index() / 64] & (1 << (actor.index() % 64)) != 0
    }

    pub fn insert(&mut self, actor: ActorId) {
        self.0[actor.index() / 64] |= 1 << (actor.index() % 64);
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

pub use resonance_content::{battle_action::SpellSlot, battle_profile::Species};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContactSource {
    Melee {
        actor: ActorId,
        action: ActionId,
    },
    Weapon {
        actor: ActorId,
        slot: WeaponSlot,
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
    pub life_drain: bool,
    pub spirit_drain: bool,
    pub common: crate::CommonRecoveryTraits,
    pub lethal: crate::LethalRescueTraits,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Actor {
    pub side: Side,
    pub species: Species,
    pub equipment: EquipmentAttributes,
    pub combat_adjustments: crate::CombatAdjustments,
    pub control: crate::Control,
    /// Prepared local controller slot, independent of character identity.
    pub control_slot: crate::ControllerPort,
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
    /// A fully healed actor at rest; entry preparation supplies placement and saved state.
    pub fn new(side: Side, control: crate::Control, equipment: EquipmentAttributes) -> Self {
        Self {
            side,
            control,
            hp: equipment.max_hp,
            tp: equipment.max_tp,
            equipment,
            combat_adjustments: Default::default(),
            availability: Default::default(),
            guard: Default::default(),
            control_ex_state: Default::default(),
            casting_state: Default::default(),
            overlimit: Default::default(),
            elements: Default::default(),
            input: Default::default(),
            conditions: Default::default(),
            secondary_wind: Default::default(),
            body: Default::default(),
            movement: Default::default(),
            reaction: Default::default(),
            species: Species::default(),
            control_slot: crate::ControllerPort::One,
            stored_spell: None,
            attack_power: 100,
            proficiency: 0,
            position: [0.; 3],
            heading: 0.,
            facing_direction: [0., 0., 1.],
            effect_scale: 1.,
            hit_stop: 0,
            time_stop: 0,
        }
    }

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

    pub fn airborne(&self) -> bool {
        self.position[1] > crate::movement::GROUND_TOLERANCE
    }

    pub(crate) fn needs_landing(&self) -> bool {
        self.airborne() && !self.movement.flying && !self.movement.fixed_height
    }

    pub(crate) fn recovered_tp(&self, percent: i32) -> (u16, i32) {
        let nominal = (i64::from(self.equipment.max_tp) * i64::from(percent.max(0)) / 100)
            .clamp(1, i64::from(i32::MAX)) as i32;
        let tp =
            (i64::from(self.tp) + i64::from(nominal)).min(i64::from(self.equipment.max_tp)) as u16;
        (tp, nominal)
    }

    /// Apply percentage recovery and report both the nominal and capped amounts.
    pub(crate) fn recover_percent(
        &mut self,
        actor: ActorId,
        kind: RecoveryKind,
        percent: i32,
        floating: bool,
    ) -> Cue {
        let (nominal, applied) = match kind {
            RecoveryKind::Hp => {
                let (next, nominal) = self.recovered_hp(percent);
                let applied = next - self.hp;
                self.hp = next;
                (nominal, applied)
            }
            RecoveryKind::Tp => {
                let (next, nominal) = self.recovered_tp(percent);
                let applied = i32::from(next) - i32::from(self.tp);
                self.tp = next;
                (nominal, applied)
            }
        };
        Cue::Recovered {
            floating,
            actor,
            kind,
            nominal,
            applied,
        }
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
}

impl From<ActionRequest> for BattleInput {
    fn from(action: ActionRequest) -> Self {
        Self {
            actions: vec![action],
            ..Default::default()
        }
    }
}

impl From<crate::ControlInput> for BattleInput {
    fn from(controller: crate::ControlInput) -> Self {
        Self {
            controllers: vec![controller],
            ..Default::default()
        }
    }
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
    TechnicalSmash {
        bonuses: crate::TechnicalSmash,
        total_percent: u8,
    },
    /// Retire existing effect emitters and particles before a cinematic's first effect.
    ClearEffects,
    /// Retire one actor's spell scene without disturbing other effect resources.
    ClearEffectBank {
        actor: ActorId,
        resource: crate::ResourceId,
    },
    Effect(crate::EffectRequest),
    /// Initialize at creation, before the ordinary emitter update.
    InitializedEffect(crate::EffectRequest),
    /// Resolve a detached emission after the current model pose is ready.
    InitializedBoneEffect {
        request: crate::EffectRequest,
        anchor: crate::MeleeAnchor,
    },
    InitializedActionEffect {
        action: ActionId,
        request: crate::EffectRequest,
    },
    /// Hold ordinary scene effects without stopping the combat schedule.
    HoldEffects(u16),
    /// Start after particle updates; initial particles become visible on the next update.
    DeferredEffect(crate::EffectRequest),
    /// Run every emitter visit after particles, publishing each birth next update.
    AfterParticlesEffect(crate::EffectRequest),
    /// Result growth feedback shares one fanfare across the awarded party members.
    ResultAdvancement {
        actors: Vec<ActorId>,
        level_up: bool,
    },
    TechniqueAcquired {
        actor: ActorId,
        action: crate::ActionKey,
        spell: bool,
    },
    /// A cinematic emitter runs while ordinary effects are held; particles keep their own clock.
    EffectDuringPause(crate::EffectRequest),
    /// Scene-owned scenery tint; its timer is independent of the released spell.
    StageTint(crate::StageTint),
    /// Apply an entrance tint before this update’s scenery color step.
    InitializedStageTint {
        tint: crate::StageTint,
        step: u8,
    },
    Cutin {
        action: ActionId,
        catalogue: crate::TechniqueId,
        duration: u16,
    },
    /// Start on the next scene update, independently of the actor's hit stop.
    ActionEffect {
        action: ActionId,
        request: crate::EffectRequest,
    },
    /// Hold this update's action emissions while its commands wait.
    PauseActionEffects(ActionId),
    /// Finish this emission visit, then stop future births; existing particles remain independent.
    RetireActionEffects(ActionId),
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
    Backstepped {
        actor: ActorId,
    },
    Taunted {
        actor: ActorId,
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
        slot: WeaponSlot,
        duration: u16,
        /// Technique feedback uses the shared bright blade palette.
        highlighted: bool,
    },
    /// Presentation owns the color and lifetime; this cannot change actor opacity.
    ActorFlash {
        actor: ActorId,
        color: [u8; 3],
    },
    /// Color first appears on the next body publication.
    DeferredFlash {
        actor: ActorId,
        color: [u8; 3],
        /// Actor callbacks fade once before publication; late recovery callbacks do not.
        fade_first: bool,
    },
    /// Brief color override lasting two active actor updates.
    ActorPulse {
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
    OverLimitExpired {
        actor: ActorId,
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
        /// Direction at emission, before the first movement updates steering.
        velocity: [f32; 3],
    },
    /// A model follows the projectile's ricochet and later floor bounces.
    ProjectileRebound {
        projectile: ProjectileId,
        first: bool,
        /// Ground ricochets begin at a new pitch. Contacts preserve the current pitch.
        pitch: Option<f32>,
    },
    /// Change the departure clip now; follow its direction after this particle update.
    ProjectileDeparture {
        projectile: ProjectileId,
        animation: resonance_content::battle_effect::ModelAnimation,
    },
    /// Attached effects retain their flight orientation when motion stops at the floor.
    ProjectileStopped {
        projectile: ProjectileId,
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
        /// Another contact reached this actor within the impact effect interval.
        repeated: bool,
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
        /// Spell and item recovery may also show a number over the recipient.
        floating: bool,
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
    TargetChanged {
        actor: ActorId,
        target: ActorId,
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
    /// Center sampled at the actor visit, before movement updates its root.
    pub effect_center: [f32; 3],
    pub activity: crate::Activity,
    /// The paid spell release pose or its recovery, distinct from chanting.
    pub cast_releasing: bool,
    /// Remaining controller updates, used by the attached stun birds.
    pub stun_remaining: u32,
    /// A fresh horizontal input is accelerating this stun update.
    pub stun_struggling: bool,
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
    /// The entry camera settles while actor animations continue.
    EntrySettle,
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
    pub combat_active: bool,
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
    pub scanned_enemies: ActorSet,
    pub update: u64,
    pub targets: Vec<Option<ActorId>>,
    pub target_selector: Option<ActorId>,
    pub actors: Vec<ActorFrame>,
    /// Scene-owned drawing data, filled after native publication.
    pub models: Vec<crate::ModelFrame>,
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
    EnteringGuard,
    Approach(crate::approach::Approach),
    Returning(crate::steering::ReturnState),
    Escaping,
    Hurt {
        remaining: u32,
    },
    Recovering {
        remaining: u32,
        after_hit: bool,
        idle_pose: Option<(crate::MotionBinding, crate::Pose)>,
    },
    /// A carried participant recovers from its host's release before choosing work.
    Released,
    Paralysis {
        remaining: u8,
    },
    Stunned {
        remaining: u32,
        struggle_ticks: u8,
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
            Self::EnteringGuard => Activity::Guarding,
            Self::Escaping => Activity::Escaping,
            Self::Hurt { .. } | Self::Paralysis { .. } => Activity::Hurt,
            Self::Recovering { .. } | Self::Released => Activity::Recovering,
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

pub(crate) const TARGET_SETTLE_UPDATES: u16 = 120;

/// State owned by one actor throughout the battle. Public actor values and model
/// poses stay separate so gameplay and animation can borrow them independently.
pub(crate) struct ActorRuntime {
    pub(crate) combo: crate::action_selection::Combo,
    pub(crate) control: Option<crate::control::Controller>,
    pub(crate) target: ActorId,
    pub(crate) target_settle_remaining: u16,
    pub(crate) enemy_choice: Option<EnemyChoiceId>,
    pub(crate) last_executed_enemy_row: Option<EnemyChoiceId>,
    pub(crate) enemy_effective_hit: bool,
    pub(crate) body_bypass: bool,
    pub(crate) body_bypass_ticks: u8,
    pub(crate) used_enemy_choices: BTreeSet<EnemyChoiceId>,
    pub(crate) enemy_walk_override: Option<f32>,
    pub(crate) enemy_behavior: crate::enemy_policy::State,
    pub(crate) capture: crate::capture::State,
    task: ActorTask,
    pub(crate) idle_timer: u32,
    pub(crate) companion_policy: Option<crate::CompanionPolicy>,
    pub(crate) recovery: crate::CommonRecoveryState,
    pub(crate) angel_tear_armed: bool,
    pub(crate) support_target: ActorId,
    pub(crate) special_guard_pending: Option<crate::TechniqueId>,
}

impl ActorRuntime {
    pub(crate) fn set_target(&mut self, target: ActorId) {
        if self.target != target {
            self.target = target;
            self.target_settle_remaining = TARGET_SETTLE_UPDATES;
        }
    }

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
    #[cfg(feature = "test-support")]
    pub(crate) contact_observations: Option<Vec<crate::contact_observation::ContactObservation>>,
    #[cfg(feature = "test-support")]
    pub(crate) automatic_decisions_suppressed: bool,
    #[cfg(feature = "test-support")]
    pub(crate) fixture_enemy_chains:
        Option<std::collections::VecDeque<crate::FixtureEnemyChainInput>>,
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) random_inputs: Option<crate::random_inputs::Inputs>,
    pub(crate) unison_gauge: i16,
    pub(crate) unison: crate::unison::State,
    pub(crate) summon_entry: crate::summon_entry::State,
    pub(crate) enemy_presentation: crate::enemy_presentation::State,
    pub(crate) defense_system: crate::defense_system::State,
    pub(crate) escape: crate::escape::State,
    pub(crate) items: crate::item::State,
    pub(crate) diagnostics: resonance_content::diagnostics::Diagnostics,
    pub(crate) diagnostic: bool,
    pub(crate) instant_kill: bool,
    pub(crate) cast_inputs: [crate::casting::CastInput; 4],
    pub(crate) ledger: crate::Ledger,
    pub(crate) weapon_flights: BTreeMap<(ActorId, WeaponSlot), crate::weapon_flight::Flight>,
    pub(crate) target_selector: Option<ActorId>,
    pub(crate) command_target: Option<(ActorId, ActorId)>,
    pub(crate) prepared: crate::prepare::BattleResources,
    /// Mutable battle-local learning membership. Persistent Party projection
    /// remains game-owned at the world-update boundary.
    pub(crate) learning_members: Vec<crate::learning::TechniqueLearningMember>,
    pub(crate) actors: Vec<Actor>,
    /// Target centers sampled before each actor's movement visit.
    pub(crate) actor_start_centers: Vec<[f32; 3]>,
    /// Prior published centers held fixed throughout one actor update.
    pub(crate) actor_emission_centers: Vec<[f32; 3]>,
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
    /// Base combat values with no equipment bonuses or elemental affinities.
    pub fn new(max_hp: i32, max_tp: u16, stats: crate::CombatStats) -> Self {
        Self {
            max_hp,
            max_tp,
            stats,
            tp_cost_reduction: false,
            quick_escape: false,
            taunt_enabled: false,
            taunt_guard: false,
            taunt_cancel: false,
            quick_turn: false,
            backstep_guard: false,
            dagger_reach: false,
            guaranteed_steal: false,
            normal_guard: false,
            stun_ex_bonus: false,
            spell_revenge: false,
            control_ex: Default::default(),
            casting: Default::default(),
            contact: Default::default(),
            damage: Default::default(),
            recovery: Default::default(),
            combo_traits: Default::default(),
            reaction_ex: Default::default(),
            normal_combo_limit: 1,
            luck: 0,
            base_element: None,
            affinities: [crate::Affinity::Normal; 9],
            speed_multiplier: 1.,
        }
    }

    fn validate(&self) -> Result<()> {
        ensure!(self.max_hp > 0, "invalid battle HP");
        ensure!(self.normal_combo_limit > 0, "invalid normal combo limit");
        ensure!(
            self.damage
                .weapon_species
                .is_none_or(|species| (1..=12).contains(&species.0)),
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
        let mut actors = prepared.actors;
        let actor_count = actors.len();
        let learning_members = prepared.technique_learning_members;
        let enemy_presentation = crate::enemy_presentation::State::new(
            &mut actors,
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
                        target_settle_remaining: TARGET_SETTLE_UPDATES,
                        enemy_choice: None,
                        last_executed_enemy_row: None,
                        enemy_effective_hit: false,
                        body_bypass: false,
                        body_bypass_ticks: 0,
                        used_enemy_choices: BTreeSet::new(),
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
            instant_kill: false,
            ledger: {
                let mut ledger = crate::Ledger::new(actor_count, prepared.resources.grade_rank);
                for (facts, actor) in ledger.achievements.iter_mut().zip(&actors) {
                    facts.stayed_poisoned = actor
                        .conditions
                        .effective()
                        .intersects(crate::conditions::POISON);
                }
                ledger
            },
            weapon_flights: BTreeMap::new(),
            cast_inputs: Default::default(),
            target_selector: None,
            command_target: None,
            pending_cues: Vec::new(),
            actor_start_centers: actors.iter().map(Actor::effect_origin).collect(),
            actor_emission_centers: actors.iter().map(Actor::effect_origin).collect(),
            defense_system: crate::defense_system::State::new(
                prepared
                    .resources
                    .defense_system
                    .as_ref()
                    .map_or(0, |definition| {
                        actors[definition.core.index()].equipment.stats.defense
                    }),
            ),
            actors,
            random: Random::new(prepared.random_seed),
            model_requests: Vec::new(),
            models: None,
            camera: prepared.camera,
            entry_remaining,
            #[cfg(feature = "test-support")]
            automatic_decisions_suppressed: false,
            #[cfg(feature = "test-support")]
            contact_observations: None,
            #[cfg(feature = "test-support")]
            fixture_enemy_chains: None,
            #[cfg(any(test, feature = "test-support"))]
            random_inputs: None,
            learning_members,
            volleys: BTreeMap::new(),
            projectiles: BTreeMap::new(),
            defer_projectile_anchors: false,
            update: 0,
            unison_gauge: prepared.resources.unison_gauge,
            unison: Default::default(),
            summon_entry: Default::default(),
            enemy_presentation,
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

    /// Testing toggle: party damage defeats enemies through the normal contact flow.
    pub fn set_instant_kill(&mut self, enabled: bool) {
        self.instant_kill = enabled;
    }

    pub fn activity(&self, actor: ActorId) -> crate::Activity {
        self.runtime[actor.index()]
            .task
            .activity(&self.actors[actor.index()])
    }

    pub fn actor_ids(&self) -> impl ExactSizeIterator<Item = ActorId> + '_ {
        (0..self.actors.len()).map(|index| ActorId(index as u8))
    }

    /// Prefer a nonautomatic party member, then the first member, regardless of availability.
    pub(crate) fn preferred_party_leader(&self) -> Option<ActorId> {
        self.actors
            .iter()
            .position(|actor| actor.side == Side::Party && actor.control != crate::Control::Auto)
            .or_else(|| {
                self.actors
                    .iter()
                    .position(|actor| actor.side == Side::Party)
            })
            .map(|index| ActorId(index as u8))
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
        runtime.body_bypass = false;
        runtime.body_bypass_ticks = 0;
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

    /// A timed battle scene disposes released combat work before taking the view.
    /// Actor commands stay held until the scene returns them to their formation.
    pub fn clear_event_effects(&mut self) -> Result<Vec<Cue>> {
        ensure!(
            !self.ended
                && self.phase() == crate::BattlePhase::Combat
                && self.clock == BattleClock::Held,
            "battle event effect cleanup requires held combat"
        );
        let mut cues = Vec::new();
        for (action, _) in std::mem::take(&mut self.volleys) {
            cues.push(Cue::Interrupted { action });
        }
        for projectile in self.projectiles.keys().copied().collect::<Vec<_>>() {
            self.expire_projectile(projectile, &mut cues);
        }
        self.retire_weapon_flights();
        cues.push(Cue::ClearEffects);
        Ok(cues)
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
                    .map(|cue| cue.into_iter().collect())
            } else if self.command_target.is_some() {
                self.render_command_target().map(|()| Vec::new())
            } else {
                self.clock = self.timed_hold.map_or(BattleClock::Running, |hold| {
                    if let Some(actor) = hold.actor {
                        BattleClock::Rescue(actor)
                    } else {
                        self.learning_actor()
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
        let opening = self.enemy_presentation.opening();
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
            self.clock = BattleClock::EntrySettle;
        }
        if !self.clock.paused() && !self.unison.active() {
            self.retarget_selected_opponent()?;
        }
        self.recognize_update();
        self.ledger
            .advance(self.phase() == crate::BattlePhase::Combat);
        if !opening
            && !self.unison.active()
            && self.phase() != crate::BattlePhase::Results
            && !self.terminal.ordinary_camera_finished
            && let Some(camera) = &mut self.camera
        {
            if let Some(tick) = entry_tick.filter(|tick| *tick <= crate::prepare::ENTRY_PAN_UPDATES)
            {
                camera.frame_entry(tick);
            } else {
                camera.step(
                    &self.actors,
                    self.runtime[camera.definition.leader.index()].target,
                    self.runtime[camera.definition.leader.index()].target_settle_remaining != 0,
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
        // Combat waits for framing while the scene keeps animating.
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
        if self.phase() == crate::BattlePhase::Results
            && let Some(camera) = &mut self.camera
        {
            camera.frame_results();
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
        self.advance_defense_system(cues);
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
        if let Some(models) = &mut self.models {
            models.prepare_approach_geometry(&self.actors)?;
        }
        self.defer_projectile_anchors = true;
        let actor_result = self.advance_actors(
            &input.controllers,
            &mut body_steps,
            &mut contacts,
            &mut transitions,
            cues,
            items,
        );
        self.defer_projectile_anchors = false;
        if let Some(models) = &mut self.models {
            models.finish_approach_queries();
        }
        let knockdown_visits = actor_result?;
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
        let knockdown_pose_finished = self.actors.iter().enumerate().fold(
            ActorSet::default(),
            |mut finished, (index, actor)| {
                if knockdown_visits.contains(ActorId(index as u8))
                    && self.models.as_ref().is_none_or(|models| {
                        models
                            .motion_finished_this_update(ActorId(index as u8), actor.hit_stop == 0)
                    })
                {
                    finished.insert(ActorId(index as u8));
                }
                finished
            },
        );
        self.prepare_contact_models(resolve_contacts, crate::model::UpdatePhase::PoseOnly)?;
        for cue in cues.iter_mut() {
            if let Cue::InitializedBoneEffect { request, anchor } = cue {
                request.origin = self
                    .models
                    .as_ref()
                    .context("carried effect has no model")?
                    .anchor_at(
                        request.owner,
                        *anchor,
                        body_placements
                            .get(request.owner.index())
                            .copied()
                            .flatten(),
                    )?;
                *cue = Cue::InitializedEffect(request.clone());
            }
        }
        // Carried creatures consume the current pose before the host's placement
        // is published, just like this update's contact geometry.
        let hosted_positions = self.hosted_positions(&body_placements)?;
        // Knockdown recovery consumes completed pose readiness before movement.
        for (index, body_step) in body_steps.iter_mut().enumerate() {
            if knockdown_visits.contains(ActorId(index as u8)) {
                let actor = ActorId(index as u8);
                if let ActorTask::Knockdown(recovery) = self.runtime[index].task
                    && self.actors[index].available()
                    && self.actors[index].time_stop == 0
                    && !self.is_captured(actor)
                {
                    self.advance_knockdown(
                        actor,
                        recovery,
                        knockdown_pose_finished.contains(actor),
                        cues,
                    )?;
                }
                *body_step = self.finish_actor_update(index, body_positions[index], false, cues)?;
            }
        }
        // The introduction preserves the prepared formation.
        if resolve_contacts && !self.unison.staged() {
            self.push_bodies(&body_positions, &body_steps, &body_placements);
        }
        self.attach_defense_arms(defense_arm_positions);
        self.follow_hosted_participants(hosted_positions);
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
        // Captured actors follow the current pose before the captor's placement is published.
        if resolve_contacts && !self.unison.staged() {
            for index in 0..self.actors.len() {
                if self.actors[index].time_stop == 0 {
                    self.follow_capture(ActorId(index as u8), &body_placements)?;
                }
            }
        }
        // Refresh placement and late requests; this update's pose clock already advanced.
        self.prepare_contact_models(resolve_contacts, crate::model::UpdatePhase::FinishPose)?;
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
        self.advance_action_weapons(cues)?;
        if resolve_contacts {
            self.advance_volleys(cues, false)?;
        }
        self.release_hosted_participants(cues)?;
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
                    self.pending_cues.push(Cue::InitializedEffect(birth));
                }
            }
            if !advance {
                return Ok(());
            }
            self.advance_projectile_seal(id, cues)?;
            let projectile = self.projectiles.get_mut(&id).unwrap();
            // A deferred rebound keeps the target origin from the ricochet visit.
            let rebound_origin = projectile.target_point;
            let rebound = projectile.take_rebound_projectile();
            if let Some(target) = projectile.pending_rebound_target() {
                projectile.target_point = self.actors[target.index()].position;
            } else if projectile.definition.motion.aqua_edge {
                projectile.target_point = self.actors[projectile.frame.target.index()].position;
            } else if projectile.definition.motion.departure.is_some() {
                projectile.target_point = self.actors[projectile.frame.owner.index()].position;
            }
            let departed = projectile.departed;
            let moving = projectile.velocity != [0.; 3];
            projectile.step()?;
            if moving
                && projectile.definition.motion.stop_on_ground
                && projectile.velocity == [0.; 3]
                && projectile.frame.position[1] == 0.
            {
                cues.push(Cue::ProjectileStopped { projectile: id });
            }
            if !departed
                && projectile.departed
                && let Some(animation) = projectile.definition.effects.departure_animation
            {
                cues.push(Cue::ProjectileDeparture {
                    projectile: id,
                    animation,
                });
            }
            if let Some(rebound) = projectile.take_rebound() {
                let pitch = match rebound {
                    Rebound::Ground => Some(f32::from(self.random.next_u16() % 360)),
                    Rebound::Contact => {
                        self.random.next_u16();
                        None
                    }
                    Rebound::Bounce => None,
                };
                cues.push(Cue::ProjectileRebound {
                    projectile: id,
                    first: rebound != Rebound::Bounce,
                    pitch,
                });
            }
            contacts.submit(projectile)?;
            for effect in projectile.effects() {
                cues.push(Cue::InitializedEffect(effect));
            }
            if let Some((target, definition)) = rebound {
                let (action, owner, heading) = (
                    projectile.action,
                    projectile.frame.owner,
                    projectile.frame.heading,
                );
                let child =
                    self.emit_at(definition, action, owner, target, rebound_origin, heading)?;
                self.visit_projectile(child, true, contacts, cues)?;
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
        if self.models.is_none() {
            return Ok(());
        }
        let mut frame = self.frame(Vec::new(), None);
        // All geometry visits share one pose clock for this update.
        frame.update += 1;
        frame.model_requests = std::mem::take(&mut self.model_requests);
        if let Some(models) = &mut self.models {
            models.advance_for_contacts(
                &mut frame,
                self.clock,
                settle,
                phase,
                &self.prepared.hosted_participants,
            )?;
        }
        self.model_requests.append(&mut frame.model_requests);
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
        input: &[crate::ControlInput],
        body_steps: &mut [f32],
        contacts: &mut Contacts,
        transitions: &mut BTreeMap<ActionId, ActionTransition>,
        cues: &mut Vec<Cue>,
        items: &mut dyn crate::item::Provider,
    ) -> Result<ActorSet> {
        let mut knockdown_visits = ActorSet::default();
        // Aim and target-origin emissions use the previous center publication.
        // Keep it immutable while movement visits publish the next centers.
        self.actor_emission_centers
            .clone_from(&self.actor_start_centers);
        for volley in self.volleys.values_mut() {
            volley.projectile_target = self.actor_emission_centers[volley.target.index()];
        }
        let dead_at_start =
            self.actors
                .iter()
                .enumerate()
                .fold(ActorSet::default(), |mut dead, (index, actor)| {
                    if actor.availability == crate::ActorAvailability::Dead {
                        dead.insert(ActorId(index as u8));
                    }
                    dead
                });
        self.paused_actors.clear();
        for (index, body_step) in body_steps.iter_mut().enumerate() {
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
            if let Some(target) = self.target(actor) {
                self.actors[index].movement.target_direction = crate::distance::planar_direction(
                    self.actor_start_centers[target.index()],
                    self.actor_start_centers[index],
                    self.actors[index].movement.target_direction,
                );
            }
            self.actor_start_centers[index] = self.actors[index].effect_origin();
            if self.unison.target() == Some(actor)
                || (self.actors[index].available() && self.unison.holds_opening_combo(actor))
            {
                let owner = &mut self.actors[index];
                if owner.hit_stop != 0 {
                    owner.hit_stop -= 1;
                } else {
                    owner
                        .movement
                        .integrate_along(&mut owner.position, owner.reaction.direction);
                    owner.movement.brake(owner.position[1], Activity::Hurt);
                }
                *body_step = self.actors[index].movement.forward_step;
                self.constrain_actor(index, Some(before));
                continue;
            }
            if self.unison.staged() && self.unison.holds_actor(actor) {
                continue;
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
            let mut action_advanced = false;
            if !self.advance_escape_actor(actor, cues) {
                match self.actors[index].availability {
                    crate::ActorAvailability::Absent => unreachable!(),
                    crate::ActorAvailability::Dead => self.advance_dead(index),
                    crate::ActorAvailability::Petrified => self.advance_petrified(index),
                    // A new revival starts its recovery task on the next actor visit.
                    crate::ActorAvailability::Active if dead_at_start.contains(actor) => {}
                    crate::ActorAvailability::Active => {
                        let was_knocked_down =
                            matches!(self.runtime[index].task, ActorTask::Knockdown(_));
                        if let Some((id, transition)) =
                            self.advance_active_actor(actor, input, contacts, cues, items)?
                        {
                            action_advanced = matches!(transition, ActionTransition::Advance);
                            transitions.insert(id, transition);
                        }
                        if was_knocked_down
                            && matches!(self.runtime[index].task, ActorTask::Knockdown(_))
                        {
                            knockdown_visits.insert(actor);
                            continue;
                        }
                    }
                }
            }
            if self.summon_entry.active() {
                break;
            }
            *body_step = self.finish_actor_update(index, before, action_advanced, cues)?;
        }
        Ok(knockdown_visits)
    }

    fn finish_actor_update(
        &mut self,
        index: usize,
        before: [f32; 3],
        action_advanced: bool,
        cues: &mut Vec<Cue>,
    ) -> Result<f32> {
        // Position commands and arena correction do not add movement to separation.
        let step = self.actors[index].movement.forward_step;
        self.constrain_actor(index, Some(before));
        self.publish_landing(index, cues);
        let owner = &mut self.actors[index];
        ensure!(
            owner.position.iter().all(|v| v.is_finite()),
            "battle movement overflow"
        );
        owner.movement.validate()?;
        self.advance_actor_common_after_action(index, action_advanced, cues)?;
        Ok(step)
    }

    /// Commands and tasks run only for available, unfrozen actors.
    /// Their transition commits after this update's contacts have resolved.
    fn advance_active_actor(
        &mut self,
        actor: ActorId,
        input: &[crate::ControlInput],
        contacts: &mut Contacts,
        cues: &mut Vec<Cue>,
        items: &mut dyn crate::item::Provider,
    ) -> Result<Option<(ActionId, ActionTransition)>> {
        let index = actor.index();
        let already_stopping = self.actors[index].movement.locomotion == crate::Locomotion::Stop;
        if self.phase() == crate::BattlePhase::Combat && self.actors[index].hit_stop == 0 {
            self.advance_enemy_transformation(index)?;
        }
        let mut next = None;
        let controls = input
            .iter()
            .find(|input| input.actor == actor)
            .copied()
            .unwrap_or_else(|| crate::ControlInput::neutral(actor));
        let unison = self.runtime[index]
            .task
            .action()
            .is_some_and(|(_, sequence)| sequence.unison)
            || self.unison.owns_commands(actor);
        if !unison {
            if matches!(self.runtime[index].task, ActorTask::EnteringGuard)
                && self.actors[index].hit_stop == 0
            {
                self.set_task(index, ActorTask::None);
                self.actors[index].guard.active = true;
            }
            // Physical selection becomes an action on the next available visit.
            if matches!(
                self.actors[index].control,
                crate::Control::Manual | crate::Control::SemiAuto
            ) && self.actors[index].hit_stop == 0
            {
                self.consume_pending_special_guard(actor, cues)?;
            }
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
                self.consume_pending_special_guard(actor, cues)?;
            }
            self.advance_ai(actor, cues)?;
        }
        if self.advance_enemy_departure(actor, cues)? {
            return Ok(None);
        }
        let task_movement = match self.runtime[index].task {
            ActorTask::Approach(_) => self.update_approach(actor, cues)?,
            ActorTask::Returning(state) => self.update_recovery_return(actor, state, cues),
            ActorTask::Hurt { remaining } => self.advance_hurt(actor, remaining, input, cues)?,
            ActorTask::Recovering {
                remaining,
                after_hit,
                idle_pose,
            } => {
                if remaining == 0 {
                    if let Some((motion, pose)) = idle_pose {
                        self.request_pose(actor, Some(motion), pose);
                        self.set_task(
                            index,
                            ActorTask::Recovering {
                                remaining: 0,
                                after_hit,
                                idle_pose: None,
                            },
                        );
                    } else {
                        self.enter_idle(actor);
                    }
                } else {
                    self.set_task(
                        index,
                        ActorTask::Recovering {
                            remaining: remaining - 1,
                            after_hit,
                            idle_pose,
                        },
                    );
                }
                false
            }
            ActorTask::Released => {
                if !self.actors[index].needs_landing() {
                    let owner = &mut self.actors[index];
                    owner.movement.direction = owner.facing_direction;
                    owner.movement.forward = 0.;
                    owner.movement.vertical = 0.;
                    owner.movement.acceleration = 0.;
                    self.set_task(
                        index,
                        ActorTask::Recovering {
                            remaining: 0,
                            after_hit: false,
                            idle_pose: None,
                        },
                    );
                    self.model_requests.push(crate::ModelRequest::Common {
                        actor,
                        pose: crate::CommonPose::Idle,
                    });
                }
                false
            }
            ActorTask::Paralysis { remaining } => self.advance_paralysis(actor, remaining),
            ActorTask::Stunned {
                remaining,
                struggle_ticks,
            } => self.advance_stun(
                actor,
                remaining,
                struggle_ticks,
                controls.horizontal_pressed != 0,
                cues,
            ),
            ActorTask::Knockdown(_) => true,
            ActorTask::Mobility(_) => self.update_mobility(actor, cues)?,
            ActorTask::Taunt { remaining } => self.advance_taunt(actor, remaining, controls, cues),
            ActorTask::Item(item) => {
                self.advance_item(actor, item, items, cues)?;
                false
            }
            ActorTask::Action { .. }
            | ActorTask::None
            | ActorTask::EnteringGuard
            | ActorTask::Escaping => false,
        };
        let entering_attack = self.runtime[index]
            .task
            .action()
            .is_some_and(|(_, sequence)| {
                sequence.started_at == self.update
                    && !self.enemy_physical_arte(actor, sequence.action)
                    && !(self.actors[index].side == Side::Party && self.actors[index].airborne())
                    && matches!(
                        sequence.definition.execution,
                        crate::ActionExecution::Attack(_)
                    )
            });
        // Ordinary admission binds before movement; airborne party input keeps its flight visit.
        if entering_attack {
            self.actors[index].movement.forward_step = 0.;
        }
        let mut movement_handled = task_movement
            || self.update_control_stop(index, already_stopping)
            || self.update_guard(index, cues)
            || entering_attack;
        let facing_ready = self.face_active_action(actor);
        if facing_ready
            && let Some((&id, _)) = self.runtime[index].task.action()
            && let Some(transition) = self.advance_sequence(id, contacts, items, cues)?
        {
            // Recovery changes acceleration now; contact ownership commits later.
            if matches!(transition, ActionTransition::Recover { .. }) {
                let movement = &mut self.actors[index].movement;
                movement.gravity = if movement.flying {
                    0.
                } else {
                    crate::movement::GRAVITY
                };
            }
            // Completed recovery owns no movement or body-separation step.
            if matches!(transition, ActionTransition::FinishRecovery) {
                self.actors[index].movement.forward_step = 0.;
                movement_handled = true;
            }
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
                let direction =
                    if actor.guard.active && actor.guard.kind == crate::GuardKind::Special {
                        actor.reaction.direction
                    } else {
                        actor.movement.direction
                    };
                actor
                    .movement
                    .integrate_along(&mut actor.position, direction);
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
                self.advance_hover(index, true);
            }
        }
        if !movement_handled
            && facing_ready
            && let Some((id, recovery)) = self.brake_control(index)
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
        if sequence.learning {
            if self.timed_hold.is_some() {
                return Ok(SequenceStep::Continue);
            }
            sequence.learning = false;
        }
        if let Execution::Recovering { remaining } = &mut sequence.execution {
            if matches!(
                &sequence.definition.execution,
                crate::ActionExecution::Casting(_)
            ) {
                self.advance_cast_recovery_pose(sequence.actor, *remaining);
            }
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
            combat_active: self.phase() == crate::BattlePhase::Combat,
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
                .zip(&self.actor_start_centers)
                .map(|((actor, runtime), &effect_center)| ActorFrame {
                    state: actor.clone(),
                    effect_center,
                    activity: runtime.task.activity(actor),
                    cast_releasing: runtime.task.action().is_some_and(|(_, sequence)| {
                        match &sequence.execution {
                            Execution::Casting(crate::casting::CastingRun::Running(cast)) => {
                                cast.releasing()
                            }
                            Execution::Recovering { .. } => matches!(
                                sequence.definition.execution,
                                crate::ActionExecution::Casting(_)
                            ),
                            _ => false,
                        }
                    }),
                    stun_struggling: matches!(
                        runtime.task,
                        ActorTask::Stunned {
                            struggle_ticks: 1..,
                            ..
                        }
                    ),
                    stun_remaining: match runtime.task {
                        ActorTask::Stunned { remaining, .. } => remaining,
                        _ => 0,
                    },
                })
                .collect(),
            models: Vec::new(),
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
                .filter_map(|(&id, volley)| volley.effect_origin.map(|point| (id, point)))
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
                ActorTask::Recovering { .. } | ActorTask::Released
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
    ) -> Result<u16> {
        self.start_unison_action(request, true, cues)
    }

    pub(crate) fn start_unison_combination(
        &mut self,
        request: ActionRequest,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        self.start_unison_action(request, false, cues).map(|_| ())
    }

    fn start_unison_action(
        &mut self,
        request: ActionRequest,
        arte: bool,
        cues: &mut Vec<Cue>,
    ) -> Result<u16> {
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
        let (action, variation) = if arte {
            ensure!(
                self.learned_technique(request.actor, request.action)
                    .is_some_and(|row| row.capabilities.target == crate::TechniqueTarget::Enemy),
                "Unison arte is not a learned offensive technique"
            );
            let selected = self.elemental_action(request.actor, request.action, cues);
            self.record_unison_use(request.actor, request.action)?;
            selected
        } else {
            (request.action, None)
        };
        let (id, mut sequence) = self.allocate_sequence(action, request.actor, request.target)?;
        if let Some(definition) = variation {
            sequence.definition = definition;
        }
        let duration = match (&sequence.definition.execution, action != request.action) {
            (crate::ActionExecution::Attack(attack), true) => attack.end_at,
            _ => 0,
        };
        sequence.unison = true;
        sequence.combo = None;
        self.cancel_pending_item(request.actor);
        self.interrupt_actor(request.actor, cues);
        self.clear_cast_memory(request.actor);
        self.clear_matching_technique_command(request.actor, request.action);
        self.actors[request.actor.index()].proficiency = 0;
        if arte {
            self.snapshot_technique_proficiency(request.actor, action);
        }
        let index = request.actor.index();
        self.runtime[index].set_target(request.target);
        self.actors[index].guard.active = false;
        self.actors[index].attack_power = 100;
        self.actors[index].movement.locomotion = crate::Locomotion::Action;
        self.put_sequence(id, sequence);
        cues.push(Cue::Started {
            definition: Some(action),
            action: id,
            actor: request.actor,
        });
        Ok(duration)
    }

    /// Commit an action immediately after its admission check.
    pub(crate) fn start_admitted_action(
        &mut self,
        request: ActionRequest,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        let (mut action, variation) = self.elemental_action(request.actor, request.action, cues);
        let selected = action;
        let mut learning = false;
        let debit = if matches!(
            &self.prepared.actions[action].execution,
            crate::ActionExecution::Attack(_)
        ) {
            let base_learning = if action != request.action {
                self.commit_martial_use(request.actor, request.action)?
            } else {
                None
            };
            let acquired = match base_learning {
                Some(learned) => Some(learned),
                None => self.commit_martial_use(request.actor, action)?,
            };
            if let Some(learned) = acquired {
                action = learned;
                learning = true;
                0
            } else {
                self.action_quote(request.actor, action)
            }
        } else {
            0
        };
        // Commit payment before any action commands execute.
        ensure!(
            debit <= u32::from(self.actors[request.actor.index()].tp),
            "admitted action exceeds available TP"
        );
        let (id, mut sequence) = self.allocate_sequence(action, request.actor, request.target)?;
        if action == selected
            && let Some(definition) = variation
        {
            sequence.definition = definition;
        }
        sequence.learning = learning;
        if learning {
            self.request_timed_hold(crate::casting::LEARNING_HOLD_TICKS, None);
        }
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
            crate::ActionExecution::LeaveArena => {}
            crate::ActionExecution::Attack(attack) => {
                if sequence.definition.normal.is_some() {
                    self.apply_normal_guard(request.actor, attack.end_at);
                }
                self.clear_matching_technique_command(request.actor, request.action);
                self.snapshot_technique_proficiency(request.actor, action);
            }
            crate::ActionExecution::Casting(cast) => {
                self.snapshot_technique_proficiency(request.actor, action);
                if cast.release.uses_support_target()
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
        self.ledger.achievements[index].acted = true;
        self.ledger.achievements[index].used_arte |= self
            .technique_catalogue_for_action(request.actor, action)
            .is_some();
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
            || threat.element.is_none()
            || !matches!(
                target_actor.affinity(threat.element),
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

    pub(crate) fn consume_pending_special_guard(
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
        if !self.special_guard_learned(actor, binding) || technique.catalogue != catalogue {
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
            self.begin_paralysis(actor, cues);
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

    #[allow(
        clippy::too_many_arguments,
        reason = "Projectile identity, placement, and optional aim stay explicit at emission"
    )]
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
        let mut projectile = Projectile::new(definition, action, frame, self.actors.len());
        projectile.attack_power = self.actor(owner)?.attack_power;
        projectile.target_point = match aim {
            Some(point) => point,
            None => *self
                .actor_start_centers
                .get(target.index())
                .context("missing projectile target")?,
        };
        #[cfg(any(test, feature = "test-support"))]
        let input_key = crate::random_inputs::Key {
            clock: self.ledger.elapsed_ticks,
            owner: owner,
            target: target,
            action: action.0,
            contact: crate::random_inputs::Contact::Projectile(id.0),
        };
        let draw = |axis, amplitude| {
            #[cfg(any(test, feature = "test-support"))]
            if let Some(inputs) = &mut self.random_inputs {
                return inputs.velocity(input_key, axis, amplitude);
            }
            #[cfg(not(any(test, feature = "test-support")))]
            let _ = (axis, amplitude);
            Ok(self.random.next_u16())
        };
        if deferred {
            projectile.prepare_velocity(draw)?;
            projectile.pending_launch = true;
        } else if let Some(birth) =
            projectile.initialize(&mut self.pending_cues, draw, direction)?
        {
            self.pending_cues.push(Cue::InitializedEffect(birth));
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
        self.recover_vitals(id, percent, true, true, cues)
    }

    pub(crate) fn recover_vitals(
        &mut self,
        id: ActorId,
        percent: i16,
        lucky: bool,
        floating: bool,
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
        cues.push(actor.recover_percent(id, RecoveryKind::Hp, value, floating));
        Ok(())
    }
}

#[cfg(test)]
mod melee_tests;

#[cfg(test)]
mod spell_tests;
