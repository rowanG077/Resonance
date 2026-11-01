use super::*;
use crate::{Control, DecisionDefinition, Side};
use crate::{NormalAttack, PreparedBattle, TechniqueId};
use std::sync::Arc;

#[path = "movement_tests.rs"]
mod movement_tests;

#[path = "probability_tests.rs"]
mod probability_tests;

// Focused tests borrow the production view and own the random stream.
fn selector<'a, 'r>(
    battle: &'a Battle,
    owner: ActorId,
    random: &'r mut crate::Random,
    policy: Option<ResolvedPolicy>,
) -> selection::Selection<'a, 'r> {
    selection::Selection {
        battle: &battle,
        owner,
        policy: policy.unwrap_or_else(|| battle.companion_policy(owner).unwrap()),
        random,
    }
}

pub(crate) fn inert_decision(prepared: &mut PreparedBattle, actor: ActorId) -> DecisionDefinition {
    let definition = DecisionDefinition {
        idle_ticks: 0,
        idle_variation: 0,
    };
    prepared.resources.actor_setup[actor.index()].decision = Some(definition);
    definition
}

pub(crate) fn prepared() -> Result<PreparedBattle> {
    let mut actions = crate::ActionDefinitions::default();
    let normals = crate::tests::normal_controls(&mut actions, crate::tests::attack(30), [0., 120.]);
    let actors: Vec<_> = [
        Control::Manual,
        Control::SemiAuto,
        Control::Auto,
        Control::Enemy,
        Control::Enemy,
    ]
    .into_iter()
    .enumerate()
    .map(|(index, control)| {
        let mut actor = crate::tests::actor(if index < 3 { Side::Party } else { Side::Enemy });
        actor.control = control;
        actor.position = [
            [-300., 0., 100.],
            [-300., 0., -100.],
            [0.; 3],
            [200., 0., 0.],
            [900., 0., 0.],
        ][index];
        actor.body.collider = Some(crate::Collider::sphere(10.));
        actor.guard.auto_chance = 67;
        actor.guard.pressure = 9;
        actor.guard.recent_hurt_ticks = 11;
        actor
    })
    .collect();
    let setup: Vec<_> = (0..5)
        .map(|i| {
            if i >= 3 {
                return crate::ActorSetup::default();
            }
            crate::ActorSetup {
                control: Some(Arc::new(crate::ControlDefinition {
                    walk_speed: 3.,
                    run_speed: 6.,
                    turn_ticks: 8,
                    motions: None,
                    shortcuts: [None; 4],
                    normals,
                })),
                companion: Some(CompanionDefinition {
                    character: crate::Character::Lloyd,
                    target_strategies: crate::TargetPolicy::PARTY,
                    positioning: crate::tests::companion_positioning(),
                    normal_selection: crate::tests::companion_normal_selection(),
                    initial_policy: [0; 3],
                    defaults: [1, 5, 2],
                    limits: std::array::from_fn(|i| PolicyLimits {
                        tp_reserve_percent: i as u8 * 10,
                        healing_threshold_percent: 90 - i as u8 * 10,
                        buff_max_level_advantage: 8,
                        idle_tp_reserve_percent: i as u8 * 10,
                    }),
                }),
                decision: Some(DecisionDefinition {
                    idle_ticks: 0,
                    idle_variation: 0,
                }),
                ..crate::ActorSetup::default()
            }
        })
        .collect();
    let mut prepared =
        PreparedBattle::new((actors).into_iter().zip(setup).collect(), actions, 0x3456)?;
    prepared.resources.actions.insert(recovery_cast(
        crate::SpellBenefit::Hp(30),
        crate::RecoveryTargets::Target,
        0,
    ));
    Ok(prepared)
}

fn recovery_cast(
    benefit: crate::SpellBenefit,
    targets: crate::RecoveryTargets,
    cost: u16,
) -> crate::ActionDefinition {
    let mut action = crate::tests::cast(30, cost);
    let crate::ActionExecution::Casting(cast) = &mut action.execution else {
        unreachable!()
    };
    let release = Arc::make_mut(&mut Arc::make_mut(cast).release);
    release.projectiles.clear();
    release.recovery = vec![crate::SpellRecovery {
        at: 0,
        duration: 1,
        benefit,
        targets,
        effect: None,
        effect_placement: Default::default(),
        flash: None,
    }];
    action
}

