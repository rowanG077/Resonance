use super::*;
use crate::{Control, DecisionDefinition, Side};
use std::sync::Arc;

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
                    shortcuts: [0; 4],
                    normals,
                })),
                companion: Some(CompanionDefinition {
                    initial_policy: [0; 3],
                    defaults: [1, 5, 2],
                    limits: std::array::from_fn(|i| PolicyLimits {
                        tp: i as u8 * 10,
                        healing: 90 - i as u8 * 10,
                        support_level: i as i8 - 4,
                    }),
                    level: 30,
                    level_difference: 0,
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
    prepared.resources.actions.insert(crate::tests::cast(30, 0));
    Ok(prepared)
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
    assert_eq!(battle.companion_policy(ActorId(0)).unwrap().choices, [0; 3]);
    battle.refresh_strategy(&rows(&battle, [2, 1, 6]))?;
    assert_eq!(battle.resolved_companion_policy(ActorId(2))?, [2, 1, 6]);
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
fn offensive_policy_filters_cost_enabled_immunity_and_prefers_weakness() -> Result<()> {
    let mut prepared = prepared()?;
    let mut offensive = (*prepared.resources.actions.entries[0]).clone();
    offensive.normal = None;
    offensive.tp_cost = 2;
    prepared.resources.actions.entries.push(Arc::new(offensive));
    Arc::make_mut(&mut prepared.resources.actions.entries[7]).tp_cost = 4;
    prepared.resources.actor_setup[2].techniques = vec![
        technique(
            crate::ActionKey(7),
            66,
            crate::TechniqueCapabilities {
                spell: true,
                offensive: true,
                target: crate::TechniqueTarget::Enemy,
                ..Default::default()
            },
        ),
        technique(
            crate::ActionKey(8),
            1,
            crate::TechniqueCapabilities {
                offensive: true,
                target: crate::TechniqueTarget::Enemy,
                ..Default::default()
            },
        ),
    ];
    prepared.resources.actor_setup[2].techniques[0].element = 1;
    let mut battle = prepared.finish().unwrap();
    let owner = ActorId(2);
    let target = ActorId(3);
    battle.actors[2].equipment.max_tp = 10;
    battle.actors[2].tp = 3;
    assert_eq!(
        battle
            .offensive_choice(owner, target, [1, 1, 6])?
            .unwrap()
            .action,
        crate::ActionKey(8)
    );
    battle.actors[2].tp = 10;
    battle.actors[3].equipment.affinities[1] = crate::Affinity::Weak;
    assert_eq!(
        battle
            .offensive_choice(owner, target, [1, 1, 6])?
            .unwrap()
            .action,
        crate::ActionKey(7)
    );
    battle.runtime[2]
        .control
        .as_mut()
        .unwrap()
        .disabled_techniques
        .insert(crate::ActionKey(7));
    assert_eq!(
        battle
            .offensive_choice(owner, target, [1, 1, 6])?
            .unwrap()
            .action,
        crate::ActionKey(8)
    );
    battle.actors[3].equipment.affinities[0] = crate::Affinity::Immune;
    assert!(battle.offensive_choice(owner, target, [1, 1, 6])?.is_none());
    assert!(battle.offensive_choice(owner, target, [1, 5, 6])?.is_none());
    Ok(())
}

#[test]
fn support_prioritizes_revival_then_lowest_eligible_health() -> Result<()> {
    let mut prepared = prepared()?;
    let revive = prepared.resources.actions.insert(crate::tests::cast(30, 0));
    prepared.resources.actor_setup[2].techniques = vec![
        technique(
            revive,
            67,
            crate::TechniqueCapabilities {
                spell: true,
                revives: true,
                uses_weapon_reach: true,
                target: crate::TechniqueTarget::Ally,
                ..Default::default()
            },
        ),
        technique(
            crate::ActionKey(7),
            66,
            crate::TechniqueCapabilities {
                spell: true,
                healing: true,
                uses_weapon_reach: true,
                target: crate::TechniqueTarget::Ally,
                ..Default::default()
            },
        ),
    ];
    let mut battle = prepared.finish().unwrap();
    battle.actors[0].availability = crate::ActorAvailability::Dead;
    battle.actors[0].hp = 0;
    battle.actors[1].hp = 10;
    let choice = battle.support_choice(ActorId(2), 50).unwrap();
    assert_eq!((choice.action, choice.target), (revive, ActorId(0)));
    battle.runtime[2]
        .control
        .as_mut()
        .unwrap()
        .disabled_techniques
        .insert(revive);
    let choice = battle.support_choice(ActorId(2), 50).unwrap();
    assert_eq!(
        (choice.action, choice.target),
        (crate::ActionKey(7), ActorId(1))
    );
    battle.actors[1].hp = battle.actors[1].equipment.max_hp;
    battle.actors[2].hp = battle.actors[2].equipment.max_hp;
    assert!(battle.support_choice(ActorId(2), 50).is_none());
    Ok(())
}

#[test]
fn idle_native_policy_starts_an_approach_without_a_policy_vm() -> Result<()> {
    let mut battle = prepared()?.finish().unwrap();
    battle.advance_ai(ActorId(2), &mut Vec::new())?;
    assert_eq!(battle.activity(ActorId(2)), crate::Activity::Approaching);
    assert!(battle.runtime[2].task().approach().is_some());
    assert!(battle.sequences().next().is_none());
    Ok(())
}

#[path = "command_tests.rs"]
mod command_tests;

#[test]
fn repertoire_tracks_acquisition_forgetting_and_live_enable_masks() -> Result<()> {
    let mut prepared = prepared()?;
    let owner = ActorId(2);
    prepared.resources.actor_setup[2].techniques = vec![technique(
        crate::ActionKey(7),
        66,
        crate::TechniqueCapabilities {
            spell: true,
            offensive: true,
            target: crate::TechniqueTarget::Enemy,
            ..Default::default()
        },
    )];
    prepared.resources.actor_setup[2].techniques[0].catalogue = 1;
    let mut battle = prepared
        .with_technique_learning_members(vec![crate::tests::counted_techniques(
            owner,
            &[],
            &[(1, 0)],
        )])?
        .finish()?;
    assert!(battle.companion_techniques(owner).next().is_none());
    battle.record_technique_acquisition(owner, 1)?;
    assert_eq!(battle.companion_techniques(owner).count(), 1);
    assert!(
        battle
            .usable_companion_technique(owner, battle.companion_techniques(owner).next().unwrap())
    );
    battle.set_technique_enabled(owner, crate::ActionKey(7), false)?;
    assert!(battle.technique_available(owner, crate::ActionKey(7)));
    assert!(
        !battle
            .usable_companion_technique(owner, battle.companion_techniques(owner).next().unwrap())
    );
    battle.forget_technique(owner, crate::ActionKey(7))?;
    assert!(battle.companion_techniques(owner).next().is_none());
    battle.record_technique_acquisition(owner, 1)?;
    assert!(
        battle
            .usable_companion_technique(owner, battle.companion_techniques(owner).next().unwrap())
    );
    Ok(())
}

#[test]
fn idle_charge_accumulates_once_per_visit_including_action_admission() -> Result<()> {
    let mut battle = prepared()?.finish().unwrap();
    let owner = ActorId(2);
    battle.actors[2].equipment.control_ex.charge = true;
    let slot = usize::from(battle.actors[2].control_slot);
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
