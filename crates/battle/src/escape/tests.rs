use super::*;
use crate::{ActorAvailability, BattleInput, Cue, ProtectionMode, Sound, tests::actor};
#[path = "ex71_tests.rs"]
mod ex71_tests;

fn line(index: u16) -> Option<Sound> {
    Some(Sound::Cue(index))
}

fn definition(count: usize, difference: i8) -> EscapeDefinition {
    EscapeDefinition {
        allowed: true,
        level_difference: difference,
        magic_mist: false,
        actors: (0..count)
            .map(|index| EscapeActorDefinition {
                actor: ActorId(index as u8),
                request: line(51),
                cancel: line(53),
                success: line(52),
            })
            .collect(),
    }
}

// Isolated gauge/recognition arithmetic does not require pose resources. The
// movement tests cover complete preparation with model and control bindings.
fn battle(party: usize, difference: i8) -> Battle {
    let mut actors = vec![actor(Side::Party); party];
    actors.push(actor(Side::Enemy));
    let mut battle = PreparedBattle::new(
        (actors)
            .into_iter()
            .map(|actor| (actor, Default::default()))
            .collect(),
        Default::default(),
        17,
    )
    .unwrap()
    .finish()
    .unwrap();
    battle.prepared.escape = Some(definition(party, difference));
    battle
}

#[test]
fn success_requires_a_full_gauge_and_a_gameplay_update() {
    for (before, after, expected) in [
        (1023, 1021, None),
        (1024, 1024, Some(BattleResult::Escaped)),
    ] {
        let mut battle = battle(1, 0);
        battle.escape.gauge = before;

        assert_eq!(battle.recognize_result(), None);
        assert_eq!(battle.escape.gauge, before);
        assert_eq!(battle.recognize_update(), expected);
        assert_eq!(battle.escape.gauge, after);
        assert_eq!(battle.recognize_update(), expected);
        assert_eq!(battle.escape.gauge, after);
    }
}

#[test]
fn cancellation_retains_progress_until_it_drains() {
    let mut battle = battle(1, 0);
    battle.toggle_escape(ActorId(0)).unwrap();
    battle.escape.gauge = 2;

    assert!(!battle.toggle_escape(ActorId(0)).unwrap());
    assert_eq!(battle.escape.gauge, 2);
    assert_eq!(battle.ledger.escape_cancellations, 1);
    battle.recognize_update();
    assert_eq!(battle.escape.gauge, 0);
    for _ in 0..5 {
        battle.toggle_escape(ActorId(0)).unwrap();
        battle.toggle_escape(ActorId(0)).unwrap();
    }
    assert_eq!(battle.ledger.escape_cancellations, 3);
}

#[test]
fn whole_update_runs_late_gain_and_next_prefix_success_with_menu_holds() {
    let mut battle = battle(1, 0);
    battle.toggle_escape(ActorId(0)).unwrap();
    for update in 0..205 {
        // The game lifecycle recognizes before dispatching its World native;
        // Battle::step repeats that prefix in the same simulation update.
        assert_eq!(battle.recognize_update(), None);
        let frame = battle.step(BattleInput::default()).unwrap();
        assert_eq!(frame.recognized_result, None);
        assert_eq!(
            frame.escape.unwrap().gauge,
            (7 + 5 * update).min(MAX_ESCAPE_GAUGE)
        );
        if update == 100 {
            let held = battle
                .step(BattleInput {
                    paused: true,
                    ..Default::default()
                })
                .unwrap();
            assert_eq!(held.escape, frame.escape);
            let held = battle
                .step(BattleInput {
                    paused: true,
                    ..Default::default()
                })
                .unwrap();
            assert_eq!(held.escape, frame.escape);
        }
    }
    assert_eq!(battle.escape.gauge, 1024);
    assert_eq!(battle.recognize_update(), Some(BattleResult::Escaped));
    assert_eq!(battle.escape.gauge, 1024);
    assert_eq!(battle.ledger.ordinary_escape, Some(ActorId(0)));
}

