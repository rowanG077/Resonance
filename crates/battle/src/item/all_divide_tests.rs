use super::*;

#[test]
fn all_divide_release_reuse_and_fresh_battle_lifetime() {
    let mut battle = battle();
    let mut storage = Storage::new(38, 2);
    assert!(!battle.all_divide_active());
    for count in 1..=2 {
        before_release(&mut battle, &mut storage, request(38, 0));
        let frame = step(&mut battle, &mut storage);
        assert!(battle.all_divide_active());
        assert_eq!(battle.ledger.items[0], count);
        assert_eq!(battle.ledger.grade(), -5 * count as i16);
        assert_eq!(storage.inventory.get(&38).copied().unwrap_or(0), 2 - count);
        assert_eq!(battle.item_cooldown(), 120);
        assert!(frame.cues.iter().any(|cue| matches!(
            cue,
            Cue::ItemNotice {
                item: 38,
                duration: 90,
                ..
            }
        )));
        assert!(
            battle
                .actors
                .iter()
                .all(|actor| actor.conditions.effective().is_empty())
        );
        for _ in 0..300 {
            step(&mut battle, &mut storage);
        }
        assert!(battle.all_divide_active());
    }
    // The flag belongs to one encounter, independently of retained inventory.
    assert!(!super::battle().all_divide_active());
}

#[test]
fn all_divide_invalidated_target_and_failed_loan_do_not_activate_or_spend() {
    for failure in ["dead", "petrified", "stock"] {
        let mut battle = battle();
        battle.set_diagnostics(Diagnostics::new(false));
        let mut storage = Storage::new(38, 1);
        before_release(&mut battle, &mut storage, request(38, 1));
        match failure {
            "dead" => battle.actors[1].availability = crate::ActorAvailability::Dead,
            "petrified" => {
                battle.actors[1].availability = crate::ActorAvailability::Petrified;
            }
            _ => storage.inventory.clear(),
        }
        step(&mut battle, &mut storage);
        assert!(!battle.all_divide_active(), "{failure}");
        assert_eq!(battle.ledger.items[0], 0);
        assert_eq!(battle.ledger.grade(), 0);
        assert!(battle.pending_item().is_none());
        if failure != "stock" {
            assert_eq!(storage.acquisitions, 0);
            assert_eq!(storage.inventory[&38], 1);
        }
    }
}

#[test]
fn all_divide_leaves_item_recovery_unchanged() {
    let mut battle = battle();
    let mut storage = Storage::new(38, 1);
    before_release(&mut battle, &mut storage, request(38, 0));
    step(&mut battle, &mut storage);
    for _ in 0..120 {
        step(&mut battle, &mut storage);
    }
    battle.actors[1].hp = 1;
    storage.inventory.insert(1, 1);
    before_release(&mut battle, &mut storage, request(1, 1));
    step(&mut battle, &mut storage);
    assert_eq!(battle.actors[1].hp, 99); // 328 * 30 / 100 = 98, unaffected.
    assert!(battle.all_divide_active());
}
