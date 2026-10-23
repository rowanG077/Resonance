use super::*;
use crate::{
    ContactSource, HitRule, HitShape, MeleeDefinition, Side,
    tests::{actor, prepared},
};

fn definition() -> Arc<MeleeDefinition> {
    Arc::new(MeleeDefinition {
        hit: HitRule {
            power: crate::Power::Fixed(5),
            ..crate::tests::hit()
        },
        trail: None,
        volume: crate::MeleeVolume::Fixed {
            offset: [0.; 3],
            radius: 30.,
            half_height: 30.,
        },
    })
}

fn contact(at: u16, duration: u16, definition: &Arc<MeleeDefinition>) -> (u16, crate::AttackEvent) {
    (
        at,
        crate::AttackEvent::Contact {
            definition: Arc::clone(definition),
            duration,
        },
    )
}

fn opposing_actions() -> BattleInput {
    BattleInput {
        actions: (0..2)
            .map(|index| ActionRequest {
                actor: ActorId(index),
                action: crate::ActionKey(index as usize),
                target: ActorId(index ^ 1),
            })
            .collect(),
        ..Default::default()
    }
}

fn started_action(frame: &BattleFrame, owner: ActorId) -> ActionId {
    frame
        .cues
        .iter()
        .find_map(|cue| match cue {
            Cue::Started { actor, action, .. } if *actor == owner => Some(*action),
            _ => None,
        })
        .expect("action admitted")
}

fn candidate(mut actors: Vec<Actor>) -> PreparedBattle {
    for actor in &mut actors {
        actor.body.collider = Some(crate::Collider::sphere(1.));
    }
    let mut prepared = prepared(actors, 6);
    let action = Arc::make_mut(&mut prepared.resources.actions.entries[0]);

    crate::tests::attack_mut(action).events =
        vec![contact(1, 2, &definition()), contact(4, 1, &definition())];
    prepared.resources.actor_setup[0] = Default::default();
    crate::tests::assign_action(&mut prepared, 0, crate::ActionKey(0));
    prepared
}

fn request(actor: u8) -> BattleInput {
    BattleInput::from(ActionRequest {
        actor: ActorId(actor),
        action: crate::ActionKey(0),
        target: ActorId(actor ^ 1),
    })
}

