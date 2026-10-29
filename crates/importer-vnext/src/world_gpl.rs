//! World GPL positions, normals, colors and UVs directly into an editable GLB.
use crate::{dol, world_terrain_collision::section};
use anyhow::{Context, Result, ensure};
use resonance_asset_writer::gltf::{self, Attribute};
use resonance_content::{SceneImage, SceneMaterial, ScenePart, TextureBinding, TextureWrap};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    sync::OnceLock,
};

pub(crate) fn is_model(bytes: &[u8]) -> bool {
    bytes.get(32..36).is_some_and(|word| {
        word == 0x005b_bc61_u32.to_be_bytes() || word == 0x00b7_49e0_u32.to_be_bytes()
    })
}

fn joint_name(name: String) -> String {
    static NAMES: OnceLock<BTreeMap<String, String>> = OnceLock::new();
    NAMES
        .get_or_init(|| {
            serde_json::from_str(include_str!("model-joints.json")).expect("joint names")
        })
        .get(&name)
        .cloned()
        .unwrap_or(name)
}

fn shadow_bounds_includes(name: &str) -> bool {
    let prefix = name.as_bytes().get(..2).unwrap_or_default();
    if prefix.eq_ignore_ascii_case(b"mo") {
        let digits = name.as_bytes()[2..]
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .count();
        return name[2..2 + digits]
            .parse::<u8>()
            .is_ok_and(|radius| (1..=15).contains(&radius));
    }
    ![b"dm", b"at", b"kk", b"pa", b"ab", b"ef", b"ki", b"ns"]
        .iter()
        .any(|tag| prefix.eq_ignore_ascii_case(*tag))
        && ![
            "Bone_ude",
            "Bone_te",
            "Bone_yu",
            "Bone_ring",
            "obj_nuno01",
            "manto",
        ]
        .iter()
        .any(|part| name.contains(part))
        && !["arm-", "hand-", "wrist-", "finger-"]
            .iter()
            .any(|part| name.starts_with(part))
}

#[derive(Clone)]
struct Mesh {
    source: usize,
    name: String,
    positions: Vec<[f32; 3]>,
    normals: Option<Vec<[f32; 3]>>,
    colors: Option<Vec<[f32; 4]>>,
    joints: Option<Vec<u16>>,
    uvs: Vec<[f32; 2]>,
    second_uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
    primitives: Vec<(u8, std::ops::Range<usize>)>,
    mode: u32,
    textures: Vec<u32>,
    texture_tables: Vec<Option<String>>,
}

fn bytes(source: &[u8], at: usize, count: usize) -> Result<&[u8]> {
    source
        .get(at..at + count)
        .context("terrain geometry range exceeds source")
}

fn raw_label(source: &[u8], at: usize) -> Result<String> {
    let remaining = source.get(at..).context("terrain label exceeds source")?;
    let end = remaining
        .iter()
        .position(|&byte| byte == 0)
        .context("unterminated terrain label")?;
    let original = &remaining[..end];
    let (decoded, invalid) = encoding_rs::SHIFT_JIS.decode_without_bom_handling(original);
    if invalid || encoding_rs::SHIFT_JIS.encode(&decoded).0.as_ref() != original {
        return Ok(original.escape_ascii().to_string());
    }
    let mut name = String::new();
    for character in decoded.chars() {
        if character.is_ascii() {
            name.extend([character as u8].escape_ascii().map(char::from));
        } else if character.is_control() {
            name.extend(character.escape_default());
        } else {
            name.push(character);
        }
    }
    Ok(name)
}

fn label(source: &[u8], at: usize) -> Result<String> {
    Ok(joint_name(raw_label(source, at)?))
}

struct Array {
    start: usize,
    count: usize,
    format: u8,
    components: u8,
}

fn array(object: &[u8], at: usize) -> Result<Array> {
    let packed = dol::u32(object, at + 4)?;
    Ok(Array {
        start: dol::u32(object, at)? as usize,
        count: (packed >> 16) as usize,
        format: ((packed >> 8) & 255) as u8,
        components: packed as u8,
    })
}

fn vector<const N: usize>(
    object: &[u8],
    array: &Array,
    index: usize,
    normal: bool,
) -> Result<[f32; N]> {
    let format = array.format >> 4;
    ensure!(
        format <= 4 && usize::from(array.components) == N && index < array.count,
        "unsupported GPL vertex array: format {:#x}, components {}, expected {N}, index {index}/{}",
        array.format,
        array.components,
        array.count
    );
    let width = match format {
        0 | 1 => 1,
        2 | 3 => 2,
        _ => 4,
    };
    let row = bytes(object, array.start + index * N * width, N * width)?;
    let fraction = if format == 4 {
        0
    } else if normal {
        [7, 6, 15, 14][format as usize]
    } else {
        array.format & 15
    };
    let scale = 2.0_f32.powi(-i32::from(fraction));
    let mut result = [0.; N];
    for (axis, value) in result.iter_mut().enumerate() {
        *value = match format {
            0 => f32::from(row[axis]),
            1 => f32::from(row[axis] as i8),
            2 => f32::from(dol::u16(row, axis * 2)?),
            3 => f32::from(dol::u16(row, axis * 2)? as i16),
            _ => f32::from_bits(dol::u32(row, axis * 4)?),
        } * scale;
        ensure!(value.is_finite(), "non-finite terrain vertex");
    }
    Ok(result)
}

