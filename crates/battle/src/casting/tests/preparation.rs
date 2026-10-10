use super::*;

#[test]
fn casting_rejects_an_empty_volley() {
    let mut prepared = prepared_cast(Control::Manual, 3, CASTER_TP);
    Arc::make_mut(&mut casting_definition(&mut prepared).release).shots = 0;
    assert!(prepared.finish().is_err());
}

#[test]
fn casting_payment_release_and_recovery_are_independent_of_artwork() -> anyhow::Result<()> {
    let mut prepared = prepared_cast(Control::Manual, 3, CASTER_TP);
    let cast = casting_definition(&mut prepared);
    cast.recovery = 6;
    let mut battle = prepared
        .with_technique_learning_members(vec![crate::tests::counted_techniques(
            ActorId(0),
            &[66],
            &[(66, 49)],
        )])?
        .finish()?;
    battle.step(request())?;
    let mut releases = 0;
    let mut projectiles = 0;
    for _ in 0..48 {
        let frame = battle.step(BattleInput::default())?;
        for cue in &frame.cues {
            if matches!(cue, Cue::Released { .. }) {
                releases += 1;
                assert_eq!(frame.actors[0].activity, crate::Activity::Recovering);
            }
            projectiles += usize::from(matches!(cue, Cue::ProjectileStarted { .. }));
        }
    }
    assert_eq!((releases, projectiles), (1, 3));
    assert_eq!(battle.actors[0].tp, 40);
    assert_eq!(battle.technique_uses(ActorId(0), 66), Some(50));
    assert_eq!(battle.activity(ActorId(0)), crate::Activity::Idle);
    assert!(!battle.is_diagnostic());
    Ok(())
}
