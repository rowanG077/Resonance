use super::*;
use crate::{ActorSetup, AttackEvent, BattleInput, Control, MeleeDefinition, MeleeVolume};

// Deliberately synthetic timings and artwork: these cases exercise runtime ownership.
fn hit(power: u16, finisher_grade: bool) -> Arc<MeleeDefinition> {
    Arc::new(MeleeDefinition {
        hit: crate::HitRule {
            sound: None,
            kind: crate::DamageKind::Slash,
            effect_on_guard: false,
            effect: None,
            arte: true,
            overlimit_pause: false,
            power: crate::Power::Fixed(power),
            element: crate::HitElement::Neutral,
            prevents_defeat: false,
            finisher_grade,
            hp_drain: false,
            guard: Default::default(),
            reaction: Default::default(),
            condition: None,
        },
        volume: MeleeVolume::Fixed {
            offset: [0.; 3],
            radius: 200.,
            half_height: 200.,
        },
        trail: None,
    })
}

fn fixture(combination: bool) -> Battle {
    let mut attack = crate::tests::attack(12);
    attack.events = vec![
        (
            0,
            AttackEvent::Contact {
                definition: hit(50, false),
                duration: 1,
            },
        ),
        (
            2,
            AttackEvent::Contact {
                definition: hit(50, true),
                duration: 1,
            },
        ),
    ];
    let mut actions: crate::ActionDefinitions = vec![ActionDefinition {
        normal: None,
        tp_cost: 9,
        execution: ActionExecution::Attack(attack),
    }]
    .into();
    let normals = crate::tests::normal_controls(&mut actions, crate::tests::attack(1), [0., 120.]);
    let actors = [Side::Party, Side::Party, Side::Enemy]
        .into_iter()
        .enumerate()
        .map(|(index, side)| {
            let mut actor = crate::tests::actor(side);
            actor.control = if index == 0 {
                Control::Manual
            } else {
                Control::Auto
            };
            actor.position = [index as f32 * 30., 0., 0.];
            actor.tp = 0;
            actor.body.collider = Some(crate::Collider::sphere(2.));
            let setup = ActorSetup {
                control: (side == Side::Party).then(|| {
                    Arc::new(crate::ControlDefinition {
                        normals: normals.clone(),
                        shortcuts: [1, 0, 0, 0],
                        walk_speed: 1.,
                        run_speed: 2.,
                        turn_ticks: 1,
                        motions: None,
                    })
                }),
                techniques: if side == Side::Party {
                    vec![crate::tests::technique(ActionKey(0), 1)]
                } else {
                    vec![]
                },
                ..Default::default()
            };
            (actor, setup)
        })
        .collect();
    let mut battle = crate::PreparedBattle::new(actors, actions, 1)
        .unwrap()
        .with_technique_learning_members(vec![
            crate::tests::counted_techniques(ActorId(0), &[1], &[(1, 0)]),
            crate::tests::counted_techniques(ActorId(1), &[1], &[(1, 0)]),
        ])
        .unwrap()
        .with_unison_gauge(0, true, true)
        .unwrap()
        .finish()
        .unwrap();
    battle
        .configure_unison(UnisonDefinition {
            members: (0..2)
                .map(|index| UnisonMember {
                    actor: ActorId(index),
                    opener: crate::tests::attack(8),
                    opener_contact_ticks: 3,
                    windup_motion: MotionBinding { model: 1, clip: 1 },
                    windup_pose: Default::default(),
                    windup_voice: None,
                    artes: vec![UnisonArte {
                        action: ActionKey(0),
                        native_id: 1,
                        duration: 24,
                        distance: 10.,
                        altitude: 0.,
                    }],
                    short_weapon: false,
                })
                .collect(),
            placement: [[-1., 0., 0.], [1., 0., 0.], [0., 0., -1.], [0., 0., 1.]],
            minimum_distance: 1.,
            short_weapon_penalty: 0.,
            restore_height: 0.,
            immune_targets: Default::default(),
            retain_gauge: false,
            windup_effect: EffectAppearance {
                resource: 1,
                member: 1,
            },
            windup_sound: Sound::Stream(1),
            windup_color: Some([1, 2, 3]),
            recipes: combination.then(|| {
                vec![UnisonRecipe {
                    id: 1,
                    native_id: 301,
                    duration: 24,
                    camera_pitch: 0.,
                    ingredients: vec![vec![1, 1]],
                    execution: Some(UnisonCombination {
                        roles: std::array::from_fn(|index| {
                            vec![UnisonRole {
                                actor: ActorId(index as u8),
                                action: ActionKey(0),
                                placement: None,
                                visible: index == 0,
                                temporary_weapons: vec![],
                            }]
                        }),
                        color: Some([32; 3]),
                        voices: vec![],
                    }),
                }]
            }),
            combination_prelude: combination.then_some(UnisonPrelude {
                first_x: -10.,
                spacing: 20.,
                camera: CameraPose {
                    eye: [0., 100., 100.],
                    focus: [0.; 3],
                    pitch: 0.,
                    yaw: 0.,
                    radius: 100.,
                },
                color: Some([64; 3]),
                effect: EffectAppearance {
                    resource: 1,
                    member: 1,
                },
                sound: Sound::Stream(1),
                voices: vec![],
            }),
            damage_bonus: false,
        })
        .unwrap();
    battle
}

