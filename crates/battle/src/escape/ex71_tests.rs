use super::*;

#[test]
fn ex71_adds_once_before_clamp_and_composes_with_the_independent_gear_flag() {
    for (difference, ordinary, skill, both) in [
        (-8, 3, 3, 7),
        (-4, 3, 7, 11),
        (0, 7, 11, 15),
        (8, 15, 19, 23),
    ] {
        for (learned_recipe, mist, expected) in [
            (false, false, ordinary),
            (true, false, skill),
            (false, true, skill),
            (true, true, both),
        ] {
            let mut battle = battle(2, difference);
            for actor in &mut battle.actors[..2] {
                actor.equipment.quick_escape = learned_recipe;
            }
            battle.refresh_escape_magic_mist(mist);
            battle.toggle_escape(ActorId(0)).unwrap();
            let random = battle.random_state();
            battle.advance_escape();
            assert_eq!(battle.escape.gauge, expected);
            assert_eq!(battle.random_state(), random);
        }
    }
    let mut battle = battle(1, 8);
    battle.prepared.escape.as_mut().unwrap().level_difference = 30;
    battle.actors[0].equipment.quick_escape = true;
    battle.refresh_escape_magic_mist(true);
    battle.escape.requested = true;
    battle.escape.gauge = i16::MAX;
    battle.advance_escape();
    assert_eq!(battle.escape.gauge, MAX_ESCAPE_GAUGE);
}

#[test]
fn ex71_queries_current_available_party_actors_while_magic_mist_survives_ko() {
    let mut battle = battle(2, 0);
    battle.actors[0].equipment.quick_escape = true;
    battle.actors[2].equipment.quick_escape = true;
    battle.refresh_escape_magic_mist(true);
    battle.toggle_escape(ActorId(0)).unwrap();
    battle.items.all_divide = true;
    for (availability, expected) in [
        (ActorAvailability::Active, 15),
        (ActorAvailability::Dead, 11),
        (ActorAvailability::Petrified, 11),
        (ActorAvailability::Absent, 11),
        (ActorAvailability::Active, 15),
    ] {
        battle.actors[0].availability = availability;
        let before = battle.escape.gauge;
        battle.advance_escape();
        assert_eq!(battle.escape.gauge - before, expected);
        assert!(battle.all_divide_active());
    }
    battle.actors[0].hp = 0;
    let before = battle.escape.gauge;
    battle.advance_escape();
    assert_eq!(battle.escape.gauge - before, 15);
    battle.actors[0].equipment.quick_escape = false;
    battle.refresh_escape_magic_mist(false);
    let before = battle.escape.gauge;
    battle.advance_escape();
    assert_eq!(battle.escape.gauge - before, 7);
    assert!(battle.all_divide_active());
}
