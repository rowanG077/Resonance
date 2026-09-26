//! Blend authored skeletal poses before script adjustments and secondary motion.
mod frame;
use super::field_view::{ActorPart, Art, Failures, State};
use super::sparse_animation::affine::{Helper, Locals, Pose};
use anyhow::{Context, Result};
use bevy::prelude::*;
use frame::Frame;
use std::collections::BTreeMap;

#[derive(Component)]
pub(super) struct Rig {
    /// At least one animated pose has been evaluated.
    pub(super) sampled: bool,
    bones: Vec<(Entity, Transform)>,
    previous: Vec<Frame>,
    from: Vec<Frame>,
    presented: Vec<Frame>,
    binding_pose: Vec<Frame>,
    bind_channels: Vec<u8>,
    authored_channels: Vec<u8>,
    clip: Option<(resonance_events::animation::AnimationSource, u32, u16, u32)>,
    late_binding: bool,
}

pub(super) fn bind(
    mut commands: Commands,
    art: Res<Art>,
    mut roots: Query<(Entity, &mut ActorPart), Without<Rig>>,
    children: Query<&Children>,
    nodes: Query<(&Transform, &bevy::gltf::GltfExtras)>,
    clips: Res<Assets<super::sparse_animation::Clip>>,
    mut failures: Failures,
) {
    for (root, mut part) in &mut roots {
        if !part.prepared || part.disabled {
            continue;
        }
        let result = (|| -> Result<Rig> {
            let model = art
                .models
                .get(&part.resource)
                .and_then(|parts| parts.get(part.part))
                .context("missing field animation model")?;
            let spec = &model.spec;
            let bones = super::sparse_animation::Binding::new(
                root,
                spec.bone_names.len(),
                &children,
                &nodes,
            )?
            .0;
            let mut rig = Rig::new(bones);
            for clip in &model.clips {
                for track in &clips
                    .get(clip)
                    .context("missing field animation clip")?
                    .0
                    .tracks
                {
                    *rig.bind_channels
                        .get_mut(usize::from(track.bone))
                        .context("field animation track exceeds skeleton")? = track.bind_channels.0;
                }
            }
            for (i, &(_, rest)) in rig.bones.iter().enumerate() {
                let frame = Frame::sample(rest.into(), 0, rig.bind_channels[i]);
                rig.previous[i] = frame;
                rig.from[i] = frame;
                rig.presented[i] = frame;
            }
            Ok(rig)
        })();
        match result {
            Ok(rig) => {
                commands.entity(root).insert(rig);
            }
            Err(error) => {
                part.disable();
                commands.entity(root).insert(Visibility::Hidden);
                if !failures.skip(
                    "field animation binding",
                    error.context(format!("actor {}", part.actor)),
                ) {
                    return;
                }
            }
        }
    }
}

/// Sparse clips omit unchanged channels, so restore every bone before sampling.
pub(super) fn restore(rigs: Query<&Rig>, mut nodes: Query<&mut Transform>) {
    for rig in &rigs {
        for &(entity, rest) in &rig.bones {
            if let Ok(mut transform) = nodes.get_mut(entity) {
                *transform = rest;
            }
        }
    }
}

