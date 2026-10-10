use super::*;

#[path = "combo_tests/aerial_spell.rs"]
mod aerial_spell;

fn shared_prepared(mode: Control) -> PreparedBattle {
    let mut prepared = shortcuts_prepared(mode);
    for row in &mut prepared.resources.actions.entries[7..] {
        let row = Arc::make_mut(row);
        {
            row.execution = crate::ActionExecution::Attack(crate::PreparedAttack {
                opening: None,
                chain_at: Some(3),
                end_at: 12,
                recovery: 4,
                events: vec![],
            });
        }
    }
    //Three distinct no-EX families and one descriptor without a family bit.
    for row in &mut prepared.resources.actor_setup[0].techniques {
        row.capabilities.family = [
            Some(crate::ArteFamily::Basic),
            Some(crate::ArteFamily::Advanced),
            Some(crate::ArteFamily::Arcane),
            None,
        ][row.action.0 - 7];
        row.capabilities.offensive = true;
        row.capabilities.target = crate::TechniqueTarget::Enemy;
    }
    prepared.resources.actor_setup[0].companion = Some(crate::CompanionDefinition {
        initial_policy: [0; 3],
        defaults: [3, 6, 2],
        limits: [crate::PolicyLimits {
            tp: 30,
            healing: 65,
            support_level: 2,
        }; 9],
        level: 1,
        level_difference: 0,
    });
    let definition = crate::DecisionDefinition {
        idle_ticks: 0,
        idle_variation: 0,
    };
    prepared.resources.actor_setup[0].decision = Some(definition);
    prepared
        .with_technique_learning_members(vec![crate::tests::counted_techniques(
            ActorId(0),
            &[1, 35, 34, 4],
            &[(1, 49), (35, 49), (34, 49)],
        )])
        .unwrap()
}

fn shared_fixture(mode: Control) -> Battle {
    shared_prepared(mode).finish().unwrap()
}

fn actor_sequence(battle: &Battle) -> (ActionId, &crate::action::Sequence) {
    battle
        .sequences()
        .find_map(|(&id, sequence)| {
            (sequence.actor == ActorId(0)
                && matches!(
                    &sequence.definition.execution,
                    crate::ActionExecution::Attack(_)
                ))
            .then_some((id, sequence))
        })
        .expect("active actor action")
}

fn until_age(battle: &mut Battle, age: u32) -> Result<()> {
    for _ in 0..100 {
        if actor_sequence(battle).1.age >= age {
            return Ok(());
        }
        battle.step(BattleInput::default())?;
    }
    anyhow::bail!("actor did not reach requested age")
}

fn normal(battle: &mut Battle) -> Result<ActionId> {
    // These fixtures start with an existing normal; learning becomes eligible
    // for its successor. Initial-action learning has separate admission coverage.
    let learning = std::mem::take(&mut battle.learning_members);
    let started = battle.start_actor_command(
        crate::ActionRequest {
            actor: ActorId(0),
            action: crate::ActionKey(0),
            target: ActorId(1),
        },
        &mut vec![],
    );
    battle.learning_members = learning;
    started?;
    let id = actor_sequence(battle).0;
    until_age(battle, 1)?;
    Ok(id)
}

fn martial(battle: &mut Battle, action: crate::ActionKey) -> Result<ActionId> {
    battle.start_actor_command(
        ActionRequest {
            actor: ActorId(0),
            target: ActorId(1),
            action,
        },
        &mut vec![],
    )?;
    let id = actor_sequence(battle).0;
    until_age(battle, 1)?;
    Ok(id)
}

