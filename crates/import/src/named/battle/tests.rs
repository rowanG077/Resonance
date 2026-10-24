use super::super::tests::{canonicalize, first_difference};
use super::*;
use resonance_content::{
    animation::Motion,
    diagnostics::Diagnostics,
    library::{Library, Mod},
};

#[test]
fn arena_preparation_follows_current_files_without_inventory_and_keeps_snapshots() -> Result<()> {
    let output = tempfile::tempdir()?;
    let write = |path: &str, bytes: &[u8]| -> Result<()> {
        fs::create_dir_all(output.path().join(path).parent().unwrap())?;
        Ok(fs::write(output.path().join(path), bytes)?)
    };
    write(
        "content.json",
        br#"{"version":1,"game":"GQSEAF","revision":0}"#,
    )?;
    write(arenas::MAPPINGS, br#"{"2":"forest"}"#)?;
    let path = "battle/arenas/forest/arena.json";
    let layer = "battle/arenas/forest/layer.json";
    let mesh = "battle/arenas/forest/trees.glb";
    let clip = "battle/arenas/forest/sway.motion.json";
    let definition = serde_json::json!({
        "version": 1, "unsupported_reason": null,
        "light_position": [0, 0, 0], "chain_acceleration": [0, 0, 0],
        "actor_color": [64, 64, 64, 255], "translation": [0, 0, 0],
        "yaw_degrees": 0, "color": [64, 64, 64, 255], "camera_pitch_offset": 0,
        "layers": {"0": layer}, "scenery_bones": {}, "scenery_fade": null,
        "animation_rates": {"0": 0.5}, "texture_animations": {}, "fog": null,
        "effects": null, "recurring_interval": null, "textures": []
    });
    let model = serde_json::json!({
        "version": 1, "model": {
            "scene": {
                "resource": 0, "mesh": mesh, "textures": [], "materials": [],
                "translation": [0, 0, 0], "autoplay": true, "texture_animations": [],
                "clips": [{"motion": clip, "resource_slot": 7, "duration_seconds": 999}]
            },
            "animation": 7, "attached_to": null, "additive": false
        }
    });
    write(path, &serde_json::to_vec(&definition)?)?;
    write(layer, &serde_json::to_vec(&model)?)?;
    write(mesh, b"geometry is decoded by model admission")?;
    let mut motion = Motion {
        duration_frames: 60.,
        tracks: vec![],
    };
    write(clip, &motion.encode()?)?;
    let library = Library::open(output.path(), &[])?;
    let mut snapshot = library.snapshot();
    let diagnostics = Diagnostics::new(true);
    let original = arenas::capture(&mut snapshot, 2, &diagnostics, &|| false)?;
    assert_eq!(original.layers[&0].scene.clips[0].duration_seconds, 2.);

    assert!(original.source_sha256.is_empty());
    motion.duration_frames = 180.;
    write(clip, &motion.encode()?)?;
    assert_eq!(
        arenas::capture(&mut library.snapshot(), 2, &diagnostics, &|| false)?.layers[&0]
            .scene
            .clips[0]
            .duration_seconds,
        6.
    );
    assert_eq!(
        arenas::capture(&mut snapshot, 2, &diagnostics, &|| false)?.layers[&0]
            .scene
            .clips[0]
            .duration_seconds,
        2.
    );
    let mut invalid = definition.clone();
    invalid["animation_rates"]["0"] = serde_json::json!(-1);
    write(path, &serde_json::to_vec(&invalid)?)?;
    assert!(
        format!(
            "{:#}",
            arenas::capture(&mut library.snapshot(), 2, &diagnostics, &|| false).unwrap_err()
        )
        .contains("animation rate")
    );
    invalid = definition.clone();
    invalid["files"] = serde_json::json!({});
    write(path, &serde_json::to_vec(&invalid)?)?;
    assert!(
        format!(
            "{:#}",
            arenas::capture(&mut library.snapshot(), 2, &diagnostics, &|| false).unwrap_err()
        )
        .contains("unknown field")
    );
    write(path, &serde_json::to_vec(&definition)?)?;
    fs::remove_file(output.path().join(mesh))?;
    let error = arenas::capture(&mut library.snapshot(), 2, &diagnostics, &|| false).unwrap_err();
    assert!(format!("{error:#}").contains(mesh));
    assert!(arenas::capture(&mut snapshot, 2, &diagnostics, &|| false).is_ok());
    assert!(arenas::capture(&mut snapshot, 2, &Diagnostics::default(), &|| true).is_err());
    Ok(())
}

#[test]
#[ignore = "requires original disc 1 and RESONANCE_TEST_ASSETS; compares native arenas and current shared effects"]
fn original_arenas_roundtrip_current_dependencies_and_scoped_edits() -> Result<()> {
    let source = std::env::var_os("RESONANCE_TEST_ASSETS")
        .map(std::path::PathBuf::from)
        .context("set RESONANCE_TEST_ASSETS")?;
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let native = tempfile::tempdir()?;
    let output = tempfile::tempdir()?;
    let prepared = crate::battle_stage::prepare_all(&extracted, native.path())?;
    let ids = prepared.keys().copied().collect::<BTreeSet<_>>();
    assert_eq!(ids.len(), 86);
    let mut publisher = Publisher::new(native.path(), output.path())?;
    publisher.battle_arenas(prepared.clone())?;
    publisher.descriptor("GQSEAF", 0)?;
    let diagnostics = Diagnostics::new(true);
    let mut original_snapshot = Library::open(&source, &[])?.snapshot();
    let mut shared = Publisher::new(&source, output.path())?;
    let mut shared_effects = BTreeMap::new();
    for path in [effects::COMMON, effects::TECHNIQUES] {
        let before = effects::capture(&mut original_snapshot, path, &diagnostics, &|| false)?;
        shared.battle_effect_bank(path.strip_suffix("/effects.json").unwrap(), before.clone())?;
        shared_effects.insert(path, before);
    }
    let library = Library::open(output.path(), &[])?;
    let mut snapshot = library.snapshot();
    let diagnostics = Diagnostics::new(true);
    let mut digests = BTreeMap::new();
    for (path, before) in shared_effects {
        let after = effects::capture(&mut snapshot, path, &diagnostics, &|| false)?;
        equivalent(&before, &after, &source, output.path(), &mut digests)?;
    }
    let mut layers = 0;
    let mut clips = 0;
    for id in &ids {
        let mut original = prepared[id].stage.clone();
        let mut restored = arenas::capture(&mut snapshot, *id, &diagnostics, &|| false)?;
        original.validate()?;
        original.source_sha256.clear();

        if let (Some(before), Some(new)) = (&prepared[id].effects, &restored.effects) {
            let mut before = before.clone();
            before.source_sha256.clear();
            if let Some(art) = &mut before.art {
                art.source_sha256.clear();
            }
            let after = effects::read(new, &diagnostics, |path| {
                snapshot.read(path, 128 * 1024 * 1024)
            })?;
            equivalent(&before, &after, native.path(), output.path(), &mut digests)?;
            if let Some(art) = &after.art {
                assert_eq!(
                    restored.textures.len(),
                    art.textures.get(&9).map_or(0, Vec::len)
                );
                for (stage, effect) in restored
                    .textures
                    .iter()
                    .zip(art.textures.get(&9).into_iter().flatten())
                {
                    assert_eq!(
                        stage
                            .images
                            .iter()
                            .map(|image| &image.path)
                            .collect::<Vec<_>>(),
                        effect
                            .images
                            .iter()
                            .map(|image| &image.path)
                            .collect::<Vec<_>>()
                    );
                }
            }
            original.effects = None;
            restored.effects = None;
        }
        layers += restored.layers.len();
        clips += restored
            .layers
            .values()
            .map(|layer| layer.scene.clips.len())
            .sum::<usize>();
        equivalent(
            &original,
            &restored,
            native.path(),
            output.path(),
            &mut digests,
        )?;
    }
    assert!(!output.path().join("assets").exists());
    assert!(!output.path().join("battle/stages").exists());
    for path in publisher
        .published
        .keys()
        .filter(|path| path.ends_with(".json"))
    {
        let value: serde_json::Value =
            serde_json::from_slice(&fs::read(output.path().join(path))?)?;
        no_inventory(&value, path);
    }

    // A mapping-only rename and a changed animation reference inherit every
    // untouched resource. The old captured view stays unchanged.
    let original = arenas::capture(&mut snapshot, 2, &diagnostics, &|| false)?;
    let modification = tempfile::tempdir()?;
    let write = |path: &str, bytes: &[u8]| -> Result<()> {
        fs::create_dir_all(modification.path().join(path).parent().unwrap())?;
        Ok(fs::write(modification.path().join(path), bytes)?)
    };
    write(arenas::MAPPINGS, br#"{"2":"edited-arena"}"#)?;
    let old_path = snapshot.resolve(arenas::MAPPINGS, 2, Some("arena.json"))?;
    let mut definition: arenas::Definition = snapshot.json(&old_path, 16 * 1024 * 1024)?;
    let slot = *original
        .layers
        .iter()
        .find(|(_, layer)| !layer.scene.clips.is_empty())
        .context("arena 2 needs a native animation")?
        .0;
    let layer_path = definition.layers[&slot].clone();
    let mut layer: arenas::Layer = snapshot.json(&layer_path, 16 * 1024 * 1024)?;
    let old_motion = layer.model.scene.clips[0].motion.clone();
    let mut motion = Motion::decode(&snapshot.read(&old_motion, 16 * 1024 * 1024)?)?;
    motion.duration_frames += 120.;
    layer.model.scene.clips[0].motion = "battle/arenas/edited-arena/longer.motion.json".into();
    write(&layer.model.scene.clips[0].motion, &motion.encode()?)?;
    let edited_layer = "battle/arenas/edited-arena/layer.json";
    write(edited_layer, &serde_json::to_vec(&layer)?)?;
    definition.layers.insert(slot, edited_layer.into());
    let edited_entry = "battle/arenas/edited-arena/arena.json";
    write(edited_entry, &serde_json::to_vec(&definition)?)?;
    let modified = Library::open(
        output.path(),
        &[Mod {
            name: "arena edit".into(),
            root: modification.path().into(),
        }],
    )?;
    let mut edited_snapshot = modified.snapshot();
    let edited = arenas::capture(&mut edited_snapshot, 2, &diagnostics, &|| false)?;
    assert_eq!(
        edited.layers[&slot].scene.clips[0].duration_seconds,
        motion.duration_frames / resonance_content::animation::FRAME_HZ
    );
    assert_eq!(
        arenas::capture(&mut snapshot, 2, &diagnostics, &|| false)?.layers[&slot]
            .scene
            .clips[0]
            .motion,
        old_motion
    );
    assert!(arenas::capture(&mut modified.snapshot(), 2, &diagnostics, &|| true).is_err());
    // No missing override fallback, even though the old layer still exists.
    let missing = "battle/arenas/edited-arena/missing.glb";
    layer.model.scene.mesh = missing.into();
    write(edited_layer, &serde_json::to_vec(&layer)?)?;
    let error = arenas::capture(&mut modified.snapshot(), 2, &diagnostics, &|| false).unwrap_err();
    assert!(format!("{error:#}").contains(missing));
    assert!(arenas::capture(&mut edited_snapshot, 2, &diagnostics, &|| false).is_ok());
    eprintln!(
        "verified {} arenas, {layers} layers, {clips} clips, {} named files",
        ids.len(),
        publisher.published.len()
    );
    Ok(())
}

#[test]
fn optional_effect_art_keeps_healthy_models_and_cancellation_is_fatal() -> Result<()> {
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
        br#"{"version":1,"programs":"programs.json","actors":"particles.json","art":"art.json"}"#,
    )?;
    write("programs.json", b"[[]]")?;
    write("particles.json", b"[]")?;
    write(
        "art.json",
        br#"{"version":1,"textures":{},"models":{"0":"missing.json","1":"healthy.json"}}"#,
    )?;
    write(
        "healthy.json",
        &serde_json::to_vec(&serde_json::json!({
            "version": 1, "model": {
                "rig": {"skeleton": {"bones": [{"name":"root", "parent":null, "bind_channels":0,
                    "bind":{"translation":[0,0,0], "rotation":[0,0,0,1], "scale":[1,1,1]}}]}, "attachments":{}},
                "layers": [{"scene": {"resource":0, "mesh":"model.glb", "textures":["missing.png"],
                    "materials":[], "translation":[0,0,0], "clips":[], "autoplay":false,
                    "texture_animations":[], "bone_names":["root"]}, "animation":null,
                    "attached_to":null, "additive":false}]
            }
        }))?,
    )?;
    write(
        "model.glb",
        b"geometry is decoded by runtime model admission",
    )?;
    let library = Library::open(output.path(), &[])?;
    let diagnostics = Diagnostics::default();
    let bank = effects::capture(&mut library.snapshot(), "bank.json", &diagnostics, &|| {
        false
    })?;
    assert_eq!(
        bank.art.unwrap().models.keys().copied().collect::<Vec<_>>(),
        [1]
    );
    let errors = diagnostics.entries();
    assert_eq!(errors.len(), 2);
    assert!(
        errors
            .iter()
            .any(|error| error.message.contains("missing.json"))
    );
    assert!(
        errors
            .iter()
            .any(|error| error.message.contains("missing.png"))
    );
    assert!(
        effects::capture(
            &mut library.snapshot(),
            "bank.json",
            &Diagnostics::new(true),
            &|| false
        )
        .is_err()
    );
    let calls = std::cell::Cell::new(0);
    let cancelled = || {
        calls.set(calls.get() + 1);
        calls.get() > 4
    };
    let error = effects::capture(
        &mut library.snapshot(),
        "bank.json",
        &Diagnostics::default(),
        &cancelled,
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("cancelled"));
    Ok(())
}

pub(crate) fn equivalent(
    before: &impl Serialize,
    after: &impl Serialize,
    source: &Path,
    output: &Path,
    digests: &mut BTreeMap<String, String>,
) -> Result<()> {
    let mut before = serde_json::to_value(before)?;
    let mut after = serde_json::to_value(after)?;
    canonicalize(&mut before, source, output, digests)?;
    canonicalize(&mut after, source, output, digests)?;
    if let Some(difference) = first_difference(&before, &after, "") {
        anyhow::bail!("named resource differs from original: {difference}");
    }
    Ok(())
}

pub(in crate::named) fn no_inventory(value: &serde_json::Value, path: &str) {
    match value {
        serde_json::Value::Object(entries) => {
            for (key, value) in entries {
                assert!(
                    !matches!(key.as_str(), "files" | "source_sha256" | "sha256"),
                    "authoring inventory in {path}"
                );
                no_inventory(value, path);
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                no_inventory(value, path);
            }
        }
        _ => {}
    }
}