/// Evaluate the original sparse curves before blending and native adjustments.
#[allow(clippy::too_many_arguments)] // Field pose resources and per-actor diagnostic recovery.
pub(super) fn sample(
    state: State,
    art: Res<Art>,
    clips: Res<Assets<super::sparse_animation::Clip>>,
    mut rigs: Query<(Entity, &mut ActorPart, &mut Rig)>,
    mut nodes: Query<&mut Transform>,
    mut affine: ResMut<Locals>,
    mut applied: ResMut<super::field_audit::Applied>,
    mut commands: Commands,
    mut failures: Failures,
) {
    let world = &state.get().events.world;
    for (root, mut part, mut rig) in &mut rigs {
        if part.disabled {
            continue;
        }
        rig.authored_channels.fill(0);
        let Some(index) = part.active_clip else {
            continue;
        };
        let result = (|| -> Result<_> {
            let animation = world
                .actors
                .get(&part.actor)
                .and_then(|actor| actor.animation.as_ref())
                .context("missing field actor animation")?;
            let model = art
                .models
                .get(&part.resource)
                .and_then(|parts| parts.get(part.part))
                .context("missing field animation model")?;
            let clip = &clips
                .get(
                    model
                        .clips
                        .get(index)
                        .context("unavailable field clip index")?,
                )
                .context("missing field animation clip")?
                .0;
            let time = animation.sample(
                world.tick,
                0,
                model
                    .spec
                    .clips
                    .get(index)
                    .context("missing field clip recipe")?
                    .duration_seconds
                    * resonance_content::ANIMATION_HZ,
            );
            for track in &clip.tracks {
                *rig.authored_channels
                    .get_mut(usize::from(track.bone))
                    .context("field animation track exceeds skeleton")? = track.channels().0;
            }
            super::sparse_animation::sample(
                &rig.bones,
                clip,
                time * resonance_content::animation::FRAME_HZ / resonance_content::ANIMATION_HZ,
                &mut nodes,
                &mut affine,
            )?;
            Ok(super::field_audit::Request::Animation {
                actor: part.actor,
                part: part.part,
                resource: animation.resource,
                slot: animation.slot,
            })
        })();
        match result {
            Ok(request) => applied.ack(request),
            Err(error) => {
                part.disable();
                commands.entity(root).insert(Visibility::Hidden);
                if !failures.skip(
                    "field animation sampling",
                    error.context(format!("actor {}", part.actor)),
                ) {
                    return;
                }
            }
        }
    }
}

impl Rig {
    pub(super) fn bone_at(&self, index: u16) -> Option<Entity> {
        self.bones
            .get(usize::from(index))
            .map(|&(entity, _)| entity)
    }

    pub(super) fn new(bones: Vec<(Entity, Transform)>) -> Self {
        let previous: Vec<_> = bones.iter().map(|(_, t)| Frame::from(*t)).collect();
        Self {
            sampled: false,
            bind_channels: vec![frame::TRS; bones.len()],
            authored_channels: vec![0; bones.len()],
            bones,
            from: previous.clone(),
            presented: previous.clone(),
            binding_pose: Vec::new(),
            previous,
            clip: None,
            late_binding: false,
        }
    }

    /// Search the authored node order, excluding geometry copies of bone names.
    pub(super) fn bone(
        &self,
        name: &str,
        names: &Query<&Name>,
    ) -> Result<Option<Entity>, bevy::ecs::query::QueryEntityError> {
        for &(entity, _) in &self.bones {
            if names.get(entity)?.as_str() == name {
                return Ok(Some(entity));
            }
        }
        Ok(None)
    }