#[test]
fn buffered_shortcut_keeps_its_action_when_mapping_changes_and_pays_once() -> Result<()> {
    for (mode, shortcut, cursed) in [
        (Control::Manual, Some(crate::ActionKey(9)), false),
        (Control::SemiAuto, None, false),
        (Control::Manual, Some(crate::ActionKey(9)), true),
    ] {
        let mut battle = shared_fixture(mode);
        let old = normal(&mut battle)?;
        battle.actors[0].equipment.combo_traits.flash = true;
        battle.actors[0].hit_stop = 3;
        let age = actor_sequence(&battle).1.age;
        battle.step(player_buttons(false, true, false, [0, 80]))?;
        assert_eq!(actor_sequence(&battle).1.age, age);

        battle.prepare_shortcut(ActorId(0), 1, shortcut)?.commit();
        if cursed {
            battle.actors[0]
                .conditions
                .apply_hit(resonance_content::battle_action::HitCondition {
                    condition: resonance_content::battle_action::Condition::Curse,
                    chance: 100,
                    value: 0,
                });
        }

        until_age(&mut battle, 15)?;
        let before = battle.actors[0].tp;
        battle.step(BattleInput::default())?;
        if cursed {
            assert!(battle.sequence(&old).is_some());
            assert_eq!(battle.actors[0].tp, before);
            assert_eq!(battle.technique_uses(ActorId(0), 35), Some(49));
            continue;
        }
        assert!(battle.sequence(&old).is_none());
        assert_eq!(actor_sequence(&battle).1.action, crate::ActionKey(8));
        assert_eq!(actor_sequence(&battle).1.age, 0);
        assert_eq!(battle.technique_uses(ActorId(0), 34), Some(49));
        assert_eq!(battle.technique_uses(ActorId(0), 35), Some(50));
        assert_eq!(battle.actors[0].tp, before - 4);
        assert_eq!(
            battle.actors[0].reaction.protection.mode,
            crate::ProtectionMode::Armor
        );
        assert_eq!(battle.pending_technique(ActorId(0)), None);
        assert_eq!(battle.actors[0].proficiency, 1);
        battle.step(BattleInput::default())?;
        assert_eq!(battle.actors[0].tp, before - 4);
        assert_eq!(battle.actors[0].proficiency, 1);
    }
    Ok(())
}

#[test]
fn normal_combo_accepts_immediate_technique_input_and_chains_families() -> Result<()> {
    let mut battle = shared_fixture(Control::Manual);
    battle.step(player_buttons(true, false, false, [0; 2]))?;
    until_age(&mut battle, 1)?;
    battle.step(player_buttons(true, false, false, [80, 0]))?;
    until_age(&mut battle, 15)?;
    battle.step(BattleInput::default())?;
    let normal = actor_sequence(&battle).0;
    assert_eq!(actor_sequence(&battle).1.age, 0);
    battle.step(player_buttons(false, true, false, [0; 2]))?;
    until_age(&mut battle, 15)?;
    battle.step(BattleInput::default())?;
    let first = actor_sequence(&battle).0;
    assert!(battle.sequence(&normal).is_none());
    assert_eq!(actor_sequence(&battle).1.action, crate::ActionKey(7));
    assert_eq!(battle.ledger.normal_variety[0], 2);
    until_age(&mut battle, 1)?;
    battle.step(player_buttons(false, true, false, [0, 80]))?;
    until_age(&mut battle, 3)?;

    battle.step(BattleInput::default())?;
    assert!(battle.sequence(&first).is_none());
    assert_eq!(actor_sequence(&battle).1.action, crate::ActionKey(8));
    assert_eq!(battle.technique_uses(ActorId(0), 1), Some(50));
    assert_eq!(battle.technique_uses(ActorId(0), 35), Some(50));
    assert_eq!(battle.actors[0].tp, 32);
    battle.step(BattleInput::default())?;
    assert_eq!(battle.actors[0].tp, 32);
    Ok(())
}

