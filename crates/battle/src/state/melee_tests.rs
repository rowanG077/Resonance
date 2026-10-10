use super::*;
use crate::{
    ContactSource, DamageKind, GuardRule, HitRule, HitShape, MeleeDefinition, Side,
    tests::{actor, prepared},
};

fn definition() -> Arc<MeleeDefinition> {
    Arc::new(MeleeDefinition {
        hit: HitRule {
            overlimit_pause: false,
            condition: None,
            arte: false,
            reaction: Default::default(),
            kind: DamageKind::Slash,
            power: crate::Power::Fixed(5),
            element: crate::HitElement::Neutral,
            prevents_defeat: false,
            guard: GuardRule::default(),
        },
        trail: None,
        volume: crate::MeleeVolume {
            offset: [0.; 3],
            radius: 30.,
            half_height: 30.,
        },
    })
}

fn candidate(mut actors: Vec<Actor>) -> PreparedBattle {
    for actor in &mut actors {
        actor.body.collider = Some(crate::Collider::sphere(1.));
    }
    let mut prepared = prepared(actors, 6);
    let action = Arc::make_mut(&mut prepared.resources.actions.entries[0]);

    crate::tests::attack_mut(action).events = vec![
        (
            1,
            crate::AttackEvent::Contact {
                definition: definition(),
                duration: 2,
            },
        ),
        (
            4,
            crate::AttackEvent::Contact {
                definition: definition(),
                duration: 1,
            },
        ),
    ];
    prepared.resources.actor_setup[0] = Default::default();
    crate::tests::assign_action(&mut prepared, 0, crate::ActionKey(0));
    prepared
}

fn battle(actors: Vec<Actor>) -> Battle {
    candidate(actors).finish().unwrap()
}

fn request(actor: u8) -> BattleInput {
    BattleInput {
        actions: vec![ActionRequest {
            actor: ActorId(actor),
            action: crate::ActionKey(0),
            target: ActorId(actor ^ 1),
        }],
        ..Default::default()
    }
}

#[test]
fn normal_contact_restores_one_tp_for_unblocked_damage_up_to_capacity() {
    use crate::{Affinity, ProtectionMode};
    for (case, arte, tp, expected) in [
        ("hit", false, 0, 1),
        ("hit", false, 39, 40),
        ("hit", false, 40, 40),
        ("hit", true, 0, 0),
        ("armor", false, 0, 1),
        ("avoid", false, 0, 0),
        ("absorb", false, 0, 0),
        ("immune", false, 0, 0),
        ("guard", false, 0, 0),
        ("break", false, 0, 1),
        ("lethal", false, 0, 1),
    ] {
        let mut owner = actor(Side::Party);
        owner.tp = tp;

        let mut target = actor(Side::Enemy);
        target.body.collider = Some(crate::Collider::sphere(1.));
        target.facing_direction = [0., 0., -1.];
        target.heading = 180.;
        let mut hit = definition();
        let rule = &mut Arc::make_mut(&mut hit).hit;
        rule.arte = arte;
        match case {
            "armor" => target.reaction.armor.threshold = 1,
            "avoid" => {
                target.side = Side::Party;
                owner.side = Side::Enemy;
                target.reaction.protection.mode = ProtectionMode::Recovery;
            }
            "absorb" => target.equipment.affinities[0] = Affinity::Absorb,
            "immune" => target.equipment.affinities[0] = Affinity::Immune,
            "guard" | "break" => {
                target.guard.active = true;
                target.guard.break_pressure = 10;
                rule.guard.breaks = case == "break";
            }
            "lethal" => target.hp = 1,
            _ => {}
        }
        let mut battle = PreparedBattle::new(
            vec![(owner, Default::default()), (target, Default::default())],
            Default::default(),
            1,
        )
        .unwrap()
        .finish()
        .unwrap();
        let mut contacts = Contacts::default();
        contacts.melee(ActorId(0), ActionId(1), &hit, &[]).unwrap();
        let mut cues = vec![];
        contacts.resolve(&mut battle, &mut cues).unwrap();
        assert!(cues.iter().any(|cue| matches!(cue, Cue::Hit { .. })));
        assert_eq!(battle.actors[0].tp, expected, "{case} arte={arte} tp={tp}");
    }
}

