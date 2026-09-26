use crate::{ActionDefinition, ActionId, ActorId, Battle, Cue, ResourceBinding};
use anyhow::{Result, anyhow};
use std::collections::BTreeMap;
use symphonia_script::authored::{NativeDeclaration, NativeField, Type};
use symphonia_script_vm::{Host, Memory, NativeBindings, NativeResult, RunEvent, Tasks, Vm};

const MOBILITY_STATE: Type = Type::Record {
    name: "battle::MobilityState",
    fields: &[
        NativeField {
            name: "kind",
            ty: Type::I32,
        },
        NativeField {
            name: "initial",
            ty: Type::Bool,
        },
        NativeField {
            name: "count",
            ty: Type::I32,
        },
        NativeField {
            name: "height",
            ty: Type::F32,
        },
        NativeField {
            name: "vertical",
            ty: Type::F32,
        },
        NativeField {
            name: "recoil",
            ty: Type::Bool,
        },
    ],
};
const ACTOR: Type = Type::Handle("battle::Actor");
const ENTRY_VOICE_PARAMETERS: Type = Type::Record {
    name: "battle::EntryVoiceParameters",
    fields: &[
        NativeField {
            name: "repeated_formation",
            ty: Type::Bool,
        },
        NativeField {
            name: "major_enemy",
            ty: Type::Bool,
        },
        NativeField {
            name: "enemy_count",
            ty: Type::I32,
        },
        NativeField {
            name: "level_difference",
            ty: Type::I32,
        },
    ],
};
const COMPANION_COMBO: Type = Type::Record {
    name: "battle::CompanionCombo",
    fields: &[
        NativeField {
            name: "selection",
            ty: Type::I32,
        },
        NativeField {
            name: "index",
            ty: Type::I32,
        },
        NativeField {
            name: "limit",
            ty: Type::I32,
        },
    ],
};
const COMPANION_PARAMETERS: Type = Type::Record {
    name: "battle::CompanionParameters",
    fields: &[
        NativeField {
            name: "target_policy",
            ty: Type::I32,
        },
        NativeField {
            name: "skill_policy",
            ty: Type::I32,
        },
        NativeField {
            name: "position_policy",
            ty: Type::I32,
        },
        NativeField {
            name: "saved_position",
            ty: Type::I32,
        },
        NativeField {
            name: "level",
            ty: Type::I32,
        },
        NativeField {
            name: "level_difference",
            ty: Type::I32,
        },
        NativeField {
            name: "tp_limit",
            ty: Type::I32,
        },
        NativeField {
            name: "healing_limit",
            ty: Type::I32,
        },
        NativeField {
            name: "support_level_limit",
            ty: Type::I32,
        },
        NativeField {
            name: "technique_count",
            ty: Type::I32,
        },
        NativeField {
            name: "speed",
            ty: Type::F32,
        },
        NativeField {
            name: "turn_ticks",
            ty: Type::I32,
        },
        NativeField {
            name: "walk_speed",
            ty: Type::F32,
        },
    ],
};
const COMPANION_TECHNIQUE: Type = Type::Record {
    name: "battle::CompanionTechnique",
    fields: &[
        NativeField {
            name: "action",
            ty: Type::I32,
        },
        NativeField {
            name: "flags",
            ty: Type::I32,
        },
        NativeField {
            name: "cost",
            ty: Type::I32,
        },
        NativeField {
            name: "learning_route",
            ty: Type::I32,
        },
        NativeField {
            name: "minimum",
            ty: Type::F32,
        },
        NativeField {
            name: "maximum",
            ty: Type::F32,
        },
        NativeField {
            name: "enabled",
            ty: Type::Bool,
        },
    ],
};
const COMPANION_NORMAL: Type = Type::Record {
    name: "battle::CompanionNormal",
    fields: &[
        NativeField {
            name: "action",
            ty: Type::I32,
        },
        NativeField {
            name: "allowed",
            ty: Type::I32,
        },
        NativeField {
            name: "fallback",
            ty: Type::I32,
        },
        NativeField {
            name: "minimum",
            ty: Type::F32,
        },
        NativeField {
            name: "maximum",
            ty: Type::F32,
        },
    ],
};
const COMPANION_ACTOR: Type = Type::Record {
    name: "battle::CompanionActor",
    fields: &[
        NativeField {
            name: "available",
            ty: Type::Bool,
        },
        NativeField {
            name: "dead",
            ty: Type::Bool,
        },
        NativeField {
            name: "hp_percent",
            ty: Type::I32,
        },
        NativeField {
            name: "tp_percent",
            ty: Type::I32,
        },
        NativeField {
            name: "tp",
            ty: Type::I32,
        },
        NativeField {
            name: "flying",
            ty: Type::Bool,
        },
        NativeField {
            name: "height",
            ty: Type::F32,
        },
        NativeField {
            name: "guarding",
            ty: Type::Bool,
        },
        NativeField {
            name: "casting",
            ty: Type::Bool,
        },
        NativeField {
            name: "petrified",
            ty: Type::Bool,
        },
    ],
};
const ENEMY_PARAMETERS: Type = Type::Record {
    name: "battle::EnemyParameters",
    fields: &[
        NativeField {
            name: "count",
            ty: Type::I32,
        },
        NativeField {
            name: "strategy",
            ty: Type::I32,
        },
        NativeField {
            name: "difficulty",
            ty: Type::I32,
        },
        NativeField {
            name: "hp_percent",
            ty: Type::I32,
        },
        NativeField {
            name: "tp",
            ty: Type::I32,
        },
        NativeField {
            name: "distance",
            ty: Type::I32,
        },
        NativeField {
            name: "rank",
            ty: Type::I32,
        },
        NativeField {
            name: "cap",
            ty: Type::I32,
        },
        NativeField {
            name: "back_count",
            ty: Type::I32,
        },
        NativeField {
            name: "speed",
            ty: Type::F32,
        },
        NativeField {
            name: "turn_ticks",
            ty: Type::I32,
        },
    ],
};
const ENEMY_CHOICE: Type = Type::Record {
    name: "battle::EnemyChoice",
    fields: &[
        NativeField {
            name: "action",
            ty: Type::I32,
        },
        NativeField {
            name: "weight",
            ty: Type::I32,
        },
        NativeField {
            name: "requirements",
            ty: Type::I32,
        },
        NativeField {
            name: "target_policy",
            ty: Type::I32,
        },
        NativeField {
            name: "minimum",
            ty: Type::I32,
        },
        NativeField {
            name: "maximum",
            ty: Type::I32,
        },
        NativeField {
            name: "tp",
            ty: Type::I32,
        },
        NativeField {
            name: "approach_minimum",
            ty: Type::F32,
        },
        NativeField {
            name: "approach_range",
            ty: Type::F32,
        },
    ],
};
const ENEMY_BACK_ROW: Type = Type::Record {
    name: "battle::EnemyBackRow",
    fields: &[
        NativeField {
            name: "choice",
            ty: Type::I32,
        },
        NativeField {
            name: "weight",
            ty: Type::I32,
        },
    ],
};
const PARTICLE: Type = Type::Handle("battle::Particle");
const EFFECT: Type = Type::Asset("battle::Effect");
const PROJECTILE: Type = Type::Asset("battle::Projectile");
const CASTING: Type = Type::Asset("battle::Casting");
const COLOR: Type = Type::Record {
    name: "battle::Color",
    fields: &[
        NativeField {
            name: "red",
            ty: Type::I32,
        },
        NativeField {
            name: "green",
            ty: Type::I32,
        },
        NativeField {
            name: "blue",
            ty: Type::I32,
        },
        NativeField {
            name: "alpha",
            ty: Type::I32,
        },
    ],
};
const EFFECT_TINT: Type = Type::Record {
    name: "battle::EffectTint",
    fields: &[
        NativeField {
            name: "enabled",
            ty: Type::Bool,
        },
        NativeField {
            name: "palette",
            ty: Type::I32,
        },
        NativeField {
            name: "red",
            ty: Type::I32,
        },
        NativeField {
            name: "green",
            ty: Type::I32,
        },
        NativeField {
            name: "blue",
            ty: Type::I32,
        },
    ],
};
const CAST_MOTION: Type = Type::Record {
    name: "battle::CastMotion",
    fields: &[
        NativeField {
            name: "age",
            ty: Type::Ticks,
        },
        NativeField {
            name: "motion",
            ty: Type::Asset("battle::Motion"),
        },
        NativeField {
            name: "blend",
            ty: Type::Ticks,
        },
        NativeField {
            name: "frame",
            ty: Type::F32,
        },
        NativeField {
            name: "rate",
            ty: Type::F32,
        },
        NativeField {
            name: "repeat",
            ty: Type::Bool,
        },
        NativeField {
            name: "loop_start",
            ty: Type::F32,
        },
    ],
};
const CAST_PARAMETERS: Type = Type::Record {
    name: "battle::CastParameters",
    fields: &[
        NativeField {
            name: "base",
            ty: Type::Ticks,
        },
        NativeField {
            name: "extra",
            ty: Type::Ticks,
        },
        NativeField {
            name: "recovery",
            ty: Type::Ticks,
        },
        NativeField {
            name: "release",
            ty: CAST_MOTION,
        },
        NativeField {
            name: "chant_count",
            ty: Type::I32,
        },
        NativeField {
            name: "pulse_member",
            ty: Type::I32,
        },
        NativeField {
            name: "effect_scale",
            ty: Type::F32,
        },
        NativeField {
            name: "tint",
            ty: EFFECT_TINT,
        },
    ],
};
const POINT: Type = Type::Record {
    name: "battle::Point",
    fields: &[
        NativeField {
            name: "x",
            ty: Type::F32,
        },
        NativeField {
            name: "y",
            ty: Type::F32,
        },
        NativeField {
            name: "z",
            ty: Type::F32,
        },
    ],
};
const INSTRUCTION_BUDGET: u32 = 8192;
const TASK_LIMIT: usize = 64;

