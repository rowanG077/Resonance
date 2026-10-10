//! Stored-spell casting, interruption, and release.
use super::*;

pub(super) fn charged(mode: Control, duration: u16) -> Battle {
    charged_prepared(mode, duration).finish().unwrap()
}

pub(super) fn charged_prepared(mode: Control, duration: u16) -> PreparedBattle {
    let mut prepared = super::delay::controlled_prepared(mode, duration, CASTER_TP);
    prepared.actors[0].control = mode;

    let normal = &mut prepared.resources.actions.entries[2];
    *crate::tests::attack_mut(Arc::make_mut(normal)) = crate::PreparedAttack {
        recovery: 5,
        ..crate::tests::attack(0)
    };
    let cast = casting_definition(&mut prepared);
    cast.threat = Some(crate::CastingThreat {
        catalogue: 66,
        offensive: true,
        element: 0,
    });
    prepared.resources.actor_setup[0].spell_charge = Some(crate::SpellChargeDefinition {
        automatic: true,
        enabled: true,
    });
    prepared
        .with_technique_learning_members(vec![crate::tests::counted_techniques(
            ActorId(0),
            &[66],
            &[(66, 49)],
        )])
        .unwrap()
}

fn attack(actor: ActorId, pressed: bool) -> BattleInput {
    let mut input = crate::ControlInput::neutral(actor);
    input.attack.held = true;
    input.attack.pressed = pressed;
    input.technique.held = true;
    BattleInput {
        controllers: vec![input],
        ..Default::default()
    }
}
fn recover() -> BattleInput {
    BattleInput {
        actions: vec![ActionRequest {
            actor: ActorId(0),
            action: NORMAL,
            target: ActorId(1),
        }],
        ..Default::default()
    }
}
pub(super) fn enter_recovery(battle: &mut Battle) -> crate::BattleFrame {
    battle.step(BattleInput::default()).unwrap();
    let frame = battle.step(recover()).unwrap();
    assert_eq!(frame.actors[0].activity, crate::Activity::Recovering);
    frame
}

pub(super) fn store(battle: &mut Battle) -> crate::BattleFrame {
    battle.step(request()).unwrap();
    battle.step(attack(ActorId(0), false)).unwrap();
    let frame = battle.step(attack(ActorId(0), true)).unwrap();
    assert_eq!(battle.actors[0].stored_spell, Some(CAST));
    frame
}

#[test]
fn automatic_storage_requires_enabled_policy_and_an_offensive_spell() {
    for (automatic, offensive) in [(false, true), (true, false), (true, true)] {
        let mut prepared = charged_prepared(Control::Auto, 0);
        prepared.resources.actor_setup[0]
            .spell_charge
            .as_mut()
            .unwrap()
            .automatic = automatic;
        casting_definition(&mut prepared)
            .threat
            .as_mut()
            .unwrap()
            .offensive = offensive;
        prepared.technique_learning_members.clear();
        let mut battle = prepared.finish().unwrap();
        let mut expected = battle.random;
        for _ in 0..8 {
            let store = automatic && offensive && expected.next_u16() & 1 != 0;
            assert_eq!(
                battle.store_cast_requested(ActorId(0), CAST).unwrap(),
                store
            );
        }
    }
}

#[test]
fn storing_requires_a_fresh_own_attack_and_commits_payment_and_use_once() {
    for mode in [Control::Manual, Control::SemiAuto] {
        {
            let mut battle = charged(mode, 0);
            battle.step(request()).unwrap();
            battle.step(attack(ActorId(0), false)).unwrap();
            assert_eq!(battle.actors[0].stored_spell, None);
            let mut other = attack(ActorId(2), true);
            other
                .controllers
                .push(attack(ActorId(0), false).controllers[0]);
            battle.step(other).unwrap();
            assert_eq!(battle.actors[0].stored_spell, None);
            let tp = battle.actors[0].tp;
            let frame = battle.step(attack(ActorId(0), true)).unwrap();
            assert_eq!(frame.actors[0].stored_spell, Some(CAST));
            assert_eq!(battle.actors[0].stored_spell, Some(CAST));
            assert_eq!(battle.actors[0].tp, tp - 7);
            assert_eq!(battle.technique_uses(ActorId(0), 66), Some(50));

            assert!(
                !frame
                    .cues
                    .iter()
                    .any(|cue| matches!(cue, Cue::Released { .. }))
            );
        }
    }
}

