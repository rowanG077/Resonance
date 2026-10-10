use super::*;
use crate::{ActionDefinition, ActionRequest, BattleInput, Control, Cue, PreparedBattle, Side};
use std::{collections::BTreeMap, sync::Arc};

fn prepared(initial: u16) -> PreparedBattle {
    let action = ActionDefinition {
        normal: None,
        execution: crate::ActionExecution::Attack(crate::tests::attack(20)),
        tp_cost: 7,
    };
    let mut actors = vec![
        crate::tests::actor(Side::Party),
        crate::tests::actor(Side::Enemy),
    ];
    actors[0].control = Control::Auto;
    actors[0].movement.target_direction = [1., 0., 0.];
    actors[1].position[0] = 100.;

    for actor in &mut actors {
        actor.body.collider = Some(crate::Collider::sphere(10.));
    }
    PreparedBattle::new(
        (actors)
            .into_iter()
            .zip(vec![
                crate::ActorSetup {
                    techniques: vec![crate::tests::technique(crate::ActionKey(0), 1)],
                    ..Default::default()
                },
                crate::ActorSetup::default(),
            ])
            .collect(),
        (vec![action]).into(),
        1,
    )
    .unwrap()
    .with_technique_learning_members(vec![crate::tests::counted_techniques(
        ActorId(0),
        &[1],
        &[(1, initial)],
    )])
    .unwrap()
}

#[test]
fn learning_during_normal_attacks_keeps_the_selected_action_and_cost() -> Result<()> {
    let mut battle = dormant_martial()?.finish()?;
    // One end-to-end scenario exercises actual learning; individual probability
    // boundaries are covered by the learning policy's explicit-roll tests.
    for _ in 0..64 {
        let tp = battle.actors[0].tp;
        battle.start(
            ActionRequest {
                action: crate::ActionKey(1),
                ..request()
            },
            &mut vec![],
        )?;
        let (&id, sequence) = battle.sequences().next().unwrap();
        assert_eq!(sequence.action, crate::ActionKey(1));
        assert_eq!(battle.actors[0].tp, tp);
        let learned = !battle.technique_acquisitions().is_empty();
        battle.step(BattleInput {
            interrupt: vec![id],
            ..Default::default()
        })?;
        if learned {
            break;
        }
    }
    assert_eq!(battle.technique_acquisitions().len(), 1);
    assert_eq!(battle.technique_acquisitions()[0].catalogue, 1);
    assert_eq!(battle.technique_uses(ActorId(0), 1), Some(49));
    let tp = battle.actors[0].tp;
    battle.start(request(), &mut vec![])?;
    assert_eq!(
        battle.sequences().next().unwrap().1.action,
        crate::ActionKey(0)
    );
    assert_eq!(battle.actors[0].tp, tp - 7);
    assert_eq!(battle.technique_uses(ActorId(0), 1), Some(50));
    Ok(())
}

