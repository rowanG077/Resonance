use super::*;
use serde_json::json;

fn mesh(names: &[&str]) -> Result<Vec<u8>> {
    resonance_asset_writer::gltf::pack_glb(
        &json!({
            "asset":{"version":"2.0"},
            "nodes": names.iter().enumerate().map(|(index,name)| json!({
                "name":name,"extras":{"resonance_bone":index}
            })).collect::<Vec<_>>()
        }),
        &[],
    )
}

#[test]
fn one_material_edit_updates_its_meshes_and_named_joints_follow_glb_edits() -> Result<()> {
    let mut value = json!({
        "mesh":"meshes/coat.glb",
        "textures":["textures/coat.ktx2", {"images":["textures/trim.ktx2"],"sampler":{"wrap":["clamp","repeat"]}}],
        "draws":[
            {"meshes":[0,2], "node":"coat", "recipe":[{"multiply_texture":0}],"textures":[{"image":0}]},
            {"meshes":[1], "draw_order":[7]},
            {"meshes":[null], "draw_order":[4], "node":"root", "recipe":"inherit"}
        ]
    });
    let mut initial: Scene = serde_json::from_value(value.clone())?;
    initial.prepare_bones(&mesh(&["root", "coat"])?)?;
    initial.validate()?;
    assert_eq!(
        initial
            .draws
            .iter()
            .map(|draw| (draw.mesh, draw.draw_order))
            .collect::<Vec<_>>(),
        [(None, 4), (Some(0), 0), (Some(1), 7), (Some(2), 2)]
    );
    assert_eq!(initial.draws[0].recipe, MaterialRecipe::Inherit);
    assert!(initial.draws[2].recipe.operations().is_empty());
    assert_ne!(initial.draws[2].recipe, MaterialRecipe::Inherit);
    value["draws"][0]["recipe"] = json!([{"replace_texture":0}]);
    value["draws"][0]["textures"][0]["image"] = json!(1);
    let mut edited: Scene = serde_json::from_value(value)?;
    edited.prepare_bones(&mesh(&["coat", "root"])?)?;
    edited.validate()?;
    for index in [1, 3] {
        assert_eq!(initial.draws[index].model_node, Some(1));
        assert_eq!(edited.draws[index].model_node, Some(0));
        assert_eq!(
            edited.draws[index].recipe.operations(),
            [MaterialOperation::ReplaceTexture(0)]
        );
        assert_eq!(edited.draws[index].textures[0].image, 1);
    }
    let serialized = serde_json::to_value(&edited)?;
    assert_eq!(serialized["draws"].as_array().unwrap().len(), 3);
    assert!(serialized.to_string().find("model_node").is_none());
    assert!(serialized["textures"][0].is_string());
    assert!(serialized["textures"][1]["sampler"].is_object());
    let mut roundtrip: Scene = serde_json::from_value(serialized)?;
    roundtrip.prepare_bones(&mesh(&["coat", "root"])?)?;
    assert_eq!(
        serde_json::to_value(&roundtrip.draws)?,
        serde_json::to_value(&edited.draws)?
    );
    let mut missing = edited.clone();
    assert!(missing.prepare_bones(&mesh(&["root", "other"])?).is_err());
    missing.prepare_bones(&mesh(&["coat", "coat", "root"])?)?;
    assert!(missing.validate().is_err());
    Ok(())
}

#[test]
fn grouped_materials_reject_ambiguous_assignments_and_keep_inheritance_explicit() -> Result<()> {
    for draws in [
        json!([{"meshes":[0,0]}]),
        json!([{"meshes":[0]},{"meshes":[0]}]),
        json!([{"meshes":[0,1],"draw_order":[4]}]),
        json!([{"meshes":[]}]),
        json!([{"meshes":[null]}]),
        json!([{"meshes":[0],"recipe":"guess"}]),
        json!([{"meshes":[0],"recipe":{"stage_count":0,"operations":[]}}]),
    ] {
        assert!(serde_json::from_value::<Scene>(json!({"mesh":"mesh.glb","draws":draws})).is_err());
    }
    let scene: Scene = serde_json::from_value(json!({"mesh":"mesh.glb","draws":[{"meshes":[0]}]}))?;
    assert!(scene.textures.is_empty());
    assert_ne!(scene.draws[0].recipe, MaterialRecipe::Inherit);
    let recipe: MaterialRecipe = serde_json::from_value(json!([
        {"decal_texture":0},"multiply_vertex_color",{"default_view_texture":2}
    ]))?;
    assert_eq!(recipe.textures().collect::<Vec<_>>(), [0, 2]);
    let inherited: MaterialRecipe = serde_json::from_value(json!("inherit"))?;
    assert_eq!(inherited, MaterialRecipe::Inherit);
    assert_eq!(serde_json::to_value(inherited)?, json!("inherit"));
    assert_eq!(serde_json::to_value(MaterialRecipe::default())?, json!([]));
    Ok(())
}

#[test]
fn image_banks_share_sampling_and_keep_real_variants_and_empty_slots() -> Result<()> {
    let value = json!({"sampler":{"wrap":["repeat","repeat"]},"entries":[
        "a.ktx2",null,{"images":["b.ktx2","b-night.ktx2"]},
        {"sampler":{},"images":["clamped.ktx2"]}
    ]});
    let bank: TextureCatalogue = serde_json::from_value(value)?;
    bank.validate()?;
    assert_eq!(
        bank.textures[0].as_ref().unwrap().sampler.wrap,
        [crate::TextureWrap::Repeat; 2]
    );
    assert!(bank.textures[1].is_none());
    assert_eq!(bank.textures[2].as_ref().unwrap().images.len(), 2);
    assert_eq!(
        bank.textures[3].as_ref().unwrap().sampler.wrap,
        [crate::TextureWrap::Clamp; 2]
    );
    let edited = serde_json::to_value(&bank)?;
    let again: TextureCatalogue = serde_json::from_value(edited)?;
    for (a, b) in bank
        .textures
        .iter()
        .flatten()
        .zip(again.textures.iter().flatten())
    {
        assert_eq!(a.sampler, b.sampler);
        assert_eq!(a.images, b.images);
    }
    let common: TextureCatalogue = serde_json::from_value(json!([
        {"sampler":{"wrap":["repeat","repeat"]},"images":["a.ktx2"]},
        {"sampler":{"wrap":["repeat","repeat"]},"images":["b.ktx2"]}
    ]))?;
    assert_eq!(
        serde_json::to_value(common)?,
        json!({"sampler":{"wrap":["repeat","repeat"]},"entries":["a.ktx2","b.ktx2"]})
    );
    assert!(serde_json::from_value::<TextureCatalogue>(json!({"entries":["a.ktx2"]})).is_err());
    let image = resonance_asset_writer::ktx2::encode_rgba8(2, 1, &[255; 8])?;
    let direct = TextureCatalogue::load("title.ktx2", |_| Ok(&image))?;
    assert_eq!(direct.textures[0].as_ref().unwrap().images, ["title.ktx2"]);
    Ok(())
}
