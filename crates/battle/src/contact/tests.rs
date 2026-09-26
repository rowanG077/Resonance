use super::*;
use crate::{
    Activity, Affinity, Battle, CombatStats, Control, DamageKind, Guard, GuardKind, GuardResult,
    GuardRule, HitElement, HurtPoint, Power, ProjectileDefinition, state::Random,
};
use serde_json::Value;
use std::sync::Arc;

#[test]
fn live_combo_only_tracks_the_leading_party_member_and_selected_target() -> Result<()> {
    for automatic in [false, true] {
        for victim in 0..4 {
            let actors = [Side::Party, Side::Party, Side::Enemy, Side::Enemy].map(|side| {
                let mut actor = crate::tests::actor(side);
                actor.hp = 1000;
                actor.max_hp = 1000;
                actor.control = Control::Auto;
                actor
            });
            let mut battle = Battle::new(crate::tests::prepared(
                "pub task run() {}",
                actors.into(),
                1,
            ));
            if !automatic {
                battle.actors[1].control = Control::Manual;
            }
            let leader = usize::from(!automatic);
            battle.targets[leader] = ActorId(3);
            for (index, actor) in battle.actors.iter_mut().enumerate() {
                actor.body.points = vec![HurtPoint {
                    center: if index == victim { [0.; 3] } else { [100.; 3] },
                    radius: 1.,
                }];
            }
            battle.actors[victim].reaction.combo_hits = 1;
            battle.actors[victim].reaction.combo_damage = 17;
            battle.advance_hud(false);
            let owner = ActorId(if victim < 2 { 2 } else { 0 });
            let mut contacts = Contacts::default();
            contacts.push(
                battle.actors[owner.index()].side,
                Contact {
                    source: ContactSource::Melee {
                        actor: owner,
                        action: ActionId(1),
                    },
                    owner,
                    position: [0.; 3],
                    radius: 2.,
                    height: 2.,
                    shape: HitShape::Box,
                    rule: HitRule {
                        kind: DamageKind::Slash,
                        arte: true,
                        power: Power::Fixed(25),
                        element: HitElement::Neutral,
                        prevents_defeat: false,
                        reaction: Default::default(),
                        guard: Default::default(),
                        impact: None,
                    },
                    cooldown: 7,
                    clash: None,
                    used: false,
                },
            )?;
            let mut cues = vec![];
            contacts.resolve(&mut battle, &mut cues)?;
            let combo: Vec<_> = cues
                .iter()
                .filter_map(|cue| match cue {
                    Cue::Combo {
                        actor,
                        hits,
                        damage,
                    } => Some((*actor, *hits, *damage)),
                    _ => None,
                })
                .collect();
            let target = &battle.actors[victim];
            assert_eq!(target.reaction.combo_hits, 2);
            assert_eq!(target.hud.combo_tracking.hits, 1);
            assert_eq!(
                combo,
                if victim == leader || victim == 3 {
                    vec![(ActorId(victim as u8), 2, target.reaction.combo_damage)]
                } else {
                    vec![]
                },
                "automatic={automatic}, victim={victim}"
            );
        }
    }
    Ok(())
}

