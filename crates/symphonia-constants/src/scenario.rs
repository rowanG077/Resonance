//! Original scenario ABI selectors recovered by the native service handlers.
//! Values name operations, rather than coincidentally equal coordinates/counts.

// resonance-events/src/native/wait.rs::Command::decode
ids! { pub enum WaitCondition: u8 {
    Ticks = 0, Resource = 1, DialogueClosed = 2, DialogueReady = 3,
    ActorMotion = 4, ActorExit = 6, ActorAnimation = 7, Camera = 8,
    CameraPathComplete = 9, CameraPathPosition = 10, CameraPathOffset = 11,
    CameraPathAngles = 12, CameraPathFov = 13, MediaReady = 14,
    MediaComplete = 15, MediaReadyAlternate = 16, MediaLoaded = 17,
    ActorHeading = 18, MediaPosition = 19,
} }

// resonance-events/src/native.rs::GetActorProperty/SetActorProperty, including
// sprite and portrait views of the same selector table.
ids! { pub enum ActorProperty: u8 {
    Reserved0 = 0, PositionX = 1, PositionY = 2, PositionZ = 3,
    TargetHeading = 4, MovementSpeed = 5, Reserved6 = 6, HeadingLock = 7,
    Opacity = 8, DisableCollision = 9, DisableGrounding = 10,
    DisableShadow = 11, HideModel = 12, CullingFlag0 = 13, CullingFlag1 = 14,
    WanderRadius = 15, Expression = 16, InteractionLabel = 17,
    ModelCollision = 18, Pushable = 19, ContactEvent = 20,
    EnemyEventParameter0 = 21, EnemyEventParameter1 = 22, EnemyNormalSpeed = 23,
    EnemyBehavior = 26, EnemyRandomTurns = 27, ScaleX = 30, ScaleY = 31,
    ScaleZ = 32, EmitterPhase = 33, Behavior = 34, TiltX = 35, TiltY = 36, Heading = 37,
    ToonLighting = 38, DrawLayer = 39, DisableSecondaryMotion = 40,
    Unlit = 41, TintRed = 42, TintGreen = 43, TintBlue = 44, Blend = 45,
    DisableDepthWrite = 46, Radius = 47, DisableRingContact = 48,
    EnemyAlerted = 49, DisableInteraction = 50, ShadowAlpha = 51,
    GroundAttributes = 52, GrabbedBlock = 53, EnemyPauseTicks = 54,
    EnemyReaction = 56, ShadeRed = 57, ShadeGreen = 58, ShadeBlue = 59,
    EnemyScriptFlag = 60, DisableRepulsion = 61, SpriteImage = 62,
    PortraitTimeline = 63, PortraitEyes = 64, PortraitMouth = 65,
    RerollLuck = 66, PauseOffscreen = 67, Conditions = 100, Level = 101,
    Hp = 102, Tp = 104, Luck = 112,
    // Native set/get handlers 80055140/8005673C expose the ten slots filled
    // by create_effect_emitter (80058F4C). Their meanings depend on its recipe.
    // Slots 0..3 are signed 16-bit fields; 4..9 are floats whose scalar writes
    // first truncate to unsigned 16 bits, unlike the signed constructor values.
    EmitterParameter0 = 113, EmitterParameter1 = 114, EmitterParameter2 = 115,
    EmitterParameter3 = 116, EmitterParameter4 = 117, EmitterParameter5 = 118,
    EmitterParameter6 = 119, EmitterParameter7 = 120, EmitterParameter8 = 121,
    EmitterParameter9 = 122,
} }

// resonance-events/src/native/field.rs::camera_property and GetCameraProperty
ids! { pub enum CameraProperty: u8 {
    Follow = 0, AnchorToActor = 1, FovHundredths = 2,
    PositionRate = 3, TargetRate = 4, AxisX = 5, AxisY = 6, AxisZ = 7,
    PositionMinX = 8, PositionMaxX = 9, PositionMinY = 10, PositionMaxY = 11,
    PositionMinZ = 12, PositionMaxZ = 13, TargetMinX = 14, TargetMaxX = 15,
    TargetMinY = 16, TargetMaxY = 17, TargetMinZ = 18, TargetMaxZ = 19,
    AngleX = 20, AngleY = 21, AngleZ = 22, Distance = 23,
} }

