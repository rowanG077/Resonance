use super::*;
use crate::battle::results::{
    Results,
    item_tests::{PreparedFixture, native_battle, native_fixture},
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
            native_battle(actors)
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
fn ordinary_battle_entry_retains_its_cover_and_fades_only_on_live_updates() -> Result<()> {
    let mut f = fixture(None)?;
    assert!(!f.lifecycle.has_events());
    let mut frame = f.battle.snapshot();
    f.lifecycle.apply_view(&mut frame)?;
    assert_eq!(frame.event_fade.as_ref().unwrap().alpha, 255);
    let mut display = Display::default();
    for (paused, alpha) in [(true, 255), (false, 239), (true, 239), (false, 223)] {
        let frame = f.lifecycle.step(
            &mut f.battle,
            Input {
                battle: BattleInput {
                    paused,
                    ..Default::default()
                },
                ..Default::default()
            },
            &mut f.candidate,
            &mut display,
        )?;
        let fade = frame.event_fade.unwrap();
        assert_eq!((fade.color, fade.alpha), ([0; 3], alpha));
    }
    for _ in 0..14 {
        frame = f.lifecycle.step(
            &mut f.battle,
            Input::default(),
            &mut f.candidate,
            &mut display,
        )?;
    }
    assert!(frame.event_fade.is_none());
    Ok(())
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
                enemy_id: resonance_battle::EnemyId(0),
                disabled_choices: Vec::new(),
                strategy: TargetPolicy::Nearest,
                difficulty: resonance_battle::Difficulty::Normal,
                choices: vec![EnemyChoice {
                    action: ActionKey(0),
                    physical_arte: false,
                    weight: 0,
                    chain_group: 0,
                    followup_chance: 0,
                    requirements: Default::default(),
                    target_policy: None,
                    target_allies: false,
                    return_to_formation: false,
                    returning: None,
                    return_speed: None,
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
                run_motion: None,
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
                    execution: ActionExecution::Attack(PreparedAttack::new(0)),
                }]
                .into(),
                1,
            )?
            .finish()
        },
    )?;
    let mut display = Display::default();
    let mut outcome = None;
    for visit in 0..=66 {
        let frame = fixture.lifecycle.step(
            &mut fixture.battle,
            Input::default(),
            &mut fixture.candidate,
            &mut display,
        )?;
        assert_eq!(frame.outcome.is_some(), visit == 66);
        let transition = fixture.battle.transition().unwrap();
        assert_eq!(transition.color, [0; 3]);
        assert_eq!(transition.alpha, (visit * 4).min(255) as u8);
        assert_eq!(
            transition.wipe.as_ref().unwrap().progress,
            visit.min(65) * 16,
        );
        if frame.outcome.is_some() {
            outcome = frame.outcome;
        }
    }
    let outcome = outcome.expect("health retreat must complete");
    assert_eq!(outcome.result, BattleResult::Escaped);
    assert_eq!(fixture.battle.ledger().elapsed_ticks, 66);
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
    for visit in 0..=222 {
        let frame = fixture.lifecycle.step(
            &mut fixture.battle,
            Input {
                confirm: true,
                ..Default::default()
            },
            &mut fixture.candidate,
            &mut display,
        )?;
        assert_eq!(
            display.requests.iter().any(|request| matches!(
                request,
                Request::PlayMusic {
                    track: DEFEAT_MUSIC,
                    ..
                }
            )),
            visit >= 91,
        );
        assert_eq!(fixture.battle.transition().is_some(), visit >= 92);
        assert_eq!(frame.outcome.is_some(), visit == 222);
        if let Some(transition) = fixture.battle.transition() {
            assert_eq!(transition.alpha, ((visit - 92) * 2).min(255) as u8);
            assert!(transition.wipe.is_some());
        }
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
fn victory_fades_and_confirmation_keep_the_result_opening_readable() -> Result<()> {
    let mut fixture = fixture(Some(Side::Enemy))?;
    let mut display = Display {
        pages_remaining: 2,
        ..Default::default()
    };
    // Holding confirm must not skip the combat wash or the opening result performance.
    for visit in 0..=233 {
        let frame = fixture.lifecycle.step(
            &mut fixture.battle,
            Input {
                confirm: true,
                ..Default::default()
            },
            &mut fixture.candidate,
            &mut display,
        )?;
        assert_eq!(frame.outcome.is_some(), visit == 233);
        assert_eq!(fixture.candidate.pending_results().is_some(), visit >= 45);
        if let Some(alpha) = match visit {
            0 => Some(0),
            1 => Some(6),
            42 => Some(252),
            43..=56 => Some(255),
            57 => Some(250),
            107..=166 => Some(0),
            168 => Some(4),
            199 => Some(128),
            231..=233 => Some(255),
            _ => None,
        } {
            assert_eq!(
                fixture.battle.transition().unwrap().alpha,
                alpha,
                "visit {visit}"
            );
        }
        if visit == 199 {
            let wipe = fixture.battle.transition().unwrap().wipe.as_ref().unwrap();
            assert_eq!(wipe.progress, 512);
            assert!(wipe.offsets.iter().all(|&offset| offset < 640));
            assert!(wipe.offsets.windows(2).any(|rows| rows[0] != rows[1]));
        }
        if let Some(pending) = fixture.candidate.pending_results() {
            assert_eq!(pending.party.gald, 50);
            assert_eq!(pending.party.members[0].tp, 19);
            assert_eq!(fixture.battle.actors()[0].tp, 19);
            assert!(pending.results.notices.contains(
                &crate::battle::results::ResultNotice::TpRecovery {
                    character: crate::battle::party::Character::Lloyd,
                    amount: 10
                }
            ));
            assert_eq!(pending.accepted, visit >= 167);
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
fn escape_notices_and_fades_finish_at_their_own_pace() -> Result<()> {
    for (forced, fade_visit, step, finish_visit) in [(false, 91, 8, 125), (true, 16, 4, 82)] {
        let mut fixture = fixture(None)?;
        fixture.battle.recognize_escape(forced)?;
        let mut display = Display::default();
        for visit in 0..=finish_visit {
            let frame = fixture.lifecycle.step(
                &mut fixture.battle,
                Input {
                    confirm: true,
                    ..Default::default()
                },
                &mut fixture.candidate,
                &mut display,
            )?;
            assert_eq!(frame.outcome.is_some(), visit == finish_visit);
            assert!(fixture.candidate.pending_results().is_none());
            if visit < fade_visit {
                assert!(fixture.battle.transition().is_none());
            } else if visit < finish_visit {
                assert_eq!(
                    fixture.battle.transition().unwrap().alpha,
                    ((visit - fade_visit) * step).min(255) as u8
                );
            }
        }
        assert_eq!(
            display.requests,
            if forced {
                vec![]
            } else {
                vec![Request::EscapeNotice]
            }
        );
    }
    Ok(())
}

#[test]
fn battle_events_pause_combat_and_release_the_boss_hold_at_rescue_entry() -> Result<()> {
    use crate::battle::event::{Plan, Prepared};
    use std::sync::Arc;
    let prepared = |delay: u16| -> Result<Prepared> {
        let words: [u16; 12] = [
            4, 0, 0, 0, 0, 0x3000, 0x4000, delay, 0x3000, 0x4000, 0x2064, 0x20ff,
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
        Plan::vidarr(prepared(20)?, prepared(4)?),
        f.field.clone(),
        boss,
        &f.battle,
    )?;
    f.battle.set_actor_vitals(boss, 20, 101, 0, 0)?;
    let mut display = Display::default();
    let mut initial = f.battle.snapshot();
    f.lifecycle.apply_view(&mut initial)?;
    assert_eq!(initial.event_fade.as_ref().unwrap().color, [0; 3]);
    assert_eq!(initial.event_fade.as_ref().unwrap().alpha, 255);
    for tick in 0..20 {
        let frame = f.lifecycle.step(
            &mut f.battle,
            Input::default(),
            &mut f.candidate,
            &mut display,
        )?;
        assert_eq!(f.lifecycle.event().is_some(), tick < 19);
        assert_eq!(
            frame.event_fade.as_ref().map_or(0, |fade| fade.alpha),
            255_u16.saturating_sub((tick + 1) * 16) as u8
        );
        assert_eq!(frame.event_dissolve, if tick == 19 { 255 } else { 0 });
        assert_eq!(f.battle.ledger().combat_ticks, 0);
        assert_eq!(f.battle.actors()[boss.index()].hp, 50);
    }
    for tick in 1..=600 {
        let frame = f.lifecycle.step(
            &mut f.battle,
            Input::default(),
            &mut f.candidate,
            &mut display,
        )?;
        if tick <= 32 {
            assert_eq!(frame.event_dissolve, 255_u16.saturating_sub(tick * 8) as u8);
        }
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
    let frame = f.lifecycle.step(
        &mut f.battle,
        Input::default(),
        &mut f.candidate,
        &mut display,
    )?;
    assert!(!frame.cues.contains(&resonance_battle::Cue::ClearEffects));
    assert_eq!(
        (
            f.battle.ledger().combat_ticks,
            f.battle.ledger().elapsed_ticks
        ),
        (600, 601)
    );
    assert_eq!(f.battle.actors()[boss.index()].hp, 20);
    for tick in 0..6 {
        let frame = f.lifecycle.step(
            &mut f.battle,
            Input::default(),
            &mut f.candidate,
            &mut display,
        )?;
        assert_eq!(
            frame
                .cues
                .iter()
                .filter(|cue| matches!(cue, resonance_battle::Cue::ClearEffects))
                .count(),
            usize::from(tick == 1),
            "only rescue entry disposes pre-event effects"
        );
        assert_eq!(f.lifecycle.event().is_some(), (1..=4).contains(&tick));
        assert_eq!(frame.event_dissolve, [0, 0, 255, 247, 239, 255][tick]);
        assert_eq!(
            (
                f.battle.ledger().combat_ticks,
                f.battle.ledger().elapsed_ticks
            ),
            (600, 602)
        );
    }
    let frame = f.lifecycle.step(
        &mut f.battle,
        Input::default(),
        &mut f.candidate,
        &mut display,
    )?;
    assert_eq!(frame.event_dissolve, 247);
    assert!(f.lifecycle.event().is_none());
    let frame = f.lifecycle.step(
        &mut f.battle,
        Input {
            battle: resonance_battle::BattleInput {
                paused: true,
                ..Default::default()
            },
            ..Default::default()
        },
        &mut f.candidate,
        &mut display,
    )?;
    assert_eq!(frame.event_dissolve, 247);
    assert_eq!(
        (
            f.battle.ledger().combat_ticks,
            f.battle.ledger().elapsed_ticks
        ),
        (601, 603)
    );
    Ok(())
}
