//! Blend authored skeletal poses before script adjustments and secondary motion.
use super::field_view::{ActorPart, Art, State};
use bevy::prelude::*;

#[derive(Component)]
pub(super) struct Rig {
    /// At least one animated pose has been evaluated.
    pub(super) sampled: bool,
    bones: Vec<(Entity, Transform)>,
    previous: Vec<Transform>,
    from: Vec<Transform>,
    presented: Vec<Transform>,
    binding_pose: Vec<Transform>,
    clip: Option<(u32, u16, u32)>,
    late_binding: bool,
}

pub(super) fn bind(
    mut commands: Commands,
    art: Res<Art>,
    roots: Query<(Entity, &ActorPart), Without<Rig>>,
    children: Query<&Children>,
    nodes: Query<(&Name, &Transform, &ChildOf)>,
    meshes: Query<(), With<Mesh3d>>,
) {
    for (root, part) in &roots {
        if !part.prepared {
            continue;
        }
        let names = super::field_pose::named_bones(root, &children, &nodes, &meshes);
        let spec = &art.models[&part.resource][part.part].spec;
        let bones: Vec<_> = spec
            .bone_names
            .iter()
            .filter_map(|name| names.get(name).map(|&(entity, rest, _)| (entity, rest)))
            .collect();
        if bones.is_empty() {
            continue;
        }
        let previous: Vec<_> = bones.iter().map(|(_, t)| *t).collect();
        commands.entity(root).insert(Rig {
            sampled: false,
            bones,
            from: previous.clone(),
            presented: previous.clone(),
            binding_pose: Vec::new(),
            previous,
            clip: None,
            late_binding: false,
        });
    }
}

/// A new clip can omit channels animated by the previous clip. Bevy writes
/// only its own channels, so reset every bone to the authored rest transform.
pub(super) fn restore(rigs: Query<&Rig>, mut nodes: Query<&mut Transform>) {
    for rig in &rigs {
        for &(entity, rest) in &rig.bones {
            if let Ok(mut transform) = nodes.get_mut(entity) {
                *transform = rest;
            }
        }
    }
}

fn mix(from: Transform, to: Transform, weight: f32) -> Transform {
    Transform {
        translation: from.translation.lerp(to.translation, weight),
        rotation: from.rotation.slerp(to.rotation, weight),
        scale: from.scale.lerp(to.scale, weight),
    }
}

impl Rig {
    fn blend_bone(&mut self, index: usize, pose: &mut Transform, weight: f32, hold: bool) {
        if weight < 1. {
            *pose = mix(self.from[index], *pose, weight);
        }
        if hold {
            if self.binding_pose.is_empty() {
                self.binding_pose.clone_from(&self.presented);
            }
            self.binding_pose[index] = *pose;
            *pose = self.presented[index];
            return;
        }
        self.presented[index] = *pose;
        // Interrupted blends retain their completed source pose. The binding
        // frame separately holds the last displayed, potentially blended pose.
        if weight >= 1. {
            self.previous[index] = *pose;
        }
    }

    /// Event attachments can observe a binding before its first model draw.
    pub(super) fn binding_attachment(
        &self,
        mut entity: Entity,
        transforms: &Query<(&Transform, Option<&ChildOf>)>,
    ) -> Option<Vec3> {
        if self.binding_pose.is_empty() {
            return None;
        }
        let mut result = GlobalTransform::IDENTITY;
        loop {
            let (&current, parent) = transforms.get(entity).ok()?;
            let local = self
                .bones
                .iter()
                .position(|&(bone, _)| bone == entity)
                .map_or(current, |index| {
                    let base = self.presented[index];
                    let binding = self.binding_pose[index];
                    // Preserve the script/secondary adjustments applied after
                    // blending, while replacing the held animation underneath.
                    Transform {
                        translation: binding.translation + (current.translation - base.translation),
                        rotation: binding.rotation * (base.rotation.inverse() * current.rotation),
                        scale: binding.scale * (current.scale / base.scale),
                    }
                });
            result = GlobalTransform::from(local) * result;
            let Some(parent) = parent else {
                return Some(result.translation());
            };
            entity = parent.parent();
        }
    }
}

