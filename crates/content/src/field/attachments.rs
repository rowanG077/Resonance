//! Gameplay attachments evaluate the same glTF animation as rendering.
//! GLB materials and vertex payloads are not gameplay; indexed bones and the
//! rigid bounds used by block interaction are.
use crate::{
    ScenePart,
    animation::{Motion, NamedMotion, Skeleton},
};
use anyhow::{Context, Result, ensure};
use std::{collections::BTreeMap, sync::Arc};

pub struct Inputs {
    pub skeleton: Arc<Skeleton>,
    pub mesh_bounds: Option<[[f32; 3]; 2]>,
    pub clips: BTreeMap<(Option<u32>, u16), Clip>,
}

/// An immutable, validated motion/rig binding. Only preparation can construct it.
/// Consumers can use the proof without scanning every curve again.
pub struct Clip {
    skeleton: Arc<Skeleton>,
    motion: Arc<Motion>,
}

impl Clip {
    pub fn into_parts(self) -> (Arc<Skeleton>, Arc<Motion>) {
        (self.skeleton, self.motion)
    }
}

#[derive(Debug)]
pub(crate) struct Rig {
    pub skeleton: Arc<Skeleton>,
    pub mesh_bounds: Option<[[f32; 3]; 2]>,
}

/// Parsed geometry belongs to its captured bytes and to the scenes using it.
#[derive(Debug)]
pub struct Geometry {
    bytes: Arc<[u8]>,
    pub(crate) bone_names: Vec<String>,
    pub(crate) material_nodes: Vec<Vec<u16>>,
    rig: std::sync::OnceLock<Arc<Rig>>,
}

impl Geometry {
    pub(crate) fn decode(bytes: Arc<[u8]>) -> Result<Self> {
        let (bone_names, material_nodes) = crate::animation::scene_nodes_from_glb(&bytes)?;
        Ok(Self {
            bytes,
            bone_names,
            material_nodes,
            rig: Default::default(),
        })
    }

    pub(crate) fn matches(&self, bytes: &Arc<[u8]>) -> bool {
        crate::same_bytes(&self.bytes, bytes)
    }

    fn rig(&self) -> Result<Arc<Rig>> {
        if let Some(rig) = self.rig.get() {
            return Ok(rig.clone());
        }
        let rig = Arc::new(Rig::decode(&self.bytes)?);
        let _ = self.rig.set(rig);
        Ok(self.rig.get().unwrap().clone())
    }
}

impl Rig {
    pub(crate) fn decode(bytes: &[u8]) -> Result<Self> {
        let (skeleton, mesh_bounds) = Skeleton::from_glb_with_bounds(bytes)?;
        Ok(Self {
            skeleton: Arc::new(skeleton),
            mesh_bounds,
        })
    }

    pub(crate) fn validate(&self, bone_names: &[String]) -> Result<()> {
        ensure!(
            self.skeleton
                .bones
                .iter()
                .map(|bone| &bone.name)
                .eq(bone_names),
            "attachment skeleton differs from scene"
        );
        Ok(())
    }
}

