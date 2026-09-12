//! Compile scenery door tags into standing positions and hinge movements.
use anyhow::{Context, Result, ensure};
use glam::{Mat4, Quat, Vec3};
use resonance_content::field::Door;
use serde_json::Value;

pub(crate) fn cook(gltf: &Value) -> Result<Vec<Door>> {
    let nodes = gltf["nodes"]
        .as_array()
        .context("field nodes are missing")?;
    let mut parents = vec![None; nodes.len()];
    for (i, node) in nodes.iter().enumerate() {
        if let Some(children) = node["children"].as_array() {
            for child in children {
                let child = child.as_u64().context("invalid field child")? as usize;
                ensure!(
                    child < nodes.len() && parents[child].is_none(),
                    "invalid field hierarchy"
                );
                parents[child] = Some(i);
            }
        }
    }
    let mut doors = Vec::new();
    for (i, node) in nodes.iter().enumerate() {
        let Some(name) = node["name"].as_str() else {
            continue;
        };
        // Mesh children repeat the hinge name; only the authored bone owns it.
        if !name.starts_with("DOOR") || !name.contains("_AUTO") || node.get("mesh").is_some() {
            continue;
        }
        let width = name.split_once("_W").map_or(Ok(200.), |(_, value)| {
            value
                .split('_')
                .next()
                .unwrap()
                .replace('e', ".")
                .parse::<f32>()
        })?;
        ensure!(
            width.is_finite() && width.abs() <= 2000.,
            "invalid door width"
        );
        let pull = name.contains("_PULL");
        ensure!(
            pull != name.contains("_PUSH"),
            "door must specify push or pull"
        );
        let mut matrix = Mat4::IDENTITY;
        let mut cursor = Some(i);
        let mut depth = 0;
        while let Some(index) = cursor {
            ensure!(depth < nodes.len(), "cycle in field hierarchy");
            let node = &nodes[index];
            ensure!(
                node.get("matrix").is_none(),
                "expected cooked TRS transform"
            );
            let translation = node
                .get("translation")
                .map(|v| serde_json::from_value(v.clone()))
                .transpose()?
                .unwrap_or([0.; 3]);
            let rotation = node
                .get("rotation")
                .map(|v| serde_json::from_value(v.clone()))
                .transpose()?
                .unwrap_or([0., 0., 0., 1.]);
            let scale = node
                .get("scale")
                .map(|v| serde_json::from_value(v.clone()))
                .transpose()?
                .unwrap_or([1.; 3]);
            matrix = Mat4::from_scale_rotation_translation(
                Vec3::from_array(scale),
                Quat::from_array(rotation),
                Vec3::from_array(translation),
            ) * matrix;
            cursor = parents[index];
            depth += 1;
        }
        let distance = if pull && width < 0. { -45.287 } else { -55.287 };
        let approach = matrix.transform_point3(Vec3::new(-0.7 * width, distance, 0.));
        // Door standing poses use the scaled matrix's first row. Quantization
        // precedes the quarter turn into the character's south-facing convention.
        let heading = (matrix
            .x_axis
            .x
            .atan2(matrix.y_axis.x)
            .mul_add(1_f32.to_degrees(), 180.)
            .trunc()
            - 90.)
            .rem_euclid(360.);
        let angle = if name.contains("_AUTO_L") { 45. } else { 30. }
            * if pull { 1. } else { -1. }
            * if width < 0. { -1. } else { 1. };
        ensure!(
            matrix.is_finite() && approach.is_finite(),
            "invalid door transform"
        );
        doors.push(Door {
            bone: name.into(),
            position: matrix.transform_point3(Vec3::ZERO).to_array(),
            approach: approach.to_array(),
            heading,
            pull,
            angle,
        });
    }
    Ok(doors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parent_transform_and_pull_direction_survive_cooking_without_duplicate_mesh_hinges() {
        let name = "DOOR01_AUTO_L_PULL_W-100";
        let mut gltf = json!({"nodes":[
            {"translation":[10.,20.,3.],"children":[1]},
            {"name":name,"children":[2]}, {"name":name,"mesh":0}
        ]});
        let doors = cook(&gltf).unwrap();
        assert_eq!(doors.len(), 1);
        assert_eq!(doors[0].position, [10., 20., 3.]);
        assert!(
            Vec3::from_array(doors[0].approach).distance(Vec3::new(80., -25.287, 3.)) < 0.00001
        );
        assert_eq!(doors[0].heading, 180.);
        assert!(doors[0].pull);
        assert_eq!(doors[0].angle, -45.);
        gltf["nodes"][0]["translation"] = json!([10., 20.]);
        assert!(
            cook(&gltf).is_err(),
            "malformed transforms must not become identity"
        );
        gltf["nodes"][0]["translation"] = json!([10., 20., 3.]);
        gltf["nodes"][2]["children"] = json!([0]);
        assert!(cook(&gltf).is_err(), "cyclic hinges must not hang cooking");
    }

    #[test]
    fn scaled_genis_door_keeps_its_observed_standing_heading() {
        let door = cook(&json!({"nodes":[{
            "name":"DOOR01_AUTO_S_PUSH_W175e82",
            "rotation":[0.,0.,0.2206106185913086,0.975361943244934],
            "scale":[0.949999988079071,1.,1.],
            "translation":[-2853.958984375,1610.7022705078125,192.68101501464844]
        }]}))
        .unwrap()
        .remove(0);
        assert_eq!(door.heading, 206.);
        assert!((door.approach[0] + 2935.7058).abs() < 0.001);
        assert!((door.approach[1] - 1510.4801).abs() < 0.001);
        assert_eq!(door.angle, -30.);
    }
}
