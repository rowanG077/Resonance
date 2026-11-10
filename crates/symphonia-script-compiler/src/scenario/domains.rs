//! Context-dependent native identities. Unconditional domains live with the
//! recovered native ABI metadata, rather than a second list of signatures.
use super::{Context, Definitions};
use symphonia_script::semantics::NativeProcedure;

pub(super) fn argument<'a>(
    native: &'a NativeProcedure,
    index: usize,
    values: &[Option<i32>],
    context: Context,
    definitions: &'a Definitions,
) -> Option<&'a str> {
    let at = |index| values.get(index).copied().flatten();
    // These operands go to battle services in mode9. Camera commands are
    // ignored outside that mode; relative movement's battle request keys
    // and final option have not been established as field actor/facing IDs.
    if native.opcode == 0x93 && context != Context::Battle {
        return None;
    }
    if native.opcode == 0x18 && context == Context::Battle && matches!(index, 0 | 4) {
        return None;
    }
    // Both model-name queries resolve scenery/controlled selectors in the field;
    // mode9 forwards the literal actor key to the battle manager.
    if matches!(native.opcode, 0xbc | 0xb4) && index == 0 {
        if at(0) == Some(999999) && !context.is_field() {
            return None;
        }
        if context.is_field() && matches!(at(0), Some(999980 | 999996 | 999997 | 999998)) {
            return Some("scenario::ActorSelector");
        }
    }
    // Attachment query skips bone -1; battle uses that bone operand as both
    // request key and node index, ignoring the field actor operand entirely.
    if native.opcode == 0xcc {
        if at(1) == Some(-1) || (index == 0 && !context.is_field()) {
            return None;
        }
        if index == 0 && matches!(at(0), Some(999980 | 999996 | 999997 | 999998)) {
            return Some("scenario::ActorSelector");
        }
    }
    // Bone -1 skips the whole adjustment, including actor/controller lookup.
    if matches!(native.opcode, 0xb5 | 0xc1 | 0xc3 | 0xc4) && at(2) == Some(-1) {
        return None;
    }
    if matches!(native.opcode, 0xb1 | 0xb5 | 0xc1 | 0xc3 | 0xc4) && index == 0 {
        if at(0) == Some(999999) && !context.is_field() {
            return None;
        }
        if context.is_field() && matches!(at(0), Some(999980 | 999996 | 999997 | 999998)) {
            return Some("scenario::ActorSelector");
        }
    }
    // Only configure accepts actor-key aliases and a fourth bank. The edit
    // handlers send these values to the main bank, so don't imply that alias.
    if matches!(native.opcode, 0x43..=0x45)
        && index == 0
        && matches!(at(0), Some(4 | 999980 | 999997 | 999998))
    {
        return None;
    }
    // Field spawn ignores the controlled-player key; battle spawn forwards
    // it as a request key, not as an existing-actor selector.
    if native.opcode == 0x10 && at(0) == Some(999999) {
        return None;
    }
    // Mode9 uses heading/model requests rather than field emotes or autonomy.
    // An unspecified host cannot establish those mode-dependent identities.
    if native.opcode == 0x10
        && index >= 4
        && matches!(context, Context::Battle | Context::Unspecified)
    {
        return None;
    }
    // These constructors assign a new scene actor key at object+B8; they
    // do not resolve an existing actor or the controlled-player alias.
    if matches!(native.opcode, 0x5c | 0xb9 | 0xba | 0xbb | 0xd6) && index == 0 {
        return None;
    }
    // Both field actor-audio lookups return no actor in mode9. These calls
    // cannot consume actor/cue/volume identities in a battle event.
    if matches!(native.opcode, 0x53 | 0xe2) && context == Context::Battle {
        return None;
    }
    // Resource0 restores automatic playback outside battle without reading
    // flags. Without an execution context, their consumption is also unproven.
    if native.opcode == 0x9b && index == 4 && at(1) == Some(0) && context != Context::Battle {
        return None;
    }
    if native.argument_types.get(index).map(String::as_str) == Some("story::StoryFlag")
        && let Some(domain) =
            at(index).and_then(|value| definitions.scoped_flag_domain(context, value))
    {
        return Some(domain);
    }
    if native.opcode == 0x27
        && let Some(value) = at(0).filter(|v| v & !0xffff != 0)
    {
        return ((value & 0xffff) < 8).then_some("scenario::FormationSlotSelector");
    }
    if native.name == "select_party_member" && matches!(at(index), Some(-1 | 999999)) {
        return Some("scenario::PartySelection");
    }
    if native.name == "get_title" && index == 1 && at(1) == Some(0) {
        return Some("scenario::TitleLookup");
    }
    if native.argument_types.get(index).map(String::as_str) == Some("characters::CharacterId")
        && at(index) == Some(999999)
        // Title exchange indexes the save records directly. It never calls
        // the actor resolver that interprets the controlled-player key.
        && native.opcode != 0x8c
    {
        return Some("scenario::ActorSelector");
    }
    let name = &native.arguments[index];
    let actor = name.ends_with("actor_id")
        || matches!(
            name.as_str(),
            "portrait_id" | "actor" | "source_actor" | "enemy_actor" | "owner_id"
        );
    // The fourth dialogue operand is either an actor, a grid cell or screen Y.
    // In particular grid cell 4 is not Raine despite having the same number.
    if native.opcode == 0x0c {
        match index {
            2 if at(2).is_some_and(|v| v >= 0) => return None,
            3 => {
                return match at(2) {
                    Some(-1) => actor_domain(at(3), context),
                    Some(-2) => Some("scenario::DialogueGrid"),
                    _ => None,
                };
            }
            _ => {}
        }
    }
    // This constructor key selects a renderer rather than resolving an actor.
    if native.opcode == 0x4b && index == 0 && context.is_field() && at(0) == Some(999989) {
        return definitions
            .name("scenario::OverlayActorId", 999989)
            .map(|_| "scenario::OverlayActorId");
    }
    if actor {
        return actor_domain(at(index), context);
    }
    let dynamic = match (native.opcode, index) {
        (0x93, 0) => Some("battle::BattleCameraCommand"),
        (0x18, 4) if context.is_field() || context == Context::Skit => {
            Some("scenario::MovementFacing")
        }
        (0xae, 1) if at(0) == Some(5) => Some("scenario::CameraTrackRepeat"),
        (0xae, 1) if at(0) == Some(11) => actor_domain(at(1), context),
        (0x46, 1) if at(0) == Some(128) => Some("scenario::RenderingToggle"),
        (0x6d, 1) if matches!(at(0), Some(3 | 4 | 5 | 12 | 15)) => Some("scenario::SessionToggle"),
        (0x42, 2) if context.is_field() && at(2).is_some_and(|id| id >= 53) => {
            Some("resources::FieldAnimationId")
        }
        // World-service transfers reinterpret field X as a landmark. Ordinary
        // field coordinates and unknown dynamic destinations remain scalars.
        (0x40, 0) | (0x4c, 1) if at(index).is_some_and(|map| map >= 3000) => {
            Some("scenario::WorldTransferId")
        }
        (0x40, 1) if at(0).is_some_and(|map| map >= 3000) => Some("locations::WorldLocationId"),
        (0x4c, 2) if at(1).is_some_and(|map| map >= 3000) => Some("locations::WorldLocationId"),
        (0x22 | 0x24 | 0x25 | 0x27, 0) if at(0) == Some(999999) => Some("scenario::ActorSelector"),
        (0x10, 4) if at(0).is_some_and(|id| (-299..=-100).contains(&id)) => {
            Some("scenario::EmoteId")
        }
        (0x10, 5) if at(0).is_some_and(|id| (-299..=-100).contains(&id)) => {
            actor_domain(at(5), context)
        }
        (0x10, 5) if context.is_field() && at(0).is_some_and(|id| id >= 0) => {
            model_domain(at(5), context, definitions)
        }
        (0xb9 | 0xba | 0xbb | 0xd6, 5) if context.is_field() => {
            model_domain(at(5), context, definitions)
        }
        (0x5c, 5) if context.is_field() => model_domain(at(5), context, definitions),
        (0x1f, 10) | (0xc6, 11) if context.is_field() => {
            model_domain(at(index), context, definitions)
        }
        (0x98, 0)
            if context.is_field()
                && at(0).is_some_and(|id| {
                    matches!(id, 68611 | 68614)
                        || definitions.name("resources::FieldOverlayId", id).is_some()
                }) =>
        {
            Some("resources::FieldOverlayId")
        }
        (0x98, 0) | (0x9b, 1) if context.is_field() && at(index).is_some_and(|id| id >= 53) => {
            if at(index).is_some_and(|id| {
                definitions
                    .name("resources::FieldAnimationId", id)
                    .is_some()
            }) {
                Some("resources::FieldAnimationId")
            } else {
                Some("resources::FieldModelId")
            }
        }
        (0x10, 6) if at(0).is_some_and(|id| id >= 0) => Some("scenario::ActorBehavior"),
        (0xb9 | 0xba | 0xbb | 0xd6, 6) => Some("scenario::ActorBehavior"),
        (0x1f, 2) | (0xc6, 3) => Some("battle::ArenaId"),
        (0x1d, 2) => match at(1) {
            Some(34) => Some("scenario::ActorBehavior"),
            Some(63) if context == Context::Skit => Some("skits::PortraitId"),
            _ => None,
        },
        (0x35, 1) if at(1).is_some_and(|reward| (0..1024).contains(&reward)) => {
            Some("items::ItemId")
        }
        (0x35, 1) if at(1).is_some_and(|reward| (1024..=65535).contains(&reward)) => {
            Some("treasure::TreasureRewardEncoding")
        }
        (0x64, 1) => match at(0) {
            Some(2 | 3) => Some("scenario::DialogueSlot"),
            Some(4 | 6 | 7 | 18) => actor_domain(at(1), context),
            Some(1) if context == Context::Skit => Some("skits::PortraitId"),
            _ => None,
        },
        // Only -1 indexes the actor's model table. Other resources resolve
        // directly to an animation pointer, while zero restores automatic
        // animation. Neither branch reads this operand as a model-table slot.
        (0x9b, 2) if context.is_field() && at(1) == Some(-1) => Some("scenario::AnimationSlot"),
        (0x9b, 1)
            if at(1) == Some(-1)
                || (at(1) == Some(0)
                    && matches!(
                        context,
                        Context::Field | Context::FieldScene(_) | Context::World | Context::Skit
                    )) =>
        {
            Some("scenario::AnimationSource")
        }
        (0x9c, 1) if at(1) == Some(0) => Some("scenario::AnimationControl"),
        (0x50, 0) if at(0).is_some_and(|v| v < 0) => Some("scenario::MusicControl"),
        (0xcd, 3) if matches!(at(3), Some(-1 | 0)) => Some("battle::BattleMusicSelection"),
        (0xcd, 3) if matches!(at(3), Some(1..=32767)) => Some("music::MusicId"),
        (0xe0, 0) if at(0) == Some(-1) && at(3).is_some_and(|slot| slot != 255) => {
            Some("scenario::SoundControl")
        }
        // A bound cue-1 stops without applying volume. Unknown cue/binding
        // values cannot establish which branch consumes this operand.
        (0xe0, 2)
            if at(2) == Some(255) && (at(0).is_some_and(|cue| cue != -1) || at(3) == Some(255)) =>
        {
            definitions
                .name("scenario::SoundVolume", 255)
                .map(|_| "scenario::SoundVolume")
        }
        (0xe1, 2) if at(1) == Some(100) && at(2) == Some(255) => definitions
            .name("scenario::SoundVolume", 255)
            .map(|_| "scenario::SoundVolume"),
        (0xe0, 3) if at(3) == Some(255) => definitions
            .name("scenario::SoundBinding", 255)
            .map(|_| "scenario::SoundBinding"),
        (0x53, 1) if at(1) == Some(-1) => Some("scenario::AmbientSoundControl"),
        (0x53, 2) if at(1).is_some_and(|cue| cue != -1) && at(2) == Some(255) => {
            Some("scenario::SpatialSoundVolume")
        }
        (0xe2, 2) if at(2) == Some(255) => Some("scenario::SpatialSoundVolume"),
        (0x8c, 1) if at(1) == Some(-1) => Some("scenario::TitleSelection"),
        (0x81, 2 | 3) if at(index) == Some(-1) => Some("scenario::ExGemUpdate"),
        (0x78, 1) => match at(0) {
            Some(13) => Some("scenario::RingLightningColor"),
            Some(18) => Some("scenario::RingCallColor"),
            Some(19) => Some("scenario::RingBubblePhase"),
            _ => None,
        },
        _ => None,
    };
    dynamic.or_else(|| {
        native
            .argument_types
            .get(index)
            .map(String::as_str)
            .filter(|t| t.contains("::"))
    })
}

