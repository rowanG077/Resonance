//! General native-function shims and event scheduling for SymphoniaScript.
//! No Bevy, title sequence, disc format, or GameCube address-space dependency.
pub mod animation;
mod native;
mod resources;
mod scheduler;
mod world;
pub use animation::Animation;
pub use resources::{AnimationClip, AttachmentTrack, ModelResource, ResourceKind, ResourceLibrary};
pub use scheduler::EventRuntime;
pub use world::{
    Actor, ActorMotion, Appearance, Attachment, AudioCommand, BoneAdjustment, CameraTrack, Emote,
    EventRecord, Face, Fade, FieldTransition, GameWorld, Overlay, Particle, Trigger, VoicePlayback,
};
mod operation;
pub use operation::{Operation, Outcome, Progress};
pub mod camera;
pub mod dialogue;
pub mod effect;
pub mod party;
mod persistent;
pub use persistent::PersistentState;

/// Script operand meaning “the currently controlled party member”.
pub(crate) const CONTROLLED_ACTOR: i32 = 999_999;