#[test]
fn learning_capacity_requires_a_real_actor_action_and_publishes_once() -> Result<()> {
    for owners in [&[ActorId(1)][..], &[ActorId(99)], &[ActorId(0), ActorId(0)]] {
        let rows = owners
            .iter()
            .map(|&actor| crate::tests::counted_techniques(actor, &[1], &[]))
            .collect();
        assert!(prepared(0).with_technique_learning_members(rows).is_err());
    }
    let mut ownerless = dormant_martial()?;
    ownerless.technique_learning_members.clear();
    let mut ownerless = ownerless.finish()?;
    assert!(
        ownerless
            .record_technique_acquisition(ActorId(0), 1)
            .is_err()
    );
    assert!(ownerless.technique_acquisitions().is_empty());
    let shortcuts = ownerless.shortcuts(ActorId(0)).copied();
    assert!(
        ownerless
            .validate_forget_technique(ActorId(0), crate::ActionKey(0))
            .is_err()
    );
    assert!(
        ownerless
            .forget_technique(ActorId(0), crate::ActionKey(0))
            .is_err()
    );
    assert!(
        ownerless
            .learned_technique(ActorId(0), crate::ActionKey(0))
            .is_some()
    );
    assert_eq!(ownerless.shortcuts(ActorId(0)).copied(), shortcuts);
    assert!(
        ownerless.runtime[0]
            .control
            .as_ref()
            .unwrap()
            .disabled_techniques
            .is_empty()
    );
    let mut candidate = prepared(0);
    candidate.resources.actor_setup[0].techniques[0].catalogue = 2;
    let mut battle = candidate
        .with_technique_learning_members(vec![crate::tests::counted_techniques(
            ActorId(0),
            &[],
            &[(2, 0)],
        )])?
        .finish()?;
    assert_eq!(
        battle.record_technique_acquisition(ActorId(0), 2)?,
        crate::ActionKey(0)
    );
    assert_eq!(
        battle.record_technique_acquisition(ActorId(0), 2)?,
        crate::ActionKey(0)
    );
    assert_eq!(battle.technique_acquisitions().len(), 1);
    assert!(battle.record_technique_acquisition(ActorId(0), 35).is_err());
    Ok(())
}

#[test]
fn prepared_technique_identity_rejects_duplicate_catalogue_or_action() {
    for (action, catalogue) in [(crate::ActionKey(0), 2), (crate::ActionKey(1), 1)] {
        let mut candidate = prepared(0);
        let extra = (*candidate.resources.actions.entries[0]).clone();

        candidate.resources.actions.entries.push(Arc::new(extra));
        candidate.resources.actor_setup[0]
            .techniques
            .push(crate::tests::technique(action, catalogue));
        assert!(candidate.finish().is_err());
    }
}

#[test]
fn forgetting_and_relearning_keep_the_accumulated_count() -> Result<()> {
    let mut battle = dormant_martial()?.finish()?;
    assert_eq!(battle.technique_uses(ActorId(0), 1), Some(49));
    assert_eq!(
        battle.record_technique_acquisition(ActorId(0), 1)?,
        crate::ActionKey(0)
    );
    assert_eq!(
        battle.record_technique_use(ActorId(0), crate::ActionKey(0), false),
        Some(1)
    );
    assert_eq!(battle.technique_uses(ActorId(0), 1), Some(50));
    battle.forget_technique(ActorId(0), crate::ActionKey(0))?;
    assert_eq!(battle.technique_is_current(ActorId(0), 1), Some(false));
    assert_eq!(battle.shortcuts(ActorId(0)), Some(&[0; 4]));
    assert_eq!(battle.technique_uses(ActorId(0), 1), Some(50));
    assert_eq!(
        battle.record_technique_acquisition(ActorId(0), 1)?,
        crate::ActionKey(0)
    );
    assert_eq!(battle.technique_uses(ActorId(0), 1), Some(50));
    assert_eq!(battle.shortcuts(ActorId(0)), Some(&[1, 0, 0, 0]));
    assert_eq!(
        battle.record_technique_use(ActorId(0), crate::ActionKey(0), false),
        Some(1)
    );
    assert_eq!(battle.technique_uses(ActorId(0), 1), Some(51));
    assert_eq!(battle.technique_acquisitions().len(), 2);
    Ok(())
}

#[test]
fn learning_capacity_rejects_missing_action_before_activation() {
    let mut candidate = prepared(0);
    candidate.resources.actor_setup[0].techniques[0].action = crate::ActionKey(999);
    assert!(candidate.finish().is_err());
}

fn parameters() -> crate::ApproachParameters {
    crate::ApproachParameters {
        minimum: 0.,
        maximum: 120.,
        motion: None,
        motion_rate: 0.5,
        speed: 6.,
        turn_ticks: 8,
    }
}

fn request() -> ActionRequest {
    ActionRequest {
        actor: ActorId(0),
        target: ActorId(1),
        action: crate::ActionKey(0),
    }
}

