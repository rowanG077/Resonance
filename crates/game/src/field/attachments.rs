//! Prepare gameplay attachment queries from the same mesh and curves as drawing.
use anyhow::{Context, Result};
use resonance_content::field::{FieldAssets, attachments};
use resonance_events::AttachmentPose;
use std::{collections::BTreeMap, sync::Arc};

pub type Attachments = BTreeMap<u32, resonance_events::ModelAttachments>;

pub fn prepare(
    assets: &FieldAssets,
    mut read: impl FnMut(&str) -> Result<Arc<[u8]>>,
) -> Result<Attachments> {
    let mut poses = Attachments::new();
    let models = assets
        .actors
        .iter()
        .map(|actor| (actor.resource, actor.parts.first()))
        .chain(assets.parts.iter().map(|part| {
            (
                resonance_content::field::SCENERY_RESOURCE_BASE + u32::from(part.resource),
                Some(part),
            )
        }));
    for (resource, model) in models {
        let model = model.context("attachment model is missing")?;
        let Some(inputs) = attachments::read(model, &mut read)? else {
            continue;
        };
        let skeleton = inputs.skeleton;
        let poses = poses.entry(resource).or_default();
        poses.mesh_bounds = inputs.mesh_bounds;
        poses.skeleton = Some(skeleton.clone());
        // Following effects and scenario attachment queries can sample any live clip.
        for ((animation_resource, slot), motion) in inputs.clips {
            use resonance_events::animation::AnimationSource;
            let (source, resource) = animation_resource
                .map_or((AnimationSource::Model, resource), |id| {
                    (AnimationSource::Resource, id)
                });
            poses.clips.insert(
                (source, resource, slot),
                AttachmentPose::new(skeleton.clone(), motion)?,
            );
        }
    }
    Ok(poses)
}
