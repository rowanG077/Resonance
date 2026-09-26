use super::*;
use resonance_battle::{ActionPhase, Activity, Control, Cue, ModelDefinition, Playback};
use resonance_content::{
    animation::{Bone, Motion, Skeleton, Transform, TransformChannels},
    arte, battle_profile,
};

fn sources(files: &mut Files) {
    for (name, text) in [
        (
            "casting",
            include_str!("../../../../scripts/battle/casting.sym"),
        ),
        (
            "genis_lightning",
            include_str!("../../../../scripts/battle/genis_lightning.sym"),
        ),
    ] {
        files.bytes.insert(
            format!("scripts/battle/{name}.sym"),
            Arc::from(text.as_bytes()),
        );
    }
    files.bytes.insert(
        "scripts/resident.sym".into(),
        Arc::from(b"script battle; pub task run() {}".as_slice()),
    );
}

fn bindings(cost: u16) -> [ActionBinding; 2] {
    [
        ActionBinding {
            id: 1,
            phase: ActionPhase::Casting,
            module: "battle::genis_lightning".into(),
            entry: "run".into(),
            duration: 0,
            tp_cost: cost,
        },
        ActionBinding {
            id: 100,
            phase: ActionPhase::Resident,
            module: "resident".into(),
            entry: "run".into(),
            duration: 20,
            tp_cost: 0,
        },
    ]
}

fn prepare(
    files: &Files,
    cost: u16,
    model: Arc<ModelDefinition>,
    mut owner: Actor,
    cache: &mut PreparationCache,
) -> Result<(Battle, ActionRequest)> {
    owner.control = Control::Auto;
    let mut enemy = actor();
    enemy.side = Side::Enemy;
    let prepared = battle::prepare(
        cache,
        files,
        &bindings(cost),
        vec![owner, enemy],
        1,
        &mut Resources {
            paths: vec![],
            fail: false,
        },
        vec![Some(model), None],
    )?;
    let ids: Vec<_> = prepared.actor_ids().collect();
    Ok((
        Battle::new(prepared),
        ActionRequest {
            actor: ids[0],
            action: 1,
            target: ids[1],
        },
    ))
}

fn request(action: ActionRequest) -> BattleInput {
    BattleInput {
        actions: vec![action],
        ..Default::default()
    }
}

