//! General native-function shims and event scheduling for SymphoniaScript.
//! No Bevy, title sequence, disc format, or GameCube address-space dependency.
pub mod animation;
mod autonomy;
pub use autonomy::{Activity, ActorOrigin, Autonomy, Behavior};
mod face;
pub use face::EyeBlink;
mod field_exit;
mod native;
mod resources;
mod scheduler;
mod world;
pub use animation::Animation;
pub use resources::{
    AnimationClip, AttachmentTrack, ModelResource, ParticleKind, ResourceKind, ResourceLibrary,
};
pub use scheduler::{BackgroundWaitOrigin, EventRuntime, ResourceWaitObservation};
pub use world::{
    Actor, ActorCreation, ActorMotion, Appearance, Attachment, AudioCommand, BoneAdjustment,
    CameraTrack, Emote, EventRecord, Face, Fade, FieldTransition, GameWorld, Overlay, OverlayKind,
    Particle, SavePoint, SpriteOverlay, Trigger, TriggerShape, VoicePlayback,
};
mod operation;
pub use operation::{Operation, Outcome, Progress};
pub mod camera;
pub mod dialogue;
pub mod effect;
mod gameplay_random;
pub mod party;
mod persistent;
pub use gameplay_random::GameplayRandom;
pub mod menu;
pub mod skit;
pub use persistent::{PersistentState, SavedProgress};

/// Script operand meaning “the currently controlled party member”.
pub const CONTROLLED_ACTOR: i32 = 999_999;
mod field_party;
