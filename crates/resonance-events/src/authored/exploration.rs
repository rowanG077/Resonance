//! Field exploration primitives. The authored event owns sequencing and rewards;
//! these bindings validate handles and delegate to ordinary engine services.
use super::*;
use crate::{TreasureKind, TreasureReward};

const CHEST: Type = Type::Handle("game::treasure::Chest");
const ACTOR: Type = Type::Handle("game::actors::Actor");
const ITEM: Type = Type::TextReference {
    name: "game::text::Item",
    kind: TextReferenceKind::Item,
};

const KIND: Type = Type::Enum {
    name: "game::treasure::Kind",
    variants: &[
        variant("UnknownId0", TreasureKind::UnknownId0 as i32, &[]),
        variant("UnknownId1", TreasureKind::UnknownId1 as i32, &[]),
        variant("UnknownId2", TreasureKind::UnknownId2 as i32, &[]),
        variant("CustomModel0", TreasureKind::CustomModel0 as i32, &[]),
        variant("CustomModel1", TreasureKind::CustomModel1 as i32, &[]),
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
        variant("Item", RewardTag::Item as i32, &[ITEM]),
        variant("Gald", RewardTag::Gald as i32, &[Type::I32]),
    ],
};

fn chest<'a>(host: &'a FieldHost<'_>, handle: i32) -> Result<&'a crate::TreasureChest, String> {
    host.world
        .treasures
        .get(usize::try_from(handle).map_err(|_| "invalid treasure handle")?)
        .ok_or_else(|| "treasure handle is stale".into())
}
fn animation<'a>(host: &'a mut FieldHost<'_>, id: i32) -> Result<&'a mut crate::Animation, String> {
    let id = host.actor_id(id)?;
    host.world
        .actors
        .get_mut(&id)
        .and_then(|a| a.animation.as_mut())
        .ok_or_else(|| "actor animation is missing".into())
}
fn sound(host: &mut FieldHost<'_>, id: i32, volume: i32) -> Result<NativeResult, String> {
    let id = i16::try_from(id).map_err(|_| "invalid field sound")?;
    let volume = u8::try_from(volume)
        .ok()
        .filter(|v| *v <= 127)
        .ok_or("invalid field sound volume")?;
    if host.world.audio_commands.len() >= 512 {
        return Err("audio command queue is not being consumed".into());
    }
    host.world.audio_commands.push(crate::AudioCommand::Sound {
        id,
        pan: 64,
        volume,
        slot: None,
    });
    Ok(NativeResult::Continue(None))
}
pub(super) const fn register(
    bindings: NativeBindings<FieldHost<'_>>,
) -> NativeBindings<FieldHost<'_>> {
    bindings
        .register_authored(
            "game::treasure::actor",
            &[CHEST],
            Some(ACTOR),
            false,
            |h, a, _| {
                let id = chest(h, a[0])?.actor;
                Ok(NativeResult::Continue(Some(h.world.authored_actor(id)?)))
            },
        )
        .register_authored(
            "game::treasure::kind",
            &[CHEST],
            Some(KIND),
            false,
            |h, a, _| Ok(NativeResult::Continue(Some(chest(h, a[0])?.kind as i32))),
        )
        .register_authored(
            "game::treasure::reward",
            &[CHEST],
            Some(REWARD),
            false,
            |h, a, _| {
                Ok(NativeResult::Values(match chest(h, a[0])?.reward {
                    TreasureReward::Item(item) => vec![RewardTag::Item as i32, i32::from(item)],
                    TreasureReward::Gald(amount) => vec![RewardTag::Gald as i32, i32::from(amount)],
                }))
            },
        )
        .register_authored(
            "game::treasure::mark_opened",
            &[CHEST],
            None,
            false,
            |h, a, _| {
                let flag = chest(h, a[0])?.flag;
                h.world
                    .party
                    .as_mut()
                    .ok_or("party is missing")?
                    .travel
                    .opened_treasures
                    .insert(flag);
                Ok(NativeResult::Continue(None))
            },
        )
        .register_authored(
            "game::actors::animate",
            &[ACTOR, Type::F32, Type::Bool],
            None,
            false,
            |h, a, _| {
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
            },
        )
        .register_authored(
            "game::actors::animation_finished",
            &[ACTOR],
            Some(Type::Bool),
            false,
            |h, a, _| {
                let tick = h.world.tick;
                let clip = animation(h, a[0])?;
                let done = if clip.rate < 0. {
                    clip.elapsed(tick, 0) <= 0.
                } else {
                    clip.elapsed(tick, 0) >= clip.duration_ticks as f32
                };
                Ok(NativeResult::Continue(Some(i32::from(done))))
            },
        )
        .register_authored(
            "game::actors::stop_animation",
            &[ACTOR],
            None,
            false,
            |h, a, _| {
                let tick = h.world.tick;
                let clip = animation(h, a[0])?;
                let frame = clip.elapsed(tick, 0).clamp(0., clip.duration_ticks as f32);
                clip.seek(frame, tick);
                clip.rate = 0.;
                Ok(NativeResult::Continue(None))
            },
        )
        .register_authored(
            "game::audio::sound",
            &[Type::I32],
            None,
            false,
            |h, a, _| sound(h, a[0], 127),
        )
        .register_authored(
            "game::party::give_item",
            &[ITEM, Type::I32],
            Some(Type::Bool),
            false,
            |h, a, _| {
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
            },
        )
        .register_authored(
            "game::party::give_gald",
            &[Type::I32],
            Some(Type::Bool),
            false,
            |h, a, _| {
                if a[0] < 0 {
                    return Err("negative gald reward".into());
                }
                let party = h.world.party.as_mut().ok_or("party is missing")?;
                let received = party.gald < 99_999_999;
                if received {
                    party.add_gald(a[0]);
                }
                Ok(NativeResult::Continue(Some(i32::from(received))))
            },
        )
        .register_authored(
            "game::field::instant_notice",
            &[Type::Message],
            None,
            true,
            |h, a, _| h.notice(a, crate::dialogue::flags::INSTANT),
        )
}
