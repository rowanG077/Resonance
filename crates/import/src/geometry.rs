//! Decode field geometry into editable glTF meshes during import.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;

use crate::digest;
use crate::model::ModelBlobJson;
use crate::tpl::{decode_texture, parse_tpl, read_u16, read_u32};

pub const MANIFEST_NAME: &str = "geometry.json";

#[derive(Debug, Error)]
pub enum GeometryError {
    #[error(transparent)]
    Texture(#[from] crate::tpl::TextureError),
    #[error("I/O error for {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid GPL: {0}")]
    Gpl(String),
    #[error("invalid Geometry project: {0}")]
    Project(String),
    #[error("PNG error: {0}")]
    Png(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TextureInfo {
    pub index: usize,
    pub width: u16,
    pub height: u16,
    pub format: u32,
    pub image: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NodeInfo {
    pub index: usize,
    pub name: String,
    pub translation: [f32; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeometryManifest {
    pub format_version: u32,
    pub tpl_file: String,
    pub gpl_file: String,
    pub scene_file: String,
    pub buffer_file: String,
    pub tpl_sha256: String,
    pub gpl_sha256: String,
    pub textures: Vec<TextureInfo>,
    pub nodes: Vec<NodeInfo>,
    /// Identifies the projection used for the resource. Geometry GPLs expose
    /// decoded display-list meshes; actor-hierarchy GPLs use node transforms
    /// and texture cards.
    pub geometry_projection: String,
    #[serde(default)]
    pub resource_kind: String,
    #[serde(default)]
    pub objects: Vec<GeometryObjectInfo>,
    /// Runtime model nodes which place geometry objects in a field actor.
    /// Geometry-only GPLs leave this empty; expanded MAP sections may carry a
    /// trailing model blob that supplies these transforms.
    #[serde(default)]
    pub model_nodes: Vec<ModelNodeInfo>,
    /// Offset of the trailing model blob in `container_file`, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_offset: Option<usize>,
    /// Size of the trailing model blob in `container_file`, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_size: Option<usize>,
    /// Byte offset of the embedded field resource inside `container_file`.
    /// Ordinary section files use zero; `0x1F` containers commonly use 0x80.
    #[serde(default)]
    pub container_offset: usize,
    /// Optional expanded MAP section containing this resource pair.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container_file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelNodeInfo {
    pub index: usize,
    pub object_index: usize,
    pub name: String,
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeometryObjectInfo {
    pub index: usize,
    /// Original object shared by one or more decoded draw primitives.
    #[serde(default)]
    pub source_index: usize,
    /// Draw sequence recovered from model priorities and hierarchy traversal.
    pub draw_order: u32,
    pub name: String,
    pub material: usize,
    pub position_count: usize,
    pub texcoord_count: usize,
    pub display_offset: usize,
    pub display_size: usize,
    pub position_accessor: usize,
    pub texcoord_accessor: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_accessor: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normal_accessor: Option<usize>,
    pub vertex_map: Vec<[usize; 2]>,
    #[serde(default)]
    pub color_map: Vec<usize>,
    #[serde(default)]
    pub normal_map: Vec<usize>,
    #[serde(default = "default_fraction_bits")]
    pub position_fraction_bits: u8,
    #[serde(default = "default_fraction_bits")]
    pub texcoord_fraction_bits: u8,
    #[serde(default = "default_true")]
    pub texcoord_signed: bool,
    #[serde(default)]
    pub color_format: Option<u8>,
    #[serde(default)]
    pub color_components: Option<u8>,
    #[serde(default)]
    pub normal_format: Option<u8>,
    #[serde(default)]
    pub normal_components: Option<u8>,
    #[serde(default)]
    pub vcd: Option<u32>,
    #[serde(default)]
    pub texture_commands: Vec<u32>,
    #[serde(default)]
    pub tev_modes: Vec<u32>,
    #[serde(default)]
    pub matrix_commands: Vec<u32>,
}

const fn default_fraction_bits() -> u8 {
    8
}

const fn default_true() -> bool {
    true
}

/// Export the geometry resources embedded in an expanded MAP section.
///
/// Field sections have a small big-endian resource header followed by a GPL,
/// then a TPL.  This convenience wrapper discovers those ranges so callers do
/// not need to split `section_XX.bin` by hand.  The resulting project contains
/// the decoded scene and source files for inspection.
pub fn export_section(section: &[u8], output: &Path) -> Result<GeometryManifest, GeometryError> {
    if section.len() < 0x20 {
        return Err(GeometryError::Gpl(
            "MAP section is shorter than its resource header".into(),
        ));
    }
    // `map-unpack` preserves outer 0x1F resource containers.  Their payload
    // begins at the recorded offset (normally 0x80) and contains the usual
    // GPL/TPL section header.
    let container_offset = if read_u32(section, 0) == Some(0x1F) {
        read_u32(section, 4)
            .and_then(|offset| usize::try_from(offset).ok())
            .filter(|offset| *offset + 0x20 <= section.len())
            .ok_or_else(|| GeometryError::Gpl("invalid 0x1F resource container".into()))?
    } else {
        0
    };
    let resource = &section[container_offset..];
    let tpl_offset = read_u32(resource, 0)
        .ok_or_else(|| GeometryError::Gpl("MAP section has no TPL offset".into()))?
        as usize;
    let tpl_end = read_u32(resource, 4)
        .ok_or_else(|| GeometryError::Gpl("MAP section has no TPL end offset".into()))?
        as usize;
    if tpl_offset < 0x20 || tpl_end < tpl_offset || tpl_end > resource.len() {
        return Err(GeometryError::Gpl(format!(
            "invalid MAP resource ranges 0x{tpl_offset:X}..0x{tpl_end:X}"
        )));
    }
    let gpl = &resource[0x20..tpl_offset];
    let tpl = &resource[tpl_offset..tpl_end];
    if !matches!(read_u32(gpl, 0), Some(0x005B_BC61 | 0x00B7_49E0)) {
        return Err(GeometryError::Gpl(
            "MAP section does not contain a geometry-palette GPL".into(),
        ));
    }
    // Field actor resources append a compact model blob after the GPL/TPL
    // pair.  Its node table supplies the local transform for each GPL object.
    // The following animation resources use the same magic, so stop at the
    // first subsequent magic rather than handing them to the model parser.
    let (model, model_offset, model_size) = match trailing_model(resource, tpl_end)? {
        Some((model, offset, size)) => (Some(model), Some(container_offset + offset), Some(size)),
        None => (None, None, None),
    };
    let mut manifest = export_geometry(tpl, gpl, output, model.as_ref())?;
    let container_file = "section.bin";
    write_file(&output.join(container_file), section)?;
    manifest.container_file = Some(container_file.into());
    manifest.model_offset = model_offset;
    manifest.model_size = model_size;
    manifest.container_offset = container_offset;
    let json = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| GeometryError::Project(error.to_string()))?;
    write_file(
        &output.join(MANIFEST_NAME),
        &[json.as_slice(), b"\n"].concat(),
    )?;
    Ok(manifest)
}

fn trailing_model(
    section: &[u8],
    start: usize,
) -> Result<Option<(ModelBlobJson, usize, usize)>, GeometryError> {
    if read_u32(section, start) != Some(0x007B_7960_u32) {
        return Ok(None);
    }
    let suffix = &section[start..];
    let end = suffix
        .get(0x20..)
        .and_then(|tail| {
            tail.windows(4)
                .position(|bytes| bytes == 0x007B_7960_u32.to_be_bytes())
        })
        .map_or(suffix.len(), |offset| 0x20 + offset);
    let model_bytes = &suffix[..end];
    // Animation resources use the same magic.  Only treat the block as a
    // model when its node-table layout validates; otherwise leave the
    // trailing resource untouched and export the GPL normally.
    let Ok(model) = ModelBlobJson::parse(model_bytes, 0) else {
        return Ok(None);
    };
    Ok(Some((model, start, end)))
}

fn io_error(path: &Path, source: std::io::Error) -> GeometryError {
    GeometryError::Io {
        path: path.to_path_buf(),
        source,
    }
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), GeometryError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| io_error(parent, source))?;
    }
    fs::write(path, bytes).map_err(|source| io_error(path, source))
}

#[derive(Debug, Clone)]
struct GeometryMesh {
    positions: Vec<[f32; 3]>,
    texcoords: Vec<[f32; 2]>,
    secondary_texcoords: Option<Vec<[f32; 2]>>,
    colors: Option<Vec<[f32; 4]>>,
    normals: Option<Vec<[f32; 3]>>,
    indices: Vec<u32>,
    vertex_map: Vec<[usize; 2]>,
    color_map: Vec<usize>,
    normal_map: Vec<usize>,
    joints: Option<Vec<u16>>,
}

#[derive(Debug, Clone, Copy)]
struct VertexArrayDesc {
    data_offset: usize,
    count: usize,
    format: u8,
    components: u8,
    scale: f32,
}

#[derive(Debug, Clone, Copy)]
struct VertexAttributeSpec {
    attr: u8,
    kind: u8,
}

#[derive(Debug, Clone, Default)]
struct RenderStateInfo {
    vcd: Option<u32>,
    texture_commands: Vec<u32>,
    tev_modes: Vec<u32>,
    matrix_commands: Vec<u32>,
}

#[derive(Debug, Clone)]
struct GeometryObject {
    source_index: usize,
    name: String,
    material: usize,
    position_count: usize,
    texcoord_count: usize,
    display_offset: usize,
    display_size: usize,
    position_fraction_bits: u8,
    texcoord_fraction_bits: u8,
    texcoord_signed: bool,
    color: Option<(VertexArrayDesc, Vec<[f32; 4]>)>,
    normal: Option<(VertexArrayDesc, Vec<[f32; 3]>)>,
    render_state: RenderStateInfo,
    mesh: GeometryMesh,
}

/// Draw the optional root object first, then sort by authored priority,
/// preserving depth-first order for ties.
fn model_draw_order(model: &ModelBlobJson) -> Result<Vec<u16>, GeometryError> {
    let mut pending = if model.nodes.is_empty() {
        vec![]
    } else {
        vec![model.node_offset]
    };
    let mut seen = std::collections::BTreeSet::new();
    let mut nodes = Vec::new();
    while let Some(pointer) = pending.pop() {
        let offset = pointer
            .checked_sub(model.node_offset)
            .ok_or_else(|| GeometryError::Gpl("draw node precedes node table".into()))?;
        let index = offset as usize / 28;
        if !offset.is_multiple_of(28) || index >= model.nodes.len() || !seen.insert(index) {
            return Err(GeometryError::Gpl(
                "invalid or cyclic draw hierarchy".into(),
            ));
        }
        let node = &model.nodes[index];
        if node.object_index != u16::MAX {
            nodes.push((node.field19, node.object_index));
        }
        // Stack order implements child traversal before the next sibling.
        for pointer in [node.next_offset, node.child_offset] {
            if pointer != 0 {
                pending.push(pointer);
            }
        }
    }
    if seen.len() != model.nodes.len() {
        return Err(GeometryError::Gpl(
            "draw hierarchy leaves model nodes unreachable".into(),
        ));
    }
    nodes.sort_by_key(|(priority, _)| *priority);
    let root = (model.field14 >> 16) as u16;
    Ok((root != u16::MAX)
        .then_some(root)
        .into_iter()
        .chain(nodes.into_iter().map(|(_, object)| object))
        .collect())
}

fn object_draw_order(
    objects: &[GeometryObject],
    model: Option<&ModelBlobJson>,
) -> Result<Vec<u32>, GeometryError> {
    let Some(model) = model else {
        return Ok((0..objects.len() as u32).collect());
    };
    let mut rank = std::collections::BTreeMap::new();
    for (index, object) in model_draw_order(model)?.into_iter().enumerate() {
        if rank.insert(usize::from(object), index).is_some() {
            return Err(GeometryError::Gpl(
                "repeated geometry instances need independent draw recipes".into(),
            ));
        }
    }
    for object in objects {
        if !rank.contains_key(&object.source_index) {
            return Err(GeometryError::Gpl(format!(
                "object {} has no authored draw: {}",
                object.source_index, object.name
            )));
        }
    }
    let mut indices: Vec<_> = (0..objects.len()).collect();
    indices.sort_by_key(|i| rank[&objects[*i].source_index]);
    let mut result = vec![0; objects.len()];
    for (rank, index) in indices.into_iter().enumerate() {
        result[index] = rank as u32;
    }
    Ok(result)
}

fn export_geometry(
    tpl: &[u8],
    gpl: &[u8],
    output: &Path,
    model: Option<&ModelBlobJson>,
) -> Result<GeometryManifest, GeometryError> {
    let textures = parse_tpl(tpl)?;
    let objects = parse_geometry(gpl)?;
    let draw_order = object_draw_order(&objects, model)?;
    fs::create_dir_all(output).map_err(|source| io_error(output, source))?;
    write_file(&output.join("model.tpl"), tpl)?;
    write_file(&output.join("model.gpl"), gpl)?;
    let mut texture_info = Vec::with_capacity(textures.len());
    for (index, texture) in textures.iter().enumerate() {
        let rgba = decode_texture(tpl, texture)?;
        let image = format!("texture_{index:03}.png");
        write_png(&output.join(&image), texture.width, texture.height, &rgba)?;
        texture_info.push(TextureInfo {
            index,
            width: texture.width,
            height: texture.height,
            format: texture.format,
            image,
            sha256: digest(&rgba),
        });
    }
    let mut buffer = Vec::new();
    let mut views = Vec::new();
    let mut accessors = Vec::new();
    let mut meshes = Vec::new();
    let model_nodes = model.map(model_node_info).unwrap_or_default();
    let mut nodes: Vec<serde_json::Value> = model_nodes
        .iter()
        .map(|n| {
            json!({
                "name": n.name, "translation": n.translation, "rotation": n.rotation,
                "scale": n.scale, "children": []
            })
        })
        .collect();
    let mut child_nodes = std::collections::BTreeSet::new();
    let mut parents = vec![None; model_nodes.len()];
    if let Some(model) = model {
        for (parent, node) in model.nodes.iter().enumerate() {
            let mut pointer = node.child_offset;
            let mut seen = std::collections::BTreeSet::new();
            while pointer != 0 {
                let offset = pointer
                    .checked_sub(model.node_offset)
                    .ok_or_else(|| GeometryError::Gpl("invalid model child pointer".into()))?
                    as usize;
                let child = offset / 28;
                if !offset.is_multiple_of(28) || child >= model.nodes.len() || !seen.insert(child) {
                    return Err(GeometryError::Gpl("invalid model hierarchy".into()));
                }
                if !child_nodes.insert(child) || child == parent {
                    return Err(GeometryError::Gpl(
                        "model node has multiple parents or a cycle".into(),
                    ));
                }
                parents[child] = Some(parent);
                nodes[parent]["children"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!(child));
                pointer = model.nodes[child].next_offset;
            }
        }
    }
    let mut roots: Vec<usize> = (0..nodes.len())
        .filter(|i| !child_nodes.contains(i))
        .collect();
    let has_skin = objects.iter().any(|o| o.mesh.joints.is_some());
    if has_skin && model_nodes.is_empty() {
        return Err(GeometryError::Gpl(
            "skinned geometry has no model hierarchy".into(),
        ));
    }
    let mut skins = Vec::new();
    if has_skin {
        align4(&mut buffer);
        let offset = buffer.len();
        for index in 0..model_nodes.len() {
            // Retail skinned positions are in bind-pose model space. glTF
            // uses the same convention with inverse global bind transforms.
            let bind = bind_transform(index, &model_nodes, &parents)?;
            let inverse = bind.inverse();
            if !inverse.is_finite() {
                return Err(GeometryError::Gpl("singular bind transform".into()));
            }
            for value in inverse.to_cols_array() {
                buffer.extend(value.to_le_bytes());
            }
        }
        let view = views.len();
        views.push(json!({"buffer":0,"byteOffset":offset,"byteLength":model_nodes.len()*64}));
        let accessor = accessors.len();
        accessors.push(
            json!({"bufferView":view,"componentType":FLOAT,"count":model_nodes.len(),"type":"MAT4"}),
        );
        skins.push(json!({"joints":(0..model_nodes.len()).collect::<Vec<_>>(),"inverseBindMatrices":accessor}));
    }
    let mut object_info = Vec::with_capacity(objects.len());
    for (index, object) in objects.iter().enumerate() {
        let (position_accessor, texcoord_accessor, color_accessor, normal_accessor, mesh) =
            append_geometry_mesh(
                &mut buffer,
                &mut views,
                &mut accessors,
                &object.mesh,
                object.material,
            );
        meshes.push(mesh);
        let mesh_node = json!({"name": object.name, "mesh": index});
        if let Some(joints) = &object.mesh.joints {
            if joints.iter().any(|j| usize::from(*j) >= model_nodes.len()) {
                return Err(GeometryError::Gpl(
                    "joint index exceeds model hierarchy".into(),
                ));
            }
            let mut node = mesh_node;
            node["skin"] = json!(0);
            roots.push(nodes.len());
            nodes.push(node);
        } else {
            let instances: Vec<usize> = model_nodes
                .iter()
                .filter(|n| n.object_index == object.source_index)
                .map(|n| n.index)
                .collect();
            if instances.is_empty() {
                roots.push(nodes.len());
                nodes.push(mesh_node);
            } else {
                for parent in instances {
                    let index = nodes.len();
                    nodes.push(mesh_node.clone());
                    nodes[parent]["children"]
                        .as_array_mut()
                        .unwrap()
                        .push(json!(index));
                }
            }
        }
        object_info.push(GeometryObjectInfo {
            index,
            source_index: object.source_index,
            draw_order: draw_order[index],
            name: object.name.clone(),
            material: object.material,
            position_count: object.position_count,
            texcoord_count: object.texcoord_count,
            display_offset: object.display_offset,
            display_size: object.display_size,
            position_accessor,
            texcoord_accessor,
            color_accessor,
            normal_accessor,
            vertex_map: object.mesh.vertex_map.clone(),
            color_map: object.mesh.color_map.clone(),
            normal_map: object.mesh.normal_map.clone(),
            position_fraction_bits: object.position_fraction_bits,
            texcoord_fraction_bits: object.texcoord_fraction_bits,
            texcoord_signed: object.texcoord_signed,
            color_format: object.color.as_ref().map(|(desc, _)| desc.format),
            color_components: object.color.as_ref().map(|(desc, _)| desc.components),
            normal_format: object.normal.as_ref().map(|(desc, _)| desc.format),
            normal_components: object.normal.as_ref().map(|(desc, _)| desc.components),
            vcd: object.render_state.vcd,
            texture_commands: object.render_state.texture_commands.clone(),
            tev_modes: object.render_state.tev_modes.clone(),
            matrix_commands: object.render_state.matrix_commands.clone(),
        });
    }
    // GX can carry transparency in vertex-color alpha even when the source
    // texture itself is fully opaque.  This is how the classroom light
    // meshes are authored: their CI8 texture has no alpha, while COLOR_0
    // fades each billboard toward zero.  Mark those materials as blended so
    // Geometry does not discard the vertex alpha under an OPAQUE material.
    let mut vertex_alpha_by_material = vec![false; texture_info.len().max(1)];
    for object in &objects {
        if object
            .mesh
            .colors
            .as_ref()
            .is_some_and(|colors| colors.iter().any(|color| color[3] < 0.999))
            && let Some(slot) = vertex_alpha_by_material.get_mut(object.material)
        {
            *slot = true;
        }
    }
    let materials = texture_info
        .iter()
        .enumerate()
        .map(|(index, texture)| {
            let rgba = decode_texture(tpl, &textures[index]).unwrap_or_default();
            let has_texture_alpha = rgba.chunks_exact(4).any(|pixel| pixel[3] != 0xFF);
            let has_vertex_alpha = vertex_alpha_by_material
                .get(index)
                .copied()
                .unwrap_or(false);
            json!({
                "name": format!("TPL material {index}"),
                "pbrMetallicRoughness": {
                    "baseColorTexture": {"index": index},
                    "metallicFactor": 0.0,
                    "roughnessFactor": 1.0
                },
                "alphaMode": if has_texture_alpha || has_vertex_alpha { "BLEND" } else { "OPAQUE" },
                "doubleSided": true,
                "extras": {"resonance": {
                    "texture_index": texture.index,
                    "vertex_alpha": has_vertex_alpha
                }}
            })
        })
        .collect::<Vec<_>>();
    let mut materials = materials;
    if materials.is_empty() {
        materials.push(json!({
            "name": "GX untextured",
            "pbrMetallicRoughness": {
                "baseColorFactor": [1.0, 1.0, 1.0, 1.0],
                "metallicFactor": 0.0,
                "roughnessFactor": 1.0
            },
            "doubleSided": true
        }));
    }
    let images = texture_info
        .iter()
        .map(|texture| json!({"uri": texture.image}))
        .collect::<Vec<_>>();
    let mut sampler_by_texture = vec![None; texture_info.len()];
    for object in &object_info {
        if let (Some(command), Some(slot)) = (
            object.texture_commands.first(),
            sampler_by_texture.get_mut(object.material),
        ) && slot.is_none()
        {
            *slot = Some(*command);
        }
    }
    let samplers = sampler_by_texture
        .iter()
        .map(|command| {
            let command = command.unwrap_or(0);
            json!({
                "wrapS": gltf_wrap((command >> 16) & 0xF),
                "wrapT": gltf_wrap((command >> 20) & 0xF),
                "minFilter": gltf_min_filter((command >> 24) & 0xF),
                "magFilter": gltf_mag_filter(command >> 28)
            })
        })
        .collect::<Vec<_>>();
    let textures_json = (0..texture_info.len())
        .map(|index| json!({"source": index, "sampler": index}))
        .collect::<Vec<_>>();
    let object_extras = object_info
        .iter()
        .map(|object| {
            json!({
                "name": object.name,
                "vcd": object.vcd,
                "texture_commands": object.texture_commands,
                "tev_modes": object.tev_modes,
                "matrix_commands": object.matrix_commands
            })
        })
        .collect::<Vec<_>>();
    let scene = serde_json::to_string_pretty(&json!({"asset":{"version":"2.0","generator":"resonance-import"},"scene":0,"scenes":[{"nodes":roots}],"nodes":nodes,"buffers":[{"uri":"scene.bin","byteLength":buffer.len()}],"bufferViews":views,"accessors":accessors,"meshes":meshes,"skins":skins,"materials":materials,"textures":textures_json,"samplers":samplers,"images":images,"extras":{"resonance":{"format":"TPL+GPL","resource_kind":"gpl","objects":object_extras}}})).map_err(|error| GeometryError::Project(error.to_string()))?;
    let manifest = GeometryManifest {
        format_version: 1,
        tpl_file: "model.tpl".into(),
        gpl_file: "model.gpl".into(),
        scene_file: "scene.gltf".into(),
        buffer_file: "scene.bin".into(),
        tpl_sha256: digest(tpl),
        gpl_sha256: digest(gpl),
        textures: texture_info,
        nodes: Vec::new(),
        geometry_projection: "gpl-display-lists".into(),
        resource_kind: "gpl".into(),
        objects: object_info,
        model_nodes,
        model_offset: None,
        model_size: None,
        container_offset: 0,
        container_file: None,
    };
    write_file(&output.join("scene.gltf"), scene.as_bytes())?;
    write_file(&output.join("scene.bin"), &buffer)?;
    let json = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| GeometryError::Project(error.to_string()))?;
    write_file(
        &output.join(MANIFEST_NAME),
        &[json.as_slice(), b"\n"].concat(),
    )?;
    Ok(manifest)
}

#[allow(clippy::too_many_lines)]
fn parse_geometry(data: &[u8]) -> Result<Vec<GeometryObject>, GeometryError> {
    // Both geometry-palette revisions use the same bounded entry/array
    // records. The older revision is used by the original setup map.
    if !matches!(read_u32(data, 0), Some(0x005B_BC61 | 0x00B7_49E0)) || data.len() < 0x14 {
        return Err(GeometryError::Gpl("missing geometry-palette header".into()));
    }
    let count = read_u32(data, 0x0C).unwrap() as usize;
    let entries = read_u32(data, 0x10).unwrap() as usize;
    if entries
        .checked_add(count * 8)
        .is_none_or(|end| end > data.len())
    {
        return Err(GeometryError::Gpl(
            "geometry entry table exceeds file".into(),
        ));
    }
    let mut descriptors = Vec::with_capacity(count);
    for index in 0..count {
        let at = entries + index * 8;
        let object_offset = read_u32(data, at).unwrap() as usize;
        let name_offset = read_u32(data, at + 4).unwrap() as usize;
        let name = c_string(data, name_offset).unwrap_or_else(|| format!("gpl_object_{index:03}"));
        descriptors.push((object_offset, name));
    }
    let mut objects = Vec::with_capacity(count);
    for (index, (offset, name)) in descriptors.iter().enumerate() {
        let end = descriptors.get(index + 1).map_or(data.len(), |item| item.0);
        if *offset < 0x14 || *offset + 0x18 > end || end > data.len() {
            return Err(GeometryError::Gpl(format!(
                "object {index} range is invalid"
            )));
        }
        let object = &data[*offset..end];
        let pos_record = read_u32(object, 0).unwrap() as usize;
        let color_record = read_u32(object, 4).unwrap() as usize;
        let tex_record = read_u32(object, 8).unwrap() as usize;
        let normal_record = read_u32(object, 0xC).unwrap() as usize;
        let material_record = read_u32(object, 16).unwrap() as usize;
        if pos_record + 8 > object.len()
            || (tex_record != 0 && tex_record + 8 > object.len())
            || material_record + 8 > object.len()
        {
            return Err(GeometryError::Gpl(format!(
                "object {index} array records are invalid"
            )));
        }
        let positions_desc = parse_vertex_desc(object, pos_record, "position")?;
        let texcoords_desc = if tex_record != 0 {
            parse_vertex_desc(object, tex_record, "texcoord")?
        } else {
            VertexArrayDesc {
                data_offset: 0,
                count: 0,
                format: 0,
                components: 0,
                scale: 1.0,
            }
        };
        if positions_desc.components != 3 || component_type(positions_desc.format) != 3 {
            return Err(GeometryError::Gpl(format!(
                "object {index} has unsupported position component format {}",
                positions_desc.format
            )));
        }
        if tex_record != 0
            && (texcoords_desc.components != 2
                || !matches!(component_type(texcoords_desc.format), 2 | 3))
        {
            return Err(GeometryError::Gpl(format!(
                "object {index} has unsupported texcoord component format {}",
                texcoords_desc.format
            )));
        }
        validate_array(object, positions_desc, "position")?;
        if tex_record != 0 {
            validate_array(object, texcoords_desc, "texcoord")?;
        }
        let positions = decode_vectors::<3>(object, positions_desc);
        let texcoords = decode_vectors::<2>(object, texcoords_desc);
        let color = if color_record != 0 {
            let desc = parse_vertex_desc(object, color_record, "color")?;
            validate_array(object, desc, "color")?;
            Some((desc, decode_colors(object, desc)))
        } else {
            None
        };
        let normal = if normal_record != 0 {
            let desc = parse_vertex_desc(object, normal_record, "normal")?;
            validate_array(object, desc, "normal")?;
            Some((desc, decode_vectors::<3>(object, desc)))
        } else {
            None
        };
        let material_ptr = read_u32(object, material_record + 4).unwrap() as usize;
        if material_ptr + 0x30 > object.len() {
            return Err(GeometryError::Gpl(format!(
                "object {index} material record is invalid"
            )));
        }
        let command_count = read_u16(object, material_record + 8)
            .ok_or_else(|| GeometryError::Gpl("missing render command count".into()))?
            as usize;
        for (draw, (render_state, display_offset, display_size)) in parse_render_commands(
            object,
            material_ptr,
            command_count,
            read_u32(data, 0) == Some(0x005B_BC61),
        )?
        .into_iter()
        .enumerate()
        {
            let material = render_state
                .texture_commands
                .first()
                .map_or(0, |value| (value & 0x1FFF) as usize);
            let mesh = decode_display_list(
                &object[display_offset..display_offset + display_size],
                &positions,
                &texcoords,
                color.as_ref().map(|(_, values)| values.as_slice()),
                normal.as_ref().map(|(_, values)| values.as_slice()),
                &render_state,
            ).map_err(|error| GeometryError::Gpl(format!("object {index} ({name}), draw {draw} at {:#x}, VCD {:?}, {} colors, {} normals: {error}", offset + display_offset, render_state.vcd, color.as_ref().map_or(0, |(_, values)| values.len()), normal.as_ref().map_or(0, |(_, values)| values.len()))))?;
            objects.push(GeometryObject {
                source_index: index,
                name: if draw == 0 {
                    name.clone()
                } else {
                    format!("{name}/draw{draw}")
                },
                material,
                position_count: positions_desc.count,
                texcoord_count: texcoords_desc.count,
                display_offset,
                display_size,
                position_fraction_bits: fraction_bits(positions_desc.format),
                texcoord_fraction_bits: fraction_bits(texcoords_desc.format),
                texcoord_signed: component_type(texcoords_desc.format) == 3,
                color: color.clone(),
                normal: normal.clone(),
                render_state,
                mesh,
            });
        }
    }
    Ok(objects)
}

fn fixed_scale(fraction_bits: u8) -> f32 {
    2.0_f32.powi(-i32::from(fraction_bits))
}

fn c_string(data: &[u8], offset: usize) -> Option<String> {
    if offset >= data.len() {
        return None;
    }
    let end = data[offset..]
        .iter()
        .position(|value| *value == 0)
        .map_or(data.len(), |size| offset + size);
    std::str::from_utf8(&data[offset..end])
        .ok()
        .map(ToOwned::to_owned)
}

fn parse_vertex_desc(
    object: &[u8],
    offset: usize,
    label: &str,
) -> Result<VertexArrayDesc, GeometryError> {
    let data_offset = read_u32(object, offset)
        .ok_or_else(|| GeometryError::Gpl(format!("{label} descriptor has no data pointer")))?
        as usize;
    let packed = read_u32(object, offset + 4)
        .ok_or_else(|| GeometryError::Gpl(format!("{label} descriptor has no format")))?;
    Ok(VertexArrayDesc {
        data_offset,
        count: (packed >> 16) as usize,
        format: ((packed >> 8) & 0xFF) as u8,
        components: (packed & 0xFF) as u8,
        scale: fixed_scale((packed >> 8 & 0x0F) as u8),
    })
}

fn component_type(format: u8) -> u8 {
    format >> 4
}

fn fraction_bits(format: u8) -> u8 {
    format & 0x0F
}

fn component_width(format: u8) -> usize {
    match component_type(format) {
        0 | 1 => 1,
        2 | 3 => 2,
        4 => 4,
        _ => 0,
    }
}

fn validate_array(object: &[u8], desc: VertexArrayDesc, label: &str) -> Result<(), GeometryError> {
    let width = if label == "color" {
        color_width(desc.format)
    } else {
        component_width(desc.format)
    };
    if width == 0 || desc.components == 0 {
        return Err(GeometryError::Gpl(format!(
            "{label} descriptor has unsupported format 0x{:02X}",
            desc.format
        )));
    }
    let size = desc
        .count
        .checked_mul(usize::from(desc.components))
        .and_then(|value| value.checked_mul(width))
        .ok_or_else(|| GeometryError::Gpl(format!("{label} array overflows")))?;
    if desc
        .data_offset
        .checked_add(size)
        .is_none_or(|end| end > object.len())
    {
        return Err(GeometryError::Gpl(format!("{label} array exceeds object")));
    }
    Ok(())
}

fn read_component(data: &[u8], offset: usize, format: u8) -> f32 {
    match component_type(format) {
        0 => f32::from(data[offset]),
        1 => f32::from(i8::from_be_bytes([data[offset]])),
        2 => f32::from(u16::from_be_bytes(
            data[offset..offset + 2].try_into().unwrap(),
        )),
        3 => f32::from(i16::from_be_bytes(
            data[offset..offset + 2].try_into().unwrap(),
        )),
        4 => f32::from_bits(u32::from_be_bytes(
            data[offset..offset + 4].try_into().unwrap(),
        )),
        _ => 0.0,
    }
}

fn decode_vectors<const N: usize>(object: &[u8], desc: VertexArrayDesc) -> Vec<[f32; N]> {
    let width = component_width(desc.format);
    (0..desc.count)
        .map(|index| {
            let at = desc.data_offset + index * usize::from(desc.components) * width;
            std::array::from_fn(|axis| {
                read_component(object, at + axis * width, desc.format) * desc.scale
            })
        })
        .collect()
}

fn decode_colors(object: &[u8], desc: VertexArrayDesc) -> Vec<[f32; 4]> {
    use crate::tpl::{expand4, expand5, expand6};
    let width = color_width(desc.format);
    (0..desc.count)
        .map(|index| {
            let at = desc.data_offset + index * width;
            let mut color = [0_u8; 4];
            match component_type(desc.format) {
                0 => {
                    let value = u16::from_be_bytes(object[at..at + 2].try_into().unwrap());
                    color = [
                        expand5(value >> 11),
                        expand6((value >> 5) & 0x3F),
                        expand5(value & 0x1F),
                        0xFF,
                    ];
                }
                3 => {
                    let value = u16::from_be_bytes(object[at..at + 2].try_into().unwrap());
                    color = [
                        expand4(value >> 12),
                        expand4((value >> 8) & 0xF),
                        expand4((value >> 4) & 0xF),
                        expand4(value & 0xF),
                    ];
                }
                5 => color.copy_from_slice(&object[at..at + 4]),
                1 | 2 => {
                    color[..3].copy_from_slice(&object[at..at + 3]);
                    color[3] = 0xFF;
                }
                4 => {
                    let value = u32::from_be_bytes([0, object[at], object[at + 1], object[at + 2]]);
                    color = [18, 12, 6, 0].map(|shift| expand6(((value >> shift) & 0x3F) as u16));
                }
                _ => {}
            }
            color.map(|value| f32::from(value) / 255.0)
        })
        .collect()
}

fn color_width(format: u8) -> usize {
    match component_type(format) {
        0 | 3 => 2,
        1 | 4 => 3,
        2 | 5 => 4,
        _ => 0,
    }
}

/// Every 16-byte command updates material state and may issue a draw. In
/// particular, skinned objects issue draws after loading their joint palettes.
fn parse_render_commands(
    object: &[u8],
    start: usize,
    count: usize,
    modern: bool,
) -> Result<Vec<(RenderStateInfo, usize, usize)>, GeometryError> {
    if count == 0
        || start
            .checked_add(count * 16)
            .is_none_or(|end| end > object.len())
    {
        return Err(GeometryError::Gpl("invalid render command table".into()));
    }
    let mut state = RenderStateInfo::default();
    let mut draws = Vec::new();
    for at in (start..start + count * 16).step_by(16) {
        let value = read_u32(object, at + 4).unwrap();
        // Palette revision identifies the command dialect before relocation.
        // Older commands number vertex/material/matrix as 3/4/5; newer ones
        // use 2/3/4 and pack material recipes into nibbles.
        let kind = match (modern, object[at]) {
            (false, 3) => 2,
            (false, 4) => 3,
            (false, 5) => 4,
            (false, 2) => {
                return Err(GeometryError::Gpl(
                    "unsupported old render command 2".into(),
                ));
            }
            (_, kind) => kind,
        };
        match kind {
            1 => {
                let stage = (value >> 13) & 7;
                state.texture_commands.retain(|v| ((v >> 13) & 7) != stage);
                state.texture_commands.push(value);
                state.texture_commands.sort_by_key(|v| (v >> 13) & 7);
            }
            2 => state.vcd = Some(value),
            3 => {
                state.tev_modes = vec![if modern {
                    value
                } else {
                    // Material modes: modulate, decal, replace, vertex color.
                    match value {
                        0 | 1 | 3 | 4 => value + 1,
                        _ => {
                            return Err(GeometryError::Gpl(format!(
                                "unsupported old material {value}"
                            )));
                        }
                    }
                }]
            }
            4 => {
                let slot = value & 0xffff;
                state.matrix_commands.retain(|v| (v & 0xffff) != slot);
                state.matrix_commands.push(value);
            }
            0 => break,
            kind => return Err(GeometryError::Gpl(format!("unknown render command {kind}"))),
        }
        let offset = read_u32(object, at + 8).unwrap() as usize;
        let size = read_u32(object, at + 12).unwrap() as usize;
        if size == 0 {
            continue;
        }
        if offset
            .checked_add(size)
            .is_none_or(|end| end > object.len())
        {
            return Err(GeometryError::Gpl("draw range exceeds object".into()));
        }
        draws.push((state.clone(), offset, size));
    }
    if draws.is_empty() {
        return Err(GeometryError::Gpl("object has no draw commands".into()));
    }
    Ok(draws)
}

fn decode_display_list(
    data: &[u8],
    positions: &[[f32; 3]],
    texcoords: &[[f32; 2]],
    colors: Option<&[[f32; 4]]>,
    normals: Option<&[[f32; 3]]>,
    state: &RenderStateInfo,
) -> Result<GeometryMesh, GeometryError> {
    let specs = if let Some(vcd) = state.vcd {
        vertex_specs(vcd)?
    } else {
        // Older synthetic GPLs and actor projections do not expose a VCD. Keep
        // a conservative compatibility path for those resources only.
        let mut specs = vec![VertexAttributeSpec {
            attr: 9,
            kind: if positions.len() > 255 { 3 } else { 2 },
        }];
        // A single color is a material constant; larger palettes use an index
        // between the position and UV indices.
        if let Some(colors) = colors.filter(|values| values.len() > 1) {
            specs.push(VertexAttributeSpec {
                attr: 11,
                kind: if colors.len() > 255 { 3 } else { 2 },
            });
        }
        if !texcoords.is_empty() {
            specs.push(VertexAttributeSpec {
                attr: 13,
                kind: if texcoords.len() > 255 { 3 } else { 2 },
            });
        }
        specs
    };
    let stride = specs
        .iter()
        .map(|spec| index_width(spec.kind))
        .sum::<usize>();
    if stride == 0 {
        return Err(GeometryError::Gpl("empty GX vertex layout".into()));
    }
    let mut out_positions = Vec::new();
    let mut out_texcoords = Vec::new();
    let mut secondary_texcoords = specs.iter().any(|s| s.attr == 14).then(Vec::new);
    let mut out_colors = colors.map(|_| Vec::new());
    let mut out_normals = normals.map(|_| Vec::new());
    let mut vertex_map = Vec::new();
    let mut color_map = Vec::new();
    let mut normal_map = Vec::new();
    let mut indices = Vec::new();
    let mut joints = specs.iter().any(|s| s.attr == 0).then(Vec::new);
    let mut cursor = 0;
    while cursor < data.len() {
        let opcode = data[cursor];
        if opcode == 0 {
            cursor += 1;
            continue;
        }
        if (0x80..=0xBF).contains(&opcode) {
            if cursor + 3 > data.len() {
                return Err(GeometryError::Gpl("truncated GX primitive header".into()));
            }
            let primitive = (opcode & 0x78) >> 3;
            let count = usize::from(u16::from_be_bytes([data[cursor + 1], data[cursor + 2]]));
            let payload = cursor + 3;
            let end = payload
                .checked_add(count * stride)
                .ok_or_else(|| GeometryError::Gpl("GX primitive overflows display list".into()))?;
            if end > data.len() {
                return Err(GeometryError::Gpl(
                    "GX primitive exceeds display list".into(),
                ));
            }
            let first_vertex = out_positions.len();
            for vertex in 0..count {
                let at = payload + vertex * stride;
                let mut offset = at;
                let mut pos_index = None;
                let mut tex_index = None;
                let mut secondary_tex_index = None;
                let mut color_index = None;
                let mut normal_index = None;
                for spec in &specs {
                    let width = index_width(spec.kind);
                    if spec.kind == 1 && spec.attr != 0 {
                        return Err(GeometryError::Gpl(
                            "direct GX vertex attributes are not supported yet".into(),
                        ));
                    }
                    let index = read_index(data, offset, width);
                    offset += width;
                    match spec.attr {
                        0 => {
                            let slot = index / 3;
                            let joint = state
                                .matrix_commands
                                .iter()
                                .find(|v| (**v & 0xffff) as usize == slot)
                                .ok_or_else(|| {
                                    GeometryError::Gpl(format!("unbound joint matrix slot {slot}"))
                                })?;
                            joints.as_mut().unwrap().push((joint >> 16) as u16);
                        }
                        9 => pos_index = Some(index),
                        10 => normal_index = Some(index),
                        11 => color_index = Some(index),
                        13 => tex_index = Some(index),
                        14 => secondary_tex_index = Some(index),
                        _ => {}
                    }
                }
                let pos_index = pos_index.ok_or_else(|| {
                    GeometryError::Gpl("GX layout has no position attribute".into())
                })?;
                let has_texcoord = tex_index.is_some();
                let tex_index = tex_index.unwrap_or(0);
                if pos_index >= positions.len() || (has_texcoord && tex_index >= texcoords.len()) {
                    return Err(GeometryError::Gpl(format!(
                        "GX vertex index exceeds source array: position {pos_index}/{}, UV {tex_index}/{}, primitive {opcode:#x}, vertex {vertex}, stride {stride}",
                        positions.len(),
                        texcoords.len()
                    )));
                }
                if let Some(index) = color_index {
                    let values = colors.ok_or_else(|| {
                        GeometryError::Gpl("GX layout references missing color array".into())
                    })?;
                    if index >= values.len() {
                        return Err(GeometryError::Gpl(
                            "GX color index exceeds source array".into(),
                        ));
                    }
                    out_colors.as_mut().unwrap().push(values[index]);
                    color_map.push(index);
                } else if let Some(values) = colors
                    && values.len() == 1
                {
                    out_colors.as_mut().unwrap().push(values[0]);
                    color_map.push(0);
                }
                if let Some(index) = normal_index {
                    let values = normals.ok_or_else(|| {
                        GeometryError::Gpl("GX layout references missing normal array".into())
                    })?;
                    if index >= values.len() {
                        return Err(GeometryError::Gpl(
                            "GX normal index exceeds source array".into(),
                        ));
                    }
                    out_normals.as_mut().unwrap().push(values[index]);
                    normal_map.push(index);
                }
                out_positions.push(positions[pos_index]);
                out_texcoords.push(texcoords.get(tex_index).copied().unwrap_or([0.0; 2]));
                if let Some(index) = secondary_tex_index {
                    let uv = texcoords.get(index).ok_or_else(|| {
                        GeometryError::Gpl("secondary UV index exceeds source array".into())
                    })?;
                    secondary_texcoords.as_mut().unwrap().push(*uv);
                }
                vertex_map.push([pos_index, tex_index]);
            }
            match primitive {
                0 | 1 => {
                    for group in (0..count).step_by(4) {
                        if group + 3 < count {
                            quad_indices(&mut indices, first_vertex + group);
                        }
                    }
                }
                2 => {
                    for group in (0..count).step_by(3) {
                        if group + 2 < count {
                            tri_indices(
                                &mut indices,
                                first_vertex + group,
                                first_vertex + group + 1,
                                first_vertex + group + 2,
                            );
                        }
                    }
                }
                3 => {
                    for group in 0..count.saturating_sub(2) {
                        let (a, b, c) = if group % 2 == 0 {
                            (group, group + 1, group + 2)
                        } else {
                            (group + 1, group, group + 2)
                        };
                        tri_indices(
                            &mut indices,
                            first_vertex + a,
                            first_vertex + b,
                            first_vertex + c,
                        );
                    }
                }
                4 => {
                    for group in 1..count.saturating_sub(1) {
                        tri_indices(
                            &mut indices,
                            first_vertex,
                            first_vertex + group,
                            first_vertex + group + 1,
                        );
                    }
                }
                _ => {}
            }
            cursor = end;
        } else {
            cursor += gx_command_size(opcode, &data[cursor..])?;
        }
    }
    Ok(GeometryMesh {
        positions: out_positions,
        texcoords: out_texcoords,
        secondary_texcoords,
        colors: out_colors.filter(|values| !values.is_empty()),
        normals: out_normals.filter(|values| !values.is_empty()),
        indices,
        vertex_map,
        color_map,
        normal_map,
        joints,
    })
}

fn vertex_specs(vcd: u32) -> Result<Vec<VertexAttributeSpec>, GeometryError> {
    let mut specs = Vec::new();
    let mut add = |attr: u8, kind: u8| {
        if kind != 0 {
            specs.push(VertexAttributeSpec { attr, kind });
        }
    };
    add(0, (vcd & 3) as u8);
    for (attr, shift) in (9_u8..=20).zip((2_u32..=24).step_by(2)) {
        add(attr, ((vcd >> shift) & 3) as u8);
    }
    add(25, ((vcd >> 26) & 3) as u8);
    if !specs.iter().any(|spec| spec.attr == 9) {
        return Err(GeometryError::Gpl(format!(
            "GX VCD 0x{vcd:08X} has no position attribute"
        )));
    }
    Ok(specs)
}

fn index_width(kind: u8) -> usize {
    match kind {
        1 | 2 => 1,
        3 => 2,
        _ => 0,
    }
}

fn read_index(data: &[u8], offset: usize, size: usize) -> usize {
    if size == 1 {
        usize::from(data[offset])
    } else {
        usize::from(u16::from_be_bytes([data[offset], data[offset + 1]]))
    }
}
fn tri_indices(indices: &mut Vec<u32>, a: usize, b: usize, c: usize) {
    // Retail GPL display lists describe outward-facing polygons clockwise in
    // the model coordinate system. glTF defines counter-clockwise triangles
    // as front-facing, so preserve the vertex/UV pairing but reverse the
    // winding at the interchange boundary.
    indices.extend([a as u32, c as u32, b as u32]);
}
fn quad_indices(indices: &mut Vec<u32>, base: usize) {
    tri_indices(indices, base, base + 1, base + 2);
    tri_indices(indices, base, base + 2, base + 3);
}

fn gx_command_size(opcode: u8, data: &[u8]) -> Result<usize, GeometryError> {
    let size = match opcode {
        0x08 => 6,
        0x10 => {
            if data.len() < 5 {
                return Err(GeometryError::Gpl("truncated GX XF command".into()));
            }
            // XF commands carry a BE command word after the opcode.  The
            // low nibble of its high half-word is the number of extra 32-bit
            // register words minus one (GX FIFO/XF semantics).
            let command = u32::from_be_bytes(data[1..5].try_into().unwrap());
            5 + (((command >> 16) as usize & 0xF) + 1) * 4
        }
        0x20 | 0x28 | 0x30 | 0x38 | 0x40 | 0x61 => {
            if opcode == 0x40 {
                9
            } else {
                5
            }
        }
        0x44 => 5,
        0x48 => 1,
        _ => {
            return Err(GeometryError::Gpl(format!(
                "unsupported GX command 0x{opcode:02X}"
            )));
        }
    };
    if data.len() < size {
        return Err(GeometryError::Gpl("GX command exceeds display list".into()));
    }
    Ok(size)
}

// glTF component and buffer-target codes.
const FLOAT: u32 = 5126;
const UNSIGNED_INT: u32 = 5125;
const UNSIGNED_SHORT: u32 = 5123;
const VERTEX_BUFFER: u32 = 34962;
const INDEX_BUFFER: u32 = 34963;

fn append_floats(buffer: &mut Vec<u8>, values: impl IntoIterator<Item = f32>) -> usize {
    align4(buffer);
    let offset = buffer.len();
    buffer.extend(values.into_iter().flat_map(f32::to_le_bytes));
    offset
}

fn float_attribute<const N: usize>(
    buffer: &mut Vec<u8>,
    views: &mut Vec<serde_json::Value>,
    accessors: &mut Vec<serde_json::Value>,
    data: &[[f32; N]],
) -> usize {
    let offset = append_floats(buffer, data.iter().flatten().copied());
    let view = views.len();
    views.push(json!({"buffer":0,"byteOffset":offset,"byteLength":std::mem::size_of_val(data),"target":VERTEX_BUFFER}));
    let accessor = accessors.len();
    accessors.push(json!({"bufferView":view,"componentType":FLOAT,"count":data.len(),"type":format!("VEC{N}")}));
    accessor
}

fn append_geometry_mesh(
    buffer: &mut Vec<u8>,
    views: &mut Vec<serde_json::Value>,
    accessors: &mut Vec<serde_json::Value>,
    mesh: &GeometryMesh,
    material: usize,
) -> (
    usize,
    usize,
    Option<usize>,
    Option<usize>,
    serde_json::Value,
) {
    let position_offset = append_floats(buffer, mesh.positions.iter().flatten().copied());
    let texcoord_offset = append_floats(buffer, mesh.texcoords.iter().flatten().copied());
    let color_accessor = mesh
        .colors
        .as_ref()
        .map(|data| float_attribute(buffer, views, accessors, data));
    let normal_accessor = mesh
        .normals
        .as_ref()
        .map(|data| float_attribute(buffer, views, accessors, data));
    align4(buffer);
    let index_offset = buffer.len();
    for index in &mesh.indices {
        buffer.extend_from_slice(&index.to_le_bytes());
    }
    let view_base = views.len();
    views.push(json!({"buffer":0,"byteOffset":position_offset,"byteLength":mesh.positions.len()*12,"target":VERTEX_BUFFER}));
    views.push(json!({"buffer":0,"byteOffset":texcoord_offset,"byteLength":mesh.texcoords.len()*8,"target":VERTEX_BUFFER}));
    views.push(json!({"buffer":0,"byteOffset":index_offset,"byteLength":mesh.indices.len()*4,"target":INDEX_BUFFER}));
    let accessor_base = accessors.len();
    let (min, max) = bounds(&mesh.positions);
    accessors.push(json!({"bufferView":view_base,"componentType":FLOAT,"count":mesh.positions.len(),"type":"VEC3","min":min,"max":max}));
    accessors.push(json!({"bufferView":view_base+1,"componentType":FLOAT,"count":mesh.texcoords.len(),"type":"VEC2"}));
    accessors.push(json!({"bufferView":view_base+2,"componentType":UNSIGNED_INT,"count":mesh.indices.len(),"type":"SCALAR"}));
    let mut attributes = json!({"POSITION":accessor_base,"TEXCOORD_0":accessor_base+1});
    if let Some(uvs) = &mesh.secondary_texcoords {
        attributes["TEXCOORD_1"] = json!(float_attribute(buffer, views, accessors, uvs));
    }
    if let Some(accessor) = color_accessor {
        attributes["COLOR_0"] = json!(accessor);
    }
    if let Some(accessor) = normal_accessor {
        attributes["NORMAL"] = json!(accessor);
    }
    if let Some(joints) = &mesh.joints {
        align4(buffer);
        let offset = buffer.len();
        for joint in joints {
            for value in [*joint, 0, 0, 0] {
                buffer.extend(value.to_le_bytes());
            }
        }
        let view = views.len();
        views.push(
            json!({"buffer":0,"byteOffset":offset,"byteLength":joints.len()*8,"target":VERTEX_BUFFER}),
        );
        attributes["JOINTS_0"] = json!(accessors.len());
        accessors.push(
            json!({"bufferView":view,"componentType":UNSIGNED_SHORT,"count":joints.len(),"type":"VEC4"}),
        );
        let offset = append_floats(buffer, joints.iter().flat_map(|_| [1., 0., 0., 0.]));
        let view = views.len();
        views.push(
            json!({"buffer":0,"byteOffset":offset,"byteLength":joints.len()*16,"target":VERTEX_BUFFER}),
        );
        attributes["WEIGHTS_0"] = json!(accessors.len());
        accessors.push(
            json!({"bufferView":view,"componentType":FLOAT,"count":joints.len(),"type":"VEC4"}),
        );
    }
    let primitive =
        json!({"mode":4,"attributes":attributes,"indices":accessor_base+2,"material":material});
    (
        accessor_base,
        accessor_base + 1,
        color_accessor,
        normal_accessor,
        json!({"name":"GPL object","primitives":[primitive]}),
    )
}

fn bounds(points: &[[f32; 3]]) -> ([f32; 3], [f32; 3]) {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for point in points {
        for axis in 0..3 {
            min[axis] = min[axis].min(point[axis]);
            max[axis] = max[axis].max(point[axis]);
        }
    }
    if points.is_empty() {
        ([0.0; 3], [0.0; 3])
    } else {
        (min, max)
    }
}

fn gltf_wrap(value: u32) -> u32 {
    match value {
        2 => 33648, // MIRRORED_REPEAT
        1 => 10497, // REPEAT
        _ => 33071, // CLAMP_TO_EDGE
    }
}

fn gltf_min_filter(value: u32) -> u32 {
    match value {
        0 => 9728, // NEAREST
        1 => 9729, // LINEAR
        2 => 9984, // NEAREST_MIPMAP_NEAREST
        3 => 9985, // LINEAR_MIPMAP_NEAREST
        4 => 9986, // NEAREST_MIPMAP_LINEAR
        _ => 9987, // LINEAR_MIPMAP_LINEAR
    }
}

fn gltf_mag_filter(value: u32) -> u32 {
    if value == 0 { 9728 } else { 9729 }
}

fn write_png(path: &Path, width: u16, height: u16, rgba: &[u8]) -> Result<(), GeometryError> {
    let file = fs::File::create(path).map_err(|source| io_error(path, source))?;
    let writer = std::io::BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, u32::from(width), u32::from(height));
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut stream = encoder
        .write_header()
        .map_err(|error| GeometryError::Png(error.to_string()))?;
    stream
        .write_image_data(rgba)
        .map_err(|error| GeometryError::Png(error.to_string()))
}

fn bind_transform(
    index: usize,
    nodes: &[ModelNodeInfo],
    parents: &[Option<usize>],
) -> Result<glam::Mat4, GeometryError> {
    let mut chain = Vec::new();
    let mut cursor = Some(index);
    while let Some(i) = cursor {
        if i >= nodes.len() || chain.contains(&i) {
            return Err(GeometryError::Gpl("invalid skeleton hierarchy".into()));
        }
        chain.push(i);
        cursor = parents[i];
    }
    let mut matrix = glam::Mat4::IDENTITY;
    for i in chain.into_iter().rev() {
        let n = &nodes[i];
        let local = glam::Mat4::from_scale_rotation_translation(
            glam::Vec3::from_array(n.scale),
            glam::Quat::from_array(n.rotation),
            glam::Vec3::from_array(n.translation),
        );
        matrix *= local;
    }
    Ok(matrix)
}

fn model_node_info(blob: &ModelBlobJson) -> Vec<ModelNodeInfo> {
    let names = decode_model_hex(&blob.raw_hex)
        .ok()
        .map(|raw| {
            let start = blob.optional_data_offset as usize;
            if start >= raw.len() {
                return Vec::new();
            }
            raw[start..]
                .split(|byte| *byte == 0)
                .filter(|part| !part.is_empty())
                .filter_map(|part| std::str::from_utf8(part).ok().map(ToOwned::to_owned))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    blob.nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            let flags = node.data_words.first().map_or(0, |word| (word >> 24) as u8);
            let word_f32 = |offset: usize| {
                node.data_words
                    .get(offset)
                    .map_or(0.0, |word| f32::from_bits(*word))
            };
            let translation = if flags & 8 != 0 {
                [word_f32(8), word_f32(9), word_f32(10)]
            } else {
                [0.0; 3]
            };
            let rotation = if flags & 4 != 0 {
                [word_f32(4), word_f32(5), word_f32(6), word_f32(7)]
            } else {
                [0.0, 0.0, 0.0, 1.0]
            };
            let scale = if flags & 1 != 0 {
                [word_f32(1), word_f32(2), word_f32(3)]
            } else {
                [1.0; 3]
            };
            ModelNodeInfo {
                index,
                object_index: usize::from(node.object_index),
                name: names
                    .get(index)
                    .cloned()
                    .unwrap_or_else(|| format!("gpl_node_{index:03}")),
                translation,
                rotation,
                scale,
            }
        })
        .collect()
}

fn decode_model_hex(text: &str) -> Result<Vec<u8>, String> {
    if !text.len().is_multiple_of(2) {
        return Err("hex payload has odd length".into());
    }
    (0..text.len())
        .step_by(2)
        .map(|offset| {
            u8::from_str_radix(&text[offset..offset + 2], 16).map_err(|error| error.to_string())
        })
        .collect()
}

fn align4(data: &mut Vec<u8>) {
    while !data.len().is_multiple_of(4) {
        data.push(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires locally extracted GQSEAF setup map; Rust cooking only"]
    fn original_setup_geometry_uses_checked_vertex_arrays() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let archive =
            crate::field::MapArchive::open(&root.join("extracted/disc1/files/MAP/_custom.bin"))
                .unwrap();
        let manifest = export_section(
            archive.section(0).unwrap(),
            &root.join("analysis/new-game-setup/geometry"),
        )
        .unwrap();
        assert!(!manifest.objects.is_empty());
    }

    #[test]
    fn packed_vertex_colors_preserve_white_and_independent_alpha() {
        for (format, bytes, expected) in [
            (0, vec![0xff, 0xff], [255u8, 255, 255, 255]),
            (0x30, vec![0xf1, 0x28], [255, 17, 34, 136]),
            (0x40, vec![0xfc, 0x1f, 0xca], [255, 4, 255, 40]),
        ] {
            let desc = VertexArrayDesc {
                data_offset: 0,
                count: 1,
                format,
                components: 4,
                scale: 1.,
            };
            assert_eq!(
                decode_colors(&bytes, desc)[0],
                expected.map(|v| f32::from(v) / 255.)
            );
        }
    }

    #[test]
    fn model_draw_order_preserves_root_priorities_and_depth_first_ties() {
        let mut bytes = vec![0u8; 32 + 3 * 28];
        bytes[..4].copy_from_slice(&0x007b7960u32.to_be_bytes());
        bytes[6..8].copy_from_slice(&3u16.to_be_bytes());
        bytes[12..16].copy_from_slice(&32u32.to_be_bytes());
        // Node 0's children are node 2, then node 1, despite table order.
        bytes[48..52].copy_from_slice(&88u32.to_be_bytes());
        bytes[52..54].copy_from_slice(&u16::MAX.to_be_bytes());
        bytes[80..82].copy_from_slice(&1u16.to_be_bytes());
        bytes[85] = 5;
        bytes[96..100].copy_from_slice(&60u32.to_be_bytes());
        bytes[108..110].copy_from_slice(&2u16.to_be_bytes());
        bytes[113] = 5;
        let mut model = ModelBlobJson::parse(&bytes, 0).unwrap();
        assert_eq!(model_draw_order(&model).unwrap(), [0, 2, 1]);
        model.nodes[1].field19 = 4;
        assert_eq!(model_draw_order(&model).unwrap(), [0, 1, 2]);
        model.nodes[1].next_offset = 32;
        assert!(model_draw_order(&model).is_err());
    }

    #[test]
    fn draws_follow_commands_beyond_the_first_three_records() {
        let mut object = vec![0; 128];
        for (index, (kind, value, offset, size)) in [
            (1u32, 0x11110002u32, 0u32, 0u32),
            (1, 0x11112000, 0, 0),
            (3, 17, 0, 0),
            (2, 0x2888, 96, 12),
            (4, 0x00070002, 108, 20),
        ]
        .into_iter()
        .enumerate()
        {
            let at = index * 16;
            object[at] = kind as u8;
            for (i, value) in [value, offset, size].into_iter().enumerate() {
                object[at + 4 + i * 4..at + 8 + i * 4].copy_from_slice(&value.to_be_bytes());
            }
        }
        let draws = parse_render_commands(&object, 0, 5, true).unwrap();
        assert_eq!(draws.len(), 2);
        assert_eq!((draws[0].1, draws[0].2), (96, 12));
        assert_eq!(draws[0].0.texture_commands, [0x11110002, 0x11112000]);
        assert_eq!(draws[1].0.matrix_commands, [0x00070002]);
        assert!(parse_render_commands(&object[..127], 0, 5, true).is_err());
    }

    #[test]
    fn old_palette_commands_keep_vertex_layout_separate_from_material_mode() {
        let mut object = vec![0; 80];
        for (index, (kind, value, offset, size)) in [(4u8, 4u32, 0u32, 0u32), (3, 8, 64, 16)]
            .into_iter()
            .enumerate()
        {
            let at = index * 16;
            object[at] = kind;
            for (i, value) in [value, offset, size].into_iter().enumerate() {
                object[at + 4 + i * 4..at + 8 + i * 4].copy_from_slice(&value.to_be_bytes());
            }
        }
        let draws = parse_render_commands(&object, 0, 2, false).unwrap();
        assert_eq!(draws[0].0.vcd, Some(8));
        assert_eq!(draws[0].0.tev_modes, [5]);
        assert!(draws[0].0.matrix_commands.is_empty());
    }

    #[test]
    fn matrix_indices_become_standard_joint_indices() {
        let state = RenderStateInfo {
            vcd: Some(0x809),
            matrix_commands: vec![0x00070002],
            ..Default::default()
        };
        // One triangle: matrix slot 2 (encoded as 6), position, texture index.
        let data = [0x90, 0, 3, 6, 0, 0, 6, 1, 1, 6, 2, 2];
        let positions = [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]];
        let uvs = [[0., 0.], [1., 0.], [0., 1.]];
        let mesh = decode_display_list(&data, &positions, &uvs, None, None, &state).unwrap();
        assert_eq!(mesh.positions, positions);
        assert_eq!(mesh.indices, [0, 2, 1]);
        assert_eq!(mesh.joints.unwrap(), [7, 7, 7]);
        let unbound = RenderStateInfo {
            matrix_commands: vec![],
            ..state
        };
        assert!(decode_display_list(&data, &positions, &uvs, None, None, &unbound).is_err());
    }

    #[test]
    fn light_material_retains_independent_secondary_uv_indices() {
        let state = RenderStateInfo {
            vcd: Some(0x2808),
            ..Default::default()
        };
        let data = [0x90, 0, 3, 0, 0, 2, 1, 1, 0, 2, 2, 1];
        let positions = [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]];
        let uvs = [[0., 0.], [1., 0.], [0., 1.]];
        let mesh = decode_display_list(&data, &positions, &uvs, None, None, &state).unwrap();
        assert_eq!(mesh.texcoords, uvs);
        assert_eq!(
            mesh.secondary_texcoords.as_ref().unwrap(),
            &[uvs[2], uvs[0], uvs[1]]
        );
        let mut buffer = Vec::new();
        let mut views = Vec::new();
        let mut accessors = Vec::new();
        let (_, _, _, _, gltf) =
            append_geometry_mesh(&mut buffer, &mut views, &mut accessors, &mesh, 0);
        let uv1 = gltf["primitives"][0]["attributes"]["TEXCOORD_1"]
            .as_u64()
            .unwrap() as usize;
        assert_eq!(accessors[uv1]["count"], 3);
        let mut invalid = data;
        invalid[5] = 3;
        assert!(decode_display_list(&invalid, &positions, &uvs, None, None, &state).is_err());
    }
}