#[test]
fn count_history_preserves_unsupported_and_forgotten_techniques() -> Result<()> {
    let mut battle = prepared(0)
        .with_technique_learning_members(vec![crate::tests::counted_techniques(
            ActorId(0),
            &[1, 2, 254],
            &[(2, 12), (3, 20), (254, 7)],
        )])?
        .finish()?;
    assert_eq!(
        battle.technique_counts(ActorId(0)).unwrap(),
        &BTreeMap::from([(1, 0), (2, 12), (3, 20), (254, 7)])
    );
    assert_eq!(battle.technique_is_current(ActorId(0), 254), Some(true));
    assert_eq!(battle.technique_uses(ActorId(0), 4), None);
    assert_eq!(
        battle.record_technique_use(ActorId(0), crate::ActionKey(0), false),
        Some(1)
    );
    assert_eq!(battle.technique_uses(ActorId(0), 1), Some(1));
    assert_eq!(battle.technique_uses(ActorId(0), 2), Some(12));
    assert_eq!(battle.technique_uses(ActorId(0), 3), Some(20));
    Ok(())
}

#[test]
fn usage_totals_and_proficiency_are_bounded() -> Result<()> {
    for (initial, total, bonus) in [
        (0, 1, 0),
        (49, 50, 1),
        (249, 250, 5),
        (998, 999, 5),
        (999, 999, 5),
    ] {
        let mut battle = prepared(initial).finish().unwrap();
        battle.start(request(), &mut vec![])?;
        assert_eq!(battle.technique_uses(ActorId(0), 1), Some(total));
        assert_eq!(battle.technique_uses(ActorId(0), 71), None);
        assert_eq!(battle.actors[0].proficiency, bonus);
        battle.step(BattleInput::default())?;
        assert_eq!(battle.technique_uses(ActorId(0), 1), Some(total));
    }
    Ok(())
}

#[test]
fn death_during_approach_does_not_count_an_unstarted_action() -> Result<()> {
    let mut battle = prepared(49).finish().unwrap();
    battle.actors[0].heading = -90.;
    assert!(battle.request_approach(ActorId(0), ActorId(1), crate::ActionKey(0), parameters())?);
    assert_eq!(battle.technique_uses(ActorId(0), 1), Some(49));
    battle.actors[0].hp = 0;
    battle.actors[0].availability = crate::ActorAvailability::Dead;
    battle.step(BattleInput::default())?;
    assert!(battle.sequences().next().is_none());
    assert_eq!(battle.technique_uses(ActorId(0), 1), Some(49));
    Ok(())
}

#[test]
fn moving_request_waits_for_arrival_and_cancelled_approach_never_counts() -> Result<()> {
    for cancel in [false, true] {
        let mut battle = prepared(49).finish().unwrap();
        battle.actors[1].position[0] = 400.;

        assert!(battle.request_approach(
            ActorId(0),
            ActorId(1),
            crate::ActionKey(0),
            parameters()
        )?);
        assert_eq!(battle.technique_uses(ActorId(0), 1), Some(49));
        battle.step(BattleInput::default())?;
        if cancel {
            battle.cancel_approach(0)?;
        } else {
            battle.actors[1].position[0] = 100.;
        }
        for _ in 0..16 {
            battle.step(BattleInput::default())?;
            if battle.runtime[0].task().approach().is_none() {
                break;
            }
        }
        assert!(battle.runtime[0].task().approach().is_none());
        assert_eq!(
            battle.technique_uses(ActorId(0), 1),
            Some(if cancel { 49 } else { 50 })
        );
    }
    Ok(())
}

#[test]
fn insufficient_tp_at_arrival_does_not_count_or_start_the_action() -> Result<()> {
    let mut battle = prepared(49).finish().unwrap();
    battle.actors[0].heading = -90.;
    assert!(battle.request_approach(ActorId(0), ActorId(1), crate::ActionKey(0), parameters())?);
    battle.actors[0].tp = 0;
    for _ in 0..8 {
        let frame = battle.step(BattleInput::default())?;
        assert!(!frame.cues.iter().any(|cue| matches!(
            cue,
            Cue::Started {
                actor: ActorId(0),
                ..
            }
        )));
    }
    assert_eq!(battle.technique_uses(ActorId(0), 1), Some(49));
    assert_eq!(battle.actors[0].tp, 0);
    Ok(())
}