#[test]
fn only_lethal_contacts_enter_death_and_dispatch_common_feedback_before_impact() -> Result<()> {
    for lethal in [false, true] {
        let owner = crate::tests::actor(Side::Party);
        let mut target = crate::tests::actor(Side::Enemy);
        target.hp = if lethal { 1 } else { 1000 };
        target.max_hp = 1000;
        target.overlimit = 73;
        target.position = [1., 2., 3.];
        target.heading = 37.;
        target.facing_direction = crate::control::direction_from_heading(target.heading);
        target.body.center_offset = [6., 11., 14.];
        target.effect_scale = 2.5;
        target.body.points = vec![HurtPoint {
            center: [0.; 3],
            radius: 1.,
        }];
        let mut prepared = crate::tests::prepared("pub task run() {}", vec![owner, target], 1);
        Arc::get_mut(&mut prepared)
            .unwrap()
            .effects
            .insert(4, crate::tests::effect_binding(4, [14, 8]));
        let prepared =
            Arc::try_unwrap(prepared)
                .unwrap()
                .with_death_feedback(crate::DeathFeedback {
                    enemy: Some(crate::DeathEffect {
                        appearance: EffectAppearance {
                            resource: 4,
                            member: 14,
                        },
                        sound: crate::SoundBinding {
                            resource: 9,
                            index: 73,
                        },
                    }),
                    allies: vec![vec![None; 2]; 2],
                })?;
        let mut battle = Battle::new(Arc::new(prepared));
        let death_center = battle.actors[1].body.center; // Prepared, scaled and rotated by heading.
        let mut contacts = Contacts::default();
        contacts.push(
            Side::Party,
            Contact {
                source: ContactSource::Melee {
                    actor: ActorId(0),
                    action: ActionId(1),
                },
                owner: ActorId(0),
                position: [0.; 3],
                radius: 20.,
                height: 20.,
                shape: HitShape::Box,
                rule: HitRule {
                    kind: DamageKind::Slash,
                    arte: true,
                    power: Power::Fixed(100),
                    element: HitElement::Neutral,
                    prevents_defeat: false,
                    reaction: Default::default(),
                    guard: Default::default(),
                    impact: Some(crate::ImpactEffect {
                        appearance: EffectAppearance {
                            resource: 4,
                            member: 8,
                        },
                        on_guard: false,
                    }),
                },
                cooldown: 7,
                clash: None,
                used: false,
            },
        )?;
        let mut cues = vec![];
        contacts.resolve(&mut battle, &mut cues)?;
        let victim = &battle.actors[1];
        assert_eq!(victim.available(), !lethal);
        assert_eq!(victim.activity == Activity::Defeated, lethal);
        assert_eq!(victim.overlimit, if lethal { 0 } else { 73 });
        assert_eq!(battle.ledger.kills[0], u16::from(lethal));
        let feedback: Vec<_> = cues
            .iter()
            .filter_map(|cue| match cue {
                Cue::Effect { member, .. } => Some(*member),
                Cue::Sound { sound, .. } => Some(sound.index),
                _ => None,
            })
            .collect();
        assert_eq!(feedback, if lethal { vec![14, 73, 8] } else { vec![8] });
        if lethal {
            let Cue::Effect {
                position, heading, ..
            } = cues
                .iter()
                .find(|cue| matches!(cue, Cue::Effect { member: 14, .. }))
                .unwrap()
            else {
                unreachable!()
            };
            assert_eq!((*position, *heading), (death_center, 37.));
            assert!(cues.iter().any(|cue| matches!(
                cue,
                Cue::Sound {
                    actor: ActorId(1),
                    position: [1., 0., 3.],
                    priority: 1,
                    ..
                }
            )));
            let sequence = battle
                .sequences
                .values()
                .find(|s| s.actor == ActorId(1) && s.effect.is_some())
                .unwrap();
            assert_eq!(sequence.target, ActorId(1));
            assert_eq!(sequence.effect.as_ref().unwrap().scale, 2.5);
            assert!(matches!(
                sequence.effect.as_ref().unwrap().follow,
                Some(crate::effect::Follow::Center(ActorId(1)))
            ));
        }
        let count = cues.len();
        contacts.resolve(&mut battle, &mut cues)?;
        assert_eq!(cues.len(), count); // Dead availability or contact cooldown prevents another tail.
    }
    Ok(())
}

