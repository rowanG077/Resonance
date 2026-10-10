use super::*;

#[test]
#[ignore = "requires current Regal and encounter publications; CPU only"]
fn learned_mirage_moves_and_completes() -> Result<()> {
    let (mut battle, owner) = encounter(8, &[])?;
    let action = battle.record_technique_acquisition(owner, 201)?;
    let before = battle.actors()[owner.index()].clone();
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
        .context("Mirage did not start")?
        .0;
    let mut completed = false;
    for _ in 0..120 {
        let frame = battle.step(BattleInput::default())?;
        completed |= frame
            .cues
            .iter()
            .any(|cue| matches!(cue, Cue::Completed { action } if *action == handle));
        if completed {
            break;
        }
    }
    assert!(completed);
    assert_ne!(battle.actors()[owner.index()].position, before.position);
    assert_eq!(battle.actors()[owner.index()].tp, before.tp - 12);
    assert_eq!(battle.technique_uses(owner, 201), Some(1));
    assert!(!battle.is_diagnostic());
    Ok(())
}
