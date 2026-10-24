//! Bind battle-scene aliases and attachment queries to their cooked actor poses.
use anyhow::{Context, Result, ensure};
use resonance_content::{animation::Motion, battle_event::Scene, prepared::Files};
use resonance_events::{
    AnimationClip, AttachmentPose, ModelResource, ResourceKind, ResourceLibrary,
    animation::AnimationSource,
};
use std::sync::Arc;

pub fn load(files: &Files, scene: Arc<Scene>) -> Result<crate::battle::event::Prepared> {
    let blink = scene
        .blink
        .clone()
        .context("battle event has no captured blink cycle")?;
    blink.validate()?;
    let mut resources = ResourceLibrary {
        blink: Some(blink),
        messages: files.json(&scene.messages)?,
        actor_names: ResourceLibrary::character_names(),
        ..Default::default()
    };
    for (&resource, model) in &scene.actors {
        model.skeleton.validate()?;
        let part = model
            .parts
            .first()
            .context("battle event actor lacks body")?;
        ensure!(
            model
                .skeleton
                .bones
                .iter()
                .map(|bone| &bone.name)
                .eq(&part.bone_names),
            "battle event body bindings differ"
        );
        let skeleton = Arc::new(model.skeleton.clone());
        let mut prepared = ModelResource {
            names: part.bone_names.clone(),
            has_eyes: part
                .appearance
                .as_ref()
                .is_some_and(|appearance| appearance.eyes.is_some()),
            toon_lighting: model.parts.iter().any(|part| part.outline_color.is_some()),
            ..Default::default()
        };
        prepared.attachments.skeleton = Some(skeleton.clone());
        for clip in &part.clips {
            let motion = Arc::new(Motion::decode(&files.read(&clip.motion)?)?);
            let (source, source_resource) = clip
                .animation_resource
                .map_or((AnimationSource::Model, resource), |id| {
                    (AnimationSource::Resource, id)
                });
            prepared.attachments.clips.insert(
                (source, source_resource, clip.resource_slot),
                AttachmentPose::new(skeleton.clone(), motion)?,
            );
            let track = AnimationClip::from(clip);
            if source == AnimationSource::Model {
                ensure!(
                    prepared.clips.insert(clip.resource_slot, track).is_none(),
                    "duplicate battle event model clip"
                );
            } else {
                resources.bindings.insert(
                    source_resource as i32,
                    (ResourceKind::Animation, source_resource),
                );
                let previous = resources
                    .animations
                    .entry(source_resource)
                    .or_default()
                    .insert(clip.resource_slot, track);
                ensure!(
                    previous
                        .is_none_or(|previous| previous.duration_ticks == clip.duration_ticks()),
                    "inconsistent battle event animation duration"
                );
            }
        }
        resources
            .bindings
            .insert(resource as i32, (ResourceKind::Model, resource));
        resources.models.insert(resource, prepared);
    }
    let program = Arc::new(symphonia_script::Program::decode(
        &files.read(&scene.script)?,
    )?);
    Ok(crate::battle::event::Prepared::new(program, Arc::new(resources)).with_scene(scene))
}