fn learnable_charge() -> Battle {
    let mut prepared = charged_prepared(Control::Manual, 0);
    casting_definition(&mut prepared).recovery = 3;
    let mut learned = prepared.resources.actions.entries[0].as_ref().clone();

    learned.tp_cost = 9;
    let cast = native_cast(&mut learned);
    Arc::make_mut(&mut cast.release).shots = 2;
    prepared.resources.actions.entries.push(Arc::new(learned));
    prepared.resources.actor_setup[0]
        .techniques
        .push(casting_technique(LEARNED, 67));
    let mut data = resonance_content::arte::Catalogue {
        definitions: vec![Default::default(); 68],
        learning: vec![vec![66, 67]],
    };
    for definition in &mut data.definitions[66..] {
        definition.capabilities.spell = true;
        definition.required_level = 1;
        definition.action_range = 100.;
    }
    data.definitions[66].learning.technical_successor = Some(67);
    data.definitions[67].learning.parent = Some(66);
    data.definitions[67].learning.parent_uses = 50;
    let member = crate::learning::LearningCatalogue::new(Arc::new(data))
        .prepare_member(crate::learning::LearningEntry {
            character: 1,
            level: 1,
            balance: 0,
            story_unlocked: true,
            current: [66].into(),
            counts: [(66, 49)].into(),
        })
        .unwrap();
    prepared
        .with_technique_learning_members(vec![crate::learning::TechniqueLearningMember {
            actor: ActorId(0),
            member,
        }])
        .unwrap()
        .finish()
        .unwrap()
}

#[test]
fn learning_keeps_the_selected_spell_and_applies_to_the_next_command() {
    for stored in [false, true] {
        let mut battle = learnable_charge();
        battle.actors[0].tp = 7;
        battle.step(request()).unwrap();
        battle.step(attack(ActorId(0), false)).unwrap();
        battle.record_technique_acquisition(ActorId(0), 67).unwrap();
        let mut frame = battle
            .step(if stored {
                attack(ActorId(0), true)
            } else {
                BattleInput::default()
            })
            .unwrap();
        if stored {
            assert_eq!(battle.actors[0].stored_spell, Some(CAST));
            frame = enter_recovery(&mut battle);
            assert_eq!(battle.actors[0].stored_spell, None);
        }
        let released: Vec<_> = frame
            .cues
            .iter()
            .filter_map(|cue| match cue {
                Cue::Released { action, .. } => Some(battle.volleys[action].definition.shots),
                _ => None,
            })
            .collect();
        assert_eq!(released, [3]);
        assert_eq!(battle.actors[0].tp, 0);
        assert_eq!(battle.technique_uses(ActorId(0), 66), Some(50));
        assert_eq!(battle.technique_acquisitions().len(), 1);
        assert_eq!(battle.technique_acquisitions()[0].catalogue, 67);
        for _ in 0..32 {
            battle.step(BattleInput::default()).unwrap();
        }
        let next = || BattleInput {
            actions: vec![ActionRequest {
                actor: ActorId(0),
                action: LEARNED,
                target: ActorId(1),
            }],
            ..Default::default()
        };
        assert!(battle.step(next()).unwrap().cues.contains(&Cue::Rejected {
            actor: ActorId(0),
            reason: crate::Rejection::InsufficientTp,
        }));
        battle.actors[0].tp = 9;
        battle.step(next()).unwrap();
        let mut released = Vec::new();
        for _ in 0..6 {
            let frame = battle.step(BattleInput::default()).unwrap();
            released.extend(frame.cues.iter().filter_map(|cue| match cue {
                Cue::Released { action, .. } => Some(battle.volleys[action].definition.shots),
                _ => None,
            }));
        }
        assert_eq!(released, [2]);
        assert_eq!(battle.actors[0].tp, 0);
        assert_eq!(battle.technique_uses(ActorId(0), 67), Some(1));
        assert_eq!(battle.technique_acquisitions().len(), 1);
    }
}