ids! {
    /// Native initializer 80060100 creates eight independent 0x94-byte camera
    /// records. Template names select those records; scripts edit their settings.
    /// Ordinary selector inputs narrow to a byte. The exact -2/-1 inputs instead
    /// start a script path or edit the next field's entry camera without changing
    /// the selected template index. A selection returns the previous index.
    pub enum CameraSelector: i32 {
    ScriptPath = -2, FollowPlayer = -1, Template0 = 0, Template1 = 1,
    Template2 = 2, Template3 = 3, Template4 = 4, Template5 = 5,
    Template6 = 6, Template7 = 7,
    }
}

// Original play_camera_track800494EC: zero starts playback; other values pause.
// Its repeat argument enables looping only when the full integer equals1.
ids! { pub enum CameraTrackStart: i32 { Playing = 0, Paused = 1 } }
ids! { pub enum CameraTrackRepeat: u8 { Once = 0, Loop = 1 } }
// Original configure_camera_track80048EF8. SetRatePercent divides by200:
// 100 selects the default half-frame step; ReadFrameStep truncates that step,
// rather than converting it back to a percentage. SetRepeat stores a byte,
// unlike play_camera_track's exact-one test; ReadRepeat returns that byte.
ids! { pub enum CameraTrackCommand: i32 {
    Resume = 0, Pause = 1, Rewind = 2, SeekFrame = 3, SetRatePercent = 4,
    SetRepeat = 5, ReadFrame = 6, ReadDuration = 7, ReadFrameStep = 8,
    ReadRepeat = 9, ReadCompleted = 10, SetTargetActor = 11,
    SetTargetOffsetX = 12, SetTargetOffsetY = 13, SetTargetOffsetZ = 14,
} }

// Original change_field (800500D4) and play_world_cinematic (8004FE90)
// interpret destinations >=3000 as world-service requests, not physical fields.
// Both observed encodings take that same branch; 3001 does not select Tethealla.
// Keep the separate literal encodings for byte matching. The X argument carries
// the landmark. These encodings do not distinguish Sylvarant from Tethealla.
ids! { pub enum WorldTransferId: u16 { Standard = 3000, Alternate = 3001 } }

// resonance-events/src/native/camera_path.rs::camera_path
ids! { pub enum CameraPathCommand: u8 {
    SetMode = 0, GetMode = 1, MovePosition = 3, ReadPosition = 6,
    ChangeFov = 7, ReadFov = 8, SetActor = 9, ReadActor = 10,
    MoveOffset = 11, ReadOffset = 12, Rotate = 14, ReadAngles = 18,
    AimAtActor = 19,
} }

// resonance-events/src/native.rs::SetTransitionMode
ids! { pub enum TransitionMode: u8 {
    FadeFromBlack = 0, FadeToBlack = 1, FadeFromWhite = 2,
    FadeToWhite = 3, Dissolve = 4, NextSceneClearShade = 5,
} }

// resonance-events/src/native/field.rs::SetActorOrientation
ids! { pub enum HeadingOperation: u8 {
    SetTarget = 0, AddTarget = 1, SetImmediate = 2, AddImmediate = 3,
    SetTurnSpeed = 4,
} }

// Original move_actor_relative800579C4 clears actor+A1's high nibble for0
// and sets it toF for every nonzero value, retaining the actor's heading.
ids! { pub enum MovementFacing: u8 { FollowMovement = 0, KeepHeading = 1 } }

// resonance-events/src/native/field.rs::SetActorAnimationProperty
ids! { pub enum AnimationProperty: u8 {
    RatePercent = 0, DurationFrames = 1, SeekFrame = 2, ReadRatePercent = 3,
    ReadFrame = 4, ReadDuration = 5, LoopStartFrame = 6,
} }

