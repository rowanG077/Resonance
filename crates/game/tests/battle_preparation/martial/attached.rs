use super::*;
use resonance_battle::{ContactSource, ControlInput, GuardResult, conditions::Condition};

#[test]
#[ignore = "requires current martial and encounter publications; CPU only"]
fn prepared_melee_techniques_make_contact_and_complete() -> Result<()> {
    for (name, character, catalogue) in [
        ("Pyre Seal", 5, 125),
        ("Power Seal", 5, 126),
        ("Infliction", 7, 163),
        ("Beast", 7, 172),
        ("Crescent Moon", 8, 176),
        ("Spin Kick", 8, 177),
    ] {
        let (mut battle, owner) = encounter(character, &[catalogue])?;
        battle.set_control_mode(owner, resonance_battle::Control::SemiAuto)?;
        let action = battle.prepared_technique(owner, catalogue).unwrap().action;
        let mut handle = None;
        let mut contact = false;
        let mut completed = false;
        for tick in 0..900 {
            let mut control = ControlInput::neutral(owner);
            control.technique.pressed = tick == 0;
            control.technique.held = tick == 0;
            let frame = battle.step(BattleInput {
                controllers: vec![control],
                ..Default::default()
            })?;
            for cue in frame.cues {
                match cue {
                    Cue::Started {
                        actor,
                        action: started,
                        ..
                    } if actor == owner => {
                        assert_eq!(battle.action_definition(started), Some(action));
                        handle = Some(started);
                    }
                    Cue::Hit {
                        source: ContactSource::Melee { action, .. },
                        actor,
                        result,
                        ..
                    } if Some(action) == handle => {
                        contact = true;
                        if catalogue == 126 {
                            assert_eq!(
                                battle.actors()[actor.index()]
                                    .conditions
                                    .effective()
                                    .contains(Condition::DefenseDown),
                                !matches!(result.guard, GuardResult::Blocked { .. }),
                            );
                        }
                    }
                    Cue::Completed { action } if Some(action) == handle => completed = true,
                    _ => {}
                }
            }
            if completed {
                break;
            }
        }
        assert!(completed && contact, "{name} did not hit and complete");
        assert_eq!(battle.technique_uses(owner, catalogue), Some(1));
        assert!(!battle.is_diagnostic());
    }
    Ok(())
}