#[repr(u8)]
enum Native {
    AtAge,
    AtCommandAge,
    AtHitAge,
    ActionEnd,
    ActionEndAt,
    AtAnimationAge,
    EndAnimation,
    WaitTicks,
    NextUpdate,
    Owner,
    Target,
    AllyCount,
    Ally,
    ActorAvailable,
    EntryVoiceParameters,
    Eligible,
    HealPercent,
    RevivePercent,
    Show,
    Finish,
    GroundPoint,
    Emit,
    HitWindow,
    ThrowWeapon,
    Animate,
    AnimationEnd,
    ForwardSpeed,
    VerticalSpeed,
    Acceleration,
    Gravity,
    Braking,
    Recover,
    Release,
    SpellActive,
    TpCost,
    ComboIndex,
    PayTp,
    BeginCastRelease,
    JitterBody,
    Notice,
    PlayMotion,
    TryPlayMotion,
    MotionIs,
    MotionDuration,
    AutomaticControl,
    CastRemaining,
    SetCastRemaining,
    AnimationFinished,
    IdlePose,
    SpawnParticle,
    ParticleAlive,
    ParticleAngles,
    SetParticleAngles,
    ParticleVelocity,
    SetParticleVelocity,
    ParticleOrbitVelocity,
    SetParticleOrbitVelocity,
    PolarPoint,
    ParticleUv,
    ParticleSize,
    SetParticleSize,
    EffectValue,
    SetEffectValue,
    RandomSigned,
    AiReady,
    AiRandomMod,
    AiSelectTarget,
    AiSetTarget,
    AiAdmit,
    AiRequest,
    AiRequestWithoutStop,
    AiIdleInitializing,
    AiHoverReady,
    AiReturnPosition,
    AiFaceDirection,
    AiFaceCurrent,
    AiCompanionParameters,
    AiCompanionTechnique,
    AiCompanionNormal,
    AiComboVisit,
    AiApproachVisit,
    AiApproachNormal,
    AiReselectNormal,
    AiCombo,
    AiChain,
    AiRefreshRetry,
    AiRetryFlags,
    AiActor,
    AiIdleTimer,
    AiSetIdleTimer,
    AiResetIdle,
    AiBodyGap,
    AiFaceTarget,
    AiEnemyParameters,
    AiEnemyChoice,
    AiEnemyBackRow,
    AiEnemyCommit,
    ShowFollowing,
    AttachEffect,
    WeaponTrail,
    WeaponVisible,
    ApplyEffectAppearance,
    Armor,
    ActionGuard,
    Sound,
    CastParameters,
    ChantStep,
    ParticleSizeVelocity,
    SetParticleSizeVelocity,
    SetParticleColor,
    SetParticleBrighten,
    SetParticleBrightenUntil,
    SetParticleUv,
    SetParticleGeometryCount,
    ParticleGeometryCount,
    ParticlePhase,
    SetParticlePhase,
    ParticleOffset,
    SetParticleOffset,
    EffectInteger,
    SetEffectInteger,
    RetargetUnavailable,
    RetainResident,
    SceneAvailable,
    BeginScene,
    SceneRemaining,
    ActivateScene,
    HideActors,
    ShowCentered,
    ParticleAngularVelocity,
    SetParticleAngularVelocity,
    ParticleSegmentAngleStep,
    SetParticleSegmentAngleStep,
    SetParticleCullBack,
    PlayEffectModel,
    SetParticleModel,
    ShowAt,
    ActorHeading,
    ShowOn,
    TintActor,
    TintStage,
    CameraBounds,
    Voice,
    CenterVoice,
    VoiceDuration,
    VoiceIdle,
    ActorPoint,
    EffectScale,
    EmitWithVelocity,
    ShowScaledAt,
    TextureLayers,
    ActorOffsetPoint,
    MobilityState,
    MobilityBegin,
    MobilityCount,
    MobilityLand,
    MobilityFinish,
    MobilityIntegrate,
    MobilityFace,
}
impl Native {
    const fn declaration(self) -> NativeDeclaration {
        let (name, parameters, result, suspends): (_, &[Type], _, _) = match self {
            Self::MobilityState => ("battle::mobility_state", &[], Some(MOBILITY_STATE), false),
            Self::MobilityBegin => ("battle::mobility_begin", &[], None, false),
            Self::MobilityCount => ("battle::mobility_count", &[Type::I32], None, false),
            Self::MobilityLand => ("battle::mobility_land", &[], None, false),
            Self::MobilityFinish => ("battle::mobility_finish", &[Type::Bool], None, false),
            Self::MobilityIntegrate => (
                "battle::mobility_integrate",
                &[Type::Bool],
                Some(Type::Bool),
                false,
            ),
            Self::MobilityFace => ("battle::mobility_face", &[], None, false),
            Self::AiEnemyParameters => (
                "battle::ai_enemy_parameters",
                &[],
                Some(ENEMY_PARAMETERS),
                false,
            ),
            Self::AiEnemyChoice => (
                "battle::ai_enemy_choice",
                &[Type::I32],
                Some(ENEMY_CHOICE),
                false,
            ),
            Self::AiEnemyBackRow => (
                "battle::ai_enemy_back_row",
                &[Type::I32],
                Some(ENEMY_BACK_ROW),
                false,
            ),
            Self::AiEnemyCommit => ("battle::ai_enemy_commit", &[Type::I32], None, false),
            Self::AiReady => ("battle::ai_ready", &[], Some(Type::Bool), false),
            Self::AiIdleInitializing => {
                ("battle::ai_idle_initializing", &[], Some(Type::Bool), false)
            }
            Self::AiHoverReady => ("battle::ai_hover_ready", &[], Some(Type::Bool), false),
            Self::AiReturnPosition => ("battle::ai_return_position", &[], Some(Type::Bool), false),
            Self::AiFaceDirection => (
                "battle::ai_face_direction",
                &[Type::F32],
                Some(Type::Bool),
                false,
            ),
            Self::AiFaceCurrent => (
                "battle::ai_face_current",
                &[Type::F32],
                Some(Type::Bool),
                false,
            ),
            Self::AiRandomMod => (
                "battle::ai_random_mod",
                &[Type::I32],
                Some(Type::I32),
                false,
            ),
            Self::AiSelectTarget => ("battle::ai_select_target", &[Type::I32], Some(ACTOR), false),
            Self::AiSetTarget => ("battle::ai_set_target", &[ACTOR], None, false),
            Self::AiAdmit => ("battle::ai_admit", &[Type::I32], Some(Type::Bool), false),
            Self::AiCompanionParameters => (
                "battle::ai_companion_parameters",
                &[],
                Some(COMPANION_PARAMETERS),
                false,
            ),
            Self::AiCompanionTechnique => (
                "battle::ai_companion_technique",
                &[Type::I32],
                Some(COMPANION_TECHNIQUE),
                false,
            ),
            Self::AiCompanionNormal => (
                "battle::ai_companion_normal",
                &[Type::I32],
                Some(COMPANION_NORMAL),
                false,
            ),
            Self::AiComboVisit => ("battle::ai_combo_visit", &[], None, true),
            Self::AiApproachVisit => ("battle::ai_approach_visit", &[], None, true),
            Self::AiApproachNormal => ("battle::ai_approach_normal", &[], Some(Type::I32), false),
            Self::AiReselectNormal => ("battle::ai_reselect_normal", &[Type::I32], None, false),
            Self::AiCombo => ("battle::ai_combo", &[], Some(COMPANION_COMBO), false),
            Self::AiChain => ("battle::ai_chain", &[Type::I32], Some(Type::Bool), false),
            Self::AiRefreshRetry => ("battle::ai_refresh_retry", &[], Some(Type::I32), false),
            Self::AiRetryFlags => ("battle::ai_retry_flags", &[], Some(Type::I32), false),
            Self::AiActor => ("battle::ai_actor", &[ACTOR], Some(COMPANION_ACTOR), false),
            Self::AiRequest => (
                "battle::ai_request",
                &[
                    Type::I32,
                    Type::F32,
                    Type::F32,
                    Type::Asset("battle::Motion"),
                    Type::Asset("battle::Motion"),
                    Type::F32,
                    Type::F32,
                    Type::I32,
                ],
                Some(Type::Bool),
                false,
            ),
            Self::AiIdleTimer => ("battle::ai_idle_timer", &[], Some(Type::I32), false),
            Self::AiRequestWithoutStop => (
                "battle::ai_request_without_stop",
                &[
                    Type::I32,
                    Type::F32,
                    Type::F32,
                    Type::Asset("battle::Motion"),
                    Type::F32,
                    Type::F32,
                    Type::I32,
                ],
                Some(Type::Bool),
                false,
            ),
            Self::AiSetIdleTimer => ("battle::ai_set_idle_timer", &[Type::I32], None, false),
            Self::AiResetIdle => ("battle::ai_reset_idle", &[], None, false),
            Self::AiBodyGap => ("battle::ai_body_gap", &[], Some(Type::F32), false),
            Self::AiFaceTarget => (
                "battle::ai_face_target",
                &[Type::F32],
                Some(Type::Bool),
                false,
            ),
            Self::TextureLayers => (
                "battle::texture_layers",
                &[Type::I32, Type::I32, Type::I32, Type::I32],
                None,
                false,
            ),
            Self::VoiceDuration => (
                "battle::voice_duration",
                &[ACTOR, Type::Asset("battle::Voice")],
                Some(Type::Ticks),
                false,
            ),
            Self::VoiceIdle => ("battle::voice_idle", &[ACTOR], Some(Type::Bool), false),
            Self::CenterVoice => ("battle::center_voice", &[ACTOR], None, false),
            Self::EntryVoiceParameters => (
                "battle::entry_voice_parameters",
                &[],
                Some(ENTRY_VOICE_PARAMETERS),
                false,
            ),
            Self::Voice => (
                "battle::voice",
                &[ACTOR, Type::Asset("battle::Voice"), Type::I32],
                Some(Type::Bool),
                false,
            ),
            Self::CameraBounds => (
                "battle::camera_bounds",
                &[Type::Ticks, Type::F32, Type::F32],
                None,
                false,
            ),
            Self::TintStage => (
                "battle::tint_stage",
                &[Type::I32, COLOR, Type::Ticks, Type::I32],
                None,
                false,
            ),
            Self::PlayEffectModel => (
                "battle::play_effect_model",
                &[Type::Asset("battle::EffectModel")],
                None,
                false,
            ),
            Self::SetParticleModel => (
                "battle::set_particle_model",
                &[PARTICLE, Type::I32],
                None,
                false,
            ),
            Self::ShowAt => (
                "battle::show_at",
                &[Type::Asset("battle::Effect"), Type::I32, POINT, Type::F32],
                None,
                false,
            ),
            Self::ActorHeading => ("battle::heading", &[ACTOR], Some(Type::F32), false),
            Self::EffectScale => ("battle::effect_scale", &[ACTOR], Some(Type::F32), false),
            Self::ActorPoint => (
                "battle::actor_point",
                &[ACTOR, Type::F32, Type::F32],
                Some(POINT),
                false,
            ),
            Self::ActorOffsetPoint => (
                "battle::actor_offset_point",
                &[ACTOR, Type::F32],
                Some(POINT),
                false,
            ),
            Self::EmitWithVelocity => (
                "battle::emit_with_velocity",
                &[PROJECTILE, POINT, POINT],
                None,
                false,
            ),
            Self::ShowScaledAt => (
                "battle::show_scaled_at",
                &[EFFECT, Type::I32, POINT, Type::F32, Type::F32],
                None,
                false,
            ),
            Self::ShowOn => ("battle::show_on", &[EFFECT, Type::I32, ACTOR], None, false),
            Self::TintActor => (
                "battle::tint_actor",
                &[ACTOR, Type::Asset("battle::ActorTints"), Type::I32],
                None,
                false,
            ),
            Self::SetParticleCullBack => (
                "battle::set_particle_cull_back",
                &[PARTICLE, Type::Bool],
                None,
                false,
            ),
            Self::ParticleAngularVelocity => (
                "battle::particle_angular_velocity",
                &[PARTICLE],
                Some(POINT),
                false,
            ),
            Self::SetParticleAngularVelocity => (
                "battle::set_particle_angular_velocity",
                &[PARTICLE, POINT],
                None,
                false,
            ),
            Self::ParticleOrbitVelocity => (
                "battle::particle_orbit_velocity",
                &[PARTICLE],
                Some(POINT),
                false,
            ),
            Self::SetParticleOrbitVelocity => (
                "battle::set_particle_orbit_velocity",
                &[PARTICLE, POINT],
                None,
                false,
            ),
            Self::ParticleSegmentAngleStep => (
                "battle::particle_segment_angle_step",
                &[PARTICLE],
                Some(Type::F32),
                false,
            ),
            Self::SetParticleSegmentAngleStep => (
                "battle::set_particle_segment_angle_step",
                &[PARTICLE, Type::F32],
                None,
                false,
            ),
            Self::ParticleGeometryCount => (
                "battle::particle_geometry_count",
                &[PARTICLE],
                Some(Type::I32),
                false,
            ),
            Self::ParticlePhase => (
                "battle::particle_phase",
                &[PARTICLE],
                Some(Type::I32),
                false,
            ),
            Self::SetParticlePhase => (
                "battle::set_particle_phase",
                &[PARTICLE, Type::I32],
                None,
                false,
            ),
            Self::ParticleOffset => ("battle::particle_offset", &[PARTICLE], Some(POINT), false),
            Self::SetParticleOffset => (
                "battle::set_particle_offset",
                &[PARTICLE, POINT],
                None,
                false,
            ),
            Self::EffectInteger => (
                "battle::effect_integer",
                &[Type::I32],
                Some(Type::I32),
                false,
            ),
            Self::SetEffectInteger => (
                "battle::set_effect_integer",
                &[Type::I32, Type::I32],
                None,
                false,
            ),
            Self::ParticleSizeVelocity => (
                "battle::particle_size_velocity",
                &[PARTICLE],
                Some(POINT),
                false,
            ),
            Self::SetParticleSizeVelocity => (
                "battle::set_particle_size_velocity",
                &[PARTICLE, POINT],
                None,
                false,
            ),
            Self::SetParticleColor => (
                "battle::set_particle_color",
                &[PARTICLE, Type::I32, Type::I32, Type::I32],
                None,
                false,
            ),
            Self::SetParticleBrighten => (
                "battle::set_particle_brighten",
                &[PARTICLE, Type::I32, Type::I32],
                None,
                false,
            ),
            Self::SetParticleBrightenUntil => (
                "battle::set_particle_brighten_until",
                &[PARTICLE, Type::Ticks],
                None,
                false,
            ),
            Self::ParticleUv => (
                "battle::particle_uv",
                &[PARTICLE, Type::I32],
                Some(Type::I32),
                false,
            ),
            Self::PolarPoint => (
                "battle::polar_point",
                &[POINT, Type::F32],
                Some(POINT),
                false,
            ),
            Self::SetParticleUv => (
                "battle::set_particle_uv",
                &[PARTICLE, Type::I32, Type::I32],
                None,
                false,
            ),
            Self::SetParticleGeometryCount => (
                "battle::set_particle_geometry_count",
                &[PARTICLE, Type::I32],
                None,
                false,
            ),
            Self::CastParameters => (
                "battle::cast_parameters",
                &[CASTING],
                Some(CAST_PARAMETERS),
                false,
            ),
            Self::ChantStep => (
                "battle::chant_step",
                &[CASTING, Type::I32],
                Some(CAST_MOTION),
                false,
            ),
            Self::Sound => (
                "battle::sound",
                &[Type::Asset("battle::Sound"), Type::I32],
                None,
                false,
            ),
            Self::Armor => ("battle::armor", &[Type::I32], None, false),
            Self::ParticleAlive => (
                "battle::particle_alive",
                &[PARTICLE],
                Some(Type::Bool),
                false,
            ),
            Self::SpawnParticle => (
                "battle::spawn_particle",
                &[Type::Asset("battle::ParticleTemplate")],
                Some(PARTICLE),
                false,
            ),
            Self::ParticleAngles => ("battle::particle_angles", &[PARTICLE], Some(POINT), false),
            Self::SetParticleAngles => (
                "battle::set_particle_angles",
                &[PARTICLE, POINT],
                None,
                false,
            ),
            Self::ParticleVelocity => {
                ("battle::particle_velocity", &[PARTICLE], Some(POINT), false)
            }
            Self::SetParticleVelocity => (
                "battle::set_particle_velocity",
                &[PARTICLE, POINT],
                None,
                false,
            ),
            Self::ParticleSize => ("battle::particle_size", &[PARTICLE], Some(POINT), false),
            Self::SetParticleSize => ("battle::set_particle_size", &[PARTICLE, POINT], None, false),
            Self::EffectValue => ("battle::effect_value", &[Type::I32], Some(Type::F32), false),
            Self::SetEffectValue => (
                "battle::set_effect_value",
                &[Type::I32, Type::F32],
                None,
                false,
            ),
            Self::RandomSigned => ("battle::random_signed", &[], Some(Type::I32), false),
            Self::ApplyEffectAppearance => {
                ("battle::apply_effect_appearance", &[PARTICLE], None, false)
            }
            Self::WeaponVisible => (
                "battle::weapon_visible",
                &[Type::I32, Type::Bool],
                None,
                false,
            ),
            Self::WeaponTrail => (
                "battle::weapon_trail",
                &[Type::I32, Type::Ticks],
                None,
                false,
            ),
            Self::AttachEffect => ("battle::attach_effect", &[EFFECT, Type::I32], None, false),
            Self::ShowFollowing => (
                "battle::show_following",
                &[EFFECT, Type::I32, ACTOR, Type::F32, Type::Bool, EFFECT_TINT],
                None,
                false,
            ),
            Self::ShowCentered => (
                "battle::show_centered",
                &[EFFECT, Type::I32, ACTOR, Type::F32, Type::Bool, EFFECT_TINT],
                None,
                false,
            ),
            Self::AtAge => ("battle::at_age", &[Type::Ticks], None, true),
            Self::ActionEnd => ("battle::action_end", &[], None, true),
            Self::ActionEndAt => ("battle::action_end_at", &[Type::Ticks], None, true),
            Self::AtHitAge => ("battle::at_hit_age", &[Type::Ticks], None, true),
            Self::AtCommandAge => ("battle::at_command_age", &[Type::Ticks], None, true),
            Self::AtAnimationAge => ("battle::at_animation_age", &[Type::Ticks], None, true),
            Self::EndAnimation => ("battle::end_animation", &[], None, false),
            Self::WaitTicks => ("battle::wait_ticks", &[Type::Ticks], None, true),
            Self::NextUpdate => ("battle::next_update", &[], None, true),
            Self::Owner => ("battle::owner", &[], Some(ACTOR), false),
            Self::Target => ("battle::target", &[], Some(ACTOR), false),
            Self::RetargetUnavailable => ("battle::retarget_unavailable", &[], Some(ACTOR), false),
            Self::AllyCount => ("battle::ally_count", &[], Some(Type::I32), false),
            Self::Ally => ("battle::ally", &[Type::I32], Some(ACTOR), false),
            Self::ActorAvailable => ("battle::actor_available", &[ACTOR], Some(Type::Bool), false),
            Self::Eligible => ("battle::eligible", &[ACTOR], Some(Type::Bool), false),
            Self::RevivePercent => ("battle::revive_percent", &[ACTOR, Type::I32], None, false),
            Self::HealPercent => ("battle::heal_percent", &[ACTOR, Type::I32], None, false),
            Self::Show => ("battle::show", &[EFFECT, Type::I32, ACTOR], None, false),
            Self::Finish => ("battle::finish", &[], None, false),
            Self::RetainResident => ("battle::retain_resident", &[], None, false),
            Self::SceneAvailable => ("battle::scene_available", &[], Some(Type::Bool), false),
            Self::BeginScene => (
                "battle::begin_scene",
                &[Type::Asset("battle::Spell"), Type::Ticks],
                None,
                false,
            ),
            Self::SceneRemaining => ("battle::scene_remaining", &[], Some(Type::Ticks), false),
            Self::ActivateScene => ("battle::activate_scene", &[], None, false),
            Self::HideActors => ("battle::hide_actors", &[], None, false),
            Self::GroundPoint => (
                "battle::ground_point",
                &[ACTOR, Type::F32, Type::F32],
                Some(POINT),
                false,
            ),
            Self::Emit => ("battle::emit", &[PROJECTILE, POINT], None, false),
            Self::HitWindow => (
                "battle::hit_window",
                &[Type::Asset("battle::Melee"), Type::Ticks, Type::Ticks],
                None,
                true,
            ),
            Self::ThrowWeapon => (
                "battle::throw_weapon",
                &[Type::Asset("battle::WeaponFlight"), Type::Ticks],
                None,
                true,
            ),
            Self::Animate => (
                "battle::animate",
                &[
                    Type::Asset("battle::Motion"),
                    Type::Ticks,
                    Type::F32,
                    Type::F32,
                    Type::Bool,
                ],
                None,
                true,
            ),
            Self::AnimationEnd => ("battle::animation_end", &[], None, true),
            Self::ForwardSpeed => (
                "battle::forward_speed",
                &[Type::F32, Type::Bool],
                None,
                false,
            ),
            Self::VerticalSpeed => ("battle::vertical_speed", &[Type::F32], None, false),
            Self::Acceleration => ("battle::acceleration", &[Type::F32], None, false),
            Self::Gravity => ("battle::gravity", &[Type::F32], None, false),
            Self::Braking => ("battle::braking", &[Type::F32], None, false),
            Self::ActionGuard => (
                "battle::action_guard",
                &[Type::I32, Type::Ticks, Type::Ticks],
                None,
                false,
            ),
            Self::Recover => ("battle::recover", &[Type::Ticks], None, true),
            Self::Release => (
                "battle::release",
                &[Type::Asset("battle::Spell"), Type::Bool],
                Some(Type::Bool),
                false,
            ),
            Self::SpellActive => (
                "battle::spell_active",
                &[Type::Bool],
                Some(Type::Bool),
                false,
            ),
            Self::TpCost => ("battle::tp_cost", &[], Some(Type::I32), false),
            Self::ComboIndex => ("battle::combo_index", &[], Some(Type::I32), false),
            Self::PayTp => ("battle::pay_tp", &[Type::I32], Some(Type::Bool), false),
            Self::BeginCastRelease => ("battle::begin_cast_release", &[], None, false),
            Self::JitterBody => ("battle::jitter_body", &[Type::Ticks], None, false),
            Self::Notice => ("battle::notice", &[Type::Ticks, Type::I32], None, false),
            Self::TryPlayMotion => (
                "battle::try_play_motion",
                &[
                    Type::Asset("battle::OptionalMotion"),
                    Type::Ticks,
                    Type::F32,
                    Type::F32,
                    Type::Bool,
                ],
                None,
                false,
            ),
            Self::PlayMotion => (
                "battle::play_motion",
                &[
                    Type::Asset("battle::Motion"),
                    Type::Ticks,
                    Type::F32,
                    Type::F32,
                    Type::Bool,
                    Type::F32,
                ],
                None,
                false,
            ),
            Self::MotionIs => (
                "battle::motion_is",
                &[Type::Asset("battle::Motion")],
                Some(Type::Bool),
                false,
            ),
            Self::MotionDuration => (
                "battle::motion_duration",
                &[Type::Asset("battle::Motion")],
                Some(Type::F32),
                false,
            ),
            Self::AutomaticControl => ("battle::automatic_control", &[], Some(Type::Bool), false),
            Self::CastRemaining => ("battle::cast_remaining", &[], Some(Type::Ticks), false),
            Self::SetCastRemaining => ("battle::set_cast_remaining", &[Type::Ticks], None, false),
            Self::AnimationFinished => ("battle::animation_finished", &[], Some(Type::Bool), false),
            Self::IdlePose => ("battle::idle_pose", &[Type::Ticks], None, false),
        };
        NativeDeclaration {
            name,
            opcode: self as u8,
            parameters,
            result,
            suspends,
        }
    }
}

