//! Prepare gameplay attachment queries from the same mesh and curves as drawing.
use anyhow::{Context, Result, ensure};
use resonance_content::{
    animation::{Motion, Skeleton},
    field::FieldAssets,
};
use resonance_events::AttachmentPose;
use std::{collections::BTreeMap, sync::Arc};

pub type Attachments = BTreeMap<u32, resonance_events::ModelAttachments>;

pub fn prepare(
    assets: &FieldAssets,
    mut read: impl FnMut(&str) -> Result<Arc<[u8]>>,
) -> Result<Attachments> {
    let mut poses = Attachments::new();
    for actor in &assets.actors {
        let model = actor.parts.first().context("attachment model is missing")?;
        if model.bone_names.is_empty() {
            continue;
        }
        let skeleton = Arc::new(Skeleton::from_glb(&read(&model.mesh)?)?);
        ensure!(
            skeleton.bones.iter().map(|b| &b.name).eq(&model.bone_names),
            "attachment skeleton differs from scene"
        );
        let poses = poses.entry(actor.resource).or_default();
        poses.skeleton = Some(skeleton.clone());
        // Following effects and scenario attachment queries can sample any live clip.
        for clip in &model.clips {
            let motion = Arc::new(Motion::decode(&read(&clip.motion)?)?);
            use resonance_events::animation::AnimationSource;
            let (source, resource) = clip
                .animation_resource
                .map_or((AnimationSource::Model, actor.resource), |id| {
                    (AnimationSource::Resource, id)
                });
            poses.clips.insert(
                (source, resource, clip.resource_slot),
                AttachmentPose::new(skeleton.clone(), motion)?,
            );
        }
    }
    Ok(poses)
}
