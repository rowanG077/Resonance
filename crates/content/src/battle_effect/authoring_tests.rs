use super::*;
use crate::{
    battle_scene::document as scenes,
    diagnostics::Diagnostics,
    library::{Library, Mod},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{fs, path::Path};

#[test]
fn visual_authoring_named_blends_atlases_and_image_mods_preserve_prepared_choices() -> Result<()> {
    let mut declaration: declaration::Declaration =
        serde_json::from_str(include_str!("../../tests/fixtures/material-particle.json"))?;
    for (mode, name) in [(0, "alpha"), (1, "additive"), (2, "subtractive")] {
        if let declaration::Declaration::Particle { visual, .. } = &mut declaration {
            visual.blend = mode;
        }
        let mut stored = serde_json::to_value(&declaration)?;
        stored["visual"]["blend"] = name.into();
        let prepared: declaration::Declaration = serde_json::from_value(stored.clone())?;
        assert_eq!(prepared.particle_visual()?.blend, mode);
        stored["visual"]["blend"] = mode.into();
        assert!(serde_json::from_value::<declaration::Declaration>(stored).is_err());
    }
    let root = tempfile::tempdir()?;
    let modification = tempfile::tempdir()?;
    fs::write(
        root.path().join("content.json"),
        serde_json::to_vec(
            &json!({"version":crate::library::VERSION,"game":crate::library::GAME}),
        )?,
    )?;
    let bank = json!({"templates":{"stun-bird":declaration},"artwork":{"textures":{"scene":[{"images":["atlas.ktx2"],"palette_sets":[[0,null],0]}]}}});
    fs::write(root.path().join("effects.json"), serde_json::to_vec(&bank)?)?;
    fs::write(
        root.path().join("atlas.ktx2"),
        resonance_asset_writer::ktx2::encode_rgba8(2, 1, &[255; 8])?,
    )?;
    fs::write(
        modification.path().join("atlas.ktx2"),
        resonance_asset_writer::ktx2::encode_rgba8(4, 2, &[255; 32])?,
    )?;
    let library = Library::open(root.path(), &[])?;
    let mut retained = library.snapshot();
    let diagnostics = Diagnostics::new(true);
    let before = document::capture(&mut retained, "effects.json", &diagnostics, &|| false)?;
    let changed = Library::open(
        root.path(),
        &[Mod {
            name: "atlas resize".into(),
            root: modification.path().into(),
        }],
    )?;
    let after = document::capture(
        &mut changed.snapshot(),
        "effects.json",
        &diagnostics,
        &|| false,
    )?;
    let texture = |bank: &SourceBank| bank.art.as_ref().unwrap().textures[&6][0].clone();
    let a = texture(&before);
    let b = texture(&after);
    assert_eq!((a.images[0].width, a.images[0].height), (2, 1));
    assert_eq!((b.images[0].width, b.images[0].height), (4, 2));
    assert!(b.palette_image(0, 1).is_err());
    assert_eq!(b.palette_image(1, 255)?, 0);
    assert_eq!(serde_json::to_value(&a)?, serde_json::to_value(&b)?);
    assert_eq!(
        texture(&document::capture(
            &mut retained,
            "effects.json",
            &diagnostics,
            &|| false
        )?)
        .images[0]
            .width,
        2
    );
    let event: EffectOperation =
        serde_json::from_value(json!({"kind":"spawn","particle":0,"blend":"alpha"}))?;
    assert!(matches!(
        event,
        EffectOperation::Spawn { blend: Some(0), .. }
    ));
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_ASSETS; reads current battle metadata without media conversion"]
fn installed_banks_and_scenes_roundtrip_current_metadata() -> Result<()> {
    let root = std::env::var_os("RESONANCE_TEST_ASSETS").context("set RESONANCE_TEST_ASSETS")?;
    let library = Library::open(Path::new(&root), &[])?;
    let mut snapshot = library.snapshot();
    let paths = snapshot
        .files_under("battle")?
        .iter()
        .filter(|path| path.ends_with("/effects.json") || path.ends_with("/scene.json"))
        .cloned()
        .collect::<Vec<_>>();
    for required in [document::COMMON, document::TECHNIQUES] {
        ensure!(
            paths.iter().any(|path| path == required),
            "missing bank {required}"
        );
    }
    let mut banks = 0;
    let mut scenes = 0;
    let mut templates = 0;
    let mut events = 0;
    for path in paths {
        if path.ends_with("/effects.json") {
            let definition: document::Definition =
                snapshot.json(&path, crate::preparation::JSON_LIMIT)?;
            let stored: Value = serde_json::from_slice(&serde_json::to_vec(&definition)?)?;
            let roundtrip: document::Definition = serde_json::from_value(stored.clone())?;
            ensure!(
                definition.programs == roundtrip.programs
                    && definition.templates == roundtrip.templates,
                "bank roundtrip changed: {path}"
            );
            let republished: Value = serde_json::from_slice(&serde_json::to_vec(&roundtrip)?)?;
            ensure!(stored == republished, "bank republication changed: {path}");
            if path == document::COMMON {
                ensure!(
                    definition
                        .template_names
                        .iter()
                        .any(|name| name == document::STUN_BIRD),
                    "direct stun bird missing"
                );
            }
            banks += 1;
            templates += definition.templates.len();
            events += definition.programs.iter().map(Vec::len).sum::<usize>();
        } else {
            let scene: scenes::Definition = snapshot.json(&path, crate::preparation::JSON_LIMIT)?;
            let roundtrip: scenes::Definition =
                serde_json::from_value(serde_json::to_value(&scene)?)?;
            ensure!(
                serde_json::to_value(scene)? == serde_json::to_value(roundtrip)?,
                "scene roundtrip changed: {path}"
            );
            scenes += 1;
        }
    }
    ensure!(scenes > 0, "library has no stored scenes");
    eprintln!(
        "{banks} banks, {templates} templates, {events} expanded events and {scenes} scenes roundtrip current metadata"
    );
    Ok(())
}
