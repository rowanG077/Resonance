use super::*;
use crate::{
    ActionDefinition, ActionId, ActionPhase, ActionRequest, Battle, BattleInput, Control, Cue,
    PreparedBattle, ResourceBinding, Side,
};
use resonance_content::animation::{Bone, Transform, TransformChannels};
use symphonia_script_compiler::compile;

fn fixture() -> serde_json::Value {
    serde_json::from_str(include_str!("../../tests/fixtures/opening-casting.json")).unwrap()
}

#[derive(serde::Deserialize)]
struct Parameters {
    recovery: u16,
    tp: u16,
    rate: f32,
    release_blend: u8,
    release_loop_start: f32,
    release_repeat: bool,
    chant: Vec<Chant>,
}

#[derive(serde::Deserialize)]
struct Chant {
    age: u16,
    clip: u16,
    blend: u8,
    frame: f32,
    rate: f32,
    repeat: bool,
}

fn battle(
    control: Control,
    duration: u16,
    before: &serde_json::Value,
    root: Option<&str>,
) -> Battle {
    let fixture = fixture();
    let parameters: Parameters = serde_json::from_value(fixture["parameters"].clone())
        .expect("casting observation parameters");
    let default_root = r#"
        await casting::prepared(definition, spell, common, release_sound, casting::Voices { chant: silent_voice, fallback: silent_voice, release: silent_voice });
        battle::finish();
    "#;
    let source = format!(
        r#"
        script battle;
        use battle;
        use battle::casting;
        asset definition: battle::Casting = "test/casting";
        asset release_sound: battle::Sound = "test/sound";
        asset silent_voice: battle::Voice = "test/silent_voice";
        asset spell: battle::Spell = "test/spell";
        asset common: battle::Effect = "test/common";
        asset release_pose: battle::Motion = "test/12";
        asset chant0: battle::Motion = "test/30";
        asset chant1: battle::Motion = "test/31";
        asset chant2: battle::Motion = "test/32";
        pub task run() {{ {} }}
        pub task resident() {{ await battle::at_age(ticks(20)); battle::heal_percent(battle::owner(), 10); }}
    "#,
        root.unwrap_or(default_root)
    );
    let compilation = compile(
        "test",
        &BTreeMap::from([
            ("test".into(), source),
            (
                "battle::casting".into(),
                include_str!("../../../../scripts/battle/casting.sym").into(),
            ),
        ]),
        &crate::native_declarations(),
    )
    .unwrap();
    let resource_index = |path: &str| {
        compilation
            .assets
            .iter()
            .find(|asset| asset.path == path)
            .unwrap()
            .index as usize
    };
    let casting = crate::CastingDefinition {
        base: duration as i16,
        extra: 0,
        recovery: parameters.recovery,
        tp_cost: parameters.tp,
        release: crate::CastMotion {
            age: 0,
            motion: resource_index("test/12"),
            blend: parameters.release_blend,
            frame: 0.,
            rate: parameters.rate,
            repeat: parameters.release_repeat,
            loop_start: parameters.release_loop_start,
        },
        chant: parameters
            .chant
            .iter()
            .map(|step| crate::CastMotion {
                age: step.age,
                motion: resource_index(&format!("test/{}", step.clip)),
                blend: step.blend,
                frame: step.frame,
                rate: step.rate,
                repeat: step.repeat,
                loop_start: step.frame,
            })
            .collect(),
        pulse_member: 3,
        effect_scale: 0.9,
        tint: Default::default(),
    };
    let assets = compilation
        .assets
        .iter()
        .map(|asset| {
            if asset.path == "test/casting" {
                ResourceBinding::Casting(Arc::new(casting.clone()))
            } else if asset.path == "test/sound" {
                ResourceBinding::Sound(crate::SoundBinding {
                    resource: 7,
                    index: 123,
                })
            } else if asset.path == "test/silent_voice" {
                ResourceBinding::Voice(vec![None; 2])
            } else if asset.path == "test/spell" {
                ResourceBinding::Spell(100)
            } else if asset.path == "test/common" {
                ResourceBinding::Effect(77)
            } else {
                ResourceBinding::Motion(MotionBinding {
                    model: 7,
                    clip: asset.path[5..].parse().unwrap(),
                })
            }
        })
        .collect::<Vec<_>>();
    let program = Arc::new(compilation.program);
    let entry = |name: &str| {
        program
            .authored()
            .unwrap()
            .functions
            .iter()
            .find(|f| f.name == name)
            .unwrap()
            .entry
    };
    let actions = vec![
        ActionDefinition {
            id: 99,
            phase: ActionPhase::Casting,
            program: program.clone(),
            entry: entry("test::run"),
            duration: 1000,
            tp_cost: parameters.tp,
            resources: assets.clone(),
        },
        ActionDefinition {
            id: 100,
            phase: ActionPhase::Resident,
            program: program.clone(),
            entry: entry("test::resident"),
            duration: 90,
            tp_cost: 0,
            resources: vec![],
        },
    ];
    let mut actor = crate::tests::actor(Side::Party);
    actor.control = control;
    actor.tp = before["tp"].as_u64().unwrap() as u16;
    actor.max_tp = 100;
    let model = ModelDefinition {
        secondary_motion: vec![],
        hurt_motions: [None; 2],
        idle_motions: [None; 2],
        guard_motions: [None; 2],
        stun: None,
        knockdown: None,
        resource: 7,
        skeleton: Skeleton {
            bones: vec![Bone {
                name: "root".into(),
                parent: None,
                bind: Transform::default(),
                bind_channels: TransformChannels(8),
            }],
        },
        motions: [
            (0, 40.),
            (12, 15.),
            (18, 8.),
            (30, 26.),
            (31, 14.),
            (32, 18.),
        ]
        .into_iter()
        .map(|(clip, duration_frames)| {
            (
                clip,
                Motion {
                    duration_frames,
                    tracks: vec![],
                },
            )
        })
        .collect(),
        initial: Playback {
            clip: before["clip"].as_u64().unwrap() as u16,
            frame: 0.,
            rate: 0.5,
            repeat: true,
        },
        anchors: vec![],
        approach_bones: vec![],
        target_bones: vec![],
        shadow: None,
        target_marker: None,
        weapons: vec![],
        hurt_bones: vec![],
        suppress_root_translation: [false; 3],
    };
    Battle::new(Arc::new(
        PreparedBattle::new(
            vec![actor, crate::tests::actor(Side::Enemy)],
            actions,
            1,
            vec![Some(Arc::new(model)), None],
            // These tests isolate casting clocks; game preparation tests bind
            // and execute the actual original particle programs.
            vec![crate::tests::effect_binding(77, [3, 7])],
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

#[test]
fn authored_casting_matches_dolphin_motion_tp_and_release_boundaries() {
    let fixture = fixture();
    let emissions: serde_json::Value = serde_json::from_str(include_str!(
        "../../../game/tests/fixtures/particle-emissions.json"
    ))
    .unwrap();
    let pulse_ticks: Vec<_> = emissions["constructors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["combat_tick"].as_u64().unwrap())
        .collect();
    let mut active = None;
    let mut start = 0;
    for row in fixture["observations"].as_array().unwrap() {
        let initializing = row["function"] == "initialize";
        if initializing {
            let duration = fixture["parameters"]["base"].as_u64().unwrap()
                + fixture["parameters"]["extra"].as_u64().unwrap();
            active = Some(battle(Control::Auto, duration as u16, &row["before"], None));
            start = row["combat_tick"].as_u64().unwrap();
        }
        let battle = active.as_mut().unwrap();
        let frame = battle
            .step(if initializing {
                request()
            } else {
                BattleInput::default()
            })
            .unwrap();
        assert_eq!(
            frame.update,
            row["combat_tick"].as_u64().unwrap() - start + 1
        );
        let tick = row["combat_tick"].as_u64().unwrap();
        if (pulse_ticks[0]..=*pulse_ticks.last().unwrap()).contains(&tick) {
            assert_eq!(
                frame
                    .cues
                    .iter()
                    .any(|cue| matches!(cue, Cue::Effect { member: 3, .. })),
                pulse_ticks.contains(&tick),
                "casting pulse at tick {tick}"
            );
        }
        let after = &row["after"];
        // Ordinary 3898C keeps 1B0=12 through the paid release pose; 385A0
        // writes 22 when entering the observed recovery callback 8.
        assert_eq!(frame.actors[0].hud.cast_released, after["activity"] == 8);
        assert_eq!(
            frame
                .cues
                .iter()
                .filter(|cue| matches!(cue, Cue::Notice { .. }))
                .cloned()
                .collect::<Vec<_>>(),
            if row["before"]["tp"] != after["tp"] {
                vec![Cue::Notice {
                    actor: ActorId(0),
                    action: 99,
                    duration: 90,
                    kind: 1,
                }]
            } else {
                vec![]
            },
            "notice must accompany source release payment, not admission: {row}"
        );
        let model = battle.models[0].as_ref().unwrap();
        assert_eq!(model.clip, after["clip"].as_u64().unwrap() as u16, "{row}");
        for (name, value) in [
            ("frame_bits", model.animation.clock.frame),
            ("start_bits", model.animation.clock.start),
            ("end_bits", model.animation.clock.end),
            ("loop_start_bits", model.animation.clock.loop_start),
            ("rate_bits", model.animation.clock.rate),
        ] {
            assert_eq!(
                value.to_bits(),
                after[name].as_u64().unwrap() as u32,
                "{name}: {row}"
            );
        }
        assert_eq!(
            model.animation.clock.finished,
            after["finished"].as_bool().unwrap(),
            "{row}"
        );
        assert_eq!(
            model.animation.clock.repeat,
            after["repeat"].as_bool().unwrap(),
            "{row}"
        );
        assert_eq!(
            model.animation.clock.stopped,
            after["stopped"].as_bool().unwrap(),
            "{row}"
        );
        if after["blend"].as_u64().unwrap() > 0 {
            assert_eq!(
                u64::from(model.animation.clock.blend),
                after["blend"].as_u64().unwrap(),
                "{row}"
            );
            assert_eq!(
                u64::from(model.animation.clock.blend_age),
                after["blend_age"].as_u64().unwrap(),
                "{row}"
            );
        } else {
            assert!(!model.blending(), "{row}");
        }
        assert_eq!(
            frame.actors[0].tp,
            after["tp"].as_u64().unwrap() as u16,
            "{row}"
        );
        if let crate::Activity::Casting { clock, .. } = frame.actors[0].activity {
            assert_eq!(
                i64::from(clock),
                after["remaining"].as_i64().unwrap(),
                "{row}"
            );
        } else {
            assert_eq!(
                frame.actors[0].activity,
                crate::Activity::Recovering,
                "{row}"
            );
            assert_eq!(after["activity"], 8, "{row}");
        }
        assert_eq!(
            frame.cues.iter().any(|c| matches!(c, Cue::Released { .. })),
            after["primary_mode"] == 1,
            "{row}"
        );
    }
}

#[test]
fn manual_release_pays_before_animation_completes_and_interrupts_without_refund() {
    for control in [Control::Manual, Control::SemiAuto] {
        let mut battle = battle(control, 2, &fixture()["observations"][0]["before"], None);
        battle.step(request()).unwrap();
        for _ in 0..2 {
            battle.step(BattleInput::default()).unwrap();
        }
        assert_eq!(battle.actors()[0].tp, 47);
        let paid = battle.step(BattleInput::default()).unwrap();
        assert_eq!(paid.actors[0].tp, 40);
        assert!(!paid.actors[0].hud.cast_released);
        assert!(paid.cues.contains(&Cue::Notice {
            actor: ActorId(0),
            action: 99,
            duration: 90,
            kind: 1
        }));
        assert_eq!(battle.models[0].as_ref().unwrap().clip, 12);
        assert!(!paid.cues.iter().any(|c| matches!(c, Cue::Released { .. })));
        let interrupted = battle
            .step(BattleInput {
                interrupt: vec![ActionId(1)],
                ..Default::default()
            })
            .unwrap();
        assert_eq!(interrupted.actors[0].tp, 40);
        assert!(!interrupted.actors[0].hud.cast_released);
        for _ in 0..60 {
            let frame = battle.step(BattleInput::default()).unwrap();
            assert!(frame.actions.is_empty());
            assert!(!frame.cues.iter().any(|c| matches!(c, Cue::Released { .. })));
            assert!(!frame.cues.iter().any(|c| matches!(c, Cue::Notice { .. })));
        }
    }
}

#[test]
fn invalid_casting_rows_fault_and_cancel_the_chant_task() -> anyhow::Result<()> {
    for index in [-1, 3, i32::MAX] {
        let root =
            format!("spawn casting::chant(definition); battle::chant_step(definition, {index});");
        let mut battle = battle(
            Control::Auto,
            140,
            &fixture()["observations"][0]["before"],
            Some(&root),
        );
        let error = battle.step(request()).expect_err("invalid row executed");
        assert!(error.to_string().contains("invalid casting motion row"));
        assert!(battle.sequences.is_empty());
        assert_eq!(battle.actors()[0].tp, 47);
        assert!(battle.step(BattleInput::default()).is_err());
    }
    Ok(())
}

#[test]
fn another_cast_restarts_the_chant_after_a_release_pose() -> anyhow::Result<()> {
    let mut before = fixture()["observations"][0]["before"].clone();
    before["clip"] = 12.into();
    let mut battle = battle(Control::Auto, 140, &before, None);
    battle.step(request())?;
    assert_eq!(battle.models[0].as_ref().expect("prepared model").clip, 30);
    Ok(())
}

#[test]
fn casting_updates_during_blends_and_local_hit_stop_but_menu_holds_everything() {
    let mut battle = battle(
        Control::Auto,
        2,
        &fixture()["observations"][0]["before"],
        None,
    );
    battle.actors[0].hit_stop = 20;
    battle.step(request()).unwrap();
    assert!(!battle.actors[0].body.jitter.active);
    for _ in 0..2 {
        battle.step(BattleInput::default()).unwrap();
        assert!(battle.actors[0].body.jitter.active);
        assert_eq!(battle.actors[0].body.jitter.remaining, 7);
    }
    let shown = battle.models[0].as_ref().unwrap().shown.clone();
    let jitter = battle.actors[0].body.jitter;
    let random = battle.random_state();
    for _ in 0..5 {
        let frame = battle
            .step(BattleInput {
                menu_open: true,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(frame.models[0], shown);
        assert_eq!(frame.actors[0].tp, 47);
        assert_eq!(frame.actors[0].hit_stop, 17);
        assert_eq!(battle.actors[0].body.jitter, jitter);
        assert_eq!(battle.random_state(), random);
    }
    let frame = battle.step(BattleInput::default()).unwrap();
    assert_eq!(frame.actors[0].tp, 40);
    assert_eq!(frame.actors[0].hit_stop, 16);
    assert!(battle.models[0].as_ref().unwrap().blending());
    // The chant callback re-arms even when it switches into release. Waiting
    // for the release animation lets that request expire; 385A0 re-arms at
    // recovery entry once the animation completes.
    assert_eq!(battle.actors[0].body.jitter.remaining, 7);
    for remaining in (0..=6).rev() {
        battle.step(BattleInput::default()).unwrap();
        assert_eq!(battle.actors[0].body.jitter.remaining, remaining);
        assert!(battle.actors[0].body.jitter.active);
    }
    battle.step(BattleInput::default()).unwrap();
    assert!(!battle.actors[0].body.jitter.active);
    assert_eq!(battle.actors[0].body.jitter.remaining, -1);
}

#[test]
fn release_rearms_jitter_for_recovery_and_keeps_the_source_c172_draw_pair() {
    let mut battle = battle(
        Control::Auto,
        140,
        &fixture()["observations"][0]["before"],
        None,
    );
    let mut released = false;
    for update in 0..180 {
        let frame = battle
            .step(if update == 0 {
                request()
            } else {
                BattleInput::default()
            })
            .unwrap();
        if frame
            .cues
            .iter()
            .any(|cue| matches!(cue, Cue::Released { .. }))
        {
            released = true;
            break;
        }
    }
    assert!(released);
    assert_eq!(battle.actors[0].activity, crate::Activity::Recovering);
    assert_eq!(battle.sequences[&ActionId(1)].recovery, Some(90));
    assert!(battle.actors[0].hud.cast_released);
    assert_eq!(battle.actors[0].body.jitter.remaining, 89);
    for remaining in (82..=88).rev() {
        battle.step(BattleInput::default()).unwrap();
        assert!(battle.actors[0].body.jitter.active);
        assert_eq!(battle.actors[0].body.jitter.remaining, remaining);
    }
    // Source04 C171 ends at draw576. C172's common actor callback consumes
    // draws577/578 and publishes these exact model600/604/608 acceleration bits.
    // This isolated fixture omits the later resident/particle random draws.
    battle.random = crate::state::Random(3_090_289_455);
    battle.step(BattleInput::default()).unwrap();
    assert_eq!(battle.random_state(), 1_722_905_673);
    assert!(battle.actors[0].body.jitter.active);
    assert_eq!(battle.actors[0].body.jitter.remaining, 81);
    assert_eq!(
        battle.actors[0]
            .body
            .jitter
            .take_acceleration()
            .map(f32::to_bits),
        [1_060_320_051, 1_045_220_557, 1_063_675_495]
    );
    let jitter = battle.actors[0].body.jitter;
    let random = battle.random_state();
    battle
        .step(BattleInput {
            menu_open: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(battle.actors[0].body.jitter, jitter);
    assert_eq!(battle.random_state(), random);
    battle.actors[0].hit_stop = 4;
    battle.step(BattleInput::default()).unwrap();
    assert!(battle.actors[0].body.jitter.active);
    assert_eq!(battle.actors[0].body.jitter.remaining, 80);
    assert!(battle.actors[0].hud.cast_released);
    for _ in 0..100 {
        if !battle.sequences.contains_key(&ActionId(1)) {
            break;
        }
        assert!(battle.snapshot().actors[0].hud.cast_released);
        battle.step(BattleInput::default()).unwrap();
    }
    assert!(!battle.sequences.contains_key(&ActionId(1)));
    assert!(!battle.actors[0].hud.cast_released);
}

#[test]
fn a_casting_child_waits_for_its_blend_without_holding_the_parent() {
    let root = r#"
        spawn child();
        await battle::at_age(ticks(2));
        battle::pay_tp(battle::tp_cost());
        await battle::at_age(ticks(8));
        battle::finish();
    } pub task child() {
        await battle::animate(chant0, ticks(4), 0.0, 0.5, false);
        battle::heal_percent(battle::owner(), 10);
    "#;
    let mut battle = battle(
        Control::Auto,
        2,
        &fixture()["observations"][0]["before"],
        Some(root),
    );
    for age in 0..5 {
        let frame = battle
            .step(if age == 0 {
                request()
            } else {
                BattleInput::default()
            })
            .unwrap();
        assert_eq!(frame.actors[0].tp, if age < 2 { 47 } else { 40 });
        assert_eq!(frame.actors[0].hp, if age < 4 { 50 } else { 60 });
    }
}

#[test]
fn occupied_primary_holds_countdown_but_early_pose_still_starts() {
    let mut battle = battle(
        Control::Auto,
        2,
        &fixture()["observations"][0]["before"],
        None,
    );
    // Casting has no fixed action lifetime; waiting for an old spell may be long.
    Arc::get_mut(&mut battle.prepared).unwrap().actions[0].duration = 1;
    let mut first = request();
    first.actions[0].action = 100;
    battle.step(first).unwrap();
    battle.step(request()).unwrap();
    for _ in 0..90 {
        let frame = battle.step(BattleInput::default()).unwrap();
        assert_eq!(frame.actors[0].tp, 47);
        assert_eq!(battle.models[0].as_ref().unwrap().clip, 12);
        assert_eq!(
            frame.actors[0].activity,
            crate::Activity::Casting {
                clock: 2,
                guard_window: [0, 0]
            }
        );
        assert!(!frame.cues.iter().any(|c| matches!(c, Cue::Released { .. })));
    }
    for _ in 0..2 {
        battle.step(BattleInput::default()).unwrap();
    }
    let paid = battle.step(BattleInput::default()).unwrap();
    assert_eq!(paid.actors[0].tp, 40);
    assert!(!paid.cues.iter().any(|c| matches!(c, Cue::Released { .. })));
    let released = battle.step(BattleInput::default()).unwrap();
    assert!(
        released
            .cues
            .iter()
            .any(|c| matches!(c, Cue::Released { .. }))
    );
}

#[test]
fn looping_release_keeps_the_early_pose_until_its_first_wrap() {
    let root = r#"
        battle::play_motion(chant0, ticks(8), 0.0, 0.5, true, 0.0);
        await casting::ordinary(spell, ticks(45), ticks(90), casting::ReleaseMotion {
            motion: release_pose, rate: 0.5, blend: ticks(4), loop_start: 15.0, repeat: true
        }, casting::Effects { effect: common, pulse_member: 3, release_member: 7, release_sound: release_sound, scale: 1.0, tint: battle::EffectTint { enabled: false, palette: 0, red: 0, green: 0, blue: 0 } }, casting::Voices { chant: silent_voice, fallback: silent_voice, release: silent_voice });
        battle::finish();
    "#;
    let mut battle = battle(
        Control::Enemy,
        45,
        &fixture()["observations"][0]["before"],
        Some(root),
    );
    Arc::make_mut(&mut battle.models[0].as_mut().unwrap().definition)
        .motions
        .get_mut(&12)
        .unwrap()
        .duration_frames = 38.;
    for age in 0..=78 {
        let frame = battle
            .step(if age == 0 {
                request()
            } else {
                BattleInput::default()
            })
            .unwrap();
        assert_eq!(frame.actors[0].tp, if age < 46 { 47 } else { 40 });
        assert_eq!(
            frame.cues.iter().any(|c| matches!(c, Cue::Released { .. })),
            age == 78
        );
        if age == 46 {
            assert_eq!(
                battle.models[0].as_ref().unwrap().animation.clock.frame,
                22.5
            );
        }
    }
    assert_eq!(
        battle.models[0].as_ref().unwrap().animation.clock.frame,
        15.5
    );
    let interrupted = battle
        .step(BattleInput {
            interrupt: vec![ActionId(1)],
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        interrupted
            .actions
            .iter()
            .map(|(_, owner, age)| (*owner, *age))
            .collect::<Vec<_>>(),
        [(ActorId(0), 1)]
    );
    for _ in 0..20 {
        battle.step(BattleInput::default()).unwrap();
    }
    assert_eq!(
        battle.actors()[0].hp,
        60,
        "released callback survives its caster"
    );
}