// resonance-events/src/animation.rs::slot
ids! { pub enum AnimationSlot: u16 {
    // Field requests below the first table offset select the default clip.
    Default = 0,
    Idle = 12, Talk = 24, Walk = 36, Run = 40, TurnRight = 44, TurnLeft = 48,
    TalkFallback = 52, Stagger = 76, EventTalk = 112, EventIdle = 116,
    EventWalk = 120, EventRun = 124,
} }

// The native track initializer (8006EB68) starts with flags 1. Bit 0 itself
// does not enable looping; playback reads Paused and Once. Replacing the low
// flags with Default selects unpaused looping playback. A zero flags-update
// operand instead resumes without clearing Once; see AnimationControl.
ids! { pub enum AnimationFlag: u8 {
    None = 0, Default = 1, Paused = 2, Once = 8,
} }
ids! { pub enum AnimationControl: u8 { Resume = 0 } }
ids! { pub enum AnimationSource: i32 { CurrentModel = -1, Automatic = 0 } }
// configure_scenery_animation accepts numbered layers and scene actor keys.
// The edit handlers only recognize numbered2/3; other inputs use the main bank.
ids! { pub enum SceneryLayer: i32 {
    Default = 0, Main = 1, Second = 2, Third = 3, Fourth = 4,
    FourthActor = 999980, MainActor = 999996, SecondActor = 999997, ThirdActor = 999998,
} }
// -1 selects the base animation track;0..3 index the layer's track arrays.
ids! { pub enum SceneryAnimationChannel: i32 { Base = -1, Slot0 = 0, Slot1 = 1, Slot2 = 2, Slot3 = 3 } }
// Native recovery dispatcher800511BC; mode1 and out-of-range inputs do nothing.
ids! { pub enum PartyRecoveryMode: i32 {
    Rest = 0,
    RestoreHp100Percent = 2, RestoreHp50Percent = 3, RestoreHp10Percent = 4,
    RestoreHp5Percent = 5, RestoreHp1Percent = 6,
    RestoreTp100Percent = 7, RestoreTp50Percent = 8, RestoreTp10Percent = 9,
    RestoreTp5Percent = 10, RestoreTp1Percent = 11,
    DamageHp100Percent = 12, DamageHp50Percent = 13, DamageHp10Percent = 14,
    DamageHp5Percent = 15, DamageHp1Percent = 16,
    DrainTp100Percent = 17, DrainTp50Percent = 18, DrainTp10Percent = 19,
    DrainTp5Percent = 20, DrainTp1Percent = 21,
    DamageHp20Percent = 22, ReviveIncapacitated = 23,
} }
// Observed configure_session commands from native dispatcher80043A44.
ids! { pub enum SessionCommand: i32 {
    Rumble = 3, SkitNotifications = 4, Stereo = 5,
    RecordGameClear = 10, GameClearCount = 11, LeaderLocked = 12,
    ReturnToTitle = 13, GameOver = 14, MenuDisabled = 15,
    StartNewGamePlus = 16, Credits = 18,
} }
// Only toggle commands interpret -1 as a query and store other values' low bit.
ids! { pub enum SessionToggle: i32 { Query = -1, Disabled = 0, Enabled = 1 } }
// Native party handlers query without writing when the value is negative.
ids! { pub enum TitleSelection: i32 { Query = -1 } }
ids! { pub enum TitleLookup: u8 { Equipped = 0 } }
ids! { pub enum PartySelection: i32 { Query = -1, RestoreFieldLeader = 999999 } }
ids! {
    /// Encoded formation-cell queries for find_party_member: a nonzero high word
    /// selects the cell given by the low word. The result is its signed character
    /// byte, or zero for an empty or out-of-range cell. Retail scripts use -65536
    /// OR the zero-based slot; these names preserve that encoding.
    /// A zero high word instead selects character lookup and returns a one-based
    /// formation position. Exact zero returns zero; other character queries narrow
    /// to a byte and search all eight cells, including empty cells.
    pub enum FormationSlotSelector: i32 {
    Slot0 = -65536, Slot1 = -65535, Slot2 = -65534, Slot3 = -65533,
    Slot4 = -65532, Slot5 = -65531, Slot6 = -65530, Slot7 = -65529,
    }
}
// resonance-events/src/ring.rs::SorcerersRing::MODES and ConfigureSorcerersRing.
ids! { pub enum SorcerersRingMode: i32 {
    Query = -1, Disabled = 0, Fire = 1, FireAlternate = 2, Shrink = 3,
    Mana = 4, SylvarantElectricOrb = 5, Radar = 6, Water = 7, Wind = 8,
    LongRangeFire = 9, Sunlight = 10, TetheAllaElectricOrb = 11,
    Bomb = 12, Lightning = 13, Ice = 14, Earthquake = 15, Darkness = 16,
    Sound = 17, AnimalCall = 18, Bubble = 19,
} }
ids! { pub enum RingLightningColor: u8 { Blue = 0, Yellow = 1, Red = 2 } }
ids! { pub enum RingCallColor: u8 { Pink = 0, White = 1, Blue = 2 } }
ids! { pub enum RingBubblePhase: u8 { Release = 0, Float = 1 } }
ids! { pub enum ExGemUpdate: i32 { Keep = -1 } }
// resonance-events/src/world.rs::MusicCommand::try_from
ids! { pub enum MusicControl: i32 { Stop = -1, Suspend = -2, Resume = -3 } }
// Original play_sound handler 8004ECB0 clears the named slot and stops its
// handle only when sound_id == -1 and binding_index != 255. With binding255, or
// play_sound_simple, this number does not select that stop branch.
ids! { pub enum SoundControl: i32 { StopSlot = -1 } }
// Original 8004E95C disables the actor's emitter on a full-width -1 cue,
// without issuing the sound-slot stop used by play_sound.
ids! { pub enum AmbientSoundControl: i32 { Disable = -1 } }
// Extended playback (8004ECB0) and slot volume (8004EB54) replace full-width
// volume255 with the current field sound-level byte at save+193. A bound
// sound_id -1 stops its slot without applying the volume operand.
ids! { pub enum SoundVolume: u8 { FieldDefault = 255 } }
// Extended playback tests the full-width binding index against 255 before
// its bound-stop branch. ConfigureSound always addresses a slot instead.
ids! { pub enum SoundBinding: u8 { Unbound = 255 } }
// Both spatial-audio paths use the current field sound level at save+193
// when their consumed volume byte is 255.
ids! { pub enum SpatialSoundVolume: u8 { Default = 255 } }
// Original configure_sound handler 8004EB54 addresses the same slot table:
// 100 sends controller 7 (volume), 101 controller 10 (pan), and 102 stops it.
// Its default switch path, including the retained 255 operands, does nothing.
ids! { pub enum SoundSlotOperation: u8 { Volume = 100, Pan = 101, Stop = 102 } }