#[test]
fn storing_rechecks_tp_before_counting_or_learning() {
    let mut battle = learnable_charge();
    battle.step(request()).unwrap();
    battle.step(attack(ActorId(0), false)).unwrap();
    battle.actors[0].tp = 6;
    battle.actors[0].equipment.casting.lucky_magic = true;
    battle.actors[0].equipment.luck = 999;
    let frame = battle.step(attack(ActorId(0), true)).unwrap();
    assert_eq!(battle.actors[0].stored_spell, None);
    assert_eq!(battle.actors[0].tp, 6);
    assert_eq!(battle.technique_uses(ActorId(0), 66), Some(49));
    assert!(battle.technique_acquisitions().is_empty());
    assert!(frame.cues.iter().any(|cue| matches!(
        cue,
        Cue::Rejected {
            actor: ActorId(0),
            reason: crate::Rejection::InsufficientTp
        }
    )));
}

#[test]
fn lucky_storage_waives_payment_but_counts_the_use_once() {
    let mut battle = charged(Control::Manual, 0);
    battle.actors[0].equipment.casting.lucky_magic = true;
    battle.actors[0].equipment.luck = 999;
    let tp = battle.actors[0].tp;
    store(&mut battle);
    assert_eq!(battle.actors[0].tp, tp);
    assert_eq!(battle.technique_uses(ActorId(0), 66), Some(50));
    enter_recovery(&mut battle);
    assert_eq!(battle.actors[0].stored_spell, None);
    assert_eq!(battle.actors[0].tp, tp);
    assert_eq!(battle.technique_uses(ActorId(0), 66), Some(50));
}

#[test]
fn stored_release_uses_current_target_without_another_payment() {
    let mut prepared = charged_prepared(Control::Manual, 0);
    prepared.actors[2].side = Side::Enemy;
    prepared.actors[2].control = Control::Enemy;
    prepared.resources.actor_setup[2] = crate::ActorSetup::default();
    let mut battle = prepared.finish().unwrap();
    store(&mut battle);
    battle.actors[0].tp = 0;
    battle.runtime[0].target = ActorId(2);
    let frame = enter_recovery(&mut battle);
    assert_eq!(battle.actors[0].stored_spell, None);
    assert_eq!(battle.actors[0].tp, 0);
    assert_eq!(battle.technique_uses(ActorId(0), 66), Some(50));
    assert!(frame.cues.iter().any(|cue| matches!(
        cue,
        Cue::Released {
            slot: crate::SpellSlot::Primary,
            ..
        }
    )));
    assert_eq!(battle.volleys.values().next().unwrap().target, ActorId(2));
}