fn snapshot() -> Result<(Files, Arc<ModelDefinition>)> {
    let mut files = files("");
    sources(&mut files);
    files.bytes.insert(
        resonance_content::battle_effect::COMMON_PATH.into(),
        files.read("test-effects.json")?,
    );
    files.bytes.insert(
        resonance_content::battle_effect::TINTS_PATH.into(),
        Arc::from(include_bytes!("../fixtures/effect-tints.json").as_slice()),
    );
    let mut profile = profile::profile();
    profile.casting.base_ticks = 5;
    profile.casting.resume_loop_start = 0;
    let chant = serde_json::from_value(serde_json::json!([
        {"time":0,"clip":30,"blend":8,"start":0,"end":0,"layer_flags":8,"resource":-1,"rate":0.5},
        {"time":2,"clip":31,"blend":2,"start":0,"end":0,"layer_flags":8,"resource":-1,"rate":0.5},
        {"time":4,"clip":32,"blend":2,"start":0,"end":0,"layer_flags":72,"resource":-1,"rate":0.5},
        {"time":-2,"clip":0,"blend":0,"start":0,"end":0,"layer_flags":0,"resource":0,"rate":0.0}
    ]))?;
    files.bytes.insert(
        battle_profile::PARTY_PATH.into(),
        serde_json::to_vec(&battle_profile::Table {
            default_strategy: [[0; 3]; 10],
            companion_policy: Default::default(),
            placement: Default::default(),
            entry: Default::default(),
            source_sha256: "a".repeat(64),
            records: vec![profile; 11],
            chant,
            voice_sequences: vec![vec![]; 10],
            death_voice_pairs: vec![],
            contact_sounds: Default::default(),
        })?
        .into(),
    );
    let mut definitions = vec![arte::Definition::default(); 253];
    definitions[78] = arte::Definition {
        tp_cost: 9,
        flags: 0x440186,
        cast_time_adjustment: 7,
        recovery_ticks: 3,
        ..Default::default()
    };
    let catalogue = arte::Catalogue {
        definitions,
        learning_storage: [0; 5],
        learning: vec![
            arte::LearningList {
                count: 0,
                technique_slots: vec![0; 40]
            };
            11
        ],
        combinations: vec![
            arte::Combination {
                name: None,
                native_id: 0,
                participant_count: 0,
                recipe_slots: [[0; 4]; 6],
                duration_ticks: 0,
                storage: [0; 2],
                camera_pitch_offset_degrees: 0.
            };
            20
        ],
    };
    files
        .bytes
        .insert(arte::PATH.into(), serde_json::to_vec(&catalogue)?.into());
    let model = ModelDefinition {
        resource: 1,
        secondary_motion: vec![],
        hurt_motions: [None; 2],
        idle_motions: [None; 2],
        guard_motions: [None; 2],
        stun: None,
        knockdown: None,
        skeleton: Skeleton {
            bones: vec![Bone {
                name: "root".into(),
                parent: None,
                bind: Transform::default(),
                bind_channels: TransformChannels(8),
            }],
        },
        motions: [0, 12, 30, 31, 32]
            .into_iter()
            .map(|clip| {
                (
                    clip,
                    Motion {
                        duration_frames: 15.,
                        tracks: vec![],
                    },
                )
            })
            .collect(),
        initial: Playback {
            clip: 0,
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
    Ok((files, Arc::new(model)))
}

#[test]
fn prepared_casting_uses_table_clocks_costs_and_rejects_incomplete_replacements() -> Result<()> {
    let (files, model) = snapshot()?;
    let mut cache = PreparationCache::default();
    let (mut active, action) = prepare(&files, 9, model.clone(), actor(), &mut cache)?;
    let first = active.step(request(action))?;
    assert!(matches!(
        first.actors[0].activity,
        Activity::Casting { clock: 12, .. }
    ));
    assert!(
        first
            .cues
            .iter()
            .any(|cue| matches!(cue, Cue::Sound { sound, priority: 1, .. } if sound.index == 109))
    );
    assert!(prepare(&files, 10, model.clone(), actor(), &mut cache).is_err());
    let mut missing = files.clone();
    missing.bytes.remove(arte::PATH);
    assert!(prepare(&missing, 9, model.clone(), actor(), &mut cache).is_err());
    let mut missing_tints = files.clone();
    missing_tints
        .bytes
        .remove(resonance_content::battle_effect::TINTS_PATH);
    assert!(prepare(&missing_tints, 9, model.clone(), actor(), &mut cache).is_err());
    let mut invalid_element = files.clone();
    let mut catalogue: arte::Catalogue = files.json(arte::PATH)?;
    catalogue.definitions[78].element = 10;
    invalid_element
        .bytes
        .insert(arte::PATH.into(), serde_json::to_vec(&catalogue)?.into());
    assert!(prepare(&invalid_element, 9, model.clone(), actor(), &mut cache).is_err());
    let mut incomplete = (*model).clone();
    incomplete.motions.remove(&32);
    assert!(prepare(&files, 9, Arc::new(incomplete), actor(), &mut cache).is_err());
    for flags in [1, 0x20] {
        let mut changed = files.clone();
        let mut catalogue: arte::Catalogue = changed.json(arte::PATH)?;
        catalogue.definitions[78].flags |= flags;
        changed
            .bytes
            .insert(arte::PATH.into(), serde_json::to_vec(&catalogue)?.into());
        let error = prepare(&changed, 9, model.clone(), actor(), &mut cache)
            .err()
            .context("unsupported route activated")?;
        assert!(error.to_string().contains(if flags == 1 {
            "stored-scene"
        } else {
            "special casting"
        }));
    }
    for age in 1..=13 {
        let frame = active.step(BattleInput::default())?;
        assert_eq!(frame.actors[0].tp, if age < 13 { 40 } else { 31 });
    }
    Ok(())
}

#[test]
#[ignore = "requires current casting tables/scripts and Genis model; no devices"]
fn cold_casting_preparation_matches_original_opening_clocks_and_tp() -> Result<()> {
    let root = common::asset_root();
    let mut cache = resonance_content::prepared::Cache::default();
    let files = Files::load(&root, &["fields/map-340.preload.json"], &mut cache, || {
        false
    })?;
    let mut files = battle::model::load_files(
        &root,
        files,
        &[battle::model::ModelSource::Party(3)],
        &mut cache,
        || false,
    )?;
    let published_source = files.read("scripts/battle/genis_lightning.sym")?.to_vec();
    assert_eq!(
        published_source,
        include_bytes!("../../../../scripts/battle/genis_lightning.sym")
    );
    // A no-op resident isolates this Fire Ball clock comparison. The complete
    // prepared Lightning sequence is exercised by lightning_cast.rs.
    files.bytes.insert(
        "scripts/resident.sym".into(),
        Arc::from(b"script battle; pub task run() {}".as_slice()),
    );
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../battle/tests/fixtures/opening-casting.json"
    ))?;
    let requests: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/casting-requests.json"))?;
    let catalogue: arte::Catalogue = files.json(arte::PATH)?;
    assert_eq!(
        (
            catalogue.definitions[78].tp_cost,
            catalogue.definitions[78].cast_time_adjustment
        ),
        (9, 60)
    );
    assert_ne!(
        catalogue.definitions[99].flags & 1,
        0,
        "Nurse requires the stored-scene route"
    );
    let profiles: battle_profile::Table = files.json(battle_profile::PARTY_PATH)?;
    assert_eq!(profiles.records[2].effect_scale.finite()?, 0.9);
    let mut compiler = PreparationCache::default();
    for (technique, cost) in [(66, 7), (78, 9)] {
        let source = std::str::from_utf8(&published_source)?
            .replace("genis/78", &format!("genis/{technique}"));
        files.bytes.insert(
            "scripts/battle/genis_lightning.sym".into(),
            Arc::from(source.as_bytes()),
        );
        let mut active = None;
        let mut initial_tp = 0;
        for row in fixture["observations"]
            .as_array()
            .context("missing casting observations")?
        {
            let initializing = row["function"] == "initialize";
            if initializing {
                let before = &row["before"];
                let mut owner = actor();
                owner.tp = serde_json::from_value(before["tp"].clone())?;
                initial_tp = owner.tp;
                owner.max_tp = 100;
                let setup = battle::model::ModelSetup {
                    resource: 1,
                    initial: Playback {
                        clip: serde_json::from_value(before["clip"].clone())?,
                        frame: f32::from_bits(serde_json::from_value(
                            before["frame_bits"].clone(),
                        )?),
                        rate: 0.5,
                        repeat: true,
                    },
                    suppress_root_translation: [false; 3],
                    stun: None,
                };
                let (owner, model) = battle::model::party(&files, 3, owner, setup)?;
                active = Some(prepare(&files, cost, model, owner, &mut compiler)?);
            }
            let (battle, action) = active
                .as_mut()
                .context("casting observation before initialization")?;
            let frame = battle.step(if initializing {
                request(*action)
            } else {
                BattleInput::default()
            })?;
            let after = &row["after"];
            let index = row["index"].as_u64().context("missing observation index")? as usize;
            let observed = &requests["observations"][index];
            assert_eq!(observed["combat_tick"], row["combat_tick"]);
            let expected: Vec<_> = observed["requests"]
                .as_array()
                .context("missing requests")?
                .iter()
                .map(|request| match request["kind"].as_str() {
                    Some("effect") => ("effect", request["member"].as_u64().unwrap(), 0),
                    Some("sound") => (
                        "sound",
                        request["index"].as_u64().unwrap(),
                        request["priority"].as_u64().unwrap(),
                    ),
                    Some("release") => ("release", 0, 0),
                    _ => panic!("unknown observed request"),
                })
                .collect();
            let actual: Vec<_> = frame
                .cues
                .iter()
                .filter_map(|cue| match cue {
                    Cue::Effect { member, .. } => Some(("effect", u64::from(*member), 0)),
                    Cue::Sound {
                        sound, priority, ..
                    } => Some(("sound", u64::from(sound.index), u64::from(*priority))),
                    Cue::Released { .. } => Some(("release", 0, 0)),
                    _ => None,
                })
                .collect();
            assert_eq!(
                actual, expected,
                "casting requests at combat tick {}",
                row["combat_tick"]
            );
            if !initializing {
                let before = &row["before"];
                assert_eq!(
                    u64::from(frame.models[0].clip),
                    before["clip"].as_u64().context("missing clip")?
                );
                assert_eq!(
                    u64::from(frame.models[0].frame.to_bits()),
                    before["frame_bits"].as_u64().context("missing frame")?
                );
            }
            if let Activity::Casting { clock, .. } = frame.actors[0].activity {
                assert_eq!(
                    i64::from(clock),
                    after["remaining"].as_i64().context("missing clock")?
                );
            } else {
                assert_eq!(frame.actors[0].activity, Activity::Recovering);
            }
            let original_tp: u16 = serde_json::from_value(after["tp"].clone())?;
            let extra_cost = if original_tp < initial_tp {
                cost - 7
            } else {
                0
            };
            assert_eq!(frame.actors[0].tp, original_tp - extra_cost);
            assert_eq!(
                frame
                    .cues
                    .iter()
                    .any(|cue| matches!(cue, Cue::Released { .. })),
                after["primary_mode"] == 1
            );
        }
    }
    Ok(())
}
