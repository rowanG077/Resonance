use super::*;
use crate::{
    ActionDefinition, ActionId, ActionRequest, Battle, BattleInput, Control, Cue, PreparedBattle,
    Side,
};

mod clocks;
mod delay;
mod facing;
mod guard_cancel;
mod items;
mod preparation;
mod spell_charge;
mod targets;

const CAST: crate::ActionKey = crate::ActionKey(0);
const ALTERNATE: crate::ActionKey = crate::ActionKey(1);
const NORMAL: crate::ActionKey = crate::ActionKey(2);
const LEARNED: crate::ActionKey = crate::ActionKey(9);

const CASTER_TP: u16 = 47;

fn battle(control: Control, duration: u16, tp: u16) -> Battle {
    prepared_cast(control, duration, tp).finish().unwrap()
}

fn native_cast(action: &mut ActionDefinition) -> &mut crate::CastingDefinition {
    let crate::ActionExecution::Casting(cast) = &mut action.execution else {
        panic!("expected casting action")
    };
    Arc::make_mut(cast)
}

fn casting_definition(prepared: &mut PreparedBattle) -> &mut crate::CastingDefinition {
    native_cast(Arc::make_mut(&mut prepared.resources.actions.entries[0]))
}

fn casting_technique(action: crate::ActionKey, catalogue: u16) -> crate::PreparedTechnique {
    crate::PreparedTechnique {
        player_range: [0., 1000.],
        ai_range: [0., 1000.],
        capabilities: crate::TechniqueCapabilities {
            target: crate::TechniqueTarget::Enemy,
            uses_weapon_reach: true,
            ..Default::default()
        },
        ..crate::tests::technique(action, catalogue)
    }
}

fn prepared_cast(control: Control, duration: u16, tp: u16) -> PreparedBattle {
    let casting = crate::CastingDefinition {
        duration: u32::from(duration),

        recovery: 90,
        release: Arc::new(crate::tests::volley()),
        threat: None,
    };
    let actions = vec![ActionDefinition {
        tp_cost: 7,
        normal: None,
        execution: crate::ActionExecution::Casting(Arc::new(casting)),
    }];
    let mut actor = crate::tests::actor(Side::Party);
    actor.control = control;
    actor.tp = tp;
    actor.equipment.max_tp = 100;
    let mut prepared = PreparedBattle::new(
        vec![
            (actor, Default::default()),
            (crate::tests::actor(Side::Enemy), Default::default()),
        ],
        actions.into(),
        1,
    )
    .unwrap();
    prepared.resources.actor_setup[0]
        .techniques
        .push(casting_technique(CAST, 66));
    prepared
}

fn request() -> BattleInput {
    BattleInput {
        actions: vec![ActionRequest {
            actor: ActorId(0),
            action: CAST,
            target: ActorId(1),
        }],
        ..Default::default()
    }
}

#[test]
fn unavailable_learning_candidates_do_not_interrupt_casting() -> anyhow::Result<()> {
    let mut table = resonance_content::arte::Catalogue {
        definitions: vec![Default::default(); 3],
        learning: vec![vec![1, 2]],
    };
    for row in &mut table.definitions[1..] {
        row.required_level = 1;
        row.capabilities.spell = true;
    }
    let catalogue = crate::learning::LearningCatalogue::new(Arc::new(table));
    let member = catalogue.prepare_member(crate::learning::LearningEntry {
        character: 1,
        level: 1,
        balance: 0,
        story_unlocked: true,
        current: [2].into(),
        counts: [(2, 49)].into(),
    })?;
    let attempt = crate::learning::LearningAttempt {
        mode: crate::learning::LearningMode::Casting,
        current: Some(2),
        airborne: false,
    };
    assert_eq!(member.select_after_count(attempt, |_| true, || 0)?, Some(1));
    let mut prepared = prepared_cast(Control::Manual, 1, CASTER_TP);
    prepared.resources.actor_setup[0].techniques[0].catalogue = 2;
    let mut battle = prepared
        .with_technique_learning_members(vec![crate::learning::TechniqueLearningMember {
            actor: ActorId(0),
            member,
        }])?
        .finish()?;
    let tp = battle.actors[0].tp;
    battle.step(request())?;
    let mut releases = 0;
    for _ in 0..180 {
        releases += battle
            .step(BattleInput::default())?
            .cues
            .iter()
            .filter(|cue| matches!(cue, Cue::Released { .. }))
            .count();
    }
    assert_eq!(releases, 1);
    assert_eq!(battle.actors[0].tp, tp - 7);
    assert!(!battle.is_diagnostic());
    assert!(battle.technique_acquisitions().is_empty());
    assert_eq!(battle.current_techniques(ActorId(0)), Some(&[2].into()));
    assert_eq!(battle.technique_counts(ActorId(0)), Some(&[(2, 50)].into()));
    Ok(())
}

