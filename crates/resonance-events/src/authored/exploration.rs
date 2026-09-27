//! Field exploration primitives. The authored event owns sequencing and rewards;
//! these bindings validate handles and delegate to ordinary engine services.
use super::*;
use crate::{TreasureKind, TreasureReward};
use symphonia_script::authored::NativeVariant;

const CHEST: Type = Type::Handle("game::treasure::Chest");
const ACTOR: Type = Type::Handle("game::actors::Actor");
const ITEM: Type = Type::TextReference {
    name: "game::text::Item",
    kind: TextReferenceKind::Item,
};

const KIND: Type = Type::Enum {
    name: "game::treasure::Kind",
    variants: &[
        NativeVariant {
            name: "UnknownId0",
            tag: TreasureKind::UnknownId0 as i32,
            payload: &[],
        },
        NativeVariant {
            name: "UnknownId1",
            tag: TreasureKind::UnknownId1 as i32,
            payload: &[],
        },
        NativeVariant {
            name: "UnknownId2",
            tag: TreasureKind::UnknownId2 as i32,
            payload: &[],
        },
        NativeVariant {
            name: "CustomModel0",
            tag: TreasureKind::CustomModel0 as i32,
            payload: &[],
        },
        NativeVariant {
            name: "CustomModel1",
            tag: TreasureKind::CustomModel1 as i32,
            payload: &[],
        },
    ],
};
#[repr(i32)]
enum RewardTag {
    Item,
    Gald,
}
const REWARD: Type = Type::Enum {
    name: "game::treasure::Reward",
    variants: &[
        NativeVariant {
            name: "Item",
            tag: RewardTag::Item as i32,
            payload: &[ITEM],
        },
        NativeVariant {
            name: "Gald",
            tag: RewardTag::Gald as i32,
            payload: &[Type::I32],
        },
    ],
};