#[test]
fn magic_mist_adds_before_clamp_and_survives_ko_until_equipment_refresh() {
    for (difference, expected) in [(-8, 3), (-4, 7), (0, 11), (8, 19)] {
        let mut battle = battle(1, difference);
        battle.refresh_escape_magic_mist(true);
        battle.toggle_escape(ActorId(0)).unwrap();
        battle.advance_escape();
        assert_eq!(battle.escape.gauge, expected);
        // A retained global flag does not query the wearer's live state.
        battle.actors[0].availability = ActorAvailability::Dead;
        battle.advance_escape();
        assert_eq!(battle.escape.gauge, expected * 2);
        battle.items.all_divide = true;
        battle.refresh_escape_magic_mist(false);
        assert!(battle.all_divide_active());
        assert_eq!(battle.escape.gauge, expected * 2);
        battle.advance_escape();
        assert_eq!(
            battle.escape.gauge,
            expected * 2 + (i16::from(difference) + 7).clamp(3, 24)
        );
    }
}

#[test]
fn magic_mist_changes_only_the_current_battle() {
    let mut equipped = battle(1, 0);
    equipped.refresh_escape_magic_mist(true);
    equipped.toggle_escape(ActorId(0)).unwrap();
    equipped.advance_escape();
    assert_eq!(equipped.escape.gauge, 11);
    assert!(!equipped.all_divide_active());
    let mut ordinary = battle(1, 0);
    ordinary.toggle_escape(ActorId(0)).unwrap();
    ordinary.advance_escape();
    assert_eq!(ordinary.escape.gauge, 7);
}

#[test]
fn progress_is_bounded_and_requires_escape_capability() {
    for (difference, expected) in [(-8, 3), (-4, 3), (0, 7), (8, 15)] {
        let mut battle = battle(1, difference);
        battle.toggle_escape(ActorId(0)).unwrap();
        battle.advance_escape();
        assert_eq!(battle.escape.gauge, expected);
    }
    let mut battle = battle(1, 8);
    battle.escape.requested = true;
    battle.escape.gauge = i16::MAX;
    battle.advance_escape();
    assert_eq!(battle.escape.gauge, MAX_ESCAPE_GAUGE);
    battle.prepared.escape.as_mut().unwrap().allowed = false;
    battle.advance_escape();
    assert_eq!(battle.escape.gauge, MAX_ESCAPE_GAUGE);
}

#[test]
fn global_timed_hold_stops_both_escape_visits_but_local_stop_does_not() {
    let mut battle = battle(1, 0);
    battle.toggle_escape(ActorId(0)).unwrap();
    battle.escape.gauge = 100;

    battle.timed_hold = Some(crate::overlimit::Hold {
        remaining: 3,
        actor: None,
    });
    assert_eq!(battle.recognize_update(), None);
    battle.advance_escape();
    assert_eq!(battle.escape.gauge, 100);
    battle.timed_hold = None;
    battle.actors[0].hit_stop = 3;
    battle.recognize_update();
    battle.advance_escape();
    assert_eq!(battle.escape.gauge, 105);
}

#[test]
fn escape_toggle_requests_feedback_only_for_available_owners() {
    let mut battle = battle(2, 0);
    let random = battle.random_state();
    assert!(battle.toggle_escape(ActorId(0)).unwrap());
    assert!(!battle.toggle_escape(ActorId(0)).unwrap());
    let frame = battle.step(BattleInput::default()).unwrap();
    let sounds: Vec<_> = frame
        .cues
        .iter()
        .filter_map(|cue| match cue {
            Cue::Voice {
                actor: ActorId(0),
                sound,
                ..
            } => Some(*sound),
            _ => None,
        })
        .collect();
    assert_eq!(sounds, [Sound::Cue(51), Sound::Cue(53)]);
    battle.actors[0].availability = ActorAvailability::Petrified;
    assert!(battle.toggle_escape(ActorId(0)).unwrap());
    let frame = battle.step(BattleInput::default()).unwrap();
    assert!(!frame.cues.iter().any(|cue| matches!(
        cue,
        Cue::Voice {
            actor: ActorId(0),
            ..
        }
    )));
    assert_eq!(battle.random_state(), random);
}

