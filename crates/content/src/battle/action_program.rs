//! Source command requirements. Recovery is not a claim of runtime support.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandProgram {
    pub commands: Vec<CommandUse>,
    pub loops: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandUse {
    pub tick: u16,
    pub kind: CommandKind,
    pub dependencies: Vec<CommandDependency>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(i16)]
pub enum CommandKind {
    WaitActionResult = -5,
    WaitHit = -3,
    ForwardSpeed = 0,
    VerticalSpeed = 1,
    ForwardAcceleration = 2,
    Gravity = 3,
    Reverse = 4,
    SetActorCollisionMode = 5,
    TextureLayers = 6,
    AttachmentVisibility = 7,
    AdvancePosition = 8,
    ActorMotionScale = 9,
    CameraMotion = 10,
    SetActorProtection = 12,
    AttachmentTrail = 13,
    ApplyConditionAndTransition = 14,
    RecoverHp = 15,
    RecoverTp = 16,
    ExtendAction = 17,
    ReleaseCapturedTarget = 18,
    CapturedTargetVisibility = 19,
    CapturedTargetForwardSpeed = 20,
    CapturedTargetVerticalSpeed = 21,
    CommonImpactFlash = 22,
    SetActorAttackMode = 23,
    DamagePower = 24,
    AttachmentAnimation = 25,
    CommonEffect = 26,
    Voice = 27,
    Sound = 28,
    SetActorAmbientColor = 29,
    Withdraw = 30,
    WithdrawAndRemove = 31,
    ModelTransform = 32,
    Reserved = 33,
    SetActorStateTimer = 34,
    TextureVariant = 35,
    CopyAttachmentAnimation = 36,
    PlayerCameraMotion = 37,
    CastPrimaryTechnique = 38,
    CastSecondaryTechnique = 39,
    TurnHeading = 40,
    TurnMotion = 41,
    RandomVoice = 42,
    ApplyPreviousTargetEvent = 43,
    ApplyTargetEvent = 44,
    SetPosition = 45,
    PositionFromTarget = 46,
    OffsetPosition = 47,
    FaceTargetDirection = 48,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CommandDependency {
    Sound { id: u16 },
    Voice { id: u16, priority: u8 },
    CommonEffect { id: u8 },
    AttachmentAnimation { attachment: i16, animation: i16 },
    CopyAttachmentAnimation { source: i16, destination: i16 },
    NativeTechnique { id: u16, slot: CastSlot },
    TargetEvent { id: i16, target: EventTarget },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CastSlot {
    Primary,
    Secondary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventTarget {
    Previous,
    Selected,
}
