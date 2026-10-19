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
                    normal_attack_opening: false,
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
fn offensive_tp_policies_distinguish_reserves_from_moderation() -> Result<()> {
    let mut prepared = prepared()?;
    let action = crate::ActionKey(7);
    Arc::make_mut(&mut prepared.resources.actions.entries[7]).tp_cost = 7;
    prepared.resources.actor_setup[2].techniques = vec![technique(
        action,
        66,
        crate::TechniqueCapabilities {
            spell: true,
            offensive: true,
            target: crate::TechniqueTarget::Enemy,
            ..Default::default()
        },
    )];
    let companion = prepared.resources.actor_setup[2]
        .companion
        .as_mut()
        .unwrap();
    for (limit, tp) in companion
        .limits
        .iter_mut()
        .zip([0, 0, 25, 50, 75, 100, 30, 25, 50])
    {
        limit.tp = tp;
    }
    let mut battle = prepared.finish()?;
    let owner = ActorId(2);
    let target = ActorId(3);
    battle.actors[2].equipment.max_tp = 100;

    // After choosing the spell, these seeds give chance rolls 24 and 25.
    for (skill, tp, outcomes) in [
        (1, 7, [true, true]),
        (2, 25, [false, false]),
        (2, 26, [true, true]),
        (3, 50, [false, false]),
        (3, 51, [true, true]),
        (4, 75, [false, false]),
        (4, 76, [true, true]),
        (5, 100, [false, false]),
        (6, 30, [true, false]),
        (6, 31, [true, true]),
        (7, 25, [true, false]),
        (7, 26, [true, true]),
        (8, 50, [true, false]),
        (8, 51, [true, true]),
    ] {
        battle.actors[2].tp = tp;
        for (seed, accepted) in [23, 60].into_iter().zip(outcomes) {
            battle.random = crate::Random::new(seed);
            assert_eq!(
                battle
                    .offensive_choice(owner, target, [4, skill, 5])?
                    .is_some(),
                accepted,
                "skill {skill}, TP {tp}, seed {seed}"
            );
        }
    }

    // Skills/Magic positioning still overrides reserves, but not Do Not Use.
    battle.actors[2].tp = 18;
    for skill in 1..=8 {
        battle.random = crate::Random::new(60);
        assert_eq!(
            battle
                .offensive_choice(owner, target, [4, skill, 6])?
                .is_some(),
            skill != 5
        );
    }

    // A low-TP caster can spend remaining TP, while cost and enable masks still apply.
    battle.actors[2].equipment.max_tp = 74;
    battle.random = crate::Random::new(23);
    assert_eq!(
        battle
            .offensive_choice(owner, target, [4, 6, 5])?
            .unwrap()
            .action,
        action
    );
    battle.actors[2].tp = 6;
    battle.random = crate::Random::new(23);
    assert!(battle.offensive_choice(owner, target, [4, 6, 5])?.is_none());
    battle.actors[2].tp = 18;
    battle.set_technique_enabled(owner, action, false)?;
    battle.random = crate::Random::new(23);
    assert!(battle.offensive_choice(owner, target, [4, 6, 5])?.is_none());
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
fn healing_uses_group_spells_for_multiple_wounded_allies() -> Result<()> {
    use crate::{RecoveryTargets, SpellBenefit, SpellRecovery};
    for targets in [
        RecoveryTargets::LivingAllies,
        RecoveryTargets::Area {
            radius: 500.,
            inclusive: true,
        },
    ] {
        let mut prepared = prepared()?;
        let single = crate::ActionKey(7);
        Arc::make_mut(&mut prepared.resources.actions.entries[single.0]).tp_cost = 8;
        let mut group = crate::tests::cast(30, 28);
        let crate::ActionExecution::Casting(cast) = &mut group.execution else {
            unreachable!()
        };
        Arc::make_mut(&mut Arc::make_mut(cast).release)
            .recovery
            .push(SpellRecovery {
                at: 0,
                duration: 1,
                benefit: SpellBenefit::Hp(40),
                targets,
                effect: None,
                effect_placement: Default::default(),
                flash: None,
            });
        let group = prepared.resources.actions.insert(group);
        prepared.resources.actor_setup[2].techniques = [single, group]
            .into_iter()
            .enumerate()
            .map(|(i, action)| {
                technique(
                    action,
                    i as u16 + 1,
                    crate::TechniqueCapabilities {
                        spell: true,
                        healing: true,
                        target: crate::TechniqueTarget::Ally,
                        ..Default::default()
                    },
                )
            })
            .collect();
        let mut battle = prepared.finish()?;
        let owner = ActorId(2);
        battle.actors[0].hp = 10;
        // Injured enemies do not make a single ally's injury a group-healing case.
        battle.actors[3].hp = 1;
        for (ally_hp, caster_hp, tp, enabled, expected) in [
            (100, 100, 40, true, single),
            (80, 100, 28, true, group),
            (81, 100, 28, true, single),
            (100, 80, 28, true, group),
            (50, 100, 27, true, single),
            (50, 100, 40, false, single),
        ] {
            battle.actors[1].hp = ally_hp;
            battle.actors[2].hp = caster_hp;
            battle.actors[2].tp = tp;
            battle.set_technique_enabled(owner, group, enabled)?;
            let choice = battle.support_choice(owner, 80).unwrap();
            assert_eq!((choice.action, choice.target), (expected, ActorId(0)));
        }
        battle.actors[2].tp = 0;
        assert!(battle.support_choice(owner, 80).is_none());
    }
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

#[test]
fn ranged_companions_use_normals_when_their_artes_cannot_sustain_them() -> Result<()> {
    for (skill, reserve, healing) in [(6, 30, false), (7, 25, true)] {
        for (tp, cost, enabled) in [(0, 7, true), (6, 7, true), (80, 81, true), (100, 7, false)] {
            let mut prepared = prepared()?;
            let owner = ActorId(2);
            let spell = crate::ActionKey(7);
            Arc::make_mut(&mut prepared.resources.actions.entries[7]).tp_cost = cost;
            prepared.actors[2].equipment.max_tp = 100;
            prepared.actors[2].tp = tp;
            let setup = &mut prepared.resources.actor_setup[2];
            let policy = setup.companion.as_mut().unwrap();
            policy.defaults = [1, skill, 5];
            policy.limits[usize::from(skill)].tp = reserve;
            setup.techniques = vec![technique(
                spell,
                66,
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
                },
            )];
            let mut battle = prepared.finish()?;
            battle.set_technique_enabled(owner, spell, enabled)?;
            battle.advance_ai(owner, &mut Vec::new())?;
            assert!(
                battle.approach_normal(owner).is_some(),
                "skill {skill}, TP {tp}, cost {cost}, enabled {enabled}"
            );
        }
    }
    Ok(())
}

#[test]
fn healer_holds_position_only_above_its_tp_reserve_and_still_prioritizes_healing() -> Result<()> {
    for (tp, hurt_ally, expect_normal, expect_heal) in [
        (25, false, true, false),
        (26, false, false, false),
        (25, true, false, true),
        (26, true, false, true),
    ] {
        let mut prepared = prepared()?;
        let owner = ActorId(2);
        let spell = crate::ActionKey(7);
        Arc::make_mut(&mut prepared.resources.actions.entries[7]).tp_cost = 7;
        prepared.actors[2].equipment.max_tp = 100;
        prepared.actors[2].tp = tp;
        if hurt_ally {
            prepared.actors[0].hp = 1;
        }
        let setup = &mut prepared.resources.actor_setup[2];
        let policy = setup.companion.as_mut().unwrap();
        policy.defaults = [1, 7, 5];
        policy.limits[7].tp = 25;
        setup.techniques = vec![technique(
            spell,
            66,
            crate::TechniqueCapabilities {
                spell: true,
                healing: true,
                target: crate::TechniqueTarget::Ally,
                ..Default::default()
            },
        )];
        let mut battle = prepared.finish()?;
        battle.advance_ai(owner, &mut Vec::new())?;
        assert_eq!(battle.approach_normal(owner).is_some(), expect_normal);
        assert_eq!(
            battle.runtime[2].task().approach().is_some(),
            expect_normal || expect_heal
        );
        if expect_heal {
            assert_eq!(battle.runtime[2].support_target, ActorId(0));
        }
    }
    Ok(())
}

#[test]
fn normal_openings_preserve_healing_ranged_orders_and_combo_artes() -> Result<()> {
    for (position, hp_percent, healing, expect_spell) in [
        (1, 100, false, false),
        (2, 100, false, false),
        (3, 100, false, false),
        (2, 30, false, false),
        (2, 29, false, true),
        (5, 100, false, true),
        (6, 100, false, true),
        (2, 100, true, true),
    ] {
        let mut prepared = prepared()?;
        let owner = ActorId(2);
        let setup = &mut prepared.resources.actor_setup[2];
        let policy = setup.companion.as_mut().unwrap();
        policy.normal_attack_opening = true;
        policy.defaults = [1, 1, position];
        let spell = crate::ActionKey(7);
        setup.techniques = vec![technique(
            spell,
            66,
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
            },
        )];
        prepared.actors[2].equipment.max_hp = 100;
        prepared.actors[2].hp = hp_percent;
        if healing {
            prepared.actors[0].hp = 1;
        }
        let mut battle = prepared.finish()?;
        battle.advance_ai(owner, &mut Vec::new())?;
        assert!(battle.runtime[2].task().approach().is_some());
        assert_eq!(battle.approach_normal(owner).is_none(), expect_spell);
        if healing {
            assert_eq!(battle.runtime[2].support_target, ActorId(0));
        }
        // Opening preference does not disable offensive artes for combo selection.
        if !healing && position == 2 {
            assert!(
                battle
                    .offensive_choice(owner, ActorId(3), [1, 1, 2])?
                    .is_some()
            );
        }
    }
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