pub fn read(
    part: &ScenePart,
    motions: &mut BTreeMap<String, (Arc<[u8]>, NamedMotion)>,
    mut read: impl FnMut(&str) -> Result<Arc<[u8]>>,
) -> Result<Option<Inputs>> {
    if part.bone_names.is_empty() {
        return Ok(None);
    }
    let bytes = read(&part.mesh)?;
    let rig = match &part.prepared_geometry {
        Some(geometry) if geometry.matches(&bytes) => geometry.rig(),
        _ => Rig::decode(&bytes).map(Arc::new),
    }
    .with_context(|| format!("field attachment geometry {}", part.mesh))?;
    rig.validate(&part.bone_names)
        .with_context(|| part.mesh.clone())?;
    let skeleton = rig.skeleton.clone();
    let mesh_bounds = rig.mesh_bounds;
    let mut clips = BTreeMap::new();
    for (index, clip) in part.clips.iter().enumerate() {
        let bytes = read(&clip.motion)?;
        let motion = match part.prepared_motion(index, &bytes) {
            Some(motion) => motion,
            None => {
                if !motions
                    .get(&clip.motion)
                    .is_some_and(|(captured, _)| crate::same_bytes(captured, &bytes))
                {
                    motions.insert(
                        clip.motion.clone(),
                        (bytes.clone(), NamedMotion::decode(&bytes)?),
                    );
                }
                Arc::new(
                    motions[&clip.motion]
                        .1
                        .bind(&part.bone_names, &clip.joints)
                        .with_context(|| format!("field attachment motion {}", clip.motion))?,
                )
            }
        };
        ensure!(
            clips
                .insert(
                    (clip.animation_resource, clip.resource_slot),
                    Clip {
                        skeleton: skeleton.clone(),
                        motion
                    },
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

    fn inputs(geometry: &Value, duration: f32) -> Result<Inputs> {
        let mut part: ScenePart = serde_json::from_value(json!({
            "resource":0, "mesh":"body.glb", "textures":["unneeded.png"],
            "materials":[], "translation":[0.,0.,0.], "autoplay":false,
            "texture_animations":[],
            "clips":[{"motion":"walk.glb", "slots":[3]}]
        }))?;
        let mesh: Arc<[u8]> = resonance_asset_writer::gltf::pack_glb(geometry, &[])?.into();
        part.prepare_bones(&mesh)?;
        let motion: Arc<[u8]> = Motion {
            duration_frames: duration,
            tracks: vec![],
        }
        .encode(&Skeleton::from_glb(&mesh)?)?
        .into();
        read(&part, &mut BTreeMap::new(), |path| match path {
            "body.glb" => Ok(mesh.clone()),
            "walk.glb" => Ok(motion.clone()),
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
        assert_eq!(original.clips[&(None, 3)].motion.duration_frames, 12.);
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
        assert_eq!(moved.clips[&(None, 3)].motion.duration_frames, 24.);
        model["nodes"][0]["children"] = json!([99]);
        assert!(inputs(&model, 12.).is_err());
        Ok(())
    }
    #[test]
    fn prepared_curves_are_shared_but_changed_bytes_and_bindings_are_fresh() -> Result<()> {
        let mesh: Arc<[u8]> = resonance_asset_writer::gltf::pack_glb(
            &json!({
                "asset":{"version":"2.0"},
                "nodes":[{"name":"root","extras":{"resonance_bone":0}}]
            }),
            &[],
        )?
        .into();
        let mut part: ScenePart = serde_json::from_value(json!({
            "mesh":"body.glb", "clips":[{"motion":"walk.glb","slots":[3]}]
        }))?;
        part.prepare_geometry(Arc::new(Geometry::decode(mesh.clone())?));
        let skeleton = Skeleton::from_glb(&mesh)?;
        let motion = Arc::new(Motion {
            duration_frames: 12.,
            tracks: vec![],
        });
        let bytes: Arc<[u8]> = motion.encode(&skeleton)?.into();
        part.clips[0].duration_seconds = 12. / crate::animation::FRAME_HZ;
        part.retain_motions(vec![Some((bytes.clone(), motion.clone()))]);
        let mut decoded = BTreeMap::new();
        let load = |part: &ScenePart,
                    bytes: &Arc<[u8]>,
                    decoded: &mut BTreeMap<String, (Arc<[u8]>, NamedMotion)>|
         -> Result<Inputs> {
            read(part, decoded, |path| match path {
                "body.glb" => Ok(mesh.clone()),
                "walk.glb" => Ok(bytes.clone()),
                _ => anyhow::bail!("unexpected input {path}"),
            })?
            .context("missing rig")
        };
        let shared = load(&part, &bytes, &mut decoded)?;
        let again = load(&part, &bytes, &mut decoded)?;
        assert!(Arc::ptr_eq(&shared.skeleton, &again.skeleton));
        assert!(
            read(&part, &mut decoded, |_| Ok(Arc::from(
                &b"invalid geometry"[..]
            )))
            .is_err()
        );
        assert!(decoded.is_empty(), "decoded already prepared curves");
        assert!(Arc::ptr_eq(&shared.clips[&(None, 3)].motion, &motion));
        assert!(Arc::ptr_eq(
            &part.prepared_motion_snapshot().unwrap()[0],
            &motion
        ));
        let round_trip: ScenePart = serde_json::from_value(serde_json::to_value(&part)?)?;
        assert!(
            round_trip.prepared_motions.is_none(),
            "persisted a prepared snapshot"
        );
        let changed: Arc<[u8]> = Motion {
            duration_frames: 24.,
            tracks: vec![],
        }
        .encode(&skeleton)?
        .into();
        let fresh = load(&part, &changed, &mut decoded)?;
        assert_eq!(fresh.clips[&(None, 3)].motion.duration_frames, 24.);
        let changed_again: Arc<[u8]> = Motion {
            duration_frames: 36.,
            tracks: vec![],
        }
        .encode(&skeleton)?
        .into();
        let fresh_again = load(&part, &changed_again, &mut decoded)?;
        assert_eq!(fresh_again.clips[&(None, 3)].motion.duration_frames, 36.);
        assert!(!Arc::ptr_eq(&fresh.clips[&(None, 3)].motion, &motion));
        let corrupt: Arc<[u8]> = Arc::from(b"invalid current animation".as_slice());
        assert!(load(&part, &corrupt, &mut decoded).is_err());
        Arc::make_mut(&mut part.clips[0].joints).insert("root".into(), None);
        assert!(part.prepared_motion_snapshot().is_none());
        let fresh = load(&part, &bytes, &mut BTreeMap::new())?;
        assert!(!Arc::ptr_eq(&fresh.clips[&(None, 3)].motion, &motion));
        assert!(fresh.clips[&(None, 3)].motion.tracks.is_empty());
        part.bone_names[0] = "other".into();
        assert!(part.prepared_motion(0, &bytes).is_none());
        Ok(())
    }
}