fn rows(battle: &Battle, choices: [u8; 3]) -> Vec<StrategyRefresh> {
    battle
        .actors
        .iter()
        .enumerate()
        .filter(|(_, actor)| actor.side == Side::Party)
        .map(|(index, _)| StrategyRefresh {
            actor: ActorId(index as u8),
            choices,
        })
        .collect()
}

fn technique(
    action: crate::ActionKey,
    catalogue: u16,
    capabilities: crate::TechniqueCapabilities,
) -> crate::PreparedTechnique {
    crate::PreparedTechnique {
        capabilities,
        player_range: [0., 800.],
        ai_range: [0., 800.],
        ..crate::tests::technique(action, catalogue)
    }
}

fn spell_capabilities(healing: bool) -> crate::TechniqueCapabilities {
    crate::TechniqueCapabilities {
        spell: true,
        offensive: !healing,
        healing,
        target: if healing {
            crate::TechniqueTarget::Ally
        } else {
            crate::TechniqueTarget::Enemy
        },
        ..Default::default()
    }
}

fn prepared_with_spell(capabilities: crate::TechniqueCapabilities) -> Result<PreparedBattle> {
    let mut prepared = prepared()?;
    prepared.resources.actor_setup[2].techniques =
        vec![technique(crate::ActionKey(7), 66, capabilities)];
    Ok(prepared)
}

#[test]
fn revival_prefers_the_player_controlled_fallen_ally_then_the_formation_leader() -> Result<()> {
    for (second_control, expected) in [(Control::Manual, ActorId(1)), (Control::Auto, ActorId(0))] {
        let mut prepared = prepared_with_spell(crate::TechniqueCapabilities {
            spell: true,
            revives: true,
            target: crate::TechniqueTarget::Ally,
            ..Default::default()
        })?;
        prepared.resources.actions.entries[7] = Arc::new(recovery_cast(
            crate::SpellBenefit::Resurrect(30),
            crate::RecoveryTargets::Target,
            48,
        ));
        for index in [0, 1] {
            prepared.actors[index].equipment.max_hp = 2000;
            prepared.actors[index].hp = 0;
            prepared.actors[index].availability = crate::ActorAvailability::Dead;
        }
        prepared.actors[0].control = Control::Auto;
        prepared.actors[1].control = second_control;
        prepared.actors[2].equipment.max_tp = 100;
        prepared.actors[2].tp = 100;
        prepared.resources.actor_setup[2]
            .companion
            .as_mut()
            .unwrap()
            .initial_policy = [1, 8, 2];
        let mut battle = prepared.finish()?;
        battle.advance_companion_ai(ActorId(2), &mut Vec::new())?;
        assert_eq!(battle.runtime[2].support_target, expected);
        for _ in 0..120 {
            battle.step(crate::BattleInput::default())?;
            if battle.actors[expected.index()].available() {
                break;
            }
        }
        assert_eq!(battle.actors[expected.index()].hp, 600);
        assert_eq!(battle.actors[2].tp, 52);
    }
    Ok(())
}

#[test]
fn strategy_changes_only_policy_and_applies_atomically() -> Result<()> {
    let mut prepared = prepared()?;
    prepared.resources.actor_setup[2].decision = None;
    let mut battle = prepared.finish()?;
    battle.actors[2].hp = 42;
    battle.actors[2].movement.forward = 3.;
    battle.runtime[2].idle_timer = 17;
    let mut refresh = rows(&battle, [2, 1, 6]);
    refresh[2].choices = [255, 0, 0];
    assert!(battle.refresh_strategy(&refresh).is_err());
    assert_eq!(battle.companion_choices(ActorId(0)).unwrap(), [0; 3]);
    battle.refresh_strategy(&rows(&battle, [2, 1, 6]))?;
    assert_eq!(
        battle.companion_policy(ActorId(2)).unwrap().position,
        PositionStrategy::SkillsAndMagic
    );
    assert_eq!(
        battle.actors[2].guard.recovery_bonus,
        strategy_guard_recovery(6)
    );
    assert_eq!(
        (battle.actors[2].hp, battle.runtime[2].idle_timer),
        (42, 17)
    );
    assert_eq!(battle.actors[2].movement.forward, 3.);
    assert!(battle.refresh_strategy(&refresh[..2]).is_err());
    Ok(())
}