fn expand(value: u16, bits: u8) -> f32 {
    let byte = match bits {
        4 => (value << 4) | value,
        5 => (value << 3) | (value >> 2),
        6 => (value << 2) | (value >> 4),
        _ => unreachable!(),
    };
    f32::from(byte) / 255.
}

fn color(object: &[u8], array: &Array, index: usize) -> Result<[f32; 4]> {
    ensure!(index < array.count, "terrain color index exceeds array");
    let mut result = match array.format {
        0 => {
            let value = dol::u16(object, array.start + index * 2)?;
            [
                expand(value >> 11, 5),
                expand((value >> 5) & 63, 6),
                expand(value & 31, 5),
                1.,
            ]
        }
        16 => {
            let rgb = bytes(object, array.start + index * 3, 3)?;
            [
                f32::from(rgb[0]) / 255.,
                f32::from(rgb[1]) / 255.,
                f32::from(rgb[2]) / 255.,
                1.,
            ]
        }
        32 => {
            let rgbx = bytes(object, array.start + index * 4, 4)?;
            [
                f32::from(rgbx[0]) / 255.,
                f32::from(rgbx[1]) / 255.,
                f32::from(rgbx[2]) / 255.,
                1.,
            ]
        }
        48 => {
            let value = dol::u16(object, array.start + index * 2)?;
            [12, 8, 4, 0].map(|shift| expand((value >> shift) & 15, 4))
        }
        80 => bytes(object, array.start + index * 4, 4)?
            .try_into()
            .map(|rgba: [u8; 4]| rgba.map(|value| f32::from(value) / 255.))?,
        _ => anyhow::bail!("unsupported terrain color format {}", array.format),
    };
    if array.count == 1 {
        // Native material constants retain the packed channel precision.
        for (channel, mask) in result.iter_mut().zip(match array.format {
            0 => [248, 252, 248, 255],
            48 => [240; 4],
            _ => [255; 4],
        }) {
            let byte = (*channel * 255.) as u8;
            *channel = f32::from(byte & mask) / 255.;
        }
    }
    Ok(result)
}

fn indexed(list: &[u8], cursor: &mut usize, kind: u8) -> Result<usize> {
    let width = match kind {
        2 => 1,
        3 => 2,
        _ => anyhow::bail!("unsupported terrain vertex index kind {kind}"),
    };
    let value = bytes(list, *cursor, width)?;
    *cursor += width;
    Ok(if width == 1 {
        usize::from(value[0])
    } else {
        usize::from(u16::from_be_bytes([value[0], value[1]]))
    })
}

