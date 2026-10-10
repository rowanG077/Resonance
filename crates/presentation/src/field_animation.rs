//! Blend authored skeletal poses before script adjustments and secondary motion.
use super::field_view::{ActorPart, Art, Failures, State};
use super::sparse_animation::affine::{Locals, Pose};
use anyhow::{Context, Result};
use bevy::prelude::*;

#[derive(Component)]
pub(super) struct Rig {
    /// This update successfully sampled and published an animated pose.
    pub(super) sampled: bool,
    bones: Vec<(Entity, Transform)>,
    camera_facing: Vec<Entity>,
    previous: Vec<Pose>,
    from: Vec<Pose>,
    clip: Option<(resonance_events::animation::AnimationSource, u32, u16, u32)>,
}

pub(super) fn bind(
    mut commands: Commands,
    art: Res<Art>,
    mut roots: Query<(Entity, &mut ActorPart), Without<Rig>>,
    children: Query<&Children>,
    nodes: Query<(&Transform, &bevy::gltf::GltfExtras)>,
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
            Rig::from_scene(root, spec.bone_names.len(), &children, &nodes)
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

/// Sample and blend locally, then publish one pose before later adjustments.
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
        rig.sampled = false;
        if part.disabled {
            continue;
        }
        let result = (|| -> Result<Vec<_>> {
            let actor = world
                .actors
                .get(&part.actor)
                .context("missing animated actor")?;
            if actor.animation.is_none() && actor.scenery_animations.is_empty() {
                rig.restore(&mut nodes, &mut affine)?;
                return Ok(Vec::new());
            }
            let model = art
                .models
                .get(&part.resource)
                .and_then(|parts| parts.get(part.part))
                .context("missing field animation model")?;
            let weight = actor.animation.as_ref().map_or(1., |animation| {
                let weight = rig.begin_blend(animation, world.tick);
                actor
                    .wings
                    .as_ref()
                    .and_then(|w| {
                        w.entrance(part.pass, world.tick.saturating_sub(animation.start_tick))
                    })
                    .map_or(weight, |entrance| entrance.weight)
            });
            let mut poses = rig.previous.clone();
            let mut requests = Vec::new();
            for animation in actor
                .animation
                .iter()
                .chain(actor.scenery_animations.values())
            {
                let Some(index) = model
                    .spec
                    .clips
                    .iter()
                    .position(|clip| animation.matches(clip, actor.resource))
                else {
                    continue;
                };
                let clip = &clips
                    .get(&model.clips[index])
                    .context("missing field animation clip")?
                    .0;
                let duration =
                    model.spec.clips[index].duration_seconds * resonance_content::ANIMATION_HZ;
                let mut time = animation.sample(world.tick, 0, duration);
                let echo = actor
                    .wings
                    .as_ref()
                    .map(|w| w.layer(part.pass, world.effect_tick));
                let delay = actor.wings.as_ref().map_or(0., |w| {
                    w.entrance(part.pass, world.tick.saturating_sub(animation.start_tick))
                        .map_or_else(
                            || w.layer(part.pass, world.effect_tick).pose_delay,
                            |entrance| entrance.delay,
                        )
                });
                if delay != 0. && duration > 0. {
                    time -= delay;
                    if time < 0. || time > duration {
                        let phase = time.rem_euclid(duration);
                        time = if phase == 0. && time > 0. {
                            duration
                        } else {
                            phase
                        };
                    }
                }
                let entrance_weight = echo
                    .and_then(|layer| layer.echo)
                    .and_then(|echo| echo.entrance_weight(world.tick));
                if entrance_weight.is_some() {
                    time = 0.;
                }
                poses = super::sparse_animation::sample(
                    &rig.bones,
                    clip,
                    time * resonance_content::animation::FRAME_HZ / resonance_content::ANIMATION_HZ,
                    Some(&poses),
                )?;
                if let Some(weight) = entrance_weight {
                    for (pose, &(_, rest)) in poses.iter_mut().zip(&rig.bones) {
                        *pose = Pose::from(rest).mix(*pose, weight);
                    }
                }
                requests.push(super::field_audit::Request::Animation {
                    actor: part.actor,
                    part: part.part,
                    resource: animation.resource,
                    slot: animation.slot,
                });
            }
            rig.publish(poses, weight, &mut nodes, &mut affine)?;
            if let Some(camera) = &world.field_camera {
                rig.face_camera(
                    super::field_view::camera_transform(camera).rotation,
                    &mut nodes,
                    &mut affine,
                )?;
            }
            Ok(requests)
        })();
        match result {
            Ok(requests) => {
                for request in requests {
                    applied.ack(request);
                }
            }
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
    fn from_scene(
        root: Entity,
        count: usize,
        children: &Query<&Children>,
        nodes: &Query<(&Transform, &bevy::gltf::GltfExtras)>,
    ) -> Result<Self> {
        let mut rig =
            Self::new(super::sparse_animation::Binding::new(root, count, children, nodes)?.0);
        for &(entity, _) in &rig.bones {
            let extras: serde_json::Value = serde_json::from_str(&nodes.get(entity)?.1.value)?;
            if extras["resonance_camera_facing"] == true {
                rig.camera_facing.push(entity);
            }
        }
        Ok(rig)
    }
    fn face_camera(
        &self,
        rotation: Quat,
        nodes: &mut Query<&mut Transform>,
        affine: &mut Locals,
    ) -> Result<()> {
        for &entity in &self.camera_facing {
            let mut transform = nodes.get_mut(entity)?;
            affine.face_camera(entity, &mut transform, rotation);
        }
        Ok(())
    }

    pub(super) fn bind_scale(&self, entity: Entity) -> Option<Vec3> {
        self.bones
            .iter()
            .find(|(bone, _)| *bone == entity)
            .map(|(_, rest)| rest.scale)
    }

    pub(super) fn bone_at(&self, index: u16) -> Option<Entity> {
        self.bones
            .get(usize::from(index))
            .map(|&(entity, _)| entity)
    }

    pub(super) fn new(bones: Vec<(Entity, Transform)>) -> Self {
        let previous: Vec<_> = bones.iter().map(|(_, t)| Pose::from(*t)).collect();
        Self {
            sampled: false,
            bones,
            camera_facing: Vec::new(),
            from: previous.clone(),
            previous,
            clip: None,
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

    fn begin_blend(&mut self, animation: &resonance_events::Animation, tick: u32) -> f32 {
        let key = Some((
            animation.source,
            animation.resource,
            animation.slot,
            animation.start_tick,
        ));
        if key != self.clip {
            self.from.clone_from(&self.previous);
            self.clip = key;
        }
        animation.blend_weight(tick)
    }

    fn restore(&mut self, nodes: &mut Query<&mut Transform>, affine: &mut Locals) -> Result<()> {
        self.sampled = false;
        let rest = self
            .bones
            .iter()
            .map(|&(_, rest)| Pose::from(rest))
            .collect::<Vec<_>>();
        super::sparse_animation::publish(&self.bones, &rest, nodes, affine)?;
        self.previous = rest;
        self.clip = None;
        Ok(())
    }

    #[cfg(test)]
    fn sample(
        &mut self,
        motion: &resonance_content::animation::Motion,
        frame: f32,
        weight: f32,
        nodes: &mut Query<&mut Transform>,
        affine: &mut Locals,
    ) -> Result<()> {
        self.sampled = false;
        let poses =
            super::sparse_animation::sample(&self.bones, motion, frame, Some(&self.previous))?;
        self.publish(poses, weight, nodes, affine)
    }

    fn publish(
        &mut self,
        mut poses: Vec<Pose>,
        weight: f32,
        nodes: &mut Query<&mut Transform>,
        affine: &mut Locals,
    ) -> Result<()> {
        for (pose, &from) in poses.iter_mut().zip(&self.from) {
            *pose = from.mix(*pose, weight);
        }
        super::sparse_animation::publish(&self.bones, &poses, nodes, affine)?;
        self.previous = poses;
        self.sampled = true;
        Ok(())
    }
}

#[cfg(test)]
mod camera_tests;
#[cfg(test)]
mod continuity_tests;
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rendered_attachment_uses_the_same_blended_skeleton() {
        use super::super::sparse_animation::affine::Helper;
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        world.insert_resource(Locals::default());
        let root = world.spawn(Transform::from_xyz(100., 0., 0.)).id();
        let bone = world
            .spawn((Transform::from_xyz(10., 0., 0.), ChildOf(root)))
            .id();
        let mut rig = Rig::new(vec![(bone, Transform::IDENTITY)]);
        rig.previous[0] = Transform::from_xyz(10., 0., 0.).into();
        let rig_entity = world.spawn(rig).id();
        let motion: resonance_content::animation::Motion =
            serde_json::from_value(serde_json::json!({
                "duration_frames":30., "tracks":[{
                    "bone":0, "bind_channels":0, "period_frames":30., "times":[0.],
                    "translation":{"interpolation":"linear","values":[[30.,0.,0.]]}
                }]
            }))
            .unwrap();
        let motion = std::sync::Arc::new(motion);
        let animation = resonance_events::Animation {
            blend_ticks: 4,
            ..resonance_events::Animation::new(1, 12, 60, 10)
        };
        for (tick, interrupted, expected) in [
            (10, false, 10.),
            (12, false, 20.),
            (12, true, 20.),
            (12, true, 20.),
            (14, true, 25.),
            (16, true, 30.),
        ] {
            let motion = motion.clone();
            let mut animation = animation.clone();
            if interrupted {
                animation.start_tick = 12;
            }
            world
                .run_system_once(
                    move |mut rigs: Query<&mut Rig>,
                          mut nodes: Query<&mut Transform>,
                          mut affine: ResMut<Locals>| {
                        let mut rig = rigs.get_mut(rig_entity).unwrap();
                        let weight = rig.begin_blend(&animation, tick);
                        rig.sample(
                            &motion,
                            animation.sample(tick, 0, 60.) * 0.5,
                            weight,
                            &mut nodes,
                            &mut affine,
                        )
                        .unwrap();
                        assert!(rig.sampled);
                    },
                )
                .unwrap();
            assert_eq!(
                world.get::<Transform>(bone).unwrap().translation.x,
                expected
            );
            let attachment = world
                .run_system_once(move |helper: Helper| {
                    helper.compute_global_transform(bone).unwrap().translation()
                })
                .unwrap();
            assert_eq!(attachment.x, 100. + expected);
        }
        // Stopped playback restores rest without claiming an animated sample.
        let restored_motion = motion.clone();
        world
            .run_system_once(
                move |mut rigs: Query<&mut Rig>,
                      mut nodes: Query<&mut Transform>,
                      mut affine: ResMut<Locals>| {
                    let mut rig = rigs.get_mut(rig_entity).unwrap();
                    rig.restore(&mut nodes, &mut affine).unwrap();
                    assert!(!rig.sampled);
                    assert_eq!(*nodes.get(bone).unwrap(), Transform::IDENTITY);
                    rig.sample(&restored_motion, 0., 1., &mut nodes, &mut affine)
                        .unwrap();
                },
            )
            .unwrap();
        world.despawn(bone);
        world
            .run_system_once(
                move |mut rigs: Query<&mut Rig>,
                      mut nodes: Query<&mut Transform>,
                      mut affine: ResMut<Locals>| {
                    let mut rig = rigs.get_mut(rig_entity).unwrap();
                    assert!(
                        rig.sample(&motion, 0., 1., &mut nodes, &mut affine)
                            .is_err()
                    );
                    assert!(!rig.sampled);
                },
            )
            .unwrap();
    }

    #[test]
    fn sparse_tracks_retain_unwritten_channels_and_untracked_bones_during_blending() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        world.init_resource::<Locals>();
        let rest = Transform::from_xyz(2., 4., 6.)
            .with_scale(Vec3::new(2., 3., 4.))
            .with_rotation(Quat::from_rotation_z(0.4));
        let bones = [world.spawn(rest).id(), world.spawn(rest).id()];
        let mut rig = Rig::new(bones.into_iter().map(|bone| (bone, rest)).collect());
        let previous = bevy::math::Affine3A::from_cols(
            Vec3::X.into(),
            Vec3::new(0.5, 2., 0.).into(),
            Vec3::Z.into(),
            Vec3::new(12., 4., 0.).into(),
        );
        let old = [
            Pose::Affine(previous),
            Transform::from_rotation(Quat::from_rotation_z(1.))
                .with_scale(Vec3::splat(5.))
                .into(),
        ];
        rig.from.clone_from_slice(&old);
        rig.previous.clone_from_slice(&old);
        let rig_entity = world.spawn(rig).id();
        let motion: resonance_content::animation::Motion =
            serde_json::from_value(serde_json::json!({
                "duration_frames":30., "tracks":[{
                    "bone":0, "bind_channels":0, "period_frames":30., "times":[0.],
                    "translation":{"interpolation":"linear","values":[[32.,8.,10.]]}
                }]
            }))
            .unwrap();
        let mut translated = previous;
        translated.translation = Vec3::new(32., 8., 10.).into();
        let targets = [Pose::Affine(translated), old[1]];
        for weight in [0., 0.5, 1.] {
            let motion = motion.clone();
            let poses = world
                .run_system_once(
                    move |mut rigs: Query<&mut Rig>,
                          mut nodes: Query<&mut Transform>,
                          mut affine: ResMut<Locals>| {
                        let mut rig = rigs.get_mut(rig_entity).unwrap();
                        rig.sample(&motion, 0., weight, &mut nodes, &mut affine)
                            .unwrap();
                        rig.previous.clone()
                    },
                )
                .unwrap();
            assert_eq!(poses, if weight < 1. { old } else { targets });
            assert_eq!(poses[0].global().affine().matrix3, previous.matrix3);
        }
    }

    #[test]
    fn mesh_names_do_not_redirect_skeletal_pose_updates() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        world.init_resource::<Locals>();
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
    }
}
