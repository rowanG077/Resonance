use super::*;

fn document() -> serde_json::Value {
    serde_json::json!({
        "font":"fonts/dialogue.json", "symbols":"ui/symbols.ktx2", "shading":"ui/shading.ktx2",
        "patterns":["ui/a.ktx2","ui/b.ktx2","ui/c.ktx2","ui/d.ktx2","ui/e.ktx2"],
        "lined_fill":"ui/lines.ktx2", "plain_fill":"ui/white.ktx2", "cursor":"ui/cursor.ktx2",
        "default_style":"ornate", "selection":{"color":[112,144,24,128],
            "row_offsets":[0,5,9,12,14,12,9,5,0], "bob_amplitude":4.0,"bob_radians_per_tick":0.125}
    })
}
fn json(value: &impl Serialize) -> Result<serde_json::Value> {
    Ok(serde_json::from_slice(&serde_json::to_vec(value)?)?)
}

#[test]
fn dialogue_roles_and_styles_are_explicit_without_legacy_dimensions_or_versions() -> Result<()> {
    let mut source = document();
    for (mode, name) in ["plain", "ornate", "gold"].into_iter().enumerate() {
        source["default_style"] = name.into();
        let art: DialogueArt = serde_json::from_value(source.clone())?;
        assert_eq!(art.selection.mode, mode as u8);
        assert_eq!(art.textures[0].path, "ui/symbols.ktx2");
        assert_eq!(art.textures[1].path, "ui/shading.ktx2");
        assert_eq!(art.textures[6].path, "ui/e.ktx2");
        assert_eq!(art.textures[7].path, "ui/lines.ktx2");
        assert_eq!(art.textures[8].path, "ui/white.ktx2");
        assert_eq!(json(&art)?, source);
    }
    for (key, value) in [
        ("default_style", serde_json::json!(1)),
        (
            "symbols",
            serde_json::json!({"path":"ui/symbols.ktx2","width":32,"height":32}),
        ),
        ("cursor", serde_json::json!("../cursor.ktx2")),
        ("version", serde_json::json!(2)),
        ("textures", serde_json::json!([])),
    ] {
        let mut invalid = source.clone();
        invalid[key] = value;
        assert!(
            serde_json::from_value::<DialogueArt>(invalid).is_err(),
            "{key}"
        );
    }
    Ok(())
}

#[test]
fn required_dialogue_images_follow_replacement_headers_without_decoding_pixels() -> Result<()> {
    let mut art: DialogueArt = serde_json::from_value(document())?;
    let normal = resonance_asset_writer::ktx2::encode_levels(32, 16, &[&vec![255; 32 * 16 * 4]])?;
    let replacement =
        resonance_asset_writer::ktx2::encode_levels(64, 32, &[&vec![255; 64 * 32 * 4]])?;
    let original = json(&art)?;
    // Layout uses only the header. The image loader owns payload decoding.
    art.prepare_images(|_| Ok(&normal[..80]))?;
    assert!(
        art.textures
            .iter()
            .all(|image| [image.width, image.height] == [32, 16])
    );
    art.prepare_images(|path| {
        Ok(if path == "ui/symbols.ktx2" {
            &replacement[..80]
        } else {
            &normal[..80]
        })
    })?;
    assert_eq!([art.textures[0].width, art.textures[0].height], [64, 32]);
    assert_eq!([art.cursor.width, art.cursor.height], [32, 16]);
    assert_eq!(
        json(&art)?,
        original,
        "replacing an image needs no JSON dimension edit"
    );
    let error = art
        .prepare_images(|path| {
            if path == "ui/cursor.ktx2" {
                anyhow::bail!("missing")
            } else {
                Ok(&normal[..80])
            }
        })
        .unwrap_err();
    assert!(format!("{error:#}").contains("ui/cursor.ktx2"));
    assert!(art.prepare_images(|_| Ok(&normal[..40])).is_err());
    Ok(())
}

#[test]
#[ignore = "requires current library and saved dialogue metadata; ten header reads only"]
fn installed_dialogue_preserves_all_drawing_inputs_and_selection_behavior() -> Result<()> {
    let root = std::path::PathBuf::from(
        std::env::var_os("RESONANCE_TEST_ASSETS").context("set RESONANCE_TEST_ASSETS")?,
    );
    let evidence = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../local/structure-review-20261027/dialogue-artwork/dialogue-before.json");
    let original: serde_json::Value = serde_json::from_slice(&std::fs::read(evidence)?)?;
    let bytes = std::fs::read(root.join("ui/dialogue.json"))?;
    let mut art: DialogueArt = serde_json::from_slice(&bytes)?;
    art.prepare_images(|path| Ok(std::fs::read(root.join(path))?))?;
    assert_eq!(json(&art.textures)?, original["textures"]);
    assert_eq!(json(&art.cursor)?, original["cursor"]);
    assert_eq!(json(&art.selection)?, original["selection"]);
    assert_eq!(art.font, original["font"]);
    assert_eq!(
        json(&art)?,
        serde_json::from_slice::<serde_json::Value>(&bytes)?
    );
    eprintln!("All ten image roles/sizes, font binding and exact selection coefficients preserved");
    Ok(())
}