#[test]
fn casting_advances_through_local_hit_stop_but_menu_holds_the_cast() {
    let mut battle = battle(Control::Auto, 20, CASTER_TP);
    battle.actors[0].hit_stop = 20;
    battle.step(request()).unwrap();
    let initial = cast_remaining(&battle);
    for _ in 0..2 {
        battle.step(BattleInput::default()).unwrap();
    }
    assert!(cast_remaining(&battle) < initial);
    assert!(battle.actors[0].hit_stop > 0);
    let remaining = cast_remaining(&battle);
    for _ in 0..5 {
        let frame = battle
            .step(BattleInput {
                paused: true,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(cast_remaining(&battle), remaining);
        assert_eq!(frame.actors[0].tp, 47);
    }
    let mut releases = 0;
    for _ in 0..220 {
        let frame = battle.step(BattleInput::default()).unwrap();
        releases += frame
            .cues
            .iter()
            .filter(|cue| matches!(cue, Cue::Released { .. }))
            .count();
    }
    assert_eq!(releases, 1);
    assert_eq!(battle.actors[0].tp, 40);
}

fn start_cast(battle: &mut Battle) -> ActionId {
    let mut cues = Vec::new();
    battle
        .start(
            ActionRequest {
                actor: ActorId(0),
                action: CAST,
                target: ActorId(1),
            },
            &mut cues,
        )
        .expect("casting admission");
    match cues.as_slice() {
        [Cue::Started { action, .. }] => *action,
        _ => panic!("casting fixture rejected: {cues:?}"),
    }
}

fn cast_remaining(battle: &Battle) -> u32 {
    battle.casting_remaining(ActorId(0)).expect("live cast")
}

#[test]
fn release_survives_forgetting_then_interruption_and_counts_and_pays_once() -> anyhow::Result<()> {
    {
        let prepared = delay::controlled_prepared(Control::Manual, 2, CASTER_TP);

        let mut battle = prepared
            .with_technique_learning_members(vec![crate::tests::counted_techniques(
                ActorId(0),
                &[66],
                &[(66, 49)],
            )])?
            .finish()?;
        let mut frame = battle.step(request())?;
        assert!(matches!(
            battle.activity(ActorId(0)),
            crate::Activity::Casting { .. }
        ));
        battle.forget_technique(ActorId(0), CAST)?;
        assert_eq!(battle.technique_is_current(ActorId(0), 66), Some(false));
        let mut releases = 0;
        for _ in 0..200 {
            releases += frame
                .cues
                .iter()
                .filter(|cue| matches!(cue, Cue::Released { .. }))
                .count();
            if releases > 0 {
                break;
            }
            frame = battle.step(BattleInput::default())?;
        }
        assert_eq!(releases, 1);
        assert_eq!(battle.actors[0].tp, 40);
        assert_eq!(battle.technique_uses(ActorId(0), 66), Some(50));
        battle.step(BattleInput {
            interrupt: vec![ActionId(1)],
            ..Default::default()
        })?;
        assert!(battle.spell_active(ActorId(0), crate::SpellSlot::Primary));
        battle.actors[0].equipment.casting.reducer = true;
        assert_eq!(
            battle.action_quote(ActorId(0), CAST),
            5,
            "a released spell keeps its repeat discount after caster interruption"
        );
        for _ in 0..120 {
            let frame = battle.step(BattleInput::default())?;
            releases += frame
                .cues
                .iter()
                .filter(|cue| matches!(cue, Cue::Released { .. }))
                .count();
        }
        assert_eq!(releases, 1);
        assert_eq!(battle.actors[0].tp, 40);
        assert_eq!(battle.technique_uses(ActorId(0), 66), Some(50));
        assert!(!battle.spell_active(ActorId(0), crate::SpellSlot::Primary));
    }
    Ok(())
}

#[test]
fn interruption_before_release_does_not_pay_or_record_a_use() -> anyhow::Result<()> {
    let prepared = prepared_cast(Control::Manual, 50, CASTER_TP);
    let mut battle = prepared
        .with_technique_learning_members(vec![crate::tests::counted_techniques(
            ActorId(0),
            &[66],
            &[(66, 49)],
        )])?
        .finish()?;
    battle.step(request())?;
    for _ in 0..5 {
        battle.step(BattleInput::default())?;
    }
    battle.step(BattleInput {
        interrupt: vec![ActionId(1)],
        ..Default::default()
    })?;
    for _ in 0..100 {
        let frame = battle.step(BattleInput::default())?;
        assert!(
            !frame
                .cues
                .iter()
                .any(|cue| matches!(cue, Cue::Released { .. }))
        );
    }
    assert_eq!(battle.actors[0].tp, 47);
    assert_eq!(battle.technique_uses(ActorId(0), 66), Some(49));
    Ok(())
}

#[test]
fn recovery_pauses_with_the_menu_and_expires() -> anyhow::Result<()> {
    let mut battle = battle(Control::Auto, 5, CASTER_TP);
    battle.step(request())?;
    for _ in 0..100 {
        battle.step(BattleInput::default())?;
        if battle.action_recovery_remaining(ActionId(1)).is_some() {
            break;
        }
    }
    let remaining = battle
        .action_recovery_remaining(ActionId(1))
        .expect("cast entered recovery");
    for _ in 0..8 {
        battle.step(BattleInput {
            paused: true,
            ..Default::default()
        })?;
        assert_eq!(
            battle.action_recovery_remaining(ActionId(1)),
            Some(remaining)
        );
    }
    for _ in 0..=remaining {
        battle.step(BattleInput::default())?;
    }
    assert_eq!(battle.action_recovery_remaining(ActionId(1)), None);
    Ok(())
}

#[test]
fn unaffordable_cast_is_rejected_at_admission() {
    let mut battle = battle(Control::Manual, 1, 6);
    let frame = battle.step(request()).unwrap();
    assert!(frame.cues.contains(&Cue::Rejected {
        actor: ActorId(0),
        reason: crate::Rejection::InsufficientTp
    }));
    assert!(frame.actions.is_empty());
    assert_eq!(battle.actors[0].tp, 6);
}

#[test]
fn spell_admission_and_payment_share_repeat_and_physical_modifiers() {
    for (reducer, previous, cost) in [
        (false, Some(CAST), 7),
        (true, Some(ALTERNATE), 7),
        (true, Some(CAST), 5),
    ] {
        let mut prepared = prepared_cast(Control::Manual, 2, cost);
        prepared.actors[0].equipment.casting.reducer = reducer;
        prepared.actors[0].casting_state.previous_spell = previous;
        prepared.actors[0].equipment.damage.physical_arte_boost = true;
        let mut battle = prepared.finish().unwrap();
        let random = battle.random.state();
        assert_eq!(battle.action_quote(ActorId(0), CAST), u32::from(cost));
        assert_eq!(battle.random.state(), random);
        let started = battle.step(request()).unwrap();
        assert!(
            started
                .cues
                .iter()
                .any(|cue| matches!(cue, Cue::Started { .. }))
        );
        let mut releases = 0;
        for _ in 0..140 {
            let frame = battle.step(BattleInput::default()).unwrap();
            releases += frame
                .cues
                .iter()
                .filter(|cue| matches!(cue, Cue::Released { .. }))
                .count();
        }
        assert_eq!(releases, 1);
        assert_eq!(battle.actors[0].tp, 0);
    }
}
