//! Shared physical contact defaults; each attack supplies its own power and reactions.
use resonance_battle::{
    DamageKind, GuardRule, HitElement, HitRule, Power, ReactionRule, RecoilRule,
};

pub(super) fn physical(power: u16) -> HitRule {
    HitRule {
        kind: DamageKind::Slash,
        arte: false,
        overlimit_pause: true,
        power: Power::Percent(power),
        element: HitElement::Inherited,
        prevents_defeat: false,
        guard: GuardRule {
            pressure: 1,
            ..Default::default()
        },
        reaction: ReactionRule {
            recoil: RecoilRule {
                impulse: [4., 0.],
                ..Default::default()
            },
            hitstun: 12,
            armor_damage: 1,
            stagger: 1,
            ..Default::default()
        },
        condition: None,
    }
}
