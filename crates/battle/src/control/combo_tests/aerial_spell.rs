use super::*;

fn prepared_fixture(mode: Control, enabled: bool) -> PreparedBattle {
    let mut prepared = shared_prepared(mode);

    let cast = &mut prepared.resources.actions.entries[8];
    let cast = Arc::make_mut(cast);
    cast.tp_cost = 7;
    let command = prepared.resources.actor_setup[0]
        .techniques
        .iter_mut()
        .find(|row| row.action == crate::ActionKey(8))
        .unwrap();
    command.capabilities = crate::TechniqueCapabilities {
        family: Some(crate::ArteFamily::Basic),
        spell: true,
        offensive: true,
        target: crate::TechniqueTarget::Enemy,
        ..Default::default()
    };
    command.catalogue = 213;
    Arc::make_mut(prepared.resources.actor_setup[0].control.as_mut().unwrap()).shortcuts[1] = 213;
    prepared = prepared
        .with_technique_learning_members(vec![crate::tests::counted_techniques(
            ActorId(0),
            &[1, 213, 34, 4],
            &[(1, 49), (213, 49), (34, 49)],
        )])
        .unwrap();
    let cast = &mut prepared.resources.actions.entries[8];
    let cast = Arc::make_mut(cast);
    cast.execution = crate::ActionExecution::Casting(Arc::new(crate::CastingDefinition {
        duration: 0,

        recovery: 0,
        release: Arc::new(crate::tests::volley()),
        threat: None,
    }));
    prepared.resources.actor_setup[0].spell_charge = Some(crate::SpellChargeDefinition {
        automatic: true,
        enabled: true,
    });
    prepared.resources.actor_setup[0].aerial_spells = enabled;
    prepared.resources.actor_setup[0].arte_chain_limit = Some(3);
    prepared
}

fn fixture(mode: Control, enabled: bool) -> Battle {
    prepared_fixture(mode, enabled).finish().unwrap()
}

fn aerial_normal(battle: &mut Battle) -> Result<ActionId> {
    battle.actors[0].position[1] = 200.;
    battle.actors[0].movement.vertical = 0.;
    battle.actors[0].movement.gravity = 0.;
    battle.start_actor_command(
        crate::ActionRequest {
            actor: ActorId(0),
            action: crate::ActionKey(5),
            target: ActorId(1),
        },
        &mut vec![],
    )?;
    let id = actor_sequence(battle).0;
    until_age(battle, 1)?;
    Ok(id)
}

fn queue_spell(battle: &mut Battle) -> Result<()> {
    battle.step(player_buttons(false, true, false, [0, 80]))?;

    until_age(battle, 15)
}

#[test]
fn secondary_release_keeps_aerial_parent_and_counts_without_learning() -> Result<()> {
    for mode in [Control::Manual, Control::SemiAuto] {
        let mut battle = fixture(mode, true);
        let parent = aerial_normal(&mut battle)?;
        queue_spell(&mut battle)?;
        let before_tp = battle.actors[0].tp;

        battle.actors[0].attack_power = 70;
        battle.actors[0].stored_spell = Some(crate::ActionKey(8));
        let frame = battle.step(BattleInput::default())?;
        assert_eq!(battle.technique_uses(ActorId(0), 213), Some(50));
        assert_eq!(battle.actors[0].tp, before_tp - 7);
        assert_eq!(battle.actors[0].attack_power, 100);
        assert_eq!(battle.actors[0].stored_spell, Some(crate::ActionKey(8)));
        assert_eq!(actor_sequence(&battle).0, parent);

        assert!(frame.cues.iter().any(|cue| matches!(cue,
            Cue::Released { parent: Some(owner), slot: crate::SpellSlot::Secondary, .. }
                if *owner == parent)));
        assert!(!frame.cues.iter().any(|cue| matches!(cue,
            Cue::Completed { action } if *action == parent)));
        assert!(!battle.spell_active(ActorId(0), crate::SpellSlot::Primary));
        assert!(battle.spell_active(ActorId(0), crate::SpellSlot::Secondary));
    }
    Ok(())
}

