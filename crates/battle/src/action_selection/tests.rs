use crate::*;
use std::sync::Arc;

const BASIC: ActionKey = ActionKey(7);
const SECOND_BASIC: ActionKey = ActionKey(8);
const ADVANCED: ActionKey = ActionKey(9);
const ARCANE: ActionKey = ActionKey(10);
const SPELL: ActionKey = ActionKey(11);

fn prepared_fixture() -> PreparedBattle {
    let actions: Vec<_> = (0..12)
        .map(|id| ActionDefinition {
            normal: NormalAttack::ALL.get(id).copied(),
            execution: crate::ActionExecution::Attack(crate::tests::attack(2)),
            tp_cost: if id >= BASIC.0 { 2 } else { 0 },
        })
        .collect();
    let mut actor = crate::tests::actor(Side::Party);
    actor.control = Control::Manual;
    actor.tp = 20;
    let mut enemy = crate::tests::actor(Side::Enemy);
    enemy.position[0] = 300.;
    let techniques = [BASIC, SECOND_BASIC, ADVANCED, ARCANE, SPELL]
        .into_iter()
        .map(|action| PreparedTechnique {
            player_range: [20., 120.],
            ai_range: [20., 120.],
            capabilities: TechniqueCapabilities {
                family: Some(match action {
                    BASIC | SECOND_BASIC | SPELL => ArteFamily::Basic,
                    ADVANCED => ArteFamily::Advanced,
                    _ => ArteFamily::Arcane,
                }),
                spell: action == SPELL,
                aerial: action == ARCANE,
                uses_weapon_reach: action != SPELL,
                target: crate::TechniqueTarget::Enemy,
                ..TechniqueCapabilities::default()
            },
            ..crate::tests::technique(action, action.0 as u16)
        })
        .collect();
    let mut setup = vec![ActorSetup::default(); 2];
    setup[0].techniques = techniques;
    setup[0].control = Some(Arc::new(ControlDefinition {
        normals: std::array::from_fn(|action| NormalControl {
            action: crate::ActionKey(action),

            reach: 120.,
            minimum_reach: 0.,
        }),
        shortcuts: [BASIC.0 as u16; 4],
        walk_speed: 5.,
        run_speed: 10.,
        turn_ticks: 8,
        motions: None,
    }));
    PreparedBattle::new(
        (vec![actor, enemy]).into_iter().zip(setup).collect(),
        actions.into(),
        17,
    )
    .unwrap()
}

fn fixture() -> Battle {
    prepared_fixture().finish().unwrap()
}

#[test]
fn previews_do_not_spend_or_reserve_a_combo_and_interruption_resets_it() -> anyhow::Result<()> {
    let mut battle = fixture();
    battle.actors[0].equipment.dagger_reach = true;
    for (action, range, expected) in [
        (BASIC, [20., 120.], [20., 80.]),
        (SPELL, [20., 120.], [20., 120.]),
        (crate::ActionKey(0), [0., 30.], [0., 25.]),
    ] {
        for _ in 0..2 {
            assert_eq!(
                battle
                    .action_candidate(ActorId(0), action, range)
                    .unwrap()
                    .range,
                expected
            );
        }
    }
    assert_eq!(battle.actors[0].tp, 20);
    let frame = battle.step(BattleInput {
        actions: vec![ActionRequest {
            actor: ActorId(0),
            target: ActorId(1),
            action: BASIC,
        }],
        ..Default::default()
    })?;
    let started = frame.actions[0].0;
    assert_eq!(battle.actors[0].tp, 18);
    assert_eq!(
        battle.action_candidate(ActorId(0), SECOND_BASIC, [0., 120.]),
        Err(Rejection::Unavailable)
    );
    battle.step(BattleInput {
        interrupt: vec![started],
        ..Default::default()
    })?;
    assert!(
        battle
            .action_candidate(ActorId(0), BASIC, [0., 120.])
            .is_ok()
    );
    Ok(())
}

#[test]
fn admission_distinguishes_airborne_ineligibility_from_insufficient_tp() -> anyhow::Result<()> {
    for (height, skill, action, tp, rejection) in [
        (0., false, ARCANE, 20, None),
        (30., false, ARCANE, 20, Some(Rejection::Unavailable)),
        (30., true, BASIC, 20, Some(Rejection::Unavailable)),
        (30., true, ARCANE, 20, None),
        (0., false, ARCANE, 1, Some(Rejection::InsufficientTp)),
    ] {
        let mut battle = fixture();
        battle.actors[0].position[1] = height;
        battle.actors[0].tp = tp;
        battle.actors[0].equipment.combo_traits.aerial_arte = skill;
        let frame = battle.step(BattleInput {
            actions: vec![ActionRequest {
                actor: ActorId(0),
                target: ActorId(1),
                action,
            }],
            ..Default::default()
        })?;
        assert_eq!(
            frame.cues.iter().find_map(|cue| match cue {
                Cue::Rejected { reason, .. } => Some(*reason),
                _ => None,
            }),
            rejection
        );
        assert_eq!(frame.actions.is_empty(), rejection.is_some());
        assert_eq!(
            battle.actors[0].tp,
            if rejection.is_some() { tp } else { tp - 2 }
        );
    }
    Ok(())
}
