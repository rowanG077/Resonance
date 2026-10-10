//! Heavy uses effective layers; Guard and normal buffering have different gates.
use super::*;
use crate::conditions::{Condition, ConditionSet};
use crate::conditions::{Conditions, Layers};

fn equip_heavy(battle: &mut Battle, enabled: bool) -> Result<()> {
    let mut replacement = battle.actors[0].clone();
    replacement.conditions = Conditions::new(Layers {
        equipment_overlay: if enabled {
            ConditionSet::of(&[Condition::Heavy])
        } else {
            ConditionSet::EMPTY
        },
        ..replacement.conditions.layers()
    });
    crate::tests::equip(battle, ActorId(0), replacement)
}

#[test]
fn heavy_blocks_rising_attack_from_guard_without_committing_selection() -> Result<()> {
    for mode in [Control::Manual, Control::SemiAuto] {
        for heavy in [false, true] {
            let mut battle = battle(mode);
            equip_heavy(&mut battle, heavy)?;
            battle.step(player_buttons(false, false, true, [0; 2]))?;

            let frame = battle.step(player_buttons(true, false, true, [0, 80]))?;
            if heavy {
                assert_eq!(frame.actors[0].activity, Activity::Guarding);
                assert!(frame.actions.is_empty());
            } else {
                assert!(matches!(frame.actors[0].activity, Activity::Action));
                let id = frame.actions[0].0;
                assert_eq!(battle.action_definition(id), Some(crate::ActionKey(1)));
            }
        }
    }
    Ok(())
}

#[test]
fn heavy_leaves_other_normals_and_techniques_available() -> Result<()> {
    for (stick, normal) in [([0, 0], 0), ([0, -80], 2), ([80, 0], 3)] {
        let mut battle = battle(Control::Manual);
        equip_heavy(&mut battle, true)?;
        battle.step(player_buttons(false, false, true, [0; 2]))?;
        battle.step(player_buttons(true, false, true, stick))?;
        let frame = battle.step(BattleInput::default())?;
        assert_eq!(
            battle.action_definition(frame.actions[0].0),
            Some(crate::ActionKey(normal))
        );
    }
    let mut battle = shortcuts_battle(Control::Manual);
    equip_heavy(&mut battle, true)?;
    battle.step(player_buttons(false, false, true, [0; 2]))?;
    battle.step(player_buttons(false, true, true, [0; 2]))?;
    battle.step(BattleInput::default())?;
    assert!(
        battle
            .sequences()
            .map(|(_, sequence)| sequence)
            .any(|sequence| sequence.action == crate::ActionKey(7))
    );
    assert_eq!(battle.actors[0].tp, 36);
    Ok(())
}
