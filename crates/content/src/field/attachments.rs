//! Gameplay attachment inputs shared by field preparation and save identity.
//! GLB materials and vertex payloads are not gameplay; indexed bones and the
//! rigid bounds used by block interaction are.
use crate::{
    ScenePart,
    animation::{Motion, Skeleton},
};
use anyhow::{Context, Result, ensure};
use std::{collections::BTreeMap, sync::Arc};

pub struct Inputs {
    pub skeleton: Arc<Skeleton>,
    pub mesh_bounds: Option<[[f32; 3]; 2]>,
    pub clips: BTreeMap<(Option<u32>, u16), Arc<Motion>>,
}

pub fn read(
    part: &ScenePart,
    mut read: impl FnMut(&str) -> Result<Arc<[u8]>>,
) -> Result<Option<Inputs>> {
    if part.bone_names.is_empty() {
        return Ok(None);
    }
    let (skeleton, mesh_bounds) = Skeleton::from_glb_with_bounds(&read(&part.mesh)?)
        .with_context(|| format!("field attachment geometry {}", part.mesh))?;
    ensure!(
        skeleton
            .bones
            .iter()
            .map(|bone| &bone.name)
            .eq(&part.bone_names),
        "attachment skeleton differs from scene in {}",
        part.mesh
    );
    let skeleton = Arc::new(skeleton);
    let mut clips = BTreeMap::new();
    for clip in &part.clips {
        let motion = Motion::decode(&read(&clip.motion)?)
            .with_context(|| format!("field attachment motion {}", clip.motion))?;
        motion.validate(&skeleton)?;
        ensure!(
            clips
                .insert(
                    (clip.animation_resource, clip.resource_slot),
                    Arc::new(motion)
                )
                .is_none(),
            "duplicate field attachment motion slot in {}",
            part.mesh
        );
    }
    Ok(Some(Inputs {
        skeleton,
        mesh_bounds,
        clips,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn geometry(value: &Value) -> Vec<u8> {
        let mut json = serde_json::to_vec(value).unwrap();
        while !json.len().is_multiple_of(4) {
            json.push(b' ');
        }
        let mut bytes = b"glTF".to_vec();
        bytes.extend(2u32.to_le_bytes());
        bytes.extend((20u32 + json.len() as u32).to_le_bytes());
        bytes.extend((json.len() as u32).to_le_bytes());
        bytes.extend(b"JSON");
        bytes.extend(json);
        bytes
    }

    fn inputs(geometry: &Value, duration: f32) -> Result<Inputs> {
        let part: ScenePart = serde_json::from_value(json!({
            "resource":0, "mesh":"body.glb", "textures":["unneeded.png"],
            "materials":[], "translation":[0.,0.,0.], "autoplay":false,
            "texture_animations":[], "bone_names":["root"],
            "clips":[{"motion":"walk.motion.json", "resource_slot":3}]
        }))?;
        let mesh: Arc<[u8]> = self::geometry(geometry).into();
        let motion: Arc<[u8]> = Motion {
            duration_frames: duration,
            tracks: vec![],
        }
        .encode()?
        .into();
        read(&part, |path| match path {
            "body.glb" => Ok(mesh.clone()),
            "walk.motion.json" => Ok(motion.clone()),
            _ => anyhow::bail!("unexpected presentation payload {path}"),
        })?
        .context("fixture has an attachment rig")
    }

    #[test]
    fn gameplay_attachment_inputs_follow_rigs_bounds_and_clips_without_material_payloads()
    -> Result<()> {
        let mut model = json!({
            "asset":{"version":"2.0"},
            "nodes":[{"name":"root", "extras":{"resonance_bone":0}, "children":[1],
                "translation":[0.,0.,0.], "rotation":[0.,0.,0.,1.], "scale":[1.,1.,1.]},
                {"mesh":0}],
            "meshes":[{"primitives":[{"attributes":{"POSITION":0}}]}],
            "accessors":[{"min":[-1.,-2.,-3.],"max":[1.,2.,3.]}]
        });
        let original = inputs(&model, 12.)?;
        assert_eq!(original.mesh_bounds, Some([[-1., -2., -3.], [1., 2., 3.]]));
        assert_eq!(original.clips[&(None, 3)].duration_frames, 12.);
        model["meshes"][0]["primitives"][0]["material"] = 7.into();
        model["materials"] = json!([{"name":"Repainted"}]);
        let painted = inputs(&model, 12.)?;
        assert_eq!(
            serde_json::to_value(&*painted.skeleton)?,
            serde_json::to_value(&*original.skeleton)?
        );
        assert_eq!(painted.mesh_bounds, original.mesh_bounds);
        model["nodes"][0]["translation"] = json!([5., 0., 0.]);
        let moved = inputs(&model, 24.)?;
        assert_eq!(moved.mesh_bounds, Some([[4., -2., -3.], [6., 2., 3.]]));
        assert_eq!(moved.skeleton.bones[0].bind.translation, [5., 0., 0.]);
        assert_eq!(moved.clips[&(None, 3)].duration_frames, 24.);
        model["nodes"][0]["children"] = json!([99]);
        assert!(inputs(&model, 12.).is_err());
        Ok(())
    }
}