// resonance-events/src/dialogue.rs::flags
ids! { pub enum DialogueFlag: u16 {
    // Original 80009FF4 initializes a 300-count close timer. Parser 8000588C
    // consumes it at message end after persistent/autopage and choice handling.
    None = 0, TimedClose = 1, Persistent = 2, Frameless = 4, Red = 8, Green = 16,
    AutoPages = 32, AutoSide = 64, Above = 128, Below = 256,
    // Original 80007BB8 chooses fixed RGBA E8C800FF after Green/Red precedence;
    // normal theme shading still applies to this base frame color.
    Yellow = 512, Pointer = 1024, NonModal = 2048, Instant = 4096,
} }
// resonance-events/src/native.rs::ConfigureDialogue
ids! { pub enum DialogueAnchor: i32 {
    GridBottomRight = -18, GridBottomCenter = -17, GridBottomLeft = -16,
    GridMiddleRight = -15, GridCenter = -14, GridMiddleLeft = -13,
    GridTopRight = -12, GridTopCenter = -11, GridTopLeft = -10,
    ScreenGrid = -2, Actor = -1,
} }
// Original message expansion8005AEE0 executes the following VM expression first.
// Control7 is retained without assigning its downstream formatting meaning.
ids! { pub enum MessageControl: u8 {
    CharacterName = 1, Delay = 2, Color = 3, ItemName = 4, Number = 5,
    Evaluate = 6, Control7 = 7, NumberInput = 8, Voice = 9,
    TitleName = 17, TechniqueName = 18,
} }

