use super::*;
use crate::battle::results::{
    Results,
    item_tests::{PreparedFixture, native_fixture},
};
use resonance_battle::{ActorAvailability, PreparedBattle, Side};

// Prepare terminal combat state; the lifecycle owns recognition and every result transition.
fn fixture(defeated: Option<Side>) -> Result<PreparedFixture> {
    native_fixture(
        &[1],
        |party, _, _| {
            party.members[0].base_stats[1] = 40;
            party.members[0].tp = 9;
            Ok(())
        },
        |mut actors, setup| {
            for actor in &mut actors {
                if Some(actor.side) == defeated {
                    actor.hp = 0;
                    actor.availability = ActorAvailability::Dead;
                }
            }
            setup.style.play_music = true;
            setup.style.celebrate = true;
            setup.enemies[0].reward.gald = 50;
            PreparedBattle::new(
                (actors)
                    .into_iter()
                    .map(|actor| (actor, Default::default()))
                    .collect(),
                Default::default(),
                1,
            )?
            .finish()
        },
    )
}

#[derive(Default)]
struct Display {
    paused: bool,
    requests: Vec<Request>,
    fail_present: bool,
    pages_remaining: u8,
}
impl Presentation for Display {
    fn present(&mut self, frame: &BattleFrame) -> Result<()> {
        self.paused = frame.clock.paused();
        ensure!(!self.fail_present, "injected presentation failure");
        Ok(())
    }
    fn results_on_last_page(&self) -> bool {
        self.pages_remaining == 0
    }
    fn request(&mut self, request: Request, _: Option<&Results>, _: &Battle) -> Result<()> {
        if request == Request::NextResultPage {
            self.pages_remaining -= 1;
        }
        self.requests.push(request);
        Ok(())
    }
}

#[test]
fn menus_hold_feedback_and_presentation_failure_invalidates_the_encounter() -> Result<()> {
    let mut fixture = fixture(None)?;
    let mut display = Display::default();
    let before = fixture.battle.snapshot();
    let ledger = fixture.battle.ledger().clone();
    let frame = fixture.lifecycle.step(
        &mut fixture.battle,
        Input {
            battle: BattleInput {
                paused: true,
                ..Default::default()
            },
            ..Default::default()
        },
        &mut fixture.candidate,
        &mut display,
    )?;
    assert_eq!(frame.actors, before.actors);
    assert_eq!(fixture.battle.ledger(), &ledger);
    assert!(display.paused);
    assert!(display.requests.is_empty());

    display.fail_present = true;
    assert!(
        fixture
            .lifecycle
            .step(
                &mut fixture.battle,
                Input::default(),
                &mut fixture.candidate,
                &mut display
            )
            .is_err()
    );
    assert_eq!(fixture.battle.phase(), BattlePhase::Finished);
    assert!(fixture.lifecycle.command_frame().is_none());
    assert!(fixture.lifecycle.take_command_events().is_empty());
    assert!(fixture.battle.finish_result().is_err());
    assert!(
        fixture
            .lifecycle
            .step(
                &mut fixture.battle,
                Input::default(),
                &mut fixture.candidate,
                &mut display
            )
            .is_err()
    );
    Ok(())
}

#[test]
fn defeat_plays_music_once_and_finishes_without_constructing_rewards() -> Result<()> {
    let mut fixture = fixture(Some(Side::Party))?;
    let mut display = Display::default();
    let mut outcome = None;
    for _ in 0..300 {
        let frame = fixture.lifecycle.step(
            &mut fixture.battle,
            Input {
                confirm: true,
                ..Default::default()
            },
            &mut fixture.candidate,
            &mut display,
        )?;
        if frame.outcome.is_some() {
            outcome = frame.outcome;
            break;
        }
    }
    let outcome = outcome.expect("defeat must complete through the lifecycle");
    assert_eq!(outcome.result, BattleResult::Defeat);
    assert_eq!(
        display
            .requests
            .iter()
            .filter(|event| matches!(
                event,
                Request::PlayMusic {
                    track: DEFEAT_MUSIC,
                    ..
                }
            ))
            .count(),
        1
    );
    assert!(fixture.candidate.pending_results().is_none());
    let completed = fixture.candidate.finish(&fixture.battle, &outcome)?;
    assert_eq!(completed.party.gald, 0);
    assert_eq!(completed.party.members[0].hp, 0);
    Ok(())
}

#[test]
fn victory_pages_confirm_and_commit_once_without_notice_or_performance_delays() -> Result<()> {
    let mut fixture = fixture(Some(Side::Enemy))?;
    let mut display = Display {
        pages_remaining: 2,
        ..Default::default()
    };
    // Repeated confirmation advances each page, then finishes within three short fades.
    for _ in 0..80 {
        let frame = fixture.lifecycle.step(
            &mut fixture.battle,
            Input {
                confirm: true,
                ..Default::default()
            },
            &mut fixture.candidate,
            &mut display,
        )?;
        if let Some(pending) = fixture.candidate.pending_results() {
            assert_eq!(pending.party.gald, 50);
            assert_eq!(pending.party.members[0].tp, 19);
            assert_eq!(fixture.battle.actors()[0].tp, 19);
            assert!(pending.results.notices.contains(
                &crate::battle::results::ResultNotice::TpRecovery {
                    character: 1,
                    amount: 10
                }
            ));
            if display.pages_remaining != 0 {
                assert!(!pending.accepted);
            }
        }
        if let Some(outcome) = frame.outcome {
            assert!(fixture.candidate.pending_results().unwrap().accepted);
            for (request, count) in [
                (Request::VictoryBanner, 1),
                (Request::SynchronizeResults, 1),
                (Request::NextResultPage, 2),
            ] {
                assert_eq!(
                    display
                        .requests
                        .iter()
                        .filter(|&&event| event == request)
                        .count(),
                    count
                );
            }
            assert!(
                fixture
                    .lifecycle
                    .step(
                        &mut fixture.battle,
                        Input::default(),
                        &mut fixture.candidate,
                        &mut display
                    )
                    .is_err()
            );
            let completed = fixture.candidate.finish(&fixture.battle, &outcome)?;
            assert_eq!(completed.result, BattleResult::Victory);
            assert_eq!(completed.party.gald, 50);
            assert_eq!(completed.party.members[0].tp, 19);
            return Ok(());
        }
    }
    anyhow::bail!("result pages or confirmation stalled")
}
