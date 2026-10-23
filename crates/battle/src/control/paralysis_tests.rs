use super::*;
use crate::TechniqueId;
use crate::conditions::Condition;

fn paralyzed_player() -> Battle {
    let mut battle = usage_shortcuts_battle();
    battle.actors[0].equipment.luck = 0;
    battle.actors[0]
        .conditions
        .apply_hit(resonance_content::battle_action::HitCondition {
            condition: resonance_content::battle_action::Condition::Paralysis,
            chance: 100,
            value: 0,
        });
    battle
}

fn enter(battle: &mut Battle) -> Result<()> {
    let frame = battle.step(input([0, 0], true))?;
    assert_eq!(frame.actors[0].activity, Activity::Approaching);
    assert!(frame.actions.is_empty());
    let frame = battle.step(BattleInput::default())?;
    assert_eq!(frame.actors[0].activity, Activity::Hurt);
    assert!(frame.actions.is_empty());
    Ok(())
}

fn arm_enemy_hit(battle: &mut PreparedBattle, amount: u16) {
    let contact = Arc::new(crate::MeleeDefinition {
        hit: crate::HitRule {
            power: crate::Power::Fixed(amount),
            guard: crate::GuardRule {
                enabled: false,
                ..Default::default()
            },
            reaction: crate::ReactionRule {
                hitstun: 5,
                ..Default::default()
            },
            ..crate::tests::hit()
        },
        trail: None,
        volume: crate::MeleeVolume::Fixed {
            offset: [0.; 3],
            radius: 200.,
            half_height: 200.,
        },
    });
    let action = battle.resources.actions.insert(crate::ActionDefinition {
        execution: crate::ActionExecution::Attack(crate::PreparedAttack {
            events: vec![(
                0,
                crate::AttackEvent::Contact {
                    definition: contact,
                    duration: 1,
                },
            )],
            ..crate::tests::attack(2)
        }),
        ..crate::tests::action(2)
    });
    crate::tests::assign_action(battle, 1, action);
    battle.actors[0].body.collider = Some(crate::Collider::sphere(1.));
}

#[test]
fn paralysis_blocks_commands_without_payment_and_recovers_without_artwork() -> Result<()> {
    for (guard, technique) in [(false, false), (true, true)] {
        let mut battle = paralyzed_player();
        if guard {
            battle.step(player_buttons(false, false, true, [0; 2]))?;
            assert_eq!(battle.activity(ActorId(0)), Activity::Guarding);
        }
        let tp = battle.actors[0].tp;
        let uses = battle.technique_uses(ActorId(0), TechniqueId(1));
        let frame = battle.step(player_buttons(!technique, technique, guard, [0; 2]))?;
        assert_eq!(frame.actors[0].activity, Activity::Approaching);
        assert!(frame.actions.is_empty());
        assert_eq!(battle.actors[0].tp, tp);
        assert_eq!(battle.technique_uses(ActorId(0), TechniqueId(1)), uses);
        let frame = battle.step(BattleInput::default())?;
        assert_eq!(frame.actors[0].activity, Activity::Hurt);
        assert!(frame.actions.is_empty());
        let position = battle.actors[0].position;
        for _ in 0..120 {
            let frame = battle.step(BattleInput {
                paused: true,
                ..Default::default()
            })?;
            assert_eq!(frame.actors[0].activity, Activity::Hurt);
            assert_eq!(frame.actors[0].position, position);
        }
        for _ in 0..120 {
            let frame = battle.step(BattleInput::default())?;
            assert!(frame.actions.is_empty());
            if battle.actor_command_ready(ActorId(0)) {
                break;
            }
        }
        assert!(battle.actor_command_ready(ActorId(0)));
        assert_eq!(battle.actors[0].tp, tp);
        assert_eq!(battle.technique_uses(ActorId(0), TechniqueId(1)), uses);
        assert!(
            battle.actors[0]
                .conditions
                .effective()
                .contains(Condition::Paralysis)
        );
    }
    Ok(())
}

#[test]
fn paralysis_checks_action_admission_after_approaching_the_target() -> Result<()> {
    let mut battle = paralyzed_player();
    battle.actors[0].control = Control::SemiAuto;
    battle.actors[1].position[0] = 400.;

    let frame = battle.step(input([0, 0], true))?;
    assert_eq!(frame.actors[0].activity, Activity::Approaching);
    assert!(frame.actions.is_empty());
    assert_eq!(battle.target(ActorId(0)), Some(ActorId(1)));

    battle.actors[1].position[0] = 100.;

    let frame = battle.step(BattleInput::default())?;

    assert_eq!(frame.actors[0].activity, Activity::Hurt);
    assert!(frame.actions.is_empty());
    Ok(())
}

#[test]
fn new_contact_or_death_replaces_the_paralysis_task() -> Result<()> {
    for lethal in [false, true] {
        let mut candidate = shortcuts_prepared(Control::Manual);
        arm_enemy_hit(&mut candidate, if lethal { 100 } else { 5 });
        let mut battle = candidate.finish()?;
        battle.actors[0].equipment.luck = 0;
        battle.actors[0]
            .conditions
            .apply_hit(resonance_content::battle_action::HitCondition {
                condition: resonance_content::battle_action::Condition::Paralysis,
                chance: 100,
                value: 0,
            });
        enter(&mut battle)?;
        let frame = battle.step(BattleInput::from(crate::ActionRequest {
            actor: ActorId(1),
            target: ActorId(0),
            action: crate::ActionKey(11),
        }))?;
        assert!(frame.cues.iter().any(|cue| matches!(
            cue,
            Cue::Hit {
                actor: ActorId(0),
                ..
            }
        )));
        if lethal {
            assert_eq!(frame.actors[0].availability, crate::ActorAvailability::Dead);
            assert_eq!(frame.actors[0].activity, Activity::Defeated);
        } else {
            assert!(frame.actors[0].hp < 50);
            for _ in 0..12 {
                battle.step(BattleInput::default())?;
                if battle.actor_command_ready(ActorId(0)) {
                    break;
                }
            }
            assert!(battle.actor_command_ready(ActorId(0)));
        }
    }
    Ok(())
}
