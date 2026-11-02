use super::*;
use std::collections::BTreeMap;

pub struct Mesh {
    pub node: Option<u16>,
    pub flags: u16,
    pub material: u16,
    pub name: String,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub colors: Vec<[f32; 4]>,
    pub texcoords: Vec<Vec<[f32; 2]>>,
    pub joints: Option<Vec<[u16; 4]>>,
    pub weights: Option<Vec<[f32; 4]>>,
    pub indices: Vec<u32>,
}

pub(super) fn read(
    source: &Reader<'_>,
    section: usize,
    payload: &[u8],
    nodes: &[Node],
    materials: usize,
) -> Result<Vec<Mesh>> {
    let rows = source.table(section + 24, 28)?;
    let palette = source.table(section + 32, 4)?;
    let node_ids = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.binding, i as u16))
        .collect::<BTreeMap<_, _>>();
    let joints = (0..palette.count)
        .map(|i| {
            let id = source.word(palette.at(i))?;
            node_ids
                .get(&id)
                .copied()
                .with_context(|| format!("Steam skin requires external node {id}"))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut meshes = Vec::new();
    for i in 0..rows.count {
        let at = rows.at(i);
        let node = source.signed(at + 6)?;
        ensure!(
            node == -1 || (node >= 0 && (node as usize) < nodes.len()),
            "unknown Steam draw node"
        );
        let flags = source.half(at + 8)?;
        let material = source.half(at + 10)?;
        ensure!(
            (material as usize) < materials,
            "Steam mesh has no material"
        );
        ensure!(source.word(at + 12)? == 0, "unsupported Steam mesh mode");
        let mut mesh = Mesh {
            node: (node >= 0).then_some(node as u16),
            flags,
            material,
            name: source.name(at + 16)?,
            positions: Vec::new(),
            normals: Vec::new(),
            colors: Vec::new(),
            texcoords: Vec::new(),
            joints: None,
            weights: None,
            indices: Vec::new(),
        };
        let data = source.slice(source.relative(at + 20)?, source.word(at + 24)? as usize)?;
        decode(&mut mesh, data, payload, &joints)
            .with_context(|| format!("Steam mesh {}", mesh.name))?;
        meshes.push(mesh);
    }
    Ok(meshes)
}

fn decode(mesh: &mut Mesh, bytes: &[u8], payload: &[u8], palette: &[u16]) -> Result<()> {
    let data = Reader(bytes);
    let payload = Reader(payload);
    let vertices = usize::from(data.half(0)?);
    let index_count = usize::from(data.half(2)?);
    let attributes = data.word(4)? as usize;
    let indices = data.word(8)? as usize;
    let uv_count = usize::from(mesh.flags & 15);
    let kind = mesh.flags & !15;
    ensure!(
        uv_count <= 8 && matches!(kind, 0 | 0x50),
        "unsupported Steam vertex layout {:#x}",
        mesh.flags
    );
    mesh.texcoords.resize_with(uv_count, Vec::new);
    let color_start;
    let stride;
    if kind == 0x50 {
        let counts = (0..4)
            .map(|i| data.word(12 + i * 4).map(|v| v as usize))
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            counts.iter().sum::<usize>() == vertices,
            "invalid Steam skin weight groups"
        );
        stride = 4 + uv_count * 8;
        color_start = attributes;
        ensure!(
            attributes.checked_add(vertices * stride) == Some(indices),
            "inconsistent Steam skin attribute stride"
        );
        let mut at = 28;
        let mut joints = Vec::new();
        let mut weights = Vec::new();
        for (extra, count) in counts.into_iter().enumerate() {
            for _ in 0..count {
                mesh.positions.push(data.floats(at)?);
                mesh.normals.push(data.floats(at + 12)?);
                let raw = data.slice(at + 24, 4)?;
                let mut joint = [0; 4];
                let mut weight = [0.; 4];
                for i in 0..=extra {
                    joint[i] = *palette
                        .get(usize::from(raw[i]))
                        .context("Steam skin index exceeds palette")?;
                    weight[i] = if i < extra {
                        data.floats::<1>(at + 28 + i * 4)?[0]
                    } else {
                        1. - weight[..i].iter().sum::<f32>()
                    };
                    ensure!((0. ..=1.).contains(&weight[i]), "invalid Steam skin weight");
                }
                joints.push(joint);
                weights.push(weight);
                at += 28 + extra * 4;
            }
        }
        ensure!(at == bytes.len(), "unexpected Steam skin data extent");
        mesh.joints = Some(joints);
        mesh.weights = Some(weights);
    } else {
        ensure!(
            bytes.len() == 20 && data.word(12)? == 0 && data.word(16)? == 0,
            "unsupported Steam rigid vertex header"
        );
        stride = 28 + uv_count * 8;
        color_start = attributes + 24;
        payload.slice(attributes, vertices * stride)?;
        for i in 0..vertices {
            mesh.positions
                .push(payload.floats(attributes + i * stride)?);
            mesh.normals
                .push(payload.floats(attributes + i * stride + 12)?);
        }
    }
    for i in 0..vertices {
        let at = color_start + i * stride;
        // Native 0x5d6500 binds this payload range; 0x5d37fa uploads it with
        // memcpy. The declaration at 0x5d6396 uses UBYTE4N (8), whose bytes
        // remain in RGBA order, rather than the packed D3DCOLOR layout.
        let rgba: [u8; 4] = payload.slice(at, 4)?.try_into()?;
        mesh.colors.push(rgba.map(|v| f32::from(v) / 255.));
        for (uv, values) in mesh.texcoords.iter_mut().enumerate() {
            values.push(payload.floats(at + 4 + uv * 8)?);
        }
    }
    let indices = payload
        .slice(indices, index_count * 2)?
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes(b.try_into().unwrap()));
    mesh.indices = triangles(indices, vertices)?;
    Ok(())
}