#[test]
fn ledger_keeps_guarded_party_contacts_and_only_party_killers() -> Result<()> {
    for (owner_side, guarded) in [
        (Side::Enemy, true),
        (Side::Enemy, false),
        (Side::Party, false),
    ] {
        let owner = crate::tests::actor(owner_side);
        let mut target = crate::tests::actor(if owner_side == Side::Party {
            Side::Enemy
        } else {
            Side::Party
        });
        target.hp = if guarded { 100 } else { 10 };
        target.max_hp = 100;
        target.heading = 180.;
        target.facing_direction = crate::control::direction_from_heading(target.heading);
        target.guard.active = guarded;
        target.guard.reduction = 75; // Keep the guarded contact nonlethal.
        target.guard.break_pressure = 31;
        target.reaction.combo_hits = 9;
        target.body.points = vec![HurtPoint {
            center: [0.; 3],
            radius: 2.,
        }];
        let mut battle = Battle::new(crate::tests::prepared(
            "pub task run() {}",
            vec![owner, target],
            1,
        ));
        let mut contacts = Contacts::default();
        contacts.push(
            owner_side,
            Contact {
                source: ContactSource::Melee {
                    actor: ActorId(0),
                    action: ActionId(1),
                },
                owner: ActorId(0),
                position: [0.; 3],
                radius: 20.,
                height: 20.,
                shape: HitShape::Box,
                rule: HitRule {
                    kind: DamageKind::Slash,
                    arte: true,
                    power: Power::Fixed(100),
                    element: HitElement::Neutral,
                    prevents_defeat: false,
                    reaction: Default::default(),
                    guard: GuardRule {
                        enabled: true,
                        pressure: 0,
                        breaks: false,
                        unbreakable: false,
                    },
                    impact: None,
                },
                cooldown: 7,
                clash: None,
                used: false,
            },
        )?;
        let mut cues = vec![];
        contacts.resolve(&mut battle, &mut cues)?;
        assert_eq!(battle.ledger().party_was_hit, owner_side == Side::Enemy);
        assert_eq!(
            cues.iter()
                .any(|cue| matches!(cue, Cue::Combo { hits: 10, .. })),
            !guarded,
        ); // The display snapshots the count before lethal initialization clears it.
        assert_eq!(
            battle.ledger().last_party_killer,
            (owner_side == Side::Party).then_some(ActorId(0))
        );
        assert_eq!(
            battle.ledger().title_events,
            if guarded {
                vec![]
            } else {
                vec![crate::TitleEvent {
                    character: 1,
                    title: 18,
                    eligible_actors: if owner_side == Side::Party { 1 } else { 2 },
                }]
            }
        ); // A8A8 samples availability before the lethal contact enters death.
        if guarded {
            assert!(cues.iter().any(|cue| matches!(
                cue,
                Cue::Hit {
                    result: crate::HitResult {
                        guard: GuardResult::Blocked { .. },
                        ..
                    },
                    ..
                }
            )));
            assert_eq!(battle.ledger().deaths, [0, 0]);
        } else if owner_side == Side::Party {
            assert_eq!(battle.ledger().kills, [1, 0]);
            assert_eq!(battle.ledger().grade(), 0); // 28DF4 clears the combo before 1FB2C.
        } else {
            assert_eq!(battle.ledger().deaths, [0, 1]);
            assert_eq!(battle.ledger().grade(), -100); // First party slot, not ActorId zero.
        }
    }
    Ok(())
}

#[test]
fn authored_impact_uses_hurt_surface_and_guard_result_before_contact_cooldown() -> Result<()> {
    for guarded in [false, true] {
        for breaks in [false, true] {
            for on_guard in [false, true] {
                let owner = crate::tests::actor(Side::Party);
                let mut target = crate::tests::actor(Side::Enemy);
                target.hp = 1000;
                target.max_hp = 1000;
                target.heading = 180.;
                target.facing_direction = crate::control::direction_from_heading(target.heading);
                target.guard.active = guarded;
                target.guard.break_pressure = 31;
                target.body.scale = 1.5;
                target.body.points = vec![HurtPoint {
                    center: [0., 5., 0.],
                    radius: 2.,
                }];
                let mut prepared =
                    crate::tests::prepared("pub task run() {}", vec![owner, target], 1);
                Arc::get_mut(&mut prepared)
                    .unwrap()
                    .effects
                    .insert(1, crate::tests::effect_binding(1, [8]));
                let mut battle = Battle::new(prepared);
                let mut contacts = Contacts::default();
                contacts.push(
                    Side::Party,
                    Contact {
                        source: ContactSource::Melee {
                            actor: ActorId(0),
                            action: ActionId(1),
                        },
                        owner: ActorId(0),
                        position: [10., 5., 0.],
                        radius: 20.,
                        height: 20.,
                        shape: HitShape::Box,
                        rule: HitRule {
                            kind: DamageKind::Slash,
                            arte: true,
                            power: Power::Fixed(100),
                            element: HitElement::Neutral,
                            prevents_defeat: false,
                            reaction: Default::default(),
                            guard: GuardRule {
                                enabled: true,
                                pressure: 0,
                                breaks,
                                unbreakable: false,
                            },
                            impact: Some(crate::ImpactEffect {
                                appearance: EffectAppearance {
                                    resource: 1,
                                    member: 8,
                                },
                                on_guard,
                            }),
                        },
                        cooldown: 7,
                        clash: None,
                        used: false,
                    },
                )?;
                let mut cues = vec![];
                contacts.resolve(&mut battle, &mut cues)?;
                let hit = cues
                    .iter()
                    .position(|cue| matches!(cue, Cue::Hit { .. }))
                    .unwrap();
                let impact = cues
                    .iter()
                    .position(|cue| matches!(cue, Cue::Effect { .. }));
                assert_eq!(impact.is_some(), !guarded || on_guard);
                if let Some(impact) = impact {
                    assert!(impact > hit);
                    let Cue::Effect {
                        position,
                        heading,
                        resource,
                        member,
                        ..
                    } = cues[impact]
                    else {
                        unreachable!()
                    };
                    assert_eq!((resource, member, heading), (1, 8, 180.));
                    assert!((position[0] - 3.).abs() < 0.000001);
                    assert_eq!([position[1], position[2]], [5., 0.]);
                }
                assert!(!battle.melee[0].can_hit(ActorId(1)));
            }
        }
    }
    Ok(())
}