#[repr(u8)]
enum Call {
    ChestActor = 16,
    ChestKind,
    ChestReward,
    MarkOpened,
    Animation,
    AnimationFinished,
    StopAnimation,
    Sound,
    GiveItem,
    GiveGald,
    InstantNotice,
}
impl Call {
    const fn declaration(self) -> NativeDeclaration {
        let (name, parameters, result, suspends): (_, &[Type], _, _) = match self {
            Self::ChestActor => ("game::treasure::actor", &[CHEST], Some(ACTOR), false),
            Self::ChestKind => ("game::treasure::kind", &[CHEST], Some(KIND), false),
            Self::ChestReward => ("game::treasure::reward", &[CHEST], Some(REWARD), false),
            Self::MarkOpened => ("game::treasure::mark_opened", &[CHEST], None, false),
            Self::Animation => (
                "game::actors::animate",
                &[ACTOR, Type::F32, Type::Bool],
                None,
                false,
            ),
            Self::AnimationFinished => (
                "game::actors::animation_finished",
                &[ACTOR],
                Some(Type::Bool),
                false,
            ),
            Self::StopAnimation => ("game::actors::stop_animation", &[ACTOR], None, false),
            Self::Sound => ("game::audio::sound", &[Type::I32], None, false),
            Self::GiveItem => (
                "game::party::give_item",
                &[ITEM, Type::I32],
                Some(Type::Bool),
                false,
            ),
            Self::GiveGald => (
                "game::party::give_gald",
                &[Type::I32],
                Some(Type::Bool),
                false,
            ),
            Self::InstantNotice => ("game::field::instant_notice", &[Type::Message], None, true),
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
fn chest<'a>(host: &'a FieldHost<'_>, handle: i32) -> Result<&'a crate::TreasureChest, String> {
    host.world
        .treasures
        .get(usize::try_from(handle).map_err(|_| "invalid treasure handle")?)
        .ok_or_else(|| "treasure handle is stale".into())
}
fn animation<'a>(host: &'a mut FieldHost<'_>, id: i32) -> Result<&'a mut crate::Animation, String> {
    host.world
        .actors
        .get_mut(&id)
        .and_then(|a| a.animation.as_mut())
        .ok_or_else(|| "actor animation is missing".into())
}
pub(super) const fn register(
    bindings: NativeBindings<FieldHost<'_>>,
) -> NativeBindings<FieldHost<'_>> {
    bindings
        .register_typed(Call::ChestActor.declaration(), |h, a, _| {
            Ok(NativeResult::Continue(Some(chest(h, a[0])?.actor)))
        })
        .register_typed(Call::ChestKind.declaration(), |h, a, _| {
            Ok(NativeResult::Continue(Some(chest(h, a[0])?.kind as i32)))
        })
        .register_typed(Call::ChestReward.declaration(), |h, a, _| {
            Ok(NativeResult::Values(match chest(h, a[0])?.reward {
                TreasureReward::Item(item) => vec![RewardTag::Item as i32, i32::from(item)],
                TreasureReward::Gald(amount) => vec![RewardTag::Gald as i32, i32::from(amount)],
            }))
        })
        .register_typed(Call::MarkOpened.declaration(), |h, a, _| {
            let flag = chest(h, a[0])?.flag;
            h.world
                .party
                .as_mut()
                .ok_or("party is missing")?
                .travel
                .opened_treasures
                .insert(flag);
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Call::Animation.declaration(), |h, a, _| {
            let tick = h.world.tick;
            let clip = animation(h, a[0])?;
            clip.seek(
                if a[2] != 0 {
                    clip.duration_ticks as f32
                } else {
                    0.
                },
                tick,
            );
            clip.rate = f32::from_bits(a[1] as u32);
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Call::AnimationFinished.declaration(), |h, a, _| {
            let tick = h.world.tick;
            let clip = animation(h, a[0])?;
            let done = if clip.rate < 0. {
                clip.elapsed(tick, 0) <= 0.
            } else {
                clip.elapsed(tick, 0) >= clip.duration_ticks as f32
            };
            Ok(NativeResult::Continue(Some(i32::from(done))))
        })
        .register_typed(Call::StopAnimation.declaration(), |h, a, _| {
            let tick = h.world.tick;
            let clip = animation(h, a[0])?;
            let frame = clip.elapsed(tick, 0).clamp(0., clip.duration_ticks as f32);
            clip.seek(frame, tick);
            clip.rate = 0.;
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Call::Sound.declaration(), |h, a, _| {
            let id = i16::try_from(a[0]).map_err(|_| "invalid field sound")?;
            if h.world.audio_commands.len() >= 512 {
                return Err("audio command queue is not being consumed".into());
            }
            h.world.audio_commands.push(crate::AudioCommand::Sound {
                id,
                pan: 64,
                volume: 127,
                slot: None,
            });
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Call::GiveItem.declaration(), |h, a, _| {
            let id = u16::try_from(a[0]).map_err(|_| "invalid item ID")?;
            let data = h
                .resources
                .session_data
                .as_ref()
                .ok_or("item definitions missing")?;
            let count = i8::try_from(a[1]).map_err(|_| "invalid item count")?;
            if count < 0 {
                return Err("negative item reward".into());
            }
            let received = h
                .world
                .party
                .as_mut()
                .ok_or("party is missing")?
                .change_item(data, id, count)?;
            Ok(NativeResult::Continue(Some(i32::from(received))))
        })
        .register_typed(Call::GiveGald.declaration(), |h, a, _| {
            if a[0] < 0 {
                return Err("negative gald reward".into());
            }
            let party = h.world.party.as_mut().ok_or("party is missing")?;
            let received = party.gald < 99_999_999;
            if received {
                party.add_gald(a[0]);
            }
            Ok(NativeResult::Continue(Some(i32::from(received))))
        })
        .register_typed(Call::InstantNotice.declaration(), |h, a, _| {
            h.notice(a, crate::dialogue::flags::INSTANT)
        })
}
