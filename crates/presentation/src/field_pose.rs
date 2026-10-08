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
pub(super) struct Authored(BTreeMap<Entity, Transform>);

pub(super) fn restore(mut saved: ResMut<Authored>, mut nodes: Query<&mut Transform>) {
    for (entity, authored) in std::mem::take(&mut saved.0) {
        if let Ok(mut transform) = nodes.get_mut(entity) {
            *transform = authored;
        }
    }
}

#[allow(clippy::too_many_arguments)] // Bevy injects the independent scene queries and pose resources.
pub(super) fn bones(
    state: State,
    actors: Query<(&ActorPart, Option<&super::field_animation::Rig>)>,
    names: Query<&Name>,
    mut nodes: Query<&mut Transform>,
    mut affine: ResMut<Locals>,
    mut saved: ResMut<Authored>,
    mut applied: ResMut<Applied>,
) {
    for (part, rig) in &actors {
        let Some(actor) = state.get().events.world.actors.get(&part.actor) else {
            continue;
        };
        if !part.prepared {
            for &slot in actor.appearance.bone_adjustments.keys() {
                applied.loading(Request::Bone(part.actor, part.part, slot));
            }
            continue;
        }
        if actor
            .appearance
            .bone_adjustments
            .values()
            .any(|a| a.translation.is_some())
            && let Some(rig) = rig
        {
            for bone in (0..).map_while(|i| rig.bone_at(i)) {
                affine.translation_boundary(bone);
            }
        }
        for (&slot, adjustment) in &actor.appearance.bone_adjustments {
            let entity = match &adjustment.bone {
                resonance_events::BoneTarget::Index(index) => {
                    rig.and_then(|rig| rig.bone_at(*index))
                }
                resonance_events::BoneTarget::Name(bone) => {
                    rig.and_then(|rig| rig.bone(bone, &names).ok().flatten())
                }
            };
            if let Some(entity) = entity
                && let Ok(mut transform) = nodes.get_mut(entity)
            {
                saved.0.entry(entity).or_insert(*transform);
                adjust_bone(
                    entity,
                    &mut transform,
                    &mut affine,
                    adjustment,
                    rig.and_then(|rig| rig.bind_scale(entity))
                        .expect("resolved rig bone"),
                    state.get().events.tick(),
                );
                applied.ack(Request::Bone(part.actor, part.part, slot));
            }
        }
    }
}

pub(super) fn outlines(
    art: Res<super::field_view::Art>,
    actors: Query<(&ActorPart, &super::field_animation::Rig)>,
    mut transforms: ParamSet<(TransformHelper, ResMut<Locals>)>,
) {
    let helper = transforms.p0();
    let primary: BTreeMap<_, Vec<_>> = actors
        .iter()
        .filter(|(part, _)| part.part == 0 && part.prepared)
        .map(|(part, rig)| {
            let matrices = (0..)
                .map_while(|i| rig.bone_at(i))
                .map(|bone| {
                    helper
                        .compute_global_transform(bone)
                        .expect("prepared bone")
                })
                .collect();
            (part.actor, matrices)
        })
        .collect();
    let mut poses = Vec::new();
    for (part, rig) in &actors {
        if !part.prepared
            || art.models[&part.resource][part.part]
                .spec
                .outline_color
                .is_none()
        {
            continue;
        }
        let Some(primary) = primary.get(&part.actor) else {
            continue;
        };
        poses.extend(
            primary
                .iter()
                .enumerate()
                .filter_map(|(i, pose)| rig.bone_at(i as u16).map(|bone| (bone, *pose))),
        );
    }
    let mut affine = transforms.p1();
    for (bone, pose) in poses {
        affine.set_world(bone, pose);
    }
}

pub(super) fn adjust_bone(
    entity: Entity,
    transform: &mut Transform,
    affine: &mut Locals,
    adjustment: &resonance_events::BoneAdjustment,
    bind_scale: Vec3,
    tick: u32,
) {
    rotate_bone(entity, transform, affine, adjustment, tick);
    if let Some(scale) = &adjustment.scale {
        affine.scale(
            entity,
            transform,
            Vec3::from_array(scale.sample(tick, bind_scale.to_array())),
        );
    }
    if adjustment.translation.is_some() {
        affine.translate(entity, Vec3::from_array(adjustment.translation(tick)));
    }
}

