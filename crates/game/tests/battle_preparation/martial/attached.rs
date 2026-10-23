use super::*;
use resonance_battle::{ContactSource, ControlInput, GuardResult, conditions::Condition};

#[test]
#[ignore = "requires current martial and encounter publications; CPU only"]
fn prepared_melee_techniques_make_contact_and_complete() -> Result<()> {
    for (name, character, catalogue) in [
        ("Pyre Seal", 5, 125),
        ("Power Seal", 5, 126),
        ("Power Seal Absolute", 5, 128),
        ("Infliction", 7, 163),
        ("Beast", 7, 172),
        ("Crescent Moon", 8, 176),
        ("Spin Kick", 8, 177),
    ] {
        // Saved OverLimit lets long windups finish against the live enemy AI.
        let (mut battle, owner) = encounter(character, &[catalogue], true)?;
        assert!(battle.actors()[owner.index()].overlimit.is_active());
        battle.set_control_mode(owner, resonance_battle::Control::SemiAuto)?;
        let action = battle
            .prepared_technique(owner, resonance_battle::TechniqueId(catalogue))
            .unwrap()
            .action;
        let mut handle = None;
        let mut contact = false;
        let mut completed = false;
        let mut victim = None;
        let mut interrupted = false;
        for tick in 0..900 {
            let mut control = ControlInput::neutral(owner);
            control.technique.pressed = tick == 0;
            control.technique.held = tick == 0;
            let frame = battle.step(BattleInput::from(control))?;
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
                        if !matches!(result.guard, GuardResult::Blocked { .. }) {
                            victim = Some(actor);
                        }
                    }
                    Cue::Completed { action } if Some(action) == handle => completed = true,
                    Cue::Interrupted { action } if Some(action) == handle => interrupted = true,
                    _ => {}
                }
            }
            if completed {
                break;
            }
        }
        assert!(
            completed && contact,
            "{name}: started={handle:?}, contact={contact}, completed={completed}, interrupted={interrupted}, activity={:?}, phase={:?}",
            battle.activity(owner),
            battle.phase(),
        );
        if catalogue == 128 {
            let victim = victim.context("absolute seal did not land an unguarded hit")?;
            assert!(
                battle.actors()[victim.index()]
                    .conditions
                    .effective()
                    .contains(Condition::DefenseDown)
            );
        }
        assert_eq!(
            battle.technique_uses(owner, resonance_battle::TechniqueId(catalogue)),
            Some(1)
        );
        assert!(!battle.is_diagnostic());
    }
    Ok(())
}
