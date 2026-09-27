//! Scenario scripts own dungeon puzzle responses.
use super::*;

#[repr(i32)]
enum HitTag {
    Actor,
    Pulse,
}

#[repr(i32)]
enum AbilityTag {
    Disabled,
    Fire,
    AlternateFire,
    Shrink,
    Mana,
    ElectricOrb,
    Radar,
    Water,
    Wind,
    LongRangeFire,
    Sunlight,
    Bomb,
    Lightning,
    Ice,
    Earthquake,
    Darkness,
    Sound,
    AnimalCall,
    Bubble,
}

const ELECTRICORB: Type = Type::Enum {
    name: "game::ring::ElectricOrbKind",
    variants: &[variant("Sylvarant", 0, &[]), variant("Tethealla", 1, &[])],
};
const LIGHTNING: Type = Type::Enum {
    name: "game::ring::LightningColor",
    variants: &[
        variant("Blue", 0, &[]),
        variant("Yellow", 1, &[]),
        variant("Red", 2, &[]),
    ],
};
const ANIMALCALL: Type = Type::Enum {
    name: "game::ring::CallColor",
    variants: &[
        variant("Pink", 0, &[]),
        variant("White", 1, &[]),
        variant("Blue", 2, &[]),
    ],
};
const BUBBLE: Type = Type::Enum {
    name: "game::ring::BubblePhase",
    variants: &[variant("Release", 0, &[]), variant("Float", 1, &[])],
};
const ABILITY: Type = Type::Enum {
    name: "game::ring::Ability",
    variants: &[
        variant("Disabled", AbilityTag::Disabled as i32, &[]),
        variant("Fire", AbilityTag::Fire as i32, &[]),
        variant("AlternateFire", AbilityTag::AlternateFire as i32, &[]),
        variant("Shrink", AbilityTag::Shrink as i32, &[]),
        variant("Mana", AbilityTag::Mana as i32, &[]),
        variant(
            "ElectricOrb",
            AbilityTag::ElectricOrb as i32,
            &[ELECTRICORB],
        ),
        variant("Radar", AbilityTag::Radar as i32, &[]),
        variant("Water", AbilityTag::Water as i32, &[]),
        variant("Wind", AbilityTag::Wind as i32, &[]),
        variant("LongRangeFire", AbilityTag::LongRangeFire as i32, &[]),
        variant("Sunlight", AbilityTag::Sunlight as i32, &[]),
        variant("Bomb", AbilityTag::Bomb as i32, &[]),
        variant("Lightning", AbilityTag::Lightning as i32, &[LIGHTNING]),
        variant("Ice", AbilityTag::Ice as i32, &[]),
        variant("Earthquake", AbilityTag::Earthquake as i32, &[]),
        variant("Darkness", AbilityTag::Darkness as i32, &[]),
        variant("Sound", AbilityTag::Sound as i32, &[]),
        variant("AnimalCall", AbilityTag::AnimalCall as i32, &[ANIMALCALL]),
        variant("Bubble", AbilityTag::Bubble as i32, &[BUBBLE]),
    ],
};

const HIT: Type = Type::Enum {
    name: "game::ring::Hit",
    variants: &[
        variant(
            "Actor",
            HitTag::Actor as i32,
            &[Type::Handle("game::actors::Actor")],
        ),
        variant("Pulse", HitTag::Pulse as i32, &[]),
    ],
};

const PLAYER_SIZE: Type = Type::Enum {
    name: "game::field::PlayerSize",
    variants: &[variant("Normal", 0, &[]), variant("Small", 1, &[])],
};

