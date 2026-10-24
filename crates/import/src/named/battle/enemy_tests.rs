use super::tests::{equivalent, no_inventory};
use super::*;
use resonance_content::{
    animation::{Motion, Skeleton},
    battle_enemy::document as enemies,
    battle_model::document::enemy as artwork,
    diagnostics::Diagnostics,
    library::{Library, Mod},
    model_preview::document as previews,
};

#[test]
#[ignore = "requires original disc 1; compares every enemy, secondary body, attachment, variant and effect bank"]
fn original_enemies_keep_rules_shared_bodies_and_all_dynamic_choices() -> Result<()> {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let raw = tempfile::tempdir()?;
    let source = raw.path().to_path_buf();
    let executable = fs::read(extracted.join("sys/main.dol"))?;
    let sources = crate::source_assets::Sources::read_with(&extracted, &executable)?;
    let usual = fs::read(extracted.join("files").join(&sources.usual))?;
    let shared = crate::battle_effect::SharedPalettes::prepare(&usual, &source)?;
    let native = crate::monsters::prepare(
        &extracted,
        &source,
        &sources,
        &usual,
        &crate::all_assets::monster_catalogue::read(&executable)?,
        &crate::all_assets::inventory_ui::read(&executable)?,
        &shared,
    )?;
    let output = tempfile::tempdir()?;
    let mut publisher = Publisher::new(&source, output.path())?;
    publisher.battle_enemies(native.enemies.clone())?;
    publisher.descriptor("GQSEAF", 0)?;
    let monsters = native.book.records;
    assert_eq!(monsters.len(), 251);
    let library = Library::open(output.path(), &[])?;
    let mut snapshot = library.snapshot();
    let diagnostics = Diagnostics::new(true);
    let mut digests = BTreeMap::new();
    let (mut bodies, mut attachments, mut variants, mut effects, mut clips) = (0, 0, 0, 0, 0);
    for monster in &monsters {
        let id = monster.id;
        let captured = enemies::capture(&mut snapshot, id, &diagnostics, &|| false)
            .with_context(|| format!("enemy {id}"))?;
        let mut rules = native.enemies[&id].rules.clone();
        rules.source_sha256.clear();
        equivalent(
            &rules,
            &captured.definition,
            &source,
            output.path(),
            &mut digests,
        )?;
        let art = captured.artwork.context("missing original enemy artwork")?;
        let mut old = native.enemies[&id].artwork.clone();

        old.source_sha256.clear();
        equivalent(&old, &art.source, &source, output.path(), &mut digests)
            .with_context(|| format!("enemy {id} models"))?;
        equivalent(
            &monster.preview,
            &art.preview,
            &source,
            output.path(),
            &mut digests,
        )
        .with_context(|| format!("enemy {id} preview"))?;
        let (_, bank) = captured.effects.context("missing original enemy effects")?;
        let mut old_bank = native.enemies[&id].effects.clone();
        old_bank.source_sha256.clear();
        if let Some(art) = &mut old_bank.art {
            art.source_sha256.clear();
        }
        equivalent(&old_bank, &bank, &source, output.path(), &mut digests)
            .with_context(|| format!("enemy {id} effects"))?;
        for layer in &art.preview.parts {
            let skeleton =
                Skeleton::from_glb(&snapshot.read(&layer.scene.mesh, 128 * 1024 * 1024)?)?;
            assert!(
                skeleton
                    .bones
                    .iter()
                    .map(|bone| &bone.name)
                    .eq(layer.scene.bone_names.iter())
            );
            for clip in &layer.scene.clips {
                Motion::decode(&snapshot.read(&clip.motion, 128 * 1024 * 1024)?)?
                    .validate(&skeleton)?;
                clips += 1;
            }
        }
        if let Some(secondary) = &art.source.secondary_body {
            bodies += 1;
            for (index, layer) in secondary.model.layers.iter().enumerate() {
                let preview = art
                    .preview
                    .parts
                    .iter()
                    .find(|part| part.scene.resource == layer.scene.resource)
                    .context("missing shared secondary body")?;
                assert_eq!(layer.scene.mesh, preview.scene.mesh);
                assert_eq!(layer.scene.textures, preview.scene.textures);
                assert!(layer.scene.clips.iter().all(|clip| {
                    preview
                        .scene
                        .clips
                        .iter()
                        .any(|other| other.motion == clip.motion)
                }));
                if let Some(model) = bank
                    .art
                    .as_ref()
                    .and_then(|art| art.models.get(&secondary.model_slot))
                {
                    assert_eq!(layer.scene.mesh, model.layers[index].scene.mesh);
                }
            }
        }
        for model in art.source.attachments.values() {
            attachments += 1;
            for layer in &model.layers {
                let preview = art
                    .preview
                    .parts
                    .iter()
                    .find(|part| part.scene.resource == layer.scene.resource)
                    .context("missing shared attachment")?;
                assert_eq!(layer.scene.mesh, preview.scene.mesh);
                assert_eq!(layer.scene.textures, preview.scene.textures);
            }
        }
        variants += art.source.weapon_variants.len();
        effects += bank.art.as_ref().map_or(0, |art| art.models.len());
        for model in art
            .source
            .attachments
            .values()
            .chain(art.source.secondary_body.iter().map(|body| &body.model))
            .chain(
                art.source
                    .weapon_variants
                    .values()
                    .map(|variant| &variant.model),
            )
            .chain(bank.art.iter().flat_map(|art| art.models.values()))
        {
            for layer in &model.layers {
                let skeleton = model
                    .layer_skeletons
                    .get(&layer.scene.resource)
                    .unwrap_or(&model.rig.skeleton);
                for clip in &layer.scene.clips {
                    Motion::decode(&snapshot.read(&clip.motion, 128 * 1024 * 1024)?)?
                        .validate(skeleton)?;
                }
            }
        }
    }
    assert_eq!(bodies, 3);
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

    let modification = tempfile::tempdir()?;
    let write = |path: &str, bytes: &[u8]| -> Result<()> {
        fs::create_dir_all(modification.path().join(path).parent().unwrap())?;
        Ok(fs::write(modification.path().join(path), bytes)?)
    };
    let entry_path = snapshot.resolve(enemies::MAPPINGS, 0, Some("enemy.json"))?;
    let entry: enemies::Entry = snapshot.json(&entry_path, 16 * 1024 * 1024)?;
    write(enemies::MAPPINGS, br#"{"0":"forest-guardian"}"#)?;
    write(
        "battle/enemies/forest-guardian/enemy.json",
        &serde_json::to_vec(&entry)?,
    )?;
    let art: artwork::Definition = snapshot.json(&entry.artwork, 16 * 1024 * 1024)?;
    let mut preview: previews::Definition = snapshot.json(&art.model, 16 * 1024 * 1024)?;
    let old_motion = preview.model.parts[0].scene.clips[0].motion.clone();
    let mut motion = Motion::decode(&snapshot.read(&old_motion, 128 * 1024 * 1024)?)?;
    motion.duration_frames += 30.;
    let clip_path = "battle/enemies/forest-guardian/idle.motion.json";
    write(clip_path, &motion.encode()?)?;
    preview.model.parts[0].scene.clips[0].motion = clip_path.into();
    write(&art.model, &serde_json::to_vec(&preview)?)?;
    let modified = Library::open(
        output.path(),
        &[Mod {
            name: "enemy edit".into(),
            root: modification.path().into(),
        }],
    )?;
    let mut edited_snapshot = modified.snapshot();
    let edited = enemies::capture(&mut edited_snapshot, 0, &diagnostics, &|| false)?;
    let edited = edited.artwork.unwrap();
    assert_eq!(edited.preview.parts[0].scene.clips[0].motion, clip_path);
    assert_eq!(
        edited.preview.parts[0].scene.clips[0].duration_seconds,
        motion.duration_frames / resonance_content::animation::FRAME_HZ
    );
    assert_eq!(
        enemies::capture(&mut snapshot, 0, &diagnostics, &|| false)?
            .artwork
            .unwrap()
            .preview
            .parts[0]
            .scene
            .clips[0]
            .motion,
        old_motion
    );
    let mut broken = art;
    broken.rig = "battle/enemies/forest-guardian/missing-rig.json".into();
    write(&entry.artwork, &serde_json::to_vec(&broken)?)?;
    let tolerant = Diagnostics::default();
    let captured = enemies::capture(&mut modified.snapshot(), 0, &tolerant, &|| false)?;
    assert!(captured.artwork.is_none());
    assert!(captured.effects.is_some());
    assert_eq!(tolerant.entries().len(), 1);
    assert!(
        format!(
            "{:#}",
            enemies::capture(&mut modified.snapshot(), 0, &diagnostics, &|| false).unwrap_err()
        )
        .contains("missing-rig.json")
    );
    assert!(
        enemies::capture(&mut edited_snapshot, 0, &diagnostics, &|| false)?
            .artwork
            .is_some()
    );
    write(&entry.rules, b"{}")?;
    assert!(
        enemies::capture(
            &mut modified.snapshot(),
            0,
            &Diagnostics::default(),
            &|| false
        )
        .is_err()
    );
    assert!(enemies::capture(&mut snapshot, 0, &Diagnostics::default(), &|| true).is_err());
    eprintln!(
        "verified 251 enemies, {bodies} secondary bodies, {attachments} carried models, {variants} weapon variants, {effects} effect models, {clips} preview clips, {} publications",
        publisher.published.len()
    );
    Ok(())
}

#[test]
fn enemy_models_capture_current_motion_and_retain_snapshot_and_failure_policy() -> Result<()> {
    let output = tempfile::tempdir()?;
    let write = |path: &str, value: &[u8]| -> Result<()> {
        Ok(fs::write(output.path().join(path), value)?)
    };
    write(
        "content.json",
        br#"{"version":1,"game":"GQSEAF","revision":0}"#,
    )?;
    let rig = serde_json::json!({"skeleton":{"bones":[{"name":"root","parent":null,"bind_channels":0,
        "bind":{"translation":[0,0,0],"rotation":[0,0,0,1],"scale":[1,1,1]}}]},"attachments":{}});
    write("rig.json", &serde_json::to_vec(&rig)?)?;
    write("body.glb", b"geometry decoded by runtime admission")?;
    let mut motion = Motion {
        duration_frames: 60.,
        tracks: vec![],
    };
    write("idle.motion.json", &motion.encode()?)?;
    let preview = serde_json::json!({"version":1,"scale":1,"elevation":0,"hidden_geometry":[],"behavior":null,
        "parts":[{"animation":0,"attached_to":null,"additive":false,
            "scene":{"resource":0,"mesh":"body.glb","textures":[],"materials":[],"translation":[0,0,0],
                "bone_names":["root"],"autoplay":true,"texture_animations":[],
                "clips":[{"resource_slot":0,"motion":"idle.motion.json","duration_seconds":999}]}}]});
    write("preview.json", &serde_json::to_vec(&preview)?)?;
    let definition = serde_json::json!({"version":1,"rig":"rig.json","model":"preview.json","secondary_body":null,
        "body_variant":null,"attached_models":[],"attachments":{},"uv_animation":{},"weapon_variants":{},"trails":{}});
    write("model.json", &serde_json::to_vec(&definition)?)?;
    let library = Library::open(output.path(), &[])?;
    let mut snapshot = library.snapshot();
    let diagnostics = Diagnostics::new(true);
    let read = |snapshot: &mut library::Snapshot, diagnostics: &Diagnostics| {
        artwork::capture(
            snapshot,
            "model.json",
            &BTreeSet::new(),
            diagnostics,
            &|| false,
        )
    };
    assert_eq!(
        read(&mut snapshot, &diagnostics)?.preview.parts[0]
            .scene
            .clips[0]
            .duration_seconds,
        2.
    );
    motion.duration_frames = 90.;
    write("idle.motion.json", &motion.encode()?)?;
    assert_eq!(
        read(&mut library.snapshot(), &diagnostics)?.preview.parts[0]
            .scene
            .clips[0]
            .duration_seconds,
        3.
    );
    assert_eq!(
        read(&mut snapshot, &diagnostics)?.preview.parts[0]
            .scene
            .clips[0]
            .duration_seconds,
        2.
    );
    assert!(
        artwork::capture(
            &mut snapshot,
            "model.json",
            &BTreeSet::from([0]),
            &diagnostics,
            &|| false
        )
        .is_err()
    );
    write("idle.motion.json", b"invalid motion")?;
    let tolerant = Diagnostics::default();
    assert!(read(&mut library.snapshot(), &tolerant).is_ok());
    assert_eq!(tolerant.entries().len(), 1);
    assert!(read(&mut library.snapshot(), &diagnostics).is_err());
    let calls = std::cell::Cell::new(0);
    let error = artwork::capture(
        &mut library.snapshot(),
        "model.json",
        &BTreeSet::new(),
        &tolerant,
        &|| {
            calls.set(calls.get() + 1);
            calls.get() > 3
        },
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("cancelled"));
    Ok(())
}