#[test]
fn occupied_primary_preserves_stored_charge_and_live_action_state() {
    let mut battle = charged(Control::Manual, 0);
    store(&mut battle);
    battle
        .dispatch_spell(
            ActorId(0),
            CAST,
            crate::SpellSlot::Primary,
            None,
            &mut vec![],
        )
        .unwrap();
    battle.actors[0].attack_power = 147;
    let tp = battle.actors[0].tp;
    let uses = battle.technique_uses(ActorId(0), 66);
    let mut cues = vec![];
    battle
        .discharge_stored_spell(ActionId(900), ActorId(0), &mut cues)
        .unwrap();
    assert_eq!(battle.actors[0].stored_spell, Some(CAST));
    assert_eq!(battle.actors[0].attack_power, 147);
    assert_eq!(battle.actors[0].tp, tp);
    assert_eq!(battle.technique_uses(ActorId(0), 66), uses);
    assert!(cues.is_empty());
    battle.volleys.clear();
    battle
        .discharge_stored_spell(ActionId(900), ActorId(0), &mut cues)
        .unwrap();
    assert_eq!(battle.actors[0].stored_spell, None);
    assert_eq!(battle.actors[0].tp, tp);
    assert_eq!(battle.technique_uses(ActorId(0), 66), uses);
    assert_eq!(
        cues.iter()
            .filter(|cue| matches!(cue, Cue::Released { .. }))
            .count(),
        1
    );
}

#[test]
fn storage_survives_interruption_death_and_revival() {
    let mut battle = charged(Control::Manual, 0);
    store(&mut battle);
    battle.actors[0].hp = 0;
    battle.enter_death(ActorId(0), &mut vec![]);
    assert_eq!(battle.actors[0].stored_spell, Some(CAST));
    crate::tests::revive(&mut battle, ActorId(0), 50, &mut vec![]).unwrap();
    assert_eq!(battle.actors[0].stored_spell, Some(CAST));
    battle
        .discharge_stored_spell(ActionId(900), ActorId(0), &mut vec![])
        .unwrap();
    assert_eq!(battle.actors[0].stored_spell, None);
}

fn revenge() -> Battle {
    let mut prepared = charged_prepared(Control::Manual, 0);
    prepared.actors[0].equipment.spell_revenge = true;
    let technique = prepared.resources.actor_setup[0]
        .techniques
        .iter_mut()
        .find(|row| row.action == CAST)
        .unwrap();
    technique.capabilities = crate::TechniqueCapabilities {
        family: Some(crate::ArteFamily::Basic),
        spell: true,
        uses_weapon_reach: true,
        target: crate::TechniqueTarget::Ally,
        ..Default::default()
    };
    Arc::make_mut(prepared.resources.actor_setup[0].control.as_mut().unwrap()).shortcuts[0] = 66;
    let mut battle = prepared.finish().unwrap();
    battle.begin_hurt(ActorId(0), 15, &mut vec![]);
    battle.actors[0].reaction.recoil.kind = crate::RecoilKind::Down;
    battle
}

#[test]
fn counter_release_has_an_independent_slot_and_commits_only_after_admission() {
    let mut battle = revenge();
    battle
        .dispatch_spell(
            ActorId(0),
            CAST,
            crate::SpellSlot::Primary,
            None,
            &mut vec![],
        )
        .unwrap();
    let mut input = crate::ControlInput::neutral(ActorId(0));
    input.technique.pressed = true;
    let tp = battle.actors[0].tp;
    let mut cues = vec![];
    battle
        .try_spell_revenge(ActorId(0), &[input], &mut cues)
        .unwrap();
    assert!(battle.spell_active(ActorId(0), crate::SpellSlot::Primary));
    assert!(battle.spell_active(ActorId(0), crate::SpellSlot::Secondary));
    assert!(battle.actors[0].reaction.spell_revenge_used);
    assert_eq!(battle.actors[0].tp, tp);
    assert_eq!(battle.technique_uses(ActorId(0), 66), Some(49));
    battle.actors[0].reaction.spell_revenge_used = false;
    battle.actors[0].attack_power = 145;
    cues.clear();
    battle
        .try_spell_revenge(ActorId(0), &[input], &mut cues)
        .unwrap();
    assert!(!battle.actors[0].reaction.spell_revenge_used);
    assert_eq!(battle.actors[0].attack_power, 145);
    assert!(cues.is_empty());
    battle.interrupt_actor(ActorId(0), &mut cues);
    assert!(battle.spell_active(ActorId(0), crate::SpellSlot::Secondary));
}
