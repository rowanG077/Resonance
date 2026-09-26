use crate::{
    ActionDefinition, ActionPhase, ActionRequest, ActorId, Battle, BattleInput, Cue, EffectBank,
    ResourceBinding, Side, SoundBinding, native_declarations,
    tests::{actor, prepared},
};
use std::{collections::BTreeMap, sync::Arc};
use symphonia_script_compiler::compile;

fn battle() -> Battle {
    let mut prepared = prepared(
        r#"asset startup: battle::Effect = "test/effect";
        pub task run() {
            battle::attach_effect(startup, 1);
            await battle::at_age(ticks(30));
        }"#,
        vec![actor(Side::Party), actor(Side::Enemy)],
        30,
    );
    let p = Arc::get_mut(&mut prepared).unwrap();
    p.actions[0].phase = ActionPhase::Actor;
    p.actions[0].tp_cost = 0;
    let compiled = compile(
        "attached",
        &BTreeMap::from([(
            "attached".into(),
            r#"script battle; use battle;
            asset tone: battle::Sound = "test/sound";
            pub task run() {
                battle::sound(tone, 0);
                await battle::at_age(ticks(2));
                battle::sound(tone, 0);
                await battle::at_age(ticks(8));
                battle::sound(tone, 0);
            }"#
            .into(),
        )]),
        &native_declarations(),
    )
    .unwrap();
    let entry = compiled.program.authored().unwrap().functions[0].entry;
    p.effects.insert(
        37,
        EffectBank {
            resource: 37,
            models: Default::default(),
            members: BTreeMap::from([(
                1,
                Arc::new(ActionDefinition {
                    id: 1,
                    phase: ActionPhase::Effect,
                    program: Arc::new(compiled.program),
                    entry,
                    duration: 9,
                    tp_cost: 0,
                    resources: vec![ResourceBinding::Sound(SoundBinding {
                        resource: 1,
                        index: 7,
                    })],
                }),
            )]),
        },
    );
    Battle::new(prepared)
}

#[test]
fn embedded_effect_starts_next_callback_follows_owner_pauses_and_cancels_with_action() {
    let mut battle = battle();
    let first = battle
        .step(BattleInput {
            actions: vec![ActionRequest {
                actor: ActorId(0),
                action: 99,
                target: ActorId(1),
            }],
            ..Default::default()
        })
        .unwrap();
    assert!(
        first
            .cues
            .iter()
            .all(|cue| !matches!(cue, Cue::Sound { .. }))
    );
    let action = first.actions[0].0;
    battle.actors[0].position = [11., 0., 23.];
    let sounds = |frame: &crate::BattleFrame| {
        frame
            .cues
            .iter()
            .filter_map(|cue| {
                if let Cue::Sound { position, .. } = cue {
                    Some(*position)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        sounds(&battle.step(BattleInput::default()).unwrap()),
        [[11., 0., 23.]]
    );
    battle.actors[0].hit_stop = 2;
    for _ in 0..3 {
        assert!(sounds(&battle.step(BattleInput::default()).unwrap()).is_empty());
    }
    battle.actors[0].position = [33., 0., 44.];
    assert_eq!(
        sounds(&battle.step(BattleInput::default()).unwrap()),
        [[33., 0., 44.]]
    );
    battle
        .step(BattleInput {
            interrupt: vec![action],
            ..Default::default()
        })
        .unwrap();
    for _ in 0..12 {
        assert!(sounds(&battle.step(BattleInput::default()).unwrap()).is_empty());
    }
}