#[test]
fn rejected_direct_request_does_not_count_and_two_admissions_count_twice() -> Result<()> {
    let mut battle = prepared(10).finish().unwrap();
    battle.actors[0].tp = 6;
    battle.start(request(), &mut vec![])?;
    assert_eq!(battle.technique_uses(ActorId(0), 1), Some(10));
    battle.actors[0].tp = 40;
    battle.start(request(), &mut vec![])?;
    let first = *battle.sequences().map(|(id, _)| id).next().unwrap();
    battle.step(BattleInput {
        interrupt: vec![first],
        ..Default::default()
    })?;
    battle.start(request(), &mut vec![])?;
    assert_eq!(battle.technique_uses(ActorId(0), 1), Some(12));
    Ok(())
}

#[test]
fn battle_end_rejects_new_actions_without_changing_usage() -> Result<()> {
    let mut battle = prepared(49).finish().unwrap();
    battle.actors[1].availability = crate::ActorAvailability::Dead;
    battle.actors[1].hp = 0;
    assert_eq!(
        battle.recognize_result(),
        Some(crate::BattleResult::Victory)
    );
    let mut cues = vec![];
    battle.start(request(), &mut cues)?;
    assert!(cues.iter().any(|cue| matches!(
        cue,
        Cue::Rejected {
            reason: crate::Rejection::BattleEnding,
            ..
        }
    )));
    assert_eq!(battle.technique_uses(ActorId(0), 1), Some(49));
    Ok(())
}

fn dormant_martial() -> Result<PreparedBattle> {
    let mut prepared = prepared(49);
    let normals = crate::tests::normal_controls(
        &mut prepared.resources.actions,
        crate::tests::attack(20),
        [0., 120.],
    );
    prepared.actors[0].control = Control::Manual;
    prepared.actors[0].movement.direction = [0., 0., 1.];
    let mut table = resonance_content::arte::Catalogue {
        definitions: vec![Default::default(); 2],
        learning: vec![vec![1]],
    };
    table.definitions[1].required_level = 1;
    let member = crate::learning::LearningCatalogue::new(Arc::new(table)).prepare_member(
        crate::learning::LearningEntry {
            character: 1,
            level: 1,
            balance: 0,
            story_unlocked: true,
            current: Default::default(),
            counts: [(1, 49)].into_iter().collect(),
        },
    )?;
    prepared.resources.actor_setup[0].control = Some(Arc::new(crate::ControlDefinition {
        normals,
        shortcuts: [0; 4],
        walk_speed: 5.,
        run_speed: 10.,
        turn_ticks: 1,
        motions: None,
    }));
    prepared.resources.actor_setup[0].techniques =
        vec![crate::tests::technique(crate::ActionKey(0), 1)];
    prepared.with_technique_learning_members(vec![crate::learning::TechniqueLearningMember {
        actor: ActorId(0),
        member,
    }])
}

#[test]
fn acquired_action_keeps_its_proficiency_after_forgetting() -> Result<()> {
    let mut battle = dormant_martial()?.finish()?;
    battle.record_technique_acquisition(ActorId(0), 1)?;
    battle.actors[0].hit_stop = 4;
    battle.start(request(), &mut vec![])?;
    let second = *battle.sequences().map(|(id, _)| id).next().unwrap();
    assert_eq!(battle.actors[0].tp, 33);
    assert_eq!(battle.actors[0].proficiency, 1);
    battle.forget_technique(ActorId(0), crate::ActionKey(0))?;
    assert_eq!(battle.technique_is_current(ActorId(0), 1), Some(false));
    for _ in 0..4 {
        battle.step(BattleInput::default())?;
        assert_eq!(battle.action_age(second), Some(0));
        assert_eq!(battle.actors[0].proficiency, 1);
    }
    for _ in 0..30 {
        battle.step(BattleInput::default())?;
    }
    assert!(battle.sequence(&second).is_none());
    assert_eq!(battle.actors[0].tp, 33);
    assert_eq!(battle.technique_uses(ActorId(0), 1), Some(50));
    Ok(())
}

