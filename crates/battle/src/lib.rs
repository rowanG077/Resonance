/// Limits for active combat participants, shared by preparation and per-actor storage.
pub const PARTY_CAPACITY: usize = 4;
pub const ENEMY_CAPACITY: usize = 8;
pub const ACTOR_CAPACITY: usize = PARTY_CAPACITY + ENEMY_CAPACITY;
// Scan masks must represent every admitted actor.
const _: () = assert!(ACTOR_CAPACITY <= u16::BITS as usize);

mod actor_common;
mod common_recovery;
pub use common_recovery::{CommonRecoveryState, CommonRecoveryTraits};
mod camera;
mod casting;
mod spell_charge;
pub use spell_charge::SpellChargeDefinition;
pub mod conditions;
mod contact;
mod contact_ex;
pub use contact_ex::ContactEx;
mod approach;
mod control;
mod ground_control;
mod mobility;
mod player_control;
pub use approach::ApproachParameters;
mod companion;
pub use companion::{CompanionDefinition, CompanionPolicy, PolicyLimits, StrategyRefresh};
mod damage;
mod death;
mod lethal_rescue;
pub use lethal_rescue::{LethalRescueTraits, RescueEquipment, RescueKind};
mod decision;
mod distance;
mod effect;
mod enemy_decision;
mod escape;
pub use escape::{EscapeActorDefinition, EscapeDefinition, EscapeFrame, MAX_ESCAPE_GAUGE};
mod action_selection;
mod aerial_spell;
mod geometry;
mod guard;
pub mod item;
mod kill_recovery;
mod knockdown;
pub mod learning;
mod ledger;
pub use action_selection::{ComboTraits, InputIntent};
mod technique_command;
mod technique_uses;
pub use technique_command::{
    ArteFamily, AssistShortcuts, PreparedTechnique, RegalArteFamily, TechniqueCapabilities,
    TechniqueTarget,
};
mod attack;
mod body_push;
mod melee;
mod model;
pub use attack::{AttackEvent, AttackPose, PreparedAttack};
mod movement;
mod outcome;
mod overlimit;
mod victory;
pub use overlimit::OverLimit;
mod particle;
mod petrify;
mod prepare;
mod projectile;
mod release;
pub use release::PreparedVolley;
mod action;
mod reaction;
mod recoil;
mod state;
mod steering;
mod stun;
mod target;
mod taunt;
mod tp;
pub use taunt::MAX_UNISON_GAUGE;
mod control_ex;
pub use control_ex::{ChargeLevel, ControlExState, ControlExTraits};
mod voice;
mod weapon_flight;

pub use camera::{CameraDefinition, CameraPose, VERTICAL_FOV_DEGREES};
pub use casting::{CastPhase, CastingDefinition, CastingState, CastingThreat, CastingTraits};
pub use control::{
    ButtonInput, ControlDefinition, ControlInput, ControlMotions, Locomotion, NormalAttack,
    NormalControl, ShortcutEdit, direction_from_heading, project_screen_point, project_screen_x,
};
pub use damage::{
    Affinity, AttackElements, CombatStats, DamageKind, DamageTraits, Element, HitCondition,
    HitElement, HitResult, HitRule, Power,
};
pub use death::ActorAvailability;
pub use decision::{DecisionDefinition, EntryChoice};
pub use enemy_decision::{EnemyChoice, EnemyDecisionDefinition, EnemyRequirements};
pub use geometry::{Body, Collider, HitShape};
pub use guard::{Activity, Control, Guard, GuardKind, GuardResult, GuardRule};
pub use knockdown::{HitProtection, Protection, ProtectionMode, Stagger};
pub use ledger::Ledger;
pub use melee::{MeleeDefinition, MeleeVolume};
pub use model::{
    ActorShadowFrame, CommonPose, EffectModelDefinition, EffectModelFrame, MainMotionObservation,
    ModelDefinition, ModelFrame, ModelMaterial, ModelRequest, Models, MotionBinding, Playback,
    Pose, PreparedEffectModel, ReactionMotions, ShadowDefinition, WeaponDefinition, WeaponFrame,
    WeaponLayerDefinition, WeaponPlayback,
};
pub use movement::{Floor, Hover, Movement};
pub use outcome::{BattlePhase, TransitionFrame, TransitionKind};
pub use particle::{
    ParticleDefinition, ParticleFrame, ParticleGeometry, ParticleId, ParticleState,
    ParticleTemplate,
};
pub use prepare::{
    ActionDefinition, ActionDefinitions, ActionExecution, ActionKey, ActorSetup, PreparedBattle,
    Sound,
};
pub use projectile::{
    EffectAppearance, ProjectileContact, ProjectileDefinition, ProjectileEffects, ProjectileFrame,
    ProjectileId, ProjectileMotion, ProjectileResponse, ProjectileShadow, ProjectileShadowFrame,
    ProjectileSteering,
};
pub use reaction::{Armor, ContactTraits, Reaction, ReactionRule, RecoilDirection};
pub use recoil::{Recoil, RecoilKind, RecoilProfile, RecoilRule, VerticalRecoil};
pub use state::*;
pub use steering::{ArenaContact, RecoveryReturnDefinition, Steering};
pub use stun::Stun;
pub use target::{TargetActor, TargetPolicy, select_target};
pub use voice::VoicePriority;
pub use weapon_flight::WeaponFlightDefinition;

#[cfg(test)]
mod tests;

pub use effect::{
    EffectBank, EffectDefinition, EffectFollow, EffectRequest, Effects, MAX_PARTICLES,
};

pub use weapon_flight::WeaponFlightFrame;
