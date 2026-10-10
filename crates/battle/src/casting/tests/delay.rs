use super::*;

pub(super) fn controlled(control: Control, duration: u16) -> Battle {
    controlled_prepared(control, duration, CASTER_TP)
        .finish()
        .unwrap()
}

pub(super) fn controlled_prepared(control: Control, duration: u16, tp: u16) -> PreparedBattle {
    let base = prepared_cast(control, duration, tp);
    let mut actors = base.actors;
    actors[0].movement.direction = [0., 0., 1.];
    actors[0].facing_direction = [0., 0., 1.];
    let mut issuer = crate::tests::actor(Side::Party);
    issuer.control = Control::Manual;
    issuer.control_slot = 3;
    issuer.movement.direction = issuer.facing_direction;
    actors.push(issuer);
    let mut actions = base.resources.actions;
    actions.insert(actions[CAST].as_ref().clone());
    let normals = crate::tests::normal_controls(&mut actions, crate::tests::attack(20), [0., 100.]);
    let controls = [0, 2].map(|_| crate::ControlDefinition {
        normals,
        shortcuts: [0; 4],
        walk_speed: 1.,
        run_speed: 2.,
        turn_ticks: 1,
        motions: None,
    });
    let mut prepared = PreparedBattle::new(
        (actors)
            .into_iter()
            .map(|actor| (actor, Default::default()))
            .collect(),
        actions,
        1,
    )
    .unwrap();
    for (index, definition) in [0, 2].into_iter().zip(controls) {
        prepared.resources.actor_setup[index].control = Some(Arc::new(definition));
    }
    prepared.resources.actor_setup[0].techniques = vec![
        casting_technique(CAST, 66),
        casting_technique(ALTERNATE, 65),
    ];
    prepared
}

fn held(actor: ActorId, technique: bool, delay_spell: bool) -> BattleInput {
    let mut control = crate::ControlInput::neutral(actor);
    control.technique.held = technique;
    control.delay_spell.held = delay_spell;
    BattleInput {
        controllers: vec![control],
        ..Default::default()
    }
}

#[test]
fn held_spell_charges_to_a_cap_and_pays_once_on_release() {
    let mut battle = controlled(Control::Manual, 2);
    battle.step(request()).unwrap();
    let tp = battle.actors[0].tp;
    for _ in 0..360 {
        battle.step(held(ActorId(0), true, false)).unwrap();
    }
    assert!(matches!(
        battle.activity(ActorId(0)),
        crate::Activity::Casting { held: true }
    ));
    assert_eq!(battle.actors[0].attack_power, 150);
    assert_eq!(battle.actors[0].tp, tp);
    let released = battle.step(BattleInput::default()).unwrap();
    assert_eq!(released.actors[0].tp, tp - 7);
    assert_eq!(
        released
            .cues
            .iter()
            .filter(|cue| matches!(cue, Cue::Released { .. }))
            .count(),
        1
    );
    assert_eq!(released.actors[0].activity, crate::Activity::Recovering);
    let next = battle.step(BattleInput::default()).unwrap();
    assert_eq!(next.actors[0].tp, tp - 7);
    assert!(
        !next
            .cues
            .iter()
            .any(|cue| matches!(cue, Cue::Released { .. }))
    );
}

#[test]
fn auto_command_uses_issuing_slot_and_clears_only_at_admitted_release() {
    let mut battle = controlled(Control::Auto, 2);
    assert!(
        battle
            .queue_technique_target_from(ActorId(0), CAST, ActorId(1), ActorId(2))
            .unwrap()
    );
    assert_eq!(battle.pending_technique_issuer(ActorId(0)), Some(3));
    battle.step(request()).unwrap();
    let tp = battle.actors[0].tp;
    // The issuer comes after the caster in actor traversal. The input snapshot
    // must already exist when the earlier caster updates.
    for _ in 0..5 {
        battle.step(held(ActorId(2), false, true)).unwrap();
        assert_eq!(battle.actors[0].tp, tp);
        assert_eq!(battle.pending_technique(ActorId(0)), Some(CAST));
    }
    assert!(matches!(
        battle.activity(ActorId(0)),
        crate::Activity::Casting { held: true }
    ));
    let released = battle.step(held(ActorId(0), false, true)).unwrap();
    assert_eq!(
        released.actors[0].tp,
        tp - 7,
        "recipient's button cannot hold issuer3"
    );
    assert_eq!(battle.pending_technique(ActorId(0)), None);
    assert_eq!(battle.pending_technique_issuer(ActorId(0)), None);
}