fn display(
    list: &[u8],
    source: usize,
    name: String,
    object: &[u8],
    position: &Array,
    normal_array: Option<&Array>,
    color_array: Option<&Array>,
    uvs: &[Array],
    vcd: u32,
    mode: u32,
    textures: Vec<u32>,
    texture_tables: &[Option<String>],
    matrices: &[u32],
) -> Result<Mesh> {
    let kind = |shift: u32| ((vcd >> shift) & 3) as u8;
    ensure!(
        vcd & !0x3fff == 0
            && vcd & 2 == 0
            && matches!(kind(2), 2 | 3)
            && matches!(kind(4), 0 | 2 | 3)
            && matches!(kind(6), 0 | 2 | 3)
            && matches!(kind(10), 0 | 2 | 3)
            && matches!(kind(12), 0 | 2 | 3)
            // A GPL object may declare a UV array that this draw does not use.
            && (kind(10) == 0 || !uvs.is_empty())
            && (kind(12) == 0 || uvs.len() == 2),
        "unsupported terrain vertex layout 0x{vcd:08x}"
    );
    let constant = color_array.filter(|array| array.count == 1);
    let mut mesh = Mesh {
        source,
        name,
        positions: Vec::new(),
        normals: normal_array.map(|_| Vec::new()),
        colors: color_array.map(|_| Vec::new()),
        joints: (vcd & 1 != 0).then(Vec::new),
        uvs: Vec::new(),
        second_uvs: Vec::new(),
        indices: Vec::new(),
        primitives: Vec::new(),
        mode,
        textures,
        texture_tables: texture_tables.to_vec(),
    };
    let mut cursor = 0;
    while cursor < list.len() {
        let opcode = list[cursor];
        cursor += 1;
        if opcode == 0 {
            continue;
        }
        ensure!(
            matches!(opcode, 0x80 | 0x90 | 0x98 | 0xa0 | 0xa8 | 0xb0 | 0xb8),
            "unsupported terrain GX command 0x{opcode:02x}"
        );
        let count = usize::from(dol::u16(list, cursor)?);
        cursor += 2;
        let base = mesh.positions.len() as u32;
        for _ in 0..count {
            if let Some(joints) = &mut mesh.joints {
                let slot = usize::from(*bytes(list, cursor, 1)?.first().unwrap()) / 3;
                cursor += 1;
                let joint = matrices
                    .iter()
                    .find(|value| (**value & 0xffff) as usize == slot)
                    .with_context(|| format!("unbound joint matrix slot {slot}"))?;
                joints.push((joint >> 16) as u16);
            }
            mesh.positions.push(vector(
                object,
                position,
                indexed(list, &mut cursor, kind(2))?,
                false,
            )?);
            if kind(4) != 0 {
                let normals = normal_array.context("unbound terrain normal array")?;
                mesh.normals.as_mut().unwrap().push(vector(
                    object,
                    normals,
                    indexed(list, &mut cursor, kind(4))?,
                    true,
                )?);
            }
            if kind(6) != 0 {
                let colors = color_array.context("unbound terrain color array")?;
                let value = color(object, colors, indexed(list, &mut cursor, kind(6))?)?;
                mesh.colors.as_mut().unwrap().push(value);
            } else if let Some(colors) = constant {
                mesh.colors
                    .as_mut()
                    .unwrap()
                    .push(color(object, colors, 0)?);
            }
            mesh.uvs.push(if kind(10) == 0 {
                if let Some(array) = uvs.first() {
                    vector(object, array, 0, false)?
                } else {
                    [0.; 2]
                }
            } else {
                vector(
                    object,
                    &uvs[0],
                    indexed(list, &mut cursor, kind(10))?,
                    false,
                )?
            });
            if kind(12) != 0 {
                mesh.second_uvs.push(vector(
                    object,
                    &uvs[1],
                    indexed(list, &mut cursor, kind(12))?,
                    false,
                )?);
            }
        }
        let first = mesh.indices.len();
        let mut triangle = |a, b, c| mesh.indices.extend([base + a, base + c, base + b]);
        match opcode {
            0x80 => {
                for i in 0..count / 4 {
                    let a = (i * 4) as u32;
                    triangle(a, a + 1, a + 2);
                    triangle(a, a + 2, a + 3);
                }
            }
            0x90 => {
                for i in 0..count / 3 {
                    let a = (i * 3) as u32;
                    triangle(a, a + 1, a + 2);
                }
            }
            0x98 => {
                for i in 0..count.saturating_sub(2) {
                    let a = i as u32;
                    if i % 2 == 0 {
                        triangle(a, a + 1, a + 2);
                    } else {
                        triangle(a + 1, a, a + 2);
                    }
                }
            }
            0xa0 => {
                for i in 1..count.saturating_sub(1) {
                    triangle(0, i as u32, (i + 1) as u32);
                }
            }
            0xa8 => {
                for i in 0..count / 2 {
                    let a = base + (i * 2) as u32;
                    mesh.indices.extend([a, a + 1]);
                }
            }
            0xb0 => {
                for i in 0..count.saturating_sub(1) {
                    let a = base + i as u32;
                    mesh.indices.extend([a, a + 1]);
                }
            }
            0xb8 => mesh.indices.extend((0..count).map(|i| base + i as u32)),
            _ => unreachable!(),
        }
        let mode = match opcode {
            0xa8 | 0xb0 => 1,
            0xb8 => 0,
            _ => 4,
        };
        let end = mesh.indices.len();
        if end > first {
            if let Some((prior_mode, range)) = mesh.primitives.last_mut()
                && *prior_mode == mode
                && range.end == first
            {
                range.end = end;
            } else {
                mesh.primitives.push((mode, first..end));
            }
        }
    }
    Ok(mesh)
}

fn geometry(resource: &[u8]) -> Result<Vec<Mesh>> {
    // Palette-free GPLs point straight from geometry to the model section.
    let palette = dol::u32(resource, 0)? as usize;
    let end = if palette == 0 {
        dol::u32(resource, 4)? as usize
    } else {
        palette
    };
    ensure!(end >= 0x20, "terrain GPL begins before resource header");
    let gpl = bytes(resource, 0x20, end - 0x20)?;
    let revision = dol::u32(gpl, 0)?;
    ensure!(
        matches!(revision, 0x005b_bc61 | 0x00b7_49e0),
        "unsupported terrain GPL"
    );
    let modern = revision == 0x005b_bc61;
    let count = dol::u32(gpl, 12)? as usize;
    let table = dol::u32(gpl, 16)? as usize;
    let mut meshes = Vec::new();
    for index in 0..count {
        let at = dol::u32(gpl, table + index * 8)? as usize;
        let next = if index + 1 == count {
            gpl.len()
        } else {
            dol::u32(gpl, table + (index + 1) * 8)? as usize
        };
        let object = bytes(gpl, at, next - at)?;
        let name = label(gpl, dol::u32(gpl, table + index * 8 + 4)? as usize)?;
        let position = array(object, dol::u32(object, 0)? as usize)?;
        let color_array = (dol::u32(object, 4)? != 0)
            .then(|| array(object, dol::u32(object, 4).unwrap() as usize))
            .transpose()?;
        let normal_array = (dol::u32(object, 12)? != 0)
            .then(|| array(object, dol::u32(object, 12).unwrap() as usize))
            .transpose()?;
        let texture = dol::u32(object, 8)? as usize;
        let uvs = (0..usize::from(object[20]))
            .map(|slot| array(object, texture + slot * 16))
            .collect::<Result<Vec<_>>>()?;
        let texture_tables = (0..uvs.len())
            .map(|slot| {
                let label_at = dol::u32(object, texture + slot * 16 + 8)? as usize;
                if label_at == 0 {
                    Ok(None)
                } else {
                    raw_label(gpl, at + label_at).map(Some)
                }
            })
            .collect::<Result<Vec<_>>>()?;
        ensure!(uvs.len() <= 2, "unsupported terrain UV count");
        let material = dol::u32(object, 16)? as usize;
        let commands = dol::u32(object, material + 4)? as usize;
        let command_count = usize::from(dol::u16(object, material + 8)?);
        let mut vcd = None;
        let mut mode = None;
        let mut textures = Vec::<u32>::new();
        let mut matrices = Vec::<u32>::new();
        let mut draw = 0;
        for command in 0..command_count {
            let at = commands + command * 16;
            let kind = *object.get(at).context("truncated terrain render command")?;
            let value = dol::u32(object, at + 4)?;
            match (modern, kind) {
                (_, 1) => {
                    let stage = (value >> 13) & 7;
                    textures.retain(|old| (old >> 13) & 7 != stage);
                    textures.push(value);
                    textures.sort_by_key(|word| (word >> 13) & 7);
                }
                (true, 2) | (false, 3) => vcd = Some(value),
                (true, 3) => mode = Some(value),
                (false, 4) => {
                    mode = Some(match value {
                        0 | 1 | 3 => value + 1,
                        // Old vertex-color material has no implied translucency.
                        4 => 0x15,
                        _ => anyhow::bail!("unsupported old GPL material {value}"),
                    })
                }
                (true, 4) | (false, 5) => {
                    matrices.retain(|old| (old & 0xffff) != (value & 0xffff));
                    matrices.push(value);
                }
                _ if !modern => {}
                _ => anyhow::bail!("unknown terrain render command {kind}"),
            }
            let start = dol::u32(object, at + 8)? as usize;
            let size = dol::u32(object, at + 12)? as usize;
            if start == 0 || size == 0 {
                continue;
            }
            let draw_name = if draw == 0 {
                name.clone()
            } else {
                format!("{name}/draw{draw}")
            };
            meshes.push(display(
                bytes(object, start, size)?,
                index,
                draw_name,
                object,
                &position,
                normal_array.as_ref(),
                color_array.as_ref(),
                &uvs,
                vcd.context("terrain draw has no vertex layout")?,
                mode.context("terrain draw has no material mode")?,
                textures.clone(),
                &texture_tables,
                &matrices,
            )?);
            draw += 1;
        }
    }
    Ok(meshes)
}

