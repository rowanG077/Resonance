use super::*;
use crate::{
    Activity, ResourceBinding, SpellSlot,
    tests::{actor, prepared},
};

fn battle(body: &str, spell: &str, duration: u16) -> Battle {
    let source = format!(
        r#"
        asset spell: battle::Spell = "test/spell";
        asset marker: battle::Effect = "test/marker";
        pub task run() {{ {body} }}
        pub task resident() {{ {spell} }}
    "#
    );
    let mut p = Arc::try_unwrap(prepared(
        &source,
        vec![actor(Side::Party), actor(Side::Enemy), actor(Side::Party)],
        50,
    ))
    .unwrap();
    p.actions[0].phase = ActionPhase::Actor;
    p.actions[0].resources = vec![ResourceBinding::Spell(100), ResourceBinding::Effect(7)];
    let mut released = p.actions[0].clone();
    released.id = 100;
    released.phase = ActionPhase::Resident;
    released.tp_cost = 0;
    released.duration = duration;
    released.entry = released
        .program
        .authored()
        .unwrap()
        .functions
        .iter()
        .find(|f| f.name == "test::resident")
        .unwrap()
        .entry;
    p.actions.push(released);
    Battle::new(Arc::new(
        PreparedBattle::new(
            p.actors,
            p.actions,
            1,
            vec![],
            vec![crate::tests::effect_binding(7, [6])],
        )
        .unwrap(),
    ))
}

fn request() -> BattleInput {
    BattleInput {
        actions: vec![ActionRequest {
            actor: ActorId(0),
            action: 99,
            target: ActorId(1),
        }],
        ..Default::default()
    }
}

