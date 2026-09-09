//! Apply native skeletal adjustments and attachments to ordinary scene bones.
use super::{
    field_audit::{Applied, Request},
    field_view::{ActorPart, State},
};
use bevy::{prelude::*, transform::helper::TransformHelper};
use std::collections::BTreeMap;

#[derive(Resource, Default)]
pub(super) struct Authored(BTreeMap<Entity, Quat>);

pub(super) fn restore(mut saved: ResMut<Authored>, mut nodes: Query<&mut Transform>) {
    for (entity, rotation) in std::mem::take(&mut saved.0) {
        if let Ok(mut transform) = nodes.get_mut(entity) {
            transform.rotation = rotation;
        }
    }
}

pub(super) fn bones(
    state: State,
    actors: Query<(Entity, &ActorPart)>,
    children: Query<&Children>,
    names: Query<&Name>,
    mut nodes: Query<&mut Transform>,
    mut saved: ResMut<Authored>,
    mut applied: ResMut<Applied>,
) {
    for (root, part) in &actors {
        let Some(actor) = state.get().events.world.actors.get(&part.actor) else {
            continue;
        };
        if !part.prepared {
            for &slot in actor.appearance.bone_adjustments.keys() {
                applied.loading(Request::Bone(part.actor, part.part, slot));
            }
            continue;
        }
        for (&slot, adjustment) in &actor.appearance.bone_adjustments {
            let entity = children.iter_descendants(root).find(|entity| {
                names
                    .get(*entity)
                    .is_ok_and(|name| name.as_str() == adjustment.bone)
            });
            if let Some(entity) = entity
                && let Ok(mut transform) = nodes.get_mut(entity)
            {
                saved.0.entry(entity).or_insert(transform.rotation);
                let [x, y, z] = adjustment
                    .sample(state.get().events.tick())
                    .map(f32::to_radians);
                transform.rotation *= Quat::from_euler(EulerRot::ZYX, z, y, x);
                applied.ack(Request::Bone(part.actor, part.part, slot));
            }
        }
    }
}

pub(super) fn attachments(
    state: State,
    actors: Query<(Entity, &ActorPart)>,
    children: Query<&Children>,
    names: Query<&Name>,
    mut transforms: ParamSet<(TransformHelper, Query<&mut Transform>)>,
    mut applied: ResMut<Applied>,
) {
    let mut targets = Vec::new();
    let helper = transforms.p0();
    for (root, part) in &actors {
        let Some(actor) = state.get().events.world.actors.get(&part.actor) else {
            continue;
        };
        let Some(attachment) = &actor.attachment else {
            continue;
        };
        let owner = actors
            .iter()
            .find(|(_, other)| other.actor == attachment.actor && other.part == 0);
        let Some((owner, owner_part)) = owner else {
            continue;
        };
        if !part.prepared || !owner_part.prepared {
            applied.loading(Request::Attachment(part.actor, part.part));
            continue;
        }
        let bone = children.iter_descendants(owner).find(|entity| {
            names
                .get(*entity)
                .is_ok_and(|name| name.as_str() == attachment.bone)
        });
        if let Some(bone) = bone
            && let Ok(pose) = helper.compute_global_transform(bone)
        {
            let local = Transform::from_translation(Vec3::from_array(actor.position))
                .with_rotation(Quat::from_rotation_z(actor.heading.to_radians()));
            targets.push((
                root,
                part.actor,
                part.part,
                pose.mul_transform(local).compute_transform(),
            ));
        }
    }
    for (root, actor, part, target) in targets {
        if let Ok(mut transform) = transforms.p1().get_mut(root) {
            *transform = target;
            applied.ack(Request::Attachment(actor, part));
        }
    }
}
