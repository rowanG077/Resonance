//! Title asset setup. Native opcode handlers live in resonance-events and
//! operate on resource IDs, independently of this scene's packed resource map.
use anyhow::Context;
use resonance_content::{TitleScene, animation::Motion};
use resonance_events::{
    AnimationClip, AttachmentPose, EventRuntime, ModelResource, ResourceKind, ResourceLibrary,
};
use std::sync::Arc;
use symphonia_script::Program;

pub fn start(
    bytes: &[u8],
    scene: &TitleScene,
    load: impl FnMut(&str) -> anyhow::Result<Arc<Motion>>,
) -> anyhow::Result<EventRuntime> {
    let program = Arc::new(Program::decode(bytes)?);
    EventRuntime::new(program, Arc::new(resources(scene, load)?))
}

fn resources(
    scene: &TitleScene,
    mut load: impl FnMut(&str) -> anyhow::Result<Arc<Motion>>,
) -> anyhow::Result<ResourceLibrary> {
    let mut library = ResourceLibrary::default();
    for part in &scene.parts {
        if part.clips.is_empty() || part.autoplay {
            continue;
        }
        let mut model = ModelResource {
            // The title draws actors before scripts read their bone matrices.
            attachment_pose_delay: 1,
            names: part.bone_names.clone(),
            ..Default::default()
        };
        let skeleton = Arc::new(
            scene
                .glow
                .skeletons
                .get(&part.resource)
                .context("missing title attachment skeleton")?
                .clone(),
        );
        for spec in &part.clips {
            model.clips.insert(
                spec.resource_slot,
                AnimationClip {
                    duration_ticks: spec.duration_ticks(),
                    attachments: Some(AttachmentPose::new(skeleton.clone(), load(&spec.motion)?)?),
                },
            );
        }
        library.models.insert(u32::from(part.resource), model);
        library.bindings.insert(
            -1179648 + i32::from(part.resource) - 16,
            (ResourceKind::Model, u32::from(part.resource)),
        );
    }
    library.bindings.insert(-1179648, (ResourceKind::Camera, 0));
    library.bindings.insert(-1179641, (ResourceKind::Camera, 1));
    library
        .bindings
        .insert(-1179645, (ResourceKind::Overlay, 19));
    library
        .particles
        .insert(10, resonance_events::ParticleKind::Glow);
    Ok(library)
}
