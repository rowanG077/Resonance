use super::*;

fn controlled(mode: Control, duration: u16) -> Battle {
    super::spell_charge::charged(mode, duration)
}

fn held(actor: ActorId, guard: bool, technique: bool) -> BattleInput {
    let mut input = crate::ControlInput::neutral(actor);
    input.guard.held = guard;
    input.technique.held = technique;
    BattleInput {
        controllers: vec![input],
        ..Default::default()
    }
}

#[test]
fn guard_cancels_unfinished_casts_without_payment_or_release() {
    for mode in [Control::Manual, Control::SemiAuto] {
        {
            let mut battle = controlled(mode, 20);
            battle.step(request()).unwrap();
            battle.step(BattleInput::default()).unwrap();
            let tp = battle.actors[0].tp;
            assert!(
                battle
                    .casting_remaining(ActorId(0))
                    .is_some_and(|remaining| remaining > 0)
            );
            let frame = battle.step(held(ActorId(0), true, false)).unwrap();
            assert_eq!(frame.actors[0].activity, crate::Activity::Idle);
            assert_eq!(frame.actors[0].tp, tp);
            assert_eq!(battle.actors[0].casting_state.interrupted, None);
            assert_eq!(battle.technique_uses(ActorId(0), 66), Some(49));
            assert!(
                !frame
                    .cues
                    .iter()
                    .any(|cue| matches!(cue, Cue::Released { .. }))
            );
        }
    }
}

#[test]
fn guard_uses_current_control_mode_and_the_casters_input_channel() {
    let mut battle = controlled(Control::Auto, 20);
    battle.step(request()).unwrap();
    battle.step(held(ActorId(0), true, false)).unwrap();
    assert!(matches!(
        battle.activity(ActorId(0)),
        crate::Activity::Casting { held: false }
    ));
    battle
        .set_control_mode(ActorId(0), Control::SemiAuto)
        .unwrap();
    battle.step(held(ActorId(2), true, false)).unwrap();
    assert!(matches!(
        battle.activity(ActorId(0)),
        crate::Activity::Casting { held: false }
    ));
    battle.step(held(ActorId(0), true, false)).unwrap();
    assert_eq!(battle.activity(ActorId(0)), crate::Activity::Idle);
}

#[test]
fn global_pause_holds_guard_but_local_hit_stop_allows_cancellation() {
    let mut battle = controlled(Control::SemiAuto, 20);
    battle.step(request()).unwrap();
    let remaining = cast_remaining(&battle);
    let mut input = held(ActorId(0), true, true);
    input.paused = true;
    battle.step(input).unwrap();
    assert_eq!(cast_remaining(&battle), remaining);
    battle.actors[0].hit_stop = 3;
    battle.step(held(ActorId(0), true, true)).unwrap();
    assert_eq!(battle.activity(ActorId(0)), crate::Activity::Idle);
}

#[test]
fn ready_delayed_cast_survives_guard_and_releases_once_when_unheld() {
    {
        let mut battle = controlled(Control::Manual, 1);
        battle.step(request()).unwrap();
        for _ in 0..4 {
            battle.step(held(ActorId(0), false, true)).unwrap();
        }
        assert!(matches!(
            battle.activity(ActorId(0)),
            crate::Activity::Casting { held: true }
        ));
        let tp = battle.actors[0].tp;
        for _ in 0..3 {
            let frame = battle.step(held(ActorId(0), true, true)).unwrap();
            assert_eq!(frame.actors[0].tp, tp);
            assert!(
                !frame
                    .cues
                    .iter()
                    .any(|cue| matches!(cue, Cue::Released { .. }))
            );
        }
        let mut releases = 0;
        for _ in 0..180 {
            let frame = battle.step(BattleInput::default()).unwrap();
            releases += frame
                .cues
                .iter()
                .filter(|cue| matches!(cue, Cue::Released { .. }))
                .count();
        }
        assert_eq!(releases, 1);
        assert_eq!(battle.actors[0].tp, tp - 7);
        assert_eq!(battle.technique_uses(ActorId(0), 66), Some(50));
    }
}
