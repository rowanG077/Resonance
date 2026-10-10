use super::*;
use resonance_battle::{Activity, ControlInput};

#[test]
#[ignore = "requires current Regal and encounter publications; CPU only"]
fn eagle_dive_waits_for_landing_then_recovers() -> Result<()> {
    let (mut battle, owner) = encounter(8, &[185])?;
    for _ in 0..60 {
        if battle.actors()[owner.index()].position[1] > 30. {
            break;
        }
        let mut input = ControlInput::neutral(owner);
        input.stick = [0, 127];
        battle.step(BattleInput {
            controllers: vec![input],
            ..Default::default()
        })?;
    }
    assert!(battle.actors()[owner.index()].position[1] > 0.);
    let action = battle.prepared_technique(owner, 185).unwrap().action;
    let tp = battle.actors()[owner.index()].tp;
    let first = battle.step(BattleInput {
        actions: vec![resonance_battle::ActionRequest {
            actor: owner,
            action,
            target: battle.target(owner).context("missing target")?,
        }],
        ..Default::default()
    })?;
    let handle = first
        .actions
        .iter()
        .find(|(_, actor, _)| *actor == owner)
        .context("Eagle Dive did not start")?
        .0;
    let mut landed = false;
    for _ in 0..180 {
        let frame = battle.step(BattleInput::default())?;
        let actor = &battle.actors()[owner.index()];
        if battle.activity(owner) == Activity::Recovering {
            assert!(actor.position[1] <= 0.1);
            landed = true;
        }
        if frame.cues.contains(&Cue::Completed { action: handle }) {
            assert!(landed);
            assert_eq!(actor.tp, tp - 8);
            assert_eq!(battle.technique_uses(owner, 185), Some(1));
            assert!(!battle.is_diagnostic());
            return Ok(());
        }
    }
    bail!("Eagle Dive did not complete")
}