#[test]
fn ordinary_success_selects_non_auto_leader_even_unavailable() {
    let mut battle = battle(3, 0);
    battle.actors[0].control = Control::Auto;
    battle.actors[1].control = Control::SemiAuto;
    battle.actors[1].availability = ActorAvailability::Petrified;
    battle.actors[2].control = Control::Manual;
    for actor in &mut battle.actors {
        actor.hit_stop = 7;
    }
    battle.actors[0].reaction.protection.remaining = 220;
    battle.escape.gauge = 1024;

    let random = battle.random_state();
    battle.recognize_update();
    assert_eq!(battle.ledger.ordinary_escape, Some(ActorId(1)));
    assert!(battle.pending_cues.is_empty());
    assert_eq!(
        battle
            .actors
            .iter()
            .map(|actor| actor.hit_stop)
            .collect::<Vec<_>>(),
        [0, 7, 0, 0]
    );
    assert_eq!(battle.actors[0].reaction.protection.remaining, 220);
    assert_eq!(
        battle.actors[0].reaction.protection.mode,
        ProtectionMode::Escape
    );
    assert_eq!(
        battle.actors[1].reaction.protection.mode,
        ProtectionMode::None
    );
    assert_eq!(battle.actors[2].reaction.protection.remaining, 180);
    assert_eq!(battle.random_state(), random);
    assert_eq!(
        battle.escape_frame(),
        Some(EscapeFrame {
            requested: false,
            gauge: 1024,
        })
    );
    battle.advance_escape();
    assert_eq!(battle.escape.gauge, 1024);
}

#[test]
fn terminal_priority_prevents_ordinary_gauge_success_and_forced_metadata() {
    for (forced, party_dead, enemy_dead, expected) in [
        (true, true, true, BattleResult::Escaped),
        (false, true, true, BattleResult::Defeat),
        (false, false, true, BattleResult::Victory),
    ] {
        let mut battle = battle(1, 0);
        battle.escape.gauge = 1024;
        if party_dead {
            battle.actors[0].availability = ActorAvailability::Dead;
        }
        if enemy_dead {
            battle.actors[1].availability = ActorAvailability::Dead;
        }
        if forced {
            battle.recognize_escape(true).unwrap();
        }
        assert_eq!(battle.recognize_update(), Some(expected));
        assert_eq!(battle.escape.gauge, 1024);
        assert_eq!(battle.ledger.ordinary_escape, None);
        assert_eq!(
            battle.actors[0].reaction.protection.mode,
            ProtectionMode::None
        );
    }
}

#[test]
fn prepared_escape_rejects_missing_bindings_without_mutating_runtime() {
    let prepared = || {
        PreparedBattle::new(
            vec![
                (actor(Side::Party), Default::default()),
                (actor(Side::Enemy), Default::default()),
            ],
            Default::default(),
            0,
        )
        .unwrap()
    };
    assert!(prepared().with_escape(definition(1, 0)).finish().is_err());
    assert!(prepared().with_escape(definition(0, 0)).finish().is_err());
    assert!(prepared().with_escape(definition(1, 9)).finish().is_err());
    let mut battle = battle(1, 0);
    battle.prepared.escape.as_mut().unwrap().allowed = false;
    assert!(battle.toggle_escape(ActorId(0)).is_err());
    assert!(!battle.escape.requested);
    assert_eq!(battle.ledger.escape_cancellations, 0);
    assert!(battle.toggle_escape(ActorId(1)).is_err());
}