#[test]
fn normal_fallback_repeats_scatter_and_keeps_modded_target_handlers() -> Result<()> {
    let mut prepared = prepared()?;
    prepared.resources.actor_setup[2]
        .companion
        .as_mut()
        .unwrap()
        .defaults = [4, 5, 2];
    let mut battle = prepared.finish()?;
    battle.set_decision_target(ActorId(0), ActorId(3))?;
    battle.set_decision_target(ActorId(1), ActorId(4))?;
    let mut expected = battle.random;
    // Both opponents are claimed. Two 16-draw target visits plus the normal
    // direction draw agree with the original selector's exhausted path.
    for _ in 0..33 {
        expected.next_u16();
    }
    battle.advance_companion_ai(ActorId(2), &mut Vec::new())?;
    assert_eq!(battle.random.state(), expected.state());
    assert_eq!(battle.target(ActorId(2)), Some(ActorId(3)));

    let mut prepared = self::prepared()?;
    let definition = prepared.resources.actor_setup[2]
        .companion
        .as_mut()
        .unwrap();
    definition.defaults = [1, 5, 2];
    definition.target_strategies[0] = crate::TargetPolicy::Farthest;
    let mut battle = prepared.finish()?;
    assert_eq!(
        battle.prepared.actor_setup[2]
            .companion
            .as_ref()
            .unwrap()
            .resolve([0; 3])?
            .target,
        crate::TargetPolicy::Farthest
    );
    battle.refresh_strategy(&rows(&battle, [1, 5, 2]))?;
    battle.advance_companion_ai(ActorId(2), &mut Vec::new())?;
    assert_eq!(battle.target(ActorId(2)), Some(ActorId(4)));
    Ok(())
}

#[path = "command_tests.rs"]
mod command_tests;

#[test]
fn repertoire_tracks_acquisition_forgetting_and_live_enable_masks() -> Result<()> {
    let mut prepared = prepared_with_spell(spell_capabilities(false))?;
    let owner = ActorId(2);
    prepared.resources.actor_setup[2].techniques[0].catalogue = TechniqueId(1);
    let mut battle = prepared
        .with_technique_learning_members(vec![crate::tests::counted_techniques(
            owner,
            &[],
            &[(1, 0)],
        )])?
        .finish()?;
    let mut random = battle.random;
    assert!(
        selector(&battle, owner, &mut random, None)
            .techniques()
            .next()
            .is_none()
    );
    battle.record_technique_acquisition(owner, TechniqueId(1))?;
    assert_eq!(
        selector(&battle, owner, &mut random, None)
            .techniques()
            .count(),
        1
    );
    assert!(
        selector(&battle, owner, &mut random, None)
            .techniques()
            .next()
            .is_some()
    );
    battle.set_technique_enabled(owner, crate::ActionKey(7), false)?;
    assert!(
        battle
            .learned_technique(owner, crate::ActionKey(7))
            .is_some()
    );
    assert!(
        selector(&battle, owner, &mut random, None)
            .techniques()
            .next()
            .is_none()
    );
    battle.forget_technique(owner, crate::ActionKey(7))?;
    assert!(
        selector(&battle, owner, &mut random, None)
            .techniques()
            .next()
            .is_none()
    );
    battle.record_technique_acquisition(owner, TechniqueId(1))?;
    assert!(
        selector(&battle, owner, &mut random, None)
            .techniques()
            .next()
            .is_some()
    );
    Ok(())
}

#[test]
fn idle_charge_accumulates_once_per_visit_including_action_admission() -> Result<()> {
    let mut battle = prepared()?.finish().unwrap();
    let owner = ActorId(2);
    battle.actors[2].equipment.control_ex.charge = true;
    let slot = battle.actors[2].control_slot.index();
    battle.cast_inputs[slot].attack_held = true;
    battle.runtime[2].idle_timer = 2;
    for expected in 1..=3 {
        battle.advance_ai(owner, &mut Vec::new())?;
        assert_eq!(battle.actors[2].control_ex_state.charge_hold, expected);
    }
    assert_eq!(battle.activity(ActorId(2)), crate::Activity::Approaching);
    battle.advance_ai(owner, &mut Vec::new())?;
    assert_eq!(battle.actors[2].control_ex_state.charge_hold, 3);
    Ok(())
}

