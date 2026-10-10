//! The encounter frame and repeated snapshots publish the same current Hourglass timer.
use super::*;

#[test]
#[ignore = "requires current Items/profile/script publications; CPU only"]
fn hourglass_stage_sample_survives_lifecycle_release_and_expiry() -> Result<()> {
    let PreparedFixture {
        mut candidate,
        mut battle,
        mut lifecycle,
        ..
    } = prepared_fixture_with_party(&[1], 2, &[], |party, session, _| {
        party.items.clear();
        party
            .change_item(session, 39, 1)
            .map_err(anyhow::Error::msg)?;
        Ok(())
    })?;
    enter_battle(&mut candidate, &mut battle)?;
    let user = candidate.setup.actors[0].0;
    battle.queue_item(Release {
        user,
        target: user,
        item: 39,
    })?;
    let mut released = false;
    let mut expired = false;
    for _ in 0..700 {
        let before = battle.hourglass_remaining();
        let frame = lifecycle.step(
            &mut battle,
            crate::battle::lifecycle::Input::default(),
            &mut candidate,
            &mut Display,
        )?;
        assert_eq!(battle.phase(), resonance_battle::BattlePhase::Combat);
        let after = battle.hourglass_remaining();
        assert_eq!(frame.hourglass_remaining, after);
        if before == 0 && after == 300 {
            assert!(!released, "the item released more than once");
            assert_eq!(frame.hourglass_remaining, 300);
            assert_eq!(battle.snapshot().hourglass_remaining, 300);
            assert!(!candidate.party.items.contains_key(&39));
            released = true;
        } else if before == 1 && after == 0 {
            assert!(released);
            assert_eq!(frame.hourglass_remaining, 0);
            assert_eq!(battle.snapshot().hourglass_remaining, 0);
            expired = true;
        } else if expired {
            assert_eq!(frame.hourglass_remaining, 0);
            assert!(!battle.is_diagnostic());
            return Ok(());
        }
    }
    anyhow::bail!("Hourglass lifecycle fixture did not release and expire")
}