ids! {
    /// Three real dialogue windows. Native waits resolve any full signed selector
    /// >=3 to Main through 80005760, without narrowing it to a byte. The retained
    /// 8/20 encodings are named separately so matching compilation preserves them.
    /// These fallback encodings apply to waits; configure_dialogue rejects slots
    /// >=3 instead of creating or replacing a main window.
    pub enum DialogueSlot: u8 {
        Main = 0, Secondary = 1, Auxiliary = 2,
        /// Hot Spring 0089 waits for the main window to reach status >=5.
        MainFallback8 = 8,
        /// Iselia Human Ranch 0193 waits for the main window to close (status0).
        MainFallback20 = 20,
    }
}
// ScreenGrid is a three-by-three grid, numbered from top left to bottom right.
ids! { pub enum DialogueGrid: u8 {
    TopLeft = 0, TopCenter = 1, TopRight = 2,
    MiddleLeft = 3, Center = 4, MiddleRight = 5,
    BottomLeft = 6, BottomCenter = 7, BottomRight = 8,
    /// Retained Meltokio 0409 encoding. Original configure_dialogue8005A504 uses
    /// the out-of-grid fallback (320,240); grid Center4 is instead (318,240).
    DefaultCenter200 = 200,
} }
ids! { pub enum DialogueFlagOperation: u8 { Persistent = 0, AutoPages = 1 } }
ids! { pub enum DialogueStatus: u8 {
    Closed = 0, Waiting = 1, Opening = 2, Revealing = 3, PageReady = 4,
    Finished = 5, Closing = 6,
} }

// Native decision selector80023164 dispatches only0..12. The observed13/90
// encodings leave selection unchanged; a forced actor state still takes priority.
// Keep the two encodings distinct for byte-exact script recompilation.
// resonance-events/src/autonomy.rs::Behavior::code
ids! { pub enum ActorBehavior: u8 {
    Stationary = 0, Wander = 1, WanderNearHome = 2, FollowPath = 3,
    WatchPlayer = 4, ApproachPlayer = 5, Player = 10, RandomPath = 11,
    ChasePlayer = 12, NoDecision13 = 13, NoDecision90 = 90, AnchoredToPlayer = 127,
} }
// resonance-events/src/emote.rs::Kind::try_from
ids! { pub enum EmoteId: u8 {
    Ellipsis = 0, Distress = 1, ColorBurst = 2, Steam = 3, Blush = 4,
    Exclamation = 5, Question = 6, Music = 7, Heart = 8, Hidden = 9,
    Sweat = 10, SweatDrops = 11, Sleep = 12, Hearts = 13, Surprise = 14, Anger = 15,
} }

