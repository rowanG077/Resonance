//! Door settings live on their GLB hinge; positions follow the current model.
use super::Door;
use crate::animation::{node_parents, rest_transform};
use anyhow::{Context, Result, ensure};
use glam::{Mat4, Quat, Vec3};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Action {
    Push,
    Pull,
}
#[derive(Deserialize)]
struct Properties {
    door_approach: [f32; 3],
    door_action: Action,
    door_angle_degrees: f32,
}

pub fn decode(bytes: &[u8], translation: [f32; 3]) -> Result<Vec<Door>> {
    let glb = gltf::binary::Glb::from_slice(bytes).context("read scenery GLB")?;
    from_gltf(&serde_json::from_slice(&glb.json)?, translation)
}

pub fn from_gltf(gltf: &Value, translation: [f32; 3]) -> Result<Vec<Door>> {
    let nodes = gltf["nodes"]
        .as_array()
        .context("scenery GLB lacks nodes")?;
    let mut doors = BTreeMap::new();
    let mut parents = None;
    for (index, node) in nodes.iter().enumerate() {
        let extras = &node["extras"];
        if !["door_action", "door_approach", "door_angle_degrees"]
            .iter()
            .any(|key| extras.get(key).is_some())
        {
            continue;
        }
        let settings: Properties = serde_json::from_value(extras.clone())
            .with_context(|| format!("door properties on node {index}"))?;
        ensure!(
            settings.door_approach.into_iter().all(f32::is_finite)
                && (1. ..=180.).contains(&settings.door_angle_degrees.abs()),
            "invalid door approach or opening angle"
        );
        let bone: u16 = extras["resonance_bone"]
            .as_u64()
            .context("door hinge is not an indexed model node")?
            .try_into()?;
        if parents.is_none() {
            parents = Some(node_parents(nodes)?);
        }
        let parents = parents.as_ref().unwrap();
        let mut matrix = Mat4::IDENTITY;
        let mut cursor = Some(index);
        let mut depth = 0;
        while let Some(index) = cursor {
            ensure!(depth < nodes.len(), "cyclic door hierarchy");
            let transform = rest_transform(&nodes[index])?;
            // Preserve the established scenery-door arithmetic, including encoded
            // quaternion precision and the scaled first row used for facing.
            matrix = Mat4::from_scale_rotation_translation(
                Vec3::from_array(transform.scale),
                Quat::from_array(transform.rotation),
                Vec3::from_array(transform.translation),
            ) * matrix;
            cursor = parents[index];
            depth += 1;
        }
        let offset = Vec3::from_array(translation);
        let position = matrix.transform_point3(Vec3::ZERO) + offset;
        let approach = matrix.transform_point3(Vec3::from_array(settings.door_approach)) + offset;
        ensure!(
            matrix.is_finite() && position.is_finite() && approach.is_finite(),
            "invalid door transform"
        );
        let heading = (matrix
            .x_axis
            .x
            .atan2(matrix.y_axis.x)
            .mul_add(1_f32.to_degrees(), 180.)
            .trunc()
            - 90.)
            .rem_euclid(360.);
        ensure!(
            doors
                .insert(
                    bone,
                    Door {
                        bone,
                        position: position.to_array(),
                        approach: approach.to_array(),
                        heading,
                        pull: matches!(settings.door_action, Action::Pull),
                        angle: settings.door_angle_degrees,
                    }
                )
                .is_none(),
            "duplicate door hinge index"
        );
    }
    // Editor node sorting must not change equal-distance interaction priority.
    Ok(doors.into_values().collect())
}

#[cfg(test)]
mod tests;