struct Task {
    vm: Vm,
    wait: Option<Wait>,
}
enum Wait {
    Age(u32),
    CommandAge(i16),
    HitAge(i16),
    ActionEnd(u32),
    ComboVisit,
    ApproachVisit,
    AnimationAge(i16),
    NextUpdate,
    Join(i32),
    Melee,
    WeaponLaunch,
    AnimationReady,
    AnimationEnd,
    Recovery,
}

pub(crate) struct Resident {
    pub slot: crate::SpellSlot,
    pub phase: ResidentPhase,
    pub retained: bool,
}

#[derive(PartialEq, Eq)]
pub(crate) enum ResidentPhase {
    Initializing,
    Active,
    Retiring,
}

pub(crate) struct Sequence {
    pub definition: ActionDefinition,
    pub effect: Option<crate::effect::Context>,
    pub attached: Option<crate::effect::Attached>,
    pub actor: ActorId,
    pub target: ActorId,
    pub age: u32,
    pub finished: bool,
    pub recovery: Option<i16>,
    /// 295B8 retains reason 5 after its countdown reaches zero.
    pub action_recovery: bool,
    pub resident: Option<Resident>,
    cost_committed: bool,
    pub command_age: i16,
    pub hit_age: i16,
    pub hit_waiting: bool,
    pub hit_row: bool,
    pub animation_age: i16,
    animation_started: bool,
    pub animation_ended: bool,
    pub animation_row: bool,
    pub animation_held: bool,
    pub melee: Option<crate::melee::Window>,
    pub weapon_launch: Option<crate::weapon_flight::Launch>,
    tasks: BTreeMap<i32, Task>,
    ownership: Tasks,
    next_task: i32,
}
impl Sequence {
    pub(crate) fn tasks_complete(&self) -> bool {
        self.tasks.is_empty()
    }
    pub fn new(action: &ActionDefinition, actor: ActorId, target: ActorId) -> Result<Self> {
        Ok(Self {
            definition: action.clone(),
            effect: None,
            attached: None,
            actor,
            target,
            age: 0,
            finished: false,
            recovery: None,
            action_recovery: false,
            resident: (action.phase == crate::ActionPhase::Resident).then_some(Resident {
                slot: crate::SpellSlot::Primary,
                phase: ResidentPhase::Initializing,
                retained: false,
            }),
            cost_committed: false,
            command_age: 0,
            hit_age: 0,
            hit_waiting: false,
            hit_row: false,
            animation_age: 0,
            animation_started: false,
            animation_ended: false,
            animation_row: false,
            animation_held: false,
            melee: None,
            weapon_launch: None,
            tasks: BTreeMap::from([(
                1,
                Task {
                    vm: Vm::new(action.program.clone(), action.entry)?,
                    wait: None,
                },
            )]),
            ownership: Tasks::default(),
            next_task: 2,
        })
    }

    pub(crate) fn action_end_ready(&self) -> bool {
        self.tasks
            .values()
            .any(|task| matches!(task.wait, Some(Wait::ActionEnd(age)) if age <= self.age))
    }

    fn cancel_children(&mut self, parent: i32) {
        for child in self.ownership.children(parent) {
            self.cancel_children(child);
            self.tasks.remove(&child);
            self.ownership.remove(child);
            if self.melee.as_ref().is_some_and(|w| w.task == child) {
                self.melee = None;
            }
            if self
                .weapon_launch
                .as_ref()
                .is_some_and(|launch| launch.task == child)
            {
                self.weapon_launch = None;
            }
        }
    }
}

pub(crate) fn step_sequence(
    battle: &mut Battle,
    id: ActionId,
    sequence: &mut Sequence,
    cues: &mut Vec<Cue>,
) -> Result<()> {
    step_sequence_phase(battle, id, sequence, cues, Visit::Ordinary)
}

/// Resume the explicit completion boundary after this visit's contact and
/// chaining decisions. Other already-existing tasks do not receive a second visit.
pub(crate) fn step_action_end(
    battle: &mut Battle,
    id: ActionId,
    sequence: &mut Sequence,
    cues: &mut Vec<Cue>,
) -> Result<()> {
    step_sequence_phase(battle, id, sequence, cues, Visit::ActionEnd)
}

/// The decision's combo child is a separate callback on the same owned task
/// tree. Ordinary NextUpdate waiters do not receive a second visit.
pub(crate) fn step_combo_visit(
    battle: &mut Battle,
    id: ActionId,
    sequence: &mut Sequence,
    cues: &mut Vec<Cue>,
) -> Result<()> {
    step_sequence_phase(battle, id, sequence, cues, Visit::Combo)
}

pub(crate) fn step_approach_visit(
    battle: &mut Battle,
    id: ActionId,
    sequence: &mut Sequence,
    cues: &mut Vec<Cue>,
) -> Result<()> {
    step_sequence_phase(battle, id, sequence, cues, Visit::Approach)
}

#[derive(PartialEq, Eq)]
enum Visit {
    Ordinary,
    ActionEnd,
    Combo,
    Approach,
}

fn step_sequence_phase(
    battle: &mut Battle,
    id: ActionId,
    sequence: &mut Sequence,
    cues: &mut Vec<Cue>,
    visit: Visit,
) -> Result<()> {
    let new_tasks = sequence.next_task;
    let mut memory = Memory::default();
    // 636A8 can call the bounded nine-draw selector one hundred times.
    let mut budget = if sequence.definition.phase == crate::ActionPhase::Decision {
        65_536
    } else {
        INSTRUCTION_BUDGET
    };
    let mut cursor = 0;
    while let Some(handle) = sequence
        .tasks
        .range((cursor + 1)..)
        .next()
        .map(|(&handle, _)| handle)
    {
        cursor = handle;
        if handle < new_tasks {
            let eligible = match visit {
                Visit::Ordinary => true,
                Visit::ActionEnd => {
                    matches!(sequence.tasks[&handle].wait, Some(Wait::ActionEnd(age)) if age <= sequence.age)
                }
                Visit::Combo => matches!(sequence.tasks[&handle].wait, Some(Wait::ComboVisit)),
                Visit::Approach => {
                    matches!(sequence.tasks[&handle].wait, Some(Wait::ApproachVisit))
                }
            };
            if !eligible {
                continue;
            }
        }
        let mut task = sequence.tasks.remove(&handle).unwrap();
        match task.wait.take() {
            Some(Wait::ApproachVisit) if visit != Visit::Approach => {
                task.wait = Some(Wait::ApproachVisit);
                sequence.tasks.insert(handle, task);
                continue;
            }
            Some(Wait::ApproachVisit) => task.vm.complete(None, &mut memory)?,
            Some(Wait::ComboVisit) if visit != Visit::Combo => {
                task.wait = Some(Wait::ComboVisit);
                sequence.tasks.insert(handle, task);
                continue;
            }
            Some(Wait::ComboVisit) => task.vm.complete(None, &mut memory)?,
            Some(Wait::ActionEnd(age)) if visit != Visit::ActionEnd || sequence.age < age => {
                task.wait = Some(Wait::ActionEnd(age));
                sequence.tasks.insert(handle, task);
                continue;
            }
            Some(Wait::ActionEnd(_)) => task.vm.complete(None, &mut memory)?,
            Some(Wait::Age(age)) if sequence.age < age => {
                task.wait = Some(Wait::Age(age));
                sequence.tasks.insert(handle, task);
                continue;
            }
            Some(Wait::CommandAge(age)) if sequence.command_age < age => {
                task.wait = Some(Wait::CommandAge(age));
                sequence.tasks.insert(handle, task);
                continue;
            }
            Some(Wait::HitAge(age)) if sequence.hit_age < age => {
                task.wait = Some(Wait::HitAge(age));
                sequence.tasks.insert(handle, task);
                continue;
            }
            Some(Wait::HitAge(_)) => {
                sequence.hit_waiting = false;
                sequence.hit_row = true;
                task.vm.complete(None, &mut memory)?;
            }
            Some(Wait::AnimationAge(age)) if sequence.animation_age < age => {
                task.wait = Some(Wait::AnimationAge(age));
                sequence.tasks.insert(handle, task);
                continue;
            }
            Some(Wait::Age(_) | Wait::CommandAge(_) | Wait::AnimationAge(_) | Wait::NextUpdate) => {
                task.vm.complete(None, &mut memory)?
            }
            Some(Wait::Melee) if sequence.melee.as_ref().is_some_and(|w| w.task == handle) => {
                task.wait = Some(Wait::Melee);
                sequence.tasks.insert(handle, task);
                continue;
            }
            Some(Wait::Melee) => task.vm.complete(None, &mut memory)?,
            Some(Wait::WeaponLaunch)
                if sequence
                    .weapon_launch
                    .as_ref()
                    .is_some_and(|launch| launch.task == handle) =>
            {
                task.wait = Some(Wait::WeaponLaunch);
                sequence.tasks.insert(handle, task);
                continue;
            }
            Some(Wait::WeaponLaunch) => task.vm.complete(None, &mut memory)?,
            Some(Wait::AnimationReady)
                if battle.models[sequence.actor.index()]
                    .as_ref()
                    .is_some_and(|m| m.blending()) =>
            {
                task.wait = Some(Wait::AnimationReady);
                sequence.tasks.insert(handle, task);
                continue;
            }
            Some(Wait::AnimationReady | Wait::Recovery) => task.vm.complete(None, &mut memory)?,
            Some(Wait::AnimationEnd) => {
                if battle.models[sequence.actor.index()]
                    .as_ref()
                    .is_some_and(|m| m.finished())
                {
                    task.vm.complete(None, &mut memory)?;
                } else {
                    task.wait = Some(Wait::AnimationEnd);
                    sequence.tasks.insert(handle, task);
                    continue;
                }
            }
            Some(Wait::Join(child)) => {
                if let Some(result) = sequence
                    .ownership
                    .join(handle, child)
                    .map_err(|e| anyhow!(e))?
                {
                    task.vm.complete_task(&result)?;
                } else {
                    task.wait = Some(Wait::Join(child));
                    sequence.tasks.insert(handle, task);
                    continue;
                }
            }
            None => {}
        }
        let mut wait = None;
        let mut host = BattleHost {
            battle,
            sequence,
            id,
            handle,
            wait: &mut wait,
            cues,
        };
        let result = task
            .vm
            .run(&mut host, &mut memory, budget)
            .map_err(|error| {
                anyhow!(
                    "battle action {id:?}, age {}, source {:?}: {error}",
                    host.sequence.age,
                    task.vm.source_trace(error.pc)
                )
            })?;
        budget -= result.steps;
        match result.event {
            RunEvent::Halted => {
                sequence.cancel_children(handle);
                sequence
                    .ownership
                    .finish(handle, task.vm.result().unwrap_or_default());
            }
            RunEvent::Suspended { .. } => {
                task.wait =
                    Some(wait.ok_or_else(|| anyhow!("battle wait has no completion condition"))?);
                sequence.tasks.insert(handle, task);
            }
            RunEvent::SuspendedTask { handle: child } => {
                task.wait = Some(Wait::Join(child));
                sequence.tasks.insert(handle, task);
            }
        }
        if sequence.finished
            || sequence.recovery.is_some()
            || sequence.definition.phase == crate::ActionPhase::Actor
                && !sequence.animation_row
                && battle.models[sequence.actor.index()]
                    .as_ref()
                    .is_some_and(|model| model.blending())
        {
            // The initial motion is bound by admission before the first command
            // callback. Later animation rows run after that callback's commands.
            break;
        }
    }
    Ok(())
}