// resonance-events/src/native/field.rs::MeasureActorGeometry
ids! { pub enum GeometryQuery: u8 { Bearing = 0, DistanceXY = 1, HeightDifference = 2, DistanceXYZ = 3 } }
ids! { pub enum CoordinateRegister: u8 { X = 0, Y = 1, Z = 2, Pitch = 3, Yaw = 4, Roll = 5 } }
ids! { pub enum EventKind: u8 { ActorInteraction = 0, Region = 1, Spawned = 2 } }
// Native trigger_exists (800530CC) queries the record status at +0x2f70,
// not the event registry kind. Confirmed records dispatch through registry 2.
ids! { pub enum TriggerKind: i16 { Touch = 1, Automatic = 2, Confirmed = 3 } }
ids! { pub enum EventControl: u8 { Resume = 0, Pause = 1, ReleaseControlGate = 50, HoldControlGate = 51 } }
ids! { pub enum ActorSelector: i32 {
    FourthScenery = 999980, MainScenery = 999996, SecondScenery = 999997,
    ThirdScenery = 999998, Controlled = 999999,
} }
// create_overlay80041BA4 selects the dedicated location-caption renderer
// 8007E1BC for this full-width creation key. Other actor APIs do not interpret
// it as a scenery/controlled selector; image resources remain separate.
ids! { pub enum OverlayActorId: i32 { LocationCaption = 999989 } }
// Observed model controller slots, independent of model-local bone indices.
ids! { pub enum BoneControllerSlot: i32 { Slot0 = 0, Slot1 = 1, Slot2 = 2, Slot3 = 3, Slot4 = 4, Slot5 = 5, Slot6 = 6, Slot7 = 7 } }
// resonance-events/src/native/field.rs::FindActorBodyPart
ids! { pub enum BodyPartId: u8 {
    Neck = 0, Head = 1, Spine1 = 2, Spine2 = 3, Spine3 = 4,
    LeftArm1 = 5, LeftArm2 = 6, RightArm1 = 7, RightArm2 = 8,
    LeftLeg1 = 9, LeftLeg2 = 10, LeftLeg3 = 11, LeftLeg4 = 12,
    RightLeg1 = 13, RightLeg2 = 14, RightLeg3 = 15, RightLeg4 = 16,
} }

// resonance-events/src/effect/property.rs::Property::decode
ids! { pub enum EffectProperty: u16 {
    PositionX = 120, PositionY = 121, PositionZ = 122, Width = 123, Height = 124,
    Red = 125, Green = 126, Blue = 127, Alpha = 128, VelocityXPercent = 129,
    VelocityYPercent = 130, VelocityZPercent = 131, SpinX = 132, SpinY = 133,
    SpinZ = 134, Growth = 135, Speed = 136, NormalizeVelocity = 137,
    VelocityX = 138, VelocityY = 139, VelocityZ = 140, RotationX = 141,
    RotationY = 142, RotationZ = 143, Orientation = 144, Blend = 145,
    Fade = 146, Anchor = 147, Fog = 148, RotationOrder = 149,
} }
// resonance-events/src/model_particle.rs::Property::decode
ids! { pub enum ModelParticleProperty: u16 {
    PositionX = 420, PositionY = 421, PositionZ = 422, VelocityXPercent = 423,
    VelocityYPercent = 424, VelocityZPercent = 425, RotationX = 426,
    RotationY = 427, RotationZ = 428, SpinX = 429, SpinY = 430, SpinZ = 431,
    ScaleX = 432, ScaleY = 433, ScaleZ = 434, GrowthX = 435, GrowthY = 436,
    GrowthZ = 437, Red = 438, Green = 439, Blue = 440, Alpha = 441,
    Speed = 442, Motion = 443, VelocityX = 444, VelocityY = 445,
    VelocityZ = 446, Blend = 447, GravityAndFade = 448, Shading = 449,
} }

// Billboard factory 800830BC, shared by create_particle/create_effect_object.
// These drawing roles are distinct from actor emitter recipes and audio cues.
// Names follow the cooked sprite roles; gaps and unrecognized retail inputs stay raw.
ids! { pub enum ParticleRecipe: u16 {
    Glow = 0, Smoke = 1, RedGlow = 2, StationGlow = 4,
    CameraDisc = 5, WorldGlow = 6, Star = 7, SpinningStar = 8,
    AirRefraction = 9, Orb = 10, Flame = 11, TrailGlow = 12,
    Burst = 13, ElectricArc = 14, Swirl = 18, SparkleCluster = 21,
    StationHalo = 22, Streak = 23, FallingLeaf = 25, RisingLeaf = 26,
    CameraRipple = 27, WorldRipple = 28,
    TextureSlot0 = 32, TextureSlot7 = 39, CameraRing = 40, Ring = 41,
    ElectricSpark = 42, FallingSpark = 43, ImpactFlare = 49,
    SealSpark = 69, SquareGlow = 70, LightningBolt = 74, FlamePlume = 80,
} }