pub(super) fn result<'a>(native: &'a NativeProcedure, values: &[Option<i32>]) -> Option<&'a str> {
    let at = |index| values.get(index).copied().flatten();
    match native.opcode {
        0x27 if at(0).is_some_and(|value| value & !0xffff != 0 && value & 0xffff < 8) => {
            Some("characters::CharacterId")
        }
        0x2a if at(1) == Some(0) => Some("characters::TitleId"),
        0x92 if at(0) == Some(13) => Some("characters::CharacterId"),
        _ => native.result_type.as_deref().filter(|t| t.contains("::")),
    }
}

fn actor_domain(value: Option<i32>, context: Context) -> Option<&'static str> {
    match (value, context) {
        (Some(999999), _) => Some("scenario::ActorSelector"),
        (
            Some(1..=10),
            Context::Field | Context::FieldScene(_) | Context::Battle | Context::Skit,
        ) => Some("characters::CharacterId"),
        (_, Context::World) => Some("world::WorldActorId"),
        _ => None,
    }
}

fn model_domain(value: Option<i32>, context: Context, definitions: &Definitions) -> Option<&str> {
    // An explicitly declared scene model can name a local handle or small ID.
    // Other scenes and unknown contexts cannot borrow that binding. The native
    // operand guards above still establish whether this argument selects a model.
    value
        .and_then(|id| definitions.scoped_model_domain(context, id))
        .or_else(|| {
            // Key24 is a fixed built-in Sphere01 descriptor, selected before
            // the field-bank lookup. The optional editable enum supplies its
            // name; other small keys still need a reviewed scene binding.
            (value == Some(24)
                && definitions
                    .name("resources::BuiltinFieldModelId", 24)
                    .is_some())
            .then_some("resources::BuiltinFieldModelId")
        })
        .or_else(|| {
            // Small IDs otherwise depend on the field's native bank; negative
            // handles are local instances rather than global catalogue entries.
            value
                .filter(|&id| id >= 53)
                .map(|_| "resources::FieldModelId")
        })
}
