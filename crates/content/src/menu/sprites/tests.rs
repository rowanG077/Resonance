use super::*;
use serde_json::json;

#[test]
fn named_menu_sprites_follow_shared_catalogue_edits_and_palette_defaults() -> Result<()> {
    let source = json!({"rects":{
        "item_images":{"wooden-sword":[1,2,32,32],"apple-gel":[3,4,32,32]},
        "recipes":{"sandwich":[5,6,32,32]},"buttons":{"a_pressed":[7,8,24,24]}},
        "number_colors":{"red":[[1,2,3,255],[4,5,6,255]],"shadow":[0,0,0,128]},
        "names":{"colette":"Chosen"}});
    let mut palette = vec![[255; 4]; 11];
    palette[1] = [1, 2, 3, 4];
    let mut art = MenuSprites::decode(source, &palette, &Diagnostics::new(true))?;
    let mut files = BTreeMap::from([
        (
            crate::items::PATH,
            json!({"mappings":{"0":"wooden-sword","1":"apple-gel"}}),
        ),
        (crate::cooking::PATH, json!({"mappings":{"0":"sandwich"}})),
        (
            "game/characters.json",
            json!({
                "mappings": ui_art::characters::NAMES.iter().enumerate().map(|(i,name)|((i+1).to_string(),*name)).collect::<BTreeMap<_,_>>(),
                "characters":ui_art::characters::NAMES.iter().map(|name|(*name,json!({"name":name}))).collect::<BTreeMap<_,_>>()
            }),
        ),
    ]);
    art.prepare_catalogues(&mut |path| Ok(serde_json::to_vec(&files[path])?));
    assert_eq!(
        art.group(Sprite::ItemImages)?,
        [[1, 2, 32, 32], [3, 4, 32, 32]]
    );
    assert_eq!(art.group(Sprite::Recipes)?, [[5, 6, 32, 32]]);
    assert_eq!(art.group(Sprite::Buttons)?[6], [7, 8, 24, 24]);
    assert_eq!(art.number_colors[1], [[1, 2, 3, 4]; 2]);
    assert_eq!(art.number_colors[2], [[1, 2, 3, 255], [4, 5, 6, 255]]);
    assert_eq!(art.names[1], "Chosen");
    let retained = art.clone();
    files.get_mut(crate::items::PATH).unwrap()["mappings"] =
        json!({"0":"apple-gel","1":"wooden-sword"});
    let characters = files.get_mut("game/characters.json").unwrap();
    characters["mappings"]["1"] = "renamed-hero".into();
    characters["characters"]["renamed-hero"] = json!({"name":"Hero"});
    characters["characters"]
        .as_object_mut()
        .unwrap()
        .remove("lloyd");
    art.prepare_catalogues(&mut |path| Ok(serde_json::to_vec(&files[path])?));
    assert_eq!(
        art.group(Sprite::ItemImages)?,
        [[3, 4, 32, 32], [1, 2, 32, 32]]
    );
    assert_eq!(art.names[0], "Hero");
    assert_eq!(retained.names[0], "lloyd");
    assert_eq!(retained.group(Sprite::ItemImages)?[0], [1, 2, 32, 32]);
    let encoded = art.encode(&palette)?;
    assert!(encoded["number_colors"].get("blue").is_none());
    assert_eq!(encoded["number_colors"]["shadow"], json!([0, 0, 0, 128]));
    assert_eq!(encoded["names"], json!({"colette":"Chosen"}));
    Ok(())
}

#[test]
fn invalid_sprite_groups_are_local_and_original_positional_rows_are_rejected() -> Result<()> {
    let source = json!({"rects":{"buttons":{"a_pressed":[0,0,8,8]},"recipes":false}});
    let diagnostics = Diagnostics::new(false);
    let art = MenuSprites::decode(source.clone(), &[[255; 4]; 11], &diagnostics)?;
    assert_eq!(diagnostics.entries().len(), 1);
    assert_eq!(art.group(Sprite::Buttons)?[6], [0, 0, 8, 8]);
    assert!(art.group(Sprite::Recipes).is_err());
    assert!(MenuSprites::decode(source, &[[255; 4]; 11], &Diagnostics::new(true)).is_err());
    for source in [
        json!({"rects":{"buttons":[[0,0,8,8]]}}),
        json!({"rects":{"buttons":{"invented":[0,0,8,8]}}}),
        json!({"names":{"invented":"name"}}),
    ] {
        assert!(MenuSprites::decode(source, &[[255; 4]; 11], &Diagnostics::new(true)).is_err());
    }
    Ok(())
}
