//! Apply native skeletal adjustments and attachments to ordinary scene bones.
use super::sparse_animation::affine::{Helper as TransformHelper, Locals, Pose};
use super::{
    field_audit::{Applied, Request},
    field_view::{ActorPart, State},
};
use bevy::prelude::*;
use std::collections::BTreeMap;

pub(super) fn has_relative_translation(actor: &resonance_events::Actor) -> bool {
    actor.appearance.bone_adjustments.values().any(|a| {
        a.translation
            .as_ref()
            .is_some_and(|translation| !translation.absolute)
    })
}

#[allow(clippy::too_many_arguments)] // Bevy injects the independent scene queries and pose resources.
pub(super) fn bones(
    state: State,
    actors: Query<(&ActorPart, Option<&super::field_animation::Rig>)>,
    names: Query<&Name>,
    mut nodes: Query<&mut Transform>,
    mut affine: ResMut<Locals>,
    mut applied: ResMut<Applied>,
) {
    for (part, rig) in &actors {
        let Some(actor) = state.get().events.world.field_model_actor(part.actor) else {
            continue;
        };
        if !part.prepared {
            for &slot in actor.appearance.bone_adjustments.keys() {
                applied.loading(Request::Bone(part.actor, part.part, slot));
            }
            continue;
        }
        if has_relative_translation(actor)
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

/// A model and its outline keep their material handles for the instance's lifetime.
#[derive(Default)]
pub(super) struct Winding(Option<(bool, bool)>);

impl Winding {
    pub(super) fn apply(
        &mut self,
        surfaces: &mut Assets<crate::materials::TitleSurface>,
        handles: &[Handle<crate::materials::TitleSurface>],
        spec: &resonance_content::ScenePart,
        world: bevy::math::Affine3A,
        double_sided: bool,
    ) {
        use resonance_content::CullFace;
        let mirrored = !double_sided && world.matrix3.determinant() < 0.;
        let mode = Some((double_sided, mirrored));
        if self.0 == mode {
            return;
        }
        self.0 = mode;
        for (handle, material) in handles.iter().zip(&spec.materials) {
            let cull = match (material.cull, mirrored) {
                _ if double_sided => CullFace::None,
                (CullFace::Back, true) => CullFace::Front,
                (CullFace::Front, true) => CullFace::Back,
                (cull, _) => cull,
            };
            if surfaces.get(handle).is_some_and(|s| s.cull != cull) {
                surfaces.get_mut(handle).unwrap().cull = cull;
            }
        }
    }
}

pub(super) fn finish_models(
    art: Res<super::field_view::Art>,
    state: State,
    mut actors: Query<(Entity, &mut ActorPart, Option<&super::field_animation::Rig>)>,
    mut surfaces: ResMut<Assets<crate::materials::TitleSurface>>,
    mut transforms: ParamSet<(TransformHelper, ResMut<Locals>)>,
) {
    let helper = transforms.p0();
    let primary: BTreeMap<_, Vec<_>> = actors
        .iter()
        .filter(|(_, part, _)| part.part == 0 && part.prepared)
        .filter_map(|(_, part, rig)| {
            let rig = rig?;
            let matrices = helper
                .compute_global_transforms((0..).map_while(|i| rig.bone_at(i)))
                .expect("prepared bones");
            Some((part.actor, matrices))
        })
        .collect();
    let mut poses = Vec::new();
    for (entity, mut part, rig) in &mut actors {
        let part = &mut *part;
        if part.disabled || !part.prepared {
            continue;
        }
        let Some(actor) = state.get().events.world.field_model_actor(part.actor) else {
            continue;
        };
        let spec = &art.models[&part.resource][part.part].spec;
        let double_sided =
            actor.ring_station || actor.resource == resonance_content::field::SAVE_POINT_RESOURCE;
        let world = helper
            .compute_global_transform(entity)
            .expect("prepared model root")
            .affine();
        part.winding
            .apply(&mut surfaces, &part.materials, spec, world, double_sided);
        let Some(rig) = rig.filter(|_| spec.outline_color.is_some()) else {
            continue;
        };
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
    if let Some(translation) = &adjustment.translation {
        let position = Vec3::from_array(adjustment.translation(tick));
        if translation.absolute {
            let pose = match affine.get(entity, *transform) {
                Pose::Trs(mut value) => {
                    value.translation = position;
                    Pose::Trs(value)
                }
                Pose::Affine(mut value) => {
                    value.translation = position.into();
                    Pose::Affine(value)
                }
            };
            affine.set(entity, transform, pose);
        } else {
            affine.translate(entity, position);
        }
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

#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct AttachedActors<'w, 's> {
    actors: Query<'w, 's, (Entity, &'static ActorPart, Option<&'static AttachmentFrame>)>,
    children: Query<'w, 's, &'static Children>,
    names: Query<'w, 's, &'static Name>,
}

impl AttachedActors<'_, '_> {
    /// Resolve nested attachments without changing the sampled actor skeletons.
    pub(super) fn roots(
        &self,
        world: &resonance_events::GameWorld,
        helper: &TransformHelper,
        mut resolved: impl FnMut(Entity, &ActorPart, Option<(&resonance_events::Attachment, Transform)>),
    ) -> BTreeMap<Entity, Pose> {
        let mut parents = BTreeMap::new();
        let mut ordered: Vec<_> = self.actors.iter().collect();
        ordered.sort_by_key(|(_, part, _)| {
            std::iter::successors(Some(part.actor), |id| {
                world
                    .field_model_actor(*id)?
                    .attachment
                    .as_ref()
                    .map(|a| a.actor)
            })
            .take(world.actors.len())
            .count()
        });
        for (root, part, retained) in ordered {
            let Some(actor) = world.field_model_actor(part.actor) else {
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
            let owner = self
                .actors
                .iter()
                .find(|(_, other, _)| other.actor == attachment.actor && other.part == 0);
            if !part.prepared || owner.is_some_and(|(_, owner, _)| !owner.prepared) {
                resolved(root, part, None);
                continue;
            }
            let parent = if let Some((owner, _, _)) = owner {
                self.children.iter_descendants(owner).find_map(|entity| {
                    self.names
                        .get(entity)
                        .ok()
                        .filter(|name| name.as_str() == attachment.bone)?;
                    let pose = helper.global_with(entity, &parents, None).ok()?;
                    Some(
                        Transform::from_translation(pose.translation()).with_rotation(
                            super::sparse_animation::affine::rotation(pose.affine()),
                        ),
                    )
                })
            } else {
                // Triet removes and recreates Colette while her wings remain;
                // retain the last resolved frame during that gap. A never-resolved
                // or changed attachment still fails the ordinary presentation audit.
                retained
                    .filter(|frame| {
                        frame.actor == attachment.actor && frame.bone == attachment.bone
                    })
                    .map(|frame| frame.parent)
            };
            if let Some(parent) = parent {
                // sync() already applied the child's own scale, Euler rotation,
                // fixed facing and visual displacement to its root.
                let Ok(local) = helper.local(root) else {
                    continue;
                };
                let target = Pose::Affine(parent.compute_affine() * local.global().affine());
                parents.insert(root, target);
                resolved(root, part, Some((attachment, parent)));
            }
        }
        parents
    }
}

#[allow(clippy::type_complexity)]
pub(super) fn attachments(
    mut commands: Commands,
    state: State,
    actors: AttachedActors,
    mut transforms: ParamSet<(TransformHelper, (Query<&mut Transform>, ResMut<Locals>))>,
    mut applied: ResMut<Applied>,
) {
    let parents = actors.roots(
        &state.get().events.world,
        &transforms.p0(),
        |root, part, frame| {
            let request = Request::Attachment(part.actor, part.part);
            if let Some((attachment, parent)) = frame {
                commands.entity(root).insert(AttachmentFrame {
                    actor: attachment.actor,
                    bone: attachment.bone.clone(),
                    parent,
                });
                applied.ack(request);
            } else {
                applied.loading(request);
            }
        },
    );
    let (mut nodes, mut affine) = transforms.p1();
    for (root, target) in parents {
        if let Ok(mut transform) = nodes.get_mut(root) {
            affine.set(root, &mut transform, target);
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
