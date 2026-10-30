//! Assign one editable material to several meshes. Order defaults to mesh index.
use super::{Draw, MaterialRecipe, Texture};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Material {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    node: Option<String>,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    recipe: MaterialRecipe,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    textures: Vec<Texture>,
}

#[derive(Serialize, Deserialize)]
struct Binding {
    meshes: Vec<Option<usize>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    draw_order: Vec<u32>,
    #[serde(flatten)]
    material: Material,
}

pub fn serialize<S: Serializer>(draws: &[Draw], serializer: S) -> Result<S::Ok, S::Error> {
    let mut bindings: Vec<Binding> = Vec::new();
    for draw in draws {
        let material = Material {
            node: draw.node.clone(),
            recipe: draw.recipe.clone(),
            textures: draw.textures.clone(),
        };
        let index = bindings.iter().position(|b| b.material == material);
        let binding = if let Some(index) = index {
            &mut bindings[index]
        } else {
            bindings.push(Binding {
                meshes: Vec::new(),
                draw_order: Vec::new(),
                material,
            });
            bindings.last_mut().unwrap()
        };
        binding.meshes.push(draw.mesh);
        binding.draw_order.push(draw.draw_order);
    }
    for binding in &mut bindings {
        if binding
            .meshes
            .iter()
            .zip(&binding.draw_order)
            .all(|(&mesh, &order)| mesh == Some(order as usize))
        {
            binding.draw_order.clear();
        }
    }
    bindings.serialize(serializer)
}

pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<Draw>, D::Error> {
    let bindings = Vec::<Binding>::deserialize(deserializer)?;
    let mut draws = Vec::new();
    let mut assigned = std::collections::BTreeSet::new();
    for binding in bindings {
        if binding.meshes.is_empty()
            || (!binding.draw_order.is_empty() && binding.draw_order.len() != binding.meshes.len())
        {
            return Err(D::Error::custom(
                "material needs meshes and one draw order per assignment",
            ));
        }
        for (index, mesh) in binding.meshes.into_iter().enumerate() {
            if mesh.is_some_and(|mesh| !assigned.insert(mesh)) {
                return Err(D::Error::custom(
                    "mesh has more than one material assignment",
                ));
            }
            let order = if let Some(&order) = binding.draw_order.get(index) {
                order
            } else {
                u32::try_from(
                    mesh.ok_or_else(|| D::Error::custom("an empty draw needs explicit order"))?,
                )
                .map_err(D::Error::custom)?
            };
            draws.push(Draw {
                mesh,
                node: binding.material.node.clone(),
                model_node: None,
                draw_order: order,
                preview_blend: false,
                recipe: binding.material.recipe.clone(),
                textures: binding.material.textures.clone(),
            });
        }
    }
    draws.sort_by_key(|draw| (draw.mesh, draw.draw_order));
    Ok(draws)
}