fn normal_learning_battle(mode: Control) -> Result<Battle> {
    let mut prepared = dormant_martial()?;
    prepared.actors[0].control = mode;
    for _ in 0..2 {
        let action = (*prepared.resources.actions.entries[0]).clone();

        prepared.resources.actions.entries.push(Arc::new(action));
    }
    prepared.resources.actor_setup[0].techniques = [
        (crate::ActionKey(0), 1),
        (crate::ActionKey(8), 2),
        (crate::ActionKey(9), 3),
    ]
    .into_iter()
    .map(|(action, catalogue)| crate::tests::technique(action, catalogue))
    .collect();
    prepared = prepared.with_technique_learning_members(vec![crate::tests::counted_techniques(
        ActorId(0),
        &[1, 2, 3],
        &[],
    )])?;
    let mut battle = prepared.finish().unwrap();
    // Forgetting a technique disables it until it is learned again.
    battle.forget_technique(ActorId(0), crate::ActionKey(0))?;
    for (slot, action) in [(0, crate::ActionKey(8)), (2, crate::ActionKey(9))] {
        let binding = battle.learned_technique(ActorId(0), action).unwrap();
        battle
            .prepare_shortcut(ActorId(0), slot, Some(binding.action))?
            .commit();
    }
    let control = battle.runtime[0].control.as_mut().unwrap();
    control.disabled_techniques.insert(crate::ActionKey(9));
    control.assist_shortcuts[1] = Some((ActorId(0), crate::ActionKey(9)));
    Ok(battle)
}

#[test]
fn learning_updates_vacant_shortcuts_once_without_changing_other_assignments() -> Result<()> {
    for full in [false, true] {
        let mut battle = normal_learning_battle(Control::Manual)?;
        if full {
            for (slot, action) in [(1, crate::ActionKey(9)), (3, crate::ActionKey(8))] {
                battle
                    .prepare_shortcut(ActorId(0), slot, Some(action))?
                    .commit();
            }
        }
        let mut expected = *battle.shortcuts(ActorId(0)).unwrap();
        if !full {
            expected[1] = 1;
        }
        battle.record_technique_acquisition(ActorId(0), 1)?;
        battle.record_technique_acquisition(ActorId(0), 1)?;
        assert_eq!(battle.shortcuts(ActorId(0)), Some(&expected));
        assert!(battle.technique_enabled(ActorId(0), crate::ActionKey(0)));
        assert!(!battle.technique_enabled(ActorId(0), crate::ActionKey(9)));
        assert_eq!(battle.technique_uses(ActorId(0), 1), Some(0));
        assert_eq!(battle.technique_acquisitions().len(), 1);
    }
    Ok(())
}

#[test]
fn unprepared_acquisition_preserves_membership_shortcuts_and_ledger() -> Result<()> {
    let mut battle = dormant_martial()?.finish()?;
    let shortcuts = *battle.shortcuts(ActorId(0)).unwrap();
    let member = battle.learning_members[0].member.current().clone();
    let counts = battle.technique_counts(ActorId(0)).cloned();
    let ledger = battle.ledger().clone();
    let error = battle
        .record_technique_acquisition(ActorId(0), 35)
        .unwrap_err();
    assert!(error.to_string().contains("unprepared learning action"));
    assert_eq!(battle.shortcuts(ActorId(0)), Some(&shortcuts));
    assert_eq!(battle.learning_members[0].member.current(), &member);
    assert_eq!(battle.technique_counts(ActorId(0)), counts.as_ref());
    assert_eq!(battle.ledger(), &ledger);
    assert!(battle.sequences().next().is_none());
    Ok(())
}