fn triangles(indices: impl IntoIterator<Item = u16>, vertices: usize) -> Result<Vec<u32>> {
    let mut previous = [0; 2];
    let mut count = 0;
    let mut result = Vec::new();
    for index in indices {
        if index == u16::MAX {
            count = 0;
            continue;
        }
        ensure!(
            usize::from(index) < vertices,
            "Steam triangle index exceeds vertices"
        );
        if count >= 2 && previous[0] != previous[1] && !previous.contains(&index) {
            let mut triangle = [previous[0], previous[1], index];
            if count % 2 == 1 {
                triangle.swap(1, 2);
            }
            result.extend(triangle.map(u32::from));
        }
        previous = [previous[1], index];
        count += 1;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mesh(flags: u16) -> Mesh {
        Mesh {
            node: None,
            flags,
            material: 0,
            name: "fixture".into(),
            positions: Vec::new(),
            normals: Vec::new(),
            colors: Vec::new(),
            texcoords: Vec::new(),
            joints: None,
            weights: None,
            indices: Vec::new(),
        }
    }

    fn floats(bytes: &mut Vec<u8>, values: &[f32]) {
        bytes.extend(values.iter().flat_map(|v| v.to_le_bytes()));
    }

    #[test]
    fn skin_groups_remap_palette_joints_and_preserve_two_uv_sets() -> Result<()> {
        let mut bytes = vec![0; 28];
        bytes[..2].copy_from_slice(&3u16.to_le_bytes());
        bytes[2..4].copy_from_slice(&3u16.to_le_bytes());
        for (at, value) in [(8, 60u32), (12, 1), (16, 1), (24, 1)] {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        for extra in [0, 1, 3] {
            floats(&mut bytes, &[1., 2., 3., 0., 1., 0.]);
            bytes.extend([2, 1, 0, 3]);
            floats(&mut bytes, &[0.125, 0.25, 0.5][..extra]);
        }
        let mut payload = Vec::new();
        for i in 0..3 {
            payload.extend([10, 20, 30, 40]);
            floats(&mut payload, &[i as f32, 2., -3., 4.]);
        }
        payload.extend([0u16, 1, 2].into_iter().flat_map(u16::to_le_bytes));
        let mut decoded = mesh(0x52);
        decode(&mut decoded, &bytes, &payload, &[20, 4, 16, 8])?;
        assert_eq!(
            decoded.joints.as_ref().unwrap(),
            &[[16, 0, 0, 0], [16, 4, 0, 0], [16, 4, 20, 8]]
        );
        assert_eq!(
            decoded.weights.as_ref().unwrap(),
            &[
                [1., 0., 0., 0.],
                [0.125, 0.875, 0., 0.],
                [0.125, 0.25, 0.5, 0.125]
            ]
        );
        assert_eq!(decoded.texcoords[0], [[0., 2.], [1., 2.], [2., 2.]]);
        assert_eq!(decoded.texcoords[1], [[-3., 4.]; 3]);
        assert_eq!(
            decoded.colors,
            vec![[10., 20., 30., 40.].map(|v| v / 255.); 3]
        );
        assert_eq!(decoded.indices, [0, 1, 2]);
        assert!(
            decode(
                &mut mesh(0x52),
                &bytes,
                &payload[..payload.len() - 1],
                &[20, 4, 16, 8]
            )
            .is_err()
        );
        assert!(decode(&mut mesh(0x52), &bytes, &payload, &[20, 4]).is_err());
        assert!(decode(&mut mesh(0x53), &bytes, &payload, &[20, 4, 16, 8]).is_err());
        let mut invalid = bytes.clone();
        invalid[84..88].copy_from_slice(&2f32.to_le_bytes());
        assert!(decode(&mut mesh(0x52), &invalid, &payload, &[20, 4, 16, 8]).is_err());
        for length in 0..bytes.len() {
            assert!(decode(&mut mesh(0x52), &bytes[..length], &payload, &[20, 4, 16, 8]).is_err());
        }
        Ok(())
    }

    #[test]
    fn rigid_vertices_keep_local_positions_normals_colors_and_uvs() -> Result<()> {
        let mut bytes = vec![0; 20];
        bytes[..2].copy_from_slice(&3u16.to_le_bytes());
        bytes[2..4].copy_from_slice(&3u16.to_le_bytes());
        bytes[8..12].copy_from_slice(&108u32.to_le_bytes());
        let mut payload = Vec::new();
        for i in 0..3 {
            floats(&mut payload, &[i as f32, 2., 3., 0., 0., 1.]);
            payload.extend([10, 20, 30, 0]);
            floats(&mut payload, &[0.5, 1.]);
        }
        payload.extend([0u16, 1, 2].into_iter().flat_map(u16::to_le_bytes));
        let mut decoded = mesh(1);
        decode(&mut decoded, &bytes, &payload, &[])?;
        assert_eq!(
            decoded.positions,
            [[0., 2., 3.], [1., 2., 3.], [2., 2., 3.]]
        );
        assert_eq!(decoded.normals, [[0., 0., 1.]; 3]);
        assert_eq!(decoded.texcoords[0], [[0.5, 1.]; 3]);
        assert_eq!(
            decoded.colors,
            vec![[10., 20., 30., 0.].map(|v| v / 255.); 3]
        );
        assert!(decoded.joints.is_none());
        assert!(decode(&mut mesh(0x71), &bytes, &payload, &[]).is_err());
        payload[..4].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(decode(&mut mesh(1), &bytes, &payload, &[]).is_err());
        Ok(())
    }

    #[test]
    fn triangle_strips_reset_parity_at_restart_and_keep_degenerate_parity() -> Result<()> {
        assert_eq!(
            triangles([0, 1, 2, 3, 3, 4, 5, u16::MAX, 6, 7, 8, 9], 10)?,
            [0, 1, 2, 1, 3, 2, 3, 4, 5, 6, 7, 8, 7, 9, 8]
        );
        assert!(triangles([0, 1, 10], 10).is_err());
        Ok(())
    }
}
