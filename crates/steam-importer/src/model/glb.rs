use super::super::coordinates;
use super::*;
use resonance_asset_writer::gltf::{self, Attribute};
use resonance_content::animation::Transform;
use serde_json::{Value, json};

impl Model {
    pub(crate) fn outline_glb(&self) -> Result<Vec<u8>> {
        self.glb_with_expansion(&vec![[1.; 4]; self.materials.records.len()], 0.5)
    }

    pub(crate) fn glb(&self, tints: &[[f32; 4]]) -> Result<Vec<u8>> {
        self.glb_with_expansion(tints, 0.)
    }

    fn glb_with_expansion(&self, tints: &[[f32; 4]], expansion: f32) -> Result<Vec<u8>> {
        ensure!(
            tints.len() == self.materials.records.len(),
            "missing material tint"
        );
        let names = self.rig.joint_names();
        let mut nodes = self
            .rig
            .nodes
            .iter()
            .enumerate()
            .map(|(i, _)| {
                json!({
                    "name": names[i], "extras": {"resonance_bone": i}, "children": []
                })
            })
            .collect::<Vec<_>>();
        for (i, node) in self.rig.nodes.iter().enumerate() {
            // The pose buffer retains authored rotations even on collapsed
            // axes, and the original signs of mirrored scales.
            apply(&mut nodes[i], coordinates::transform(node.rest));
            if !node.visible {
                nodes[i]["extensions"] = json!({"KHR_node_visibility":{"visible":false}});
            }
        }
        let mut roots = Vec::new();
        for (i, node) in self.rig.nodes.iter().enumerate() {
            if let Some(parent) = node.parent {
                nodes[usize::from(parent)]["children"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!(i));
            } else {
                roots.push(i);
            }
        }
        let mut binary = Vec::new();
        let mut views = Vec::new();
        let mut accessors = Vec::new();
        let mut skins = Vec::new();
        if self.meshes.iter().any(|mesh| mesh.joints.is_some()) {
            let offset = gltf::append_floats(
                &mut binary,
                self.rig
                    .nodes
                    .iter()
                    .flat_map(|n| coordinates::matrix(n.inverse_bind)),
            );
            views
                .push(json!({"buffer":0,"byteOffset":offset,"byteLength":self.rig.nodes.len()*64}));
            accessors.push(
                json!({"bufferView":0,"componentType":5126,"count":self.rig.nodes.len(),"type":"MAT4"}),
            );
            skins.push(
                json!({"joints":(0..self.rig.nodes.len()).collect::<Vec<_>>(),"inverseBindMatrices":0}),
            );
        }
        let mut meshes = Vec::new();
        for mesh in &self.meshes {
            let tint = tints[usize::from(mesh.material)];
            let colors = mesh
                .colors
                .iter()
                .map(|color| std::array::from_fn(|i| color[i] * tint[i]))
                .collect::<Vec<[f32; 4]>>();
            let positions = mesh
                .positions
                .iter()
                .zip(&mesh.normals)
                .map(|(position, normal)| {
                    coordinates::vector(
                        (glam::Vec3::from_array(*position)
                            + glam::Vec3::from_array(*normal).normalize_or_zero() * expansion)
                            .to_array(),
                    )
                })
                .collect::<Vec<_>>();
            let normals = mesh
                .normals
                .iter()
                .copied()
                .map(coordinates::vector)
                .collect::<Vec<_>>();
            let indices = coordinates::triangles(&mesh.indices);
            let mut attributes = mesh
                .texcoords
                .iter()
                .enumerate()
                .skip(1)
                .map(|(i, uv)| (format!("TEXCOORD_{i}"), Attribute::Float2(uv)))
                .collect::<Vec<_>>();
            if let (Some(joints), Some(weights)) = (&mesh.joints, &mesh.weights) {
                attributes.push(("JOINTS_0".into(), Attribute::UnsignedShort4(joints)));
                attributes.push(("WEIGHTS_0".into(), Attribute::Float4(weights)));
            }
            let untextured = vec![[0.; 2]; mesh.positions.len()];
            let mut node = json!({"name":mesh.name});
            if !mesh.indices.is_empty() {
                node["mesh"] = json!(meshes.len());
                meshes.push(gltf::append_mesh(
                    &mut binary,
                    &mut views,
                    &mut accessors,
                    gltf::Mesh {
                        name: &mesh.name,
                        positions: &positions,
                        texcoords: mesh.texcoords.first().unwrap_or(&untextured),
                        colors: Some(&colors),
                        normals: Some(&normals),
                        attributes: &attributes,
                        indices: &indices,
                        primitives: &[(4, 0..mesh.indices.len())],
                    },
                ));
                if mesh.joints.is_some() {
                    node["skin"] = json!(0);
                }
            }
            let mesh_node = nodes.len();
            if let Some(parent) = mesh.node {
                nodes[usize::from(parent)]["children"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!(mesh_node));
            } else {
                roots.push(nodes.len());
            }
            nodes.push(node);
        }
        let mut document = json!({"asset":{"version":"2.0","generator":"resonance-import"},
            "scene":0,"scenes":[{"nodes":roots}],"nodes":nodes,"meshes":meshes,"skins":skins,
            "buffers":[{"byteLength":binary.len()}],"bufferViews":views,"accessors":accessors});
        if self.rig.nodes.iter().any(|node| !node.visible) {
            document["extensionsUsed"] = json!(["KHR_node_visibility"]);
        }
        gltf::pack_glb(&document, &binary)
    }
}

fn apply(node: &mut Value, transform: Transform) {
    node["translation"] = json!(transform.translation);
    node["rotation"] = json!(transform.rotation);
    node["scale"] = json!(transform.scale);
}
