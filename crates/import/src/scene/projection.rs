//! Admit authored materials to the runtime's supported shader combinations.
use crate::all_assets::physical_scene::{Scene, Texture};
use anyhow::{Context, Result, ensure};
use resonance_content::{SceneMaterial, ScenePart, TextureBinding, texture::Filter};
use serde_json::{Value, json};

pub(super) fn project(scene: Scene, gltf: &mut Value, textures: Vec<String>) -> Result<ScenePart> {
    let mut materials = Vec::new();
    let mut templates = Vec::new();
    let mut material_nodes = Vec::new();
    let meshes = gltf["meshes"].as_array_mut().context("cooked meshes")?;
    for draw in scene.draws {
        ensure!(
            (draw.index_count == 0) == draw.mesh.is_none(),
            "invalid empty draw binding"
        );
        let Some(mesh) = draw.mesh else { continue };
        ensure!(mesh == materials.len(), "invalid cooked draw ordering");
        ensure!(
            draw.textures
                .iter()
                .enumerate()
                .all(|(i, texture)| usize::from(texture.stage) == i),
            "runtime material needs consecutive texture stages"
        );
        let combination = draw
            .recipe
            .combination()
            .with_context(|| format!("unsupported runtime material {}", draw.name))?;
        let binding = |stage: u8| -> Result<TextureBinding> {
            let texture = draw
                .textures
                .iter()
                .find(|t| t.stage == stage)
                .context("missing cooked material texture stage")?;
            ensure!(
                usize::from(texture.image) < textures.len(),
                "missing runtime texture"
            );
            bind(texture)
        };
        let color = (combination.texture_count() > 0)
            .then(|| binding(0))
            .transpose()?;
        let multiply = (combination.texture_count() == 2)
            .then(|| binding(1))
            .transpose()?;
        for primitive in meshes.get_mut(mesh).context("missing cooked mesh")?["primitives"]
            .as_array_mut()
            .context("cooked primitives")?
        {
            // Decoding emits point, line and triangle lists; keep their topology.
            ensure!(
                matches!(primitive["mode"].as_u64().unwrap_or(4), 0 | 1 | 4),
                "unsupported cooked primitive topology"
            );
            primitive["material"] = json!(materials.len());
            let attributes = primitive["attributes"]
                .as_object_mut()
                .context("primitive attributes")?;
            ensure!(
                !attributes.contains_key("_NORMAL_BASIS_1"),
                "runtime material {} needs a normal-basis consumer",
                draw.name
            );
            if !combination.vertex_color() {
                attributes.remove("COLOR_0");
            }
            ensure!(
                multiply.is_none() || attributes.contains_key("TEXCOORD_1"),
                "multiply material needs secondary UVs"
            );
        }
        templates.push(json!({
            "name": draw.name,
            "pbrMetallicRoughness": {
                "baseColorFactor": draw.color, "metallicFactor": 0.0, "roughnessFactor": 1.0
            },
            "alphaMode": if draw.preview_blend { "BLEND" } else { "OPAQUE" },
            "doubleSided": true,
            "extensions": {"KHR_materials_unlit": {}}
        }));
        ensure!(
            draw.draw_order < 65536,
            "too many authored draws in scene part"
        );
        materials.push(SceneMaterial {
            blend: draw.preview_blend || multiply.is_some(),
            color,
            multiply,
            depth_write: true,
            cull: resonance_content::CullFace::Back,
            draw_order: draw.draw_order,
        });
        material_nodes.push(
            draw.model_node
                .map(u16::try_from)
                .transpose()?
                .into_iter()
                .collect(),
        );
    }
    ensure!(materials.len() == meshes.len(), "unbound cooked mesh");
    gltf["materials"] = json!(templates);
    gltf["extensionsUsed"] = json!(["KHR_materials_unlit"]);
    Ok(ScenePart {
        resource: 0,
        mesh: String::new(),
        textures,
        materials,
        appearance: None,
        translation: [0.; 3],
        clips: Vec::new(),
        autoplay: false,
        texture_animations: Vec::new(),
        bone_names: scene.bone_names,
        material_nodes,
        outline_color: None,
        secondary_motion: Default::default(),
    })
}

fn bind(texture: &Texture) -> Result<TextureBinding> {
    ensure!(
        matches!(texture.mag_filter, Filter::Nearest | Filter::Linear),
        "invalid material magnification filter"
    );
    Ok(TextureBinding {
        texture: usize::from(texture.image),
        wrap_u: texture.wrap[0],
        wrap_v: texture.wrap[1],
        nearest_min: matches!(
            texture.min_filter,
            Filter::Nearest | Filter::NearestMipmapNearest | Filter::NearestMipmapLinear
        ),
        nearest_mag: matches!(texture.mag_filter, Filter::Nearest),
    })
}