struct Node {
    name: String,
    source_name: String,
    object: u16,
    binding_id: u16,
    parent: Option<usize>,
    position: [f32; 3],
    rotation: [f32; 4],
    scale: [f32; 3],
    camera_facing: bool,
    priority: u8,
}

fn model(resource: &[u8]) -> Result<Vec<Node>> {
    let at = dol::u32(resource, 4)? as usize;
    let size = dol::u32(resource, 8)? as usize;
    if at == 0 && size == 0 {
        return Ok(Vec::new());
    }
    let model = bytes(resource, at, size)?;
    ensure!(
        dol::u32(model, 0)? == 0x007b_7960,
        "unsupported terrain model"
    );
    let count = usize::from(dol::u16(model, 6)?);
    let names_at = dol::u32(model, 28)? as usize;
    let mut names = Vec::new();
    let mut cursor = names_at;
    for _ in 0..count {
        let name = raw_label(model, cursor)?;
        cursor += bytes(model, cursor, model.len() - cursor)?
            .iter()
            .position(|&byte| byte == 0)
            .context("unterminated terrain node name")?
            + 1;
        names.push(name);
    }
    let mut nodes = Vec::new();
    let mut seen = BTreeSet::new();
    let mut pending = vec![(dol::u32(model, 12)? as usize, None)];
    while let Some((at, parent)) = pending.pop() {
        if at == 0 {
            continue;
        }
        ensure!(
            at >= 0x20 && (at - 0x20) % 0x1c == 0 && seen.insert(at),
            "invalid terrain model hierarchy"
        );
        let row = bytes(model, at, 0x1c)?;
        let data_at = dol::u32(row, 0)? as usize;
        let flags = if data_at == 0 {
            0
        } else {
            dol::u32(model, data_at)? >> 24
        };
        let float = |word: usize| -> Result<f32> {
            if data_at == 0 {
                Ok(0.)
            } else {
                Ok(f32::from_bits(dol::u32(model, data_at + word * 4)?))
            }
        };
        let position = if flags & 8 != 0 {
            [float(8)?, float(9)?, float(10)?]
        } else {
            [0.; 3]
        };
        let rotation = if flags & 4 != 0 {
            [float(4)?, float(5)?, float(6)?, float(7)?]
        } else {
            [0., 0., 0., 1.]
        };
        let scale = if flags & 1 != 0 {
            [float(1)?, float(2)?, float(3)?]
        } else {
            [1.; 3]
        };
        let index = nodes.len();
        pending.push((dol::u32(row, 8)? as usize, parent));
        pending.push((dol::u32(row, 16)? as usize, Some(index)));
        let source_name = names
            .get(index)
            .filter(|name| !name.is_empty())
            .cloned()
            .unwrap_or_else(|| format!("gpl_node_{index:03}"));
        nodes.push(Node {
            name: joint_name(source_name.clone()),
            source_name,
            object: dol::u16(row, 20)?,
            binding_id: dol::u16(row, 22)?,
            parent,
            position,
            rotation,
            scale,
            camera_facing: dol::u16(row, 26)? >> 8 != 0,
            priority: row[25],
        });
    }
    ensure!(nodes.len() == count, "incomplete terrain model hierarchy");
    let reserved: BTreeSet<_> = nodes.iter().map(|node| node.name.clone()).collect();
    let mut used = BTreeSet::new();
    for node in &mut nodes {
        if used.insert(node.name.clone()) {
            continue;
        }
        for suffix in 2.. {
            let name = format!("{}-{suffix}", node.name);
            if !reserved.contains(&name) && used.insert(name.clone()) {
                node.name = name;
                break;
            }
        }
    }
    Ok(nodes)
}