#[test]
fn secondary_busy_rejects_without_payment_while_primary_remains_independent() -> Result<()> {
    for slot in [crate::SpellSlot::Primary, crate::SpellSlot::Secondary] {
        let mut battle = fixture(Control::Manual, true);
        let parent = aerial_normal(&mut battle)?;
        queue_spell(&mut battle)?;
        battle.release_volley(
            Arc::new(crate::tests::volley()),
            ActorId(0),
            ActorId(1),
            slot,
            Some(parent),
            &mut vec![],
        )?;
        let before_tp = battle.actors[0].tp;
        let frame = battle.step(BattleInput::default())?;
        let released = slot == crate::SpellSlot::Primary;
        assert_eq!(
            battle.actors[0].tp,
            before_tp - if released { 7 } else { 0 }
        );
        assert_eq!(
            battle.technique_uses(ActorId(0), 213),
            Some(if released { 50 } else { 49 })
        );
        assert_eq!(actor_sequence(&battle).0, parent);
        assert_eq!(
            frame
                .cues
                .iter()
                .filter(|cue| matches!(
                    cue,
                    Cue::Released {
                        slot: crate::SpellSlot::Secondary,
                        ..
                    }
                ))
                .count(),
            usize::from(slot == crate::SpellSlot::Primary)
        );
    }
    Ok(())
}

#[test]
fn quote_rejection_disabled_ex_and_holds_preserve_count_payment_and_dispatch() -> Result<()> {
    for (enabled, tp) in [(false, 40), (true, 6)] {
        let mut battle = fixture(Control::Manual, enabled);
        aerial_normal(&mut battle)?;
        queue_spell(&mut battle)?;
        battle.actors[0].tp = tp;
        battle.step(BattleInput::default())?;
        assert_eq!(battle.actors[0].tp, tp);
        assert_eq!(battle.technique_uses(ActorId(0), 213), Some(49));
        assert!(!battle.spell_active(ActorId(0), crate::SpellSlot::Secondary));
    }
    let mut battle = fixture(Control::Manual, true);
    aerial_normal(&mut battle)?;
    queue_spell(&mut battle)?;
    battle.actors[0].hit_stop = 3;
    let before_tp = battle.actors[0].tp;
    battle.step(BattleInput::default())?;
    assert_eq!(battle.actors[0].tp, before_tp);
    assert_eq!(battle.technique_uses(ActorId(0), 213), Some(49));
    assert!(!battle.spell_active(ActorId(0), crate::SpellSlot::Secondary));
    Ok(())
}

#[test]
fn preparation_rejects_wrong_identity_and_ground_or_auto_has_no_aerial_prefix() -> Result<()> {
    let mut prepared = prepared_fixture(Control::Manual, false);
    prepared.resources.actor_setup[0]
        .techniques
        .iter_mut()
        .find(|row| row.action == crate::ActionKey(8))
        .unwrap()
        .capabilities
        .spell = false;
    prepared.resources.actor_setup[0].aerial_spells = true;
    assert!(prepared.finish().is_err());
    for (mode, height) in [(Control::Manual, 0.1), (Control::Auto, 200.)] {
        let mut battle = fixture(mode, true);
        battle.actors[0].position[1] = height;
        let mut cues = vec![];
        battle.try_aerial_spell_selection(ActorId(0), crate::ActionKey(8), None, &mut cues)?;
        assert_eq!(battle.technique_uses(ActorId(0), 213), Some(49));
        assert!(cues.is_empty());
    }
    Ok(())
}

#[test]
fn menu_holds_release_and_interrupting_the_parent_preserves_its_resident() -> Result<()> {
    let mut battle = fixture(Control::Manual, true);
    let parent = aerial_normal(&mut battle)?;
    queue_spell(&mut battle)?;
    let before_tp = battle.actors[0].tp;
    let before_age = actor_sequence(&battle).1.age;
    battle.step(BattleInput {
        paused: true,
        ..Default::default()
    })?;
    assert_eq!(battle.actors[0].tp, before_tp);
    assert_eq!(battle.technique_uses(ActorId(0), 213), Some(49));
    assert_eq!(actor_sequence(&battle).1.age, before_age);
    assert!(!battle.spell_active(ActorId(0), crate::SpellSlot::Secondary));
    battle.step(BattleInput::default())?;
    assert!(battle.spell_active(ActorId(0), crate::SpellSlot::Secondary));
    battle.step(BattleInput {
        interrupt: vec![parent],
        ..Default::default()
    })?;
    assert!(battle.sequence(&parent).is_none());
    assert!(battle.spell_active(ActorId(0), crate::SpellSlot::Secondary));
    assert_eq!(battle.technique_uses(ActorId(0), 213), Some(50));
    Ok(())
}
