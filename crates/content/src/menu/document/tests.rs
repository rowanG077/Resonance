use super::*;

fn json(value: &impl Serialize) -> Result<serde_json::Value> {
    Ok(serde_json::from_slice(&serde_json::to_vec(value)?)?)
}

fn image(path: &str) -> serde_json::Value {
    serde_json::json!({"image":path,"size":[32,16]})
}

fn document() -> serde_json::Value {
    serde_json::json!({
        "fill": vec![0;4], "popup_fill": vec![0;4], "shade": vec![[0;4];2], "palette": super::super::sprites::COLORS.iter().map(|name| (*name,[0;4])).collect::<BTreeMap<_,_>>(),
        "patterns": [image("ui/pattern.ktx2"), image("ui/pattern.ktx2"), image("ui/pattern.ktx2"), image("ui/pattern.ktx2"), image("ui/pattern.ktx2")],
        "images": {"icons": image("ui/icons.ktx2")}, "labels": {},
        "windows": {"ornate": {"fill": image("ui/fill.ktx2"), "cursor_motion": [4.,0.13], "outset":3}},
        "sprites": {"rects": {"buttons": {"stick_neutral":[0,0,8,8]}}}
    })
}

#[test]
fn named_roles_use_document_dimensions_and_isolate_broken_optional_art() -> Result<()> {
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
    // Increasing physical resolution preserves the JSON coordinate system.
    let hd = resonance_asset_writer::ktx2::encode_levels(128, 64, &[&vec![255; 128 * 64 * 4]])?;
    art.prepare_resources(|path| {
        Ok(if path == "ui/icons.ktx2" {
            &hd[..]
        } else {
            &image[..]
        })
    });
    assert_eq!(art.sprite(Sprite::Buttons, 0)?, [0, 0, 8, 8]);
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
fn style_overrides_and_frames_round_trip_with_coordinate_sizes() -> Result<()> {
    let mut source = document();
    source["windows"]["plain"] = serde_json::json!({
        "fill": {"image":"ui/fill.ktx2", "size":[32,16], "repeat":true, "opaque":true},
        "frame": EDGES.into_iter().map(|name| (name, image("ui/edge.ktx2"))).collect::<BTreeMap<_,_>>(),
        "patterns": [image("a.ktx2"),image("b.ktx2"),image("c.ktx2"),image("d.ktx2"),image("e.ktx2")],
        "heading": image("ui/heading.ktx2"), "cursor": image("ui/cursor.ktx2"), "cursor_motion": [2.,0.2], "outset":4
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
fn independent_item_images_prepare_once_and_do_not_replace_the_shared_atlas() -> Result<()> {
    let mut source = document();
    source["sprites"]["rects"]["item_images"] = serde_json::json!({
        "gel":{"image":"mod/gel.ktx2","size":[64,32],"rect":[0,0,64,32]},
        "berry":{"image":"ui/icons.ktx2","size":[32,16],"rect":[0,0,8,8]}
    });
    let mut art = MenuArt::decode(&serde_json::to_vec(&source)?, &Diagnostics::new(true))?;
    let image = resonance_asset_writer::ktx2::encode_levels(32, 16, &[&vec![255; 32 * 16 * 4]])?;
    let catalogue = serde_json::to_vec(&serde_json::json!({"mappings":{"0":"gel","1":"berry"}}))?;
    let read = |path: &str| -> Result<&[u8]> {
        Ok(if path == crate::items::PATH {
            &catalogue
        } else {
            &image
        })
    };
    art.prepare_resources(read);
    assert_eq!(art.sprite(Sprite::ItemImages, 0)?, [0, 0, 64, 32]);
    let texture = art.sprite_texture(Sprite::ItemImages, 0);
    assert_ne!(texture, MenuArt::ATLAS_TEXTURE);
    assert_eq!(art.texture(texture)?.path, "mod/gel.ktx2");
    assert_eq!(
        art.sprite_texture(Sprite::ItemImages, 1),
        MenuArt::ATLAS_TEXTURE
    );
    let count = art.textures.len();
    art.prepare_resources(read);
    assert_eq!(art.textures.len(), count);
    assert_eq!(art.texture(MenuArt::ATLAS_TEXTURE)?.path, "ui/icons.ktx2");
    assert_eq!(art.sprite(Sprite::ItemImages, 1)?, [0, 0, 8, 8]);
    source["sprites"]["rects"]["item_images"]["gel"]["unknown"] = true.into();
    assert!(MenuArt::decode(&serde_json::to_vec(&source)?, &Diagnostics::new(true)).is_err());
    source["sprites"]["rects"]["item_images"]["bad-size"] = serde_json::json!({
        "image":"mod/bad.ktx2","size":[0,32],"rect":[0,0,8,8]
    });
    let diagnostics = Diagnostics::default();
    let mut tolerant = MenuArt::decode(&serde_json::to_vec(&source)?, &diagnostics)?;
    tolerant.prepare_resources(read);
    assert!(tolerant.sprite(Sprite::ItemImages, 0).is_err());
    assert_eq!(tolerant.sprite(Sprite::ItemImages, 1)?, [0, 0, 8, 8]);
    Ok(())
}
