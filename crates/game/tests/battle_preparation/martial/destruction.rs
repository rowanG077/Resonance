use super::*;

#[test]
#[ignore = "requires current Presea, projectile and encounter publications; CPU only"]
fn destruction_releases_three_rocks_and_finishes() -> Result<()> {
    let (mut battle, owner) = encounter(7, &[154])?;
    let action = battle.prepared_technique(owner, 154).unwrap().action;
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
        .context("Destruction did not start")?
        .0;
    let mut emitted = 0;
    let mut ended = false;
    for _ in 0..300 {
        let frame = battle.step(BattleInput::default())?;
        emitted += frame
            .cues
            .iter()
            .filter(|cue| matches!(cue, Cue::ProjectileStarted { action, .. } if *action == handle))
            .count();
        ended |= frame
            .cues
            .iter()
            .any(|cue| matches!(cue, Cue::Completed { action } if *action == handle));
        if ended && frame.projectiles.iter().all(|p| p.owner != owner) {
            break;
        }
    }
    assert!(ended);
    assert_eq!(emitted, 3);
    assert!(
        battle
            .snapshot()
            .projectiles
            .iter()
            .all(|p| p.owner != owner)
    );
    assert_eq!(battle.actors()[owner.index()].tp, tp - 6);
    assert_eq!(battle.technique_uses(owner, 154), Some(1));
    assert!(!battle.is_diagnostic());
    Ok(())
}