#[test]
fn native_armor_survives_two_hits_then_cancels_later_work_and_clears_on_recovery() {
    use crate::{ActionDefinition, ActionExecution, AttackEvent, PreparedAttack};
    for interrupted in [false, true] {
        let mut actors = vec![actor(Side::Party), actor(Side::Enemy)];
        for actor in &mut actors {
            actor.body.collider = Some(crate::Collider::sphere(1.));
        }
        actors[1].reaction.armor.base = 1;
        let mut hit = definition();
        Arc::make_mut(&mut hit).hit.reaction.armor_damage = 1;
        let contact = |at| {
            (
                at,
                AttackEvent::Contact {
                    definition: hit.clone(),
                    duration: 1,
                },
            )
        };
        let attack = |events| ActionDefinition {
            normal: None,
            tp_cost: 0,
            execution: ActionExecution::Attack(PreparedAttack {
                opening: None,
                chain_at: None,
                events,
                end_at: 40,
                recovery: 2,
            }),
        };
        let mut prepared = PreparedBattle::new(
            (actors)
                .into_iter()
                .map(|actor| (actor, Default::default()))
                .collect(),
            (vec![
                attack(if interrupted {
                    vec![contact(2), contact(5), contact(8)]
                } else {
                    vec![]
                }),
                attack(vec![(0, AttackEvent::Armor(2)), contact(30)]),
            ])
            .into(),
            1,
        )
        .unwrap();
        for index in 0..prepared.actors.len() {
            crate::tests::assign_action(&mut prepared, index, crate::ActionKey(index));
        }
        let mut battle = prepared.finish().unwrap();
        let first = battle
            .step(BattleInput {
                actions: vec![
                    ActionRequest {
                        actor: ActorId(0),
                        action: crate::ActionKey(0),
                        target: ActorId(1),
                    },
                    ActionRequest {
                        actor: ActorId(1),
                        action: crate::ActionKey(1),
                        target: ActorId(0),
                    },
                ],
                ..Default::default()
            })
            .unwrap();
        let enemy_action = first
            .cues
            .iter()
            .find_map(|cue| match cue {
                Cue::Started {
                    actor: ActorId(1),
                    action,
                    ..
                } => Some(*action),
                _ => None,
            })
            .unwrap();
        assert_eq!(battle.actors[1].reaction.armor.threshold, 2);
        let mut hits = 0;
        let mut cancelled = false;
        for _ in 0..100 {
            let frame = battle.step(BattleInput::default()).unwrap();
            for cue in &frame.cues {
                if let Cue::Hit {
                    actor: ActorId(1),
                    result,
                    ..
                } = cue
                {
                    hits += 1;
                    assert_eq!(
                        result.protection == crate::HitProtection::Armored,
                        hits <= 2
                    );
                    assert_eq!(result.hp_change, -5);
                }
            }
            if frame.cues.contains(&Cue::Interrupted {
                action: enemy_action,
            }) {
                assert_eq!(hits, 3);
                cancelled = true;
            }
            if battle.activity(ActorId(1)) == crate::Activity::Recovering {
                assert_eq!(battle.actors[1].reaction.armor.threshold, 0);
            }
        }
        assert_eq!(cancelled, interrupted);
        assert_eq!(hits, if interrupted { 3 } else { 0 });
        assert_eq!(battle.actors[0].hp, if interrupted { 50 } else { 45 });
        assert_eq!(battle.actors[1].hp, if interrupted { 35 } else { 50 });
        assert_eq!(battle.activity(ActorId(1)), crate::Activity::Idle);
        assert_eq!(
            battle.actors[1].reaction.armor,
            crate::Armor {
                base: 1,
                threshold: 1,
                received: 0,
            }
        );
    }
}