#[test]
fn queued_combo_revalidates_target_and_cost_after_final_contacts() -> Result<()> {
    for (affordable, survives, pause) in [
        (false, true, false),
        (true, true, false),
        (true, false, false),
        (true, true, true),
    ] {
        let mut prepared = shared_prepared(Control::Auto);
        let action = Arc::make_mut(&mut prepared.resources.actions.entries[0]);
        action.execution = crate::ActionExecution::Attack(crate::PreparedAttack {
            opening: None,
            chain_at: Some(15),
            end_at: 15,
            recovery: 4,
            events: vec![(
                15,
                crate::AttackEvent::Contact {
                    duration: 1,
                    definition: Arc::new(crate::MeleeDefinition {
                        hit: crate::HitRule {
                            kind: crate::DamageKind::Slash,
                            arte: false,
                            overlimit_pause: pause,
                            power: crate::Power::Fixed(5),
                            element: crate::HitElement::Neutral,
                            prevents_defeat: false,
                            guard: Default::default(),
                            reaction: Default::default(),
                            condition: None,
                        },
                        trail: None,
                        volume: crate::MeleeVolume {
                            offset: [0., 0., 20.],
                            radius: 10.,
                            half_height: 1.,
                        },
                    }),
                },
            )],
        });

        prepared.actors[2].body.collider = Some(crate::Collider::sphere(1.));
        prepared.actors[2].hp = if survives { 50 } else { 5 };
        if pause {
            prepared.actors[2].overlimit = crate::OverLimit::active(100)?;
            prepared.resources.actor_setup[2].overlimit_gain = 1;
        }
        let mut battle = prepared.finish()?;
        let parent = normal(&mut battle)?;
        assert!(battle.queue_technique_target(ActorId(0), crate::ActionKey(7), ActorId(2))?);
        battle.actors[0].tp = if affordable { 40 } else { 0 };
        until_age(&mut battle, 15)?;
        battle.actors[2].position = battle.actors[0].position;
        battle.actors[2].position[0] += 20.;
        let frame = battle.step(BattleInput::default())?;
        assert!(frame.cues.iter().any(|cue| matches!(cue,
            Cue::Hit { actor: ActorId(2), source: crate::ContactSource::Melee { action, .. }, .. }
                if *action == parent)));
        assert_eq!(
            battle.actors[2].hp,
            if pause {
                48
            } else if survives {
                45
            } else {
                0
            }
        );
        if pause {
            assert!(battle.is_paused());
            assert_eq!(frame.clock, crate::BattleClock::Running);
        }
        assert!(battle.actors[1].available(), "another opponent must remain");
        let starts = affordable && survives;
        assert_eq!(
            battle.technique_uses(ActorId(0), 1),
            Some(if starts { 50 } else { 49 })
        );
        assert_eq!(
            actor_sequence(&battle).1.action,
            if starts {
                crate::ActionKey(7)
            } else {
                crate::ActionKey(0)
            }
        );
        assert_eq!(
            battle.actors[0].tp,
            if starts {
                36
            } else if affordable {
                40
            } else {
                1
            }
        );
        if starts {
            assert_eq!(actor_sequence(&battle).1.target, ActorId(2));
            let next = battle.step(BattleInput::default())?;
            assert_eq!(next.clock.paused(), pause);
            if pause {
                assert_eq!(next.actions, frame.actions);
            }
            assert_eq!(battle.actors[0].tp, 36);
            assert_eq!(battle.pending_technique(ActorId(0)), None);
        } else if !survives {
            assert_eq!(battle.pending_technique(ActorId(0)), None);
            assert!(!frame.cues.contains(&Cue::Completed { action: parent }));
        } else {
            assert_eq!(
                battle.pending_technique(ActorId(0)),
                Some(crate::ActionKey(7))
            );
        }
    }
    Ok(())
}

#[test]
fn only_explicitly_capable_techniques_chain_without_contact() -> Result<()> {
    for permitted in [false, true] {
        let mut prepared = shared_prepared(Control::Auto);
        prepared.resources.actor_setup[0].techniques[3]
            .capabilities
            .chains_without_contact = permitted;
        let mut battle = prepared.finish()?;
        martial(&mut battle, crate::ActionKey(10))?;
        assert!(battle.queue_technique(ActorId(0), crate::ActionKey(8))?);
        until_age(&mut battle, 3)?;
        battle.step(BattleInput::default())?;
        assert_eq!(
            actor_sequence(&battle).1.action,
            if permitted {
                crate::ActionKey(8)
            } else {
                crate::ActionKey(10)
            }
        );
    }
    Ok(())
}

fn regal_fixture(mode: Control) -> Battle {
    let mut prepared = shared_prepared(mode);
    for (i, (catalogue, family)) in [
        (176, crate::RegalArteFamily::AntiAir),
        (177, crate::RegalArteFamily::Ground),
        (185, crate::RegalArteFamily::Aerial),
    ]
    .into_iter()
    .enumerate()
    {
        let capabilities = crate::TechniqueCapabilities {
            regal_family: Some(family),
            offensive: true,
            target: crate::TechniqueTarget::Enemy,
            ..Default::default()
        };
        prepared.resources.actor_setup[0].techniques[i].capabilities = capabilities;
        prepared.resources.actor_setup[0].techniques[i].catalogue = catalogue;
    }
    Arc::make_mut(prepared.resources.actor_setup[0].control.as_mut().unwrap()).shortcuts =
        [176, 177, 185, 0];
    prepared = prepared
        .with_technique_learning_members(vec![crate::tests::counted_techniques(
            ActorId(0),
            &[176, 177, 185],
            &[(176, 49), (177, 49), (185, 49)],
        )])
        .unwrap();
    prepared.finish().unwrap()
}