pub(super) const fn register(
    bindings: NativeBindings<FieldHost<'_>>,
) -> NativeBindings<FieldHost<'_>> {
    bindings
        .function("game::ring::end", &[], None, true, |host, _, _| {
            host.call_event(
                0,
                crate::ring::SECONDARY_CALLBACK,
                crate::ring::Hit::Pulse.event_actor(),
            )
        })
        .function(
            "game::ring::insufficient_tp",
            &[],
            None,
            true,
            |host, _, _| {
                host.call_event(
                    0,
                    crate::ring::SECONDARY_CALLBACK,
                    crate::ring::Hit::Pulse.event_actor(),
                )
            },
        )
        .function(
            "game::field::player_size",
            &[],
            Some(PLAYER_SIZE),
            false,
            |host, _, _| {
                Ok(NativeResult::Continue(Some(match host.world.player_size {
                    crate::world::PlayerSize::Normal => 0,
                    crate::world::PlayerSize::Small => 1,
                })))
            },
        )
        .function(
            "game::field::set_player_size",
            &[PLAYER_SIZE],
            None,
            false,
            |host, a, _| {
                host.world.player_size = match a[0] {
                    0 => crate::world::PlayerSize::Normal,
                    1 => crate::world::PlayerSize::Small,
                    _ => return Err("invalid player size".into()),
                };
                Ok(NativeResult::Continue(None))
            },
        )
        .function(
            "game::ring::hit",
            &[HIT],
            None,
            true,
            |host, arguments, _| {
                let hit = match arguments[0] {
                    tag if tag == HitTag::Actor as i32 => {
                        crate::ring::Hit::Actor(host.actor_id(arguments[1])? as i16)
                    }
                    tag if tag == HitTag::Pulse as i32 => crate::ring::Hit::Pulse,
                    _ => return Err("invalid ring hit".into()),
                };
                host.call_event(0, crate::ring::CALLBACK, hit.event_actor())
            },
        )
        .function(
            "game::ring::ability",
            &[],
            Some(ABILITY),
            false,
            |host, _, _| {
                use crate::ring::{
                    BubblePhase, CallColor, ElectricOrbKind, LightningColor, SorcerersRing as R,
                };
                let ability = host
                    .world
                    .party
                    .as_ref()
                    .ok_or("ring party is missing")?
                    .travel
                    .sorcerers_ring;
                let (tag, variant) = match ability {
                    R::Disabled => (AbilityTag::Disabled, 0),
                    R::Fire => (AbilityTag::Fire, 0),
                    R::AlternateFire => (AbilityTag::AlternateFire, 0),
                    R::Shrink => (AbilityTag::Shrink, 0),
                    R::Mana => (AbilityTag::Mana, 0),
                    R::ElectricOrb(ElectricOrbKind::Sylvarant) => (AbilityTag::ElectricOrb, 0),
                    R::ElectricOrb(ElectricOrbKind::Tethealla) => (AbilityTag::ElectricOrb, 1),
                    R::Radar => (AbilityTag::Radar, 0),
                    R::Water => (AbilityTag::Water, 0),
                    R::Wind => (AbilityTag::Wind, 0),
                    R::LongRangeFire => (AbilityTag::LongRangeFire, 0),
                    R::Sunlight => (AbilityTag::Sunlight, 0),
                    R::Bomb => (AbilityTag::Bomb, 0),
                    R::Lightning(LightningColor::Blue) => (AbilityTag::Lightning, 0),
                    R::Lightning(LightningColor::Yellow) => (AbilityTag::Lightning, 1),
                    R::Lightning(LightningColor::Red) => (AbilityTag::Lightning, 2),
                    R::Ice => (AbilityTag::Ice, 0),
                    R::Earthquake => (AbilityTag::Earthquake, 0),
                    R::Darkness => (AbilityTag::Darkness, 0),
                    R::Sound => (AbilityTag::Sound, 0),
                    R::AnimalCall(CallColor::Pink) => (AbilityTag::AnimalCall, 0),
                    R::AnimalCall(CallColor::White) => (AbilityTag::AnimalCall, 1),
                    R::AnimalCall(CallColor::Blue) => (AbilityTag::AnimalCall, 2),
                    R::Bubble(BubblePhase::Release) => (AbilityTag::Bubble, 0),
                    R::Bubble(BubblePhase::Float) => (AbilityTag::Bubble, 1),
                };
                Ok(NativeResult::Values(vec![tag as i32, variant]))
            },
        )
}