#[test]
fn contact_enters_hurt_and_cancels_pending_attack_events() {
    let mut prepared = candidate(vec![actor(Side::Party), actor(Side::Enemy)]);
    let mut hit = definition();
    Arc::make_mut(&mut hit).hit.reaction.hitstun = 2;
    *crate::tests::attack_mut(Arc::make_mut(&mut prepared.resources.actions.entries[0])) =
        crate::PreparedAttack {
            events: vec![
                (0, crate::AttackEvent::PassThrough(true)),
                (
                    1,
                    crate::AttackEvent::Contact {
                        definition: hit.clone(),
                        duration: 1,
                    },
                ),
                (
                    3,
                    crate::AttackEvent::Contact {
                        definition: hit,
                        duration: 1,
                    },
                ),
                (3, crate::AttackEvent::Sound(Some(crate::Sound::Stream(1)))),
            ],
            ..crate::tests::attack(20)
        };
    crate::tests::assign_action(&mut prepared, 1, crate::ActionKey(0));
    let mut battle = prepared.finish().unwrap();
    let mut input = request(0);
    input.actions.extend(request(1).actions);
    battle.step(input).unwrap();
    assert!(battle.bypasses_body_collision(ActorId(1)));
    battle.actors[1].hit_stop = 3;
    let frame = battle.step(BattleInput::default()).unwrap();
    assert_eq!(recipients(&frame), [ActorId(1)]);
    assert!(frame.cues.contains(&Cue::Interrupted {
        action: ActionId(2)
    }));
    assert_eq!(frame.actors[1].activity, crate::Activity::Hurt);
    assert!(!battle.bypasses_body_collision(ActorId(1)));
    assert_eq!(frame.actors[1].hit_stop, 0);

    assert_eq!(frame.actors[1].reaction.combo_hits, 1);
    assert_eq!(frame.actors[1].reaction.combo_damage, 5);
    assert_eq!(frame.actors[1].hp, 45);
    assert_eq!(frame.actors[1].movement.braking, 0.275);
    assert!(battle.sequence(&ActionId(2)).is_none());
    for _ in 0..6 {
        let frame = battle.step(BattleInput::default()).unwrap();
        assert_eq!(frame.actors[0].hp, 50);
        assert!(!frame.cues.iter().any(|cue| matches!(
            cue,
            Cue::Sound {
                actor: ActorId(1),
                ..
            }
        )));
    }
    assert_eq!(battle.actors[1].hp, 40);
    assert_eq!(battle.activity(ActorId(1)), crate::Activity::Idle);
}

fn recipients(frame: &BattleFrame) -> Vec<ActorId> {
    frame
        .cues
        .iter()
        .filter_map(|c| match c {
            Cue::Hit {
                source: ContactSource::Melee { .. },
                actor,
                ..
            } => Some(*actor),
            _ => None,
        })
        .collect()
}

#[test]
fn melee_windows_expire_and_rearm_without_repeating_targets() {
    let mut distant = actor(Side::Enemy);
    distant.position[0] = 1000.;
    let mut battle = battle(vec![actor(Side::Party), actor(Side::Enemy), distant]);
    assert!(recipients(&battle.step(request(0)).unwrap()).is_empty());
    assert_eq!(
        recipients(&battle.step(BattleInput::default()).unwrap()),
        [ActorId(1)]
    );
    let id = battle.snapshot().actions[0].0;
    let age = battle.action_age(id);
    let paused = battle
        .step(BattleInput {
            paused: true,
            ..Default::default()
        })
        .unwrap();
    assert!(recipients(&paused).is_empty());
    assert_eq!(battle.action_age(id), age);
    assert!(recipients(&battle.step(BattleInput::default()).unwrap()).is_empty());
    // The first window lasted two updates. A late entrant must wait for the next one.
    battle.actors[2].position = [10., 0., 0.];
    assert!(recipients(&battle.step(BattleInput::default()).unwrap()).is_empty());
    assert_eq!(
        recipients(&battle.step(BattleInput::default()).unwrap()),
        [ActorId(1), ActorId(2)]
    );
    assert!(recipients(&battle.step(BattleInput::default()).unwrap()).is_empty());
    assert_eq!((battle.actors[1].hp, battle.actors[2].hp), (40, 45));
}

