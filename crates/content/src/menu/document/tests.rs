use super::*;

fn json(value: &impl Serialize) -> Result<serde_json::Value> {
    Ok(serde_json::from_slice(&serde_json::to_vec(value)?)?)
}

fn document() -> serde_json::Value {
    serde_json::json!({
        "fill": vec![0;4], "popup_fill": vec![0;4], "shade": vec![[0;4];2], "palette": super::super::sprites::COLORS.iter().map(|name| (*name,[0;4])).collect::<BTreeMap<_,_>>(),
        "patterns": ["ui/pattern.ktx2", "ui/pattern.ktx2", "ui/pattern.ktx2", "ui/pattern.ktx2", "ui/pattern.ktx2"],
        "images": {"icons": "ui/icons.ktx2"}, "labels": {},
        "windows": {"ornate": {"fill": "ui/fill.ktx2", "cursor_motion": [4.,0.13], "outset":3}},
        "sprites": {"rects": {"buttons": {"stick_neutral":[0,0,8,8]}}}
    })
}

#[test]
fn named_roles_use_image_dimensions_and_isolate_broken_optional_art() -> Result<()> {
    let mut source = document();
    source["sprites"]["rects"]["recipes"] = "broken".into();
    source["windows"]["gold"] = serde_json::json!({"fill": false});
    source["patterns"][1] = false.into();
    let diagnostics = Diagnostics::new(false);
    let mut art = MenuArt::decode(&serde_json::to_vec(&source)?, &diagnostics)?;
    assert_eq!(diagnostics.entries().len(), 3);
    let image = resonance_asset_writer::ktx2::encode_levels(32, 16, &[&vec![255; 32 * 16 * 4]])?;
    // Only the KTX2 header is needed; no pixel decoding or external lookup.
    art.prepare_resources(|_| Ok(&image[..80]));
    art.validate_structure()?;
    art.validate_window(1, 5)?;
    art.validate_window(1, 0)?;
    assert!(art.validate_window(1, 1).is_err());
    assert_eq!(art.sprite(Sprite::Buttons, 0)?, [0, 0, 8, 8]);
    assert!(art.validate_window(2, 0).is_err());
    assert!(art.sprite(Sprite::Recipes, 0).is_err());
    assert_eq!(art.texture(MenuArt::ATLAS_TEXTURE)?.height, 16);
    // A replacement image changes dimensions without a companion JSON edit.
    let small = resonance_asset_writer::ktx2::encode_levels(4, 4, &[&[255; 64]])?;
    art.prepare_resources(|path| {
        Ok(if path == "ui/icons.ktx2" {
            &small[..]
        } else {
            &image[..]
        })
    });
    assert!(art.sprite(Sprite::Buttons, 0).is_err());
    art.validate_window(1, 5)?;
    art.prepare_resources(|path| {
        if path == "ui/icons.ktx2" {
            anyhow::bail!("missing icon atlas")
        } else {
            Ok(&image[..])
        }
    });
    assert!(art.sprite(Sprite::Buttons, 0).is_err());
    art.validate_window(1, 5)?;
    assert!(MenuArt::decode(&serde_json::to_vec(&source)?, &Diagnostics::new(true)).is_err());
    Ok(())
}

#[test]
fn style_overrides_and_frames_round_trip_without_native_slots_or_sizes() -> Result<()> {
    let mut source = document();
    source["windows"]["plain"] = serde_json::json!({
        "fill": {"image":"ui/fill.ktx2", "repeat":true, "opaque":true},
        "frame": EDGES.into_iter().map(|name| (name, "ui/edge.ktx2")).collect::<BTreeMap<_,_>>(),
        "patterns": ["a.ktx2","b.ktx2","c.ktx2","d.ktx2","e.ktx2"],
        "heading": "ui/heading.ktx2", "cursor": "ui/cursor.ktx2", "cursor_motion": [2.,0.2], "outset":4
    });
    let art = MenuArt::decode(&serde_json::to_vec(&source)?, &Diagnostics::new(true))?;
    let encoded = json(&art)?;
    assert_eq!(encoded["windows"]["plain"], source["windows"]["plain"]);
    assert_eq!(encoded["images"], source["images"]);
    assert_eq!(encoded["patterns"], source["patterns"]);
    assert!(encoded.get("textures").is_none() && encoded.get("version").is_none());
    assert!(encoded["windows"]["ornate"].get("patterns").is_none());
    let decoded: MenuArt = serde_json::from_value(encoded.clone())?;
    assert_eq!(json(&decoded)?, encoded);
    source["version"] = 19.into();
    assert!(MenuArt::decode(&serde_json::to_vec(&source)?, &Diagnostics::new(true)).is_err());
    Ok(())
}

#[test]
#[ignore = "requires current library and recorded menu metadata; header reads only"]
fn installed_menu_preserves_every_image_window_sprite_and_caption() -> Result<()> {
    let root = std::path::PathBuf::from(
        std::env::var_os("RESONANCE_TEST_ASSETS").context("set RESONANCE_TEST_ASSETS")?,
    );
    let before = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../local/structure-review-20261027/menu-artwork/menu-before.json");
    let original: serde_json::Value = serde_json::from_slice(&std::fs::read(before)?)?;
    let current = std::fs::read(root.join("ui/menu.json"))?;
    let mut art: MenuArt = serde_json::from_slice(&current)?;
    art.prepare_resources(|path| Ok(std::fs::read(root.join(path))?));
    assert_eq!(json(&art.textures)?, original["textures"]);
    assert_eq!(json(&art.windows)?, original["windows"]);
    // Compare prepared drawing values, independently of today's named JSON.
    for (value, pointer) in [
        (json(&art.fill)?, "/fill"),
        (json(&art.popup_fill)?, "/popup_fill"),
        (json(&art.shade)?, "/shade"),
        (json(&art.palette)?, "/palette"),
        (json(&art.labels)?, "/labels"),
        (json(&art.sprites.rects)?, "/sprites/rects"),
        (json(&art.sprites.names)?, "/sprites/names"),
        (json(&art.sprites.number_colors)?, "/sprites/number_colors"),
        (json(&art.sprites.bar_colors)?, "/sprites/bar_colors"),
    ] {
        assert_eq!(&value, original.pointer(pointer).unwrap(), "{pointer}");
    }
    assert_eq!(
        json(&art)?,
        serde_json::from_slice::<serde_json::Value>(&current)?
    );
    eprintln!(
        "All 51 image bindings and actual sizes, three windows, sprites, palettes and captions preserved"
    );
    Ok(())
}