pub(super) fn blend(
    state: State,
    mut rigs: Query<(&ActorPart, &mut Rig)>,
    mut nodes: Query<&mut Transform>,
) {
    let world = &state.get().events.world;
    for (part, mut rig) in &mut rigs {
        let actor = world.actors.get(&part.actor);
        let animation = actor.and_then(|a| a.animation.as_ref());
        let key = animation.map(|a| (a.resource, a.slot, a.start_tick));
        if key != rig.clip {
            // Event bindings occur after the actor's draw. Preserve that pose
            // for the binding frame, including repeated captures of this tick.
            rig.late_binding = rig.clip.is_some()
                && animation.is_some_and(|a| {
                    a.binding_timing == resonance_events::animation::BindingTiming::AfterDraw
                        && a.start_tick == world.tick
                });
            rig.from = rig.previous.clone();
            rig.clip = key;
        }
        let hold = rig.late_binding && animation.is_some_and(|a| a.start_tick == world.tick);
        if !hold {
            rig.binding_pose.clear();
        }
        let weight = animation.map_or(1., |a| a.blend_weight(world.tick));
        for i in 0..rig.bones.len() {
            if let Ok(mut transform) = nodes.get_mut(rig.bones[i].0) {
                rig.blend_bone(i, &mut transform, weight, hold);
            }
        }
        rig.sampled = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn omitted_channels_return_to_rest_without_retaining_the_old_clip() {
        let old = Transform::from_xyz(12., 4., 0.).with_rotation(Quat::from_rotation_z(1.));
        let rest = Transform::IDENTITY;
        let mut world = World::new();
        let bone = world.spawn(old).id();
        let rig = world
            .spawn(Rig {
                sampled: false,
                bones: vec![(bone, rest)],
                previous: vec![old],
                from: vec![old],
                presented: vec![old],
                binding_pose: Vec::new(),
                clip: None,
                late_binding: false,
            })
            .id();
        let mut schedule = Schedule::default();
        schedule.add_systems(restore);
        schedule.run(&mut world);
        // The next clip writes rotation only; the prior translation must be
        // gone even though no animation channel overwrites it this frame.
        world.get_mut::<Transform>(bone).unwrap().rotation = Quat::from_rotation_x(0.4);
        assert_eq!(
            world.get::<Transform>(bone).unwrap().translation,
            Vec3::ZERO
        );
        let middle = mix(old, rest, 0.5);
        assert_eq!(middle.translation, Vec3::new(6., 2., 0.));
        assert!(middle.rotation.angle_between(Quat::from_rotation_z(0.5)) < 0.001);
        let end = mix(old, rest, 1.);
        assert_eq!(end.translation, Vec3::ZERO);
        assert!(end.rotation.angle_between(Quat::IDENTITY) < 0.001);

        // A binding during this unfinished blend holds the actual visible
        // midpoint, but its later cross-fade still starts at the completed pose.
        let mut rig = world.get_mut::<Rig>(rig).unwrap();
        let mut pose = rest;
        rig.blend_bone(0, &mut pose, 0.5, false);
        assert_eq!(pose, middle);
        let next = Transform::from_xyz(-8., 2., 6.);
        rig.from = rig.previous.clone();
        for _ in 0..2 {
            pose = next;
            rig.blend_bone(0, &mut pose, 0.25, true);
            assert_eq!(pose, middle);
        }
        assert_eq!(rig.previous, vec![old]);
        pose = next;
        rig.blend_bone(0, &mut pose, 0.25, false);
        assert_eq!(pose, mix(old, next, 0.25));
    }

    #[test]
    fn dialogue_attachment_observes_binding_without_changing_the_drawn_pose() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        let root = world.spawn(Transform::from_xyz(100., 200., 0.)).id();
        let quarter_turn = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
        let adjusted = Transform::from_rotation(quarter_turn);
        let neck = world.spawn((adjusted, ChildOf(root))).id();
        let held_head = Transform::from_xyz(0., 10., 20.);
        let head = world.spawn((held_head, ChildOf(neck))).id();
        let held = vec![Transform::IDENTITY, held_head];
        let rig = world
            .spawn(Rig {
                sampled: true,
                bones: vec![(neck, held[0]), (head, held_head)],
                previous: held.clone(),
                from: held.clone(),
                presented: held,
                binding_pose: vec![adjusted, Transform::from_xyz(0., 10., 19.)],
                clip: None,
                late_binding: true,
            })
            .id();
        let position = world
            .run_system_once(
                move |rigs: Query<&Rig>, transforms: Query<(&Transform, Option<&ChildOf>)>| {
                    rigs.get(rig).unwrap().binding_attachment(head, &transforms)
                },
            )
            .unwrap()
            .unwrap();
        // The new animation adds a quarter turn; the script's existing quarter
        // turn still applies, while the displayed neck/head remain unchanged.
        assert!(position.distance(Vec3::new(100., 190., 19.)) < 0.0001);
        assert_eq!(*world.get::<Transform>(neck).unwrap(), adjusted);
        assert_eq!(*world.get::<Transform>(head).unwrap(), held_head);
    }

    #[test]
    fn mesh_names_do_not_redirect_skeletal_pose_updates() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        let root = world.spawn_empty().id();
        let rest = Transform::from_xyz(5., 2., 1.);
        let bone = world.spawn((Name::new("sheath"), rest, ChildOf(root))).id();
        let mesh = world
            .spawn((Name::new("sheath"), Transform::IDENTITY, ChildOf(bone)))
            .id();
        world.spawn((
            Name::new("sheath"),
            Transform::IDENTITY,
            Mesh3d::default(),
            ChildOf(mesh),
        ));
        // Skinned geometry is a sibling of the skeleton and may be visited first.
        let skinned = world
            .spawn((Name::new("sheath"), Transform::IDENTITY, ChildOf(root)))
            .id();
        world.spawn((Mesh3d::default(), ChildOf(skinned)));
        let names = world
            .run_system_once(
                move |children: Query<&Children>,
                      nodes: Query<(&Name, &Transform, &ChildOf)>,
                      meshes: Query<(), With<Mesh3d>>| {
                    super::super::field_pose::named_bones(root, &children, &nodes, &meshes)
                },
            )
            .unwrap();
        let &(entity, authored, parent) = &names["sheath"];
        assert_eq!((entity, authored, parent), (bone, rest, root));
        world.spawn(Rig {
            sampled: false,
            bones: vec![(entity, authored)],
            previous: vec![authored],
            from: vec![authored],
            presented: vec![authored],
            binding_pose: Vec::new(),
            clip: None,
            late_binding: false,
        });
        world.get_mut::<Transform>(bone).unwrap().translation = Vec3::ZERO;
        world.run_system_once(restore).unwrap();
        assert_eq!(*world.get::<Transform>(bone).unwrap(), rest);
        assert_eq!(*world.get::<Transform>(mesh).unwrap(), Transform::IDENTITY);
    }
}
