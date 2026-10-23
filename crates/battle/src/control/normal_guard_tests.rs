use super::*;
use crate::ProtectionMode;

#[test]
fn normal_guard_is_admitted_only_for_the_opening_attack() -> Result<()> {
    for (enabled, chained) in [(true, false), (false, false), (true, true)] {
        let mut prepared = prepared(Control::Manual);
        prepared.actors[0].equipment.normal_guard = enabled;
        if let crate::ActionExecution::Attack(attack) =
            &mut Arc::make_mut(&mut prepared.resources.actions.entries[0]).execution
        {
            attack.end_at = attack.chain_at.unwrap();
        }
        let mut battle = prepared.finish()?;
        let control = battle.prepared.actor_setup[0].control.clone().unwrap();
        battle.start_control_normal(ActorId(0), &control, NormalAttack::Neutral, &mut vec![])?;
        let (id, _) = battle.runtime[0].task().action().unwrap();
        let id = *id;
        if chained {
            battle.step(BattleInput::default())?;
            battle.step(input([80, 0], true))?;
            wait_for_action(&mut battle, crate::ActionKey(3))?;
            assert_eq!(
                battle.actors[0].reaction.protection.mode,
                ProtectionMode::None
            );
            continue;
        }
        let protection = battle.actors[0].reaction.protection;
        assert_eq!(protection.mode == ProtectionMode::Armor, enabled);
        if !enabled {
            continue;
        }
        assert!(protection.remaining > 0);
        battle.step(BattleInput {
            paused: true,
            ..Default::default()
        })?;
        assert_eq!(battle.actors[0].reaction.protection, protection);
        battle.step(BattleInput::default())?;
        assert_eq!(
            battle.actors[0].reaction.protection.mode,
            ProtectionMode::Armor
        );
        let protection = battle.actors[0].reaction.protection;
        let mut replacement = battle.actors[0].clone();
        replacement.equipment.normal_guard = false;
        crate::tests::equip(&mut battle, ActorId(0), replacement)?;
        assert_eq!(battle.actors[0].reaction.protection, protection);
        battle.step(BattleInput {
            interrupt: vec![id],
            ..Default::default()
        })?;
        assert_eq!(battle.activity(ActorId(0)), Activity::Idle);
        assert_eq!(battle.action_age(id), None);
    }
    Ok(())
}