pub fn animation_bindings(resource: &[u8]) -> Result<Vec<(u16, String)>> {
    Ok(model(resource)?
        .into_iter()
        .map(|node| (node.binding_id, node.source_name))
        .collect())
}

/// Native binding ID, editable GLB joint label, and original joint label.
pub(crate) fn named_animation_bindings(resource: &[u8]) -> Result<Vec<(u16, String, String)>> {
    Ok(model(resource)?
        .into_iter()
        .map(|node| (node.binding_id, node.name, node.source_name))
        .collect())
}

/// Match native binding IDs to the editable joint labels emitted in the GLB.
pub(crate) fn animation_target_bindings(bindings: Vec<(u16, String)>) -> Vec<(u16, String)> {
    let mut names = bindings
        .into_iter()
        .map(|(id, source)| (id, joint_name(source)))
        .collect::<Vec<_>>();
    let reserved = names
        .iter()
        .map(|(_, name)| name.clone())
        .collect::<BTreeSet<_>>();
    let mut used = BTreeSet::new();
    for (_, name) in &mut names {
        if used.insert(name.clone()) {
            continue;
        }
        for suffix in 2.. {
            let candidate = format!("{name}-{suffix}");
            if !reserved.contains(&candidate) && used.insert(candidate.clone()) {
                *name = candidate;
                break;
            }
        }
    }
    names
}

pub(crate) fn noishe_hair(
    resource: &[u8],
) -> Result<resonance_content::secondary_motion::Definition> {
    use resonance_content::secondary_motion::{ChainDefinition, Definition, Parameter};
    let nodes = model(resource)?;
    let roots: Vec<_> = nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| node.source_name.starts_with("AB_ROOT"))
        .collect();
    let [(first, root)] = roots.as_slice() else {
        anyhow::bail!("Noishe needs one native hair chain");
    };
    let mut index = *first;
    ensure!(
        root.source_name.contains("_kami00_"),
        "unexpected Noishe hair root"
    );
    let parameter = |tag: &str| -> Result<f32> {
        let value = root
            .source_name
            .split_once(tag)
            .with_context(|| format!("Noishe hair is missing {tag}"))?
            .1
            .chars()
            .take_while(|character| {
                character.is_ascii_digit() || matches!(character, '-' | '+' | 'e' | '.')
            })
            .map(|character| if character == 'e' { '.' } else { character })
            .collect::<String>()
            .parse::<f32>()?;
        ensure!(value.is_finite(), "nonfinite Noishe hair parameter");
        Ok(value)
    };
    let mut joints = Vec::new();
    loop {
        ensure!(joints.len() < 128, "oversized Noishe hair chain");
        joints.push(nodes[index].name.clone());
        let children: Vec<_> = nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.parent == Some(index))
            .collect();
        match children.as_slice() {
            [] => break,
            [(next, _)] => index = *next,
            _ => anyhow::bail!("branching Noishe hair chain"),
        }
    }
    let names = nodes
        .iter()
        .map(|node| node.name.clone())
        .collect::<Vec<_>>();
    let definition = Definition {
        chains: vec![ChainDefinition {
            name: "hair-0".into(),
            joints,
            gravity: Parameter::Uniform(parameter("_Dt")?),
            damping: Parameter::Uniform(parameter("_Bb")?),
            attraction: parameter("_Pow")?,
            pose_attraction: 0.2,
            preserve_rotation: root.source_name.contains("_NR_"),
            lock_rotation_x: false,
            lock_rotation_y: false,
            collision_plane: None,
        }],
    };
    definition.prepare(&names)?;
    Ok(definition)
}

pub(crate) fn thoda_cape(
    resource: &[u8],
) -> Result<resonance_content::secondary_motion::Definition> {
    use resonance_content::secondary_motion::{ChainDefinition, Definition, Parameter};
    let nodes = model(resource)?;
    let mut chains = Vec::new();
    for (side, tag) in [("cape-left", "_manto_L"), ("cape-right", "_manto_R")] {
        let (mut index, root) = nodes
            .iter()
            .enumerate()
            .find(|(_, node)| {
                node.source_name.starts_with("AB_ROOT") && node.source_name.contains(tag)
            })
            .with_context(|| format!("missing {side} source chain"))?;
        let parameter = |tag: &str| -> Result<f32> {
            let value = root
                .source_name
                .split_once(tag)
                .with_context(|| format!("{side} is missing {tag}"))?
                .1
                .chars()
                .take_while(|character| {
                    character.is_ascii_digit() || matches!(character, '-' | '+' | 'e' | '.')
                })
                .map(|character| if character == 'e' { '.' } else { character })
                .collect::<String>()
                .parse::<f32>()?;
            ensure!(value.is_finite(), "nonfinite cape parameter");
            Ok(value)
        };
        let mut joints = Vec::new();
        loop {
            ensure!(joints.len() < 128, "oversized cape chain");
            joints.push(nodes[index].name.clone());
            let children = nodes
                .iter()
                .enumerate()
                .filter(|(_, node)| node.parent == Some(index))
                .collect::<Vec<_>>();
            match children.as_slice() {
                [] => break,
                [(next, _)] => index = *next,
                _ => anyhow::bail!("branching cape chain"),
            }
        }
        chains.push(ChainDefinition {
            name: side.into(),
            joints,
            gravity: Parameter::Uniform(parameter("_Dt")?),
            damping: Parameter::Uniform(parameter("_Bb")?),
            attraction: parameter("_Pow")?,
            pose_attraction: 0.2,
            preserve_rotation: root.source_name.contains("_NR_"),
            lock_rotation_x: false,
            lock_rotation_y: false,
            collision_plane: None,
        });
    }
    let names = nodes.into_iter().map(|node| node.name).collect::<Vec<_>>();
    let definition = Definition { chains };
    definition.prepare(&names)?;
    Ok(definition)
}

