//! A material is edited once and assigned to a set of GLB mesh indices.
//! Rendering still receives one material per mesh, in mesh order.
use crate::SceneMaterial;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};

#[derive(Serialize, Deserialize)]
struct Binding {
    meshes: Vec<u32>,
    /// Omitted when draw order equals the corresponding mesh index.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    draw_order: Vec<u32>,
    #[serde(flatten)]
    surface: SceneMaterial<()>,
}

pub fn serialize<S: Serializer>(
    materials: &[SceneMaterial],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let mut bindings: Vec<Binding> = Vec::new();
    for (mesh, material) in materials.iter().enumerate() {
        let surface = material.clone().with_draw_order(());
        let index = bindings
            .iter()
            .position(|binding| binding.surface == surface);
        let binding = if let Some(index) = index {
            &mut bindings[index]
        } else {
            bindings.push(Binding {
                meshes: Vec::new(),
                draw_order: Vec::new(),
                surface,
            });
            bindings.last_mut().unwrap()
        };
        binding
            .meshes
            .push(u32::try_from(mesh).map_err(serde::ser::Error::custom)?);
        binding.draw_order.push(material.draw_order);
    }
    for binding in &mut bindings {
        if binding.draw_order == binding.meshes {
            binding.draw_order.clear();
        }
    }
    bindings.serialize(serializer)
}

pub fn deserialize<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<SceneMaterial>, D::Error> {
    let bindings = Vec::<Binding>::deserialize(deserializer)?;
    // Allocate from the number of declared assignments, never from an arbitrary
    // mesh index. A missing, duplicate or out-of-range assignment is an error.
    let count = bindings.iter().map(|binding| binding.meshes.len()).sum();
    let mut materials = vec![None; count];
    for binding in bindings {
        if binding.meshes.is_empty()
            || (!binding.draw_order.is_empty() && binding.draw_order.len() != binding.meshes.len())
        {
            return Err(D::Error::custom(
                "material needs meshes and one draw order per mesh",
            ));
        }
        for (index, mesh) in binding.meshes.into_iter().enumerate() {
            let target = materials.get_mut(mesh as usize).ok_or_else(|| {
                D::Error::custom("material mesh assignments must cover 0..mesh_count")
            })?;
            if target.is_some() {
                return Err(D::Error::custom(
                    "mesh has more than one material assignment",
                ));
            }
            *target = Some(
                binding
                    .surface
                    .clone()
                    .with_draw_order(binding.draw_order.get(index).copied().unwrap_or(mesh)),
            );
        }
    }
    materials
        .into_iter()
        .map(|material| material.ok_or_else(|| D::Error::custom("mesh has no material")))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[derive(Serialize, Deserialize)]
    struct Part {
        #[serde(with = "super")]
        materials: Vec<SceneMaterial>,
    }

    #[test]
    fn grouped_edits_preserve_mesh_order_draw_priority_and_sampler_exceptions() {
        let input = json!({"materials": [
            {"meshes":[0,2,3],"draw_order":[65539,65536,65540],"color":{"texture":1}},
            {"meshes":[1],"image":"capture","vertex_color":false,"blend":true,
             "depth_write":false,"cull":"none"},
            {"meshes":[4],"color":{"texture":2,"wrap_u":"clamp","wrap_v":"mirror",
             "nearest_min":true,"nearest_mag":true},"multiply":{"texture":0},"cull":"front"}
        ]});
        let part: Part = serde_json::from_value(input.clone()).unwrap();
        assert_eq!(
            part.materials
                .iter()
                .map(|m| m.draw_order)
                .collect::<Vec<_>>(),
            [65539, 1, 65536, 65540, 4]
        );
        assert_eq!(serde_json::to_value(&part).unwrap(), input);
        let mut edit = input;
        edit["materials"][0]["color"]["texture"] = json!(7);
        let changed: Part = serde_json::from_value(edit).unwrap();
        for mesh in [0, 2, 3] {
            assert_eq!(changed.materials[mesh].color.as_ref().unwrap().texture, 7);
        }
        assert_eq!(changed.materials[1], part.materials[1]);
        assert_eq!(changed.materials[4], part.materials[4]);
        let roundtrip: Part =
            serde_json::from_value(serde_json::to_value(&changed).unwrap()).unwrap();
        assert_eq!(roundtrip.materials, changed.materials);
    }

    #[test]
    fn missing_duplicate_and_invalid_mesh_bindings_are_rejected() {
        for bindings in [
            json!([{"meshes":[]}]),
            json!([{"meshes":[0,0]}]),
            json!([{"meshes":[1]}]),
            json!([{"meshes":[0,1],"draw_order":[4]}]),
            json!([{"meshes":[4294967295_u32]}]),
            json!([{"color":{"texture":0},"draw_order":0}]),
        ] {
            assert!(serde_json::from_value::<Part>(json!({"materials":bindings})).is_err());
        }
    }
}
