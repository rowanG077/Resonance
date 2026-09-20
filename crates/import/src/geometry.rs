//! Decode field geometry into editable glTF meshes during import.

use std::fs;
use std::path::{Path, PathBuf};

use self::GeometryError::Gpl;
use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;
mod vertex;

use crate::model::Model;
use crate::tpl::{decode_texture, parse_tpl, read_u16, read_u32};

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
    #[error("PNG error: {0}")]
    Png(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TextureInfo {
    pub index: usize,
    pub width: u16,
    pub height: u16,
    pub image: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeometryMetadata {
    pub textures: Vec<TextureInfo>,
    pub objects: Vec<GeometryObjectInfo>,
    /// Runtime model nodes which place geometry objects in a field actor.
    /// Geometry-only GPLs leave this empty; expanded MAP sections may carry a
    /// trailing model blob that supplies these transforms.
    pub model_nodes: Vec<ModelNodeInfo>,
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
    /// Model node owning this draw; absent for a root draw or geometry-only resource.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_node: Option<usize>,
    /// Draw sequence recovered from model priorities and hierarchy traversal.
    pub draw_order: u32,
    pub name: String,
    pub material: usize,
    pub position_count: usize,
    pub texcoord_count: usize,
    pub display_offset: usize,
    pub display_size: usize,
    /// Decoded indices, including authored zero-primitive draws.
    pub index_count: usize,
    pub position_accessor: usize,
    pub texcoord_accessor: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_accessor: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normal_accessor: Option<usize>,
    /// Array indices; inline attributes have no source array index.
    pub vertex_map: Vec<[Option<usize>; 2]>,
    #[serde(default)]
    pub color_map: Vec<Option<usize>>,
    #[serde(default)]
    pub normal_map: Vec<Option<usize>>,
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
    /// Authored table labels for each texture stage; these are not file paths.
    pub texture_tables: Vec<Option<String>>,
    #[serde(default)]
    pub tev_modes: Vec<u32>,
    #[serde(default)]
    pub matrix_commands: Vec<u32>,
}

/// For a nonzero stage count, start with vertex color and apply the operations
/// in order, clamping to [0, 1]. Texture indices identify authored UV/sampler slots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MaterialRecipe {
    pub context: MaterialContext,
    pub source_mode: u32,
    pub source_texture_count: u8,
    /// Requested stage count, including pass-through stages. Zero requires prior
    /// graphics state and has no standalone ordinary-material projection.
    pub stage_count: u8,
    pub operations: Vec<MaterialOperation>,
}

/// Ordinary pass with the constructor-installed callback and no caller render-mode
/// override. Lighting overlays and replacement callbacks require their own recipes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MaterialContext {
    DefaultGplOrdinary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum MaterialOperation {
    MultiplyTexture {
        texture: u8,
    },
    /// Interpolate RGB toward the texture using its alpha; preserve previous alpha.
    DecalTexture {
        texture: u8,
    },
    /// Interpolate RGB toward white using texture RGB; multiply alpha by texture alpha.
    BlendTexture {
        texture: u8,
    },
    ReplaceTexture {
        texture: u8,
    },
    /// Add texture RGB, clamp to [0, 1], and preserve previous alpha.
    AddTexture {
        texture: u8,
    },
    /// Subtract texture RGB, clamp to [0, 1], and preserve previous alpha.
    SubtractTexture {
        texture: u8,
    },
    /// Requires the draw matrix for its UV transform, prior alpha to weight the
    /// added texture RGB, and the caller's constant alpha for its output alpha.
    DefaultViewTexture {
        texture: u8,
    },
    MultiplyVertexColor,
}

impl MaterialRecipe {
    /// Decode the ordinary material path of a freshly constructed GPL object.
    pub(crate) fn parse(modes: &[u32], texture_count: usize) -> Result<Self, GeometryError> {
        let &[source_mode] = modes else {
            return Err(Gpl("material needs one authored recipe".into()));
        };
        if texture_count > 8 {
            return Err(Gpl("material texture count exceeds eight slots".into()));
        }
        use MaterialOperation::*;
        let mut operations = Vec::new();
        let mut stage = 0;
        for shift in (0..32).step_by(4) {
            let operation = match (source_mode >> shift) & 15 {
                0 => break,
                5 => {
                    stage += 1;
                    break;
                }
                1 => Some(MultiplyTexture { texture: stage }),
                2 => Some(DecalTexture { texture: stage }),
                3 => Some(BlendTexture { texture: stage }),
                4 => Some(ReplaceTexture { texture: stage }),
                6 => Some(AddTexture { texture: stage }),
                7 => Some(SubtractTexture { texture: stage }),
                8 => Some(DefaultViewTexture { texture: stage }),
                15 => {
                    operations.push(DecalTexture { texture: stage });
                    stage += 1;
                    Some(MultiplyVertexColor)
                }
                // The default callback only reports kinds 9..14; neither counter advances.
                _ => None,
            };
            if let Some(operation) = operation {
                operations.push(operation);
                stage += 1;
            }
            if usize::from(stage) >= texture_count {
                break;
            }
        }
        Ok(Self {
            context: MaterialContext::DefaultGplOrdinary,
            source_mode,
            source_texture_count: texture_count as u8,
            stage_count: stage,
            operations,
        })
    }

    pub(crate) fn textures(&self) -> impl Iterator<Item = u8> + '_ {
        self.operations
            .iter()
            .filter_map(|operation| match operation {
                MaterialOperation::MultiplyTexture { texture }
                | MaterialOperation::DecalTexture { texture }
                | MaterialOperation::BlendTexture { texture }
                | MaterialOperation::ReplaceTexture { texture }
                | MaterialOperation::AddTexture { texture }
                | MaterialOperation::SubtractTexture { texture }
                | MaterialOperation::DefaultViewTexture { texture } => Some(*texture),
                MaterialOperation::MultiplyVertexColor => None,
            })
    }

    /// Select only expressions implemented by the current scene shader.
    pub(crate) fn combination(&self) -> Result<MaterialCombination, GeometryError> {
        use MaterialOperation::*;
        if self.stage_count == 0 {
            return Err(Gpl(
                "scene shader cannot project a zero-stage material".into()
            ));
        }
        Ok(match self.operations.as_slice() {
            [] => MaterialCombination::VertexColor,
            [MultiplyTexture { texture: 0 }] => MaterialCombination::VertexColorTexture,
            [ReplaceTexture { texture: 0 }] => MaterialCombination::Texture,
            [
                MultiplyTexture { texture: 0 },
                MultiplyTexture { texture: 1 },
            ] => MaterialCombination::VertexColorMultiplyTextures,
            [
                ReplaceTexture { texture: 0 },
                MultiplyTexture { texture: 1 },
            ] => MaterialCombination::MultiplyTextures,
            _ => {
                return Err(Gpl(format!(
                    "scene shader does not support material operations {:?}",
                    self.operations
                )));
            }
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MaterialCombination {
    VertexColor,
    Texture,
    VertexColorTexture,
    MultiplyTextures,
    VertexColorMultiplyTextures,
}

impl MaterialCombination {
    pub(crate) fn texture_count(self) -> u8 {
        match self {
            Self::VertexColor => 0,
            Self::Texture | Self::VertexColorTexture => 1,
            Self::MultiplyTextures | Self::VertexColorMultiplyTextures => 2,
        }
    }

    pub(crate) fn vertex_color(self) -> bool {
        !matches!(self, Self::Texture | Self::MultiplyTextures)
    }
}

const fn default_fraction_bits() -> u8 {
    8
}

const fn default_true() -> bool {
    true
}

/// Bounded geometry, texture and skeleton resources from one model container.
struct Section<'a> {
    tpl: &'a [u8],
    gpl: &'a [u8],
    model: Option<Model>,
}

/// Select a primary model while validating and bounding its actor directory.
pub(crate) fn model_resource(section: &[u8]) -> Result<(usize, &[u8]), GeometryError> {
    if read_u32(section, 0) != Some(31) {
        return Ok((0, section));
    }
    let parts = crate::field::sections(section)
        .map_err(|error| Gpl(format!("invalid actor section directory: {error}")))?;
    let range = parts[0]
        .as_ref()
        .ok_or_else(|| Gpl("actor package has no primary model".into()))?;
    Ok((range.start, &section[range.clone()]))
}

fn section_source(section: &[u8]) -> Result<Section<'_>, GeometryError> {
    if section.len() < 0x20 {
        return Err(Gpl("MAP section is shorter than its resource header".into()));
    }
    let (_, resource) = model_resource(section)?;
    let tpl_offset =
        read_u32(resource, 0).ok_or_else(|| Gpl("MAP section has no TPL offset".into()))? as usize;
    let tpl_end = read_u32(resource, 4)
        .ok_or_else(|| Gpl("MAP section has no TPL end offset".into()))? as usize;
    if tpl_offset < 0x20 || tpl_end < tpl_offset || tpl_end > resource.len() {
        return Err(Gpl(format!(
            "invalid MAP resource ranges 0x{tpl_offset:X}..0x{tpl_end:X}"
        )));
    }
    let gpl = &resource[0x20..tpl_offset];
    let tpl = &resource[tpl_offset..tpl_end];
    if !matches!(read_u32(gpl, 0), Some(0x005B_BC61 | 0x00B7_49E0)) {
        return Err(Gpl(
            "MAP section does not contain a geometry-palette GPL".into()
        ));
    }
    // The resource header gives the exact copied skeleton span after the TPL.
    let model = trailing_model(resource)?.map(|(model, _, _)| model);
    Ok(Section { tpl, gpl, model })
}

/// Decode geometry and its per-node draw closure without files or image conversion.
#[cfg(test)]
pub(crate) fn preflight_section(section: &[u8]) -> Result<(), GeometryError> {
    let source = section_source(section)?;
    parse_tpl(source.tpl)?;
    let objects = parse_geometry(source.gpl)?;
    object_draw_recipes(&objects, source.model.as_ref(), DecodeMode::Runtime)?;
    if let Some(model) = &source.model {
        let nodes = model_node_info(model);
        let parents = model_parents(model);
        if objects.iter().any(|object| object.mesh.joints.is_some()) {
            for index in 0..nodes.len() {
                if !bind_transform(index, &nodes, &parents)?
                    .inverse()
                    .is_finite()
                {
                    return Err(Gpl("singular bind transform".into()));
                }
            }
        }
    }
    for object in objects {
        if object.mesh.joints.as_ref().is_some_and(|joints| {
            joints.iter().any(|&j| {
                source
                    .model
                    .as_ref()
                    .is_none_or(|m| usize::from(j) >= m.nodes.len())
            })
        }) {
            return Err(Gpl("joint index exceeds model hierarchy".into()));
        }
    }
    Ok(())
}

/// Compare a bound field against source hierarchy/draw recipes without conversion.
#[cfg(test)]
pub(crate) fn compare_field_layer(
    section: &[u8],
    part: &resonance_content::ScenePart,
    gltf: &serde_json::Value,
    order: u32,
) -> anyhow::Result<()> {
    use anyhow::{Context, ensure};
    let source = section_source(section)?;
    let objects = parse_geometry(source.gpl)?;
    let draws = object_draw_recipes(&objects, source.model.as_ref(), DecodeMode::Runtime)?;
    let bones = source
        .model
        .as_ref()
        .map(model_node_info)
        .unwrap_or_default();
    let parents = source.model.as_ref().map(model_parents).unwrap_or_default();
    let nodes = gltf["nodes"].as_array().context("missing field nodes")?;
    let roots = gltf["scenes"][0]["nodes"]
        .as_array()
        .context("missing field roots")?;
    let mut original = bones
        .iter()
        .map(|bone| {
            json!({
                "name":bone.name,"translation":bone.translation,"rotation":bone.rotation,
                "scale":bone.scale,"children":[]
            })
        })
        .collect::<Vec<_>>();
    ensure!(
        part.bone_names
            == bones
                .iter()
                .map(|bone| bone.name.clone())
                .collect::<Vec<_>>(),
        "field bone names differ from source"
    );
    for (index, parent) in parents.iter().enumerate() {
        ensure!(
            nodes[index]["name"] == original[index]["name"],
            "field bone {index}/name differs"
        );
        for field in ["translation", "rotation", "scale"] {
            // GLB JSON can round through f64; the renderer and source use f32.
            let actual: Vec<f32> = serde_json::from_value(nodes[index][field].clone())?;
            let expected: Vec<f32> = serde_json::from_value(original[index][field].clone())?;
            ensure!(actual == expected, "field bone {index}/{field} differs");
        }
        if let Some(parent) = parent {
            original[*parent]["children"]
                .as_array_mut()
                .unwrap()
                .push(json!(index));
            ensure!(
                nodes[*parent]["children"]
                    .as_array()
                    .context("missing bone children")?
                    .contains(&json!(index)),
                "field bone parent differs"
            );
        } else {
            ensure!(roots.contains(&json!(index)), "field bone root differs");
        }
    }
    ensure!(
        serde_json::to_value(crate::field_doors::cook(&json!({"nodes":original}))?)?
            == serde_json::to_value(crate::field_doors::cook(gltf)?)?,
        "field doors differ from source hierarchy"
    );
    let draws = draws
        .iter()
        .filter(|draw| !objects[draw.object].mesh.indices.is_empty())
        .collect::<Vec<_>>();
    let meshes = gltf["meshes"].as_array().context("missing field meshes")?;
    ensure!(
        part.materials.len() == draws.len()
            && part.material_nodes.len() == draws.len()
            && meshes.len() == draws.len(),
        "field draw count differs"
    );
    for (index, draw) in draws.into_iter().enumerate() {
        let object = &objects[draw.object];
        let material = &part.materials[index];
        ensure!(
            material.draw_order == order * 65536 + draw.order
                && material.depth_write == (part.resource != 2)
                && material.cull == resonance_content::CullFace::Back,
            "field draw order/depth/culling differs"
        );
        ensure!(
            part.material_nodes[index]
                == draw
                    .model_node
                    .into_iter()
                    .map(|node| node as u16)
                    .collect::<Vec<_>>(),
            "field draw visibility binding differs"
        );
        let count = meshes[index]["primitives"]
            .as_array()
            .context("missing field primitives")?
            .iter()
            .map(|primitive| {
                let accessor = primitive["indices"]
                    .as_u64()
                    .context("missing index accessor")?;
                gltf["accessors"][accessor as usize]["count"]
                    .as_u64()
                    .context("missing index count")
            })
            .collect::<anyhow::Result<Vec<_>>>()?
            .into_iter()
            .sum::<u64>();
        ensure!(
            count == object.mesh.indices.len() as u64,
            "field primitive count differs"
        );
        let mesh_nodes = nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node["mesh"].as_u64() == Some(index as u64))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let [node] = mesh_nodes.as_slice() else {
            anyhow::bail!("field mesh instance count differs");
        };
        if let Some(parent) = draw.model_node.filter(|_| object.mesh.joints.is_none()) {
            ensure!(
                nodes[parent]["children"]
                    .as_array()
                    .context("missing mesh parent")?
                    .contains(&json!(node)),
                "field mesh attachment differs"
            );
        } else {
            ensure!(roots.contains(&json!(node)), "field root draw differs");
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
pub(crate) enum DecodeMode {
    Runtime,
    Physical,
}

pub(crate) struct DecodedGeometry {
    pub manifest: GeometryMetadata,
    pub gltf: serde_json::Value,
    pub binary: Vec<u8>,
}

/// Decode each image once and release its pixels after the caller consumes them.
pub(crate) fn decode_section(
    section: &[u8],
    mode: DecodeMode,
    image: impl FnMut(&TextureInfo, &[u8]) -> Result<(), GeometryError>,
) -> Result<DecodedGeometry, GeometryError> {
    let Section { tpl, gpl, model } = section_source(section)?;
    decode_geometry(tpl, gpl, model.as_ref(), mode, image)
}

/// The native loader copies exactly size bytes from the resource-relative start.
pub(crate) fn skeleton_range(resource: &[u8]) -> Result<std::ops::Range<usize>, GeometryError> {
    let start =
        read_u32(resource, 4).ok_or_else(|| Gpl("missing skeleton offset".into()))? as usize;
    let size = read_u32(resource, 8).ok_or_else(|| Gpl("missing skeleton size".into()))? as usize;
    let end = start
        .checked_add(size)
        .filter(|&end| start >= 0x20 && end <= resource.len())
        .ok_or_else(|| {
            Gpl(format!(
                "skeleton range {start:#x}+{size:#x} exceeds resource"
            ))
        })?;
    Ok(start..end)
}

fn trailing_model(section: &[u8]) -> Result<Option<(Model, usize, usize)>, GeometryError> {
    let range = skeleton_range(section)?;
    if range.is_empty() {
        return Ok(None);
    }
    let model = Model::parse(&section[range.clone()])
        .map_err(|error| Gpl(format!("declared skeleton: {error}")))?;
    Ok(Some((model, range.start, range.len())))
}

fn io_error(path: &Path, source: std::io::Error) -> GeometryError {
    GeometryError::Io {
        path: path.to_path_buf(),
        source,
    }
}

#[derive(Debug, Clone, Default)]
struct GeometryMesh {
    positions: Vec<[f32; 3]>,
    texcoords: Vec<[f32; 2]>,
    extra_texcoords: Vec<(u8, Vec<[f32; 2]>)>,
    colors: Option<Vec<[f32; 4]>>,
    secondary_colors: Option<Vec<[f32; 4]>>,
    unused_colors: Option<Vec<[f32; 4]>>,
    normals: Option<Vec<[f32; 3]>>,
    normal_basis: Option<[Vec<[f32; 3]>; 2]>,
    indices: Vec<u32>,
    primitives: Vec<(Topology, std::ops::Range<usize>)>,
    vertex_map: Vec<[Option<usize>; 2]>,
    color_map: Vec<Option<usize>>,
    normal_map: Vec<Option<usize>>,
    joints: Option<Vec<u16>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum Topology {
    Points = 0,
    Lines = 1,
    Triangles = 4,
}

#[derive(Debug, Clone, Copy)]
struct VertexArrayDesc {
    data_offset: usize,
    count: usize,
    format: u8,
    components: u8,
}

#[derive(Clone, Copy)]
struct VertexArray<'a, const N: usize> {
    desc: VertexArrayDesc,
    values: &'a [[f32; N]],
}

struct VertexArrays<'a> {
    source: Option<&'a [u8]>,
    positions: VertexArray<'a, 3>,
    texcoords: Vec<VertexArray<'a, 2>>,
    colors: Option<VertexArray<'a, 4>>,
    normals: Option<(VertexArrayDesc, &'a NormalArray)>,
}

#[derive(Debug, Clone)]
enum NormalArray {
    Xyz(Vec<[f32; 3]>),
    /// Three independently indexed vectors, with component offsets 0, 3 and 6.
    Nbt3(Vec<[f32; 3]>),
}

impl NormalArray {
    fn read(object: &[u8], desc: VertexArrayDesc) -> Result<Self, GeometryError> {
        Ok(match desc.components {
            2 => Self::Nbt3(decode_vectors(
                object,
                VertexArrayDesc {
                    components: 3,
                    ..desc
                },
                VectorKind::Normal,
            )?),
            3 | 6 => Self::Xyz(decode_vectors(object, desc, VectorKind::Normal)?),
            n => return Err(Gpl(format!("unsupported normal layout {n}"))),
        })
    }

    fn vector(&self, index: usize, component: usize) -> Result<[f32; 3], GeometryError> {
        match self {
            Self::Xyz(values) => values.get(index).copied(),
            Self::Nbt3(values) => values.get(index + component).copied(),
        }
        .ok_or_else(|| {
            Gpl(format!(
                "normal component {component} index {index} exceeds array"
            ))
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct VertexAttributeSpec {
    attr: u8,
    kind: u8,
}

#[derive(Debug, Clone, Default, PartialEq)]
struct RenderStateInfo {
    vcd: Option<u32>,
    vertex_revision: usize,
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
    texture_tables: Vec<Option<String>>,
    color: Option<(VertexArrayDesc, Vec<[f32; 4]>)>,
    normal: Option<(VertexArrayDesc, NormalArray)>,
    render_state: RenderStateInfo,
    mesh: GeometryMesh,
}

/// Draw the optional root object first, then sort by authored priority,
/// preserving depth-first order for ties.
fn model_draw_order(model: &Model) -> Vec<(u16, Option<usize>)> {
    let mut nodes: Vec<_> = model
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| node.object_index != u16::MAX)
        .map(|(index, node)| (node.draw_priority, node.object_index, index))
        .collect();
    nodes.sort_by_key(|(priority, _, _)| *priority);
    let root = model.root_geometry;
    (root != u16::MAX)
        .then_some((root, None))
        .into_iter()
        .chain(
            nodes
                .into_iter()
                .map(|(_, object, node)| (object, Some(node))),
        )
        .collect()
}

#[derive(Debug, Clone, Copy)]
struct DrawRecipe {
    object: usize,
    model_node: Option<usize>,
    order: u32,
    instanced: bool,
}

fn object_draw_recipes(
    objects: &[GeometryObject],
    model: Option<&Model>,
    mode: DecodeMode,
) -> Result<Vec<DrawRecipe>, GeometryError> {
    let Some(model) = model else {
        return Ok((0..objects.len())
            .map(|object| DrawRecipe {
                object,
                model_node: None,
                order: object as u32,
                instanced: true,
            })
            .collect());
    };
    let instances = model_draw_order(model);
    let mut recipes = Vec::new();
    for (object, geometry) in objects.iter().enumerate() {
        let draws: Vec<_> = instances
            .iter()
            .enumerate()
            .filter(|(_, (source, _))| usize::from(*source) == geometry.source_index)
            .collect();
        if draws.is_empty() {
            if matches!(mode, DecodeMode::Physical) {
                recipes.push(DrawRecipe {
                    object,
                    model_node: None,
                    order: u32::MAX,
                    instanced: false,
                });
                continue;
            }
            return Err(Gpl(format!(
                "object {} has no authored draw: {}",
                geometry.source_index, geometry.name
            )));
        }
        if geometry.mesh.joints.is_some() && draws.len() > 1 {
            return Err(Gpl(
                "repeated skinned geometry needs an explicit instance matrix contract".into(),
            ));
        }
        for (rank, (_, node)) in draws {
            recipes.push(DrawRecipe {
                object,
                model_node: *node,
                order: rank as u32,
                instanced: true,
            });
        }
    }
    // Keep mesh table order stable; only the draw recipe priority follows the model queue.
    let mut indices: Vec<_> = (0..recipes.len()).collect();
    indices.sort_by_key(|&i| recipes[i].order);
    for (rank, index) in indices.into_iter().enumerate() {
        recipes[index].order = rank as u32;
    }
    Ok(recipes)
}

fn decode_geometry(
    tpl: &[u8],
    gpl: &[u8],
    model: Option<&Model>,
    mode: DecodeMode,
    mut image: impl FnMut(&TextureInfo, &[u8]) -> Result<(), GeometryError>,
) -> Result<DecodedGeometry, GeometryError> {
    let textures = parse_tpl(tpl)?;
    let objects = parse_geometry(gpl)?;
    let draws = object_draw_recipes(&objects, model, mode)?;
    let mut texture_info = Vec::with_capacity(textures.len());
    let mut texture_alpha = Vec::with_capacity(textures.len());
    for (index, texture) in textures.iter().enumerate() {
        let rgba = decode_texture(tpl, texture)?;
        let info = TextureInfo {
            index,
            width: texture.width,
            height: texture.height,
            image: format!("texture_{index:03}.png"),
        };
        image(&info, &rgba)?;
        texture_info.push(info);
        texture_alpha.push(rgba.chunks_exact(4).any(|pixel| pixel[3] != 0xFF));
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
    let parents = model.map(model_parents).unwrap_or_default();
    for (child, parent) in parents.iter().enumerate() {
        if let Some(parent) = parent {
            nodes[*parent]["children"]
                .as_array_mut()
                .unwrap()
                .push(json!(child));
        }
    }
    let mut roots: Vec<usize> = (0..nodes.len()).filter(|i| parents[*i].is_none()).collect();
    let has_skin = objects.iter().any(|o| o.mesh.joints.is_some());
    if has_skin && model_nodes.is_empty() {
        return Err(Gpl("skinned geometry has no model hierarchy".into()));
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
                return Err(Gpl("singular bind transform".into()));
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
    // Instance meshes share vertex/index accessors; their material recipes remain independent.
    let geometry: Vec<_> = objects
        .iter()
        .map(|object| {
            append_geometry_mesh(
                &mut buffer,
                &mut views,
                &mut accessors,
                &object.mesh,
                object.material,
            )
        })
        .collect();
    let mut object_info = Vec::with_capacity(draws.len());
    for (index, draw) in draws.iter().enumerate() {
        let object = &objects[draw.object];
        let (position_accessor, texcoord_accessor, color_accessor, normal_accessor, mesh) =
            &geometry[draw.object];
        meshes.push(mesh.clone());
        let mesh_node = json!({"name": object.name, "mesh": index});
        if let Some(joints) = &object.mesh.joints {
            if joints.iter().any(|j| usize::from(*j) >= model_nodes.len()) {
                return Err(Gpl("joint index exceeds model hierarchy".into()));
            }
            let mut node = mesh_node;
            node["skin"] = json!(0);
            if draw.instanced {
                roots.push(nodes.len());
            }
            nodes.push(node);
        } else {
            let index = nodes.len();
            nodes.push(mesh_node);
            // Unused physical meshes remain detached glTF nodes; their model
            // does not place them in the authored scene.
            if draw.instanced {
                if let Some(parent) = draw.model_node {
                    nodes[parent]["children"]
                        .as_array_mut()
                        .unwrap()
                        .push(json!(index));
                } else {
                    roots.push(index);
                }
            }
        }
        object_info.push(GeometryObjectInfo {
            index,
            source_index: object.source_index,
            model_node: draw.model_node,
            draw_order: draw.order,
            name: object.name.clone(),
            material: object.material,
            position_count: object.position_count,
            texcoord_count: object.texcoord_count,
            display_offset: object.display_offset,
            display_size: object.display_size,
            index_count: object.mesh.indices.len(),
            position_accessor: *position_accessor,
            texcoord_accessor: *texcoord_accessor,
            color_accessor: *color_accessor,
            normal_accessor: *normal_accessor,
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
            texture_tables: object.texture_tables.clone(),
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
            let has_texture_alpha = texture_alpha[index];
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
    let gltf = json!({"asset":{"version":"2.0","generator":"resonance-import"},"scene":0,"scenes":[{"nodes":roots}],"nodes":nodes,"buffers":[{"uri":"scene.bin","byteLength":buffer.len()}],"bufferViews":views,"accessors":accessors,"meshes":meshes,"skins":skins,"materials":materials,"textures":textures_json,"samplers":samplers,"images":images,"extras":{"resonance":{"format":"TPL+GPL","resource_kind":"gpl","objects":object_extras}}});
    let manifest = GeometryMetadata {
        textures: texture_info,
        objects: object_info,
        model_nodes,
    };
    Ok(DecodedGeometry {
        manifest,
        gltf,
        binary: buffer,
    })
}

#[allow(clippy::too_many_lines)]
fn parse_geometry(data: &[u8]) -> Result<Vec<GeometryObject>, GeometryError> {
    // Both geometry-palette revisions use the same bounded entry/array
    // records. The older revision is used by the original setup map.
    if !matches!(read_u32(data, 0), Some(0x005B_BC61 | 0x00B7_49E0)) || data.len() < 0x14 {
        return Err(Gpl("missing geometry-palette header".into()));
    }
    let auxiliary = [read_u32(data, 4).unwrap(), read_u32(data, 8).unwrap()];
    // The loader relocates the second word when both are nonzero; its payload
    // layout is not known. Do not silently skip a potential resource reference.
    if auxiliary != [0, 0] {
        return Err(Gpl(format!("unresolved GPL header fields {auxiliary:x?}")));
    }
    let count = read_u32(data, 0x0C).unwrap() as usize;
    let entries = read_u32(data, 0x10).unwrap() as usize;
    if entries
        .checked_add(count * 8)
        .is_none_or(|end| end > data.len())
    {
        return Err(Gpl("geometry entry table exceeds file".into()));
    }
    let mut descriptors = Vec::with_capacity(count);
    for index in 0..count {
        let at = entries + index * 8;
        let object_offset = read_u32(data, at).unwrap() as usize;
        let name_offset = read_u32(data, at + 4).unwrap() as usize;
        let name = c_string(data, name_offset)
            .map_err(|error| Gpl(format!("object {index} name: {error}")))?;
        descriptors.push((object_offset, name));
    }
    let mut objects = Vec::with_capacity(count);
    for (index, (offset, name)) in descriptors.iter().enumerate() {
        let end = descriptors.get(index + 1).map_or(data.len(), |item| item.0);
        if *offset < 0x14 || *offset + 0x18 > end || end > data.len() {
            return Err(Gpl(format!("object {index} range is invalid")));
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
            return Err(Gpl(format!("object {index} array records are invalid")));
        }
        let positions_desc = parse_vertex_desc(object, pos_record, "position")?;
        let texture_count = usize::from(object[0x14]);
        if texture_count > 8 || (texture_count != 0 && tex_record == 0) {
            return Err(Gpl(format!(
                "object {index} has invalid texture array count {texture_count}"
            )));
        }
        // Each texture coordinate slot binds its own 16-byte descriptor. Slots
        // can have different lengths and encodings, even within the same draw.
        let texture_arrays = (0..texture_count)
            .map(|slot| {
                let desc = parse_vertex_desc(object, tex_record + slot * 16, "texcoord")?;
                Ok((
                    desc,
                    decode_vectors::<2>(object, desc, VectorKind::Coordinate)?,
                ))
            })
            .collect::<Result<Vec<_>, GeometryError>>()?;
        let texture_tables = (0..texture_count)
            .map(|slot| {
                let at = tex_record + slot * 16 + 8;
                let label = read_u32(object, at)
                    .ok_or_else(|| Gpl("truncated texture table descriptor".into()))?
                    as usize;
                if label == 0 {
                    Ok(None)
                } else {
                    let name = c_string(data, offset.saturating_add(label))?;
                    if name.is_empty() {
                        return Err(Gpl(format!(
                            "object {index} texture table {slot} has an empty label"
                        )));
                    }
                    Ok(Some(name))
                }
            })
            .collect::<Result<Vec<_>, GeometryError>>()?;
        let texcoords_desc = texture_arrays.first().map(|(desc, _)| *desc);
        let positions = decode_vectors::<3>(object, positions_desc, VectorKind::Coordinate)?;
        let color = if color_record != 0 {
            let desc = parse_vertex_desc(object, color_record, "color")?;
            Some((desc, decode_colors(object, desc)?))
        } else {
            None
        };
        let normal = if normal_record != 0 {
            let desc = parse_vertex_desc(object, normal_record, "normal")?;
            Some((desc, NormalArray::read(object, desc)?))
        } else {
            None
        };
        let arrays = VertexArrays {
            source: Some(object),
            positions: VertexArray {
                desc: positions_desc,
                values: &positions,
            },
            texcoords: texture_arrays
                .iter()
                .map(|(desc, values)| VertexArray {
                    desc: *desc,
                    values,
                })
                .collect(),
            colors: color.as_ref().map(|(desc, values)| VertexArray {
                desc: *desc,
                values,
            }),
            normals: normal.as_ref().map(|(desc, values)| (*desc, values)),
        };
        let material_ptr = read_u32(object, material_record + 4).unwrap() as usize;
        let command_count = read_u16(object, material_record + 8)
            .ok_or_else(|| Gpl("missing render command count".into()))?
            as usize;
        let mut formats = vertex::Formats::new(&arrays);
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
            let meshes = decode_display_list_with_formats(
                &object[display_offset..display_offset + display_size],
                &arrays,
                &render_state,
                &mut formats,
            ).map_err(|error| Gpl(format!("object {index} ({name}), draw {draw} at {:#x}, VCD {:?}, {} colors, {} normals: {error}", offset + display_offset, render_state.vcd, color.as_ref().map_or(0, |(_, values)| values.len()), normal.as_ref().map_or(0, |(desc, _)| desc.count))))?;
            for (segment, mesh) in meshes.into_iter().enumerate() {
                let draw_name = if draw == 0 {
                    name.clone()
                } else {
                    format!("{name}/draw{draw}")
                };
                objects.push(GeometryObject {
                    source_index: index,
                    name: if segment == 0 {
                        draw_name
                    } else {
                        format!("{draw_name}/part{segment}")
                    },
                    material,
                    position_count: positions_desc.count,
                    texcoord_count: texcoords_desc.map_or(0, |desc| desc.count),
                    display_offset,
                    display_size,
                    position_fraction_bits: fraction_bits(positions_desc.format),
                    texcoord_fraction_bits: texcoords_desc
                        .map_or(0, |desc| fraction_bits(desc.format)),
                    texcoord_signed: texcoords_desc
                        .is_some_and(|desc| matches!(component_type(desc.format), 1 | 3)),
                    texture_tables: texture_tables.clone(),
                    color: color.clone(),
                    normal: normal.clone(),
                    render_state: render_state.clone(),
                    mesh,
                });
            }
        }
    }
    Ok(objects)
}

fn fixed_scale(fraction_bits: u8) -> f32 {
    2.0_f32.powi(-i32::from(fraction_bits))
}

fn c_string(data: &[u8], offset: usize) -> Result<String, GeometryError> {
    crate::read::c_string(data, offset)
        .map(|bytes| bytes.escape_ascii().to_string())
        .map_err(|error| Gpl(format!("label at {offset:#x}: {error}")))
}

fn parse_vertex_desc(
    object: &[u8],
    offset: usize,
    label: &str,
) -> Result<VertexArrayDesc, GeometryError> {
    let data_offset = read_u32(object, offset)
        .ok_or_else(|| Gpl(format!("{label} descriptor has no data pointer")))?
        as usize;
    let packed = read_u32(object, offset + 4)
        .ok_or_else(|| Gpl(format!("{label} descriptor has no format")))?;
    Ok(VertexArrayDesc {
        data_offset,
        count: (packed >> 16) as usize,
        format: ((packed >> 8) & 0xFF) as u8,
        components: (packed & 0xFF) as u8,
    })
}

fn component_type(format: u8) -> u8 {
    format >> 4
}

fn fraction_bits(format: u8) -> u8 {
    format & 0x0F
}

#[derive(Clone, Copy)]
enum ComponentFormat {
    U8,
    I8,
    U16,
    I16,
    F32,
}

impl ComponentFormat {
    fn parse(format: u8) -> Result<Self, GeometryError> {
        Ok(match component_type(format) {
            0 => Self::U8,
            1 => Self::I8,
            2 => Self::U16,
            3 => Self::I16,
            4 => Self::F32,
            _ => return Err(Gpl(format!("unsupported vertex format {format:#04x}"))),
        })
    }

    fn width(self) -> usize {
        match self {
            Self::U8 | Self::I8 => 1,
            Self::U16 | Self::I16 => 2,
            Self::F32 => 4,
        }
    }

    fn read(self, data: &[u8], offset: usize) -> f32 {
        match self {
            Self::U8 => f32::from(data[offset]),
            Self::I8 => f32::from(data[offset] as i8),
            Self::U16 => f32::from(read_u16(data, offset).unwrap()),
            Self::I16 => f32::from(read_u16(data, offset).unwrap() as i16),
            Self::F32 => f32::from_bits(read_u32(data, offset).unwrap()),
        }
    }
}

#[derive(Clone, Copy)]
enum VectorKind {
    Coordinate,
    Normal,
}

#[derive(Clone, Copy)]
struct VectorFormat {
    component: ComponentFormat,
    scale: f32,
}

impl VectorFormat {
    fn new(format: u8, kind: VectorKind) -> Result<Self, GeometryError> {
        Ok(Self::with_fraction(
            ComponentFormat::parse(format)?,
            fraction_bits(format),
            kind,
        ))
    }

    fn with_fraction(component: ComponentFormat, fraction: u8, kind: VectorKind) -> Self {
        let fraction = match (component, kind) {
            (ComponentFormat::F32, _) => 0,
            (_, VectorKind::Coordinate) => fraction,
            (ComponentFormat::U8, VectorKind::Normal) => 7,
            (ComponentFormat::I8, VectorKind::Normal) => 6,
            (ComponentFormat::U16, VectorKind::Normal) => 15,
            (ComponentFormat::I16, VectorKind::Normal) => 14,
        };
        Self {
            component,
            scale: fixed_scale(fraction),
        }
    }

    fn read<const N: usize>(&self, data: &[u8], at: usize) -> Result<[f32; N], GeometryError> {
        let vector = std::array::from_fn(|axis| {
            self.component
                .read(data, at + axis * self.component.width())
                * self.scale
        });
        if vector.iter().any(|v| !v.is_finite()) {
            return Err(Gpl("non-finite vertex component".into()));
        }
        Ok(vector)
    }
}

fn validate_array(
    object: &[u8],
    desc: VertexArrayDesc,
    stride: usize,
    element_size: usize,
) -> Result<(), GeometryError> {
    // The last vector needs its components, not trailing stride padding. Short
    // strides can overlap vectors; check the bytes actually read in that case.
    let size = if desc.count == 0 {
        0
    } else {
        (desc.count - 1)
            .checked_mul(stride)
            .and_then(|size| size.checked_add(element_size))
            .ok_or_else(|| Gpl("vertex array overflows".into()))?
    };
    if desc
        .data_offset
        .checked_add(size)
        .is_none_or(|end| end > object.len())
    {
        return Err(Gpl("vertex array exceeds object".into()));
    }
    Ok(())
}

fn decode_vectors<const N: usize>(
    object: &[u8],
    desc: VertexArrayDesc,
    kind: VectorKind,
) -> Result<Vec<[f32; N]>, GeometryError> {
    let format = VectorFormat::new(desc.format, kind)?;
    if matches!(kind, VectorKind::Normal) && !matches!(desc.components, 3 | 6) {
        return Err(Gpl(format!(
            "unsupported normal vector stride {}",
            desc.components
        )));
    }
    let width = format.component.width();
    // The loader always binds XYZ/ST; descriptor components specify stride,
    // which the array register stores in one byte.
    let stride = (usize::from(desc.components) * width) & 0xff;
    validate_array(object, desc, stride, N * width)?;
    (0..desc.count)
        .map(|index| format.read(object, desc.data_offset + index * stride))
        .collect()
}

fn decode_colors(object: &[u8], desc: VertexArrayDesc) -> Result<Vec<[f32; 4]>, GeometryError> {
    let format = ColorFormat::parse(desc.format)?;
    if desc.components == 0 {
        return Err(Gpl("empty color layout".into()));
    }
    let width = format.width();
    validate_array(object, desc, width, width)?;
    Ok((0..desc.count)
        .map(|index| format.read(object, desc.data_offset + index * width, desc.count == 1))
        .collect())
}

#[derive(Clone, Copy)]
enum ColorFormat {
    Rgb565,
    Rgb8,
    Rgbx8,
    Rgba4,
    Rgba6,
    Rgba8,
}

impl ColorFormat {
    fn parse(format: u8) -> Result<Self, GeometryError> {
        Ok(match component_type(format) {
            0 => Self::Rgb565,
            1 => Self::Rgb8,
            2 => Self::Rgbx8,
            3 => Self::Rgba4,
            4 => Self::Rgba6,
            5 => Self::Rgba8,
            _ => return Err(Gpl(format!("unsupported color format {format:#04x}"))),
        })
    }

    fn width(self) -> usize {
        match self {
            Self::Rgb565 | Self::Rgba4 => 2,
            Self::Rgb8 | Self::Rgba6 => 3,
            Self::Rgbx8 | Self::Rgba8 => 4,
        }
    }

    fn read(self, data: &[u8], at: usize, constant: bool) -> [f32; 4] {
        use crate::tpl::{expand4, expand5, expand6};
        let mut color = match self {
            Self::Rgb565 => {
                let v = read_u16(data, at).unwrap();
                [
                    expand5(v >> 11),
                    expand6((v >> 5) & 63),
                    expand5(v & 31),
                    255,
                ]
            }
            Self::Rgba4 => {
                let v = read_u16(data, at).unwrap();
                [12, 8, 4, 0].map(|shift| expand4((v >> shift) & 15))
            }
            Self::Rgb8 | Self::Rgbx8 => [data[at], data[at + 1], data[at + 2], 255],
            Self::Rgba8 => data[at..at + 4].try_into().unwrap(),
            Self::Rgba6 if constant => {
                // The material-constant loader reads only a 16-bit word here.
                let v = u32::from(read_u16(data, at).unwrap());
                [v >> 16, v >> 10, v >> 4, v << 2].map(|c| (c & 252) as u8)
            }
            Self::Rgba6 => {
                let v = u32::from_be_bytes([0, data[at], data[at + 1], data[at + 2]]);
                [18, 12, 6, 0].map(|shift| expand6(((v >> shift) & 63) as u16))
            }
        };
        if constant {
            // Material constants left-align packed channels; vertex colors expand them.
            let masks = match self {
                Self::Rgb565 => [248, 252, 248, 255],
                Self::Rgba4 => [240; 4],
                _ => [255; 4],
            };
            color = std::array::from_fn(|i| color[i] & masks[i]);
        }
        color.map(|value| f32::from(value) / 255.0)
    }
}

enum RenderCommand {
    Unchanged,
    Texture(u32),
    VertexFormat(u32),
    Material(u32),
    Matrix(u32),
}

impl RenderCommand {
    fn decode(kind: u8, value: u32, modern: bool) -> Result<Self, GeometryError> {
        // The GPL revision selects vertex/material/matrix numbering.
        Ok(match (modern, kind) {
            (_, 1) => Self::Texture(value),
            (true, 2) | (false, 3) => Self::VertexFormat(value),
            (true, 3) => Self::Material(value),
            (false, 4) => Self::Material(match value {
                // Modulate, decal, replace, vertex color.
                0 | 1 | 3 | 4 => value + 1,
                _ => return Err(Gpl(format!("unsupported old material {value}"))),
            }),
            (true, 4) | (false, 5) => Self::Matrix(value),
            // Unhandled state opcodes still submit their optional draw.
            _ => Self::Unchanged,
        })
    }
}

/// Every counted record may issue a draw, including commands that leave state
/// unchanged. Skinned objects issue draws after loading their joint palettes.
fn parse_render_commands(
    object: &[u8],
    start: usize,
    count: usize,
    modern: bool,
) -> Result<Vec<(RenderStateInfo, usize, usize)>, GeometryError> {
    let commands = object
        .get(start..)
        .and_then(|bytes| bytes.get(..count.checked_mul(16)?))
        .ok_or_else(|| Gpl("invalid render command table".into()))?;
    let mut state = RenderStateInfo::default();
    let mut draws = Vec::new();
    for command in commands.chunks_exact(16) {
        match RenderCommand::decode(command[0], read_u32(command, 4).unwrap(), modern)? {
            RenderCommand::Unchanged => {}
            RenderCommand::Texture(value) => {
                let stage = (value >> 13) & 7;
                state.texture_commands.retain(|v| ((v >> 13) & 7) != stage);
                state.texture_commands.push(value);
                state.texture_commands.sort_by_key(|v| (v >> 13) & 7);
            }
            RenderCommand::VertexFormat(value) => {
                state.vcd = Some(value);
                state.vertex_revision += 1;
            }
            RenderCommand::Material(value) => state.tev_modes = vec![value],
            RenderCommand::Matrix(value) => {
                let slot = value & 0xffff;
                state.matrix_commands.retain(|v| (v & 0xffff) != slot);
                state.matrix_commands.push(value);
            }
        }
        let offset = read_u32(command, 8).unwrap() as usize;
        let size = read_u32(command, 12).unwrap() as usize;
        if offset == 0 || size == 0 {
            continue;
        }
        if offset
            .checked_add(size)
            .is_none_or(|end| end > object.len())
        {
            return Err(Gpl("draw range exceeds object".into()));
        }
        draws.push((state.clone(), offset, size));
    }
    // Empty and state-only tables contribute no meshes; model nodes are independent.
    Ok(draws)
}

#[cfg(test)]
fn decode_display_list(
    data: &[u8],
    arrays: &VertexArrays<'_>,
    state: &RenderStateInfo,
) -> Result<GeometryMesh, GeometryError> {
    let mut meshes =
        decode_display_list_with_formats(data, arrays, state, &mut vertex::Formats::new(arrays))?;
    assert_eq!(meshes.len(), 1, "fixture expected a single attribute set");
    Ok(meshes.pop().unwrap())
}

fn decode_display_list_with_formats(
    data: &[u8],
    arrays: &VertexArrays<'_>,
    state: &RenderStateInfo,
    formats: &mut vertex::Formats,
) -> Result<Vec<GeometryMesh>, GeometryError> {
    use vertex::Format;
    let colors = arrays.colors.map(|array| array.values);
    let specs = if let Some(vcd) = state.vcd {
        vertex_specs(vcd)?
    } else {
        // Older synthetic GPLs and actor projections do not expose a VCD. Keep
        // a conservative compatibility path for those resources only.
        let mut specs = vec![VertexAttributeSpec {
            attr: 9,
            kind: if arrays.positions.values.len() > 255 {
                3
            } else {
                2
            },
        }];
        // A single color is a material constant; larger palettes use an index
        // between the position and UV indices.
        if let Some(colors) = colors.filter(|values| values.len() > 1) {
            specs.push(VertexAttributeSpec {
                attr: 11,
                kind: if colors.len() > 255 { 3 } else { 2 },
            });
        }
        if let Some(texcoords) = arrays
            .texcoords
            .first()
            .filter(|array| !array.values.is_empty())
        {
            specs.push(VertexAttributeSpec {
                attr: 13,
                kind: if texcoords.values.len() > 255 { 3 } else { 2 },
            });
        }
        specs
    };
    formats.bind(state, specs);
    let mut schema = None;
    let mut meshes = Vec::new();
    let mut mesh = GeometryMesh::default();
    let mut cursor = 0;
    while cursor < data.len() {
        let opcode = data[cursor];
        if opcode == 0 {
            cursor += 1;
            continue;
        }
        if (0x80..=0xBF).contains(&opcode) {
            if cursor + 3 > data.len() {
                return Err(Gpl("truncated GX primitive header".into()));
            }
            let primitive = (opcode & 0x78) >> 3;
            let count = usize::from(u16::from_be_bytes([data[cursor + 1], data[cursor + 2]]));
            if count == 0 {
                cursor += 3;
                continue;
            }
            let inputs = formats.inputs(opcode & 7)?;
            let shape: Vec<_> = inputs
                .iter()
                .map(|input| {
                    (
                        input.attr,
                        matches!(input.format, Format::Normal { basis: true, .. }),
                    )
                })
                .collect();
            if schema.as_ref() != Some(&shape) {
                if schema.is_some() {
                    meshes.push(std::mem::take(&mut mesh));
                }
                if colors.is_some_and(|colors| colors.len() == 1) {
                    mesh.colors = Some(Vec::new());
                }
                for input in &inputs {
                    match input.attr {
                        0 => mesh.joints = Some(Vec::new()),
                        10 => {
                            mesh.normals = Some(Vec::new());
                            if matches!(input.format, Format::Normal { basis: true, .. }) {
                                mesh.normal_basis = Some([Vec::new(), Vec::new()]);
                            }
                        }
                        11 => {
                            mesh.colors = Some(Vec::new());
                            if colors.is_some_and(|colors| colors.len() == 1) {
                                mesh.unused_colors = Some(Vec::new());
                            }
                        }
                        12 => mesh.secondary_colors = Some(Vec::new()),
                        14..=20 => mesh.extra_texcoords.push((input.attr - 13, Vec::new())),
                        _ => {}
                    }
                }
                schema = Some(shape);
            }
            let stride: usize = inputs.iter().map(|input| input.width()).sum();
            let payload = cursor + 3;
            let end = payload
                .checked_add(count * stride)
                .ok_or_else(|| Gpl("GX primitive overflows display list".into()))?;
            if end > data.len() {
                return Err(Gpl("GX primitive exceeds display list".into()));
            }
            let first_vertex = mesh.positions.len();
            for vertex in 0..count {
                let at = payload + vertex * stride;
                let mut offset = at;
                let mut source = [None; 2];
                let mut has_color = false;
                let mut has_uv = false;
                for input in &inputs {
                    match input.attr {
                        0 => {
                            let slot = usize::from(data[offset]) / 3;
                            let joint = state
                                .matrix_commands
                                .iter()
                                .find(|v| (**v & 0xffff) as usize == slot)
                                .ok_or_else(|| Gpl(format!("unbound joint matrix slot {slot}")))?;
                            mesh.joints.as_mut().unwrap().push((joint >> 16) as u16);
                        }
                        9 => {
                            let (index, value) = input.coordinate(
                                data,
                                offset,
                                Some(arrays.positions),
                                arrays.source,
                            )?;
                            source[0] = index;
                            mesh.positions.push(value);
                        }
                        10 => {
                            let Format::Normal {
                                format,
                                basis,
                                index3,
                            } = input.format
                            else {
                                unreachable!()
                            };
                            for component in 0..if basis { 3 } else { 1 } {
                                let vector_offset = component * 3 * format.component.width();
                                let (index, value) = if input.kind == 1 {
                                    (None, format.read(data, offset + vector_offset)?)
                                } else {
                                    let width = index_width(input.kind);
                                    let index = read_index(
                                        data,
                                        offset + if index3 { component * width } else { 0 },
                                        width,
                                    );
                                    let (desc, values) = arrays
                                        .normals
                                        .ok_or_else(|| Gpl("unbound normal array".into()))?;
                                    if index >= desc.count && arrays.source.is_some() {
                                        return Err(
                                            Gpl("normal index exceeds source array".into()),
                                        );
                                    }
                                    let value = if let Some(source) = arrays.source {
                                        let stride = (usize::from(if desc.components == 2 {
                                            3
                                        } else {
                                            desc.components
                                        }) * ComponentFormat::parse(desc.format)?
                                            .width())
                                            & 255;
                                        let at = desc.data_offset + index * stride + vector_offset;
                                        vertex::bounded(source, at, format.component.width() * 3)?;
                                        format.read(source, at)?
                                    } else {
                                        values.vector(index, component)?
                                    };
                                    (Some(index), value)
                                };
                                if component == 0 {
                                    mesh.normals.as_mut().unwrap().push(value);
                                    mesh.normal_map.push(index);
                                } else {
                                    mesh.normal_basis.as_mut().unwrap()[component - 1].push(value);
                                }
                            }
                        }
                        11 | 12 => {
                            let Format::Color(format) = input.format else {
                                unreachable!()
                            };
                            let (index, value) = if input.kind == 1 {
                                (None, format.read(data, offset, false))
                            } else {
                                if input.attr == 12 {
                                    return Err(Gpl("unbound COLOR1 array".into()));
                                }
                                let array = arrays
                                    .colors
                                    .ok_or_else(|| Gpl("unbound color array".into()))?;
                                if array.desc.count == 1 {
                                    return Err(Gpl(
                                        "material constant does not bind a vertex color array"
                                            .into(),
                                    ));
                                }
                                let index = read_index(data, offset, index_width(input.kind));
                                let value = if let Some(source) = arrays.source {
                                    if index >= array.desc.count {
                                        return Err(Gpl("color index exceeds source array".into()));
                                    }
                                    let at = array.desc.data_offset
                                        + index * ColorFormat::parse(array.desc.format)?.width();
                                    vertex::bounded(source, at, format.width())?;
                                    format.read(source, at, false)
                                } else {
                                    indexed_vertex(colors, index, "color")?
                                };
                                (Some(index), value)
                            };
                            if input.attr == 12 {
                                mesh.secondary_colors.as_mut().unwrap().push(value);
                            } else {
                                if let Some(unused) = &mut mesh.unused_colors {
                                    unused.push(value);
                                    mesh.colors.as_mut().unwrap().push(colors.unwrap()[0]);
                                } else {
                                    mesh.colors.as_mut().unwrap().push(value);
                                }
                                mesh.color_map.push(index);
                                has_color = true;
                            }
                        }
                        13..=20 => {
                            let slot = input.attr - 13;
                            let array = arrays.texcoords.get(usize::from(slot)).copied();
                            let (index, value) =
                                input.coordinate(data, offset, array, arrays.source)?;
                            if slot == 0 {
                                mesh.texcoords.push(value);
                                source[1] = index;
                                has_uv = true;
                            } else {
                                mesh.extra_texcoords
                                    .iter_mut()
                                    .find(|(i, _)| *i == slot)
                                    .unwrap()
                                    .1
                                    .push(value);
                            }
                        }
                        _ => unreachable!(),
                    }
                    offset += input.width();
                }
                if !has_color
                    && let Some(values) = colors
                    && values.len() == 1
                {
                    mesh.colors.as_mut().unwrap().push(values[0]);
                    mesh.color_map.push(Some(0));
                }
                if !has_uv {
                    mesh.texcoords.push(
                        arrays
                            .texcoords
                            .first()
                            .and_then(|array| array.values.first())
                            .copied()
                            .unwrap_or([0.; 2]),
                    );
                }
                mesh.vertex_map.push(source);
            }
            let first_index = mesh.indices.len();
            let mut triangle = |vertices| tri_indices(&mut mesh.indices, first_vertex, vertices);
            match primitive {
                0 | 1 => {
                    for group in 0..count / 4 {
                        let base = group * 4;
                        triangle([base, base + 1, base + 2]);
                        triangle([base, base + 2, base + 3]);
                    }
                }
                2 => {
                    for group in 0..count / 3 {
                        let base = group * 3;
                        triangle([base, base + 1, base + 2]);
                    }
                }
                3 => {
                    for group in 0..count.saturating_sub(2) {
                        triangle(if group % 2 == 0 {
                            [group, group + 1, group + 2]
                        } else {
                            [group + 1, group, group + 2]
                        });
                    }
                }
                4 => {
                    for group in 1..count.saturating_sub(1) {
                        triangle([0, group, group + 1]);
                    }
                }
                5 => mesh
                    .indices
                    .extend((0..count / 2 * 2).map(|index| (first_vertex + index) as u32)),
                6 => {
                    for index in 0..count.saturating_sub(1) {
                        mesh.indices.extend(
                            [first_vertex + index, first_vertex + index + 1]
                                .map(|index| index as u32),
                        );
                    }
                }
                7 => mesh
                    .indices
                    .extend((0..count).map(|index| (first_vertex + index) as u32)),
                _ => unreachable!(),
            }
            if mesh.indices.len() != first_index {
                let topology = match primitive {
                    5 | 6 => Topology::Lines,
                    7 => Topology::Points,
                    _ => Topology::Triangles,
                };
                if let Some((previous, range)) = mesh.primitives.last_mut()
                    && *previous == topology
                {
                    range.end = mesh.indices.len();
                } else {
                    mesh.primitives
                        .push((topology, first_index..mesh.indices.len()));
                }
            }
            cursor = end;
        } else if opcode == 8 {
            cursor += gx_command_size(opcode, &data[cursor..])?;
            formats.write(data[cursor - 5], read_u32(data, cursor - 4).unwrap())?;
        } else {
            cursor += gx_command_size(opcode, &data[cursor..])?;
        }
    }
    meshes.push(mesh);
    Ok(meshes)
}

fn indexed_vertex<T: Copy>(
    values: Option<&[T]>,
    index: usize,
    label: &str,
) -> Result<T, GeometryError> {
    values
        .ok_or_else(|| Gpl(format!("GX layout references missing {label} array")))?
        .get(index)
        .copied()
        .ok_or_else(|| Gpl(format!("GX {label} index exceeds source array")))
}

fn vertex_specs(vcd: u32) -> Result<Vec<VertexAttributeSpec>, GeometryError> {
    let mut specs = Vec::new();
    let mut add = |attr: u8, kind: u8| {
        if kind != 0 {
            specs.push(VertexAttributeSpec { attr, kind });
        }
    };
    add(0, (vcd & 1) as u8);
    add(9, ((vcd >> 2) & 3) as u8);
    // NBT aliases the normal input. Although specified last in the command,
    // its indices precede colors and UVs in the vertex stream.
    let nbt = ((vcd >> 26) & 3) as u8;
    add(
        10,
        if nbt != 0 {
            nbt
        } else {
            ((vcd >> 4) & 3) as u8
        },
    );
    for (attr, shift) in (11_u8..=20).zip((6_u32..=24).step_by(2)) {
        add(attr, ((vcd >> shift) & 3) as u8);
    }
    if !specs.iter().any(|spec| spec.attr == 9) {
        return Err(Gpl(format!("GX VCD 0x{vcd:08X} has no position attribute")));
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
fn tri_indices(indices: &mut Vec<u32>, base: usize, [a, b, c]: [usize; 3]) {
    // Retail GPL display lists describe outward-facing polygons clockwise in
    // the model coordinate system. glTF defines counter-clockwise triangles
    // as front-facing, so preserve the vertex/UV pairing but reverse the
    // winding at the interchange boundary.
    indices.extend([a, c, b].map(|index| (base + index) as u32));
}

fn gx_command_size(opcode: u8, data: &[u8]) -> Result<usize, GeometryError> {
    let size = match opcode {
        0x08 => 6,
        0x10 => {
            if data.len() < 5 {
                return Err(Gpl("truncated GX XF command".into()));
            }
            // XF commands carry a BE command word after the opcode.  The
            // low nibble of its high half-word is the number of extra 32-bit
            // register words minus one (GX FIFO/XF semantics).
            let command = u32::from_be_bytes(data[1..5].try_into().unwrap());
            5 + (((command >> 16) as usize & 0xF) + 1) * 4
        }
        0x20 | 0x28 | 0x30 | 0x38 | 0x44 | 0x61 => 5,
        0x40 => 9,
        0x48 => 1,
        _ => {
            return Err(Gpl(format!("unsupported GX command 0x{opcode:02X}")));
        }
    };
    if data.len() < size {
        return Err(Gpl("GX command exceeds display list".into()));
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
    let mut attributes = json!({"POSITION":accessor_base,"TEXCOORD_0":accessor_base+1});
    for (slot, uvs) in &mesh.extra_texcoords {
        attributes[format!("TEXCOORD_{slot}")] =
            json!(float_attribute(buffer, views, accessors, uvs));
    }
    if let Some(accessor) = color_accessor {
        attributes["COLOR_0"] = json!(accessor);
    }
    for (name, colors) in [
        ("COLOR_1", &mesh.secondary_colors),
        ("_UNUSED_COLOR_0", &mesh.unused_colors),
    ] {
        if let Some(colors) = colors {
            attributes[name] = json!(float_attribute(buffer, views, accessors, colors));
        }
    }
    if let Some(accessor) = normal_accessor {
        attributes["NORMAL"] = json!(accessor);
    }
    if let Some(basis) = &mesh.normal_basis {
        // Preserve both authored vectors exactly. glTF's tangent+handedness
        // representation cannot encode an arbitrary non-orthogonal basis.
        for (i, values) in basis.iter().enumerate() {
            attributes[format!("_NORMAL_BASIS_{}", i + 1)] =
                json!(float_attribute(buffer, views, accessors, values));
        }
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
    let empty = [(Topology::Triangles, 0..0)];
    let primitives = if mesh.primitives.is_empty() {
        empty.as_slice()
    } else {
        &mesh.primitives
    };
    let primitives = primitives.iter().map(|(topology, range)| {
        let indices = accessors.len();
        accessors.push(json!({"bufferView":view_base+2,"byteOffset":range.start*4,"componentType":UNSIGNED_INT,"count":range.len(),"type":"SCALAR"}));
        json!({"mode":*topology as u8,"attributes":attributes,"indices":indices,"material":material})
    }).collect::<Vec<_>>();
    (
        accessor_base,
        accessor_base + 1,
        color_accessor,
        normal_accessor,
        json!({"name":"GPL object","primitives":primitives}),
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

pub(crate) fn write_png(
    path: &Path,
    width: u16,
    height: u16,
    rgba: &[u8],
) -> Result<(), GeometryError> {
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
            return Err(Gpl("invalid skeleton hierarchy".into()));
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

pub(crate) fn model_parents(model: &Model) -> Vec<Option<usize>> {
    model.nodes.iter().map(|node| node.parent).collect()
}

pub(crate) fn model_node_info(blob: &Model) -> Vec<ModelNodeInfo> {
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
                name: blob
                    .names
                    .as_ref()
                    .and_then(|names| names.get(index))
                    .filter(|name| !name.is_empty())
                    .cloned()
                    .unwrap_or_else(|| format!("gpl_node_{index:03}")),
                translation,
                rotation,
                scale,
            }
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

    fn array<const N: usize>(format: u8, values: &[[f32; N]]) -> VertexArray<'_, N> {
        VertexArray {
            desc: VertexArrayDesc {
                data_offset: 0,
                count: values.len(),
                format,
                components: N as u8,
            },
            values,
        }
    }

    fn decode_indexed(
        data: &[u8],
        positions: &[[f32; 3]],
        texcoords: &[&[[f32; 2]]],
        colors: Option<&[[f32; 4]]>,
        normals: Option<&NormalArray>,
        state: &RenderStateInfo,
    ) -> Result<GeometryMesh, GeometryError> {
        decode_display_list(
            data,
            &VertexArrays {
                source: None,
                positions: array(0x40, positions),
                texcoords: texcoords.iter().map(|values| array(0x40, values)).collect(),
                colors: colors.map(|values| array(0x50, values)),
                normals: normals.map(|values| {
                    (
                        VertexArrayDesc {
                            data_offset: 0,
                            count: 0,
                            format: 0x40,
                            components: if matches!(values, NormalArray::Nbt3(_)) {
                                2
                            } else {
                                3
                            },
                        },
                        values,
                    )
                }),
            },
            state,
        )
    }

    #[test]
    fn material_recipes_follow_stage_limits_and_keep_supported_operations_distinct() {
        use MaterialCombination::*;
        use MaterialOperation::*;
        for (mode, count, expected) in [
            (1, 1, VertexColorTexture),
            (1, 2, VertexColorTexture),
            (0x11, 1, VertexColorTexture),
            (0x11, 2, VertexColorMultiplyTextures),
            (0x14, 1, Texture),
            (0x14, 2, MultiplyTextures),
            (5, 0, VertexColor),
            (5, 1, VertexColor),
            (0xffff_fff5, 8, VertexColor),
            (0x51, 2, VertexColorTexture),
            (0xffff_f001, 8, VertexColorTexture),
        ] {
            let recipe = MaterialRecipe::parse(&[mode], count).unwrap();
            assert_eq!(recipe.source_mode, mode);
            assert_eq!(usize::from(recipe.source_texture_count), count);
            assert_eq!(recipe.combination().unwrap(), expected);
        }
        for (mode, count, operations, textures) in [
            (2, 1, vec![DecalTexture { texture: 0 }], vec![0]),
            (3, 1, vec![BlendTexture { texture: 0 }], vec![0]),
            (6, 1, vec![AddTexture { texture: 0 }], vec![0]),
            (7, 1, vec![SubtractTexture { texture: 0 }], vec![0]),
            (8, 1, vec![DefaultViewTexture { texture: 0 }], vec![0]),
            (
                0x61,
                2,
                vec![MultiplyTexture { texture: 0 }, AddTexture { texture: 1 }],
                vec![0, 1],
            ),
            (
                0x81,
                2,
                vec![
                    MultiplyTexture { texture: 0 },
                    DefaultViewTexture { texture: 1 },
                ],
                vec![0, 1],
            ),
            (
                15,
                1,
                vec![DecalTexture { texture: 0 }, MultiplyVertexColor],
                vec![0],
            ),
            (
                0x1f,
                2,
                vec![DecalTexture { texture: 0 }, MultiplyVertexColor],
                vec![0],
            ),
            (
                0x1f,
                3,
                vec![
                    DecalTexture { texture: 0 },
                    MultiplyVertexColor,
                    MultiplyTexture { texture: 2 },
                ],
                vec![0, 2],
            ),
            (
                0x41,
                2,
                vec![
                    MultiplyTexture { texture: 0 },
                    ReplaceTexture { texture: 1 },
                ],
                vec![0, 1],
            ),
        ] {
            let recipe = MaterialRecipe::parse(&[mode], count).unwrap();
            assert_eq!(recipe.operations, operations);
            assert_eq!(recipe.textures().collect::<Vec<_>>(), textures);
            assert!(
                recipe
                    .combination()
                    .unwrap_err()
                    .to_string()
                    .contains("scene shader")
            );
        }
        for callback in 9..=14 {
            for mode in [callback, callback | 1 << 4] {
                let recipe = MaterialRecipe::parse(&[mode], 0).unwrap();
                assert_eq!(recipe.stage_count, 0);
                assert!(recipe.operations.is_empty());
            }
            for mode in [callback | 1 << 4, callback << 4 | 1] {
                let recipe = MaterialRecipe::parse(&[mode], 2).unwrap();
                assert_eq!(recipe.source_mode, mode);
                assert_eq!(recipe.stage_count, 1);
                assert_eq!(recipe.operations, [MultiplyTexture { texture: 0 }]);
                assert_eq!(recipe.combination().unwrap(), VertexColorTexture);
            }
        }
        for mode in [0, 0x10] {
            let recipe = MaterialRecipe::parse(&[mode], 1).unwrap();
            assert_eq!(recipe.stage_count, 0);
            assert!(recipe.operations.is_empty());
            assert!(
                recipe
                    .combination()
                    .unwrap_err()
                    .to_string()
                    .contains("zero-stage")
            );
        }
        for modes in [vec![], vec![1, 5]] {
            assert!(MaterialRecipe::parse(&modes, 1).is_err());
        }
        let unbound = MaterialRecipe::parse(&[1], 0).unwrap();
        assert_eq!(unbound.stage_count, 1);
        assert_eq!(unbound.textures().collect::<Vec<_>>(), [0]);
        assert_eq!(MaterialRecipe::parse(&[0x51], 2).unwrap().stage_count, 2);
        assert!(MaterialRecipe::parse(&[1], 9).is_err());
        let value = serde_json::to_value(MaterialRecipe::parse(&[0x1f], 3).unwrap()).unwrap();
        assert_eq!(value["context"], "default_gpl_ordinary");
        assert_eq!(value["source_mode"], 0x1f);
        assert_eq!(value["source_texture_count"], 3);
        assert_eq!(value["stage_count"], 3);
        assert_eq!(
            value["operations"],
            json!([
                {"kind":"decal_texture","texture":0},
                {"kind":"multiply_vertex_color"},
                {"kind":"multiply_texture","texture":2}
            ])
        );
        let recipe = MaterialRecipe::parse(&[0x81], 2).unwrap();
        assert_eq!(
            serde_json::from_value::<MaterialRecipe>(serde_json::to_value(&recipe).unwrap())
                .unwrap(),
            recipe,
        );
    }

    #[test]
    fn geometry_labels_are_bounded_byte_strings_and_header_roots_are_not_skipped() {
        let mut bytes = vec![0; 96];
        for (at, value) in [
            (0, 0x005bbc61_u32),
            (12, 1),
            (16, 20),
            (20, 32),
            (24, 99),
            (32, 24),
            (48, 32),
            (60, 0x3803),
            (68, 48),
            (80, 2 << 24),
            (84, 0x2888),
            (88, 64),
            (92, 3),
        ] {
            bytes[at..at + 4].copy_from_slice(&value.to_be_bytes());
        }
        bytes[72..74].copy_from_slice(&1_u16.to_be_bytes());
        bytes.extend_from_slice(b"\x90\0\0\xb5\\\0");
        assert_eq!(parse_geometry(&bytes).unwrap()[0].name, r"\xb5\\");
        bytes[88..92].fill(0);
        assert!(parse_geometry(&bytes).unwrap().is_empty());
        bytes[88..92].copy_from_slice(&64_u32.to_be_bytes());
        // An unhandled state opcode cannot hide an unsupported draw payload.
        bytes[80] = 0xff;
        bytes[96] = 0x7f;
        assert!(
            parse_geometry(&bytes)
                .unwrap_err()
                .to_string()
                .contains("unsupported GX command")
        );
        bytes[80] = 2;
        bytes[96] = 0x90;
        // A raw zero becomes the GPL base in the native pointer fixup.
        bytes[24..28].fill(0);
        assert_eq!(parse_geometry(&bytes).unwrap()[0].name, "");
        bytes[24..28].copy_from_slice(&99_u32.to_be_bytes());
        assert!(parse_geometry(&bytes[..101]).is_err());
        bytes[24..28].copy_from_slice(&103_u32.to_be_bytes());
        assert!(parse_geometry(&bytes).is_err());
        bytes[24..28].copy_from_slice(&99_u32.to_be_bytes());
        bytes[4..8].copy_from_slice(&1_u32.to_be_bytes());
        bytes[8..12].copy_from_slice(&99_u32.to_be_bytes());
        assert!(
            parse_geometry(&bytes)
                .unwrap_err()
                .to_string()
                .contains("unresolved GPL header")
        );
    }

    #[test]
    fn actor_preview_validates_directory_and_bounds_its_primary_model() {
        let mut bytes = [0; 256];
        for (at, value) in [
            (0, 31_u32),
            (4, 128),
            (8, 224),
            (128, 64),
            (132, 96),
            (160, 0x005bbc61),
        ] {
            bytes[at..at + 4].copy_from_slice(&value.to_be_bytes());
        }
        let source = section_source(&bytes).unwrap();
        assert_eq!(model_resource(&bytes).unwrap().0, 128);
        assert_eq!(source.gpl.len(), 32);
        assert_eq!(source.tpl.len(), 32);

        // A texture extent must not reach into the next section.
        bytes[132..136].copy_from_slice(&112_u32.to_be_bytes());
        assert!(section_source(&bytes).is_err());
        bytes[132..136].copy_from_slice(&96_u32.to_be_bytes());
        // A valid first model cannot hide a corrupt sibling directory entry.
        bytes[8..12].copy_from_slice(&512_u32.to_be_bytes());
        assert!(section_source(&bytes).is_err());
    }

    #[test]
    fn skeleton_extent_ignores_magic_inside_transform_data_and_later_resources() {
        let mut bytes = vec![0; 192];
        for (at, value) in [
            (4, 32u32),
            (8, 128),
            (32, 0x007b7960),
            (44, 32),
            (64, 60),
            (96, 0x007b7960),
            (160, 0x007b7960),
        ] {
            bytes[at..at + 4].copy_from_slice(&value.to_be_bytes());
        }
        bytes[38..40].copy_from_slice(&1u16.to_be_bytes());
        let (model, start, size) = trailing_model(&bytes).unwrap().unwrap();
        assert_eq!((start, size, model.nodes.len()), (32, 128, 1));
        assert_eq!(skeleton_range(&bytes).unwrap(), 32..160);
        assert_eq!(model.nodes[0].data_words[1], 0x007b7960);
        bytes[8..12].copy_from_slice(&192u32.to_be_bytes());
        assert!(trailing_model(&bytes).is_err());
        bytes[8..12].fill(0);
        assert!(trailing_model(&bytes).unwrap().is_none());
    }

    #[test]
    fn model_labels_preserve_byte_names_and_every_joint_and_object_index() {
        let mut bytes = vec![0u8; 32 + 3 * 28];
        bytes[..4].copy_from_slice(&0x007b7960u32.to_be_bytes());
        bytes[6..8].copy_from_slice(&3u16.to_be_bytes());
        bytes[12..16].copy_from_slice(&32u32.to_be_bytes());
        bytes[24..28].copy_from_slice(&1u32.to_be_bytes());
        bytes[28..32].copy_from_slice(&116u32.to_be_bytes());
        bytes[40..44].copy_from_slice(&60u32.to_be_bytes());
        bytes[68..72].copy_from_slice(&88u32.to_be_bytes());
        for (index, object) in [5u16, 2, 7].into_iter().enumerate() {
            bytes[52 + index * 28..54 + index * 28].copy_from_slice(&object.to_be_bytes());
        }
        for (names, expected) in [
            (
                b"\xb5\0dm05_tail\r\n\0\\xb5\0".as_slice(),
                [r"\xb5", r"dm05_tail\r\n", r"\\xb5"],
            ),
            (b"root\0\0at00\0", ["root", "gpl_node_001", "at00"]),
        ] {
            let mut named = bytes.clone();
            named.extend_from_slice(names);
            let model = Model::parse(&named).unwrap();
            let nodes = model_node_info(&model);
            assert_eq!(
                nodes.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(),
                expected
            );
            assert_eq!(
                nodes
                    .iter()
                    .map(|n| (n.index, n.object_index))
                    .collect::<Vec<_>>(),
                [(0, 5), (1, 2), (2, 7)]
            );
            assert_eq!(model_parents(&model), [None; 3]);
        }
        bytes[28..32].fill(0);
        let unnamed = Model::parse(&bytes).unwrap();
        assert_eq!(
            model_node_info(&unnamed)
                .iter()
                .map(|n| n.name.as_str())
                .collect::<Vec<_>>(),
            ["gpl_node_000", "gpl_node_001", "gpl_node_002"]
        );
    }

    fn commands(records: &[[u32; 4]], length: usize) -> Vec<u8> {
        let mut bytes: Vec<_> = records
            .iter()
            .flat_map(|&[kind, value, offset, size]| [kind << 24, value, offset, size])
            .flat_map(u32::to_be_bytes)
            .collect();
        bytes.resize(length, 0);
        bytes
    }

    #[test]
    fn floating_vertex_coordinates_reject_non_finite_values() {
        let desc = VertexArrayDesc {
            data_offset: 0,
            count: 1,
            format: 0x4f,
            components: 3,
        };
        for (values, valid) in [
            ([1., -2.5, 0.125], true),
            ([f32::NAN, 0., 0.], false),
            ([0., f32::INFINITY, 0.], false),
        ] {
            let bytes: Vec<_> = values.into_iter().flat_map(f32::to_be_bytes).collect();
            let result = decode_vectors::<3>(&bytes, desc, VectorKind::Coordinate);
            if valid {
                assert_eq!(result.unwrap(), [values]);
            } else {
                assert!(result.is_err());
            }
        }
    }

    #[test]
    fn numeric_vertex_arrays_follow_coordinate_and_normal_quantization() {
        for (format, bytes, coordinate, normal) in [
            (0x01, vec![128, 64, 32], [64., 32., 16.], [1., 0.5, 0.25]),
            (
                0x11,
                vec![192, 32, 240],
                [-32., 16., -8.],
                [-1., 0.5, -0.25],
            ),
            (
                0x21,
                [32768_u16, 16384, 8192]
                    .into_iter()
                    .flat_map(u16::to_be_bytes)
                    .collect(),
                [16384., 8192., 4096.],
                [1., 0.5, 0.25],
            ),
            (
                0x31,
                [-16384_i16, 8192, -4096]
                    .into_iter()
                    .flat_map(i16::to_be_bytes)
                    .collect(),
                [-8192., 4096., -2048.],
                [-1., 0.5, -0.25],
            ),
            (
                0x4f,
                [1_f32, -0.5, 0.25]
                    .into_iter()
                    .flat_map(f32::to_be_bytes)
                    .collect(),
                [1., -0.5, 0.25],
                [1., -0.5, 0.25],
            ),
        ] {
            for components in [3, 6] {
                let desc = VertexArrayDesc {
                    data_offset: 0,
                    count: 2,
                    format,
                    components,
                };
                let mut data = bytes.clone();
                data.resize(bytes.len() * usize::from(components) / 3, 0xff);
                data.extend_from_slice(&bytes);
                for (kind, expected) in [
                    (VectorKind::Coordinate, coordinate),
                    (VectorKind::Normal, normal),
                ] {
                    assert_eq!(
                        decode_vectors::<3>(&data, desc, kind).unwrap(),
                        [expected; 2]
                    );
                    assert!(decode_vectors::<3>(&data[..data.len() - 1], desc, kind).is_err());
                }
            }
        }
        let mut desc = VertexArrayDesc {
            data_offset: 0,
            count: 2,
            format: 0,
            components: 2,
        };
        assert_eq!(
            decode_vectors::<3>(&[1, 2, 3, 4, 5], desc, VectorKind::Coordinate).unwrap(),
            [[1., 2., 3.], [3., 4., 5.]]
        );
        assert!(decode_vectors::<3>(&[1, 2, 3, 4], desc, VectorKind::Coordinate).is_err());
        assert!(decode_vectors::<3>(&[1, 2, 3, 4, 5], desc, VectorKind::Normal).is_err());
        desc.format = 0x50;
        assert!(decode_vectors::<3>(&[0; 32], desc, VectorKind::Coordinate).is_err());
        // A 64-float stride becomes zero in the byte-sized array register.
        desc.format = 0x40;
        desc.components = 64;
        let data: Vec<_> = [1_f32, 2., 3.]
            .into_iter()
            .flat_map(f32::to_be_bytes)
            .collect();
        assert_eq!(
            decode_vectors::<3>(&data, desc, VectorKind::Coordinate).unwrap(),
            [[1., 2., 3.]; 2]
        );
    }

    #[test]
    fn packed_colors_distinguish_indexed_expansion_from_material_constants() {
        for (count, format, bytes, expected) in [
            (2, 0, vec![0xff, 0xff], [255u8, 255, 255, 255]),
            (2, 0x10, vec![1, 2, 3], [1, 2, 3, 255]),
            (2, 0x20, vec![1, 2, 3, 99], [1, 2, 3, 255]),
            (2, 0x30, vec![0xf1, 0x28], [255, 17, 34, 136]),
            (2, 0x40, vec![0xfc, 0x1f, 0xca], [255, 4, 255, 40]),
            (2, 0x50, vec![1, 2, 3, 99], [1, 2, 3, 99]),
            (1, 0, vec![0xff, 0xff], [248, 252, 248, 255]),
            (1, 0, vec![0x84, 0x21], [128, 132, 8, 255]),
            (1, 0x30, vec![0xf1, 0x28], [240, 16, 32, 128]),
            (1, 0x40, vec![0xfc, 0x1f, 0xca], [0, 60, 192, 124]),
        ] {
            let desc = VertexArrayDesc {
                data_offset: 0,
                count,
                format,
                components: if format < 0x30 { 3 } else { 4 },
            };
            let bytes = bytes.repeat(count);
            assert!(decode_colors(&bytes[..bytes.len() - 1], desc).is_err());
            assert_eq!(
                decode_colors(&bytes, desc).unwrap(),
                vec![expected.map(|v| f32::from(v) / 255.); count]
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
        let mut model = Model::parse(&bytes).unwrap();
        assert_eq!(
            model_draw_order(&model),
            [(0, None), (2, Some(1)), (1, Some(2))]
        );
        model.nodes[2].draw_priority = 4;
        assert_eq!(
            model_draw_order(&model),
            [(0, None), (1, Some(2)), (2, Some(1))]
        );
        model.nodes[2].object_index = 2;
        assert_eq!(
            model_draw_order(&model),
            [(0, None), (2, Some(2)), (2, Some(1))]
        );
        bytes[68..72].copy_from_slice(&32u32.to_be_bytes());
        assert!(Model::parse(&bytes).is_err());
    }

    #[test]
    fn draws_follow_commands_beyond_the_first_three_records() {
        let mut object = commands(
            &[
                [1, 0x11110002, 0, 0],
                [1, 0x11112000, 0, 0],
                [3, 17, 0, 0],
                [2, 0x2888, 112, 8],
                [4, 0x70002, 120, 12],
                [0, 99, 132, 12],
                [3, 18, 144, 16],
            ],
            160,
        );
        let draws = parse_render_commands(&object, 0, 7, true).unwrap();
        assert_eq!(draws.len(), 4);
        assert_eq!((draws[0].1, draws[0].2), (112, 8));
        assert_eq!(draws[0].0.texture_commands, [0x11110002, 0x11112000]);
        assert_eq!(draws[1].0.matrix_commands, [0x00070002]);
        assert_eq!(draws[2].0, draws[1].0);
        assert_eq!(draws[3].0.tev_modes, [18]);
        for kind in (0..=u8::MAX).filter(|kind| !(1..=4).contains(kind)) {
            object[5 * 16] = kind;
            assert_eq!(parse_render_commands(&object, 0, 7, true).unwrap(), draws);
        }
        assert!(
            parse_render_commands(&object, 0, 3, true)
                .unwrap()
                .is_empty()
        );
        assert!(parse_render_commands(&object[..159], 0, 7, true).is_err());
        assert!(
            parse_render_commands(&object, object.len(), 0, true)
                .unwrap()
                .is_empty()
        );
        assert!(parse_render_commands(&object, object.len() + 1, 0, true).is_err());
        assert!(parse_render_commands(&object, object.len(), 1, true).is_err());
        assert!(parse_render_commands(&object, 0, usize::MAX, true).is_err());
        assert!(
            parse_render_commands(&[0; 16], 0, 1, true)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn old_commands_preserve_layout_material_and_draws_without_state_changes() {
        let mut object = commands(
            &[
                [4, 4, 0, 0],
                [3, 8, 96, 4],
                [2, 99, 100, 4],
                [0, 99, 104, 4],
                [5, 0x70002, 108, 4],
                [0, 0, 0, u32::MAX],
            ],
            112,
        );
        let draws = parse_render_commands(&object, 0, 6, false).unwrap();
        assert_eq!(draws.len(), 4);
        assert_eq!(draws[0].0.vcd, Some(8));
        assert_eq!(draws[0].0.tev_modes, [5]);
        assert!(draws[0].0.matrix_commands.is_empty());
        for (index, (state, offset, size)) in draws.iter().enumerate() {
            assert_eq!((*offset, *size), (96 + index * 4, 4));
            assert_eq!(state.vcd, Some(8));
            assert_eq!(state.tev_modes, [5]);
        }
        assert_eq!(draws[3].0.matrix_commands, [0x70002]);
        for kind in (0..=u8::MAX).filter(|kind| ![1, 3, 4, 5].contains(kind)) {
            object[2 * 16] = kind;
            assert_eq!(parse_render_commands(&object, 0, 6, false).unwrap(), draws);
        }
    }

    #[test]
    fn physical_primitives_preserve_lines_points_and_triangle_order() {
        let positions = [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]];
        let decode = |bytes: &[u8]| {
            decode_indexed(
                bytes,
                &positions,
                &[],
                None,
                None,
                &RenderStateInfo::default(),
            )
        };
        let triangle = [0x90, 0, 3, 0, 1, 2];
        for (opcode, topology, expected) in [
            (0xa8, Topology::Lines, vec![0, 1]),
            (0xb0, Topology::Lines, vec![0, 1, 1, 2]),
            (0xb8, Topology::Points, vec![0, 1, 2]),
        ] {
            let packet = [opcode, 0, 3, 0, 1, 2];
            assert_eq!(decode(&packet).unwrap().indices, expected);
            for bytes in [
                packet.to_vec(),
                [triangle.as_slice(), &packet].concat(),
                [packet.as_slice(), &triangle].concat(),
            ] {
                let mesh = decode(&bytes).unwrap();
                assert!(
                    mesh.primitives
                        .iter()
                        .any(|(kind, range)| *kind == topology && range.len() == expected.len())
                );
                assert_eq!(
                    mesh.primitives
                        .iter()
                        .map(|(_, range)| range.len())
                        .sum::<usize>(),
                    mesh.indices.len()
                );
                let (_, _, _, _, gltf) = append_geometry_mesh(
                    &mut Vec::new(),
                    &mut Vec::new(),
                    &mut Vec::new(),
                    &mesh,
                    0,
                );
                assert!(
                    gltf["primitives"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|primitive| primitive["mode"] == topology as u8)
                );
            }
            // A zero-count packet submits no vertices of any topology.
            let empty = [opcode, 0, 0];
            assert!(decode(&empty).unwrap().indices.is_empty());
            assert_eq!(
                decode(&[triangle.as_slice(), &empty].concat())
                    .unwrap()
                    .indices,
                [0, 2, 1]
            );
        }
        // Incomplete polygon packets likewise submit no complete primitive.
        for opcode in [0x80, 0x88, 0x90, 0x98, 0xa0] {
            assert!(decode(&[opcode, 0, 2, 0, 1]).unwrap().indices.is_empty());
        }
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
        let mesh = decode_indexed(&data, &positions, &[&uvs], None, None, &state).unwrap();
        assert_eq!(mesh.positions, positions);
        assert_eq!(mesh.indices, [0, 2, 1]);
        assert_eq!(mesh.joints.unwrap(), [7, 7, 7]);
        let unbound = RenderStateInfo {
            matrix_commands: vec![],
            ..state
        };
        assert!(decode_indexed(&data, &positions, &[&uvs], None, None, &unbound).is_err());
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
        let secondary = [[5., 6.], [7., 8.], [9., 10.]];
        let mesh =
            decode_indexed(&data, &positions, &[&uvs, &secondary], None, None, &state).unwrap();
        assert_eq!(mesh.texcoords, uvs);
        assert_eq!(
            mesh.extra_texcoords[0].1,
            [secondary[2], secondary[0], secondary[1]]
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
        assert!(
            decode_indexed(
                &invalid,
                &positions,
                &[&uvs, &secondary],
                None,
                None,
                &state
            )
            .is_err()
        );
    }

    #[test]
    fn nbt3_preserves_three_indices_before_colors_and_uvs() {
        let vectors = [[1., 2., 3.], [4., 5., 6.], [7., 8., 9.], [10., 11., 12.]];
        let bytes = vectors
            .iter()
            .flatten()
            .flat_map(|v: &f32| v.to_be_bytes())
            .collect::<Vec<_>>();
        let desc = VertexArrayDesc {
            data_offset: 0,
            count: 4,
            format: 0x4f,
            components: 2,
        };
        let normals = NormalArray::read(&bytes, desc).unwrap();
        assert!(NormalArray::read(&bytes[..bytes.len() - 1], desc).is_err());
        let minimal =
            NormalArray::read(&bytes[..36], VertexArrayDesc { count: 3, ..desc }).unwrap();
        for (component, vector) in vectors[..3].iter().enumerate() {
            assert_eq!(minimal.vector(0, component).unwrap(), *vector);
        }
        let positions = [[30., 40., 50.]];
        let colors = [[1., 0., 0., 1.], [0., 1., 0., 1.]];
        let uvs = [[0.25, 0.75], [0.5, 1.]];
        for kind in [2, 3] {
            let state = RenderStateInfo {
                vcd: Some((kind << 26) | (2 << 10) | (2 << 6) | (2 << 2)),
                ..Default::default()
            };
            let mut data = vec![0xb8, 0, 1, 0];
            for index in [1u8, 0, 1] {
                if kind == 3 {
                    data.push(0);
                }
                data.push(index);
            }
            data.extend([1, 0]);
            let mesh = decode_indexed(
                &data,
                &positions,
                &[&uvs],
                Some(&colors),
                Some(&normals),
                &state,
            )
            .unwrap();
            assert_eq!(mesh.normals.as_deref(), Some(&[vectors[1]][..]));
            assert_eq!(
                mesh.normal_basis,
                Some([vec![vectors[1]], vec![vectors[3]]])
            );
            assert_eq!(mesh.colors, Some(vec![colors[1]]));
            assert_eq!(mesh.texcoords, [uvs[0]]);
            let mut buffer = Vec::new();
            let mut views = Vec::new();
            let mut accessors = Vec::new();
            let (_, _, _, _, gltf) =
                append_geometry_mesh(&mut buffer, &mut views, &mut accessors, &mesh, 0);
            for (name, expected) in [
                ("NORMAL", vectors[1]),
                ("_NORMAL_BASIS_1", vectors[1]),
                ("_NORMAL_BASIS_2", vectors[3]),
            ] {
                let accessor = &accessors
                    [gltf["primitives"][0]["attributes"][name].as_u64().unwrap() as usize];
                let view = &views[accessor["bufferView"].as_u64().unwrap() as usize];
                let offset = view["byteOffset"].as_u64().unwrap() as usize;
                let actual = std::array::from_fn::<_, 3, _>(|i| {
                    f32::from_le_bytes(
                        buffer[offset + i * 4..offset + i * 4 + 4]
                            .try_into()
                            .unwrap(),
                    )
                });
                assert_eq!(actual, expected);
            }
            // The third independent index must be validated, too.
            let third = 3 + 3 * index_width(kind as u8);
            data[third] = 2;
            assert!(
                decode_indexed(
                    &data,
                    &positions,
                    &[&uvs],
                    Some(&colors),
                    Some(&normals),
                    &state
                )
                .is_err()
            );
        }
    }

    #[test]
    fn inline_attributes_use_loader_formats_with_independent_indexed_uvs() {
        // Source-derived packets: the loader binds XYZ/ST, irrespective of array stride.
        let numbers = [
            (
                0x01,
                vec![2, 4, 6],
                [1., 2., 3.],
                [2. / 128., 4. / 128., 6. / 128.],
            ),
            (
                0x11,
                vec![254, 4, 250],
                [-1., 2., -3.],
                [-2. / 64., 4. / 64., -6. / 64.],
            ),
            (
                0x21,
                [2u16, 512, 6]
                    .into_iter()
                    .flat_map(u16::to_be_bytes)
                    .collect(),
                [1., 256., 3.],
                [2. / 32768., 512. / 32768., 6. / 32768.],
            ),
            (
                0x31,
                [-2i16, 512, -6]
                    .into_iter()
                    .flat_map(i16::to_be_bytes)
                    .collect(),
                [-1., 256., -3.],
                [-2. / 16384., 512. / 16384., -6. / 16384.],
            ),
            (
                0x4f,
                [1f32, -2.5, 3.]
                    .into_iter()
                    .flat_map(f32::to_be_bytes)
                    .collect(),
                [1., -2.5, 3.],
                [1., -2.5, 3.],
            ),
        ];
        let colors = [
            (0x00, vec![0x80, 0x41], [132u8, 8, 8, 255]),
            (0x10, vec![19, 37, 61], [19, 37, 61, 255]),
            (0x20, vec![19, 37, 61, 7], [19, 37, 61, 255]),
            (0x30, vec![0x12, 0x34], [17, 34, 51, 68]),
            (0x40, vec![0x04, 0x20, 0xc4], [4, 8, 12, 16]),
            (0x50, vec![19, 37, 61, 7], [19, 37, 61, 7]),
        ];
        let secondary = [[7., 8.], [9., 10.]];
        let normals = NormalArray::Xyz(Vec::new());
        let state = RenderStateInfo {
            vcd: Some((1 << 2) | (1 << 4) | (1 << 6) | (1 << 10) | (3 << 12)),
            ..Default::default()
        };
        for (format, encoded, position, normal) in numbers {
            for (color_format, color_bytes, color) in &colors {
                let arrays = VertexArrays {
                    source: None,
                    positions: array(format, &[]),
                    texcoords: vec![array(format, &[]), array(0x40, &secondary)],
                    colors: Some(array(*color_format, &[])),
                    normals: Some((array::<3>(format, &[]).desc, &normals)),
                };
                let uv_bytes = &encoded[..encoded.len() / 3 * 2];
                let data = [
                    &[0xb8, 0, 1][..],
                    &encoded,
                    &encoded,
                    color_bytes,
                    uv_bytes,
                    &[0, 1],
                ]
                .concat();
                let mesh = decode_display_list(&data, &arrays, &state).unwrap();
                assert_eq!(mesh.positions, [position]);
                assert_eq!(mesh.normals, Some(vec![normal]));
                assert_eq!(mesh.texcoords, [[position[0], position[1]]]);
                assert_eq!(mesh.extra_texcoords, [(1, vec![secondary[1]])]);
                assert_eq!(mesh.colors, Some(vec![color.map(|v| f32::from(v) / 255.)]));
                assert_eq!(mesh.vertex_map, [[None; 2]]);
                assert_eq!(mesh.color_map, [None]);
                assert_eq!(mesh.normal_map, [None]);
                for length in 1..data.len() {
                    assert!(decode_display_list(&data[..length], &arrays, &state).is_err());
                }
                if format == 0x4f {
                    for offset in [3, 3 + encoded.len()] {
                        let mut invalid = data.clone();
                        invalid[offset..offset + 4].copy_from_slice(&f32::NAN.to_be_bytes());
                        assert!(decode_display_list(&invalid, &arrays, &state).is_err());
                    }
                }
            }
        }
    }

    #[test]
    fn inline_normal_bases_and_matrix_bytes_preserve_values_and_reject_inherited_formats() {
        let normals = NormalArray::Nbt3(Vec::new());
        let colors = [[1., 0., 0., 1.], [0., 1., 0., 1.]];
        let arrays = VertexArrays {
            source: None,
            positions: array(0x10, &[]),
            texcoords: Vec::new(),
            colors: Some(array(0x50, &colors)),
            normals: Some((
                VertexArrayDesc {
                    components: 2,
                    ..array::<3>(0x4f, &[]).desc
                },
                &normals,
            )),
        };
        let vectors = [[1f32, 2., 3.], [4., 5., 6.], [7., 8., 9.]];
        let mut data = vec![0xb8, 0, 1, 6, 254, 3, 4];
        data.extend(vectors.into_iter().flatten().flat_map(f32::to_be_bytes));
        data.push(1);
        let state = RenderStateInfo {
            vcd: Some(3 | (1 << 2) | (2 << 6) | (1 << 26)),
            matrix_commands: vec![(7 << 16) | 2],
            ..Default::default()
        };
        let mesh = decode_display_list(&data, &arrays, &state).unwrap();
        assert_eq!(mesh.positions, [[-2., 3., 4.]]);
        assert_eq!(mesh.normals, Some(vec![vectors[0]]));
        assert_eq!(
            mesh.normal_basis,
            Some([vec![vectors[1]], vec![vectors[2]]])
        );
        assert_eq!(mesh.joints, Some(vec![7]));
        assert_eq!(mesh.colors, Some(vec![colors[1]]));
        assert_eq!(mesh.color_map, [Some(1)]);
        let constant = VertexArrays {
            colors: Some(array(0x50, &colors[..1])),
            ..arrays
        };
        for vcd in [
            (1 << 2) | (1 << 6),
            (1 << 2) | (1 << 8),
            (1 << 2) | (1 << 10),
        ] {
            let unsupported = RenderStateInfo {
                vcd: Some(vcd),
                ..Default::default()
            };
            assert!(decode_display_list(&data, &constant, &unsupported).is_err());
        }
        data[0] |= 1;
        assert!(decode_display_list(&data, &constant, &state).is_err());
    }

    fn cp(register: u8, value: u32) -> Vec<u8> {
        [&[8, register][..], &value.to_be_bytes()].concat()
    }

    #[test]
    fn authored_vertex_formats_convert_raw_arrays_with_the_original_binding_stride() {
        // Source-derived CP packets: changing VAT conversion does not rebind arrays.
        let bytes = [0, 0, 0, 0, 0, 0, 99, 99, 6, 10, 0, 0, 0, 0];
        let values = [[0.; 3]; 2];
        let arrays = VertexArrays {
            source: Some(&bytes),
            positions: VertexArray {
                desc: VertexArrayDesc {
                    components: 4,
                    ..array(0x30, &values).desc
                },
                values: &values,
            },
            colors: None,
            normals: None,
            texcoords: Vec::new(),
        };
        let state = RenderStateInfo {
            vcd: Some(2 << 2),
            ..Default::default()
        };
        for (dequant, expected) in [(0, [6., 10., 0.]), (1, [3., 5., 0.])] {
            let data = [cp(0x71, (dequant << 30) | (1 << 4)), vec![0xb9, 0, 1, 1]].concat();
            let mesh = decode_display_list(&data, &arrays, &state).unwrap();
            assert_eq!(mesh.positions, [expected]);
            assert_eq!(mesh.vertex_map, [[Some(1), None]]);
        }
        // Five fractional bits belong to VAT even though GPL descriptors have four.
        let data = [
            cp(0x71, (2 << 1) | (17 << 4)),
            cp(0x50, 1 << 9),
            vec![0xb9, 0, 1, 0, 2, 0, 4],
        ]
        .concat();
        assert_eq!(
            decode_display_list(&data, &arrays, &state)
                .unwrap()
                .positions,
            [[2. / 131072., 4. / 131072., 0.]]
        );
        for register in [0xa0, 0xb0] {
            assert!(decode_display_list(&cp(register, 0), &arrays, &state).is_err());
        }
        for length in 1..6 {
            assert!(decode_display_list(&cp(0x71, 0)[..length], &arrays, &state).is_err());
        }
    }

    #[test]
    fn authored_vertex_formats_preserve_secondary_and_unused_constant_colors() {
        let constant = [[0.25, 0.5, 0.75, 1.]];
        let arrays = VertexArrays {
            source: None,
            positions: array(0x40, &[]),
            colors: Some(array(0x50, &constant)),
            normals: None,
            texcoords: Vec::new(),
        };
        let state = RenderStateInfo {
            vcd: Some((1 << 2) | (1 << 6) | (1 << 8)),
            ..Default::default()
        };
        let mut data = cp(0x70, 1 | (4 << 1) | (5 << 18));
        data.extend([0xb8, 0, 1]);
        data.extend([1f32, 2., 3.].into_iter().flat_map(f32::to_be_bytes));
        data.extend([0xf8, 0, 19, 37, 61, 7]);
        let mesh = decode_display_list(&data, &arrays, &state).unwrap();
        assert_eq!(mesh.colors, Some(constant.to_vec()));
        assert_eq!(mesh.unused_colors, Some(vec![[1., 0., 0., 1.]]));
        assert_eq!(
            mesh.secondary_colors,
            Some(vec![[19., 37., 61., 7.].map(|v| v / 255.)])
        );
        let (_, _, _, _, gltf) =
            append_geometry_mesh(&mut Vec::new(), &mut Vec::new(), &mut Vec::new(), &mesh, 0);
        for attribute in ["COLOR_0", "COLOR_1", "_UNUSED_COLOR_0"] {
            assert!(gltf["primitives"][0]["attributes"][attribute].is_number());
        }
    }

    #[test]
    fn authored_vertex_formats_index_normal_bases_with_bound_stride_and_component_offsets() {
        // The header binds XYZ with six-component strides; CP changes only conversion.
        let bytes: Vec<_> = (0..21)
            .flat_map(|value| (value as f32).to_be_bytes())
            .collect();
        let normals = NormalArray::Xyz(vec![[0.; 3]; 3]);
        let arrays = VertexArrays {
            source: Some(&bytes),
            positions: array(0x40, &[]),
            normals: Some((
                VertexArrayDesc {
                    data_offset: 0,
                    count: 3,
                    components: 6,
                    format: 0x40,
                },
                &normals,
            )),
            colors: None,
            texcoords: Vec::new(),
        };
        let state = RenderStateInfo {
            vcd: Some((1 << 2) | (2 << 4)),
            ..Default::default()
        };
        for (index3, indices, expected) in [
            (0, vec![0], [[3., 4., 5.], [6., 7., 8.]]),
            (1, vec![0, 1, 2], [[9., 10., 11.], [18., 19., 20.]]),
        ] {
            let mut data = cp(0x70, 1 | (4 << 1) | (1 << 9) | (4 << 10) | (index3 << 31));
            data.extend([0xb8, 0, 1]);
            data.extend([0; 12]);
            data.extend(indices);
            let mesh = decode_display_list(&data, &arrays, &state).unwrap();
            assert_eq!(mesh.normals, Some(vec![[0., 1., 2.]]));
            assert_eq!(mesh.normal_basis, Some(expected.map(|value| vec![value])));
        }
    }

    #[test]
    fn authored_vertex_formats_persist_until_native_layout_commands_rebind() {
        let values = [[2., 4., 6.]];
        let arrays = VertexArrays {
            source: None,
            positions: array(0x40, &values),
            colors: None,
            normals: None,
            texcoords: Vec::new(),
        };
        let mut state = RenderStateInfo {
            vcd: Some(2 << 2),
            ..Default::default()
        };
        let mut formats = vertex::Formats::new(&arrays);
        let writes = [cp(0x71, 1 | (4 << 1)), cp(0x50, 1 << 9)].concat();
        decode_display_list_with_formats(&writes, &arrays, &state, &mut formats).unwrap();
        let packet = [
            &[0xb9, 0, 1][..],
            &[1f32, 2., 3.]
                .into_iter()
                .flat_map(f32::to_be_bytes)
                .collect::<Vec<_>>(),
        ]
        .concat();
        assert_eq!(
            decode_display_list_with_formats(&packet, &arrays, &state, &mut formats)
                .unwrap()
                .remove(0)
                .positions,
            [[1., 2., 3.]]
        );
        assert!(decode_display_list(&packet, &arrays, &state).is_err());
        // Even an identical native VCD command rewrites the hardware layout.
        state.vertex_revision += 1;
        assert_eq!(
            decode_display_list_with_formats(&[0xb9, 0, 1, 0], &arrays, &state, &mut formats)
                .unwrap()
                .remove(0)
                .positions,
            values
        );
    }

    #[test]
    fn authored_vertex_formats_split_attribute_sets_without_reordering_draws() {
        // One native draw submits ordered CP/primitive packets. Only its layout changes.
        let point = |position: [f32; 3]| {
            [
                &[0xb8, 0, 1][..],
                &position
                    .into_iter()
                    .flat_map(f32::to_be_bytes)
                    .collect::<Vec<_>>(),
            ]
            .concat()
        };
        let mut display = point([1., 2., 3.]);
        display.extend(cp(0x50, (1 << 9) | (1 << 15)));
        display.extend(cp(0x70, 1 | (4 << 1) | (5 << 18)));
        display.extend(point([4., 5., 6.]));
        display.extend([19, 37, 61, 255]);
        display.extend(cp(0x50, 1 << 9));
        display.extend(point([7., 8., 9.]));
        // A conversion change with the same output attributes stays in the same mesh.
        display.extend(cp(0x70, 1 | (3 << 1) | (1 << 4)));
        display.extend([0xb8, 0, 1, 0, 2, 0, 4, 0, 6]);
        display.extend(cp(0x50, (1 << 9) | (1 << 15)));
        let mut bytes = vec![0; 96];
        for (at, value) in [
            (0, 0x005bbc61_u32),
            (12, 1),
            (16, 20),
            (20, 32),
            (24, 96 + display.len() as u32),
            (32, 24),
            (48, 32),
            (60, 0x4003),
            (68, 48),
            (80, 2 << 24),
            (84, 1 << 2),
            (88, 64),
            (92, display.len() as u32),
        ] {
            bytes[at..at + 4].copy_from_slice(&value.to_be_bytes());
        }
        bytes[72..74].copy_from_slice(&1u16.to_be_bytes());
        bytes.extend(display);
        bytes.extend(b"mesh\0");
        let objects = parse_geometry(&bytes).unwrap();
        assert_eq!(objects.len(), 3);
        assert_eq!(
            objects
                .iter()
                .map(|object| object.name.as_str())
                .collect::<Vec<_>>(),
            ["mesh", "mesh/part1", "mesh/part2"]
        );
        assert_eq!(objects[0].mesh.positions, [[1., 2., 3.]]);
        assert_eq!(objects[1].mesh.positions, [[4., 5., 6.]]);
        assert_eq!(objects[2].mesh.positions, [[7., 8., 9.], [1., 2., 3.]]);
        assert_eq!(objects[2].mesh.indices, [0, 1]);
        assert_eq!(
            objects[1].mesh.secondary_colors,
            Some(vec![[19., 37., 61., 255.].map(|v| v / 255.)])
        );
        for (index, object) in objects.iter().enumerate() {
            assert_eq!(object.source_index, 0);
            assert_eq!(object.render_state, objects[0].render_state);
            let (_, _, _, _, gltf) = append_geometry_mesh(
                &mut Vec::new(),
                &mut Vec::new(),
                &mut Vec::new(),
                &object.mesh,
                0,
            );
            assert_eq!(
                gltf["primitives"][0]["attributes"]["COLOR_1"].is_number(),
                index == 1
            );
        }
        assert_eq!(
            object_draw_recipes(&objects, None, DecodeMode::Physical)
                .unwrap()
                .iter()
                .map(|draw| (draw.object, draw.order))
                .collect::<Vec<_>>(),
            [(0, 0), (1, 1), (2, 2)]
        );
    }

    #[test]
    fn authored_vertex_formats_split_normal_basis_changes() {
        let arrays = VertexArrays {
            source: None,
            positions: array(0x40, &[]),
            colors: None,
            normals: None,
            texcoords: Vec::new(),
        };
        let state = RenderStateInfo {
            vcd: Some((1 << 2) | (1 << 4)),
            ..Default::default()
        };
        let mut display = Vec::new();
        for basis in [false, true, false] {
            display.extend(cp(0x70, 1 | (4 << 1) | (4 << 10) | (u32::from(basis) << 9)));
            display.extend([0xb8, 0, 1]);
            display.extend([0; 12]);
            display.extend(
                (0..if basis { 9 } else { 3 }).flat_map(|value| (value as f32).to_be_bytes()),
            );
        }
        let meshes = decode_display_list_with_formats(
            &display,
            &arrays,
            &state,
            &mut vertex::Formats::new(&arrays),
        )
        .unwrap();
        assert_eq!(meshes.len(), 3);
        for (index, mesh) in meshes.iter().enumerate() {
            assert_eq!(mesh.normals, Some(vec![[0., 1., 2.]]));
            assert_eq!(mesh.indices, [0]);
            assert_eq!(mesh.normal_basis.is_some(), index == 1);
        }
        assert_eq!(
            meshes[1].normal_basis,
            Some([vec![[3., 4., 5.]], vec![[6., 7., 8.]]])
        );
    }

    #[test]
    #[ignore = "requires both original extracted discs; model metadata only"]
    fn original_party_models_preserve_names_ids_parents_and_draw_order() -> anyhow::Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let mut checked = 0;
        for disc in ["disc1", "disc2"] {
            for name in [
                "lloyd", "collet", "genius", "refill", "kratos", "zelosz", "shihna", "presea",
                "regal",
            ] {
                let package = fs::read(root.join(disc).join(format!("files/{name}000.bin")))?;
                let parts = crate::field::sections(&package)?;
                for part in parts.iter().take(2).flatten() {
                    let part = &package[part.clone()];
                    let bytes = &part[skeleton_range(part)?];
                    let model = Model::parse(bytes)?;
                    let bindings = crate::animation::ModelBindings::read(bytes)?;
                    let info = model_node_info(&model);
                    assert_eq!(model.root_offset, 32);
                    let mut expected_draws = Vec::new();
                    let mut labels = &bytes[crate::read::u32(bytes, 28)? as usize..];
                    for (index, node) in model.nodes.iter().enumerate() {
                        let at = 32 + index * 28;
                        assert_eq!(node.source_offset, at);
                        assert_eq!(bindings.node_ids[index], crate::read::u16(bytes, at + 22)?);
                        let end = labels.iter().position(|&byte| byte == 0).unwrap();
                        let name = labels[..end].escape_ascii().to_string();
                        labels = &labels[end + 1..];
                        assert_eq!(bindings.names.as_ref().unwrap()[index], name);
                        assert_eq!(info[index].name, name);
                        let parent = crate::read::u32(bytes, at + 12)? as usize;
                        assert_eq!(node.parent, (parent != 0).then(|| (parent - 32) / 28));
                        let object = crate::read::u16(bytes, at + 20)?;
                        if object != u16::MAX {
                            expected_draws.push((bytes[at + 25], object, index));
                        }
                    }
                    expected_draws.sort_by_key(|&(priority, _, _)| priority);
                    let root_geometry = crate::read::u16(bytes, 20)?;
                    let expected: Vec<_> = (root_geometry != u16::MAX)
                        .then_some((root_geometry, None))
                        .into_iter()
                        .chain(
                            expected_draws
                                .into_iter()
                                .map(|(_, object, index)| (object, Some(index))),
                        )
                        .collect();
                    assert_eq!(model_draw_order(&model), expected);
                    checked += 1;
                }
            }
        }
        assert_eq!(checked, 36);
        Ok(())
    }

    #[test]
    #[ignore = "requires original extracted disc"]
    fn original_sheena_skin_uses_both_authored_texture_arrays() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../local/extracted/disc1/files/shihna003.bin");
        let bytes = fs::read(path).unwrap();
        let primary = crate::field::sections(&bytes).unwrap()[0].clone().unwrap();
        let source = section_source(&bytes[primary]).unwrap();
        let objects = parse_geometry(source.gpl).unwrap();
        let skin = &objects[0].mesh;
        assert_eq!(objects[0].name, "shi000_skin");
        assert_eq!(objects[0].texcoord_count, 1846);
        assert_eq!(skin.extra_texcoords[0].0, 1);
        assert_eq!(skin.extra_texcoords[0].1.len(), skin.positions.len());
        assert_ne!(skin.extra_texcoords[0].1, skin.texcoords);
        assert!(!skin.indices.is_empty());
    }

    #[test]
    #[ignore = "requires original extracted disc"]
    fn original_unused_meshes_survive_without_invented_instances() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../local/extracted/disc1/files/MAP/yum_d00.bin");
        let map = crate::field::MapArchive::open(&path).unwrap();
        let package = map.section(28).unwrap();
        let range = crate::field::sections(package).unwrap()[0].clone().unwrap();
        let bytes =
            crate::character::texture_palette(&package[range.clone()], &package[range]).unwrap();
        let source = section_source(&bytes).unwrap();
        let objects = parse_geometry(source.gpl).unwrap();
        assert!(object_draw_recipes(&objects, source.model.as_ref(), DecodeMode::Runtime).is_err());
        let draws =
            object_draw_recipes(&objects, source.model.as_ref(), DecodeMode::Physical).unwrap();
        assert_eq!(draws.len(), objects.len());
        assert!(draws.iter().any(|draw| !draw.instanced));

        let DecodedGeometry { manifest, gltf, .. } =
            decode_section(&bytes, DecodeMode::Physical, |_, _| Ok(())).unwrap();
        let nodes = gltf["nodes"].as_array().unwrap();
        let attached: std::collections::BTreeSet<_> = gltf["scenes"][0]["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .chain(
                nodes
                    .iter()
                    .flat_map(|node| node["children"].as_array().into_iter().flatten()),
            )
            .map(|index| index.as_u64().unwrap() as usize)
            .collect();
        assert_eq!(manifest.objects.len(), objects.len());
        assert_eq!(gltf["meshes"].as_array().unwrap().len(), objects.len());
        assert!(
            nodes
                .iter()
                .enumerate()
                .any(|(index, node)| node.get("mesh").is_some() && !attached.contains(&index))
        );
    }
}
