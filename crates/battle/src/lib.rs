//! Transient battle state and authored action execution, independent of the game,
//! renderer, filesystem and save data. Call `step` at the game's 60000/1001 cadence.
mod actor_common;
mod body_jitter;
mod camera;
mod casting;
mod contact;
mod contact_audio;
mod contact_feedback;
pub use contact_audio::{ContactActorAudio, ContactAudio, ContactVoices};
pub use contact_feedback::{ContactElementFeedback, ContactFeedback};
mod approach;
mod control;
mod mobility;
mod player_control;
pub use approach::ApproachParameters;
mod companion;
pub use companion::{CompanionDefinition, CompanionTechnique};
mod damage;
mod death;
mod decision;
mod distance;
mod effect;
mod enemy_decision;
mod geometry;
mod guard;
mod hud;
mod knockdown;
mod ledger;
mod markers;
pub use markers::{StunMarkerFrame, TargetMarkerFrame};
mod melee;
mod model;
mod movement;
mod outcome;
mod particle;
mod prepare;
mod projectile;
mod reaction;
mod recoil;
mod scene;
mod script;
mod stage;
mod state;
mod steering;
mod stun;
mod target;
mod trail;
mod voice;
mod weapon_flight;

pub use body_jitter::BodyJitter;
pub use camera::{ActorFraming, CameraDefinition, CameraPose, EntryCamera, ResultCameraParameters};
pub use casting::{CastMotion, CastingDefinition};
pub use control::{
    ButtonInput, ControlDefinition, ControlInput, ControlMotions, NormalControl, TechniqueControl,
    direction_from_heading, project_depth, project_screen_point, project_screen_x,
};
pub use damage::{
    Affinity, AttackElements, CombatStats, DamageKind, Element, HitElement, HitResult, HitRule,
    ImpactEffect, Power,
};
pub use death::{ActorAvailability, AllyDeathReaction, DeathBinding, DeathEffect, DeathFeedback};
pub use decision::{DecisionDefinition, EntryChoice, EntryTimers, EntryVoiceDefinition};
pub use enemy_decision::{EnemyChoice, EnemyDecisionDefinition};
pub use geometry::{Body, HitShape, HurtPoint};
pub use guard::{Activity, Control, Guard, GuardKind, GuardResult, GuardRule};
pub use hud::{ActorHud, ComboTracking, FloatingNumber, HudHolds, RecoveryKind, RecoveryNumber};
pub use knockdown::{HitProtection, KnockdownBinding, Protection, ProtectionMode, Stagger};
pub use ledger::{Ledger, TitleEvent};
pub use melee::MeleeDefinition;
pub use model::{
    ActorShadowFrame, Anchor, EffectModelDefinition, EffectModelFrame, EffectMotionBinding,
    ModelDefinition, ModelFrame, MotionBinding, Playback, PreparedEffectModel, ShadowDefinition,
    WeaponDefinition, WeaponFrame, WeaponPlayback,
};
pub use movement::{Floor, Hover, Movement};
pub use outcome::{BattlePhase, TransitionFrame, TransitionKind};
pub use particle::{
    ParticleDefinition, ParticleFrame, ParticleGeometry, ParticleId, ParticleState,
    ParticleTemplate,
};
pub use prepare::{
    ActionDefinition, ActionPhase, EffectBank, PreparedBattle, ResourceBinding, SoundBinding,
    VoiceLine,
};
pub use projectile::{
    EffectAppearance, ProjectileContact, ProjectileDefinition, ProjectileEffects, ProjectileFrame,
    ProjectileId, ProjectileMotion, ProjectileShadow, ProjectileSteering,
};
pub use reaction::{Armor, Reaction, ReactionRule, RecoilDirection};
pub use recoil::{Recoil, RecoilKind, RecoilProfile, RecoilRule, VerticalRecoil};
pub use scene::SceneFrame;
pub use script::native_declarations;
pub use state::*;
pub use steering::{ArenaContact, RecoveryReturnDefinition, Steering};
pub use stun::{Stun, StunBinding};
pub use target::{TargetActor, select_target};
pub use trail::{TrailDefinition, TrailFrame, TrailSource, TrailVertex};
pub use voice::VoiceId;
pub use weapon_flight::WeaponFlightDefinition;

#[cfg(test)]
mod tests;