#[test]
fn hold_position_keeps_current_spot_and_suppresses_autonomous_healing() -> Result<()> {
    let mut prepared = prepared_with_spell(spell_capabilities(true))?;
    prepared.resources.actor_setup[2]
        .companion
        .as_mut()
        .unwrap()
        .defaults = [1, 7, 3];
    prepared.actors[0].hp = 1;
    let mut battle = prepared.finish()?;
    let owner = ActorId(2);
    // Formation and current position differ after an interrupted movement.
    battle.actors[2].position[0] = 100.;
    let position = battle.actors[2].position;
    let random = battle.random.state();
    battle.advance_ai(owner, &mut Vec::new())?;
    assert_eq!(battle.activity(owner), crate::Activity::Idle);
    assert_eq!(battle.actors[2].position, position);
    assert_eq!(battle.actors[2].movement.forward, 0.);
    assert_eq!(battle.random.state(), random);
    assert!(battle.sequences().next().is_none());
    // A player command remains available even with the autonomous technique disabled.
    battle.set_technique_enabled(owner, crate::ActionKey(7), false)?;
    assert!(battle.queue_technique_target(owner, crate::ActionKey(7), ActorId(0))?);
    battle.runtime[2].idle_timer = 0;
    battle.advance_ai(owner, &mut Vec::new())?;
    assert_eq!(battle.runtime[2].support_target, ActorId(0));
    assert!(matches!(
        battle.activity(owner),
        crate::Activity::Casting { .. }
    ));
    Ok(())
}

#[test]
fn hold_position_stops_an_uncommitted_approach_and_preserves_a_queued_order() -> Result<()> {
    for explicit in [false, true] {
        let mut prepared = prepared_with_spell(spell_capabilities(false))?;
        prepared.resources.actor_setup[2].techniques[0].player_range = [0., 50.];
        let mut battle = prepared.finish()?;
        let owner = ActorId(2);
        if explicit {
            assert!(battle.queue_technique_target(owner, crate::ActionKey(7), ActorId(3))?);
        }
        battle.advance_ai(owner, &mut Vec::new())?;
        assert!(battle.runtime[2].task().approach().is_some());
        battle.refresh_strategy(&rows(&battle, [1, 5, 3]))?;
        battle.update_approach(owner, &mut Vec::new())?;
        assert_eq!(battle.runtime[2].task().approach().is_some(), explicit);
        assert_eq!(
            battle.pending_technique(owner),
            explicit.then_some(crate::ActionKey(7))
        );
    }
    Ok(())
}

#[test]
fn imported_positioning_controls_idle_boundaries_and_timeout() -> Result<()> {
    let mut prepared = prepared()?;
    let policy = prepared.resources.actor_setup[2]
        .companion
        .as_mut()
        .unwrap();
    policy.defaults = [1, 5, 3];
    policy.positioning.formation_radius = 600.;
    policy.positioning.idle_arrival_distance = 10.;
    policy.positioning.approach_timeout_ticks = 2;
    let mut battle = prepared.finish()?;
    let owner = ActorId(2);
    // Request a formation point outside the editable limit, with no obstacle there.
    battle.actors[0].movement.steering.home[0] += 1000.;
    assert_eq!(battle.formation_home(owner)?[0], 600.);
    battle.actors[2].position = [591., 0., 0.];
    assert!(!battle.return_position(owner)?);
    battle.actors[2].position[0] = 589.;
    assert!(battle.return_position(owner)?);

    battle.actors[2].position = [0.; 3];
    battle.refresh_strategy(&rows(&battle, [1, 5, 2]))?;
    battle.advance_companion_ai(owner, &mut Vec::new())?;
    for _ in 0..2 {
        assert!(battle.update_approach(owner, &mut Vec::new())?);
    }
    assert!(!battle.update_approach(owner, &mut Vec::new())?);
    assert!(battle.runtime[2].task().approach().is_none());
    Ok(())
}

fn prepared_combo_arte(weapon_reach: bool) -> Result<PreparedBattle> {
    let mut prepared = prepared()?;
    let action = prepared.resources.actions.insert(crate::ActionDefinition {
        normal: None,
        tp_cost: 5,
        execution: crate::ActionExecution::Attack(crate::tests::attack(10)),
    });
    prepared.resources.actor_setup[2].techniques = vec![technique(
        action,
        10,
        crate::TechniqueCapabilities {
            offensive: true,
            uses_weapon_reach: weapon_reach,
            ..Default::default()
        },
    )];
    Ok(prepared)
}

