//! Ordinary damage/guard from fn_1_61578 and HP application from fn_1_1D864.
//! The physical-arte EX bonus is prepared on the actor; other equipment/status
//! modifiers and actor reactions remain separate work.
use crate::{Actor, GuardKind, GuardResult, GuardRule, guard, state::Random};
pub use resonance_content::menu_data::Element;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HitElement {
    #[default]
    Inherited,
    Neutral,
    Element(Element),
}

/// Live attack-element sources, in fn_1_20704 priority order. The base is the
/// equipped weapon's element for party actors and the profile's for enemies.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AttackElements {
    pub action: Option<Element>,
    pub enchantment: Option<Element>,
    pub base: Option<Element>,
}

impl HitElement {
    pub(crate) fn resolve(self, actor: &Actor) -> Option<Element> {
        match self {
            Self::Inherited => actor
                .elements
                .action
                .or(actor.elements.enchantment)
                .or(actor.elements.base),
            Self::Neutral => None,
            Self::Element(element) => Some(element),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CombatStats {
    pub slash: i16,
    pub thrust: i16,
    pub defense: i16,
    pub intelligence: i16,
    pub accuracy: i16,
    pub evasion: i16,
    pub level: u8,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Affinity {
    #[default]
    Normal,
    Weak,
    Resistant,
    Absorb,
    Immune,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageKind {
    Slash,
    Thrust,
    Magic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Power {
    Normal,
    Percent(u16),
    Fixed(u16),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImpactEffect {
    pub appearance: crate::EffectAppearance,
    /// Original hit flag 0x800 permits the authored effect on guarded contacts.
    pub on_guard: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HitRule {
    pub kind: DamageKind,
    /// Arte contacts do not restore the attacker's TP. The physical damage
    /// branch also uses this classification for the prepared EX bonus.
    pub arte: bool,
    pub power: Power,
    /// Inherited elements are resolved from the live owner at each contact.
    pub element: HitElement,
    pub prevents_defeat: bool,
    pub guard: GuardRule,
    pub reaction: crate::ReactionRule,
    pub impact: Option<ImpactEffect>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HitResult {
    /// Full computed amount, before the original signed-16-bit HP argument.
    pub amount: i32,
    /// Actual HP change: negative for damage, positive for healing.
    pub hp_change: i32,
    pub critical: bool,
    pub affinity: Affinity,
    pub guard: GuardResult,
    /// Automatic selection can succeed even when the hit subsequently breaks guard.
    pub auto_guard: bool,
    /// This contact advanced the target's armor counter without interrupting it.
    pub armored: bool,
    pub protection: crate::HitProtection,
}

fn power(value: i32, rule: Power) -> i32 {
    match rule {
        Power::Normal => value,
        Power::Percent(percent) => value.wrapping_mul(i32::from(percent)) / 100,
        Power::Fixed(amount) => i32::from(amount),
    }
}

pub(crate) fn resolve(
    owner: &Actor,
    target: &mut Actor,
    rule: HitRule,
    attack_power: u16,
    incoming: [f32; 3],
    random: &mut Random,
) -> HitResult {
    let hp_before = target.hp;
    let mut critical = false;
    let mut auto_guard = false;
    let element = rule.element.resolve(owner);
    let affinity = target.affinities[element.map_or(0, |e| e as usize + 1)];
    let mut amount = match rule.kind {
        DamageKind::Slash | DamageKind::Thrust => {
            if !rule.guard.enabled {
                target.guard.active = false;
            }
            let attack = i32::from(match rule.kind {
                DamageKind::Slash => owner.stats.slash,
                _ => owner.stats.thrust,
            });
            let base = ((attack >> 1) - i32::from(target.stats.defense)).max(1);
            let scaled = base.wrapping_mul(i32::from(attack_power)) / 100;
            let mut rolled = scaled.wrapping_mul(100 + i32::from(random.next() as i16) % 5) / 100;
            if rule.arte && owner.physical_arte_boost {
                rolled = rolled.wrapping_add(rolled.wrapping_mul(20) / 100);
            }
            // The original narrows after the arithmetic shift, before clamping.
            let accuracy = ((i32::from(owner.stats.accuracy) - i32::from(target.stats.evasion) - 5)
                >> 1) as i16;
            let mut value = rolled.wrapping_mul(90 + i32::from(accuracy.clamp(-10, 10))) / 100;
            let chance = (i32::from(owner.luck) - i32::from(target.luck)).max(0) / 20
                + 1
                + i32::from(random.next() & 3);
            critical = i32::from(random.next() % 100) < chance;
            if critical {
                // Original 61E3C shifts the already-truncated amount.
                value = value.wrapping_add(value >> 1);
            }
            auto_guard = guard::attempt(target, affinity, random);
            if matches!(rule.power, Power::Fixed(_)) {
                critical = false;
            }
            power(value, rule.power)
        }
        DamageKind::Magic => {
            let mut resistance = i32::from(target.stats.intelligence) >> 3;
            if target.stats.level != 0 {
                resistance += i32::from(random.next() % u16::from(target.stats.level));
            }
            // Spell power precedes resistance. The physical combo percentage
            // retained by a projectile does not scale this branch.
            let mut base = power(i32::from(owner.stats.intelligence), rule.power) - resistance;
            if target.guard.active && target.guard.kind == GuardKind::Normal {
                base >>= 1;
                target.guard.active = false;
            }
            base.wrapping_mul(100 + i32::from(random.next() as i16) % 5) / 100
        }
    };
    match affinity {
        Affinity::Weak => amount = amount.wrapping_add(amount >> 1),
        Affinity::Resistant => amount >>= 1,
        _ => {}
    }
    // 62A9C: test the old byte counter, then add with byte wrapping. This write
    // also happens on absorbed/immune contacts. Automatic guard has already run.
    let armor = &mut target.reaction.armor;
    let mut armored = !target.reaction.unflinching && armor.received < armor.threshold;
    if armored {
        armor.received = armor.received.wrapping_add(rule.reaction.armor_damage);
    }
    let (mut protection, shift) = target.reaction.protection.hit(
        target.side,
        target.reaction.stagger.window,
        rule.reaction.hits_down,
    );
    amount >>= shift;
    let mut guard = if armored || protection != crate::HitProtection::None {
        GuardResult::None
    } else {
        guard::resolve(
            target,
            rule.kind,
            rule.guard,
            affinity,
            incoming,
            &mut amount,
        )
    };
    if guard != GuardResult::None {
        critical = false;
    }
    if !target.reaction.unflinching
        && !armored
        && protection == crate::HitProtection::None
        && !matches!(affinity, Affinity::Absorb | Affinity::Immune)
        && !matches!(guard, GuardResult::Blocked { .. })
    {
        target.reaction.stagger.received = target
            .reaction
            .stagger
            .received
            .wrapping_add(rule.reaction.stagger);
    }
    if affinity != Affinity::Immune
        && (protection != crate::HitProtection::Avoided || affinity == Affinity::Absorb)
    {
        amount = amount.max(1);
        let delta = if affinity == Affinity::Absorb {
            amount as i16
        } else {
            amount.wrapping_neg() as i16
        };
        let before = target.hp;
        let cap = if target.recovery.weak {
            target.max_hp >> 1
        } else {
            target.max_hp
        };
        // fn_1_1D864 preserves HP already above the Weak cap, including signed
        // narrowing that turns a large damage request into positive recovery.
        if delta > 0 {
            if !target.recovery.weak || target.hp < cap {
                target.hp = target.hp.wrapping_add(i32::from(delta)).min(cap);
            }
        } else {
            target.hp = target.hp.wrapping_add(i32::from(delta)).max(0);
            if before <= cap {
                target.hp = target.hp.min(cap);
            }
        }
    }
    if rule.prevents_defeat {
        target.hp = target.hp.max(1);
    }
    if target.hp == 0 {
        target.guard.active = false;
        guard = GuardResult::None;
        critical = false;
        armored = false;
        protection = crate::HitProtection::None;
    }
    HitResult {
        amount,
        hp_change: target.hp.wrapping_sub(hp_before),
        critical,
        affinity,
        guard,
        auto_guard,
        armored,
        protection,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Side, tests::actor};

    fn rule(kind: DamageKind, power: Power) -> HitRule {
        HitRule {
            impact: None,
            arte: false,
            reaction: Default::default(),
            kind,
            power,
            element: crate::HitElement::Neutral,
            prevents_defeat: false,
            guard: GuardRule::default(),
        }
    }

    #[test]
    fn element_selection_preserves_explicit_neutral_and_override_priority() {
        for side in [Side::Party, Side::Enemy] {
            let mut owner = actor(side);
            assert_eq!(HitElement::Inherited.resolve(&owner), None);
            owner.elements.base = Some(Element::Fire);
            assert_eq!(HitElement::Inherited.resolve(&owner), Some(Element::Fire));
            owner.elements.enchantment = Some(Element::Water);
            assert_eq!(HitElement::Inherited.resolve(&owner), Some(Element::Water));
            owner.elements.action = Some(Element::Wind);
            assert_eq!(HitElement::Inherited.resolve(&owner), Some(Element::Wind));
            assert_eq!(HitElement::Neutral.resolve(&owner), None);
            assert_eq!(
                HitElement::Element(Element::Ice).resolve(&owner),
                Some(Element::Ice)
            );
            owner.elements.action = None;
            owner.elements.enchantment = None;
            assert_eq!(HitElement::Inherited.resolve(&owner), Some(Element::Fire));
        }
    }

    #[test]
    fn armor_tests_the_old_counter_and_suppresses_guard_and_hurt_without_reducing_damage() {
        let owner = actor(Side::Party);
        let mut target = actor(Side::Enemy);
        target.reaction.armor.threshold = 2;
        target.guard.active = true;
        target.guard.break_pressure = 10;
        target.guard.reduction = 75;
        let mut rule = rule(DamageKind::Slash, Power::Fixed(8));
        rule.reaction.armor_damage = 1;
        let mut random = Random(1);
        for received in 1..=2 {
            let hit = resolve(&owner, &mut target, rule, 100, [0.; 3], &mut random);
            assert_eq!(target.reaction.armor.received, received);
            assert!(hit.armored);
            assert_eq!(hit.hp_change, -8);
            assert_eq!(hit.guard, GuardResult::None);
            assert!(target.guard.active);
            assert!(crate::reaction::respond(&mut target, rule.reaction, hit, [0.; 3]).is_none());
            assert_eq!(target.reaction.combo_hits, 0);
        }
        let hit = resolve(&owner, &mut target, rule, 100, [0.; 3], &mut random);
        assert!(!hit.armored);
        assert_eq!(target.reaction.armor.received, 2);
        assert_eq!(hit.hp_change, -2);
        assert!(matches!(hit.guard, GuardResult::Blocked { .. }));
        let mut expected_random = Random(1);
        for _ in 0..9 {
            expected_random.next();
        }
        assert_eq!(random.0, expected_random.0); // Armor adds no RNG draw.
    }

    #[test]
    fn armor_wraps_on_immune_contacts_but_unflinching_and_lethal_hits_keep_their_own_gates() {
        let owner = actor(Side::Party);
        let mut rule = rule(DamageKind::Slash, Power::Fixed(8));
        rule.reaction.armor_damage = 10;
        for affinity in [Affinity::Immune, Affinity::Absorb, Affinity::Normal] {
            let mut target = actor(Side::Enemy);
            target.affinities[0] = affinity;
            target.reaction.armor.threshold = 255;
            target.reaction.armor.received = 250;
            let hit = resolve(&owner, &mut target, rule, 100, [0.; 3], &mut Random(1));
            assert!(hit.armored);
            assert_eq!(target.reaction.armor.received, 4);
            target.reaction.unflinching = true;
            let hit = resolve(&owner, &mut target, rule, 100, [0.; 3], &mut Random(1));
            assert!(!hit.armored);
            assert_eq!(target.reaction.armor.received, 4);
        }
        let mut target = actor(Side::Enemy);
        target.hp = 8;
        target.reaction.armor.threshold = 20;
        let hit = resolve(&owner, &mut target, rule, 100, [0.; 3], &mut Random(1));
        assert_eq!(target.hp, 0);
        assert!(!hit.armored); // The original lethal result replaces its earlier flags.
        assert_eq!(target.reaction.armor.received, 10);
    }

    #[test]
    fn automatic_guard_roll_precedes_armor_and_zero_armor_damage_keeps_the_threshold() {
        let owner = actor(Side::Party);
        let mut target = actor(Side::Enemy);
        target.control = crate::Control::Enemy;
        target.activity = crate::Activity::Action {
            clock: 1,
            guard_window: [0, 2],
        };
        target.guard.enemy_chance = 100;
        target.guard.break_pressure = 10;
        target.reaction.armor.threshold = 1;
        let hit = rule(DamageKind::Slash, Power::Fixed(8));
        let mut random = Random(1);
        let result = resolve(&owner, &mut target, hit, 100, [0.; 3], &mut random);
        assert!(result.auto_guard && result.armored);
        assert_eq!(result.guard, GuardResult::None);
        assert_eq!(result.hp_change, -8);
        assert_eq!(target.reaction.armor.received, 0);
        assert!(target.guard.active);
        let mut expected = Random(1);
        for _ in 0..4 {
            expected.next();
        }
        assert_eq!(random.0, expected.0);
    }

    #[test]
    fn contact_element_selection_matches_original_opening_calls() {
        let trace: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/opening-element-selection.json"
        ))
        .unwrap();
        let element = |value: &serde_json::Value| {
            let index = value.as_u64().unwrap() as usize;
            index.checked_sub(1).map(|i| Element::ALL[i])
        };
        for row in trace["observations"].as_array().unwrap() {
            let mut owner = actor(if row["side"] == "enemy" {
                Side::Enemy
            } else {
                Side::Party
            });
            owner.elements = AttackElements {
                action: element(&row["action"]),
                enchantment: element(&row["enchantment"]),
                base: element(&row["base"]),
            };
            let rule = match row["rule"].as_u64().unwrap() {
                0 => HitElement::Inherited,
                10 => HitElement::Neutral,
                _ => HitElement::Element(element(&row["rule"]).unwrap()),
            };
            assert_eq!(rule.resolve(&owner), element(&row["selected"]), "{row}");
        }
    }

    fn read_actor(row: &serde_json::Value, side: Side) -> Actor {
        let mut a = actor(side);
        a.max_hp = row["max_hp"].as_i64().unwrap() as i32;
        a.hp = row["hp"].as_i64().unwrap() as i32;
        a.luck = row["luck"].as_u64().unwrap() as u8;
        let stats = &row["stats"];
        let value = |key: &str| stats[key].as_i64().unwrap() as i16;
        a.stats = CombatStats {
            slash: value("slash"),
            thrust: value("thrust"),
            defense: value("defense"),
            intelligence: value("intelligence"),
            accuracy: value("accuracy"),
            evasion: value("evasion"),
            level: value("level") as u8,
        };
        a
    }

    #[test]
    fn ordinary_damage_hp_and_rng_match_original_opening_calls() {
        let trace: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/opening-damage.json")).unwrap();
        for row in trace["observations"].as_array().unwrap() {
            let owner = read_actor(&row["owner"], Side::Party);
            let mut target = read_actor(&row["target"], Side::Enemy);
            target.affinities[0] = Affinity::Resistant;
            let before = target.hp;
            let mut random = Random(row["random_before"].as_u64().unwrap() as u32);
            let result = resolve(
                &owner,
                &mut target,
                rule(DamageKind::Slash, Power::Normal),
                row["power"].as_u64().unwrap() as u16,
                [0.; 3],
                &mut random,
            );
            assert_eq!(
                result.amount,
                row["amount"].as_i64().unwrap() as i32,
                "{row}"
            );
            assert_eq!(target.hp, row["hp_after"].as_i64().unwrap() as i32);
            assert_eq!(result.hp_change, target.hp - before);
            assert!(!result.critical);
            assert_eq!(random.0, row["random_after"].as_u64().unwrap() as u32);
        }
    }

    #[test]
    fn physical_critical_precedes_power_and_fixed_power_still_consumes_the_rolls() {
        let mut owner = actor(Side::Party);
        owner.stats.slash = 200;
        owner.stats.accuracy = 100;
        let mut target = actor(Side::Enemy);
        target.hp = 1000;
        target.max_hp = 1000;
        target.stats.defense = 10;
        target.stats.evasion = 85;
        let mut random = Random(32);
        let result = resolve(
            &owner,
            &mut target,
            rule(DamageKind::Slash, Power::Percent(150)),
            100,
            [0.; 3],
            &mut random,
        );
        assert_eq!(result.amount, 190); // 85 + (85 >> 1), then 150 percent.
        assert!(result.critical);
        assert_eq!(random.0, 0x74cc1a01);
        let mut random = Random(32);
        let result = resolve(
            &owner,
            &mut target,
            rule(DamageKind::Thrust, Power::Fixed(40)),
            100,
            [0.; 3],
            &mut random,
        );
        assert_eq!(result.amount, 40);
        assert!(!result.critical);
        assert_eq!(random.0, 0x74cc1a01);
    }

    #[test]
    fn physical_arte_bonus_precedes_accuracy_critical_and_power_without_extra_draws() {
        let mut owner = actor(Side::Party);
        owner.stats.slash = 200;
        owner.stats.thrust = 200;
        owner.stats.accuracy = 100;
        owner.stats.intelligence = 80;
        let mut defender = actor(Side::Enemy);
        defender.hp = 1000;
        defender.max_hp = 1000;
        defender.stats.defense = 10;
        defender.stats.evasion = 85;
        for kind in [DamageKind::Slash, DamageKind::Thrust, DamageKind::Magic] {
            for power in [Power::Percent(150), Power::Fixed(40)] {
                for (arte, boost) in [(false, false), (false, true), (true, false), (true, true)] {
                    owner.physical_arte_boost = boost;
                    let mut target = defender.clone();
                    let mut random = Random(32);
                    let mut hit = rule(kind, power);
                    hit.arte = arte;
                    let result = resolve(&owner, &mut target, hit, 100, [0.; 3], &mut random);
                    let expected = match (kind, power) {
                        (DamageKind::Magic, Power::Fixed(_)) => 40,
                        (DamageKind::Magic, _) => 121,
                        (_, Power::Fixed(_)) => 40,
                        _ if arte && boost => 229, // 90 -> 108 -> 102 -> 153 -> 229.
                        _ => 190,                  // 90 -> 85 -> 127 -> 190.
                    };
                    assert_eq!(result.amount, expected, "{kind:?} {power:?} {arte} {boost}");
                    assert_eq!(target.hp, 1000 - expected);
                    assert_eq!(
                        random.0,
                        if kind == DamageKind::Magic {
                            0x38dc_a427
                        } else {
                            0x74cc_1a01
                        }
                    );
                }
            }
        }
    }

    #[test]
    fn guarded_and_unguarded_opening_hits_match_dolphin_state_and_rng() {
        use crate::{Activity, Control, Guard};
        let trace: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/opening-guard.json")).unwrap();
        for row in trace["observations"].as_array().unwrap() {
            let owner = read_actor(&row["owner"], Side::Party);
            let mut target = read_actor(&row["target"], Side::Enemy);
            let t = &row["target"];
            let g = &t["guard"];
            target.control = Control::Enemy;
            target.activity = match t["activity"].as_str().unwrap() {
                "idle" => Activity::Idle,
                "approaching" => Activity::Approaching,
                "action" => Activity::Action {
                    clock: t["clock"].as_i64().unwrap() as i16,
                    guard_window: std::array::from_fn(|i| {
                        t["guard_window"][i].as_i64().unwrap() as i16
                    }),
                },
                "guarding" => Activity::Guarding,
                "hurt" => Activity::Hurt,
                other => panic!("unexpected fixture activity {other}"),
            };
            target.position = std::array::from_fn(|i| {
                f32::from_bits(t["position_bits"][i].as_u64().unwrap() as u32)
            });
            target.heading = f32::from_bits(t["heading_bits"].as_u64().unwrap() as u32);
            target.facing_direction = crate::control::direction_from_heading(target.heading);
            target.guard = Guard {
                active: g["active"].as_bool().unwrap(),
                kind: if g["special"].as_bool().unwrap() {
                    GuardKind::Special
                } else {
                    GuardKind::Normal
                },
                pressure: g["pressure"].as_u64().unwrap() as u8,
                break_pressure: g["break_pressure"].as_i64().unwrap() as i16,
                reduction: g["reduction"].as_u64().unwrap() as u8,
                auto_chance: 0,
                enemy_chance: g["auto_chance"].as_i64().unwrap() as i8,
                recovery_bonus: 0,
                allow_airborne: g["allow_airborne"].as_bool().unwrap(),
                auto_disabled: g["auto_disabled"].as_bool().unwrap(),
                recently_hurt: g["recently_hurt"].as_bool().unwrap(),
            };
            target.affinities[0] = match row["affinity"].as_str().unwrap() {
                "normal" => Affinity::Normal,
                "resistant" => Affinity::Resistant,
                other => panic!("unexpected fixture affinity {other}"),
            };
            let mut hit = rule(
                DamageKind::Slash,
                if row["power_mode"] == "percent" {
                    Power::Percent(row["power_value"].as_u64().unwrap() as u16)
                } else {
                    Power::Normal
                },
            );
            let gr = &row["guard_rule"];
            target.reaction.stagger.received = row["stagger"]["before"].as_u64().unwrap() as u8;
            target.reaction.stagger.threshold = row["stagger"]["threshold"].as_u64().unwrap() as u8;
            hit.reaction.stagger = row["stagger"]["amount"].as_u64().unwrap() as u8;
            hit.guard = GuardRule {
                enabled: gr["enabled"].as_bool().unwrap(),
                pressure: gr["pressure"].as_u64().unwrap() as u8,
                breaks: gr["breaks"].as_bool().unwrap(),
                unbreakable: gr["unbreakable"].as_bool().unwrap(),
            };
            let before = target.hp;
            let mut random = Random(row["random_before"].as_u64().unwrap() as u32);
            let result = resolve(
                &owner,
                &mut target,
                hit,
                row["power"].as_u64().unwrap() as u16,
                std::array::from_fn(|i| {
                    f32::from_bits(row["hit_direction_bits"][i].as_u64().unwrap() as u32)
                }),
                &mut random,
            );
            let flags = row["result"].as_u64().unwrap();
            assert_eq!(
                u64::from(target.reaction.stagger.received),
                row["stagger"]["after"].as_u64().unwrap()
            );
            let expected_guard = if flags & 0x400 != 0 {
                GuardResult::Broken
            } else if flags & 0x10 != 0 {
                GuardResult::Blocked {
                    first: flags & 0x8000 != 0,
                    special: flags & 0x10000 != 0,
                }
            } else {
                GuardResult::None
            };
            assert_eq!(
                result.amount,
                row["amount"].as_i64().unwrap() as i32,
                "observation {}",
                row["index"]
            );
            assert_eq!(target.hp, row["hp_after"].as_i64().unwrap() as i32);
            assert_eq!(result.hp_change, target.hp - before);
            assert_eq!(result.critical, flags & 0x200 != 0);
            assert_eq!(result.guard, expected_guard);
            assert_eq!(
                target.guard.active,
                row["guard_after"]["active"].as_bool().unwrap()
            );
            assert_eq!(
                target.guard.pressure,
                row["guard_after"]["pressure"].as_u64().unwrap() as u8
            );
            assert_eq!(random.0, row["random_after"].as_u64().unwrap() as u32);
        }
    }

    #[test]
    fn magic_consumes_normal_guard_before_variation_but_special_guard_after_affinity() {
        let mut owner = actor(Side::Party);
        owner.stats.intelligence = 203;
        for (kind, amount, active, result) in [
            (GuardKind::Normal, 103, false, GuardResult::None),
            (
                GuardKind::Special,
                41,
                true,
                GuardResult::Blocked {
                    first: false,
                    special: true,
                },
            ),
        ] {
            let mut target = actor(Side::Enemy);
            target.hp = 1000;
            target.max_hp = 1000;
            target.guard.active = true;
            target.guard.kind = kind;
            let mut random = Random(1);
            let hit = resolve(
                &owner,
                &mut target,
                rule(DamageKind::Magic, Power::Normal),
                100,
                [0.; 3],
                &mut random,
            );
            assert_eq!(hit.amount, amount);
            assert_eq!(hit.guard, result);
            assert_eq!(target.guard.active, active);
            assert_eq!(target.guard.pressure, 0);
            assert_eq!(random.next(), 48001); // One signed variation draw, no auto-guard roll.
        }
    }

    #[test]
    fn unguardable_physical_hit_can_select_auto_guard_then_clear_it_without_reduction() {
        let owner = actor(Side::Enemy);
        let mut target = actor(Side::Party);
        target.control = crate::Control::Auto;
        target.guard.active = true;
        target.guard.auto_chance = 100;
        let mut rule = rule(DamageKind::Slash, Power::Fixed(20));
        rule.guard.enabled = false;
        let mut random = Random(1);
        let hit = resolve(&owner, &mut target, rule, 100, [0.; 3], &mut random);
        assert!(hit.auto_guard);
        assert_eq!((hit.amount, hit.guard), (20, GuardResult::None));
        assert!(!target.guard.active);
        let mut expected = Random(1);
        for _ in 0..4 {
            expected.next();
        }
        assert_eq!(random.0, expected.0);
    }

    #[test]
    fn magic_scales_before_resistance_and_only_draws_for_nonzero_level() {
        let mut owner = actor(Side::Party);
        owner.stats.intelligence = 200;
        let mut target = actor(Side::Enemy);
        target.max_hp = 1000;
        target.hp = 1000;
        target.stats.intelligence = 80;
        target.stats.level = 10;
        let mut random = Random(1);
        let result = resolve(
            &owner,
            &mut target,
            rule(DamageKind::Magic, Power::Percent(150)),
            10,
            [0.; 3],
            &mut random,
        );
        assert_eq!(result.amount, 283);
        assert_eq!(random.next(), 58770); // Two consumed, no physical critical rolls.
        target.stats.level = 0;
        let mut random = Random(1);
        let result = resolve(
            &owner,
            &mut target,
            rule(DamageKind::Magic, Power::Percent(150)),
            200,
            [0.; 3],
            &mut random,
        );
        assert_eq!(result.amount, 295);
        assert_eq!(random.next(), 48001);
    }

    #[test]
    fn affinity_floor_caps_nonlethal_and_signed_hp_arguments() {
        let owner = actor(Side::Party);
        for (affinity, expected, change) in [
            (Affinity::Normal, 21, -21),
            (Affinity::Weak, 31, -31),
            (Affinity::Resistant, 10, -10),
            (Affinity::Absorb, 21, 21),
            (Affinity::Immune, 21, 0),
        ] {
            let mut target = actor(Side::Enemy);
            target.affinities[0] = affinity;
            let hit = resolve(
                &owner,
                &mut target,
                rule(DamageKind::Slash, Power::Fixed(21)),
                100,
                [0.; 3],
                &mut Random(1),
            );
            assert_eq!((hit.amount, hit.hp_change), (expected, change));
        }
        let mut target = actor(Side::Enemy);
        let mut hit = rule(DamageKind::Slash, Power::Fixed(65535));
        assert_eq!(
            resolve(&owner, &mut target, hit, 100, [0.; 3], &mut Random(1)).hp_change,
            1
        );
        target.recovery.weak = true;
        assert_eq!(
            resolve(&owner, &mut target, hit, 100, [0.; 3], &mut Random(1)).hp_change,
            0
        );
        target.affinities[0] = Affinity::Absorb;
        assert_eq!(
            resolve(&owner, &mut target, hit, 100, [0.; 3], &mut Random(1)).hp_change,
            -1
        );
        target.affinities[0] = Affinity::Normal;
        hit.power = Power::Fixed(1000);
        hit.prevents_defeat = true;
        resolve(&owner, &mut target, hit, 100, [0.; 3], &mut Random(1));
        assert_eq!(target.hp, 1);
        hit.power = Power::Fixed(0);
        hit.prevents_defeat = false;
        assert_eq!(
            resolve(&owner, &mut target, hit, 100, [0.; 3], &mut Random(1)).amount,
            1
        );
        assert_eq!(target.hp, 0);
    }
}
