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

#[test]
fn enemy_retreat_returns_after_departure_without_rewards_or_escape_history() -> Result<()> {
    use resonance_battle::{
        ActionDefinition, ActionExecution, ActionKey, ActorSetup, EnemyBehavior, EnemyChoice,
        EnemyDecisionDefinition, PreparedAttack, TargetPolicy,
    };
    let mut fixture = native_fixture(
        &[1],
        |_, _, _| Ok(()),
        |mut actors, setup| {
            setup.enemies[0].reward.gald = 50;
            setup.enemies[0].reward.experience = 100;
            let enemy = actors.last_mut().unwrap();
            enemy.hp = 1;
            enemy.equipment.max_hp = 2;
            let decision = EnemyDecisionDefinition {
                enemy_id: 0,
                disabled_choices: Vec::new(),
                strategy: TargetPolicy::Nearest,
                difficulty: 0,
                choices: vec![EnemyChoice {
                    action: ActionKey(0),
                    weight: 0,
                    chain_group: 0,
                    followup_chance: 0,
                    requirements: Default::default(),
                    target_policy: None,
                    target_allies: false,
                    return_to_formation: false,
                    guard_chance: 0,
                    range: [0, 1],
                    tp: 0,
                    approach_minimum: 0.,
                    approach_range: 1.,
                    approach: None,
                }],
                back_row: vec![],
                walk_speed: 1.,
                walk_override: None,
                behavior: Some(EnemyBehavior::Retreat {
                    hp_percent: 50,
                    after_ticks: 3600,
                }),
                walk_motion: None,
                turn_ticks: 1,
            };
            PreparedBattle::new(
                actors
                    .into_iter()
                    .map(|actor| {
                        let setup = ActorSetup {
                            enemy_decision: (actor.side == Side::Enemy).then(|| decision.clone()),
                            ..Default::default()
                        };
                        (actor, setup)
                    })
                    .collect(),
                vec![ActionDefinition {
                    normal: None,
                    tp_cost: 0,
                    execution: ActionExecution::Attack(PreparedAttack {
                        contact_recovery: None,
                        chain_at: None,
                        end_at: 0,
                        opening: None,
                        events: vec![],
                        recovery: 0,
                    }),
                }]
                .into(),
                1,
            )?
            .finish()
        },
    )?;
    let mut display = Display::default();
    let mut outcome = None;
    for visit in 0..=65 {
        let frame = fixture.lifecycle.step(
            &mut fixture.battle,
            Input::default(),
            &mut fixture.candidate,
            &mut display,
        )?;
        assert_eq!(frame.outcome.is_some(), visit == 65);
        if frame.outcome.is_some() {
            outcome = frame.outcome;
        }
    }
    let outcome = outcome.expect("health retreat must complete");
    assert_eq!(outcome.result, BattleResult::Escaped);
    assert_eq!(fixture.battle.ledger().elapsed_ticks, 65);
    assert_eq!(fixture.battle.ledger().combat_ticks, 0);
    assert!(fixture.battle.ledger().ordinary_escape.is_none());
    assert!(fixture.candidate.pending_results().is_none());
    assert!(display.requests.is_empty());
    let completed = fixture.candidate.finish(&fixture.battle, &outcome)?;
    assert_eq!(
        completed.party.battles.escaped,
        fixture.field.battles.escaped
    );
    assert_eq!(
        completed.party.battles.ordinary_escape_used,
        fixture.field.battles.ordinary_escape_used
    );
    assert_eq!(completed.party.items, fixture.field.items);
    assert_eq!(completed.party.gald, fixture.field.gald);
    let vitals = |party: &resonance_events::party::Party| {
        party
            .members
            .iter()
            .map(|member| (member.hp, member.tp, member.experience))
            .collect::<Vec<_>>()
    };
    assert_eq!(vitals(&completed.party), vitals(&fixture.field));
    Ok(())
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

#[test]
fn battle_events_pause_combat_and_release_the_boss_hold_at_rescue_entry() -> Result<()> {
    use crate::battle::event::{Plan, Prepared};
    use std::sync::Arc;
    let prepared = || -> Result<Prepared> {
        let words: [u16; 12] = [
            4, 0, 0, 0, 0, 0x3000, 0x4000, 2, 0x3000, 0x4000, 0x2064, 0x20ff,
        ];
        let bytes: Vec<_> = words.into_iter().flat_map(u16::to_be_bytes).collect();
        Ok(Prepared::new(
            Arc::new(symphonia_script::Program::decode(&bytes)?),
            Arc::new(resonance_events::ResourceLibrary::default()),
        ))
    };
    let mut f = fixture(None)?;
    let boss = f
        .battle
        .actor_ids()
        .find(|id| f.battle.actors()[id.index()].side == Side::Enemy)
        .unwrap();
    f.lifecycle = Lifecycle::new(None).with_events(
        Plan::vidarr(prepared()?, prepared()?),
        f.field.clone(),
        boss,
        &f.battle,
    )?;
    f.battle.set_actor_vitals(boss, 20, 101, 0, 0)?;
    let mut display = Display::default();
    for tick in 0..2 {
        f.lifecycle.step(
            &mut f.battle,
            Input::default(),
            &mut f.candidate,
            &mut display,
        )?;
        assert_eq!(f.lifecycle.event().is_some(), tick == 0);
        assert_eq!(f.battle.ledger().combat_ticks, 0);
        assert_eq!(f.battle.actors()[boss.index()].hp, 50);
    }
    for _ in 0..600 {
        f.lifecycle.step(
            &mut f.battle,
            Input::default(),
            &mut f.candidate,
            &mut display,
        )?;
    }
    assert!(f.lifecycle.event().is_none());
    assert_eq!(
        (
            f.battle.ledger().combat_ticks,
            f.battle.ledger().elapsed_ticks
        ),
        (600, 600)
    );
    f.battle.set_actor_vitals(boss, 20, 101, 0, 0)?;
    f.lifecycle.step(
        &mut f.battle,
        Input::default(),
        &mut f.candidate,
        &mut display,
    )?;
    assert_eq!(
        (
            f.battle.ledger().combat_ticks,
            f.battle.ledger().elapsed_ticks
        ),
        (600, 601)
    );
    assert_eq!(f.battle.actors()[boss.index()].hp, 20);
    for tick in 0..4 {
        f.lifecycle.step(
            &mut f.battle,
            Input::default(),
            &mut f.candidate,
            &mut display,
        )?;
        assert_eq!(f.lifecycle.event().is_some(), matches!(tick, 1 | 2));
        assert_eq!(
            (
                f.battle.ledger().combat_ticks,
                f.battle.ledger().elapsed_ticks
            ),
            (600, 602)
        );
    }
    f.lifecycle.step(
        &mut f.battle,
        Input::default(),
        &mut f.candidate,
        &mut display,
    )?;
    assert!(f.lifecycle.event().is_none());
    assert_eq!(
        (
            f.battle.ledger().combat_ticks,
            f.battle.ledger().elapsed_ticks
        ),
        (601, 603)
    );
    Ok(())
}
