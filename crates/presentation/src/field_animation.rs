//! Blend authored skeletal poses before script adjustments and secondary motion.
use super::field_view::{ActorPart, Art, State};
use bevy::prelude::*;

#[derive(Component)]
pub(super) struct Rig {
    /// At least one animated pose has reached transform propagation.
    pub(super) sampled: bool,
    bones: Vec<(Entity, Transform)>,
    previous: Vec<Transform>,
    from: Vec<Transform>,
    clip: Option<(u32, u16, u32)>,
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
            previous,
            clip: None,
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

pub(super) fn blend(
    state: State,
    mut rigs: Query<(&ActorPart, &mut Rig)>,
    mut nodes: Query<&mut Transform>,
) {
    let world = &state.get().events.world;
    for (part, mut rig) in &mut rigs {
        let animation = world
            .actors
            .get(&part.actor)
            .and_then(|a| a.animation.as_ref());
        let key = animation.map(|a| (a.resource, a.slot, a.start_tick));
        if key != rig.clip {
            rig.from = rig.previous.clone();
            rig.clip = key;
        }
        let weight = animation.map_or(1., |a| a.blend_weight(world.tick));
        for i in 0..rig.bones.len() {
            if let Ok(mut transform) = nodes.get_mut(rig.bones[i].0) {
                if weight < 1. {
                    *transform = mix(rig.from[i], *transform, weight);
                }
                // An interrupted blend keeps its original source pose. Cache
                // only completed poses, before mouth, cloth and bone overrides.
                if weight >= 1. {
                    rig.previous[i] = *transform;
                }
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
        world.spawn(Rig {
            sampled: false,
            bones: vec![(bone, rest)],
            previous: vec![old],
            from: vec![old],
            clip: None,
        });
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
            clip: None,
        });
        world.get_mut::<Transform>(bone).unwrap().translation = Vec3::ZERO;
        world.run_system_once(restore).unwrap();
        assert_eq!(*world.get::<Transform>(bone).unwrap(), rest);
        assert_eq!(*world.get::<Transform>(mesh).unwrap(), Transform::IDENTITY);
    }
}