fn bind_transform(index: usize, nodes: &[Node]) -> Result<glam::Mat4> {
    let mut chain = Vec::new();
    let mut cursor = Some(index);
    while let Some(current) = cursor {
        ensure!(
            current < nodes.len() && !chain.contains(&current),
            "invalid model hierarchy"
        );
        chain.push(current);
        cursor = nodes[current].parent;
    }
    let mut matrix = glam::Mat4::IDENTITY;
    for index in chain.into_iter().rev() {
        let node = &nodes[index];
        matrix *= glam::Mat4::from_scale_rotation_translation(
            glam::Vec3::from_array(node.scale),
            glam::Quat::from_array(node.rotation),
            glam::Vec3::from_array(node.position),
        );
    }
    Ok(matrix)
}

fn decode_resource(
    resource: &[u8],
) -> Result<(Vec<u8>, Vec<Mesh>, Vec<Node>, Vec<Option<usize>>, Vec<bool>)> {
    let source_meshes = geometry(resource)?;
    let model = model(resource)?;
    let mut buffer = Vec::new();
    let mut views = Vec::<Value>::new();
    let mut accessors = Vec::<Value>::new();
    let mut meshes = Vec::new();
    let mut nodes = model
        .iter()
        .enumerate()
        .map(|(index, node)| {
            let mut extras = json!({"resonance_bone":index});
            if node.camera_facing {
                extras["resonance_camera_facing"] = json!(true);
            }
            if !shadow_bounds_includes(&node.name) {
                extras["shadow_bounds"] = json!(false);
            }
            json!({"name":node.name,"translation":node.position,"rotation":node.rotation,"scale":node.scale,"children":[],"extras":extras})
        })
        .collect::<Vec<_>>();
    let mut roots = Vec::new();
    for (index, node) in model.iter().enumerate() {
        if let Some(parent) = node.parent {
            nodes[parent]["children"]
                .as_array_mut()
                .unwrap()
                .push(json!(index));
        } else {
            roots.push(index);
        }
    }
    let skins = if source_meshes.iter().any(|mesh| mesh.joints.is_some()) {
        let offset = buffer.len();
        for index in 0..model.len() {
            let inverse = bind_transform(index, &model)?.inverse();
            ensure!(inverse.is_finite(), "singular model bind transform");
            for value in inverse.to_cols_array() {
                buffer.extend(value.to_le_bytes());
            }
        }
        views.push(json!({"buffer":0,"byteOffset":offset,"byteLength":model.len()*64}));
        accessors
            .push(json!({"bufferView":0,"componentType":5126,"count":model.len(),"type":"MAT4"}));
        vec![json!({"joints":(0..model.len()).collect::<Vec<_>>(),"inverseBindMatrices":0})]
    } else {
        Vec::new()
    };
    let mut instance_meshes = Vec::new();
    let mut instance_parents = Vec::new();
    let mut instance_detached = Vec::new();
    for mesh in &source_meshes {
        let parents = if mesh.joints.is_some() {
            vec![None]
        } else if model.is_empty() {
            vec![None]
        } else {
            model
                .iter()
                .enumerate()
                .filter_map(|(index, node)| {
                    (usize::from(node.object) == mesh.source).then_some(Some(index))
                })
                .collect::<Vec<_>>()
        };
        // Physical GPLs may carry geometry unused by the authored hierarchy.
        // Keep it editable in the GLB but do not attach it to the scene roots.
        let detached = parents.is_empty();
        let parents = if detached { vec![None] } else { parents };
        let mut extra = Vec::new();
        if !mesh.second_uvs.is_empty() {
            extra.push(("TEXCOORD_1".to_owned(), Attribute::Float2(&mesh.second_uvs)));
        }
        let joints = mesh.joints.as_ref().map(|values| {
            values
                .iter()
                .map(|&joint| [joint, 0, 0, 0])
                .collect::<Vec<_>>()
        });
        let weights = joints
            .as_ref()
            .map(|values| vec![[1., 0., 0., 0.]; values.len()]);
        if let (Some(joints), Some(weights)) = (&joints, &weights) {
            ensure!(
                joints
                    .iter()
                    .all(|joint| usize::from(joint[0]) < model.len()),
                "joint index exceeds model hierarchy"
            );
            extra.push(("JOINTS_0".to_owned(), Attribute::UnsignedShort4(joints)));
            extra.push(("WEIGHTS_0".to_owned(), Attribute::Float4(weights)));
        }
        let gltf_mesh = gltf::append_mesh(
            &mut buffer,
            &mut views,
            &mut accessors,
            gltf::Mesh {
                name: "GPL object",
                positions: &mesh.positions,
                texcoords: &mesh.uvs,
                colors: mesh.colors.as_deref(),
                normals: mesh.normals.as_deref(),
                attributes: &extra,
                indices: &mesh.indices,
                primitives: &mesh.primitives,
            },
        );
        for parent in parents {
            let index = nodes.len();
            if parent.is_none() {
                let mut node = json!({"name":mesh.name,"mesh":meshes.len()});
                if mesh.joints.is_some() {
                    node["skin"] = json!(0);
                }
                nodes.push(node);
                if !detached {
                    roots.push(index);
                }
            } else {
                nodes.push(json!({"name":mesh.name,"mesh":meshes.len()}));
                nodes[parent.context("terrain mesh has no model node")?]["children"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!(index));
            }
            meshes.push(gltf_mesh.clone());
            instance_meshes.push(mesh.clone());
            instance_parents.push(parent);
            instance_detached.push(detached);
        }
    }
    let gltf = json!({"asset":{"version":"2.0","generator":"resonance-importer-vnext"},"scene":0,"scenes":[{"nodes":roots}],"nodes":nodes,"buffers":[{"byteLength":buffer.len()}],"bufferViews":views,"accessors":accessors,"meshes":meshes,"skins":skins});
    Ok((
        gltf::pack_glb(&gltf, &buffer)?,
        instance_meshes,
        model,
        instance_parents,
        instance_detached,
    ))
}