fn vector(row: &Value) -> [f32; 3] {
    std::array::from_fn(|i| f32::from_bits(row[i].as_u64().unwrap() as u32))
}

fn actor(row: &Value) -> Actor {
    let number = |name: &str| row[name].as_i64().unwrap();
    let mut actor = crate::tests::actor(if row["side"] == "party" {
        Side::Party
    } else {
        Side::Enemy
    });
    actor.hp = number("hp") as i32;
    actor.max_hp = number("max_hp") as i32;
    actor.tp = number("tp") as u16;
    actor.max_tp = number("max_tp") as u16;
    actor.hit_stop = number("hit_stop") as u8;
    actor.luck = number("luck") as u8;
    let stat = |i| row["stats"][i].as_i64().unwrap() as i16;
    actor.stats = CombatStats {
        slash: stat(0),
        thrust: stat(1),
        defense: stat(2),
        intelligence: stat(3),
        accuracy: stat(4),
        evasion: stat(5),
        level: stat(6) as u8,
    };
    actor.position = vector(&row["position_bits"]);
    actor.heading = f32::from_bits(number("heading_bits") as u32);
    actor.facing_direction = crate::control::direction_from_heading(actor.heading);
    actor.control = match number("control") {
        0 => Control::Manual,
        1 => Control::SemiAuto,
        2 => Control::Auto,
        3 => Control::Enemy,
        other => panic!("unobserved control {other}"),
    };
    actor.activity = match number("activity") {
        3 => Activity::Idle,
        4 => Activity::Approaching,
        5 => Activity::Action {
            clock: number("clock") as i16,
            guard_window: std::array::from_fn(|i| row["guard_window"][i].as_i64().unwrap() as i16),
        },
        8 => Activity::Casting {
            clock: number("clock") as i16,
            guard_window: std::array::from_fn(|i| row["guard_window"][i].as_i64().unwrap() as i16),
        },
        9 => Activity::Hurt,
        11 => Activity::Guarding,
        other => panic!("unobserved activity {other}"),
    };
    let g = &row["guard"];
    actor.guard = Guard {
        active: g["active"].as_bool().unwrap(),
        kind: if g["special"].as_bool().unwrap() {
            GuardKind::Special
        } else {
            GuardKind::Normal
        },
        pressure: g["pressure"].as_u64().unwrap() as u8,
        break_pressure: g["break_pressure"].as_i64().unwrap() as i16,
        reduction: g["reduction"].as_u64().unwrap() as u8,
        auto_chance: g["party_chance"].as_u64().unwrap() as u8,
        enemy_chance: g["enemy_chance"].as_i64().unwrap() as i8,
        allow_airborne: g["allow_airborne"].as_bool().unwrap(),
        auto_disabled: g["auto_disabled"].as_bool().unwrap(),
        recently_hurt: g["recently_hurt"].as_bool().unwrap(),
        recovery_bonus: 0,
    };
    actor
}