#[test]
fn requested_chain_bypasses_enabled_but_requires_current_membership_without_payment() -> Result<()>
{
    for learned in [false, true] {
        let mut battle = regal_fixture(Control::Auto);
        assert!(battle.queue_technique(ActorId(0), crate::ActionKey(7))?);
        battle.runtime[0]
            .control
            .as_mut()
            .unwrap()
            .disabled_techniques
            .insert(crate::ActionKey(7));
        if !learned {
            battle.learning_members[0].member.forget(176)?;
        }

        let selected = battle.queue_pending_technique_chain(ActorId(0))?;
        assert_eq!(selected, Some(learned));
        assert_eq!(
            battle.pending_technique(ActorId(0)),
            Some(crate::ActionKey(7))
        );
        assert_eq!(battle.actors[0].tp, 40);
        assert_eq!(battle.technique_uses(ActorId(0), 176), Some(49));
    }
    Ok(())
}

#[test]
fn automatic_arte_does_not_fall_back_to_a_normal() -> Result<()> {
    for action in [crate::ActionKey(7), crate::ActionKey(10)] {
        let mut battle = shared_fixture(Control::Auto);
        normal(&mut battle)?;
        assert!(battle.queue_technique(ActorId(0), action)?);
        until_age(&mut battle, 15)?;
        battle.confirm_actor_contact(ActorId(0));
        battle.step(BattleInput::default())?;
        assert_eq!(actor_sequence(&battle).1.action, action);
        until_age(&mut battle, 3)?;
        let old = actor_sequence(&battle).0;

        battle.runtime[0]
            .control
            .as_mut()
            .unwrap()
            .disabled_techniques
            .extend((7..11).map(crate::ActionKey));
        battle.confirm_actor_contact(ActorId(0));

        battle.step(BattleInput::default())?;
        assert!(battle.sequence(&old).is_some());
    }
    Ok(())
}

fn add_chain_learning(
    mut prepared: PreparedBattle,
    action: crate::ActionKey,
    seed: u64,
    current: &[u16],
) -> Result<PreparedBattle> {
    let mut table = resonance_content::arte::Catalogue {
        definitions: vec![Default::default(); 35],
        learning: vec![vec![1, 34]],
    };
    table.definitions[1].required_level = 1;
    let catalogue = crate::learning::LearningCatalogue::new(Arc::new(table));
    prepared.random_seed = seed;
    Arc::make_mut(prepared.resources.actor_setup[0].control.as_mut().unwrap()).shortcuts = [0; 4];
    let row = prepared.resources.actor_setup[0]
        .techniques
        .iter_mut()
        .find(|row| row.action == action)
        .expect("prepared learning action");
    row.catalogue = 1;
    prepared.with_technique_learning_members(vec![crate::learning::TechniqueLearningMember {
        actor: ActorId(0),
        member: catalogue.prepare_member(crate::learning::LearningEntry {
            character: 1,
            level: 1,
            balance: 0,
            story_unlocked: true,
            current: current.iter().copied().collect(),
            counts: [(1, 49), (34, 49)].into_iter().collect(),
        })?,
    }])
}

fn learning_chain_fixture(mode: Control, seed: u64) -> Result<Battle> {
    let prepared = add_chain_learning(shared_prepared(mode), crate::ActionKey(7), seed, &[34])?;
    Ok(prepared.finish().unwrap())
}

#[test]
fn initial_shortcut_and_assist_reject_dormant_techniques() -> Result<()> {
    for assist in [false, true] {
        let mut prepared = add_chain_learning(
            shared_prepared(Control::Manual),
            crate::ActionKey(7),
            6,
            &[34],
        )?;
        let setup = &mut prepared.resources.actor_setup[0];
        if assist {
            setup.assist_shortcuts[0] = Some((ActorId(0), crate::ActionKey(7)));
        } else {
            Arc::make_mut(setup.control.as_mut().unwrap()).shortcuts[0] = 1;
        }
        assert!(
            prepared
                .finish()
                .err()
                .unwrap()
                .to_string()
                .contains("not learned")
        );
    }
    Ok(())
}