fn rotate_bone(
    entity: Entity,
    transform: &mut Transform,
    affine: &mut Locals,
    adjustment: &resonance_events::BoneAdjustment,
    tick: u32,
) {
    let [x, y, z] = adjustment.sample(tick).map(f32::to_radians);
    let rotation = Quat::from_euler(EulerRot::ZYX, z, y, x);
    if adjustment.absolute_rotation {
        // Door scripts supply the complete angle, including the closed door's
        // authored rotation (Salvation starts at -45°).
        let mut pose = *transform;
        pose.rotation = rotation;
        affine.set(entity, transform, Pose::Trs(pose));
    } else if [x, y, z] != [0.; 3] {
        affine.rotate_local(entity, transform, rotation);
    }
}

#[derive(Component)]
pub(super) struct AttachmentFrame {
    actor: i32,
    bone: String,
    parent: Transform,
}

#[allow(clippy::type_complexity)] // Read attachment world poses before writing their local transforms.
pub(super) fn attachments(
    mut commands: Commands,
    state: State,
    actors: Query<(Entity, &ActorPart, Option<&AttachmentFrame>)>,
    children: Query<&Children>,
    names: Query<&Name>,
    mut transforms: ParamSet<(TransformHelper, (Query<&mut Transform>, ResMut<Locals>))>,
    mut applied: ResMut<Applied>,
) {
    let mut targets = Vec::new();
    let mut parents = BTreeMap::new();
    let world = &state.get().events.world;
    let mut ordered: Vec<_> = actors.iter().collect();
    ordered.sort_by_key(|(_, part, _)| {
        std::iter::successors(Some(part.actor), |id| {
            world.actors.get(id)?.attachment.as_ref().map(|a| a.actor)
        })
        .take(world.actors.len())
        .count()
    });
    let helper = transforms.p0();
    for (root, part, retained) in ordered {
        let Some(actor) = world.actors.get(&part.actor) else {
            continue;
        };
        let Some(attachment) = actor
            .wings
            .as_ref()
            .and_then(|w| w.layer(part.pass, world.effect_tick).echo)
            .map_or(actor.attachment.as_ref(), |echo| echo.attachment.as_ref())
        else {
            continue;
        };
        let owner = actors
            .iter()
            .find(|(_, other, _)| other.actor == attachment.actor && other.part == 0);
        if !part.prepared || owner.is_some_and(|(_, owner, _)| !owner.prepared) {
            applied.loading(Request::Attachment(part.actor, part.part));
            continue;
        }
        let parent = if let Some((owner, _, _)) = owner {
            children.iter_descendants(owner).find_map(|entity| {
                names
                    .get(entity)
                    .ok()
                    .filter(|name| name.as_str() == attachment.bone)?;
                let pose = helper.global_with(entity, &parents).ok()?;
                Some(
                    Transform::from_translation(pose.translation())
                        .with_rotation(super::sparse_animation::affine::rotation(pose.affine())),
                )
            })
        } else {
            // Triet removes and recreates Colette while her wings remain;
            // retain the last resolved frame during that gap. A never-resolved
            // or changed attachment still fails the ordinary presentation audit.
            retained
                .filter(|frame| frame.actor == attachment.actor && frame.bone == attachment.bone)
                .map(|frame| frame.parent)
        };
        if let Some(parent) = parent {
            commands.entity(root).insert(AttachmentFrame {
                actor: attachment.actor,
                bone: attachment.bone.clone(),
                parent,
            });
            // sync() already applied the child's own scale, Euler rotation,
            // fixed facing and visual displacement to its root.
            let Ok(local) = helper.local(root) else {
                continue;
            };
            let target = Pose::Affine(parent.compute_affine() * local.global().affine());
            parents.insert(root, target);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn door_setters_do_not_add_the_authored_hinge_angle_twice() {
        let mut world = World::new();
        let entity = world.spawn_empty().id();
        let rest = Transform::from_xyz(40., 20., 10.)
            .with_rotation(Quat::from_rotation_z(-45_f32.to_radians()));
        let mut affine = Locals::default();
        for (absolute_rotation, angle, expected) in [
            (true, -45., -45_f32),
            (true, -60., -60.),
            (true, -90., -90.),
            (false, -30., -75.),
        ] {
            let adjustment = resonance_events::BoneAdjustment {
                bone: resonance_events::BoneTarget::Name("Door_L".into()),
                absolute_rotation,
                angles: [0., 0., angle],
                from: [0.; 3],
                duration_ticks: 1,
                start_tick: 0,
                translation: None,
                scale: None,
            };
            let mut pose = rest;
            rotate_bone(entity, &mut pose, &mut affine, &adjustment, 0);
            let direction = pose.rotation * Vec3::X;
            let expected = Quat::from_rotation_z(expected.to_radians()) * Vec3::X;
            assert!(direction.distance(expected) < 0.0001);
            assert_eq!(pose.translation, rest.translation);
            assert_eq!(pose.scale, rest.scale);
        }
    }
}