#[test]
fn opening_melee_rejection_happens_after_sampling_and_the_reserve_roll() -> Result<()> {
    let mut prepared = prepared_combo_arte(true)?;
    let mut ranged = prepared.resources.actor_setup[2].techniques[0];
    ranged.action = prepared
        .resources
        .actions
        .insert((*prepared.resources.actions.entries[ranged.action.0]).clone());
    ranged.catalogue = TechniqueId(1);
    ranged.capabilities.uses_weapon_reach = false;
    prepared.resources.actor_setup[2].techniques.push(ranged);
    prepared.actors[2].equipment.max_tp = 100;
    prepared.actors[2].tp = 20;
    let battle = prepared.finish()?;
    let mut random;
    let strategy = battle.prepared.actor_setup[2]
        .companion
        .as_ref()
        .unwrap()
        .resolve([1, 6, 2])?;
    let mut branches = std::collections::BTreeSet::new();
    for seed in 0..128 {
        random = crate::Random::new(seed);
        let mut expected = random;
        let ranged_sampled = expected.next_u16() % 2 == 1;
        let reserve_admitted = expected.next_u16() % 100 < 25;
        branches.insert((ranged_sampled, reserve_admitted));
        let choice = selector(&battle, ActorId(2), &mut random, Some(strategy))
            .offensive_opening_choice(ActorId(3));
        assert_eq!(
            choice.map(|choice| choice.action),
            (ranged_sampled && reserve_admitted).then_some(ranged.action),
            "seed {seed}"
        );
        assert_eq!(random.state(), expected.state(), "seed {seed}: draw order");
    }
    assert_eq!(branches.len(), 4);
    Ok(())
}

#[test]
fn grounded_normal_opening_preserves_rising_until_an_approach_reselects_it() -> Result<()> {
    for expected in [
        NormalAttack::Neutral,
        NormalAttack::Thrust,
        NormalAttack::Low,
    ] {
        let mut battle = prepared()?.finish()?;
        let mut random;
        let owner = ActorId(2);
        let target = ActorId(3);
        let seed = (0..100)
            .find(|&seed| crate::Random::new(seed).next_u16() % 4 == 1)
            .unwrap();
        random = crate::Random::new(seed);
        let selected = selector(&battle, owner, &mut random, None).normal_choice(target);
        assert_eq!(
            selected.action,
            battle.prepared.actor_setup[2]
                .control
                .as_ref()
                .unwrap()
                .normals[NormalAttack::Rising as usize]
                .action
        );
        battle.request_approach(
            crate::ActionRequest {
                actor: owner,
                target: target,
                action: selected.action,
            },
            target,
            crate::ApproachParameters {
                minimum: battle.selection_range(owner, selected.action)[0],
                maximum: battle.selection_range(owner, selected.action)[1],
                motion: None,
                motion_rate: 0.5,
                speed: 6.,
                turn_ticks: 8,
            },
        )?;
        assert_eq!(battle.approach_normal(owner), Some(NormalAttack::Rising));
        let next_seed = (0..100)
            .find(|&seed| crate::Random::new(seed).next_u16() % 4 == expected as u16)
            .unwrap();
        battle.random = crate::Random::new(next_seed);
        battle.advance_ai_approach(owner)?;
        assert_eq!(battle.approach_normal(owner), Some(expected));
    }
    Ok(())
}

#[test]
fn sheenas_summon_filter_applies_to_autonomous_choices_but_not_explicit_orders() -> Result<()> {
    let mut prepared = prepared_combo_arte(true)?;
    prepared.actors[2].overlimit = crate::OverLimit::active(200)?;
    let companion = prepared.resources.actor_setup[2]
        .companion
        .as_mut()
        .unwrap();
    companion.character = crate::Character::Sheena;
    companion.initial_policy = [1, 1, 6];
    let mut battle = prepared.finish()?;
    let mut random = battle.random;
    let owner = ActorId(2);
    let action = battle.prepared.actor_setup[2].techniques[0].action;
    assert!(
        selector(&battle, owner, &mut random, None)
            .techniques()
            .next()
            .is_none()
    );
    assert!(battle.queue_technique_target(owner, action, ActorId(3))?);
    assert!(battle.request_queued_technique(owner, &mut Vec::new())?);
    assert!(battle.runtime[2].task().approach().is_some());
    battle.prepared.actor_setup[2].techniques[0]
        .capabilities
        .summon = true;
    assert!(
        selector(&battle, owner, &mut random, None)
            .techniques()
            .next()
            .is_some()
    );
    Ok(())
}

