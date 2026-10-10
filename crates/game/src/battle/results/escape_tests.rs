//! Escape commands and result commits through a prepared CPU encounter.
use super::item_tests::{Display, PreparedFixture, prepared_fixture};
use super::*;
use crate::battle::{command, lifecycle};
use resonance_battle::{BattleFrame, ButtonInput};

fn visit(f: &mut PreparedFixture, command: command::Input) -> Result<BattleFrame> {
    f.lifecycle.step(
        &mut f.battle,
        lifecycle::Input {
            command,
            ..Default::default()
        },
        &mut f.candidate,
        &mut Display,
    )
}

fn ready(f: &mut PreparedFixture) -> Result<()> {
    for _ in 0..600 {
        if f.battle.phase() != resonance_battle::BattlePhase::Entry {
            return Ok(());
        }
        visit(f, Default::default())?;
    }
    anyhow::bail!("ordinary Escape fixture entry did not finish")
}
fn edge() -> ButtonInput {
    ButtonInput {
        held: true,
        pressed: true,
        released: false,
    }
}

#[test]
#[ignore = "requires ordinary formation0, Sheena/Lloyd models and Escape voices; CPU only"]
fn escape_command_exports_before_toggle_and_keeps_the_closing_visit_held() -> Result<()> {
    let mut f = prepared_fixture(&[5, 1], 0, &[])?;
    ready(&mut f)?;
    let suspended = serde_json::to_value(&f.field)?;
    let owner = f.candidate.setup.actors[0].0;
    visit(
        &mut f,
        command::Input {
            open: edge(),
            ..Default::default()
        },
    )?;
    assert!(matches!(
        f.lifecycle.command_frame().unwrap().view,
        command::View::Strip
    ));
    visit(
        &mut f,
        command::Input {
            step: -1,
            ..Default::default()
        },
    )?;
    assert_eq!(
        f.lifecycle.command_frame().unwrap().selected,
        command::Command::Escape
    );
    f.candidate.party.members[4].hp = 1; // Stale candidate snapshot must refresh when the page opens.
    let live_hp = f.battle.actors()[owner.index()].hp as u16;
    let before = f.battle.escape_frame().unwrap();
    f.lifecycle.take_command_events();
    let frame = visit(
        &mut f,
        command::Input {
            confirm_a: edge(),
            cancel_b: edge(),
            ..Default::default()
        },
    )?;
    assert!(f.lifecycle.command_frame().is_none());
    assert_eq!(f.candidate.party.members[4].hp, live_hp);
    let after = frame.escape.unwrap();
    assert!(after.requested);
    assert_eq!(after.gauge, before.gauge);
    assert_eq!(f.battle.ledger().escape_cancellations, 0);
    let events = f.lifecycle.take_command_events();
    assert_eq!(
        events
            .iter()
            .filter(|&&event| event == command::Event::Cue(2))
            .count(),
        1
    );
    assert!(!events.contains(&command::Event::Cue(3)));
    assert_eq!(serde_json::to_value(&f.field)?, suspended);
    assert!(!f.candidate.party.battles.ordinary_escape_used);
    assert_eq!(f.candidate.party.battles.escaped, 0);
    Ok(())
}

#[test]
#[ignore = "requires ordinary formation0 and prepared Escape departure; CPU only"]
fn ordinary_escape_history_precedes_once_only_total_and_never_constructs_rewards() -> Result<()> {
    for leader in [1, 5] {
        let mut f = prepared_fixture(&[leader], 0, &[])?;
        ready(&mut f)?;
        let suspended = serde_json::to_value(&f.field)?;
        let actor = f.candidate.setup.actors[0].0;
        f.candidate.party.battles.sheena_escapes = 49;
        assert!(f.candidate.toggle_escape(&mut f.battle, actor)?);
        let mut observed_prefix = false;
        let mut outcome = None;
        for _ in 0..1800 {
            let frame = visit(&mut f, Default::default())?;
            if f.battle.ledger().ordinary_escape.is_some() && !observed_prefix {
                observed_prefix = true;
                assert_eq!(f.battle.ledger().ordinary_escape, Some(actor));
                assert!(f.candidate.party.battles.ordinary_escape_used);
                assert_eq!(
                    f.candidate.party.battles.sheena_escapes,
                    if leader == 5 { 50 } else { 49 }
                );
                assert_eq!(f.candidate.party.battles.escaped, 0);
                let history = f.candidate.party.battles.clone();
                f.candidate.sync_escape_history(&f.battle)?;
                f.candidate.sync_escape_history(&f.battle)?;
                assert_eq!(f.candidate.party.battles, history);
            }
            if frame.outcome.is_some() {
                outcome = frame.outcome;
                break;
            }
        }
        assert!(observed_prefix);
        let outcome = outcome.context("ordinary Escape lifecycle did not finish")?;
        assert_eq!(outcome.result, BattleResult::Escaped);
        assert!(!f.battle.forced_escape());
        assert!(f.candidate.results.is_none());
        assert_eq!(f.candidate.party.battles.escaped, 1);
        assert_eq!(serde_json::to_value(&f.field)?, suspended);
        let before_retry = serde_json::to_value(&f.candidate.party)?;
        assert!(visit(&mut f, Default::default()).is_err());
        assert_eq!(serde_json::to_value(&f.candidate.party)?, before_retry);
        let completed = f.candidate.finish(&f.battle, &outcome)?;
        assert_eq!(completed.party.battles.escaped, 1);
        assert_eq!(completed.party.items, f.field.items);
        assert_eq!(completed.party.gald, f.field.gald);
        assert_eq!(completed.party.battles.grade, f.field.battles.grade);
        assert_eq!(
            completed.party.members[usize::from(leader - 1)].experience,
            f.field.members[usize::from(leader - 1)].experience
        );
    }
    Ok(())
}

#[test]
#[ignore = "requires prepared opening and ordinary formations; CPU only"]
fn disabled_or_failed_escape_does_not_mutate_core_or_candidate() -> Result<()> {
    for restricted in [true, false] {
        let mut f = prepared_fixture(&[5, 1], if restricted { 1 } else { 0 }, &[])?;
        ready(&mut f)?;
        let actor = f.candidate.setup.actors[0].0;
        if !restricted {
            f.candidate.setup.actors[1].1 = 99;
        }
        let before = serde_json::to_value(&f.candidate.party)?;
        let progress = f.battle.escape_frame();
        let random = f.battle.random_state();
        let ledger = f.battle.ledger().clone();
        assert!(f.candidate.toggle_escape(&mut f.battle, actor).is_err());
        assert_eq!(serde_json::to_value(&f.candidate.party)?, before);
        assert_eq!(f.battle.escape_frame(), progress);
        assert_eq!(f.battle.random_state(), random);
        assert_eq!(f.battle.ledger(), &ledger);
    }
    Ok(())
}