fn decode(
    tile: &[u8],
    layer: usize,
) -> Result<(Vec<u8>, Vec<Mesh>, Vec<Node>, Vec<Option<usize>>, Vec<bool>)> {
    ensure!(
        matches!(layer, 0 | 2),
        "terrain visual layer must be 0 or 2"
    );
    decode_resource(section(tile, layer)?)
}

fn binding(word: u32) -> Result<TextureBinding> {
    let wrap = |value| -> Result<TextureWrap> {
        Ok(match value {
            0 => TextureWrap::Clamp,
            1 => TextureWrap::Repeat,
            2 => TextureWrap::Mirror,
            _ => anyhow::bail!("unsupported terrain texture wrap {value}"),
        })
    };
    let min = (word >> 24) & 15;
    let mag = word >> 28;
    ensure!(
        matches!(min, 0 | 1 | 5) && matches!(mag, 0 | 1),
        "unsupported terrain texture filter min={min} mag={mag}"
    );
    Ok(TextureBinding {
        texture: (word & 0x1fff) as usize,
        wrap_u: wrap((word >> 16) & 15)?,
        wrap_v: wrap((word >> 20) & 15)?,
        nearest_min: min == 0,
        nearest_mag: mag == 0,
        mipmapped: min == 5,
    })
}

