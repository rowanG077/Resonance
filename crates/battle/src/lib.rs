/// Maximum active party size.
pub const PARTY_CAPACITY: usize = 4;

mod actor_common;
mod secondary_wind;
pub use secondary_wind::SecondaryWind;
mod seals;
pub use seals::SealDefinition;
mod common_recovery;
pub use common_recovery::{CommonRecoveryState, CommonRecoveryTraits};
mod camera;
mod capture;
mod casting;
mod spell_charge;
pub use spell_charge::SpellChargeDefinition;
pub mod conditions;
mod contact;
#[cfg(feature = "test-support")]
pub mod contact_observation;
mod contact_recovery;
pub use contact_recovery::ContactRecovery;
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
mod drains;
mod lethal_rescue;
pub use lethal_rescue::{LethalRescueTraits, RescueEquipment, RescueKind};
mod decision;
mod defense_system;
mod hosted;
pub use hosted::HostedParticipant;
mod distance;
mod effect;
mod enemy_decision;
mod enemy_policy;
mod enemy_transformation;
pub use enemy_policy::EnemyBehavior;
mod enemy_presentation;
pub use enemy_presentation::{EnemyPresentation, EnemyPresentationFrame};
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
mod technical_smash;
pub use action_selection::{ComboTraits, InputIntent};
pub use technical_smash::TechnicalSmash;
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
pub use attack::{
    AttackEvent, AttackOrigin, AttackPose, HammerProjectile, HammerRing, PreparedAttack,
    ProjectileScatter, ProjectileShot, StealDefinition,
};
mod movement;
mod outcome;
mod overlimit;
mod victory;
pub use overlimit::OverLimit;
mod particle;
#[cfg(feature = "test-support")]
pub use particle::observed::FixtureParticle;
mod petrify;
mod prepare;
mod projectile;
mod release;
pub use release::{
    PreparedSpell, RecoveryEffectPlacement, RecoveryTargets, SpellBenefit, SpellCamera,
    SpellEffect, SpellEffectPlacement, SpellLaunch, SpellLift, SpellOrigin, SpellPolicy,
    SpellProjectile, SpellRecovery, SpellStartup, StageTint,
};
mod action;
#[cfg(any(test, feature = "test-support"))]
pub mod random_inputs;
mod reaction;
mod recoil;
mod state;
pub use state::ActorSet;
mod steering;
mod stun;
mod summon_entry;
pub use summon_entry::{SummonEntryDefinition, SummonFrame};
mod summon;
pub use summon::{
    PreparedSummon, SummonBlessing, SummonContact, SummonEffect, SummonHeading, SummonOrigin,
    SummonPoint,
};
mod target;
mod taunt;
mod unison;
pub use unison::{
    UnisonArte, UnisonCombination, UnisonCombinationFrame, UnisonCompletion, UnisonDefinition,
    UnisonFeedback, UnisonFrame, UnisonInput, UnisonMember, UnisonPhase, UnisonPlacement,
    UnisonPrelude, UnisonRecipe, UnisonRole,
};
mod tp;
pub use taunt::MAX_UNISON_GAUGE;
mod control_ex;
pub use control_ex::{ChargeLevel, ControlExState, ControlExTraits};
mod voice;
mod weapon_flight;

pub use camera::{CameraDefinition, CameraPose, VERTICAL_FOV_DEGREES};
pub use casting::{
    CastPhase, CastReleasePose, CastReplacement, CastReplacementTrigger, CastingDefinition,
    CastingState, CastingThreat, CastingTraits,
};
pub use control::{
    ButtonInput, ControlDefinition, ControlInput, ControlMotions, Locomotion, NormalAttack,
    NormalControl, ShortcutEdit, direction_from_heading, project_screen_point, project_screen_x,
};
pub use damage::{
    Affinity, AttackElements, CombatAdjustments, CombatStats, DamageKind, DamageTraits, Element,
    HitCondition, HitElement, HitResult, HitRule, Power,
};
pub use death::ActorAvailability;
pub use decision::{DecisionDefinition, EntryChoice};
pub use defense_system::{DefenseArm, DefenseSystemDefinition};
#[cfg(feature = "test-support")]
pub use enemy_decision::FixtureEnemyChainInput;
pub use enemy_decision::{EnemyChoice, EnemyDecisionDefinition, EnemyRequirements};
pub use geometry::{Body, Collider, HitShape};
pub use guard::{Activity, Control, Guard, GuardKind, GuardResult, GuardRule};
pub use knockdown::{HitProtection, Protection, ProtectionMode, Stagger};
pub use ledger::Ledger;
pub use melee::{MeleeAnchor, MeleeDefinition, MeleeVolume};
pub use model::{
    ActorShadowFrame, BodySphere, CommonPose, EffectModelDefinition, EffectModelFrame,
    MainMotionObservation, ModelDefinition, ModelFrame, ModelMaterial, ModelRequest, ModelSample,
    ModelSampler, Models, MotionBinding, Playback, Pose, PreparedEffectModel, ReactionMotions,
    ShadowDefinition, WeaponDefinition, WeaponFrame, WeaponLayerDefinition, WeaponPlayback,
    WeaponTextureAnimation,
};
#[cfg(feature = "test-support")]
pub use movement::FixtureMotion;
pub use movement::{Floor, Hover, Movement};
pub use outcome::{BattlePhase, DepartureReason, TransitionFrame, TransitionKind};
pub use particle::{
    ParticleDefinition, ParticleFrame, ParticleGeometry, ParticleId, ParticleState,
    ParticleTemplate,
};
pub use prepare::{
    ActionDefinition, ActionDefinitions, ActionExecution, ActionKey, ActorSetup, PreparedBattle,
    Sound,
};
pub use projectile::{
    EffectAppearance, ProjectileBoneDirection, ProjectileContact, ProjectileContactFeedback,
    ProjectileDefinition, ProjectileDeparture, ProjectileEffects, ProjectileFrame, ProjectileId,
    ProjectileMotion, ProjectileOrigin, ProjectileResponse, ProjectileShadow,
    ProjectileShadowFrame, ProjectileSteering, ProjectileVelocityReset,
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

#[cfg(feature = "test-support")]
pub use effect::FixtureEffectBirth;
pub use effect::{
    EffectBank, EffectDefinition, EffectFollow, EffectRequest, Effects, PassiveEffect,
};

pub use weapon_flight::WeaponFlightFrame;
