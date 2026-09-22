//! glTF buffer/accessor construction and binary container encoding.
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::ops::Range;

const FLOAT: u32 = 5126;
const VERTEX_BUFFER: u32 = 34962;
const INDEX_BUFFER: u32 = 34963;

/// Additional standard or application-defined vertex attributes, in buffer order.
pub enum Attribute<'a> {
    Float2(&'a [[f32; 2]]),
    Float3(&'a [[f32; 3]]),
    Float4(&'a [[f32; 4]]),
    UnsignedShort4(&'a [[u16; 4]]),
}

pub struct Mesh<'a> {
    pub name: &'a str,
    pub positions: &'a [[f32; 3]],
    pub texcoords: &'a [[f32; 2]],
    pub colors: Option<&'a [[f32; 4]]>,
    pub normals: Option<&'a [[f32; 3]]>,
    pub attributes: &'a [(String, Attribute<'a>)],
    pub indices: &'a [u32],
    /// glTF primitive mode and its range within `indices`; empty lists stay empty.
    pub primitives: &'a [(u8, Range<usize>)],
}

pub fn append_mesh(
    buffer: &mut Vec<u8>,
    views: &mut Vec<Value>,
    accessors: &mut Vec<Value>,
    mesh: Mesh<'_>,
) -> Value {
    let position_offset = append_floats(buffer, mesh.positions.iter().flatten().copied());
    let texcoord_offset = append_floats(buffer, mesh.texcoords.iter().flatten().copied());
    let color = mesh
        .colors
        .map(|data| float_attribute(buffer, views, accessors, data));
    let normal = mesh
        .normals
        .map(|data| float_attribute(buffer, views, accessors, data));
    align4(buffer);
    let index_offset = buffer.len();
    buffer.extend(mesh.indices.iter().flat_map(|index| index.to_le_bytes()));
    let view_base = views.len();
    views.push(json!({"buffer":0,"byteOffset":position_offset,"byteLength":mesh.positions.len()*12,"target":VERTEX_BUFFER}));
    views.push(json!({"buffer":0,"byteOffset":texcoord_offset,"byteLength":mesh.texcoords.len()*8,"target":VERTEX_BUFFER}));
    views.push(json!({"buffer":0,"byteOffset":index_offset,"byteLength":mesh.indices.len()*4,"target":INDEX_BUFFER}));
    let accessor_base = accessors.len();
    let (min, max) = bounds(mesh.positions);
    accessors.push(json!({"bufferView":view_base,"componentType":FLOAT,"count":mesh.positions.len(),"type":"VEC3","min":min,"max":max}));
    accessors.push(json!({"bufferView":view_base+1,"componentType":FLOAT,"count":mesh.texcoords.len(),"type":"VEC2"}));
    let mut attributes = json!({"POSITION":accessor_base,"TEXCOORD_0":accessor_base+1});
    for (name, accessor) in [("COLOR_0", color), ("NORMAL", normal)] {
        if let Some(accessor) = accessor {
            attributes[name] = json!(accessor);
        }
    }
    for (name, data) in mesh.attributes {
        let accessor = match data {
            Attribute::Float2(data) => float_attribute(buffer, views, accessors, data),
            Attribute::Float3(data) => float_attribute(buffer, views, accessors, data),
            Attribute::Float4(data) => float_attribute(buffer, views, accessors, data),
            Attribute::UnsignedShort4(data) => {
                align4(buffer);
                let offset = buffer.len();
                buffer.extend(data.iter().flatten().flat_map(|value| value.to_le_bytes()));
                let view = views.len();
                views.push(json!({"buffer":0,"byteOffset":offset,"byteLength":std::mem::size_of_val(*data),"target":VERTEX_BUFFER}));
                let accessor = accessors.len();
                accessors.push(json!({"bufferView":view,"componentType":5123,"count":data.len(),"type":"VEC4"}));
                accessor
            }
        };
        attributes[name] = json!(accessor);
    }
    let primitives = mesh.primitives.iter().map(|(mode, range)| {
        let indices = accessors.len();
        accessors.push(json!({"bufferView":view_base+2,"byteOffset":range.start*4,"componentType":5125,"count":range.len(),"type":"SCALAR"}));
        json!({"mode":mode,"attributes":attributes,"indices":indices})
    }).collect::<Vec<_>>();
    json!({"name":mesh.name,"primitives":primitives})
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
        ([0.; 3], [0.; 3])
    } else {
        (min, max)
    }
}

pub fn pack_glb(gltf: &Value, binary: &[u8]) -> Result<Vec<u8>> {
    let mut json = serde_json::to_vec(gltf)?;
    while json.len() % 4 != 0 {
        json.push(b' ');
    }
    let binary_length = binary.len().next_multiple_of(4);
    let length = 12 + 8 + json.len() + 8 + binary_length;
    ensure!(
        length <= u32::MAX as usize,
        "GLB exceeds its 32-bit size limit"
    );
    let mut glb = Vec::with_capacity(length);
    for value in [0x46546c67, 2, length as u32, json.len() as u32, 0x4e4f534a] {
        glb.extend(value.to_le_bytes());
    }
    glb.extend(json);
    for value in [binary_length as u32, 0x004e4942] {
        glb.extend(value.to_le_bytes());
    }
    glb.extend_from_slice(binary);
    glb.resize(length, 0);
    Ok(glb)
}

pub fn align4(data: &mut Vec<u8>) {
    while !data.len().is_multiple_of(4) {
        data.push(0);
    }
}

pub fn append_floats(buffer: &mut Vec<u8>, values: impl IntoIterator<Item = f32>) -> usize {
    align4(buffer);
    let offset = buffer.len();
    buffer.extend(values.into_iter().flat_map(f32::to_le_bytes));
    offset
}

pub fn float_attribute<const N: usize>(
    buffer: &mut Vec<u8>,
    views: &mut Vec<Value>,
    accessors: &mut Vec<Value>,
    data: &[[f32; N]],
) -> usize {
    let offset = append_floats(buffer, data.iter().flatten().copied());
    let view = views.len();
    views.push(json!({"buffer":0,"byteOffset":offset,"byteLength":std::mem::size_of_val(data),"target":VERTEX_BUFFER}));
    let accessor = accessors.len();
    accessors.push(json!({"bufferView":view,"componentType":FLOAT,"count":data.len(),"type":format!("VEC{N}")}));
    accessor
}