enum EffectOrigin {
    Fixed,
    Root,
    Center,
}

struct BattleHost<'a> {
    battle: &'a mut Battle,
    sequence: &'a mut Sequence,
    id: ActionId,
    handle: i32,
    wait: &'a mut Option<Wait>,
    cues: &'a mut Vec<Cue>,
}

impl BattleHost<'_> {
    fn spell(&self, index: i32) -> Result<u16, String> {
        match usize::try_from(index)
            .ok()
            .and_then(|i| self.sequence.definition.resources.get(i))
        {
            Some(ResourceBinding::Spell(spell)) => Ok(*spell),
            _ => Err("unbound battle spell".into()),
        }
    }
    fn casting(&self, index: i32) -> Result<&crate::CastingDefinition, String> {
        match usize::try_from(index)
            .ok()
            .and_then(|i| self.sequence.definition.resources.get(i))
        {
            Some(ResourceBinding::Casting(definition)) => Ok(definition),
            _ => Err("unbound battle casting parameters".into()),
        }
    }
    fn emit(&mut self, args: &[i32], velocity: Option<[f32; 3]>) -> Result<NativeResult, String> {
        let action = &self.sequence.definition;
        let Some(ResourceBinding::Projectile(definition)) = usize::try_from(args[0])
            .ok()
            .and_then(|i| action.resources.get(i))
        else {
            return Err("unbound battle projectile".into());
        };
        let position = std::array::from_fn(|i| f32::from_bits(args[i + 1] as u32));
        let result = if let Some(velocity) = velocity {
            self.battle.emit_with_velocity(
                definition.clone(),
                self.id,
                self.sequence.actor,
                self.sequence.target,
                (position, Some(velocity)),
            )
        } else {
            self.battle.emit(
                definition.clone(),
                self.id,
                self.sequence.actor,
                self.sequence.target,
                position,
            )
        };
        result.map_err(|e| e.to_string())?;
        Ok(NativeResult::Continue(None))
    }
    fn show_at(&mut self, args: &[i32], scale: f32) -> Result<NativeResult, String> {
        if !scale.is_finite() || scale < 0. {
            return Err("invalid battle effect scale".into());
        }
        let Some(ResourceBinding::Effect(resource)) = usize::try_from(args[0])
            .ok()
            .and_then(|i| self.sequence.definition.resources.get(i))
        else {
            return Err("unbound battle effect".into());
        };
        let member = u16::try_from(args[1]).map_err(|_| "invalid battle effect member")?;
        let origin = [args[2], args[3], args[4]].map(|v| f32::from_bits(v as u32));
        let heading = f32::from_bits(args[5] as u32);
        if !origin.iter().all(|v| v.is_finite()) || !heading.is_finite() {
            return Err("invalid battle effect placement".into());
        }
        self.battle
            .show_effect(
                crate::effect::Spawn {
                    action: self.id,
                    scene: self
                        .sequence
                        .effect
                        .as_ref()
                        .and_then(|effect| effect.scene)
                        .or_else(|| self.battle.scene_owner(self.id)),
                    owner: self.sequence.actor,
                    target: self.sequence.actor,
                    appearance: crate::EffectAppearance {
                        resource: *resource,
                        member,
                    },
                    origin,
                    heading,
                    follow: None,
                    scale,
                    late: false,
                    tint: Default::default(),
                },
                self.cues,
            )
            .map_err(|e| e.to_string())?;
        Ok(NativeResult::Continue(None))
    }
    fn show_following(
        &mut self,
        args: &[i32],
        origin: EffectOrigin,
    ) -> Result<NativeResult, String> {
        let byte = |value| u8::try_from(value).map_err(|_| "effect tint must be in 0..255");
        let tint = crate::effect::EffectTint {
            enabled: args[5] != 0,
            palette: byte(args[6])?,
            rgb: [byte(args[7])?, byte(args[8])?, byte(args[9])?],
        };
        self.show(
            args,
            origin,
            f32::from_bits(args[3] as u32),
            args[4] != 0,
            tint,
            self.sequence.actor,
        )
    }
    fn show(
        &mut self,
        args: &[i32],
        origin: EffectOrigin,
        scale: f32,
        late: bool,
        tint: crate::effect::EffectTint,
        owner: ActorId,
    ) -> Result<NativeResult, String> {
        if !scale.is_finite() {
            return Err("invalid battle effect scale".into());
        }
        let actor_id = self.actor_id(args[2])?;
        let Some(ResourceBinding::Effect(resource)) = usize::try_from(args[0])
            .ok()
            .and_then(|i| self.sequence.definition.resources.get(i))
        else {
            return Err("unbound battle effect".into());
        };
        let member = u16::try_from(args[1]).map_err(|_| "invalid battle effect member")?;
        let actor = &self.battle.actors[actor_id.index()];
        let (origin, follow) = match origin {
            EffectOrigin::Fixed => (actor.position, None),
            EffectOrigin::Root => (actor.position, Some(crate::effect::Follow::Actor(actor_id))),
            EffectOrigin::Center => (
                actor.body.center,
                Some(crate::effect::Follow::Center(actor_id)),
            ),
        };
        self.battle
            .show_effect(
                crate::effect::Spawn {
                    scene: self
                        .sequence
                        .effect
                        .as_ref()
                        .and_then(|effect| effect.scene)
                        .or_else(|| self.battle.scene_owner(self.id)),
                    action: self.id,
                    owner,
                    target: actor_id,
                    appearance: crate::EffectAppearance {
                        resource: *resource,
                        member,
                    },
                    origin,
                    heading: actor.heading,
                    follow,
                    scale,
                    late,
                    tint,
                },
                self.cues,
            )
            .map_err(|e| e.to_string())?;
        Ok(NativeResult::Continue(None))
    }
    fn wait_until(&mut self, age: u32) -> Result<NativeResult, String> {
        if age <= self.sequence.age {
            return Ok(NativeResult::Continue(None));
        }
        *self.wait = Some(Wait::Age(age));
        Ok(NativeResult::Suspend)
    }
    fn actor_id(&self, value: i32) -> Result<ActorId, String> {
        let id = ActorId(u8::try_from(value).map_err(|_| "invalid battle actor handle")?);
        self.battle.actor(id).map_err(|e| e.to_string())?;
        Ok(id)
    }
    fn voice_line(
        &self,
        actor: ActorId,
        resource: i32,
    ) -> Result<Option<crate::VoiceLine>, String> {
        let Some(ResourceBinding::Voice(lines)) = usize::try_from(resource)
            .ok()
            .and_then(|index| self.sequence.definition.resources.get(index))
        else {
            return Err("unbound actor voice".into());
        };
        Ok(lines[actor.index()])
    }
    fn motion(&mut self, bits: i32) -> Result<(&mut crate::Movement, f32), String> {
        if !self.sequence.definition.phase.is_actor()
            && self.sequence.definition.phase != crate::ActionPhase::Decision
        {
            return Err("movement needs an actor sequence".into());
        }
        let value = f32::from_bits(bits as u32);
        if !value.is_finite() {
            return Err("invalid battle movement value".into());
        }
        Ok((
            &mut self.battle.actors[self.sequence.actor.index()].movement,
            value,
        ))
    }
    fn bound_motion(
        &mut self,
        index: i32,
    ) -> Result<(&mut crate::model::Model, crate::MotionBinding), String> {
        let action = &self.sequence.definition;
        if !action.phase.is_actor() && action.phase != crate::ActionPhase::Decision {
            return Err("animation binding needs an actor sequence".into());
        }
        let Some(ResourceBinding::Motion(binding)) = usize::try_from(index)
            .ok()
            .and_then(|i| action.resources.get(i))
        else {
            return Err("unbound battle motion".into());
        };
        let model = self.battle.models[self.sequence.actor.index()]
            .as_mut()
            .ok_or("actor has no prepared model")?;
        Ok((model, *binding))
    }
    fn cast_remaining(&mut self) -> Result<&mut i16, String> {
        if self.sequence.definition.phase == crate::ActionPhase::Casting
            && let crate::Activity::Casting { clock, .. } =
                &mut self.battle.actors[self.sequence.actor.index()].activity
        {
            Ok(clock)
        } else {
            Err("casting clock needs a casting actor".into())
        }
    }
    fn play_motion(&mut self, args: &[i32]) -> Result<bool, String> {
        let blend = u8::try_from(args[1]).map_err(|_| "animation blend exceeds 255 updates")?;
        let (model, binding) = self.bound_motion(args[0])?;
        let frame = f32::from_bits(args[2] as u32);
        model
            .play(
                binding,
                frame,
                f32::from_bits(args[3] as u32),
                args[4] != 0,
                blend,
            )
            .map_err(|e| e.to_string())?;
        model
            .set_loop_start(args.get(5).map_or(frame, |v| f32::from_bits(*v as u32)))
            .map_err(|e| e.to_string())?;
        Ok(model.blending())
    }
    fn particle_vector(&mut self, id: i32, vector: u8) -> Result<&mut [f32; 3], String> {
        if vector == 6 {
            return Ok(&mut self.battle.owned_particle(id, self.id)?.orbit_velocity);
        }
        let state = self.battle.particle(id, self.id)?;
        match vector {
            0 => Ok(&mut state.angles),
            1 => Ok(&mut state.velocity),
            4 => Ok(&mut state.offset),
            5 => Ok(&mut state.angular_velocity),
            _ => match &mut state.geometry {
                crate::ParticleGeometry::Size {
                    value, velocity, ..
                } => Ok(if vector == 2 { value } else { velocity }),
                _ => Err("particle has no size vector".into()),
            },
        }
    }
    fn get_particle_vector(&mut self, id: i32, vector: u8) -> Result<NativeResult, String> {
        Ok(NativeResult::Values(
            self.particle_vector(id, vector)?
                .map(|v| v.to_bits() as i32)
                .to_vec(),
        ))
    }
    fn set_particle_vector(&mut self, args: &[i32], vector: u8) -> Result<NativeResult, String> {
        let value = std::array::from_fn(|i| f32::from_bits(args[i + 1] as u32));
        if !value.iter().all(|v| v.is_finite()) {
            return Err("invalid particle transform".into());
        }
        *self.particle_vector(args[0], vector)? = value;
        Ok(NativeResult::Continue(None))
    }
    fn effect_value(&mut self, index: i32) -> Result<&mut f32, String> {
        self.sequence
            .effect
            .as_mut()
            .and_then(|e| {
                usize::try_from(index)
                    .ok()
                    .and_then(|i| e.values.get_mut(i))
            })
            .ok_or_else(|| "invalid effect value slot".into())
    }
    fn effect_integer(&mut self, index: i32) -> Result<&mut i16, String> {
        self.sequence
            .effect
            .as_mut()
            .and_then(|effect| {
                usize::try_from(index)
                    .ok()
                    .and_then(|i| effect.integers.get_mut(i))
            })
            .ok_or_else(|| "invalid effect integer slot".into())
    }
    fn particle_phase(&mut self, id: i32) -> Result<&mut u8, String> {
        match &mut self.battle.particle(id, self.id)?.geometry {
            crate::ParticleGeometry::Ribbon { phase, .. } => Ok(phase),
            _ => Err("particle has no ribbon phase".into()),
        }
    }
    fn particle_segment_angle_step(&mut self, id: i32) -> Result<&mut f32, String> {
        match &mut self.battle.particle(id, self.id)?.geometry {
            crate::ParticleGeometry::BillboardTrail {
                segment_angle_step, ..
            } => Ok(segment_angle_step),
            _ => Err("particle has no segment angle step".into()),
        }
    }
    fn allies(&self) -> impl Iterator<Item = ActorId> + '_ {
        let side = self.battle.actors[self.sequence.actor.index()].side;
        self.battle
            .actors
            .iter()
            .enumerate()
            .filter(move |(_, actor)| actor.side == side)
            .map(|(i, _)| ActorId(i as u8))
    }
}

