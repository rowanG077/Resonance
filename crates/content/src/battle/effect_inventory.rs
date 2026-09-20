//! Source effect coverage. Discovery does not imply preparation, execution or fidelity.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectInventory {
    pub sources: Vec<EffectInventorySource>,
    pub banks: Vec<EffectBankInventory>,
    pub unresolved: Vec<EffectInventoryIssue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectInventorySource {
    pub path: String,
    pub sha256: String,
    pub kind: EffectArchiveKind,
    /// Indexed entries inspected, including empty entries and packages without effects.
    pub entries: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectArchiveKind {
    Catalog,
    Usual,
    Enemy,
    Magic,
    Skill,
    Arena,
    Party,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EffectSourceBank {
    Common,
    Techniques,
    Enemy {
        monster: u16,
    },
    Magic {
        package: u16,
    },
    Skill {
        package: u16,
    },
    Arena {
        arena: u16,
    },
    Party {
        character: u8,
        variant: u16,
        member: u16,
    },
    Unclassified {
        source: u16,
        member: u16,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EffectBankBinding {
    Fixed { slot: u8 },
    EnemySlot { base: u8 },
    DynamicSlot { base: u8 },
    Unresolved,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectBankInventory {
    pub id: EffectSourceBank,
    pub source: u16,
    /// Offset within the decompressed package, or within the source for uncompressed banks.
    pub offset: u32,
    pub sha256: String,
    pub binding: EffectBankBinding,
    /// A source action can select the first physical package through another zero-offset entry.
    pub alias_of: Option<EffectSourceBank>,
    pub declared_programs: u8,
    pub declared_uv_tracks: u8,
    /// Derived from the halfword actor-region bounds and 352-byte records.
    pub declared_actors: u16,
    pub programs: Vec<EffectProgramInventory>,
    pub actors: Vec<EffectActorInventory>,
    pub modifiers: Vec<EffectModifierInventory>,
    pub uv_tracks: Vec<EffectUvInventory>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectProgramInventory {
    pub id: u16,
    pub offset: u32,
    pub timeline: Vec<EffectTimelineRecord>,
    /// Direct actor references, including repeated commands. Secondary emissions are separate.
    pub actors: Vec<u16>,
    pub terminated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectTimelineRecord {
    pub offset: u32,
    pub tick: i16,
    pub command: EffectTimelineCommand,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EffectTimelineCommand {
    Emit {
        actor: u16,
        attachment: EffectAttachment,
        modifier: Option<u16>,
    },
    Repeat {
        count: u8,
        interval: i16,
        command: Box<EffectTimelineCommand>,
    },
    ModifyRetained {
        slot: u8,
        modifier: u16,
    },
    Sound {
        sound: u16,
        priority: u8,
    },
    End,
    Unresolved {
        opcode: u8,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", content = "index", rename_all = "snake_case")]
pub enum EffectAttachment {
    Emitter,
    Bone(u8),
    BoneGroup(u8),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectActorInventory {
    pub id: u16,
    pub offset: u32,
    pub native_type: u8,
    pub geometry: EffectGeometryRequirement,
    /// Render flags; zero for controller actors that return before allocation.
    pub flags: u32,
    pub requirements: Vec<EffectActorRequirement>,
    pub model: Option<EffectModelReference>,
    pub texture: Option<EffectTextureReference>,
    pub uv_track: Option<u8>,
    pub secondary: Vec<EffectSecondaryReference>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EffectGeometryRequirement {
    Model,
    Ring,
    Quad,
    BillboardRing,
    BillboardTrail,
    RadialQuads,
    Ellipsoid,
    NoDraw,
    TriangleBand,
    JitterRibbon,
    Shake,
    Camera,
    StageColor,
    Caption,
    SphereBands,
    Spiral,
    Disc,
    CurvedShell,
    VertexQuad,
    /// Native controllers and shapes whose complete interpretation is still unresolved.
    Unresolved {
        native_type: u8,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectActorRequirement {
    Bounce,
    BottomAnchor,
    ModelAnimation,
    ColorGradient,
    RepeatedUv,
    GeometryMode,
    Billboard,
    AlignToVelocity,
    DuringPause,
    FollowEmitter,
    BoneGroupCopies,
    FlaredRing,
    GroundRelative,
    #[serde(rename = "owner_layer", alias = "owner_after")]
    OwnerAfter,
    OwnerBefore,
    RetainedActor,
    RetainedModelJoint,
    NoDepthTest,
    DepthWrite,
    CullBack,
    FineTessellation,
    CoarseTessellation,
    CameraRelative,
    DualTexture,
    FollowBone,
    ElementPalette,
    ClampToGround,
    IndependentModelHeading,
    OwnerBoneTransform,
    UnresolvedFlags,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct EffectModelReference {
    /// Authored model slot; 2 and 6 are adjusted by owner/resource slot at runtime.
    pub slot: EffectModelSlot,
    pub index: u8,
    pub animation: Option<u8>,
    pub loop_animation: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", content = "index", rename_all = "snake_case")]
pub enum EffectModelSlot {
    Authored(u8),
    EffectContext,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct EffectTextureReference {
    pub slot: u8,
    pub color_palette: u8,
    pub alpha_palette: Option<u8>,
    pub palette_stride: u8,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct EffectSecondaryReference {
    pub trigger: EffectSecondaryTrigger,
    /// The effect context's resource bank, not a globally fixed bank number.
    pub program: u8,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectSecondaryTrigger {
    GroundContact,
    Periodic,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectModifierInventory {
    pub offset: u16,
    pub operations: Vec<EffectModifierUse>,
    pub terminated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectModifierUse {
    pub offset: u32,
    pub operation: EffectModifierOperation,
    pub destination: EffectModifierDestination,
    pub model: Option<EffectModelReference>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EffectModifierOperation {
    Integer {
        width: EffectIntegerWidth,
        operation: EffectArithmetic,
    },
    Float {
        operation: EffectArithmetic,
    },
    RandomInteger,
    RandomFloat,
    SetVector,
    RandomPolarVector,
    RotateVector,
    PolarVector,
    SetColor,
    ClearFlags,
    SetFlags,
    TranslateVertices,
    ModelAnimation,
    AnimationPosition,
    AnimationRate,
    EmitterAxisVector {
        axis: u8,
    },
    Unresolved {
        opcode: i16,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectIntegerWidth {
    Byte,
    Halfword,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectArithmetic {
    Set,
    Add,
    Subtract,
    Multiply,
    Divide,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", content = "index", rename_all = "snake_case")]
pub enum EffectModifierDestination {
    ActorField(u16),
    IntegerTemporary(u8),
    FloatTemporary(u8),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectUvInventory {
    pub id: u8,
    pub offset: u32,
    pub commands: Vec<EffectUvCommand>,
    pub terminated: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EffectUvCommand {
    Frame { duration: u8 },
    Palette { duration: u8 },
    Scroll { opcode: u8 },
    Loop { target: u8 },
    End,
    Unresolved { opcode: u8 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectInventoryIssue {
    pub source: u16,
    pub bank: Option<EffectSourceBank>,
    /// Archive-relative for package errors; otherwise relative to the identified EF1 bank.
    pub offset: u32,
    pub kind: EffectInventoryIssueKind,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectInventoryIssueKind {
    Package,
    Bank,
    Program,
    Modifier,
    Actor,
    UvTrack,
    UnknownBankBinding,
    UnknownGeometry,
    UnknownModifier,
    UnknownFlags,
    UnknownSecondaryEmission,
}

impl EffectInventory {
    pub fn validate(&self) -> Result<()> {
        for (i, bank) in self.banks.iter().enumerate() {
            ensure!(
                usize::from(bank.source) < self.sources.len(),
                "effect inventory source missing"
            );
            ensure!(
                self.banks[..i].iter().all(|b| b.id != bank.id),
                "duplicate effect source bank"
            );
            ensure!(
                bank.alias_of
                    .is_none_or(|id| id != bank.id && self.banks.iter().any(|b| b.id == id)),
                "effect source alias has no target"
            );
            ensure!(
                bank.programs.len() == usize::from(bank.declared_programs),
                "effect program count differs from header"
            );
            for (j, program) in bank.programs.iter().enumerate() {
                ensure!(
                    program.id < u16::from(bank.declared_programs),
                    "effect program ID exceeds header"
                );
                ensure!(
                    bank.programs[..j].iter().all(|p| p.id != program.id),
                    "duplicate effect program ID"
                );
                ensure!(
                    program.actors.windows(2).all(|w| w[0] < w[1]),
                    "unordered effect actor references"
                );
            }
            for (j, actor) in bank.actors.iter().enumerate() {
                ensure!(
                    actor.id < bank.declared_actors
                        && bank.actors[..j]
                            .iter()
                            .all(|previous| previous.id != actor.id),
                    "duplicate or out-of-range effect actor ID"
                );
            }
        }
        for issue in &self.unresolved {
            ensure!(
                usize::from(issue.source) < self.sources.len(),
                "effect issue source missing"
            );
        }
        Ok(())
    }
}
