//! Apply native skeletal adjustments and attachments to ordinary scene bones.
use super::sparse_animation::affine::{Helper as TransformHelper, Locals, Pose};
use super::{
    field_audit::{Applied, Request},
    field_view::{ActorPart, State},
};
use bevy::prelude::*;
use std::collections::BTreeMap;

/// glTF geometry nodes and their primitive children can repeat bone names.
/// Exclude both so pose updates always reach the skeleton.
pub(super) fn named_bones(
    root: Entity,
    children: &Query<&Children>,
    nodes: &Query<(&Name, &Transform, &ChildOf)>,
    meshes: &Query<(), With<Mesh3d>>,
) -> BTreeMap<String, (Entity, Transform, Entity)> {
    let mut bones = BTreeMap::new();
    for entity in children.iter_descendants(root) {
        if meshes.contains(entity)
            || children
                .get(entity)
                .is_ok_and(|children| children.iter().any(|child| meshes.contains(child)))
        {
            continue;
        }
        if let Ok((name, transform, parent)) = nodes.get(entity) {
            bones.insert(
                name.as_str().to_owned(),
                (entity, *transform, parent.parent()),
            );
        }
    }
    bones
}

#[derive(Resource, Default)]
pub(super) struct Authored(BTreeMap<Entity, Quat>);

pub(super) fn restore(mut saved: ResMut<Authored>, mut nodes: Query<&mut Transform>) {
    for (entity, rotation) in std::mem::take(&mut saved.0) {
        if let Ok(mut transform) = nodes.get_mut(entity) {
            transform.rotation = rotation;
        }
    }
}

#[allow(clippy::too_many_arguments)] // Bevy injects the independent scene queries and pose resources.
pub(super) fn bones(
    state: State,
    actors: Query<(Entity, &ActorPart, Option<&super::field_animation::Rig>)>,
    children: Query<&Children>,
    names: Query<&Name>,
    mut nodes: Query<&mut Transform>,
    mut affine: ResMut<Locals>,
    mut saved: ResMut<Authored>,
    mut applied: ResMut<Applied>,
) {
    for (root, part, rig) in &actors {
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
            let entity = match &adjustment.bone {
                resonance_events::BoneTarget::Index(index) => {
                    rig.and_then(|rig| rig.bone_at(*index))
                }
                resonance_events::BoneTarget::Name(bone) => children
                    .iter_descendants(root)
                    .find(|entity| names.get(*entity).is_ok_and(|name| name.as_str() == bone)),
            };
            if let Some(entity) = entity
                && let Ok(mut transform) = nodes.get_mut(entity)
            {
                saved.0.entry(entity).or_insert(transform.rotation);
                let [x, y, z] = adjustment
                    .sample(state.get().events.tick())
                    .map(f32::to_radians);
                affine.rotate(
                    entity,
                    &mut transform,
                    Quat::from_euler(EulerRot::ZYX, z, y, x),
                );
                applied.ack(Request::Bone(part.actor, part.part, slot));
            }
        }
    }
}

#[allow(clippy::type_complexity)] // Read attachment world poses before writing their local transforms.
pub(super) fn attachments(
    state: State,
    actors: Query<(Entity, &ActorPart)>,
    children: Query<&Children>,
    names: Query<&Name>,
    mut transforms: ParamSet<(TransformHelper, (Query<&mut Transform>, ResMut<Locals>))>,
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
            let pose = pose.mul_transform(local);
            let target = if helper.has_affine(bone) {
                Pose::Affine(pose.affine())
            } else {
                Pose::Trs(pose.compute_transform())
            };
            targets.push((root, part.actor, part.part, target));
        }
    }
    let (mut nodes, mut affine) = transforms.p1();
    for (root, actor, part, target) in targets {
        if let Ok(mut transform) = nodes.get_mut(root) {
            affine.set(root, &mut transform, target);
            applied.ack(Request::Attachment(actor, part));
        }
    }
}