// create_effect_emitter stores this selector separately from model resources,
// sound cues and billboard sprite recipes. Original renderer 800884D0 dispatches
// it through the signed halfword at actor+0x84c. Names follow the implemented
// emitter behaviors in resonance-events/src/emitter/{native,stream}.rs. Only
// observed script recipes are declared; gaps remain open to unknown/mod values.
ids! { pub enum EmitterRecipe: u16 {
    Fire = 0, FireWithSmoke = 1, Awakening = 4, Splash = 9,
    VioletProjectile = 10, Gathering = 11, Glow = 12, Scatter = 13,
    Portal = 14, RisingOrbs = 15, Crown = 16, Rain = 17, Mote = 18,
    Charge = 19, ConvergingShafts = 20, Spiral = 21, Quake = 22,
    Column = 23, Smoke = 24, Flame = 25, Shafts = 26, Convergence = 27,
    Aura = 28, Burst = 29, DriftingOrbs = 30, Contract = 31,
    Ray = 32, TwinTrail = 33, Bloom = 34, TargetedStars = 36,
    PaletteProjectile = 37, Inward = 38, Lightning = 39,
    WhiteProjectile = 40, RisingSmoke = 41, TwinGlow = 42,
    Explosion = 43, FilledStars = 44, RadialSpray = 45,
    TravelWithAfterimages = 46, Travel = 47, Rising = 48, Seal = 49,
    ModelTrail = 50, Fireball = 51, SparkFlash = 52, Flash = 53,
    Stars = 54, AttachedTrail = 55, LinearTrail = 56, PlanarLights = 57,
    ChargedTrail = 58, Cloud = 59, Cardinal = 60,
    ColorChangingProjectile = 61, Embers = 62, ExpandingColumn = 63,
    RisingCircle = 64, Orbiting = 65, ColorChangingProjectileWithImpact = 66,
    ChargedRay = 67, ExpandingOrbit = 68, Cylinder = 70, Sheet = 71,
    BurstSheet = 72, DestinationOrbs = 73, SoftBurst = 74, FilledSplash = 75,
} }

// Native E7 (80042C44) exchanges persistent battle settings and returns the old
// value. The adjustment bytes are signed percentages in battle REL 1:61578.
ids! { pub enum BattleRule: u8 {
    Modifiers = 0, DisabledCommands = 1, Coliseum = 2,
    AttackPercent = 3, DefensePercent = 4, IntelligencePercent = 5,
} }

// Native89 (80040C24) copies eight formation bytes to an independent snapshot,
// or clears those bytes. Neither operation restores/reorders the live formation.
ids! { pub enum PartySnapshotOperation: u8 { Capture = 0, Clear = 1 } }

// Native46 writes one of eight field-dependent settings, changes enable bit0,
// or clears the initialization bit7. Values are not globally actor/texture IDs.
ids! { pub enum RenderingCommand: u8 {
    Setting0 = 0, Setting1 = 1, Setting2 = 2, Setting3 = 3,
    Setting4 = 4, Setting5 = 5, Setting6 = 6, Setting7 = 7,
    Enabled = 128, Reinitialize = 129,
} }
ids! { pub enum RenderingToggle: u8 { Disabled = 0, Enabled = 1 } }
// NativeAB's two independent copy planes. Depth inputs/results are hundredths;
// read/reset commands still evaluate the ignored second argument.
ids! { pub enum ScreenCopyCommand: u8 {
    SetPass0Depth = 0, SetPass1Depth = 1, ReadPass0Depth = 2,
    ReadPass1Depth = 3, ResetPass0Depth = 4, ResetPass1Depth = 5,
} }

// Native7E uses two destination buffers: zero selects primary; every nonzero
// selector selects secondary. These are buffer roles, not source archive IDs.
ids! { pub enum VoiceBankSlot: u8 { Primary = 0, Secondary = 1 } }

// Dialogue control9 consumes these sentinels before archive/member playback.
// Preload7E has different semantics and must not receive these aliases.
ids! { pub enum VoiceControl: i32 { None = 0, Stop = -1 } }
