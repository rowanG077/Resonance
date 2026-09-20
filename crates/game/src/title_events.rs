//! Title asset setup. Native opcode handlers live in resonance-events and
//! operate on resource IDs, independently of this scene's packed resource map.
use resonance_content::TitleScene;
use resonance_events::{
    AnimationClip, AttachmentTrack, EventRuntime, ModelResource, ResourceKind, ResourceLibrary,
};
use std::{collections::BTreeMap, sync::Arc};
use symphonia_script::Program;

pub fn start(bytes: &[u8], scene: &TitleScene) -> anyhow::Result<EventRuntime> {
    let program = Arc::new(Program::decode(bytes)?);
    EventRuntime::new(program, Arc::new(resources(scene)))
}

fn resources(scene: &TitleScene) -> ResourceLibrary {
    let mut library = ResourceLibrary::default();
    for part in &scene.parts {
        if part.clips.is_empty() || part.autoplay {
            continue;
        }
        let mut model = ModelResource {
            // The title draws actors before scripts read their bone matrices.
            attachment_pose_delay: 1,
            ..Default::default()
        };
        let local = |p: [f32; 3]| std::array::from_fn(|i| p[i] - part.translation[i]);
        let samples =
            |p: &[[f32; 3]]| AttachmentTrack::Samples(p.iter().copied().map(local).collect());
        for spec in &part.clips {
            let mut attachments = BTreeMap::new();
            match (part.resource, spec.resource_slot) {
                (17, 12) => {
                    attachments.insert("Fz_Bone01".into(), samples(&scene.glow.feather));
                    attachments.insert(
                        "Dummy".into(),
                        AttachmentTrack::Constant(local(scene.glow.landing)),
                    );
                }
                (17, 36) => {
                    attachments.insert("Dummy".into(), samples(&scene.glow.landing_loop));
                }
                (18, 12) => {
                    attachments.insert("Rf_Fez_Ref_120".into(), samples(&scene.glow.reflection));
                }
                _ => {}
            }
            model.clips.insert(
                spec.resource_slot,
                AnimationClip {
                    duration_ticks: spec.duration_ticks(),
                    attachments,
                },
            );
        }
        model.names = model
            .clips
            .values()
            .flat_map(|c| c.attachments.keys().cloned())
            .collect();
        model.names.sort();
        model.names.dedup();
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
    library
}