#[test]
fn rejected_or_interrupted_normal_chain_does_not_attempt_learning() -> Result<()> {
    for interrupt in [false, true] {
        let mut battle = learning_chain_fixture(Control::Manual, 6)?;
        let old = normal(&mut battle)?;
        until_age(&mut battle, 15)?;
        if interrupt {
            battle.actors[0].hit_stop = 2;
            battle.step(player_buttons(true, false, false, [80, 0]))?;
            battle.step(BattleInput {
                interrupt: vec![old],
                ..Default::default()
            })?;
            assert!(battle.sequence(&old).is_none());
        } else {
            battle.actors[0].equipment.normal_combo_limit = 1;
            battle.step(player_buttons(true, false, false, [80, 0]))?;
            assert_eq!(actor_sequence(&battle).0, old);
        }
        assert!(battle.technique_acquisitions().is_empty());
        assert_eq!(battle.technique_uses(ActorId(0), 1), Some(49));
        assert_eq!(battle.actors[0].tp, 40);
    }
    Ok(())
}

#[test]
fn buffered_normal_chain_starts_once_without_payment() -> Result<()> {
    let mut battle = add_chain_learning(
        shared_prepared(Control::Manual),
        crate::ActionKey(7),
        0,
        &[1, 34],
    )?
    .finish()?;
    let started = |cues: &[Cue]| {
        cues.iter()
            .filter_map(|cue| match cue {
                Cue::Started {
                    action,
                    actor: ActorId(0),
                    ..
                } => Some(*action),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let mut frame = battle.step(player_buttons(true, false, false, [0; 2]))?;
    for _ in 0..16 {
        if !started(&frame.cues).is_empty() {
            break;
        }
        frame = battle.step(BattleInput::default())?;
    }
    let initial = started(&frame.cues);
    assert_eq!(
        initial.len(),
        1,
        "the attack must start once within the admission bound"
    );
    assert_eq!(actor_sequence(&battle).1.action, crate::ActionKey(0));
    until_age(&mut battle, 1)?;
    let mut chained = started(
        &battle
            .step(player_buttons(true, false, false, [80, 0]))?
            .cues,
    );
    for _ in 0..32 {
        chained.extend(started(&battle.step(BattleInput::default())?.cues));
    }
    assert_eq!(
        chained.len(),
        1,
        "one buffered attack must start one successor"
    );
    assert!(battle.sequence(&initial[0]).is_none());
    assert_eq!(actor_sequence(&battle).0, chained[0]);
    assert_eq!(actor_sequence(&battle).1.action, crate::ActionKey(3));
    assert_eq!(actor_sequence(&battle).1.target, ActorId(1));

    assert!(battle.technique_acquisitions().is_empty());
    assert_eq!(battle.technique_uses(ActorId(0), 1), Some(49));
    assert_eq!(battle.actors[0].tp, 40);
    Ok(())
}

#[test]
fn sky_combo_allows_airborne_normals_to_continue_rising() -> Result<()> {
    for (mode, enabled) in [
        (Control::Manual, false),
        (Control::Manual, true),
        (Control::Auto, false),
        (Control::Auto, true),
    ] {
        let mut battle = shared_fixture(mode);
        battle.runtime[0]
            .control
            .as_mut()
            .unwrap()
            .disabled_techniques = battle.prepared.actor_setup[0]
            .techniques
            .iter()
            .map(|technique| technique.action)
            .collect();
        battle.actors[0].equipment.combo_traits.sky_combo = enabled;
        let definition = battle.prepared.actor_setup[0]
            .control
            .as_ref()
            .unwrap()
            .clone();
        battle.actors[0].position[1] = 400.;
        battle.start_control_normal(
            ActorId(0),
            &definition,
            NormalAttack::AerialSlash,
            &mut vec![],
        )?;
        let id = actor_sequence(&battle).0;
        until_age(&mut battle, 14)?;
        battle.actors[0].position[1] = 100.;
        battle.actors[0].movement.vertical = -9.;
        battle.confirm_actor_contact(ActorId(0));
        battle.step(player_buttons(true, false, false, [0; 2]))?;
        battle.step(BattleInput::default())?;
        assert_eq!(battle.sequence(&id).is_none(), enabled);
        if enabled {
            let height = battle.actors[0].position[1];
            battle.step(BattleInput::default())?;
            assert!(battle.actors[0].movement.vertical > 0.);
            assert!(battle.actors[0].position[1] > height);
        }
        battle.actors[0].control = Control::Manual;
        for _ in 0..200 {
            if battle.activity(ActorId(0)) == Activity::Idle {
                break;
            }
            battle.step(BattleInput::default())?;
        }
        assert_eq!(battle.activity(ActorId(0)), Activity::Idle);
        assert_eq!(battle.actors[0].position[1], 0.);
        battle.step(player_buttons(true, false, false, [0; 2]))?;
        wait_for_action(&mut battle, crate::ActionKey(0))?;
        battle.step(BattleInput::default())?;
        assert_eq!(
            battle.actors[0].position[1], 0.,
            "a fresh grounded attack must not inherit aerial lift"
        );
    }
    Ok(())
}

#[test]
fn combo_force_retains_more_power_and_flash_protects_the_successor() -> Result<()> {
    for power in [100, 1] {
        let mut powers = Vec::new();
        for boosted in [false, true] {
            let mut battle = shared_fixture(Control::Manual);
            battle.actors[0].equipment.combo_traits.flash = true;
            battle.actors[0].equipment.combo_traits.combo_force = boosted;
            let id = normal(&mut battle)?;
            battle.actors[0].attack_power = power;
            battle.step(player_buttons(true, false, false, [0; 2]))?;
            assert_eq!(battle.actors[0].attack_power, power);
            assert_eq!(
                battle.actors[0].reaction.protection.mode,
                crate::ProtectionMode::None
            );
            until_age(&mut battle, 15)?;
            battle.step(BattleInput::default())?;
            assert!(battle.sequence(&id).is_none());
            assert_eq!(
                battle.actors[0].reaction.protection.mode,
                crate::ProtectionMode::Armor
            );
            powers.push(battle.actors[0].attack_power);
        }
        assert!(powers[0] > 0 && powers[1] > powers[0]);
    }
    Ok(())
}

#[test]
fn counter_combo_trait_allows_the_buffered_attack_to_continue() -> Result<()> {
    let mut battle = shared_fixture(Control::Manual);
    let id = normal(&mut battle)?;
    battle.actors[0].control_ex_state.counter_active = true;
    battle.step(player_buttons(true, false, false, [0; 2]))?;
    until_age(&mut battle, 15)?;
    battle.step(BattleInput::default())?;
    assert!(battle.sequence(&id).is_some());
    assert_eq!(battle.actors[0].attack_power, 100);
    battle.actors[0].equipment.combo_traits.counter_combo = true;
    battle.step(BattleInput::default())?;
    assert!(battle.sequence(&id).is_none());
    assert!(battle.actors[0].attack_power < 100);
    Ok(())
}

#[test]
fn landing_accepts_jump_combo_input_and_preserves_a_queued_technique() -> Result<()> {
    for jump_combo in [false, true] {
        for technique in [false, true] {
            let mut prepared = shared_prepared(Control::Manual);
            let actor = &mut prepared.actors[0];
            actor.equipment.combo_traits.jump_combo = jump_combo;

            let mut battle = prepared.finish()?;
            for _ in 0..12 {
                battle.step(player_buttons(false, false, false, [0, 80]))?;
                if battle.activity(ActorId(0)) == Activity::Jumping
                    && battle.actors()[0].position[1] > 0.1
                {
                    break;
                }
            }
            assert_eq!(battle.activity(ActorId(0)), Activity::Jumping);
            assert!(battle.actors()[0].position[1] > 0.1);
            let tp = battle.actors()[0].tp;
            let mut started = false;
            for update in 0..12 {
                let frame = battle.step(player_buttons(update == 0, false, false, [0; 2]))?;
                if frame.cues.iter().any(|cue| {
                    matches!(cue,
                    Cue::Started { actor: ActorId(0), action, .. }
                        if battle.action_definition(*action) == Some(crate::ActionKey(5)))
                }) {
                    started = true;
                    break;
                }
            }
            assert!(started, "airborne normal never started");

            let mut landed = false;
            let mut successors = Vec::new();
            for _ in 0..60 {
                // Submit the shortcut at the physical landing boundary. Recovery
                // must keep that request until its next action can be admitted.
                let input = player_buttons(
                    false,
                    technique
                        && battle.actors()[0].movement.vertical < 0.
                        && battle.actors()[0].position[1] + battle.actors()[0].movement.vertical
                            <= 0.1,
                    false,
                    [0; 2],
                );
                let frame = battle.step(input)?;
                for cue in &frame.cues {
                    if let Cue::Started {
                        actor: ActorId(0),
                        action,
                        ..
                    } = cue
                    {
                        successors.push(battle.action_definition(*action).unwrap());
                    }
                }
                if frame.actors[0].activity == Activity::Recovering || !successors.is_empty() {
                    assert!(
                        frame.actors[0].position[1] <= 0.1,
                        "successor started before landing"
                    );
                    landed = true;
                    break;
                }
            }
            assert!(landed, "airborne normal never completed landing");
            for update in 0..60 {
                let frame = battle.step(player_buttons(
                    !technique && update == 0,
                    false,
                    false,
                    [0; 2],
                ))?;
                for cue in &frame.cues {
                    if let Cue::Started {
                        actor: ActorId(0),
                        action,
                        ..
                    } = cue
                    {
                        successors.push(battle.action_definition(*action).unwrap());
                    }
                }
            }
            assert_eq!(
                successors,
                if technique {
                    vec![crate::ActionKey(7)]
                } else if jump_combo {
                    vec![crate::ActionKey(0)]
                } else {
                    vec![]
                }
            );
            assert_eq!(battle.actors()[0].tp, tp - if technique { 4 } else { 0 });
            if !technique && !jump_combo {
                assert_eq!(battle.activity(ActorId(0)), Activity::Idle);
            }
        }
    }
    Ok(())
}

#[test]
fn committed_family_chains_obey_ability_plus_and_super_chain() -> Result<()> {
    for (actions, ability_plus, super_chain, last_allowed) in [
        (
            vec![crate::ActionKey(7), crate::ActionKey(7)],
            true,
            false,
            false,
        ),
        (
            vec![crate::ActionKey(7), crate::ActionKey(8)],
            false,
            false,
            false,
        ),
        (
            vec![crate::ActionKey(7), crate::ActionKey(8)],
            true,
            false,
            true,
        ),
        (
            vec![
                crate::ActionKey(7),
                crate::ActionKey(8),
                crate::ActionKey(7),
            ],
            true,
            false,
            false,
        ),
        (
            vec![crate::ActionKey(9), crate::ActionKey(10)],
            false,
            false,
            false,
        ),
        (
            vec![
                crate::ActionKey(9),
                crate::ActionKey(10),
                crate::ActionKey(7),
            ],
            false,
            true,
            true,
        ),
    ] {
        let mut prepared = shared_prepared(Control::Manual);
        prepared.resources.actor_setup[0].techniques[1]
            .capabilities
            .family = Some(crate::ArteFamily::Basic);
        prepared.resources.actor_setup[0].techniques[3]
            .capabilities
            .family = Some(crate::ArteFamily::Advanced);
        let mut battle = prepared.finish()?;
        battle.actors[0].equipment.combo_traits.ability_plus = ability_plus;
        battle.actors[0].equipment.combo_traits.super_chain = super_chain;
        martial(&mut battle, actions[0])?;
        for (index, &action) in actions.iter().enumerate().skip(1) {
            let old = actor_sequence(&battle).0;
            let tp = battle.actors[0].tp;
            battle
                .prepare_shortcut(ActorId(0), 0, Some(action))?
                .commit();
            battle.step(player_buttons(false, true, false, [0; 2]))?;
            until_age(&mut battle, 3)?;
            battle.step(BattleInput::default())?;
            let allowed = index + 1 < actions.len() || last_allowed;
            assert_eq!(battle.sequence(&old).is_none(), allowed, "{actions:?}");
            assert_eq!(battle.actors[0].tp, tp - if allowed { 4 } else { 0 });
        }
    }
    Ok(())
}