#[test]
fn combo_commits_after_final_contacts_and_loses_to_interruption() {
    let mut final_damage = Vec::new();
    for (chain, incoming) in [(false, false), (true, false), (true, true)] {
        let mut owner = actor(Side::Party);
        owner.control = crate::Control::Manual;
        owner.equipment.stats.slash = 100;
        owner.movement.turning_disabled = true;
        let mut prepared = candidate(vec![owner, actor(Side::Enemy), actor(Side::Enemy)]);
        for actor in &mut prepared.actors {
            actor.hp = 2000;
            actor.equipment.max_hp = 2000;
        }
        let mut contact = definition();
        Arc::make_mut(&mut contact).hit.power = crate::Power::Normal;
        *crate::tests::attack_mut(Arc::make_mut(&mut prepared.resources.actions.entries[0])) =
            crate::PreparedAttack {
                events: vec![(
                    1,
                    crate::AttackEvent::Contact {
                        definition: contact,
                        duration: 3,
                    },
                )],
                recovery: 2,
                chain_at: Some(3),
                ..crate::tests::attack(3)
            };
        let mut next = crate::tests::action(0);
        crate::tests::attack_mut(&mut next).recovery = 1;
        let mut counter = crate::tests::action(0);
        let attack = crate::tests::attack_mut(&mut counter);
        attack.recovery = 1;
        attack.events = vec![(
            0,
            crate::AttackEvent::Contact {
                definition: definition(),
                duration: 1,
            },
        )];
        let first = Arc::make_mut(&mut prepared.resources.actions.entries[0]);
        first.normal = Some(crate::NormalAttack::Neutral);
        let normals = crate::NormalAttack::ALL.map(|kind| {
            let action = if kind == crate::NormalAttack::Neutral {
                crate::ActionKey(0)
            } else {
                let mut definition = next.clone();
                definition.normal = Some(kind);
                prepared.resources.actions.insert(definition)
            };
            crate::NormalControl {
                action,
                minimum_reach: 0.,
                reach: 120.,
            }
        });
        let successor = normals[crate::NormalAttack::Thrust as usize].action;
        let counter = prepared.resources.actions.insert(counter);
        prepared.resources.actor_setup[0].techniques.clear();
        prepared.resources.actor_setup[0].enemy_decision = None;
        if incoming {
            crate::tests::assign_action(&mut prepared, 2, counter);
        }
        prepared.actors[0].equipment.normal_combo_limit = 2;
        prepared.resources.actor_setup[0].control = Some(Arc::new(crate::ControlDefinition {
            normals,
            shortcuts: [0; 4],
            walk_speed: 5.,
            run_speed: 10.,
            turn_ticks: 1,
            motions: None,
        }));
        let mut battle = prepared.finish().unwrap();
        battle.actors[2].position = [1000., 0., 0.];

        let attack = |stick| crate::ControlInput {
            attack: crate::ButtonInput {
                pressed: true,
                held: true,
                released: false,
            },
            stick,
            ..crate::ControlInput::neutral(ActorId(0))
        };
        let start = battle
            .step(BattleInput {
                controllers: vec![attack([0; 2])],
                ..Default::default()
            })
            .unwrap();
        let id = start
            .cues
            .iter()
            .find_map(|cue| match cue {
                Cue::Started {
                    actor: ActorId(0),
                    action,
                    ..
                } => Some(*action),
                _ => None,
            })
            .expect("normal admitted through player input");
        for _ in 0..30 {
            if battle.action_age(id) == Some(3) {
                break;
            }
            battle.step(BattleInput::default()).unwrap();
        }
        assert_eq!(battle.action_age(id), Some(3));
        let first_hp = battle.actors[1].hp;
        assert!(first_hp < 2000);
        battle.actors[2].position = [0.; 3];

        let frame = battle
            .step(BattleInput {
                controllers: if chain {
                    vec![attack([0, -80])]
                } else {
                    vec![]
                },
                actions: if incoming {
                    vec![ActionRequest {
                        actor: ActorId(2),
                        action: counter,
                        target: ActorId(0),
                    }]
                } else {
                    vec![]
                },
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            battle.actors[1].hp, first_hp,
            "the final update must remember previously struck targets"
        );
        final_damage.push(2000 - battle.actors[2].hp);
        let successors = frame.cues.iter().filter(|cue| matches!(cue,
            Cue::Started { actor: ActorId(0), action, .. } if battle.action_definition(*action) == Some(successor)
        )).count();
        assert_eq!(successors, usize::from(chain && !incoming));
        if incoming {
            assert_eq!(battle.activity(ActorId(0)), crate::Activity::Hurt);
        }
        for _ in 0..8 {
            let frame = battle.step(BattleInput::default()).unwrap();
            assert!(!frame.cues.iter().any(|cue| matches!(
                cue,
                Cue::Started {
                    actor: ActorId(0),
                    ..
                }
            )));
        }
    }
    assert!(final_damage[0] > 0);
    assert_eq!(
        final_damage,
        vec![final_damage[0]; 3],
        "the final contact uses the retiring action's power"
    );
}

#[test]
fn queued_strikes_keep_prior_hits_and_survive_the_owners_defeat() {
    for prior_hit in [false, true] {
        let mut party = actor(Side::Party);
        party.hp = 5;
        let mut enemy = actor(Side::Enemy);
        enemy.hp = 5;
        let mut actors = vec![party, enemy];
        if prior_hit {
            actors[0].position[0] = 1000.;
            actors.push(actor(Side::Party));
        }
        let mut prepared = candidate(actors);
        *crate::tests::attack_mut(Arc::make_mut(&mut prepared.resources.actions.entries[0])) =
            crate::PreparedAttack {
                events: vec![(
                    0,
                    crate::AttackEvent::Contact {
                        definition: definition(),
                        duration: if prior_hit { 2 } else { 1 },
                    },
                )],
                ..crate::tests::attack(0)
            };
        crate::tests::assign_action(&mut prepared, 1, crate::ActionKey(0));
        let mut battle = prepared.finish().unwrap();
        let mut input = request(0);
        if prior_hit {
            let frame = battle.step(request(1)).unwrap();
            assert_eq!(recipients(&frame), [ActorId(2)]);
            battle.actors[0].position = [0.; 3];
        } else {
            input.actions.extend(request(1).actions);
        }
        let frame = battle.step(input).unwrap();
        assert_eq!(recipients(&frame), [ActorId(1), ActorId(0)]);
        assert_eq!((frame.actors[0].hp, frame.actors[1].hp), (0, 0));
        if prior_hit {
            assert_eq!(
                frame.actors[2].hp, 45,
                "defeat cannot rearm an already submitted strike"
            );
        }
        assert!(frame.outcome.is_none());
        assert_eq!(frame.recognized_result, None);
        let frame = battle.step(BattleInput::default()).unwrap();
        assert_eq!(
            frame.recognized_result,
            Some(if prior_hit {
                BattleResult::Victory
            } else {
                BattleResult::Defeat
            })
        );
    }
}

#[test]
fn projectile_can_clash_with_an_already_submitted_melee_origin_without_consuming_it() {
    let mut prepared = candidate(vec![actor(Side::Party), actor(Side::Enemy)]);
    *crate::tests::attack_mut(Arc::make_mut(&mut prepared.resources.actions.entries[0])) =
        crate::PreparedAttack {
            events: vec![(
                0,
                crate::AttackEvent::Contact {
                    definition: definition(),
                    duration: 1,
                },
            )],
            ..crate::tests::attack(2)
        };
    let mut battle = prepared.finish().unwrap();
    let definition = Arc::new(ProjectileDefinition {
        motion: Default::default(),
        effects: Default::default(),
        lifetime: Some(20),
        velocity: [0.; 3],
        acceleration: [0.; 3],
        offset: [0.; 3],
        clamp_ground: false,
        active: None,
        birth: None,
        contact: Some(crate::ProjectileContact {
            hit: definition().hit,
            cooldown: 1,
            repeat_limit: 0,
            radius: 30.,
            height: 30.,
            shape: HitShape::Box,
            offset: [0.; 3],
            radius_growth: 0.,
            height_growth: 0.,
            survives_contact: true,
            clashes: true,
        }),
    });
    battle
        .emit(definition, ActionId(9), ActorId(1), ActorId(0), [0.; 3])
        .unwrap();
    battle.step(BattleInput::default()).unwrap();
    let frame = battle.step(request(0)).unwrap();
    assert_eq!(recipients(&frame), [ActorId(1)]);
    assert!(frame.cues.iter().any(|cue| matches!(
        cue,
        Cue::ProjectileClashed {
            other: ContactSource::Melee {
                actor: ActorId(0),
                action: ActionId(1)
            },
            ..
        }
    )));
    assert!(frame.projectiles[0].disarmed);
    assert_eq!(frame.actors[0].hp, 50);
}