    fn blend_bone(&mut self, index: usize, pose: &mut Frame, weight: f32, hold: bool) {
        if weight < 1. {
            *pose = self.from[index].mix(
                *pose,
                self.bones[index].1,
                self.bind_channels[index],
                weight,
            );
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
    pub(super) fn binding_locals(&self, transforms: &Helper) -> Option<BTreeMap<Entity, Pose>> {
        if self.binding_pose.is_empty() {
            return None;
        }
        self.bones
            .iter()
            .enumerate()
            .map(|(index, &(entity, _))| {
                Some((
                    entity,
                    transforms.adjusted(
                        entity,
                        self.binding_pose[index].pose,
                        self.presented[index].pose,
                        transforms.local(entity).ok()?,
                    ),
                ))
            })
            .collect()
    }

    /// Without dynamics, native adjustments can be replayed directly. Dynamics
    /// instead evaluate these binding locals before changing the drawn skeleton.
    pub(super) fn binding_attachment(&self, entity: Entity, transforms: &Helper) -> Option<Vec3> {
        transforms
            .global_with(entity, &self.binding_locals(transforms)?)
            .ok()
            .map(|world| world.translation())
    }
}

pub(super) fn blend(
    state: State,
    mut rigs: Query<(&ActorPart, &mut Rig)>,
    mut nodes: Query<&mut Transform>,
    mut affine: ResMut<Locals>,
) {
    let world = &state.get().events.world;
    for (part, mut rig) in &mut rigs {
        let actor = world.actors.get(&part.actor);
        let animation = actor.and_then(|a| a.animation.as_ref());
        let key = animation.map(|a| (a.source, a.resource, a.slot, a.start_tick));
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
                let entity = rig.bones[i].0;
                let mut pose = Frame::sample(
                    affine.get(entity, *transform),
                    rig.authored_channels[i],
                    rig.bind_channels[i],
                );
                rig.blend_bone(i, &mut pose, weight, hold);
                affine.set(entity, &mut transform, pose.pose);
            }
        }
        rig.sampled = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matrix_transitions_snap_but_binding_holds_keep_the_complete_pose() {
        let entity = Entity::PLACEHOLDER;
        let mut rig = Rig::new(vec![(entity, Transform::IDENTITY)]);
        let from = Pose::Affine(bevy::math::Affine3A::from_cols(
            Vec3::X.into(),
            Vec3::new(0.5, 1., 0.).into(),
            Vec3::Z.into(),
            Vec3::Y.into(),
        ));
        let to = Pose::Affine(bevy::math::Affine3A::from_translation(Vec3::X));
        let from = Frame::sample(from, 16, 0);
        let to = Frame::sample(to, 16, 0);
        rig.previous[0] = from;
        rig.from[0] = from;
        rig.presented[0] = from;
        let mut pose = to;
        rig.blend_bone(0, &mut pose, 0.25, true);
        assert_eq!(pose, from);
        assert_eq!(rig.binding_pose, [to]);
        assert_eq!(rig.previous, [from]);
        pose = to;
        rig.blend_bone(0, &mut pose, 0.25, false);
        assert_eq!(pose, to);
    }

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
                previous: vec![old.into()],
                from: vec![old.into()],
                presented: vec![old.into()],
                binding_pose: Vec::new(),
                bind_channels: vec![frame::TRS],
                authored_channels: vec![0],
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
        let Pose::Trs(middle) = Pose::from(old).mix(rest.into(), 0.5) else {
            panic!()
        };
        assert_eq!(middle.translation, Vec3::new(6., 2., 0.));
        assert!(middle.rotation.angle_between(Quat::from_rotation_z(0.5)) < 0.001);
        let Pose::Trs(end) = Pose::from(old).mix(rest.into(), 1.) else {
            panic!()
        };
        assert_eq!(end.translation, Vec3::ZERO);
        assert!(end.rotation.angle_between(Quat::IDENTITY) < 0.001);

        // A binding during this unfinished blend holds the actual visible
        // midpoint, but its later cross-fade still starts at the completed pose.
        let mut rig = world.get_mut::<Rig>(rig).unwrap();
        let mut pose = Frame::from(rest);
        rig.blend_bone(0, &mut pose, 0.5, false);
        assert_eq!(pose, middle.into());
        let next = Transform::from_xyz(-8., 2., 6.);
        rig.from = rig.previous.clone();
        for _ in 0..2 {
            pose = next.into();
            rig.blend_bone(0, &mut pose, 0.25, true);
            assert_eq!(pose, middle.into());
        }
        assert_eq!(rig.previous, vec![old.into()]);
        pose = next.into();
        rig.blend_bone(0, &mut pose, 0.25, false);
        assert_eq!(
            pose,
            Frame::from(old).mix(next.into(), rest, frame::TRS, 0.25)
        );
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
        let held = vec![Frame::from(Transform::IDENTITY), held_head.into()];
        let rig = world
            .spawn(Rig {
                sampled: true,
                bones: vec![(neck, Transform::IDENTITY), (head, held_head)],
                previous: held.clone(),
                from: held.clone(),
                presented: held,
                binding_pose: vec![adjusted.into(), Transform::from_xyz(0., 10., 19.).into()],
                bind_channels: vec![frame::TRS; 2],
                authored_channels: vec![0; 2],
                clip: None,
                late_binding: true,
            })
            .id();
        let position = world
            .run_system_once(move |rigs: Query<&Rig>, transforms: Helper| {
                rigs.get(rig).unwrap().binding_attachment(head, &transforms)
            })
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
            previous: vec![authored.into()],
            from: vec![authored.into()],
            presented: vec![authored.into()],
            binding_pose: Vec::new(),
            bind_channels: vec![frame::TRS],
            authored_channels: vec![0],
            clip: None,
            late_binding: false,
        });
        world.get_mut::<Transform>(bone).unwrap().translation = Vec3::ZERO;
        world.run_system_once(restore).unwrap();
        assert_eq!(*world.get::<Transform>(bone).unwrap(), rest);
        assert_eq!(*world.get::<Transform>(mesh).unwrap(), Transform::IDENTITY);
    }
}