#[test]
fn rejected_automatic_combo_checks_wait_for_the_authored_retry() -> Result<()> {
    let mut prepared = prepared_combo_arte(true)?;
    let normal = crate::ActionKey(0);
    let attack =
        crate::tests::attack_mut(Arc::make_mut(&mut prepared.resources.actions.entries[0]));
    attack.chain_at = Some(2);
    attack.chain_retry_at = Some(6);
    let owner = ActorId(2);
    prepared.actors[2].equipment.stats.level = 30;
    prepared.actors[2].equipment.max_tp = 100;
    prepared.actors[2].tp = 20;
    prepared.resources.actor_setup[2]
        .companion
        .as_mut()
        .unwrap()
        .initial_policy = [1, 6, 1];
    let mut battle = prepared.finish()?;
    let mut cues = Vec::new();
    battle.start_admitted_action(
        crate::ActionRequest {
            actor: owner,
            target: ActorId(3),
            action: normal,
        },
        &mut cues,
    )?;
    let id = cues
        .iter()
        .find_map(|cue| {
            if let crate::Cue::Started { action, .. } = cue {
                Some(*action)
            } else {
                None
            }
        })
        .unwrap();
    let mut sequence = battle.take_sequence(&id).unwrap();
    battle.update = sequence.started_at + 1;
    battle.runtime[2].combo.normal_links = 2;
    battle.confirm_actor_contact(owner);
    let seed = (0..100)
        .find(|&seed| {
            let mut random = crate::Random::new(seed);
            random.next_u16() % 100 >= 25 && random.next_u16() % 100 >= 25
        })
        .unwrap();
    battle.random = crate::Random::new(seed);
    let mut expected = battle.random;
    expected.next_u16();
    for age in 2..6 {
        sequence.age = age;
        assert!(!battle.chain_normal(id, &sequence, &mut Vec::new())?);
        assert_eq!(
            battle.random.state(),
            expected.state(),
            "unexpected retry at age {age}"
        );
    }
    sequence.age = 6;
    expected.next_u16();
    assert!(!battle.chain_normal(id, &sequence, &mut Vec::new())?);
    assert_eq!(battle.random.state(), expected.state());
    sequence.age = 7;
    assert!(!battle.chain_normal(id, &sequence, &mut Vec::new())?);
    assert_eq!(battle.random.state(), expected.state());
    Ok(())
}

#[test]
fn automatic_normal_requests_use_the_authored_fallback_after_sampling() -> Result<()> {
    let mut prepared = prepared()?;
    let control = Arc::make_mut(prepared.resources.actor_setup[2].control.as_mut().unwrap());
    control.normals[NormalAttack::Neutral as usize].continuation = crate::NormalContinuation {
        directions: 1 << NormalAttack::Thrust as u8,
        fallback: Some(NormalAttack::Low),
    };
    let mut battle = prepared.finish()?;
    let owner = ActorId(2);
    let mut seen = std::collections::BTreeSet::new();
    for seed in 0..30 {
        battle.random = crate::Random::new(seed);
        assert!(battle.advance_ai_combo(
            owner,
            Some(crate::control::NormalState::Attack(NormalAttack::Neutral))
        )?);
        let Some(crate::action_selection::BufferedAction::Normal(normal)) =
            battle.runtime[2].combo.buffered.take()
        else {
            panic!("normal continuation missing")
        };
        seen.insert(normal);
    }
    assert_eq!(
        seen,
        [NormalAttack::Thrust, NormalAttack::Low]
            .into_iter()
            .collect()
    );
    Ok(())
}

#[test]
fn unchanged_approach_does_not_query_geometry_or_consume_randomness() -> Result<()> {
    let prepared = prepared()?;
    for character in [crate::Character::Lloyd, crate::Character::Colette] {
        let mut policy = prepared.resources.actor_setup[2]
            .companion
            .as_ref()
            .unwrap()
            .resolve([1, 5, 2])?;
        policy.character = character;
        let mut random = crate::Random::new(7);
        let before = random.state();
        assert_eq!(
            selection::approach(
                &prepared.actors[2],
                &prepared.actors[3],
                policy,
                NormalAttack::Neutral,
                &mut random,
                || panic!("unchanged approach queried geometry")
            ),
            None
        );
        assert_eq!(random.state(), before);
    }
    Ok(())
}
