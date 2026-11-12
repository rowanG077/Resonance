//! Item effects beyond the original game's, as mods bind them.
use super::*;

#[test]
fn all_multiply_lasts_for_the_battle_like_all_divide() {
    let mut battle = battle();
    let mut storage = Storage::new(700, 1);
    assert!(!battle.all_multiply_active());
    use_item(&mut battle, &mut storage, request(700, 0));
    assert!(battle.all_multiply_active() && !battle.all_divide_active());
    assert_eq!(storage.inventory.get(&700).copied().unwrap_or(0), 0);
    advance(&mut battle, &mut storage, 300);
    assert!(battle.all_multiply_active());
    assert!(!super::battle().all_multiply_active());
}

#[test]
fn unison_gauge_item_fills_the_gauge_once_unison_attacks_exist() {
    for unlocked in [true, false] {
        let mut battle = battle();
        battle.prepared.unison_available = unlocked;
        battle.unison_gauge = 100;
        let mut storage = Storage::new(701, 1);
        let frame = use_item(&mut battle, &mut storage, request(701, 0));
        assert_eq!(
            battle.unison_gauge(),
            if unlocked {
                crate::MAX_UNISON_GAUGE
            } else {
                100
            }
        );
        assert_eq!(
            frame.cues.iter().any(|cue| matches!(cue, Cue::UnisonReady)),
            unlocked
        );
        // The item is spent either way, like any other battle item.
        assert_eq!(storage.inventory.get(&701).copied().unwrap_or(0), 0);
    }
}

#[test]
fn over_limit_item_enters_at_once_and_waits_behind_an_ally() {
    let mut battle = battle();
    let mut storage = Storage::new(702, 2);
    assert!(battle.item_target_eligible(702, ActorId(1)).unwrap());
    assert!(!battle.item_target_eligible(702, ActorId(2)).unwrap());
    let frame = use_item(&mut battle, &mut storage, request(702, 1));
    assert!(battle.actors[1].overlimit.is_active());
    assert!(
        frame
            .cues
            .iter()
            .any(|cue| matches!(cue, Cue::OverLimitEntered { actor, .. } if *actor == ActorId(1)))
    );
    assert!(!battle.item_target_eligible(702, ActorId(1)).unwrap());

    // One actor per side: the second is charged and enters when the first ends.
    advance(&mut battle, &mut storage, 200);
    assert!(battle.actors[1].overlimit.is_active());
    use_item(&mut battle, &mut storage, request(702, 0));
    assert!(!battle.actors[0].overlimit.is_active());
    assert_eq!(battle.actors[0].overlimit.charge(), 1000);
    for _ in 0..2000 {
        if battle.actors[0].overlimit.is_active() {
            break;
        }
        step(&mut battle, &mut storage);
    }
    assert!(battle.actors[0].overlimit.is_active() && !battle.actors[1].overlimit.is_active());
}