#[test]
fn original_contacts_match_damage_rng_tp_and_local_hit_stop() {
    let trace: Value =
        serde_json::from_str(include_str!("../../tests/fixtures/opening-contact-tp.json")).unwrap();
    for row in trace["observations"].as_array().unwrap() {
        let mut owner = actor(&row["owner"]);
        owner.attack_power = row["power"].as_u64().unwrap() as u16;
        let mut target = actor(&row["target"]);
        target.affinities[0] = match row["affinity"].as_u64().unwrap() {
            0 => Affinity::Normal,
            1 => Affinity::Weak,
            2 => Affinity::Resistant,
            other => panic!("unobserved affinity {other}"),
        };
        let g = &row["guard_rule"];
        let rule = HitRule {
            impact: None,
            arte: row["arte"].as_bool().unwrap(),
            kind: match row["kind"].as_u64().unwrap() {
                0 => DamageKind::Slash,
                1 => DamageKind::Thrust,
                2 => DamageKind::Magic,
                _ => panic!(),
            },
            power: match row["power_mode"].as_u64().unwrap() {
                0 | 2 => Power::Normal,
                1 => Power::Percent(row["power_value"].as_u64().unwrap() as u16),
                3 => Power::Fixed(row["power_value"].as_u64().unwrap() as u16),
                _ => panic!(),
            },
            element: HitElement::Neutral, // Fixture resolves original element selection to this slot.
            prevents_defeat: false,
            guard: GuardRule {
                enabled: g["enabled"].as_bool().unwrap(),
                pressure: g["pressure"].as_u64().unwrap() as u8,
                breaks: g["breaks"].as_bool().unwrap(),
                unbreakable: g["unbreakable"].as_bool().unwrap(),
            },
            reaction: Default::default(), // Other contact-tail operations have separate fixtures.
        };
        let seed = row["random_before"].as_u64().unwrap() as u32;
        let mut random = Random(seed);
        let result = damage::resolve(
            &owner,
            &mut target.clone(),
            rule,
            owner.attack_power,
            vector(&row["incoming_direction_bits"]),
            &mut random,
        );
        assert_eq!(
            result.amount,
            row["amount"].as_i64().unwrap() as i32,
            "damage {}",
            row["index"]
        );
        assert_eq!(
            random.0,
            row["random_after"].as_u64().unwrap() as u32,
            "RNG {}",
            row["index"]
        );
        let flags = row["result"].as_u64().unwrap();
        let guard = if flags & 0x400 != 0 {
            GuardResult::Broken
        } else if flags & 0x10 != 0 {
            GuardResult::Blocked {
                first: flags & 0x8000 != 0,
                special: flags & 0x10000 != 0,
            }
        } else {
            GuardResult::None
        };
        assert_eq!(result.guard, guard, "guard {}", row["index"]);
        assert_eq!(result.critical, flags & 0x200 != 0);

        // Submit an already-admitted contact with the observed incoming vector.
        // Geometry/controller timing is outside this fixture's scope.
        target.body.points = vec![HurtPoint {
            center: target.position,
            radius: 1.,
        }];
        let position = target.position;
        let side = owner.side;
        let mut prepared = crate::tests::prepared("pub task run() {}", vec![owner, target], 1);
        Arc::get_mut(&mut prepared).unwrap().random_seed = seed;
        let mut battle = Battle::new(prepared);
        battle
            .emit(
                Arc::new(ProjectileDefinition {
                    motion: Default::default(),
                    effects: Default::default(),
                    lifetime: 1,
                    velocity: vector(&row["incoming_direction_bits"]),
                    acceleration: [0.; 3],
                    offset: [0.; 3],
                    clamp_ground: false,
                    active: None,
                    birth: None,
                    contact: Some(crate::ProjectileContact {
                        hit: rule,
                        cooldown: 0,
                        repeat_limit: 0,
                        radius: 10.,
                        height: 10.,
                        shape: HitShape::Box,
                        offset: [0.; 3],
                        radius_growth: 0.,
                        height_growth: 0.,
                        survives_contact: true,
                        clash_effect: None,
                    }),
                }),
                ActionId(1),
                ActorId(0),
                ActorId(1),
                position,
            )
            .unwrap();
        let projectile = battle.projectiles.values_mut().next().unwrap();
        projectile.velocity = vector(&row["incoming_direction_bits"]);
        projectile.frame.contact_active = true;
        let mut contacts = Contacts::default();
        contacts.submit(projectile, side).unwrap();
        let mut cues = vec![];
        contacts.resolve(&mut battle, &mut cues).unwrap();
        assert!(cues.iter().any(|cue| matches!(
            cue,
            Cue::Hit {
                actor: ActorId(1),
                ..
            }
        )));
        assert_eq!(
            battle.actors[1].hp,
            row["hp_after"].as_i64().unwrap() as i32,
            "HP {}",
            row["index"]
        );
        assert_eq!(
            battle.actors[0].tp,
            row["tp_after"].as_u64().unwrap() as u16,
            "TP {}",
            row["index"]
        );
        for (index, name) in ["owner", "target"].into_iter().enumerate() {
            assert_eq!(
                battle.actors[index].hit_stop,
                row[format!("{name}_hit_stop_after")].as_u64().unwrap() as u8,
                "{name} local hit-stop {}",
                row["index"]
            );
        }
    }
}
