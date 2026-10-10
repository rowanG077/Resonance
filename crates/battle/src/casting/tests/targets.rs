use super::*;

fn released(frame: &crate::BattleFrame, actor: ActorId) -> Option<ActionId> {
    frame.cues.iter().find_map(|cue| match cue {
        Cue::Released {
            action,
            actor: owner,
            ..
        } if *owner == actor => Some(*action),
        _ => None,
    })
}

#[test]
fn ordinary_and_stored_casts_use_current_opponent_without_repaying() {
    for stored in [false, true] {
        let mut battle = spell_charge::charged_prepared(Control::Manual, 0)
            .finish()
            .unwrap();
        let initial_tp = battle.actors[0].tp;
        let cost = battle.prepared.actions.entries[0].tp_cost;
        if stored {
            spell_charge::store(&mut battle);
        } else {
            let mut input = request();
            // The admission target deliberately differs from the current opponent.
            input.actions[0].target = ActorId(2);
            battle.step(input).unwrap();
        }
        battle.runtime[0].target = ActorId(1);
        battle.runtime[0].support_target = ActorId(2);
        let mut frame = if stored {
            spell_charge::enter_recovery(&mut battle)
        } else {
            battle.step(BattleInput::default()).unwrap()
        };
        let mut resident = None;
        for _ in 0..80 {
            if let Some(action) = released(&frame, ActorId(0)) {
                resident = Some(action);
                break;
            }
            frame = battle.step(BattleInput::default()).unwrap();
        }
        let resident = resident.expect("cast should release once");
        assert_eq!(battle.actors[0].tp, initial_tp - cost);
        assert_eq!(battle.technique_uses(ActorId(0), 66), Some(50));
        assert_eq!(
            battle.volleys[&resident].target,
            ActorId(1),
            "stored={stored}"
        );
        assert_eq!(battle.actors[0].stored_spell, None);
        for _ in 0..8 {
            let frame = battle.step(BattleInput::default()).unwrap();
            assert!(released(&frame, ActorId(0)).is_none());
            assert_eq!(battle.actors[0].tp, initial_tp - cost);
            assert_eq!(battle.technique_uses(ActorId(0), 66), Some(50));
        }
    }
}

#[test]
fn special_guard_can_cancel_the_retargeted_partys_unfinished_cast() {
    {
        let mut prepared = spell_charge::charged_prepared(Control::Manual, 0);
        casting_definition(&mut prepared).duration = 30;
        casting_definition(&mut prepared).threat = Some(crate::CastingThreat {
            catalogue: 66,
            offensive: true,
            element: 1,
        });
        prepared.actors[2].side = Side::Enemy;
        prepared.actors[2].control = Control::Enemy;
        let mut enemy_cast = prepared.resources.actions.entries[0].as_ref().clone();

        native_cast(&mut enemy_cast).duration = 1;
        prepared
            .resources
            .actions
            .entries
            .push(Arc::new(enemy_cast));
        prepared.resources.actor_setup[2] = Default::default();
        crate::tests::assign_action(&mut prepared, 2, LEARNED);
        prepared.resources.actor_setup[0].spell_charge = None;
        prepared.targets[0] = ActorId(2);

        let guard = prepared.resources.actions.insert(crate::ActionDefinition {
            normal: None,
            tp_cost: 0,
            execution: crate::ActionExecution::Attack(crate::PreparedAttack {
                events: vec![(0, crate::AttackEvent::SpecialGuard)],
                ..crate::tests::attack(0)
            }),
        });
        let mut members = vec![];
        for actor in [ActorId(0), ActorId(1)] {
            prepared.actors[actor.index()].side = Side::Party;
            prepared.actors[actor.index()].control = Control::Auto;
            prepared.resources.actor_setup[actor.index()].special_guard = Some(guard);
            prepared.resources.actor_setup[actor.index()]
                .techniques
                .push(crate::PreparedTechnique {
                    player_range: [0.; 2],
                    ai_range: [0.; 2],
                    capabilities: crate::TechniqueCapabilities {
                        target: crate::TechniqueTarget::SelfTarget,
                        ..Default::default()
                    },
                    ..crate::tests::technique(guard, 34)
                });
            members.push(crate::tests::counted_techniques(actor, &[34, 66], &[]));
        }
        prepared.actors[1].tp = 47;
        prepared.actors[1].equipment.max_tp = 100;
        prepared.resources.actor_setup[1]
            .techniques
            .push(casting_technique(CAST, 66));
        let mut battle = prepared
            .with_technique_learning_members(members)
            .unwrap()
            .finish()
            .unwrap();
        let tp = battle.actors[2].tp;
        let cost = battle.prepared.actions.entries[0].tp_cost;
        battle
            .step(BattleInput {
                actions: vec![
                    ActionRequest {
                        actor: ActorId(2),
                        action: LEARNED,
                        target: ActorId(0),
                    },
                    ActionRequest {
                        actor: ActorId(1),
                        action: CAST,
                        target: ActorId(2),
                    },
                ],
                ..Default::default()
            })
            .unwrap();
        battle.runtime[2].target = ActorId(1);
        let mut resident = None;
        let mut threatened = false;
        for _ in 0..80 {
            let frame = battle.step(BattleInput::default()).unwrap();
            if let Some(action) = released(&frame, ActorId(2)) {
                assert_eq!(battle.volleys[&action].target, ActorId(1));
                assert!(resident.replace(action).is_none());
                // Retargeting the caster leaves the released spell's target intact.
                battle.runtime[2].target = ActorId(0);
            }
            assert_eq!(battle.runtime[0].special_guard_pending, None);
            if battle.runtime[1].special_guard_pending == Some(34) {
                threatened = true;
                break;
            }
        }
        assert!(
            resident.is_some() && threatened,
            "resident={resident:?}, threatened={threatened}"
        );
        assert_eq!(battle.actors[2].tp, tp - cost);
        assert!(
            battle
                .casting_remaining(ActorId(1))
                .is_some_and(|remaining| remaining > 0)
        );
        battle.step(BattleInput::default()).unwrap();
        assert_eq!(battle.casting_remaining(ActorId(1)), None);
        assert_eq!(battle.actors[1].tp, 47);
        assert_eq!(battle.activity(ActorId(1)), crate::Activity::Idle);
    }
}