impl Host for BattleHost<'_> {
    const AUTHORED_NATIVES: NativeBindings<Self> = NativeBindings::<Self>::new()
        .register_typed(Native::PlayEffectModel.declaration(), |host, args, _| {
            let effect = host
                .sequence
                .effect
                .as_ref()
                .ok_or("model playback needs an effect sequence")?;
            let scene = effect.scene.ok_or("model playback needs a scene")?;
            let Some(ResourceBinding::EffectMotion(binding)) = usize::try_from(args[0])
                .ok()
                .and_then(|i| host.sequence.definition.resources.get(i))
            else {
                return Err("unbound effect model motion".into());
            };
            if effect.resource != binding.bank {
                return Err("model belongs to another effect bank".into());
            }
            host.battle
                .play_effect_model(scene, *binding)
                .map_err(|e| e.to_string())?;
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::SetParticleModel.declaration(), |host, args, _| {
            let model = u8::try_from(args[1]).map_err(|_| "invalid particle model slot")?;
            host.battle
                .set_particle_model(args[0], host.id, model)
                .map_err(|e| e.to_string())?;
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::ActorHeading.declaration(), |host, args, _| {
            let actor = host.actor_id(args[0])?;
            Ok(NativeResult::Continue(Some(
                host.battle.actors[actor.index()].heading.to_bits() as i32,
            )))
        })
        .register_typed(Native::EffectScale.declaration(), |host, args, _| {
            let actor = host.actor_id(args[0])?;
            Ok(NativeResult::Continue(Some(
                host.battle.actors[actor.index()].effect_scale.to_bits() as i32,
            )))
        })
        .register_typed(Native::ActorPoint.declaration(), |host, args, _| {
            let actor = host.actor_id(args[0])?;
            let actor = &host.battle.actors[actor.index()];
            let mut point = actor.position;
            point[1] += f32::from_bits(args[1] as u32)
                + actor.body.center_offset[1] * f32::from_bits(args[2] as u32);
            if !point.iter().all(|v| v.is_finite()) {
                return Err("invalid actor-relative point".into());
            }
            Ok(NativeResult::Values(
                point.map(|v| v.to_bits() as i32).to_vec(),
            ))
        })
        .register_typed(Native::ActorOffsetPoint.declaration(), |host, args, _| {
            let actor = host.actor_id(args[0])?;
            let actor = &host.battle.actors[actor.index()];
            let scale = f32::from_bits(args[1] as u32);
            let point: [f32; 3] = std::array::from_fn(|axis| {
                actor.body.center_offset[axis] * scale + actor.position[axis]
            });
            if !point.iter().all(|value| value.is_finite()) {
                return Err("invalid actor-relative point".into());
            }
            Ok(NativeResult::Values(
                point.map(|value| value.to_bits() as i32).to_vec(),
            ))
        })
        .register_typed(Native::ShowAt.declaration(), |host, args, _| {
            host.show_at(args, 1.)
        })
        .register_typed(Native::ShowScaledAt.declaration(), |host, args, _| {
            host.show_at(args, f32::from_bits(args[6] as u32))
        })
        .register_typed(
            Native::SetParticleCullBack.declaration(),
            |host, args, _| {
                host.battle.particle(args[0], host.id)?.cull_back = args[1] != 0;
                Ok(NativeResult::Continue(None))
            },
        )
        .register_typed(
            Native::ParticleAngularVelocity.declaration(),
            |host, args, _| host.get_particle_vector(args[0], 5),
        )
        .register_typed(
            Native::SetParticleAngularVelocity.declaration(),
            |host, args, _| host.set_particle_vector(args, 5),
        )
        .register_typed(
            Native::ParticleOrbitVelocity.declaration(),
            |host, args, _| host.get_particle_vector(args[0], 6),
        )
        .register_typed(
            Native::SetParticleOrbitVelocity.declaration(),
            |host, args, _| host.set_particle_vector(args, 6),
        )
        .register_typed(
            Native::ParticleSegmentAngleStep.declaration(),
            |host, args, _| {
                Ok(NativeResult::Continue(Some(
                    host.particle_segment_angle_step(args[0])?.to_bits() as i32,
                )))
            },
        )
        .register_typed(
            Native::SetParticleSegmentAngleStep.declaration(),
            |host, args, _| {
                let value = f32::from_bits(args[1] as u32);
                if !value.is_finite() {
                    return Err("invalid particle segment angle step".into());
                }
                *host.particle_segment_angle_step(args[0])? = value;
                Ok(NativeResult::Continue(None))
            },
        )
        .register_typed(Native::ParticleOffset.declaration(), |host, args, _| {
            host.get_particle_vector(args[0], 4)
        })
        .register_typed(Native::SetParticleOffset.declaration(), |host, args, _| {
            host.set_particle_vector(args, 4)
        })
        .register_typed(
            Native::ParticleGeometryCount.declaration(),
            |host, args, _| {
                Ok(NativeResult::Continue(Some(i32::from(
                    host.battle.particle(args[0], host.id)?.geometry_count,
                ))))
            },
        )
        .register_typed(Native::ParticlePhase.declaration(), |host, args, _| {
            Ok(NativeResult::Continue(Some(i32::from(
                *host.particle_phase(args[0])?,
            ))))
        })
        .register_typed(Native::SetParticlePhase.declaration(), |host, args, _| {
            *host.particle_phase(args[0])? =
                u8::try_from(args[1]).map_err(|_| "particle phase exceeds byte range")?;
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::EffectInteger.declaration(), |host, args, _| {
            Ok(NativeResult::Continue(Some(i32::from(
                *host.effect_integer(args[0])?,
            ))))
        })
        .register_typed(Native::SetEffectInteger.declaration(), |host, args, _| {
            *host.effect_integer(args[0])? =
                i16::try_from(args[1]).map_err(|_| "effect integer exceeds signed range")?;
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::Sound.declaration(), |host, args, _| {
            let Some(ResourceBinding::Sound(sound)) = usize::try_from(args[0])
                .ok()
                .and_then(|i| host.sequence.definition.resources.get(i))
            else {
                return Err("unbound battle sound".into());
            };
            let priority = u8::try_from(args[1]).map_err(|_| "invalid battle sound priority")?;
            if sound.index != 0 {
                host.cues.push(Cue::Sound {
                    actor: host.sequence.actor,
                    sound: *sound,
                    position: host.sequence.effect.as_ref().map_or_else(
                        || host.battle.actors[host.sequence.actor.index()].position,
                        |effect| effect.origin,
                    ),
                    priority,
                });
            }
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::SpawnParticle.declaration(), |host, args, _| {
            let effect = host
                .sequence
                .effect
                .as_ref()
                .ok_or("particle emission needs an effect sequence")?;
            let Some(ResourceBinding::Particle(definition)) = usize::try_from(args[0])
                .ok()
                .and_then(|i| host.sequence.definition.resources.get(i))
            else {
                return Err("unbound battle particle".into());
            };
            let id = host
                .battle
                .spawn_particle(
                    definition.clone(),
                    Some(host.id),
                    host.sequence.actor,
                    host.sequence.target,
                    effect.origin,
                    effect.heading,
                )
                .map_err(|e| e.to_string())?;
            if let Some(id) = id {
                let particle = host.battle.particles.get_mut(&id).unwrap();
                particle.scene = effect.scene;
                if definition.data.follow_origin {
                    particle.follow =
                        Some(effect.follow.ok_or("particle needs an origin attachment")?);
                }
            }
            Ok(NativeResult::Continue(Some(id.map_or(0, |id| id.0))))
        })
        .register_typed(Native::ParticleAlive.declaration(), |host, args, _| {
            Ok(NativeResult::Continue(Some(i32::from(
                host.battle.particle(args[0], host.id).is_ok(),
            ))))
        })
        .register_typed(
            Native::ApplyEffectAppearance.declaration(),
            |host, args, _| {
                let effect = host
                    .sequence
                    .effect
                    .as_ref()
                    .ok_or("particle appearance needs an effect sequence")?;
                host.battle
                    .apply_effect_appearance(args[0], host.id, effect.scale, effect.tint)?;
                Ok(NativeResult::Continue(None))
            },
        )
        .register_typed(Native::ParticleAngles.declaration(), |host, args, _| {
            host.get_particle_vector(args[0], 0)
        })
        .register_typed(Native::SetParticleAngles.declaration(), |host, args, _| {
            host.set_particle_vector(args, 0)
        })
        .register_typed(Native::ParticleVelocity.declaration(), |host, args, _| {
            host.get_particle_vector(args[0], 1)
        })
        .register_typed(Native::PolarPoint.declaration(), |_, args, _| {
            let angles = std::array::from_fn(|i| f32::from_bits(args[i] as u32));
            let radius = f32::from_bits(args[3] as u32);
            if !angles.iter().all(|v| v.is_finite()) || !radius.is_finite() {
                return Err("invalid polar point".into());
            }
            let point = crate::geometry::polar_point(angles, radius);
            Ok(NativeResult::Values(
                point.map(|value| value.to_bits() as i32).to_vec(),
            ))
        })
        .register_typed(Native::ParticleUv.declaration(), |host, args, _| {
            let state = host.battle.particle(args[0], host.id)?;
            let index = usize::try_from(args[1]).map_err(|_| "invalid particle UV index")?;
            let value = *state.uv.get(index).ok_or("invalid particle UV index")?;
            Ok(NativeResult::Continue(Some(i32::from(value))))
        })
        .register_typed(
            Native::SetParticleVelocity.declaration(),
            |host, args, _| host.set_particle_vector(args, 1),
        )
        .register_typed(Native::ParticleSize.declaration(), |host, args, _| {
            host.get_particle_vector(args[0], 2)
        })
        .register_typed(Native::SetParticleSize.declaration(), |host, args, _| {
            host.set_particle_vector(args, 2)
        })
        .register_typed(
            Native::ParticleSizeVelocity.declaration(),
            |host, args, _| host.get_particle_vector(args[0], 3),
        )
        .register_typed(
            Native::SetParticleSizeVelocity.declaration(),
            |host, args, _| host.set_particle_vector(args, 3),
        )
        .register_typed(Native::SetParticleColor.declaration(), |host, args, _| {
            let state = host.battle.particle(args[0], host.id)?;
            let channel = usize::try_from(args[1])
                .ok()
                .and_then(|edge| state.colors.get_mut(edge))
                .and_then(|color| usize::try_from(args[2]).ok().and_then(|i| color.get_mut(i)))
                .ok_or("invalid particle color channel")?;
            *channel = i16::try_from(args[3]).map_err(|_| "particle color exceeds signed range")?;
            Ok(NativeResult::Continue(None))
        })
        .register_typed(
            Native::SetParticleBrighten.declaration(),
            |host, args, _| {
                let state = host.battle.particle(args[0], host.id)?;
                let channel = usize::try_from(args[1])
                    .ok()
                    .and_then(|i| state.brighten.get_mut(i))
                    .ok_or("invalid particle brightening channel")?;
                *channel =
                    u8::try_from(args[2]).map_err(|_| "particle brightening exceeds byte range")?;
                Ok(NativeResult::Continue(None))
            },
        )
        .register_typed(
            Native::SetParticleBrightenUntil.declaration(),
            |host, args, _| {
                host.battle.particle(args[0], host.id)?.brighten_until = u8::try_from(args[1])
                    .map_err(|_| "particle brightening duration exceeds byte range")?;
                Ok(NativeResult::Continue(None))
            },
        )
        .register_typed(Native::SetParticleUv.declaration(), |host, args, _| {
            let state = host.battle.particle(args[0], host.id)?;
            let component = usize::try_from(args[1])
                .ok()
                .and_then(|i| state.uv.get_mut(i))
                .ok_or("invalid particle UV component")?;
            *component = i16::try_from(args[2]).map_err(|_| "particle UV exceeds signed range")?;
            Ok(NativeResult::Continue(None))
        })
        .register_typed(
            Native::SetParticleGeometryCount.declaration(),
            |host, args, _| {
                host.battle.particle(args[0], host.id)?.geometry_count = u8::try_from(args[1])
                    .map_err(|_| "particle geometry count exceeds byte range")?;
                Ok(NativeResult::Continue(None))
            },
        )
        .register_typed(Native::EffectValue.declaration(), |host, args, _| {
            Ok(NativeResult::Continue(Some(
                host.effect_value(args[0])?.to_bits() as i32,
            )))
        })
        .register_typed(Native::SetEffectValue.declaration(), |host, args, _| {
            let value = f32::from_bits(args[1] as u32);
            if !value.is_finite() {
                return Err("invalid effect value".into());
            }
            *host.effect_value(args[0])? = value;
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::RandomSigned.declaration(), |host, _, _| {
            Ok(NativeResult::Continue(Some(i32::from(
                host.battle.random.next() as i16,
            ))))
        })
        .register_typed(Native::AiReady.declaration(), |host, _, _| {
            Ok(NativeResult::Continue(Some(i32::from(
                host.battle.decision_ready(host.sequence.actor),
            ))))
        })
        .register_typed(Native::AiIdleInitializing.declaration(), |host, _, _| {
            Ok(NativeResult::Continue(Some(i32::from(
                host.battle.idle_initializing(host.sequence.actor),
            ))))
        })
        .register_typed(Native::AiHoverReady.declaration(), |host, _, _| {
            Ok(NativeResult::Continue(Some(i32::from(
                host.battle.actors[host.sequence.actor.index()]
                    .movement
                    .hover_ready(),
            ))))
        })
        .register_typed(Native::AiReturnPosition.declaration(), |host, _, _| {
            let walking = host
                .battle
                .return_position(host.sequence.actor)
                .map_err(|error| error.to_string())?;
            Ok(NativeResult::Continue(Some(i32::from(walking))))
        })
        .register_typed(Native::AiFaceDirection.declaration(), |host, args, _| {
            let step = f32::from_bits(args[0] as u32);
            if !step.is_finite() || step < 0. {
                return Err("invalid decision turn step".into());
            }
            let actor = &mut host.battle.actors[host.sequence.actor.index()];
            let direction = actor.movement.direction;
            actor.facing_direction = direction;
            let faced = crate::control::face_cached(actor, direction, step);
            Ok(NativeResult::Continue(Some(i32::from(faced))))
        })
        .register_typed(Native::AiFaceCurrent.declaration(), |host, args, _| {
            let step = f32::from_bits(args[0] as u32);
            if !step.is_finite() || step < 0. {
                return Err("invalid decision turn step".into());
            }
            let actor = &mut host.battle.actors[host.sequence.actor.index()];
            let direction = actor.facing_direction;
            Ok(NativeResult::Continue(Some(i32::from(
                crate::control::face_cached(actor, direction, step),
            ))))
        })
        .register_typed(Native::AiEnemyParameters.declaration(), |host, _, _| {
            Ok(NativeResult::Values(
                host.battle
                    .enemy_parameters(host.sequence.actor)
                    .map_err(|e| e.to_string())?,
            ))
        })
        .register_typed(Native::AiEnemyChoice.declaration(), |host, args, _| {
            Ok(NativeResult::Values(
                host.battle
                    .enemy_choice(host.sequence.actor, args[0])
                    .map_err(|e| e.to_string())?,
            ))
        })
        .register_typed(Native::AiEnemyBackRow.declaration(), |host, args, _| {
            let definition = host
                .battle
                .enemy_decision(host.sequence.actor)
                .map_err(|e| e.to_string())?;
            let &(choice, weight) = usize::try_from(args[0])
                .ok()
                .and_then(|i| definition.back_row.get(i))
                .ok_or("invalid enemy back-row choice")?;
            Ok(NativeResult::Values(vec![
                i32::from(choice),
                i32::from(weight),
            ]))
        })
        .register_typed(Native::AiEnemyCommit.declaration(), |host, args, _| {
            let definition = host
                .battle
                .enemy_decision(host.sequence.actor)
                .map_err(|e| e.to_string())?;
            let index = usize::try_from(args[0])
                .ok()
                .filter(|&i| i < definition.choices.len())
                .ok_or("invalid selected enemy row")?;
            // 355EC publishes actor104 before an action starts; 61578 reads
            // that selected row's signed guard byte during approach as well.
            let chance = definition.choices[index].guard_chance;
            let actor = host.sequence.actor.index();
            host.battle.enemy_selected[actor] = Some(index);
            host.battle.actors[actor].guard.enemy_chance = chance;
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::AiRandomMod.declaration(), |host, args, _| {
            let divisor = u16::try_from(args[0])
                .ok()
                .filter(|&v| v != 0)
                .ok_or("invalid decision random divisor")?;
            Ok(NativeResult::Continue(Some(i32::from(
                host.battle.random.next() % divisor,
            ))))
        })
        .register_typed(Native::AiSelectTarget.declaration(), |host, args, _| {
            let policy = u8::try_from(args[0]).map_err(|_| "invalid target policy")?;
            let target = host
                .battle
                .decision_target(host.sequence.actor, policy)
                .map_err(|e| e.to_string())?;
            Ok(NativeResult::Continue(Some(target.index() as i32)))
        })
        .register_typed(Native::AiSetTarget.declaration(), |host, args, _| {
            let target = host.actor_id(args[0])?;
            host.battle
                .set_decision_target(host.sequence.actor, target)
                .map_err(|e| e.to_string())?;
            host.sequence.target = target;
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::AiAdmit.declaration(), |host, args, _| {
            let action = u16::try_from(args[0]).map_err(|_| "invalid decision action")?;
            let admitted = host
                .battle
                .admit_decision(host.sequence.actor, host.sequence.target, action, host.cues)
                .map_err(|e| e.to_string())?;
            Ok(NativeResult::Continue(Some(i32::from(admitted))))
        })
        .register_typed(Native::AiRequest.declaration(), |host, args, _| {
            if host.sequence.definition.phase != crate::ActionPhase::Decision {
                return Err("approach request needs an authored decision".into());
            }
            let action = u16::try_from(args[0]).map_err(|_| "invalid decision action")?;
            let (_, motion) = host.bound_motion(args[3])?;
            let (_, stop_motion) = host.bound_motion(args[4])?;
            let parameters = crate::ApproachParameters {
                minimum: f32::from_bits(args[1] as u32),
                maximum: f32::from_bits(args[2] as u32),
                motion: Some(motion),
                stop_motion: Some(stop_motion),
                motion_rate: f32::from_bits(args[5] as u32),
                speed: f32::from_bits(args[6] as u32),
                turn_ticks: u8::try_from(args[7]).map_err(|_| "invalid decision turn ticks")?,
            };
            let admitted = host
                .battle
                .request_approach(
                    host.sequence.actor,
                    host.sequence.target,
                    action,
                    parameters,
                )
                .map_err(|e| e.to_string())?;
            Ok(NativeResult::Continue(Some(i32::from(admitted))))
        })
        .register_typed(
            Native::AiRequestWithoutStop.declaration(),
            |host, args, _| {
                if host.sequence.definition.phase != crate::ActionPhase::Decision {
                    return Err("approach request needs an authored decision".into());
                }
                let action = u16::try_from(args[0]).map_err(|_| "invalid decision action")?;
                let (_, motion) = host.bound_motion(args[3])?;
                let parameters = crate::ApproachParameters {
                    minimum: f32::from_bits(args[1] as u32),
                    maximum: f32::from_bits(args[2] as u32),
                    motion: Some(motion),
                    stop_motion: None,
                    motion_rate: f32::from_bits(args[4] as u32),
                    speed: f32::from_bits(args[5] as u32),
                    turn_ticks: u8::try_from(args[6]).map_err(|_| "invalid decision turn ticks")?,
                };
                let admitted = host
                    .battle
                    .request_approach(
                        host.sequence.actor,
                        host.sequence.target,
                        action,
                        parameters,
                    )
                    .map_err(|e| e.to_string())?;
                Ok(NativeResult::Continue(Some(i32::from(admitted))))
            },
        )
        .register_typed(Native::AiCompanionParameters.declaration(), |host, _, _| {
            Ok(NativeResult::Values(
                host.battle
                    .companion_parameters(host.sequence.actor)
                    .map_err(|e| e.to_string())?,
            ))
        })
        .register_typed(
            Native::AiCompanionTechnique.declaration(),
            |host, args, _| {
                let index =
                    usize::try_from(args[0]).map_err(|_| "invalid companion technique index")?;
                Ok(NativeResult::Values(
                    host.battle
                        .companion_technique(host.sequence.actor, index)
                        .map_err(|e| e.to_string())?,
                ))
            },
        )
        .register_typed(Native::AiCompanionNormal.declaration(), |host, args, _| {
            let index =
                usize::try_from(args[0]).map_err(|_| "invalid companion normal selector")?;
            Ok(NativeResult::Values(
                host.battle
                    .companion_normal(host.sequence.actor, index)
                    .map_err(|e| e.to_string())?,
            ))
        })
        .register_typed(Native::AiComboVisit.declaration(), |host, _, _| {
            if host.sequence.definition.phase != crate::ActionPhase::Decision {
                return Err("combo callback needs an authored decision".into());
            }
            *host.wait = Some(Wait::ComboVisit);
            Ok(NativeResult::Suspend)
        })
        .register_typed(Native::AiApproachVisit.declaration(), |host, _, _| {
            if host.sequence.definition.phase != crate::ActionPhase::Decision {
                return Err("approach callback needs an authored decision".into());
            }
            *host.wait = Some(Wait::ApproachVisit);
            Ok(NativeResult::Suspend)
        })
        .register_typed(Native::AiApproachNormal.declaration(), |host, _, _| {
            Ok(NativeResult::Continue(Some(
                host.battle
                    .approach_normal(host.sequence.actor)
                    .map_or(-1, i32::from),
            )))
        })
        .register_typed(Native::AiReselectNormal.declaration(), |host, args, _| {
            if host.sequence.definition.phase != crate::ActionPhase::Decision {
                return Err("approach selection needs an authored decision".into());
            }
            let selector = u8::try_from(args[0]).map_err(|_| "invalid approach normal selector")?;
            host.battle
                .reselect_approach_normal(host.sequence.actor, selector)
                .map_err(|e| e.to_string())?;
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::AiCombo.declaration(), |host, _, _| {
            Ok(NativeResult::Values(
                host.battle
                    .companion_combo(host.sequence.actor)
                    .map_err(|e| e.to_string())?,
            ))
        })
        .register_typed(Native::AiChain.declaration(), |host, args, _| {
            if host.sequence.definition.phase != crate::ActionPhase::Decision {
                return Err("chain selection needs an authored decision".into());
            }
            let action = u16::try_from(args[0]).map_err(|_| "invalid companion chain action")?;
            Ok(NativeResult::Continue(Some(
                host.battle
                    .queue_companion_chain(host.sequence.actor, action)
                    .map_err(|e| e.to_string())?
                    .into(),
            )))
        })
        .register_typed(Native::AiRefreshRetry.declaration(), |host, _, _| {
            Ok(NativeResult::Continue(Some(
                host.battle
                    .refresh_companion_retry(host.sequence.actor)
                    .map_err(|e| e.to_string())?
                    .into(),
            )))
        })
        .register_typed(Native::AiRetryFlags.declaration(), |host, _, _| {
            let control = host.battle.controls[host.sequence.actor.index()]
                .as_ref()
                .ok_or("companion has no control state")?;
            Ok(NativeResult::Continue(Some(control.companion_retry.into())))
        })
        .register_typed(Native::AiActor.declaration(), |host, args, _| {
            let actor = host.actor_id(args[0])?;
            Ok(NativeResult::Values(
                host.battle
                    .companion_actor(actor)
                    .map_err(|e| e.to_string())?,
            ))
        })
        .register_typed(Native::AiIdleTimer.declaration(), |host, _, _| {
            Ok(NativeResult::Continue(Some(i32::from(
                host.battle.idle_timers[host.sequence.actor.index()],
            ))))
        })
        .register_typed(Native::AiSetIdleTimer.declaration(), |host, args, _| {
            let value = i16::try_from(args[0]).map_err(|_| "invalid decision idle clock")?;
            host.battle.idle_timers[host.sequence.actor.index()] = value;
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::AiResetIdle.declaration(), |host, _, _| {
            host.battle
                .initialize_idle(host.sequence.actor)
                .map_err(|e| e.to_string())?;
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::AiBodyGap.declaration(), |host, _, _| {
            let distance = crate::control::body_gap(
                &host.battle.actors[host.sequence.actor.index()],
                &host.battle.actors[host.sequence.target.index()],
            );
            Ok(NativeResult::Continue(Some(distance.to_bits() as i32)))
        })
        .register_typed(Native::AiFaceTarget.declaration(), |host, args, _| {
            let step = f32::from_bits(args[0] as u32);
            if !step.is_finite() || step < 0. {
                return Err("invalid decision turn step".into());
            }
            let owner = &mut host.battle.actors[host.sequence.actor.index()];
            // 32298's idle timeout copies18FC, sampled by31C88 from the
            // retained body centers before this actor's center refresh.
            let direction = owner.movement.target_direction;
            owner.facing_direction = direction;
            Ok(NativeResult::Continue(Some(i32::from(
                crate::control::face_cached(owner, direction, step),
            ))))
        })
        .register_typed(Native::AtAge.declaration(), |host, args, _| {
            host.wait_until(args[0] as u32)
        })
        .register_typed(Native::AtCommandAge.declaration(), |host, args, _| {
            let age = i16::try_from(args[0]).map_err(|_| "invalid command age")?;
            if host.sequence.definition.phase != crate::ActionPhase::Actor {
                return Err("command age needs an ordinary actor sequence".into());
            }
            if host.sequence.command_age >= age {
                return Ok(NativeResult::Continue(None));
            }
            *host.wait = Some(Wait::CommandAge(age));
            Ok(NativeResult::Suspend)
        })
        .register_typed(Native::ActionEnd.declaration(), |host, _, _| {
            if host.sequence.definition.phase != crate::ActionPhase::Actor {
                return Err("action end needs an ordinary actor sequence".into());
            }
            *host.wait = Some(Wait::ActionEnd(u32::from(
                host.sequence.definition.duration,
            )));
            Ok(NativeResult::Suspend)
        })
        .register_typed(Native::ActionEndAt.declaration(), |host, args, _| {
            if host.sequence.definition.phase != crate::ActionPhase::Actor {
                return Err("action end needs an ordinary actor sequence".into());
            }
            let age = u32::try_from(args[0]).map_err(|_| "invalid action end age")?;
            *host.wait = Some(Wait::ActionEnd(age));
            Ok(NativeResult::Suspend)
        })
        .register_typed(Native::AtHitAge.declaration(), |host, args, _| {
            let age = i16::try_from(args[0]).map_err(|_| "invalid hit age")?;
            if host.sequence.definition.phase != crate::ActionPhase::Actor {
                return Err("hit age needs an ordinary actor sequence".into());
            }
            if host.sequence.hit_age >= age {
                host.sequence.hit_row = true;
                return Ok(NativeResult::Continue(None));
            }
            host.sequence.hit_waiting = true;
            *host.wait = Some(Wait::HitAge(age));
            Ok(NativeResult::Suspend)
        })
        .register_typed(Native::AtAnimationAge.declaration(), |host, args, _| {
            let age = i16::try_from(args[0]).map_err(|_| "invalid animation age")?;
            if host.sequence.definition.phase != crate::ActionPhase::Actor
                || host.sequence.animation_ended
            {
                return Err("animation age needs an active ordinary animation stream".into());
            }
            if host.sequence.animation_age >= age {
                return Ok(NativeResult::Continue(None));
            }
            *host.wait = Some(Wait::AnimationAge(age));
            Ok(NativeResult::Suspend)
        })
        .register_typed(Native::EndAnimation.declaration(), |host, _, _| {
            if host.sequence.definition.phase != crate::ActionPhase::Actor {
                return Err("animation end needs an ordinary actor sequence".into());
            }
            host.sequence.animation_ended = true;
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::WaitTicks.declaration(), |host, args, _| {
            let age = host
                .sequence
                .age
                .checked_add(args[0] as u32)
                .ok_or("battle wait age overflow")?;
            host.wait_until(age)
        })
        .register_typed(Native::MobilityState.declaration(), |host, _, _| {
            Ok(NativeResult::Values(
                host.battle.mobility_state(host.sequence.actor),
            ))
        })
        .register_typed(Native::MobilityBegin.declaration(), |host, _, _| {
            host.battle
                .begin_mobility(host.sequence.actor)
                .map_err(|error| error.to_string())?;
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::MobilityCount.declaration(), |host, args, _| {
            let count =
                i16::try_from(args[0]).map_err(|_| "mobility count exceeds signed range")?;
            host.battle.actors[host.sequence.actor.index()]
                .reaction
                .remaining = count;
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::MobilityLand.declaration(), |host, _, _| {
            host.battle
                .land_mobility(host.sequence.actor)
                .map_err(|error| error.to_string())?;
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::MobilityFinish.declaration(), |host, args, _| {
            host.battle
                .finish_mobility(host.sequence.actor, args[0] != 0)
                .map_err(|error| error.to_string())?;
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::MobilityIntegrate.declaration(), |host, args, _| {
            let stopped = host
                .battle
                .integrate_mobility(host.sequence.actor, args[0] != 0)
                .map_err(|error| error.to_string())?;
            Ok(NativeResult::Continue(Some(i32::from(stopped))))
        })
        .register_typed(Native::MobilityFace.declaration(), |host, _, _| {
            host.battle
                .face_mobility(host.sequence.actor)
                .map_err(|error| error.to_string())?;
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::NextUpdate.declaration(), |host, _, _| {
            *host.wait = Some(Wait::NextUpdate);
            Ok(NativeResult::Suspend)
        })
        .register_typed(Native::Owner.declaration(), |host, _, _| {
            Ok(NativeResult::Continue(Some(i32::from(
                host.sequence.actor.0,
            ))))
        })
        .register_typed(Native::Target.declaration(), |host, _, _| {
            Ok(NativeResult::Continue(Some(i32::from(
                host.sequence.target.0,
            ))))
        })
        .register_typed(Native::RetargetUnavailable.declaration(), |host, _, _| {
            // 37034's ordinary selector: keep an available target, otherwise
            // take the first available member of its side. No candidate keeps
            // the original target; selection consumes no randomness.
            let target = &host.battle.actors[host.sequence.target.index()];
            if (target.hp <= 0 || target.petrified)
                && let Some(index) =
                    host.battle.actors.iter().position(|actor| {
                        actor.side == target.side && actor.hp > 0 && !actor.petrified
                    })
            {
                host.sequence.target = ActorId(index as u8);
            }
            Ok(NativeResult::Continue(Some(i32::from(
                host.sequence.target.0,
            ))))
        })
        .register_typed(Native::AllyCount.declaration(), |host, _, _| {
            Ok(NativeResult::Continue(Some(host.allies().count() as i32)))
        })
        .register_typed(Native::Ally.declaration(), |host, args, _| {
            let actor = usize::try_from(args[0])
                .ok()
                .and_then(|index| host.allies().nth(index))
                .ok_or("invalid battle roster index")?;
            Ok(NativeResult::Continue(Some(i32::from(actor.0))))
        })
        .register_typed(Native::Eligible.declaration(), |host, args, _| {
            let id = host.actor_id(args[0])?;
            Ok(NativeResult::Continue(Some(i32::from(
                host.battle.actors[id.index()].hp > 0 && !host.battle.actors[id.index()].petrified,
            ))))
        })
        .register_typed(Native::ActorAvailable.declaration(), |host, args, _| {
            let actor = host.actor_id(args[0])?;
            Ok(NativeResult::Continue(Some(i32::from(
                host.battle.actors[actor.index()].available(),
            ))))
        })
        .register_typed(Native::EntryVoiceParameters.declaration(), |host, _, _| {
            let voice = host
                .battle
                .prepared
                .entry_voice
                .ok_or("entry voice is not prepared")?;
            Ok(NativeResult::Values(vec![
                i32::from(voice.repeated_formation),
                i32::from(voice.major_enemy),
                i32::from(voice.enemy_count),
                i32::from(voice.level_difference),
            ]))
        })
        .register_typed(Native::RevivePercent.declaration(), |host, args, _| {
            let id = host.actor_id(args[0])?;
            let percent = i16::try_from(args[1]).map_err(|_| "invalid revival percent")?;
            host.battle
                .revive_percent(id, percent, host.cues)
                .map_err(|e| e.to_string())?;
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::HealPercent.declaration(), |host, args, _| {
            let id = host.actor_id(args[0])?;
            let percent = i16::try_from(args[1])
                .ok()
                .filter(|v| (0..=100).contains(v))
                .ok_or("recovery percent must be in 0..100")?;
            host.battle
                .recover(id, percent, host.cues)
                .map_err(|e| e.to_string())?;
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::TintStage.declaration(), |host, args, _| {
            let index = usize::try_from(args[0])
                .ok()
                .filter(|v| *v < 2)
                .ok_or("stage color channel must be 0 or 1")?;
            let mut color = [0; 4];
            for (out, &value) in color.iter_mut().zip(&args[1..5]) {
                *out = u8::try_from(value).map_err(|_| "stage color must be in 0..255")?;
            }
            let duration = u16::try_from(args[5])
                .ok()
                .filter(|v| *v <= i16::MAX as u16)
                .ok_or("invalid stage color duration")?;
            let step = u8::try_from(args[6]).map_err(|_| "stage color step must be in 0..255")?;
            host.battle
                .stage_colors
                .request(index, color, duration, step);
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::Voice.declaration(), |host, args, _| {
            let actor = host.actor_id(args[0])?;
            let priority = u8::try_from(args[2])
                .ok()
                .filter(|v| *v < 16)
                .ok_or("voice priority must be in 0..15")?;
            let accepted = host.voice_line(actor, args[1])?.is_some_and(|line| {
                host.battle.voices[actor.index()].request(line.sound, priority)
            });
            Ok(NativeResult::Continue(Some(i32::from(accepted))))
        })
        .register_typed(Native::CenterVoice.declaration(), |host, args, _| {
            let actor = host.actor_id(args[0])?;
            host.battle.voices[actor.index()].centered = true;
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::VoiceDuration.declaration(), |host, args, _| {
            let actor = host.actor_id(args[0])?;
            let duration = host
                .voice_line(actor, args[1])?
                .map_or(0, |line| line.duration);
            Ok(NativeResult::Continue(Some(i32::from(duration))))
        })
        .register_typed(Native::VoiceIdle.declaration(), |host, args, _| {
            let actor = host.actor_id(args[0])?;
            let idle = host.battle.voices[actor.index()].playing.is_none();
            Ok(NativeResult::Continue(Some(i32::from(idle))))
        })
        .register_typed(Native::CameraBounds.declaration(), |host, args, _| {
            let duration = u16::try_from(args[0])
                .ok()
                .filter(|v| *v <= i16::MAX as u16)
                .ok_or("invalid camera constraint duration")?;
            let radius = f32::from_bits(args[1] as u32);
            let pitch = f32::from_bits(args[2] as u32);
            if !radius.is_finite() || radius < 0. || !pitch.is_finite() {
                return Err("invalid camera constraint".into());
            }
            host.battle
                .camera
                .as_mut()
                .ok_or("battle camera is not prepared")?
                .constrain(duration, radius, pitch);
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::TintActor.declaration(), |host, args, _| {
            let actor = host.actor_id(args[0])?;
            let Some(ResourceBinding::ActorTints(colors)) = usize::try_from(args[1])
                .ok()
                .and_then(|i| host.sequence.definition.resources.get(i))
            else {
                return Err("unbound actor tint table".into());
            };
            let color = usize::try_from(args[2])
                .ok()
                .and_then(|i| colors.get(i))
                .ok_or("actor tint index must be in 0..12")?;
            host.battle.actors[actor.index()].body.tint = *color;
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::Show.declaration(), |host, args, _| {
            host.show(
                args,
                EffectOrigin::Fixed,
                1.,
                false,
                Default::default(),
                host.sequence.actor,
            )
        })
        .register_typed(Native::ShowOn.declaration(), |host, args, _| {
            let actor = host.actor_id(args[2])?;
            let scale = host.battle.actors[actor.index()].effect_scale;
            host.show(
                args,
                EffectOrigin::Center,
                scale,
                false,
                Default::default(),
                actor,
            )
        })
        .register_typed(Native::WeaponVisible.declaration(), |host, args, _| {
            if !host.sequence.definition.phase.is_actor() {
                return Err("weapon visibility requires an actor sequence".into());
            }
            let slot = u8::try_from(args[0]).map_err(|_| "invalid weapon slot")?;
            host.battle.models[host.sequence.actor.index()]
                .as_mut()
                .ok_or("weapon visibility requires a prepared model")?
                .weapon_visible(slot, args[1] != 0)
                .map_err(|e| e.to_string())?;
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::WeaponTrail.declaration(), |host, args, _| {
            if !host.sequence.definition.phase.is_actor() {
                return Err("weapon trail requires an actor action".into());
            }
            let slot = usize::try_from(args[0]).map_err(|_| "invalid weapon trail slot")?;
            let timer = u16::try_from(args[1]).map_err(|_| "invalid weapon trail timer")?;
            let value = host.battle.trail_timers[host.sequence.actor.index()]
                .get_mut(slot)
                .ok_or("invalid weapon trail slot")?;
            *value = timer as u8;
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::AttachEffect.declaration(), |host, args, _| {
            if host.sequence.definition.phase != crate::ActionPhase::Actor {
                return Err("attached effect requires an actor action".into());
            }
            let Some(ResourceBinding::Effect(resource)) = usize::try_from(args[0])
                .ok()
                .and_then(|i| host.sequence.definition.resources.get(i))
            else {
                return Err("unbound battle effect".into());
            };
            let appearance = crate::EffectAppearance {
                resource: *resource,
                member: u16::try_from(args[1]).map_err(|_| "invalid battle effect member")?,
            };
            host.battle
                .attach_effect(host.id, host.sequence, appearance, host.cues)
                .map_err(|e| e.to_string())?;
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::ShowFollowing.declaration(), |host, args, _| {
            host.show_following(args, EffectOrigin::Root)
        })
        .register_typed(Native::ShowCentered.declaration(), |host, args, _| {
            host.show_following(args, EffectOrigin::Center)
        })
        .register_typed(Native::Finish.declaration(), |host, _, _| {
            host.sequence.finished = true;
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::RetainResident.declaration(), |host, _, _| {
            let resident = host
                .sequence
                .resident
                .as_mut()
                .filter(|resident| resident.phase == ResidentPhase::Initializing)
                .ok_or("retain_resident requires resident initialization")?;
            resident.retained = true;
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::GroundPoint.declaration(), |host, args, _| {
            let target = host.actor_id(args[0])?;
            let height = f32::from_bits(args[1] as u32);
            let nudge = f32::from_bits(args[2] as u32);
            // fn_1_37ED0 / fn_1_4DA50: flatten the target, then nudge toward
            // the owner only when the horizontal separation reaches 0.5.
            let owner = &host.battle.actors[host.sequence.actor.index()];
            let target = &host.battle.actors[target.index()];
            let mut point = [target.position[0], height, target.position[2]];
            let dx = owner.position[0] - target.position[0];
            let dz = owner.position[2] - target.position[2];
            let length = (dx * dx + dz * dz).sqrt();
            if length >= 0.5 {
                point[0] += dx / length * nudge;
                point[2] += dz / length * nudge;
            }
            Ok(NativeResult::Values(
                point.map(|v| v.to_bits() as i32).to_vec(),
            ))
        })
        .register_typed(Native::HitWindow.declaration(), |host, args, _| {
            let action = &host.sequence.definition;
            if action.phase != crate::ActionPhase::Actor
                || host.sequence.melee.is_some()
                || host.sequence.weapon_launch.is_some()
            {
                return Err("hit window needs an actor sequence with no pending window".into());
            }
            let Some(ResourceBinding::Melee(definition)) = usize::try_from(args[0])
                .ok()
                .and_then(|i| action.resources.get(i))
            else {
                return Err("unbound melee contact".into());
            };
            let start = i16::try_from(args[1]).map_err(|_| "invalid hit window start")?;
            let end = args[1]
                .checked_add(args[2])
                .and_then(|v| i16::try_from(v).ok())
                .ok_or("invalid hit window end")?;
            let window = crate::melee::Window {
                task: host.handle,
                definition: definition.clone(),
                start,
                end,
            };
            window
                .validate_anchors(&host.battle.actors[host.sequence.actor.index()])
                .map_err(|e| e.to_string())?;
            host.sequence.melee = Some(window);
            *host.wait = Some(Wait::Melee);
            Ok(NativeResult::Suspend)
        })
        .register_typed(Native::ThrowWeapon.declaration(), |host, args, _| {
            let action = &host.sequence.definition;
            if action.phase != crate::ActionPhase::Actor
                || host.sequence.melee.is_some()
                || host.sequence.weapon_launch.is_some()
            {
                return Err("weapon launch needs an actor sequence with no pending hit row".into());
            }
            let Some(ResourceBinding::WeaponFlight(definition)) = usize::try_from(args[0])
                .ok()
                .and_then(|i| action.resources.get(i))
            else {
                return Err("unbound weapon flight".into());
            };
            let start = i16::try_from(args[1])
                .ok()
                .filter(|start| *start >= 0)
                .ok_or("invalid weapon launch start")?;
            host.battle.models[host.sequence.actor.index()]
                .as_ref()
                .ok_or("weapon launch requires actor model")?
                .weapon_attachment(definition.slot)
                .map_err(|error| error.to_string())?;
            host.sequence.weapon_launch = Some(crate::weapon_flight::Launch {
                task: host.handle,
                definition: definition.clone(),
                start,
            });
            *host.wait = Some(Wait::WeaponLaunch);
            Ok(NativeResult::Suspend)
        })
        .register_typed(Native::Animate.declaration(), |host, args, _| {
            let blending = host.play_motion(args)?;
            if host.sequence.definition.phase == crate::ActionPhase::Actor {
                // 2BC3C binds the initial row before ordinary action visits.
                // Later 2B910 rows follow commands/hits on an admitted visit.
                if host.sequence.animation_started {
                    host.sequence.animation_row = true;
                    // A declared blend of1 also holds the animation counter,
                    // even though the model has no multi-update blend to wait.
                    host.sequence.animation_held = args[1] != 0;
                }
                host.sequence.animation_started = true;
                host.sequence.animation_ended = false;
            }
            if blending {
                *host.wait = Some(Wait::AnimationReady);
                Ok(NativeResult::Suspend)
            } else {
                Ok(NativeResult::Continue(None))
            }
        })
        .register_typed(Native::PlayMotion.declaration(), |host, args, _| {
            host.play_motion(args)?;
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::TryPlayMotion.declaration(), |host, args, _| {
            if !host.sequence.definition.phase.is_actor()
                && host.sequence.definition.phase != crate::ActionPhase::Decision
            {
                return Err("motion needs an actor sequence".into());
            }
            let Some(ResourceBinding::OptionalMotion(binding)) = usize::try_from(args[0])
                .ok()
                .and_then(|i| host.sequence.definition.resources.get(i))
            else {
                return Err("unbound optional battle motion".into());
            };
            if let Some(binding) = binding[host.sequence.actor.index()] {
                let model = host.battle.models[host.sequence.actor.index()]
                    .as_mut()
                    .ok_or("optional motion needs an actor model")?;
                if !model.is_playing(binding).map_err(|e| e.to_string())? {
                    let blend =
                        u8::try_from(args[1]).map_err(|_| "animation blend exceeds 255 updates")?;
                    model
                        .play(
                            binding,
                            f32::from_bits(args[2] as u32),
                            f32::from_bits(args[3] as u32),
                            args[4] != 0,
                            blend,
                        )
                        .map_err(|e| e.to_string())?;
                }
            }
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::MotionIs.declaration(), |host, args, _| {
            let (model, binding) = host.bound_motion(args[0])?;
            Ok(NativeResult::Continue(Some(i32::from(
                model.is_playing(binding).map_err(|e| e.to_string())?,
            ))))
        })
        .register_typed(Native::MotionDuration.declaration(), |host, args, _| {
            let (model, binding) = host.bound_motion(args[0])?;
            Ok(NativeResult::Continue(Some(
                model
                    .duration(binding)
                    .map_err(|e| e.to_string())?
                    .to_bits() as i32,
            )))
        })
        .register_typed(Native::AutomaticControl.declaration(), |host, _, _| {
            Ok(NativeResult::Continue(Some(i32::from(matches!(
                host.battle.actors[host.sequence.actor.index()].control,
                crate::Control::Auto | crate::Control::Enemy
            )))))
        })
        .register_typed(Native::AnimationEnd.declaration(), |host, _, _| {
            let action = &host.sequence.definition;
            if !action.phase.is_actor() {
                return Err("animation wait needs an actor sequence".into());
            }
            let model = host.battle.models[host.sequence.actor.index()]
                .as_ref()
                .ok_or("actor has no prepared model")?;
            if model.finished() {
                Ok(NativeResult::Continue(None))
            } else {
                *host.wait = Some(Wait::AnimationEnd);
                Ok(NativeResult::Suspend)
            }
        })
        .register_typed(Native::ForwardSpeed.declaration(), |host, args, _| {
            let (motion, value) = host.motion(args[0])?;
            if args[1] == 0 || motion.forward < value {
                motion.forward = value;
            }
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::VerticalSpeed.declaration(), |host, args, _| {
            let (motion, value) = host.motion(args[0])?;
            motion.vertical = value;
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::Acceleration.declaration(), |host, args, _| {
            let (motion, value) = host.motion(args[0])?;
            motion.acceleration = value;
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::Gravity.declaration(), |host, args, _| {
            let (motion, value) = host.motion(args[0])?;
            motion.gravity = value;
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::Braking.declaration(), |host, args, _| {
            let (motion, value) = host.motion(args[0])?;
            if value < 0. {
                return Err("negative battle braking".into());
            }
            motion.braking = value;
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::ActionGuard.declaration(), |host, args, _| {
            if !host.sequence.definition.phase.is_actor() {
                return Err("action guard needs an actor sequence".into());
            }
            let chance = i8::try_from(args[0]).map_err(|_| "invalid action guard chance")?;
            let window = [args[1], args[2]].map(i16::try_from);
            let window = [
                window[0].map_err(|_| "invalid action guard window")?,
                window[1].map_err(|_| "invalid action guard window")?,
            ];
            let actor = &mut host.battle.actors[host.sequence.actor.index()];
            match &mut actor.activity {
                crate::Activity::Action { guard_window, .. }
                | crate::Activity::Casting { guard_window, .. } => *guard_window = window,
                _ => return Err("action guard needs an active action".into()),
            }
            actor.guard.enemy_chance = chance;
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::Armor.declaration(), |host, args, _| {
            if !host.sequence.definition.phase.is_actor() {
                return Err("armor needs an actor sequence".into());
            }
            let threshold = u8::try_from(args[0]).map_err(|_| "invalid battle armor threshold")?;
            let armor = &mut host.battle.actors[host.sequence.actor.index()]
                .reaction
                .armor;
            armor.threshold = threshold;
            armor.received = 0;
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::Recover.declaration(), |host, args, _| {
            if host.battle.transition_owner() == Some(host.sequence.actor) {
                return Err("activate the scene before entering recovery".into());
            }
            let remaining = i16::try_from(args[0])
                .ok()
                .filter(|v| *v >= 0)
                .ok_or("invalid battle recovery duration")?;
            let (motion, _) = host.motion(0)?;
            motion.gravity = if motion.flying { 0. } else { -1. };
            host.sequence.cancel_children(host.handle);
            host.sequence.melee = None;
            host.sequence.recovery = Some(remaining);
            host.sequence.action_recovery = true;
            let actor = &mut host.battle.actors[host.sequence.actor.index()];
            actor.activity = crate::Activity::Recovering;
            // 385A0 keeps state 22 throughout 301A4 recovery. The ordinary
            // reset/interruption paths clear it with the other HUD targets.
            // 3DA00 clears enemy action armor before its recovery motion.
            if actor.side == crate::Side::Enemy {
                actor.reaction.armor.threshold = 0;
                actor.reaction.armor.received = 0;
            }
            *host.wait = Some(Wait::Recovery);
            Ok(NativeResult::Suspend)
        })
        .register_typed(Native::Release.declaration(), |host, args, _| {
            let action = &host.sequence.definition;
            if !action.phase.is_actor() {
                return Err("spell release needs an actor sequence".into());
            }
            let spell = host.spell(args[0])?;
            let slot = if args[1] != 0 {
                crate::SpellSlot::Secondary
            } else {
                crate::SpellSlot::Primary
            };
            let released = host
                .battle
                .release(
                    spell,
                    host.sequence.actor,
                    host.sequence.target,
                    slot,
                    host.id,
                    host.cues,
                )
                .map_err(|e| e.to_string())?;
            Ok(NativeResult::Continue(Some(i32::from(released.is_some()))))
        })
        .register_typed(Native::SceneAvailable.declaration(), |host, _, _| {
            Ok(NativeResult::Continue(Some(i32::from(
                host.battle.scene_available(),
            ))))
        })
        .register_typed(Native::BeginScene.declaration(), |host, args, _| {
            if host.sequence.definition.phase != crate::ActionPhase::Casting {
                return Err("scene transition needs a casting sequence".into());
            }
            let spell = host.spell(args[0])?;
            let duration =
                u16::try_from(args[1]).map_err(|_| "invalid scene transition duration")?;
            host.battle
                .begin_scene(host.id, host.sequence.actor, spell, duration)
                .map_err(|e| e.to_string())?;
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::SceneRemaining.declaration(), |host, _, _| {
            let remaining = host
                .battle
                .scene_remaining(host.id)
                .map_err(|e| e.to_string())?;
            Ok(NativeResult::Continue(Some(i32::from(remaining))))
        })
        .register_typed(Native::ActivateScene.declaration(), |host, _, _| {
            host.battle
                .activate_scene(host.id, host.sequence.target, host.cues)
                .map_err(|e| e.to_string())?;
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::HideActors.declaration(), |host, _, _| {
            host.battle
                .hide_actors(host.id)
                .map_err(|e| e.to_string())?;
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::SpellActive.declaration(), |host, args, _| {
            let slot = if args[0] != 0 {
                crate::SpellSlot::Secondary
            } else {
                crate::SpellSlot::Primary
            };
            let occupied = host
                .sequence
                .resident
                .as_ref()
                .is_some_and(|r| r.slot == slot)
                || host.battle.spell_active(host.sequence.actor, slot);
            Ok(NativeResult::Continue(Some(i32::from(occupied))))
        })
        .register_typed(Native::TpCost.declaration(), |host, _, _| {
            Ok(NativeResult::Continue(Some(i32::from(
                host.sequence.definition.tp_cost,
            ))))
        })
        .register_typed(Native::ComboIndex.declaration(), |host, _, _| {
            let combo = host.battle.controls[host.sequence.actor.index()]
                .as_ref()
                .map_or(0, |control| control.combo);
            Ok(NativeResult::Continue(Some(i32::from(combo))))
        })
        .register_typed(Native::CastRemaining.declaration(), |host, _, _| {
            Ok(NativeResult::Continue(Some(i32::from(
                *host.cast_remaining()?,
            ))))
        })
        .register_typed(Native::CastParameters.declaration(), |host, args, _| {
            let cast = host.casting(args[0])?;
            let mut values = vec![
                i32::from(cast.base),
                i32::from(cast.extra),
                i32::from(cast.recovery),
            ];
            values.extend(cast.release.values());
            values.extend([
                cast.chant.len() as i32,
                i32::from(cast.pulse_member),
                cast.effect_scale.to_bits() as i32,
                i32::from(cast.tint.enabled),
                i32::from(cast.tint.palette),
                i32::from(cast.tint.rgb[0]),
                i32::from(cast.tint.rgb[1]),
                i32::from(cast.tint.rgb[2]),
            ]);
            Ok(NativeResult::Values(values))
        })
        .register_typed(Native::ChantStep.declaration(), |host, args, _| {
            let cast = host.casting(args[0])?;
            let step = usize::try_from(args[1])
                .ok()
                .and_then(|i| cast.chant.get(i))
                .ok_or("invalid casting motion row")?;
            Ok(NativeResult::Values(step.values().to_vec()))
        })
        .register_typed(Native::SetCastRemaining.declaration(), |host, args, _| {
            let value = i16::try_from(args[0]).map_err(|_| "casting clock exceeds signed range")?;
            *host.cast_remaining()? = value;
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::AnimationFinished.declaration(), |host, _, _| {
            if !host.sequence.definition.phase.is_actor()
                && host.sequence.definition.phase != crate::ActionPhase::Decision
            {
                return Err("animation query needs an actor sequence".into());
            }
            let model = host.battle.models[host.sequence.actor.index()]
                .as_ref()
                .ok_or("actor has no prepared model")?;
            Ok(NativeResult::Continue(Some(i32::from(model.finished()))))
        })
        .register_typed(Native::IdlePose.declaration(), |host, args, _| {
            if !host.sequence.definition.phase.is_actor()
                && host.sequence.definition.phase != crate::ActionPhase::Decision
            {
                return Err("idle pose needs an actor sequence".into());
            }
            let blend = u8::try_from(args[0]).map_err(|_| "idle blend exceeds byte range")?;
            host.battle
                .play_idle_pose(host.sequence.actor, blend)
                .map_err(|e| e.to_string())?;
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::TextureLayers.declaration(), |host, args, _| {
            if !host.sequence.definition.phase.is_actor() {
                return Err("texture layers need an actor sequence".into());
            }
            let mut layers = [0; 4];
            for (layer, &value) in layers.iter_mut().zip(args) {
                *layer = u8::try_from(value).map_err(|_| "texture layer exceeds byte range")?;
            }
            host.battle.models[host.sequence.actor.index()]
                .as_mut()
                .ok_or("texture layers need an actor model")?
                .shown
                .texture_layers = layers;
            host.battle.eye_expressions[host.sequence.actor.index()] = layers[0];
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::BeginCastRelease.declaration(), |host, _, _| {
            host.cast_remaining()?;
            host.battle.actors[host.sequence.actor.index()]
                .hud
                .cast_released = true;
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::JitterBody.declaration(), |host, args, _| {
            if !host.sequence.definition.phase.is_actor() {
                return Err("body jitter needs an actor sequence".into());
            }
            let duration = i16::try_from(args[0])
                .ok()
                .filter(|duration| *duration >= 0)
                .ok_or("invalid body jitter duration")?;
            host.battle.actors[host.sequence.actor.index()]
                .body
                .jitter
                .request(duration);
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::Notice.declaration(), |host, args, _| {
            if !host.sequence.definition.phase.is_actor() {
                return Err("battle notice needs an actor sequence".into());
            }
            let duration = u16::try_from(args[0])
                .ok()
                .filter(|value| *value <= i16::MAX as u16)
                .ok_or("invalid battle notice duration")?;
            let kind = u8::try_from(args[1])
                .ok()
                .filter(|value| *value < 4)
                .ok_or("invalid battle notice kind")?;
            host.cues.push(Cue::Notice {
                actor: host.sequence.actor,
                action: host.sequence.definition.id,
                duration,
                kind,
            });
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::PayTp.declaration(), |host, args, _| {
            if !host.sequence.definition.phase.is_actor() {
                return Err("TP payment needs an actor sequence".into());
            }
            let amount = u16::try_from(args[0])
                .ok()
                .filter(|v| *v <= i16::MAX as u16)
                .ok_or("invalid battle TP cost")?;
            if host.sequence.cost_committed {
                return Err("battle action TP already committed".into());
            }
            host.sequence.cost_committed = true;
            let actor = &mut host.battle.actors[host.sequence.actor.index()];
            let paid = actor.tp >= amount;
            if paid {
                actor.tp -= amount;
            }
            Ok(NativeResult::Continue(Some(i32::from(paid))))
        })
        .register_typed(Native::Emit.declaration(), |host, args, _| {
            host.emit(args, None)
        })
        .register_typed(Native::EmitWithVelocity.declaration(), |host, args, _| {
            host.emit(
                args,
                Some(std::array::from_fn(|i| f32::from_bits(args[i + 4] as u32))),
            )
        });

    fn spawn(&mut self, function: u16, arguments: &[i32]) -> Result<i32, String> {
        if self.sequence.tasks.len() + 1 >= TASK_LIMIT {
            return Err("battle task limit exceeded (64)".into());
        }
        let program = &self.sequence.definition.program;
        let function = program
            .authored()
            .and_then(|m| m.functions.get(usize::from(function)))
            .filter(|f| f.is_task)
            .ok_or("spawn target is not a task")?;
        let vm = Vm::with_arguments(program.clone(), function.entry, arguments)
            .map_err(|e| e.to_string())?;
        let handle = self.sequence.next_task;
        let next = handle
            .checked_add(1)
            .ok_or("battle task handle exhausted")?;
        self.sequence.ownership.register(self.handle, handle)?;
        self.sequence.next_task = next;
        self.sequence.tasks.insert(handle, Task { vm, wait: None });
        Ok(handle)
    }
    fn join(&mut self, handle: i32) -> Result<Option<Vec<i32>>, String> {
        self.sequence.ownership.join(self.handle, handle)
    }
}

pub fn native_declarations() -> Vec<NativeDeclaration> {
    BattleHost::AUTHORED_NATIVES.declarations().collect()
}
