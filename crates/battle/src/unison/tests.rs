use super::*;
use crate::TechniqueId;
use crate::{
    ActorSetup, AttackEvent, BattleInput, Control, MeleeDefinition, MeleeVolume, ResourceId,
};

// Deliberately synthetic timings and artwork: these cases exercise runtime ownership.
fn hit(power: u16, finisher_grade: bool) -> Arc<MeleeDefinition> {
    Arc::new(MeleeDefinition {
        hit: crate::HitRule {
            arte: true,
            power: crate::Power::Fixed(power),
            finisher_grade,
            ..crate::tests::hit()
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
                        normals,
                        shortcuts: [Some(TechniqueId(1)), None, None, None],
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
                    windup_motion: MotionBinding {
                        model: ResourceId(1),
                        clip: 1,
                    },
                    windup_pose: Default::default(),
                    windup_voice: None,
                    artes: vec![UnisonArte {
                        action: ActionKey(0),
                        native_id: crate::UnisonArteId(1),
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
                resource: ResourceId(1),
                member: 1,
            },
            windup_sound: Sound::Stream(1),
            windup_color: Some([1, 2, 3]),
            recipes: combination.then(|| {
                vec![UnisonRecipe {
                    id: crate::UnisonRecipeId(1),
                    native_id: crate::UnisonArteId(301),
                    duration: 24,
                    camera_pitch: 0.,
                    ingredients: vec![vec![crate::UnisonArteId(1); 2]],
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
                    resource: ResourceId(1),
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
    let starts = battle.ledger.achievements[0].unison_starts;
    let frame = battle
        .step(BattleInput {
            unison: Some(UnisonInput::Activate(ActorId(0))),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(frame.unison.unwrap().phase, UnisonPhase::Windup);
    assert_eq!(battle.ledger.achievements[0].unison_starts, starts + 1);
    assert_eq!(battle.unison_gauge(), 0);
    advance_to_phase(battle, UnisonPhase::Opening, 70);
    battle.unison.run.as_ref().unwrap().opener_action.unwrap()
}

fn advance_to_phase(battle: &mut Battle, phase: UnisonPhase, max_ticks: u32) {
    for _ in 0..max_ticks {
        let frame = battle.step(Default::default()).unwrap();
        if frame.unison.unwrap().phase == phase {
            return;
        }
    }
    panic!("unison did not reach {phase:?}");
}

fn select(pressed_slots: u8) -> BattleInput {
    BattleInput {
        unison: Some(UnisonInput::Select {
            pressed_slots,
            shortcut: crate::ShortcutSlot::Neutral,
        }),
        ..Default::default()
    }
}

fn contact(battle: &mut Battle, action: ActionId) {
    let mut contacts = crate::contact::Contacts::default();
    contacts
        .melee(ActorId(0), action, &hit(1, false), &[])
        .unwrap();
    contacts.resolve(battle, &mut vec![], &[]).unwrap();
}

#[test]
fn unison_elemental_conversion_keeps_base_counts_and_recipe_identity() {
    use resonance_content::arte::elemental;

    for variant in elemental::VARIANTS {
        let mut battle = fixture(true);
        battle.actors[0].equipment.base_element = Some(variant.element);
        let action = battle.prepared.actions.insert(ActionDefinition {
            normal: None,
            tp_cost: 7,
            execution: ActionExecution::Attack(crate::tests::attack(120)),
        });
        battle.prepared.actor_setup[0].techniques = vec![
            crate::tests::technique(ActionKey(0), variant.base),
            crate::tests::technique(action, variant.id),
        ];
        battle.learning_members[0] = crate::tests::counted_techniques(
            ActorId(0),
            &[variant.base],
            &[(variant.base, 199), (variant.id, 49)],
        );
        battle
            .prepare_shortcut(ActorId(0), crate::ShortcutSlot::Neutral, Some(ActionKey(0)))
            .unwrap()
            .commit();
        let opener = opening(&mut battle);
        contact(&mut battle, opener);
        advance_to_phase(&mut battle, UnisonPhase::Selecting, 4);
        battle.step(select(1)).unwrap();
        battle.step(Default::default()).unwrap();
        let sequence = battle
            .sequences()
            .find(|(_, sequence)| sequence.actor == ActorId(0))
            .unwrap()
            .1;
        assert_eq!(
            sequence.action, action,
            "{} was not converted",
            variant.name
        );
        assert_eq!(
            battle.technique_uses(ActorId(0), TechniqueId(variant.base)),
            Some(200)
        );
        assert_eq!(
            battle.technique_uses(ActorId(0), TechniqueId(variant.id)),
            Some(49)
        );
        assert!(battle.technique_acquisitions().is_empty());
        assert_eq!(battle.actors[0].tp, 0);
        let run = battle.unison.run.as_ref().unwrap();
        assert_eq!(run.selections[0].arte.native_id, crate::UnisonArteId(1));
        assert!(run.deadline >= 120, "longer elemental action was truncated");
    }
}

#[test]
fn unison_elemental_hammer_rolls_once_and_preserves_lucky_toss() {
    use resonance_content::arte::elemental;

    for luck in [0, 800] {
        let mut battle = fixture(false);
        battle.actors[0].equipment.base_element = Some(crate::Element::Ice);
        battle.actors[0].equipment.luck = luck;
        let variants = [24, 30, 40].map(|duration| {
            Arc::new(ActionDefinition {
                normal: None,
                tp_cost: 7,
                execution: ActionExecution::Attack(crate::tests::attack(duration)),
            })
        });
        let ice = battle.prepared.actions.insert((*variants[1]).clone());
        crate::tests::attack_mut(Arc::make_mut(&mut battle.prepared.actions.entries[0])).events =
            vec![(0, AttackEvent::ChooseHammer(variants))];
        battle.prepared.actor_setup[0].techniques = vec![
            crate::tests::technique(ActionKey(0), 40),
            crate::tests::technique(ice, elemental::ICE_HAMMER),
        ];
        battle.learning_members[0] = crate::tests::counted_techniques(
            ActorId(0),
            &[40],
            &[(40, 199), (elemental::ICE_HAMMER, 49)],
        );
        let mut expected_random = battle.random;
        expected_random.next_u16();
        let duration = battle
            .start_unison_arte(
                ActionRequest {
                    actor: ActorId(0),
                    action: ActionKey(0),
                    target: ActorId(2),
                },
                &mut vec![],
            )
            .unwrap();
        assert_eq!(battle.random, expected_random);
        let sequence = battle.sequences().next().unwrap().1;
        assert_eq!(sequence.action, if luck == 0 { ice } else { ActionKey(0) });
        assert_eq!(duration, if luck == 0 { 30 } else { 0 });
        let ActionExecution::Attack(attack) = &sequence.definition.execution else {
            unreachable!()
        };
        assert_eq!(attack.end_at, if luck == 0 { 30 } else { 40 });
        assert!(
            !attack
                .events
                .iter()
                .any(|(_, event)| matches!(event, AttackEvent::ChooseHammer(_)))
        );
        assert_eq!(
            battle.technique_uses(ActorId(0), TechniqueId(40)),
            Some(200)
        );
        assert_eq!(
            battle.technique_uses(ActorId(0), TechniqueId(elemental::ICE_HAMMER)),
            Some(49)
        );
    }
}

#[test]
fn normal_finisher_keeps_the_combo_through_unison_opening_and_selection() {
    // Include the last hurt and recovery visits: acceptance owns the still-live
    // combo even when less recovery remains than the opener's contact startup.
    for delay in [0, 4, 8, 24, 30, 31] {
        let mut battle = fixture(false);
        let owner = ActorId(0);
        let target = ActorId(2);
        let mut contact = hit(1, false);
        let rule = &mut Arc::make_mut(&mut contact).hit;
        rule.arte = false;
        rule.reaction.hitstun = 30;
        let normals = battle.prepared.actor_setup[0]
            .control
            .as_ref()
            .unwrap()
            .normals;
        for normal in [crate::NormalAttack::Neutral, crate::NormalAttack::Finisher] {
            let action = Arc::make_mut(
                &mut battle.prepared.actions.entries[normals[normal as usize].action.0],
            );
            let attack = crate::tests::attack_mut(action);
            attack.end_at = 30;
            attack.chain_at = Some(2);
            attack.events = vec![(
                0,
                AttackEvent::Contact {
                    definition: Arc::clone(&contact),
                    duration: 1,
                },
            )];
        }
        let opener = battle.unison.openers[0];
        let attack = crate::tests::attack_mut(Arc::make_mut(
            &mut battle.prepared.actions.entries[opener.0],
        ));
        attack.end_at = 30;
        attack.events = vec![(
            13,
            AttackEvent::Contact {
                definition: contact,
                duration: 9,
            },
        )];
        Arc::make_mut(battle.unison.definition.as_mut().unwrap()).members[0].opener_contact_ticks =
            30;
        assert!(
            battle
                .start_actor_command(
                    ActionRequest {
                        actor: owner,
                        action: normals[crate::NormalAttack::Neutral as usize].action,
                        target,
                    },
                    &mut vec![],
                )
                .unwrap()
        );
        battle.step(Default::default()).unwrap();
        assert_eq!(battle.actors[2].reaction.combo_hits, 1);
        let mut input = crate::ControlInput::neutral(owner);
        input.attack.pressed = true;
        battle.step(BattleInput::from(input)).unwrap();
        for _ in 0..8 {
            if battle.actors[2].reaction.combo_hits == 2 {
                break;
            }
            battle.step(Default::default()).unwrap();
        }
        assert_eq!(battle.actors[2].reaction.combo_hits, 2);
        assert_eq!(
            battle.runtime[0]
                .task()
                .action()
                .unwrap()
                .1
                .definition
                .normal,
            Some(crate::NormalAttack::Finisher)
        );
        crate::tests::step_idle(&mut battle, delay).unwrap();
        let remaining = match battle.runtime[2].task() {
            crate::state::ActorTask::Hurt { remaining } => Some(*remaining),
            crate::state::ActorTask::Recovering {
                after_hit: true, ..
            } => None,
            _ => panic!("ordinary combo recovered before activation"),
        };
        let opener = opening(&mut battle);
        assert_eq!(battle.actors[2].reaction.combo_hits, 2);
        match remaining {
            Some(remaining) => assert!(matches!(
                battle.runtime[2].task(),
                crate::state::ActorTask::Hurt { remaining: current } if *current == remaining
            )),
            None => assert!(matches!(
                battle.runtime[2].task(),
                crate::state::ActorTask::Recovering {
                    after_hit: true,
                    ..
                }
            )),
        }
        let mut saw_opener = false;
        for _ in 0..35 {
            let frame = battle.step(Default::default()).unwrap();
            assert!(battle.actors[2].reaction.combo_hits >= 2);
            assert!(battle.actors[2].reaction.combo_damage >= 2);
            saw_opener |= frame.cues.iter().any(|cue| {
                matches!(
                    cue,
                    Cue::Hit {
                        actor,
                        source: crate::ContactSource::Melee { action, .. },
                        ..
                    } if *actor == target && *action == opener
                )
            });
            if frame.unison.unwrap().phase == UnisonPhase::Selecting {
                break;
            }
        }
        assert!(saw_opener);
        assert_eq!(battle.actors[2].reaction.combo_hits, 3);
        for _ in 0..20 {
            let frame = battle.step(Default::default()).unwrap();
            assert_eq!(frame.unison.unwrap().phase, UnisonPhase::Selecting);
            assert_eq!(battle.actors[2].reaction.combo_hits, 3);
        }
        battle.step(select(1)).unwrap();
        battle.step(Default::default()).unwrap();
        assert_eq!(battle.actors[2].reaction.combo_hits, 4);
    }
}

#[test]
fn opening_combo_handoff_releases_on_miss_and_excludes_finished_or_immune_targets() {
    for (expired, immune) in [(false, false), (true, false), (false, true)] {
        let mut battle = fixture(false);
        let target = ActorId(2);
        contact(&mut battle, ActionId(100));
        contact(&mut battle, ActionId(101));
        battle.step(Default::default()).unwrap();
        assert!(matches!(
            battle.runtime[2].task(),
            crate::state::ActorTask::Hurt { remaining: 1 }
        ));
        if expired {
            crate::tests::step_idle(&mut battle, 3).unwrap();
            assert_eq!(battle.actors[2].reaction.combo_hits, 0);
        }
        if immune {
            Arc::make_mut(battle.unison.definition.as_mut().unwrap())
                .immune_targets
                .insert(target);
        }
        // Another victim's recovery must keep running during this opener.
        battle.begin_hurt(ActorId(1), 1, &mut vec![]);
        battle.actors[1].reaction.combo_hits = 2;
        battle.actors[1].reaction.combo_damage = 10;
        Arc::make_mut(battle.unison.definition.as_mut().unwrap()).members[0].opener_contact_ticks =
            30;
        let opener = opening(&mut battle);
        let held = !expired && !immune;
        assert_eq!(battle.unison.holds_opening_combo(target), held);
        assert!(!battle.unison.holds_opening_combo(ActorId(1)));
        // An ineffective geometric contact cannot claim a different victim.
        battle.unison_opener_contact(ActorId(0), opener, ActorId(1), false);
        assert!(!battle.unison.holds_opening_combo(ActorId(1)));
        let mut cancelled = false;
        for _ in 0..30 {
            let frame = battle.step(Default::default()).unwrap();
            if frame.unison.is_none() {
                cancelled = true;
                break;
            }
            if held {
                assert_eq!(battle.actors[2].reaction.combo_hits, 2);
                assert_eq!(battle.actors[2].reaction.combo_damage, 2);
                assert!(matches!(
                    battle.runtime[2].task(),
                    crate::state::ActorTask::Hurt { remaining: 1 }
                ));
            }
        }
        assert!(cancelled);
        assert!(!battle.unison.holds_opening_combo(target));
        assert_eq!(battle.actors[1].reaction.combo_hits, 0);
        if held {
            assert_eq!(battle.actors[2].reaction.combo_hits, 2);
            assert!(matches!(
                battle.runtime[2].task(),
                crate::state::ActorTask::Hurt { remaining: 0 }
            ));
            crate::tests::step_idle(&mut battle, 2).unwrap();
        }
        assert_eq!(battle.actors[2].reaction.combo_hits, 0);
        assert_eq!(battle.actors[2].reaction.combo_damage, 0);
        assert_eq!(battle.activity(target), crate::Activity::Idle);
    }
}

#[test]
fn finisher_arte_blocks_unison_until_its_chain_ends() {
    for family in [
        None,
        Some(crate::ArteFamily::Basic),
        Some(crate::ArteFamily::Advanced),
        Some(crate::ArteFamily::Arcane),
        Some(crate::ArteFamily::Finisher),
    ] {
        let mut battle = fixture(false);
        battle.actors[0].tp = 10;
        battle.prepared.actor_setup[0].techniques[0]
            .capabilities
            .family = family;
        assert!(
            battle
                .start_actor_command(
                    ActionRequest {
                        actor: ActorId(0),
                        action: ActionKey(0),
                        target: ActorId(2),
                    },
                    &mut vec![],
                )
                .unwrap()
        );
        let input = BattleInput {
            unison: Some(UnisonInput::Activate(ActorId(0))),
            ..Default::default()
        };
        let blocked = family == Some(crate::ArteFamily::Finisher);
        assert_eq!(battle.step(input).unwrap().unison.is_none(), blocked);
        if blocked {
            assert_eq!(battle.unison_gauge(), crate::MAX_UNISON_GAUGE);
            // A subsequent familyless arte does not release the finisher lock.
            let mut capabilities = battle.prepared.actor_setup[0].techniques[0].capabilities;
            capabilities.family = None;
            battle.runtime[0]
                .combo
                .record(ActionKey(0), Some(capabilities));
            battle
                .step_unison(Some(UnisonInput::Activate(ActorId(0))), &mut vec![])
                .unwrap();
            assert!(!battle.unison.active());
            battle.interrupt_actor(ActorId(0), &mut vec![]);
            battle.enter_idle(ActorId(0));
            battle
                .step_unison(Some(UnisonInput::Activate(ActorId(0))), &mut vec![])
                .unwrap();
            assert!(battle.unison.active());
        }
    }
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
fn surviving_target_keeps_combo_through_unison_and_ordinary_recovery() {
    for combination in [false, true] {
        for (duration, followup, can_knock_down) in
            [(30, false, true), (60, true, true), (60, false, false)]
        {
            let mut battle = fixture(combination);
            let target = ActorId(2);
            battle.actors[2].hp = 10_000;
            battle.actors[2].equipment.max_hp = 10_000;
            battle.actors[2].reaction.stagger.duration = duration;
            battle.actors[2].reaction.profile.can_knock_down = can_knock_down;
            let action = opening(&mut battle);
            contact(&mut battle, action);
            contact(&mut battle, action);
            assert_eq!(battle.actors[2].reaction.combo_hits, 2);

            let mut selecting = 0;
            let mut second_selected = false;
            let mut saw_combination_pose = false;
            let mut saw_combined = false;
            let mut combo = (2, 2);
            let mut contributors = battle.actors[2].reaction.contributor_hits.clone();
            for _ in 0..300 {
                let run = battle.unison.run.as_ref().unwrap();
                if run.phase == UnisonPhase::Restoring {
                    // The held victim never visits ordinary recovery during
                    // the scene. Its final hit leaves a fresh down window.
                    assert!(!battle.actors[2].reaction.stagger.initialized);
                    assert!(!battle.actors[2].reaction.contact_recovery_command);
                    assert!(!battle.actors[2].reaction.contact_recovery_sampled);
                }
                let input = match run.phase {
                    // Keep the first selection open, then leave a gap between
                    // performers longer than either synthetic hit's stun.
                    UnisonPhase::Selecting if selecting == 60 => select(1),
                    UnisonPhase::Selecting => {
                        selecting += 1;
                        Default::default()
                    }
                    UnisonPhase::Executing if !second_selected && run.elapsed == 20 => {
                        second_selected = true;
                        select(2)
                    }
                    _ => Default::default(),
                };
                let frame = battle.step(input).unwrap();
                let reaction = &battle.actors[2].reaction;
                if frame.unison.is_none() && !can_knock_down {
                    assert_eq!((reaction.combo_hits, reaction.combo_damage), (0, 0));
                    break;
                }
                assert!(
                    reaction.combo_hits >= combo.0 && reaction.combo_damage >= combo.1,
                    "combo lost at {:?}: {:?} -> ({}, {})",
                    frame.unison.as_ref().map(|scene| scene.phase),
                    combo,
                    reaction.combo_hits,
                    reaction.combo_damage,
                );
                combo = (reaction.combo_hits, reaction.combo_damage);
                contributors.clone_from(&reaction.contributor_hits);
                if let Some(scene) = frame.unison {
                    saw_combination_pose |= scene.phase == UnisonPhase::CombinationPose;
                    saw_combined |= scene.phase == UnisonPhase::Combined;
                } else {
                    break;
                }
            }
            assert!(!battle.unison.active());
            assert!(second_selected && combo.0 > 2);
            assert_eq!([saw_combination_pose, saw_combined], [combination; 2]);
            if !can_knock_down {
                assert_eq!(battle.activity(target), crate::Activity::Idle);
                assert_eq!(battle.ledger.maximum_combo_damage, combo.1 as u32);
                continue;
            }
            assert_eq!(battle.activity(target), crate::Activity::KnockedDown);
            assert!(battle.actors[2].reaction.stagger.initialized);
            assert!(battle.actors[2].reaction.stagger.window > 0);
            assert!(matches!(
                battle.runtime[2].task(),
                crate::state::ActorTask::Knockdown(crate::knockdown::Recovery::Down {
                    remaining
                }) if *remaining == u32::from(duration) - 1
            ));
            if !followup {
                // With no follow-up, the existing down/get-up tasks own the
                // counter until they actually return the victim to idle.
                for _ in 0..u32::from(duration) + crate::knockdown::GET_UP_DURATION + 2 {
                    battle.step(Default::default()).unwrap();
                    let reaction = &battle.actors[2].reaction;
                    if battle.activity(target) == crate::Activity::Idle {
                        assert_eq!((reaction.combo_hits, reaction.combo_damage), (0, 0));
                        break;
                    }
                    assert_eq!((reaction.combo_hits, reaction.combo_damage), combo);
                    assert_eq!(reaction.contributor_hits, contributors);
                }
                assert_eq!(battle.activity(target), crate::Activity::Idle);
                assert_eq!(battle.ledger.maximum_combo_damage, combo.1 as u32);
                continue;
            }

            // An ordinary attack that can hit downed enemies continues the
            // received combo after control returns from the scene.
            let mut followup = hit(7, false);
            let rule = &mut Arc::make_mut(&mut followup).hit;
            rule.reaction.hits_down = true;
            rule.reaction.hitstun = 5;
            let mut contacts = crate::contact::Contacts::default();
            contacts
                .melee(ActorId(0), ActionId(999), &followup, &[])
                .unwrap();
            let mut cues = Vec::new();
            contacts.resolve(&mut battle, &mut cues, &[]).unwrap();
            assert_eq!(battle.actors[2].reaction.combo_hits, combo.0 + 1);
            assert_eq!(battle.actors[2].reaction.combo_damage, combo.1 + 7);
            assert_eq!(
                battle.actors[2].reaction.contributor_hits[0],
                contributors[0] + 1
            );
            assert!(cues.contains(&Cue::Combo {
                actor: target,
                hits: combo.0 + 1,
                damage: combo.1 + 7,
            }));
            for _ in 0..30 {
                battle.step(Default::default()).unwrap();
                if battle.activity(target) == crate::Activity::Idle {
                    break;
                }
            }
            assert_eq!(battle.activity(target), crate::Activity::Idle);
            assert_eq!(battle.actors[2].reaction.combo_hits, 0);
            assert_eq!(battle.actors[2].reaction.combo_damage, 0);
            assert_eq!(battle.actors[2].reaction.contributor_hits, vec![0; 3]);
            assert_eq!(battle.ledger.maximum_combo_damage, (combo.1 + 7) as u32);
        }
    }
}

#[test]
fn selected_artes_defer_defeat_then_restore_the_ordinary_battle() {
    for combination in [false, true] {
        let mut battle = fixture(combination);
        let mut model = crate::tests::model_definition(1, [(0, 100.), (1, 3.)]);
        model.idle_motions = [Some(0); 2];
        let model = Arc::new(model);
        let mut models = crate::Models::new(
            battle.actors(),
            vec![Some(model.clone()), Some(model), None],
            Default::default(),
            resonance_content::diagnostics::Diagnostics::new(true),
        )
        .unwrap();
        battle
            .prepare_shortcut(ActorId(1), crate::ShortcutSlot::Neutral, None)
            .unwrap()
            .commit();
        assert!(
            battle.unison.definition.as_ref().unwrap().members[1]
                .selected_arte(&battle, crate::ShortcutSlot::Neutral)
                .is_none()
        );
        battle
            .prepare_shortcut(ActorId(1), crate::ShortcutSlot::Neutral, Some(ActionKey(0)))
            .unwrap()
            .commit();
        let action = opening(&mut battle);
        battle.actors[2].reaction.combo_hits = 3;
        battle.actors[2].reaction.combo_damage = 30;
        battle.actors[2].reaction.contributor_hits[0] = 3;
        contact(&mut battle, action);
        advance_to_phase(&mut battle, UnisonPhase::Selecting, 4);
        let positions: Vec<_> = battle.actors.iter().map(|actor| actor.position).collect();
        let staged = battle.step(select(0b11)).unwrap().unison.unwrap();
        assert_eq!(staged.phase, UnisonPhase::Staging);
        assert_eq!(staged.selected, [ActorId(0)]);
        assert_eq!(battle.actors[2].reaction.combo_hits, 4);
        assert_eq!(battle.actors[2].reaction.combo_damage, 31);
        assert_eq!(battle.actors[2].reaction.contributor_hits[0], 4);
        let running = battle.step(select(0b10)).unwrap().unison.unwrap();
        assert_eq!(running.selected, [ActorId(0), ActorId(1)]);
        let mut saw_deferred = false;
        let mut saw_followup = false;
        let mut marked_defeat = false;
        let mut defeated = 0;
        let mut combined_phases = [false; 2];
        let mut charge_frames = [0.; 2];
        let mut combo = (4, 31);
        for _ in 0..160 {
            let mut frame = battle.step(Default::default()).unwrap();
            let clock = frame.clock;
            models.advance(&mut frame, clock, true).unwrap();
            if battle.actors[2].available() {
                let reaction = &battle.actors[2].reaction;
                assert!(reaction.combo_hits >= combo.0);
                assert!(reaction.combo_damage >= combo.1);
                combo = (reaction.combo_hits, reaction.combo_damage);
            }
            if let Some(unison) = &frame.unison {
                combined_phases[0] |= unison.phase == UnisonPhase::CombinationPose;
                combined_phases[1] |= unison.phase == UnisonPhase::Combined;
                if unison.phase == UnisonPhase::CombinationPose {
                    assert_eq!(unison.hidden, [ActorId(2)]);
                    for model in &frame.models {
                        assert_eq!(model.clip, 1, "charge pose reverted during the light scene");
                        assert!(model.frame >= charge_frames[model.actor.index()]);
                        charge_frames[model.actor.index()] = model.frame;
                    }
                } else if unison.phase == UnisonPhase::Combined {
                    assert!(unison.hidden.contains(&ActorId(1)));
                    assert!(!unison.hidden.contains(&ActorId(2)));
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
        assert_eq!(battle.ledger.maximum_combo, if combination { 8 } else { 6 });
        assert_eq!(battle.ledger.maximum_combo_damage, combo.1 as u32);
        assert_eq!(battle.actors[2].reaction.combo_hits, 0);
        assert_eq!(combined_phases, [combination; 2]);
        assert_eq!(charge_frames, [if combination { 3. } else { 0. }; 2]);
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
                    id: crate::UnisonRecipeId(1),
                    native_id: crate::UnisonArteId(301),
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
            assert_eq!(battle.technique_uses(actor, TechniqueId(1)), Some(1));
        }
    }
}
