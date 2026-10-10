use super::*;

fn pending(battle: &mut Battle) -> crate::item::Release {
    let request = crate::item::Release {
        user: ActorId(0),
        target: ActorId(0),
        item: 1,
    };
    // These callback tests isolate the already-admitted reservation. The item
    // module separately tests public queue validation and storage consumption.
    battle.items.pending = Some(request);
    request
}

pub(super) fn occupy_primary(battle: &mut Battle) {
    battle
        .release_volley(
            Arc::new(crate::tests::volley()),
            ActorId(0),
            ActorId(1),
            crate::SpellSlot::Primary,
            None,
            &mut vec![],
        )
        .unwrap()
        .expect("fixture resident slot is vacant");
    battle.step(BattleInput::default()).unwrap();
}

#[test]
fn pending_item_cancels_chanting_without_payment() {
    for (control, occupied) in [
        (Control::Manual, false),
        (Control::SemiAuto, false),
        (Control::Auto, true),
    ] {
        let mut battle = super::spell_charge::charged(control, 30);
        if occupied {
            occupy_primary(&mut battle);
        }
        battle.step(request()).unwrap();
        let request = pending(&mut battle);
        let tp = battle.actors[0].tp;
        let frame = battle.step(BattleInput::default()).unwrap();
        assert_eq!(frame.actors[0].activity, crate::Activity::Idle);
        assert_eq!(frame.actors[0].tp, tp);
        assert_eq!(battle.technique_uses(ActorId(0), 66), Some(49));
        assert_eq!(battle.pending_item(), Some(request));
        assert!(
            !frame
                .cues
                .iter()
                .any(|cue| matches!(cue, Cue::Released { .. } | Cue::ItemNotice { .. }))
        );
        assert!(
            !battle
                .sequences()
                .map(|(_, sequence)| sequence)
                .any(|row| matches!(
                    &row.definition.execution,
                    crate::ActionExecution::Casting(_)
                ))
        );
    }
}

#[test]
fn pending_item_does_not_cancel_zero_count_release_or_occupied_release_wait() {
    for occupied in [false, true] {
        let mut battle = battle(Control::Auto, 1, CASTER_TP);
        battle.step(request()).unwrap();
        battle.step(BattleInput::default()).unwrap();
        assert_eq!(battle.casting_remaining(ActorId(0)), Some(0));
        if occupied {
            // Reserve the slot without advancing the ready cast.
            battle
                .release_volley(
                    Arc::new(crate::tests::volley()),
                    ActorId(0),
                    ActorId(1),
                    crate::SpellSlot::Primary,
                    None,
                    &mut vec![],
                )
                .unwrap()
                .unwrap();
        }
        let request = pending(&mut battle);
        let tp = battle.actors[0].tp;
        let frame = battle.step(BattleInput::default()).unwrap();
        assert_eq!(
            frame
                .cues
                .iter()
                .filter(|cue| matches!(cue, Cue::Released { .. }))
                .count(),
            usize::from(!occupied)
        );
        assert_eq!(frame.actors[0].tp, if occupied { tp } else { tp - 7 });
        assert_eq!(battle.pending_item(), Some(request));
        assert!(
            !frame
                .cues
                .iter()
                .any(|cue| matches!(cue, Cue::ItemNotice { .. }))
        );
    }
}