fn project(
    resource: &[u8],
    resource_slot: u16,
    depth_write: bool,
    paths: Vec<String>,
    mut alpha: Vec<bool>,
    texture_animations: Vec<resonance_content::TextureAnimation>,
) -> Result<(Vec<u8>, ScenePart)> {
    let (glb, meshes, nodes, parents, detached) = decode_resource(resource)?;
    ensure!(
        paths.len() == alpha.len(),
        "terrain texture metadata mismatch"
    );
    let vertex_blend = |mesh: &Mesh| {
        mesh.colors
            .as_ref()
            .is_some_and(|colors| colors.iter().any(|color| color[3] < 0.999))
    };
    for mesh in &meshes {
        if vertex_blend(mesh) {
            if let Some(image) = mesh.textures.first() {
                if let Some(blend) = alpha.get_mut((image & 0x1fff) as usize) {
                    *blend = true;
                }
            }
        }
    }
    let mut draw_order = vec![0; meshes.len()];
    let mut sorted: Vec<_> = (0..meshes.len()).collect();
    // Skin/root geometry is drawn before the model-node priority queue.
    sorted.sort_by_key(|&index| {
        if detached[index] {
            (2, 0, 0)
        } else {
            parents[index].map_or((0, 0, 0), |parent| (1, nodes[parent].priority, parent))
        }
    });
    for (rank, index) in sorted.into_iter().enumerate() {
        draw_order[index] = rank as u32;
    }
    let materials = meshes
        .iter()
        .enumerate()
        .map(|(index, mesh)| {
            let count = match mesh.mode {
                1 | 4 => 1,
                0x11 | 0x14 => 2,
                5 | 0x15 => 0,
                _ => anyhow::bail!("unsupported terrain material mode {}", mesh.mode),
            };
            let stage = |id| -> Result<TextureBinding> {
                let word = mesh
                    .textures
                    .iter()
                    .find(|word| ((**word >> 13) & 7) as usize == id)
                    .context("missing terrain texture stage")?;
                let binding = binding(*word)?;
                ensure!(binding.texture < paths.len(), "terrain texture exceeds TPL");
                Ok(binding)
            };
            let color = (count != 0).then(|| stage(0)).transpose()?;
            let multiply = (count == 2).then(|| stage(1)).transpose()?;
            Ok(SceneMaterial {
                image: SceneImage::Palette,
                vertex_color: matches!(mesh.mode, 1 | 5 | 0x11 | 0x15),
                blend: color.as_ref().is_some_and(|color| alpha[color.texture])
                    || multiply.is_some()
                    || vertex_blend(mesh)
                    // A color-only draw shares native material slot zero.
                    || (count == 0 && alpha.first().copied().unwrap_or(false)),
                depth_write,
                cull: Default::default(),
                color,
                multiply,
                draw_order: draw_order[index],
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok((
        glb,
        ScenePart {
            resource: resource_slot,
            mesh: String::new(),
            textures: paths,
            materials,
            appearance: None,
            translation: [0.; 3],
            clips: Vec::new(),
            autoplay: false,
            texture_animations,
            bone_names: nodes.into_iter().map(|node| node.name).collect(),
            material_nodes: if meshes.iter().all(|mesh| mesh.joints.is_some()) {
                Vec::new()
            } else {
                parents
                    .into_iter()
                    .zip(&meshes)
                    .map(|(parent, mesh)| match (parent, &mesh.joints) {
                        (Some(parent), None) => vec![parent as u16],
                        _ => vec![],
                    })
                    .collect()
            },
            outline_color: None,
            secondary_motion: Default::default(),
        },
    ))
}

pub(crate) fn visual(
    tile: &[u8],
    layer: usize,
    paths: Vec<String>,
    alpha: Vec<bool>,
) -> Result<(Vec<u8>, ScenePart)> {
    let resource = section(tile, layer)?;
    let texture_animations = if layer == 2 {
        resource
            .get(0x13..0x19)
            .context("terrain texture animation slots missing")?
            .iter()
            .enumerate()
            .filter_map(|(slot, &id)| {
                let texture = usize::from(id.checked_sub(1)?);
                let offsets = if slot < 2 {
                    (0..1000).map(|tick| [tick as f32 * 0.001; 2]).collect()
                } else {
                    (0..160)
                        .map(|tick| [0., (tick / 20) as f32 * 0.125])
                        .collect()
                };
                Some(resonance_content::TextureAnimation {
                    texture,
                    delay_ticks: 0,
                    loop_start: 0,
                    offsets,
                })
            })
            .collect()
    } else {
        Vec::new()
    };
    project(
        resource,
        layer as u16,
        layer == 0,
        paths,
        alpha,
        texture_animations,
    )
}

pub(crate) fn sky(
    resource: &[u8],
    paths: Vec<String>,
    alpha: Vec<bool>,
) -> Result<(Vec<u8>, ScenePart)> {
    project(resource, 0, false, paths, alpha, Vec::new())
}

pub(crate) fn static_model(
    resource: &[u8],
    paths: Vec<String>,
    alpha: Vec<bool>,
) -> Result<(Vec<u8>, ScenePart)> {
    project(resource, 0, true, paths, alpha, Vec::new())
}

/// Palette-free GPL geometry keeps its texture table names in draw descriptors.
/// The caller supplies those tables when the model is rendered.
pub(crate) fn unbound_model(resource: &[u8], mesh_path: &str) -> Result<(Vec<u8>, Value)> {
    let (glb, meshes, nodes, parents, _) = decode_resource(resource)?;
    let mut draws = Vec::new();
    for (index, mesh) in meshes.iter().enumerate() {
        let recipe = match mesh.mode {
            1 => json!([{"multiply_texture":0}]),
            4 => json!([{"replace_texture":0}]),
            other => anyhow::bail!("unbound GPL draw needs material mode {other} recipe"),
        };
        ensure!(
            mesh.textures.len() == 1,
            "unbound GPL draw needs one texture"
        );
        let word = mesh.textures[0];
        ensure!(
            (word >> 13) & 7 == 0,
            "unbound GPL texture stage is not zero"
        );
        let table = mesh
            .texture_tables
            .first()
            .and_then(Option::as_ref)
            .context("unbound GPL draw has no caller texture table")?;
        let mut draw = json!({
            "meshes":[index],
            "recipe":recipe,
            "textures":[{"image":word & 0x1fff,"table":table}]
        });
        if let Some(parent) = parents[index] {
            draw["node"] = json!(nodes[parent].name);
        }
        draws.push(draw);
    }
    Ok((
        glb,
        json!({"mesh":mesh_path,"textures":"caller","draws":draws}),
    ))
}

pub(crate) fn with_primary_palette(primary: &[u8], secondary: &[u8]) -> Result<Vec<u8>> {
    ensure!(
        dol::u32(secondary, 0)? == 0,
        "secondary model owns a palette"
    );
    let start = dol::u32(primary, 0)? as usize;
    let end = dol::u32(primary, 4)? as usize;
    let palette = primary
        .get(start..end)
        .context("primary palette exceeds model")?;
    let insert = dol::u32(secondary, 4)? as usize;
    ensure!(
        insert <= secondary.len(),
        "secondary model boundary exceeds source"
    );
    let mut combined = secondary[..insert].to_vec();
    combined.extend_from_slice(palette);
    combined.extend_from_slice(&secondary[insert..]);
    combined[..4].copy_from_slice(&(insert as u32).to_be_bytes());
    combined[4..8].copy_from_slice(&((insert + palette.len()) as u32).to_be_bytes());
    Ok(combined)
}

pub fn cook_world_terrain_visual(source: &Path, output: &Path, layer: usize) -> Result<()> {
    ensure!(!output.exists(), "world visual output already exists");
    let glb = decode(&fs::read(source)?, layer)?.0;
    crate::output::write(&output, &glb)?;
    Ok(())
}
