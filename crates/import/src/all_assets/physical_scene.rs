//! Authored meshes and material recipes, independent of renderer capabilities.
use crate::{geometry, geometry::MaterialRecipe, write_atomic};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

#[derive(Serialize, Deserialize)]
pub(crate) struct Scene {
    pub(crate) mesh: String,
    pub(crate) bone_names: Vec<String>,
    pub(crate) textures: TextureSource,
    pub(crate) draws: Vec<Draw>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum TextureSource {
    Local { catalogue: String },
    Caller,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Draw {
    pub(crate) source_index: usize,
    /// Empty authored draws retain their metadata and hierarchy without a mesh.
    pub(crate) mesh: Option<usize>,
    pub(crate) model_node: Option<usize>,
    pub(crate) draw_order: u32,
    pub(crate) name: String,
    pub(crate) index_count: usize,
    pub(crate) color: [f32; 4],
    /// Exporter alpha classification, not an authored blend-state command.
    /// Runtime binding must recalculate alpha when caller textures become known.
    pub(crate) preview_blend: bool,
    pub(crate) recipe: MaterialRecipe,
    pub(crate) textures: Vec<Texture>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Texture {
    pub(crate) stage: u8,
    /// Authored table label, never a filesystem path.
    pub(crate) table: Option<String>,
    pub(crate) image: u16,
    pub(crate) wrap: [resonance_content::TextureWrap; 2],
    pub(crate) min_filter: crate::tpl::Filter,
    pub(crate) mag_filter: crate::tpl::Filter,
}

pub(crate) fn cook(
    bytes: &[u8],
    name: &str,
    output: &Path,
    textures: TextureSource,
) -> Result<Scene> {
    let geometry::DecodedGeometry {
        manifest,
        mut gltf,
        mut binary,
    } = geometry::decode_section(bytes, geometry::DecodeMode::Physical, |_, _| Ok(()))?;
    let mut scene = from_geometry(&manifest, &mut gltf, textures)?;
    let glb = crate::scene::pack_glb(&gltf, &mut binary)?;
    scene.mesh = format!("{name}/{}.glb", crate::digest(&glb));
    write_atomic(&output.join(&scene.mesh), &glb)?;
    Ok(scene)
}

/// Keep every decoded attribute, topology, skin and node. Only material preview
/// objects and proven empty mesh bindings leave the geometry-only GLB.
pub(crate) fn from_geometry(
    manifest: &geometry::GeometryMetadata,
    gltf: &mut Value,
    textures: TextureSource,
) -> Result<Scene> {
    let local = matches!(textures, TextureSource::Local { .. });
    ensure!(
        local || manifest.textures.is_empty(),
        "caller-bound model has local images"
    );
    let meshes = gltf["meshes"].as_array().context("meshes")?;
    ensure!(
        meshes.len() == manifest.objects.len(),
        "scene lost a decoded draw"
    );
    let mut draws = Vec::with_capacity(meshes.len());
    let mut next_mesh = 0;
    for (index, (mesh, object)) in meshes.iter().zip(&manifest.objects).enumerate() {
        let mut count = 0_u64;
        for primitive in mesh["primitives"].as_array().context("mesh primitives")? {
            let accessor = primitive["indices"].as_u64().context("index accessor")? as usize;
            count = count
                .checked_add(
                    gltf["accessors"][accessor]["count"]
                        .as_u64()
                        .context("index count")?,
                )
                .context("index count overflow")?;
        }
        ensure!(
            object.index == index && count == object.index_count as u64,
            "scene lost decoded indices for draw {index}"
        );
        let template = gltf["materials"]
            .get(if local { object.material } else { 0 })
            .context("material template")?;
        let color: [f32; 4] = template["pbrMetallicRoughness"]
            .get("baseColorFactor")
            .map(|value| serde_json::from_value(value.clone()))
            .transpose()?
            .unwrap_or([1.; 4]);
        ensure!(
            color.iter().all(|value| value.is_finite()),
            "nonfinite material color"
        );
        let recipe = MaterialRecipe::parse(&object.tev_modes, object.texture_tables.len())?;
        let bindings = texture_bindings(object)?;
        ensure!(
            recipe
                .textures()
                .all(|stage| bindings.iter().any(|binding| binding.stage == stage)),
            "material is missing a texture stage"
        );
        ensure!(
            !local
                || bindings
                    .iter()
                    .all(|binding| usize::from(binding.image) < manifest.textures.len()),
            "material image exceeds local texture catalogue"
        );
        let mesh = (count != 0).then(|| {
            let mesh = next_mesh;
            next_mesh += 1;
            mesh
        });
        draws.push(Draw {
            source_index: object.source_index,
            mesh,
            model_node: object.model_node,
            draw_order: object.draw_order,
            name: object.name.clone(),
            index_count: object.index_count,
            color,
            preview_blend: template["alphaMode"] == "BLEND",
            recipe,
            textures: bindings,
        });
    }
    for node in gltf["nodes"].as_array_mut().context("nodes")? {
        if let Some(mesh) = node["mesh"].as_u64() {
            match draws
                .get(mesh as usize)
                .context("node mesh exceeds draw table")?
                .mesh
            {
                Some(mesh) => node["mesh"] = mesh.into(),
                None => {
                    node.as_object_mut().context("node")?.remove("mesh");
                }
            }
        }
    }
    let meshes = gltf["meshes"].as_array_mut().context("meshes")?;
    let mut index = 0;
    meshes.retain(|_| {
        let keep = draws[index].mesh.is_some();
        index += 1;
        keep
    });
    for mesh in meshes {
        for primitive in mesh["primitives"]
            .as_array_mut()
            .context("mesh primitives")?
        {
            primitive
                .as_object_mut()
                .context("mesh primitive")?
                .remove("material");
        }
    }
    for key in ["materials", "images", "textures", "samplers", "extras"] {
        gltf.as_object_mut().context("scene")?.remove(key);
    }
    gltf["buffers"][0]
        .as_object_mut()
        .context("mesh buffer")?
        .remove("uri");
    Ok(Scene {
        mesh: String::new(),
        bone_names: manifest
            .model_nodes
            .iter()
            .map(|node| node.name.clone())
            .collect(),
        textures,
        draws,
    })
}

fn texture_bindings(object: &geometry::GeometryObjectInfo) -> Result<Vec<Texture>> {
    object
        .texture_commands
        .iter()
        .map(|&command| {
            let stage = ((command >> 13) & 7) as u8;
            ensure!(command >> 28 <= 1, "invalid material magnification filter");
            Ok(Texture {
                stage,
                table: object
                    .texture_tables
                    .get(usize::from(stage))
                    .context("texture stage has no descriptor")?
                    .clone(),
                image: (command & 0x1fff) as u16,
                wrap: [
                    crate::tpl::wrap((command >> 16) & 15)?,
                    crate::tpl::wrap((command >> 20) & 15)?,
                ],
                min_filter: crate::tpl::filter((command >> 24) & 15)?,
                mag_filter: crate::tpl::filter(command >> 28)?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn physical_scene_keeps_material_recipes_attributes_and_empty_draw_hierarchy() -> Result<()> {
        let object = |index, count, mode| {
            json!({
                "index":index, "source_index":7, "model_node":0, "draw_order":index,
                "name":format!("draw-{index}"), "material":0, "position_count":3,
                "texcoord_count":3, "display_offset":0, "display_size":count,
                "index_count":count, "position_accessor":3, "texcoord_accessor":4,
                "vertex_map":[], "texture_tables":["shared.tpl"], "tev_modes":[mode],
                "texture_commands":[0x15020000_u32]
            })
        };
        let mut manifest: geometry::GeometryMetadata = serde_json::from_value(json!({
            "objects":[object(0,0,5),object(1,5,2)],
            "textures":[{"index":0,"width":4,"height":4,"image":"unused.png"}],
            "model_nodes":[{"index":0,"object_index":7,"name":"root",
                "translation":[1.,2.,3.],"rotation":[0.,0.,0.,1.],"scale":[1.,1.,1.]}]
        }))?;
        let attributes = json!({"POSITION":3,"TEXCOORD_0":4,"TEXCOORD_1":5,
            "COLOR_0":6,"NORMAL":7,"JOINTS_0":8,"WEIGHTS_0":9});
        let original = json!({
            "meshes":[
                {"primitives":[{"indices":0,"attributes":attributes,"material":0}]},
                {"primitives":[
                    {"indices":1,"attributes":attributes,"mode":4,"material":0},
                    {"indices":2,"attributes":attributes,"mode":1,"material":0}]}],
            "nodes":[{"name":"root","translation":[1.,2.,3.],"children":[1,2]},
                {"mesh":0},{"mesh":1,"skin":0}],
            "skins":[{"joints":[0],"inverseBindMatrices":10}],
            "scenes":[{"nodes":[0]}],"scene":0,
            "accessors":[{"count":0},{"count":3},{"count":2}],
            "materials":[{"pbrMetallicRoughness":{"baseColorFactor":[0.25,0.5,0.75,0.4]},
                "alphaMode":"BLEND"}],
            "buffers":[{"uri":"scene.bin","byteLength":64}]
        });
        let local = || TextureSource::Local {
            catalogue: "model/palettes/textures.json".into(),
        };
        let mut gltf = original.clone();
        let scene = from_geometry(&manifest, &mut gltf, local())?;
        assert_eq!(scene.bone_names, ["root"]);
        assert_eq!(
            scene.draws.iter().map(|draw| draw.mesh).collect::<Vec<_>>(),
            [None, Some(0)]
        );
        let draw = &scene.draws[1];
        assert_eq!(
            (draw.source_index, draw.model_node, draw.index_count),
            (7, Some(0), 5)
        );
        assert_eq!(draw.color, [0.25, 0.5, 0.75, 0.4]);
        assert!(draw.preview_blend);
        assert!(
            draw.recipe.combination().is_err(),
            "physical cooking must retain unsupported shaders"
        );
        assert_eq!(
            draw.recipe.operations,
            [geometry::MaterialOperation::DecalTexture { texture: 0 }]
        );
        let texture = &draw.textures[0];
        assert_eq!(
            (texture.stage, texture.image, texture.table.as_deref()),
            (0, 0, Some("shared.tpl"))
        );
        assert!(matches!(
            texture.wrap,
            [
                resonance_content::TextureWrap::Mirror,
                resonance_content::TextureWrap::Clamp
            ]
        ));
        assert!(matches!(
            texture.min_filter,
            crate::tpl::Filter::LinearMipmapLinear
        ));
        assert!(matches!(texture.mag_filter, crate::tpl::Filter::Linear));
        assert!(gltf["nodes"][1].get("mesh").is_none());
        assert_eq!(gltf["nodes"][2]["mesh"], 0);
        assert_eq!(gltf["nodes"][0], original["nodes"][0]);
        assert_eq!(gltf["nodes"][2]["skin"], 0);
        for key in ["skins", "scenes", "accessors"] {
            assert_eq!(gltf[key], original[key]);
        }
        for (index, mode) in [4, 1].into_iter().enumerate() {
            let primitive = &gltf["meshes"][0]["primitives"][index];
            assert_eq!(primitive["attributes"], attributes);
            assert_eq!(primitive["mode"], mode);
            assert!(primitive.get("material").is_none());
        }
        assert!(gltf.get("materials").is_none());
        assert!(gltf["buffers"][0].get("uri").is_none());
        let published = serde_json::to_value(&scene)?;
        let restored: Scene = serde_json::from_value(published.clone())?;
        assert_eq!(serde_json::to_value(restored)?, published);

        // A bad decoder manifest cannot turn a nonempty draw into an empty one.
        manifest.objects[1].index_count = 0;
        assert!(from_geometry(&manifest, &mut original.clone(), local()).is_err());
        manifest.objects[1].index_count = 5;
        manifest.objects[1].texture_commands.clear();
        assert!(from_geometry(&manifest, &mut original.clone(), local()).is_err());
        Ok(())
    }
}