#[test]
fn contact_recovery_respects_success_capacity_and_small_hits() {
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
        let mut battle = crate::tests::prepare_actors(vec![owner, target], 1)
            .finish()
            .unwrap();
        let mut contacts = Contacts::default();
        contacts.melee(ActorId(0), ActionId(1), &hit, &[]).unwrap();
        let mut cues = vec![];
        contacts.resolve(&mut battle, &mut cues, &[]).unwrap();
        assert!(cues.iter().any(|cue| matches!(cue, Cue::Hit { .. })));
        assert_eq!(battle.actors[0].tp, expected, "{case} arte={arte} tp={tp}");
    }
    for (kind, percent, hp, tp) in [
        (crate::RecoveryKind::Hp, 10, 51, 0),
        (crate::RecoveryKind::Tp, 3, 50, 1),
    ] {
        let mut owner = actor(Side::Party);
        owner.tp = 0;
        let mut prepared = candidate(vec![owner, actor(Side::Enemy)]);
        let mut hit = definition();
        Arc::make_mut(&mut hit).hit.arte = true;
        crate::tests::attack_mut(Arc::make_mut(&mut prepared.resources.actions.entries[0]))
            .events = vec![
            contact(1, 1, &hit),
            (3, crate::AttackEvent::Drain { kind, percent }),
        ];
        let mut battle = prepared.finish().unwrap();
        battle.step(request(0)).unwrap();
        crate::tests::step_idle(&mut battle, 8).unwrap();
        assert_eq!(battle.actors[1].hp, 45);
        assert_eq!((battle.actors[0].hp, battle.actors[0].tp), (hp, tp));
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
        let contact = |at| contact(at, 1, &hit);
        let attack = |events| ActionDefinition {
            normal: None,
            tp_cost: 0,
            execution: ActionExecution::Attack(PreparedAttack {
                events,
                recovery: 2,
                ..crate::tests::attack(40)
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
        let first = battle.step(opposing_actions()).unwrap();
        let enemy_action = started_action(&first, ActorId(1));
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
            if !interrupted && battle.activity(ActorId(1)) == crate::Activity::Recovering {
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
fn spell_row_armor_exposes_the_correct_contact_and_restores_idle_armor() {
    for (threshold, spent, protected_hits) in [
        (Some(0), 0, 0),
        (Some(4), 0, 4),
        (Some(5), 0, 5),
        (Some(4), 3, 4),
        (None, 3, 3),
    ] {
        let mut actors = vec![actor(Side::Party), actor(Side::Enemy)];
        for actor in &mut actors {
            actor.body.collider = Some(crate::Collider::sphere(1.));
        }
        actors[1].reaction.armor = crate::Armor {
            base: 6,
            threshold: 6,
            received: spent,
        };
        actors[1].guard.auto_disabled = true;
        let mut prepared = crate::PreparedBattle::new(
            actors
                .into_iter()
                .map(|actor| (actor, Default::default()))
                .collect(),
            vec![crate::tests::action(80), crate::tests::cast(300, 0)].into(),
            1,
        )
        .unwrap();
        let mut hit = definition();
        let rule = &mut Arc::make_mut(&mut hit).hit;
        rule.reaction.armor_damage = 1;
        rule.reaction.hitstun = 30;
        let attack =
            crate::tests::attack_mut(Arc::make_mut(&mut prepared.resources.actions.entries[0]));
        attack.end_at = 80;
        attack.events = (0..=protected_hits)
            .map(|index| contact(2 + index * 5, 1, &hit))
            .collect();
        let mut cast = crate::tests::cast(300, 0);
        let crate::ActionExecution::Casting(definition) = &mut cast.execution else {
            unreachable!()
        };
        Arc::make_mut(definition).armor = threshold;
        prepared.resources.actions.entries[1] = Arc::new(cast);
        for index in 0..2 {
            crate::tests::assign_action(&mut prepared, index, crate::ActionKey(index));
        }
        let mut battle = prepared.finish().unwrap();
        let started = battle.step(opposing_actions()).unwrap();
        let cast = started_action(&started, ActorId(1));
        let mut hits = 0;
        let mut interrupted = false;
        for _ in 0..200 {
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
                        hits <= protected_hits
                    );
                }
                assert!(!matches!(cue, Cue::Released { action, .. } if *action == cast));
            }
            if frame.cues.contains(&Cue::Interrupted { action: cast }) {
                assert_eq!(hits, protected_hits + 1);
                interrupted = true;
            }
        }
        assert!(
            interrupted,
            "spell with armor {threshold:?} was not interrupted"
        );
        assert_eq!(hits, protected_hits + 1);
        assert_eq!(
            battle.actors[1].reaction.armor,
            crate::Armor {
                base: 6,
                threshold: 6,
                received: 0
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
                (
                    0,
                    crate::AttackEvent::PassThrough {
                        enabled: true,
                        duration: 0,
                    },
                ),
                contact(1, 1, &hit),
                contact(3, 1, &hit),
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
    let mut prepared = candidate(vec![actor(Side::Party), actor(Side::Enemy), distant]);
    for (at, event) in
        &mut crate::tests::attack_mut(Arc::make_mut(&mut prepared.resources.actions.entries[0]))
            .events
    {
        if let crate::AttackEvent::Contact {
            definition,
            duration,
        } = event
        {
            if *at == 4 {
                *duration = 2;
            }
            Arc::make_mut(definition).volume = crate::MeleeVolume::BodyCenter {
                shape: HitShape::Sphere,
                radius: 30.,
                half_height: 30.,
            };
        }
    }
    let mut battle = prepared.finish().unwrap();
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
    // One contact can hit one actor per update. The renewed window admits
    // each victim once across its two updates.
    for expected in [ActorId(1), ActorId(2)] {
        assert_eq!(
            recipients(&battle.step(BattleInput::default()).unwrap()),
            [expected]
        );
    }
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
        let mut hit = definition();
        Arc::make_mut(&mut hit).hit.power = crate::Power::Normal;
        *crate::tests::attack_mut(Arc::make_mut(&mut prepared.resources.actions.entries[0])) =
            crate::PreparedAttack {
                events: vec![contact(1, 3, &hit)],
                recovery: 2,
                chain_at: Some(3),
                ..crate::tests::attack(3)
            };
        let mut next = crate::tests::action(0);
        crate::tests::attack_mut(&mut next).recovery = 1;
        let mut counter = crate::tests::action(0);
        let attack = crate::tests::attack_mut(&mut counter);
        attack.recovery = 1;
        attack.events = vec![contact(0, 1, &definition())];
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
                continuation: crate::tests::normal_continuation(kind),
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
            shortcuts: [None; 4],
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
        let start = battle.step(request(0)).unwrap();
        let id = started_action(&start, ActorId(0));
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
                events: vec![contact(0, if prior_hit { 2 } else { 1 }, &definition())],
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
            events: vec![contact(0, 1, &definition())],
            ..crate::tests::attack(2)
        };
    let mut battle = prepared.finish().unwrap();
    let mut projectile = crate::tests::contact_projectile(true, true);
    projectile.clamp_ground = false;
    let contact = projectile.contact.as_mut().unwrap();
    contact.hit = definition().hit;
    contact.cooldown = 1;
    contact.radius = 30.;
    contact.height = 30.;
    contact.shape = HitShape::Box;
    battle
        .emit(
            Arc::new(projectile),
            ActionId(9),
            ActorId(1),
            ActorId(0),
            [0.; 3],
        )
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
