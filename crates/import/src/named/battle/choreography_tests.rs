use super::tests::{equivalent, no_inventory};
use super::*;
use resonance_content::{
    animation::{Motion, Skeleton},
    battle_event::document as events,
    battle_scene::document::{self as scenes, Kind},
    diagnostics::Diagnostics,
    library::{Library, Mod},
    prepared::{Cache, Files},
};

#[test]
fn named_scene_scopes_capture_current_rules_and_reject_legacy_inventory() -> Result<()> {
    let output = tempfile::tempdir()?;
    let write = |path: &str, bytes: &[u8]| -> Result<()> {
        fs::create_dir_all(output.path().join(path).parent().unwrap())?;
        Ok(fs::write(output.path().join(path), bytes)?)
    };
    write(
        "content.json",
        br#"{"version":1,"game":"GQSEAF","revision":0}"#,
    )?;
    write(
        "bank.json",
        br#"{"version":1,"programs":"programs.json","actors":"particles.json","art":null}"#,
    )?;
    write("programs.json", b"[]")?;
    write("particles.json", b"[]")?;
    let mut rules = serde_json::json!({"version":1,"projectiles":[],"actions":{},"actions_error":null,
        "duration":40,"stage_color":null,"camera":null,"strike_points":[],"recovery_radius":null,"spell_recipe":null});
    for (kind, duration) in [(Kind::Technique, 40), (Kind::Skill, 70)] {
        let category = kind.mapping().strip_suffix("/mappings.json").unwrap();
        write(kind.mapping(), br#"{"228":"example"}"#)?;
        rules["duration"] = duration.into();
        let path = format!("{category}/example/rules.json");
        write(&path, &serde_json::to_vec(&rules)?)?;
        write(
            &format!("{category}/example/scene.json"),
            &serde_json::to_vec(&serde_json::json!({
                "version":1,"rules":path,"effects":"bank.json","cutin":null,"weapon":null
            }))?,
        )?;
    }
    let library = Library::open(output.path(), &[])?;
    let mut snapshot = library.snapshot();
    let diagnostics = Diagnostics::new(true);
    assert_eq!(
        scenes::capture(&mut snapshot, Kind::Technique, 228, &diagnostics, &|| false)?
            .0
            .duration,
        40
    );
    assert_eq!(
        scenes::capture(&mut snapshot, Kind::Skill, 228, &diagnostics, &|| false)?
            .0
            .duration,
        70
    );
    rules["duration"] = 100.into();
    write(
        "battle/scenes/example/rules.json",
        &serde_json::to_vec(&rules)?,
    )?;
    assert_eq!(
        scenes::capture(
            &mut library.snapshot(),
            Kind::Technique,
            228,
            &diagnostics,
            &|| false
        )?
        .0
        .duration,
        100
    );
    let files = Files::from_snapshot(
        snapshot,
        &mut Cache::default(),
        || false,
        diagnostics.clone(),
    )?;
    assert_eq!(scenes::load(&files, Kind::Technique, 228)?.duration, 40);
    assert_eq!(scenes::load(&files, Kind::Skill, 228)?.duration, 70);
    rules["files"] = serde_json::json!({});
    write(
        "battle/scenes/example/rules.json",
        &serde_json::to_vec(&rules)?,
    )?;
    assert!(
        format!(
            "{:#}",
            scenes::capture(
                &mut library.snapshot(),
                Kind::Technique,
                228,
                &diagnostics,
                &|| false
            )
            .unwrap_err()
        )
        .contains("unknown field")
    );
    write(scenes::TECHNIQUES, br#"{"228":"absent-replacement"}"#)?;
    assert!(
        format!(
            "{:#}",
            scenes::capture(
                &mut library.snapshot(),
                Kind::Technique,
                228,
                &diagnostics,
                &|| false
            )
            .unwrap_err()
        )
        .contains("absent-replacement")
    );
    assert!(
        scenes::capture(
            &mut library.snapshot(),
            Kind::Skill,
            228,
            &Diagnostics::default(),
            &|| true
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn named_events_keep_external_motion_scopes_and_require_current_blink() -> Result<()> {
    let output = tempfile::tempdir()?;
    let write = |path: &str, bytes: &[u8]| -> Result<()> {
        fs::create_dir_all(output.path().join(path).parent().unwrap())?;
        Ok(fs::write(output.path().join(path), bytes)?)
    };
    write(
        "content.json",
        br#"{"version":1,"game":"GQSEAF","revision":0}"#,
    )?;
    write(events::MAPPINGS, br#"{"2":"intro"}"#)?;
    write(
        "battle/events/intro/event.json",
        &serde_json::to_vec(&serde_json::json!({
            "version":1,"script":"scenario.ssb","messages":"messages.json","blink":"blink.json",
            "audio":{"music":[],"sounds":[],"streams":[],"voices":[]},"actors":{"1":"actor.json"}
        }))?,
    )?;
    write(
        "scenario.ssb",
        b"scenario bytecode is decoded by runtime admission",
    )?;
    write("messages.json", b"[]")?;
    write("blink.json", br#"{"frames":[0,1,2,0]}"#)?;
    write("mesh.glb", b"geometry is decoded by runtime admission")?;
    write(
        "idle.motion.json",
        &Motion {
            duration_frames: 30.,
            tracks: vec![],
        }
        .encode()?,
    )?;
    write(
        "external.motion.json",
        &Motion {
            duration_frames: 90.,
            tracks: vec![],
        }
        .encode()?,
    )?;
    let mut actor = serde_json::json!({"version":1,
    "skeleton":{"bones":[{"name":"root","parent":null,"bind_channels":0,
        "bind":{"translation":[0,0,0],"rotation":[0,0,0,1],"scale":[1,1,1]}}]},
    "parts":[{"resource":0,"mesh":"mesh.glb","textures":[],"materials":[],"translation":[0,0,0],
        "bone_names":["root"],"autoplay":false,"texture_animations":[],"clips":[
            {"resource_slot":0,"motion":"idle.motion.json","duration_seconds":0},
            {"resource_slot":0,"animation_resource":196750,"motion":"external.motion.json","duration_seconds":0}
        ]}]});
    write("actor.json", &serde_json::to_vec(&actor)?)?;
    let library = Library::open(output.path(), &[])?;
    let mut snapshot = library.snapshot();
    let diagnostics = Diagnostics::new(true);
    let scene = events::capture(&mut snapshot, 2, &diagnostics, &|| false)?;
    assert_eq!(
        scene.actors[&1].parts[0]
            .clips
            .iter()
            .map(|clip| clip.duration_seconds)
            .collect::<Vec<_>>(),
        [1., 3.]
    );
    actor["parts"][0]["clips"][1]["animation_resource"] = serde_json::Value::Null;
    write("actor.json", &serde_json::to_vec(&actor)?)?;
    assert!(
        format!(
            "{:#}",
            events::capture(&mut library.snapshot(), 2, &diagnostics, &|| false).unwrap_err()
        )
        .contains("duplicate battle motion")
    );
    fs::remove_file(output.path().join("blink.json"))?;
    assert!(
        events::capture(&mut library.snapshot(), 2, &Diagnostics::default(), &|| {
            false
        })
        .is_err()
    );
    assert!(events::capture(&mut snapshot, 2, &diagnostics, &|| false).is_ok());
    Ok(())
}

#[test]
#[ignore = "requires original disc 1; compares native stored scenes and named overrides"]
fn original_stored_scenes_preserve_rules_art_and_separate_id_scopes() -> Result<()> {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let source = tempfile::tempdir()?;
    let native = crate::battle_scene::prepare_all(&extracted, source.path())?;
    assert!(native.failures.is_empty(), "{:?}", native.failures);
    assert!(native.unreferenced_skills.is_empty());
    let output = tempfile::tempdir()?;
    let mut publisher = Publisher::new(source.path(), output.path())?;
    let technique: Mappings =
        serde_json::from_str(include_str!("../../naming/battle-scenes.json"))?;
    let skill: Mappings = serde_json::from_str(include_str!("../../naming/battle-skills.json"))?;
    assert_eq!(technique.iter().count(), 87);
    assert_eq!(skill.iter().count(), 2);
    assert_eq!(
        native
            .techniques
            .keys()
            .map(|&id| u32::from(id))
            .collect::<Vec<_>>(),
        technique.iter().map(|(id, _)| id).collect::<Vec<_>>()
    );
    assert_eq!(
        native
            .skills
            .keys()
            .map(|&id| u32::from(id))
            .collect::<Vec<_>>(),
        skill.iter().map(|(id, _)| id).collect::<Vec<_>>()
    );
    publisher.battle_scenes(Kind::Technique, native.techniques.clone())?;
    publisher.battle_scenes(Kind::Skill, native.skills.clone())?;
    publisher.descriptor("GQSEAF", 0)?;
    let library = Library::open(output.path(), &[])?;
    let mut snapshot = library.snapshot();
    let diagnostics = Diagnostics::new(true);
    let mut digests = BTreeMap::new();
    let (mut model_count, mut clip_count, mut weapons, mut cutins) = (0, 0, 0, 0);
    for (kind, names) in [(Kind::Technique, &technique), (Kind::Skill, &skill)] {
        for (id, name) in names.iter() {
            let id = id as u16;
            assert_eq!(crate::naming::battle_scene(kind, id)?, name);
            let prepared = match kind {
                Kind::Technique => &native.techniques[&id],
                Kind::Skill => &native.skills[&id],
            };
            let mut old = prepared.scene.clone();
            let mut old_bank = prepared.effects.clone();
            let (mut scene, bank) =
                scenes::capture(&mut snapshot, kind, id, &diagnostics, &|| false)
                    .with_context(|| format!("{kind:?} {id}"))?;

            old.source_sha256.clear();
            old.effects.clear();
            if let Some(weapon) = &mut old.weapon {
                weapon.source_sha256.clear();
            }
            old_bank.source_sha256.clear();
            if let Some(art) = &mut old_bank.art {
                art.source_sha256.clear();
            }
            scene.effects.clear();
            equivalent(&old, &scene, source.path(), output.path(), &mut digests)?;
            equivalent(&old_bank, &bank, source.path(), output.path(), &mut digests)?;
            weapons += usize::from(scene.weapon.is_some());
            cutins += usize::from(scene.cutin.is_some());
            for model in scene
                .models
                .values()
                .chain(scene.weapon.iter().flat_map(|weapon| weapon.parts.values()))
            {
                model_count += 1;
                for layer in &model.layers {
                    let skeleton = model
                        .layer_skeletons
                        .get(&layer.scene.resource)
                        .unwrap_or(&model.rig.skeleton);
                    let mesh =
                        Skeleton::from_glb(&snapshot.read(&layer.scene.mesh, 128 * 1024 * 1024)?)?;
                    assert!(
                        mesh.bones
                            .iter()
                            .map(|bone| &bone.name)
                            .eq(layer.scene.bone_names.iter())
                    );
                    for clip in &layer.scene.clips {
                        Motion::decode(&snapshot.read(&clip.motion, 128 * 1024 * 1024)?)?
                            .validate(skeleton)?;
                        clip_count += 1;
                    }
                }
            }
        }
    }
    for path in publisher
        .published
        .keys()
        .filter(|path| path.ends_with(".json"))
    {
        no_inventory(
            &serde_json::from_slice(&fs::read(output.path().join(path))?)?,
            path,
        );
    }
    // Same ID, different original namespace: Gravity Well and Falcon's Crest.
    let technique_path = snapshot.resolve(scenes::TECHNIQUES, 228, Some("scene.json"))?;
    let skill_path = snapshot.resolve(scenes::SKILLS, 228, Some("scene.json"))?;
    assert_ne!(technique_path, skill_path);
    let mut definition: scenes::Definition = snapshot.json(&technique_path, 64 * 1024 * 1024)?;
    let mut rules: scenes::Rules = snapshot.json(&definition.rules, 64 * 1024 * 1024)?;
    let original_duration = rules.duration;
    rules.duration += 1;
    let modification = tempfile::tempdir()?;
    let write = |path: &str, bytes: &[u8]| -> Result<()> {
        fs::create_dir_all(modification.path().join(path).parent().unwrap())?;
        Ok(fs::write(modification.path().join(path), bytes)?)
    };
    write(scenes::TECHNIQUES, br#"{"228":"changed-gravity"}"#)?;
    definition.rules = "battle/scenes/changed-gravity/rules.json".into();
    write(&definition.rules, &serde_json::to_vec(&rules)?)?;
    write(
        "battle/scenes/changed-gravity/scene.json",
        &serde_json::to_vec(&definition)?,
    )?;
    let modified = Library::open(
        output.path(),
        &[Mod {
            name: "choreography edit".into(),
            root: modification.path().into(),
        }],
    )?;
    let mut edited = modified.snapshot();
    assert_eq!(
        scenes::capture(&mut edited, Kind::Technique, 228, &diagnostics, &|| false)?
            .0
            .duration,
        original_duration + 1
    );
    let before_skill = scenes::capture(&mut snapshot, Kind::Skill, 228, &diagnostics, &|| false)?.0;
    let after_skill = scenes::capture(&mut edited, Kind::Skill, 228, &diagnostics, &|| false)?.0;
    assert_eq!(
        serde_json::to_value(before_skill)?,
        serde_json::to_value(after_skill)?
    );
    let files = Files::from_snapshot(edited, &mut Cache::default(), || false, diagnostics.clone())?;
    assert_eq!(
        scenes::load(&files, Kind::Technique, 228)?.duration,
        original_duration + 1
    );
    write(&definition.rules, b"{}")?;
    assert!(
        scenes::capture(
            &mut modified.snapshot(),
            Kind::Technique,
            228,
            &diagnostics,
            &|| false
        )
        .is_err()
    );
    assert_eq!(
        scenes::load(&files, Kind::Technique, 228)?.duration,
        original_duration + 1
    );
    assert!(scenes::capture(&mut snapshot, Kind::Technique, 228, &diagnostics, &|| true).is_err());
    eprintln!(
        "verified 89 stored scenes, {model_count} models, {clip_count} clips, {weapons} temporary weapons, {cutins} cut-ins, {} publications",
        publisher.published.len()
    );
    Ok(())
}

#[test]
#[ignore = "requires original disc 1; compares native event scripts and shared actor ownership"]
fn original_battle_events_keep_scoped_actors_motions_and_editable_blink() -> Result<()> {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let source = tempfile::tempdir()?;
    let output = tempfile::tempdir()?;
    let party =
        crate::battle_model::party::prepare_selected(&extracted, source.path(), &[1, 2, 3, 9])?;
    let native = crate::battle_event::prepare(&extracted, &party, source.path())?;
    let mut publisher = Publisher::new(source.path(), output.path())?;
    publisher.battle_party(party)?;
    publisher.battle_victories(
        &crate::battle_victory::prepare(&extracted, source.path())?,
        "battle",
    )?;
    publisher.battle_events(native.scenes.clone())?;
    publisher.descriptor("GQSEAF", 0)?;
    let library = Library::open(output.path(), &[])?;
    let mut snapshot = library.snapshot();
    let diagnostics = Diagnostics::new(true);
    let mut digests = BTreeMap::new();
    let mut clips = 0;
    for id in [2, 3] {
        let original = &native.scenes[&id];
        let scene = events::capture(&mut snapshot, id, &diagnostics, &|| false)?;
        equivalent(original, &scene, source.path(), output.path(), &mut digests)?;
        assert!(scene.blink.is_some());
        for model in scene.actors.values() {
            let mut scopes = BTreeSet::new();
            for clip in &model.parts[0].clips {
                assert!(scopes.insert((clip.animation_resource, clip.resource_slot)));
                Motion::decode(&snapshot.read(&clip.motion, 128 * 1024 * 1024)?)?
                    .validate(&model.skeleton)?;
                clips += 1;
            }
        }
    }
    for path in publisher
        .published
        .keys()
        .filter(|path| path.ends_with(".json"))
    {
        no_inventory(
            &serde_json::from_slice(&fs::read(output.path().join(path))?)?,
            path,
        );
    }
    let blink = resonance_content::effect::BlinkCycle {
        frames: vec![0, 1, 2, 0],
    };
    fs::write(
        output.path().join(events::BLINK),
        serde_json::to_vec(&blink)?,
    )?;
    assert_eq!(
        events::capture(&mut library.snapshot(), 2, &diagnostics, &|| false)?
            .blink
            .unwrap()
            .frames,
        blink.frames
    );
    assert_ne!(
        events::capture(&mut snapshot, 2, &diagnostics, &|| false)?
            .blink
            .unwrap()
            .frames,
        blink.frames
    );
    assert!(events::capture(&mut snapshot, 2, &diagnostics, &|| true).is_err());
    eprintln!(
        "verified both battle event scripts, 10 actor bindings, {clips} primary clips, {} publications including shared party and Vidarr owners",
        publisher.published.len()
    );
    Ok(())
}