fn opening(battle: &mut Battle) -> ActionId {
    let frame = battle
        .step(BattleInput {
            unison: Some(UnisonInput::Activate(ActorId(0))),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(frame.unison.unwrap().phase, UnisonPhase::Windup);
    assert_eq!(battle.unison_gauge(), 0);
    for _ in 0..70 {
        let frame = battle.step(Default::default()).unwrap();
        if frame.unison.unwrap().phase == UnisonPhase::Opening {
            return battle.unison.run.as_ref().unwrap().opener_action.unwrap();
        }
    }
    panic!("opener did not start");
}

fn contact(battle: &mut Battle, action: ActionId) {
    let mut contacts = crate::contact::Contacts::default();
    contacts
        .melee(ActorId(0), action, &hit(1, false), &[])
        .unwrap();
    contacts.resolve(battle, &mut vec![], &[]).unwrap();
}

#[test]
fn opener_requires_its_own_action_and_effective_contact() {
    let mut battle = fixture(false);
    let action = opening(&mut battle);
    contact(&mut battle, ActionId(action.0 + 1));
    assert!(
        battle
            .unison
            .run
            .as_ref()
            .unwrap()
            .geometric_contact
            .is_none()
    );
    battle.actors[2].equipment.affinities[0] = crate::Affinity::Immune;
    contact(&mut battle, action);
    let run = battle.unison.run.as_ref().unwrap();
    assert_eq!(run.geometric_contact, Some(ActorId(2)));
    assert!(run.effective_contact.is_none());
    let hp = battle.actors[2].hp;
    battle.actors[2].equipment.affinities[0] = crate::Affinity::Absorb;
    battle.actors[2].reaction.armor.threshold = 10;
    contact(&mut battle, action);
    assert!(battle.actors[2].hp < hp);
    assert_eq!(
        battle.unison.run.as_ref().unwrap().effective_contact,
        Some(ActorId(2))
    );
}

#[test]
fn selected_artes_defer_defeat_then_restore_the_ordinary_battle() {
    for combination in [false, true] {
        let mut battle = fixture(combination);
        battle
            .prepare_shortcut(ActorId(1), 0, None)
            .unwrap()
            .commit();
        assert!(
            battle.unison.definition.as_ref().unwrap().members[1]
                .selected_arte(&battle, 0)
                .is_none()
        );
        battle
            .prepare_shortcut(ActorId(1), 0, Some(ActionKey(0)))
            .unwrap()
            .commit();
        let action = opening(&mut battle);
        contact(&mut battle, action);
        for _ in 0..4 {
            if battle
                .step(Default::default())
                .unwrap()
                .unison
                .unwrap()
                .phase
                == UnisonPhase::Selecting
            {
                break;
            }
        }
        let positions: Vec<_> = battle.actors.iter().map(|actor| actor.position).collect();
        let select = |pressed_slots| BattleInput {
            unison: Some(UnisonInput::Select {
                pressed_slots,
                shortcut: 0,
            }),
            ..Default::default()
        };
        let staged = battle.step(select(0b11)).unwrap().unison.unwrap();
        assert_eq!(staged.phase, UnisonPhase::Staging);
        assert_eq!(staged.selected, [ActorId(0)]);
        let running = battle.step(select(0b10)).unwrap().unison.unwrap();
        assert_eq!(running.selected, [ActorId(0), ActorId(1)]);
        let mut saw_deferred = false;
        let mut saw_followup = false;
        let mut marked_defeat = false;
        let mut defeated = 0;
        let mut combined_phases = [false; 2];
        for _ in 0..160 {
            let frame = battle.step(Default::default()).unwrap();
            if let Some(unison) = &frame.unison {
                combined_phases[0] |= unison.phase == UnisonPhase::CombinationPose;
                combined_phases[1] |= unison.phase == UnisonPhase::Combined;
                if unison.phase == UnisonPhase::Combined {
                    assert!(unison.hidden.contains(&ActorId(1)));
                }
            }
            for cue in &frame.cues {
                if let Cue::Hit {
                    actor: ActorId(2),
                    result,
                    ..
                } = cue
                {
                    saw_followup |= result.hp_change == 0 && result.is_damage();
                }
                defeated += usize::from(matches!(cue, Cue::Defeated { actor: ActorId(2) }));
            }
            if battle.actors[2].hp == 0 && battle.actors[2].available() {
                saw_deferred = true;
                marked_defeat |= battle.unison.run.as_ref().unwrap().pending_defeat == Some(true);
                assert!(battle.recognize_result().is_none());
                assert_eq!(battle.ledger.kills, [0, 0, 0]);
            }
            if frame.unison.is_none() {
                break;
            }
        }
        assert!(saw_deferred && saw_followup && marked_defeat);
        assert!(!battle.unison.active());
        assert_eq!(defeated, 1);
        assert_eq!(combined_phases, [combination; 2]);
        assert_eq!(
            battle.ledger.kills,
            if combination { [1, 0, 0] } else { [0, 1, 0] }
        );
        assert!(i64::from(battle.ledger.grade()) >= START_GRADE + 50);
        assert_eq!(
            battle.recognize_result(),
            Some(crate::BattleResult::Victory)
        );
        assert_eq!(
            battle.last_unison_completion(),
            Some(&if combination {
                UnisonCompletion::CombinationExecuted {
                    id: 1,
                    native_id: 301,
                    actors: vec![ActorId(0), ActorId(1)],
                }
            } else {
                UnisonCompletion::RecipeCatalogueUnavailable
            })
        );
        assert_eq!(
            battle
                .actors
                .iter()
                .map(|actor| actor.position)
                .collect::<Vec<_>>(),
            positions
        );
        for actor in [ActorId(0), ActorId(1)] {
            assert_eq!(battle.actors[actor.index()].tp, 0);
            assert_eq!(battle.technique_uses(actor, 1), Some(1));
        }
    }
}
