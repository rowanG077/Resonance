//! General native-function shims and event scheduling for SymphoniaScript.
//! No Bevy, title sequence, disc format, or GameCube address-space dependency.
mod ambient;
pub mod animation;
mod attachments;
pub mod authored;
mod autonomy;
mod emitter;
mod enemy_source;
pub use ambient::AmbientSound;
pub mod battle;
pub use autonomy::{Activity, Autonomy, Behavior};
mod face;
pub use face::EyeBlink;
pub mod field_damage;
mod field_exit;
mod native;
mod resources;
mod scheduler;
mod trigger;
mod wings;
pub use wings::COLETTE_WINGS_ACTOR;
mod world;
pub use animation::Animation;
pub use resources::{
    AnimationClip, AttachmentPose, MemoryCircleText, ModelAttachments, ModelResource, ParticleKind,
    ResourceKind, ResourceLibrary,
};
pub use scheduler::EventRuntime;
pub use world::{
    ACTOR_CONTACT_HEIGHT, Actor, ActorContact, ActorMotion, ActorRole, Appearance, Attachment,
    AudioCommand, BoneAdjustment, BoneScale, BoneTarget, CameraTrack, Emote, Enemy, EventRecord,
    Face, Fade, FieldTransition, GameWorld, MusicCommand, Overlay, OverlayKind, Particle,
    PlayerSize, SavePoint, SceneDestination, SpriteOverlay, TreasureChest, TreasureKind,
    TreasureReward, Trigger, TriggerShape, VoicePlayback, WorldTransition,
};
mod operation;
pub use operation::{Operation, Outcome, Progress};
pub mod camera;
pub mod caption;
pub mod collision;
pub mod dialogue;
pub mod effect;
mod gameplay_random;
pub mod input;
pub mod model_particle;
pub mod party;
mod persistent;
pub mod projectile;
pub mod ring;
pub mod rumble;
pub use gameplay_random::GameplayRandom;
pub mod menu;
pub mod skit;
pub use persistent::{PersistentState, SavedProgress, script_global};

/// Script operand meaning “the currently controlled party member”.
pub const CONTROLLED_ACTOR: i32 = 999_999;
mod field_party;