fn effects(frame: &BattleFrame) -> Vec<ActionId> {
    frame
        .cues
        .iter()
        .filter_map(|c| {
            if let Cue::Effect { action, .. } = c {
                Some(*action)
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn two_slots_visit_primary_first_and_do_not_keep_the_caster_busy() {
    let mut battle = battle(
        "battle::release(spell, true); battle::release(spell, false); battle::finish();",
        "battle::show(marker, 6, battle::owner());",
        3,
    );
    let first = battle.step(request()).unwrap();
    assert_eq!(effects(&first), [ActionId(3), ActionId(2)]);
    assert_eq!(
        first.actions,
        [(ActionId(2), ActorId(0), 0), (ActionId(3), ActorId(0), 0)]
    );
    assert_eq!(battle.actors()[0].tp, 40);
    let next = battle.step(request()).unwrap();
    assert!(next.cues.iter().any(|c| matches!(
        c,
        Cue::Started {
            actor: ActorId(0),
            ..
        }
    )));
    assert!(!next.cues.iter().any(|c| matches!(c, Cue::Released { .. })));
    assert_eq!(
        next.actions,
        [(ActionId(2), ActorId(0), 1), (ActionId(3), ActorId(0), 1)]
    );
}

#[test]
fn released_spell_survives_caster_interruption_and_death() {
    let mut battle = battle(
        "battle::release(spell, false); await battle::at_age(ticks(30));",
        "await battle::at_age(ticks(2)); battle::show(marker, 6, battle::owner());",
        4,
    );
    battle.step(request()).unwrap();
    let interrupted = battle
        .step(BattleInput {
            interrupt: vec![ActionId(1)],
            ..Default::default()
        })
        .unwrap();
    assert!(interrupted.cues.contains(&Cue::Interrupted {
        action: ActionId(1)
    }));
    battle.actors[0].hp = 0;
    battle.step(BattleInput::default()).unwrap();
    let emission = battle.step(BattleInput::default()).unwrap();
    assert_eq!(effects(&emission), [ActionId(2)]);
    assert!(battle.spell_active(ActorId(0), SpellSlot::Primary));
    battle.step(BattleInput::default()).unwrap();
    let end = battle.step(BattleInput::default()).unwrap();
    assert_eq!(
        end.cues,
        [Cue::Completed {
            action: ActionId(2)
        }]
    );
    assert!(!battle.spell_active(ActorId(0), SpellSlot::Primary));
}

#[test]
fn released_spell_survives_caster_hurt_while_actor_tasks_are_cancelled() {
    let mut battle = battle(
        "battle::release(spell, false); await battle::at_age(ticks(30));",
        "await battle::at_age(ticks(2)); battle::show(marker, 6, battle::owner());",
        4,
    );
    battle.step(request()).unwrap();
    battle.actors[0].activity = Activity::Hurt;
    battle.actors[0].reaction.remaining = 10;
    let interrupted = battle.step(BattleInput::default()).unwrap();
    assert!(interrupted.cues.contains(&Cue::Interrupted {
        action: ActionId(1)
    }));
    assert!(!battle.sequences.contains_key(&ActionId(1)));
    assert!(battle.spell_active(ActorId(0), SpellSlot::Primary));
    battle.step(BattleInput::default()).unwrap();
    let emission = battle.step(BattleInput::default()).unwrap();
    assert_eq!(effects(&emission), [ActionId(2)]);
    assert_eq!(emission.actors[0].activity, Activity::Hurt);
    battle.step(BattleInput::default()).unwrap();
    let end = battle.step(BattleInput::default()).unwrap();
    assert_eq!(
        end.cues,
        [Cue::Completed {
            action: ActionId(2)
        }]
    );
    assert!(!battle.spell_active(ActorId(0), SpellSlot::Primary));
}

#[test]
fn initialization_and_next_update_preserve_the_first_active_age_zero() {
    let mut battle = battle(
        "battle::release(spell, false); battle::finish();",
        "battle::show(marker, 6, battle::owner()); await battle::next_update(); battle::show(marker, 6, battle::owner()); await battle::at_age(ticks(1)); battle::show(marker, 6, battle::owner());",
        2,
    );
    for (update, age) in [(0, Some(0)), (1, Some(1)), (2, Some(2)), (3, None)] {
        let frame = battle
            .step(if update == 0 {
                request()
            } else {
                BattleInput::default()
            })
            .unwrap();
        assert_eq!(effects(&frame).len(), usize::from(update < 3));
        assert_eq!(battle.action_age(ActionId(2)), age);
    }
}

#[test]
fn occupied_slot_reports_failure_and_retains_its_old_target_and_clock() {
    let mut battle = battle(
        "battle::release(spell, false); await battle::next_update(); if battle::spell_active(false) && !battle::release(spell, false) { battle::heal_percent(battle::owner(), 10); }",
        "await battle::at_age(ticks(4));",
        10,
    );
    battle.step(request()).unwrap();
    battle.sequences.get_mut(&ActionId(1)).unwrap().target = ActorId(2);
    let next = battle.step(BattleInput::default()).unwrap();
    assert_eq!(next.actors[0].hp, 60);
    assert_eq!(battle.sequences[&ActionId(2)].target, ActorId(1));
    assert_eq!(battle.action_age(ActionId(2)), Some(1));
}

#[test]
fn script_commits_tp_at_its_release_boundary_and_interrupting_beforehand_is_free() {
    for interrupt_at in [1, 3] {
        let mut battle = battle(
            "await battle::at_age(ticks(2)); battle::pay_tp(battle::tp_cost()); battle::release(spell, false); await battle::at_age(ticks(10));",
            "",
            10,
        );
        battle.step(request()).unwrap();
        assert_eq!(battle.actors()[0].tp, 40);
        for _ in 1..interrupt_at {
            battle.step(BattleInput::default()).unwrap();
        }
        battle
            .step(BattleInput {
                interrupt: vec![ActionId(1)],
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            battle.actors()[0].tp,
            if interrupt_at == 1 { 40 } else { 12 }
        );
        assert_eq!(
            battle.spell_active(ActorId(0), SpellSlot::Primary),
            interrupt_at == 3
        );
    }
}

#[test]
fn insufficient_live_tp_and_duplicate_commit_do_not_debit_again() {
    let mut battle = battle(
        "await battle::at_age(ticks(1)); if !battle::pay_tp(battle::tp_cost()) { battle::heal_percent(battle::owner(), 10); }",
        "",
        10,
    );
    battle.step(request()).unwrap();
    battle.actors[0].tp = 10;
    battle.step(BattleInput::default()).unwrap();
    assert_eq!((battle.actors()[0].tp, battle.actors()[0].hp), (10, 60));
    let mut battle = self::battle(
        "battle::pay_tp(battle::tp_cost()); battle::pay_tp(battle::tp_cost());",
        "",
        10,
    );
    assert!(
        battle
            .step(request())
            .unwrap_err()
            .to_string()
            .contains("already committed")
    );
    assert_eq!(battle.actors()[0].tp, 12);
    assert!(battle.sequences.is_empty());
}

#[test]
fn preparation_rejects_unresolved_nonresident_and_charged_spell_bindings() {
    let b = battle("", "", 10);
    for fault in 0..3 {
        let mut actions = b.prepared.actions.clone();
        match fault {
            0 => {
                actions.remove(1);
            }
            1 => actions[1].phase = ActionPhase::Actor,
            _ => actions[1].tp_cost = 1,
        }
        assert!(
            PreparedBattle::new(
                b.actors.clone(),
                actions,
                1,
                vec![],
                vec![crate::tests::effect_binding(7, [6])]
            )
            .unwrap_err()
            .to_string()
            .contains("spell binding")
        );
    }
}

#[test]
fn ordinary_resident_initialization_and_active_ages_match_dolphin() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/fixtures/opening-residents.json")).unwrap();
    let rows = fixture["observations"].as_array().unwrap();
    assert_eq!(rows.len(), 110);
    let mut b = battle("battle::release(spell, false); battle::finish();", "", 90);
    // The checkpoint starts partway through one spell. Later rows contain a
    // complete fresh admission, initialization and inclusive age-90 expiry.
    b.step(request()).unwrap();
    b.sequences.get_mut(&ActionId(2)).unwrap().age =
        rows[0]["before"]["age"].as_u64().unwrap() as u32;
    for row in rows {
        if row["before"]["phase"] == 0 {
            b = battle("battle::release(spell, false); battle::finish();", "", 90);
        }
        let frame = b
            .step(if row["before"]["phase"] == 0 {
                request()
            } else {
                BattleInput::default()
            })
            .unwrap();
        if row["after"]["mode"] == 0 {
            assert!(frame.cues.contains(&Cue::Completed {
                action: ActionId(2)
            }));
            assert!(!b.spell_active(ActorId(0), SpellSlot::Primary));
        } else {
            assert_eq!(
                b.action_age(ActionId(2)).unwrap(),
                row["after"]["age"].as_u64().unwrap() as u32,
                "combat tick {}",
                row["combat_tick"]
            );
        }
    }
}
