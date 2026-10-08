//! Blend skeletal poses before script adjustments and secondary motion.
#[cfg(test)]
mod camera_tests;
#[cfg(test)]
mod continuity_tests;
mod frame;
use super::field_view::{ActorPart, Art, State};
use super::sparse_animation::affine::{Locals, Pose};
use bevy::prelude::*;
use frame::Frame;

#[derive(Component)]
pub(super) struct Rig {
    /// At least one animated pose has been evaluated.
    pub(super) sampled: bool,
    bones: Vec<(Entity, Transform)>,
    camera_facing: Vec<Entity>,
    from: Vec<Frame>,
    presented: Vec<Frame>,
    bind_channels: Vec<u8>,
    authored_channels: Vec<u8>,
    clip: Option<(resonance_events::animation::AnimationSource, u32, u16, u32)>,
}

pub(super) fn bind(
    mut commands: Commands,
    art: Res<Art>,
    roots: Query<(Entity, &ActorPart), Without<Rig>>,
    children: Query<&Children>,
    nodes: Query<(&Transform, &bevy::gltf::GltfExtras)>,
    clips: Res<Assets<super::sparse_animation::Clip>>,
) {
    for (root, part) in &roots {
        if !part.prepared {
            continue;
        }
        let spec = &art.models[&part.resource][part.part].spec;
        let mut rig = Rig::from_scene(root, spec.bone_names.len(), &children, &nodes)
            .expect("prepared animation skeleton must contain every bone");
        for clip in &art.models[&part.resource][part.part].clips {
            for track in &clips.get(clip).expect("prepared sparse clip").0.tracks {
                rig.bind_channels[usize::from(track.bone)] = track.bind_channels.0;
            }
        }
        for (i, &(_, rest)) in rig.bones.iter().enumerate() {
            let frame = Frame::sample(rest.into(), 0, rig.bind_channels[i]);
            rig.from[i] = frame;
            rig.presented[i] = frame;
        }
        commands.entity(root).insert(rig);
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

/// Resolve the displayed clip and its sampling time.
fn sample_clip<'a>(
    model: &'a resonance_content::ScenePart,
    handles: &[Handle<super::sparse_animation::Clip>],
    clips: &'a Assets<super::sparse_animation::Clip>,
    animation: &resonance_events::Animation,
    resource: u32,
    tick: u32,
    delay: f32,
) -> Option<(&'a resonance_content::animation::Motion, f32, &'a [u16])> {
    let index = model
        .clips
        .iter()
        .position(|clip| animation.matches(clip, resource))?;
    let spec = &model.clips[index];
    let duration = spec.duration_seconds * resonance_content::ANIMATION_HZ;
    let mut time = animation.sample(tick, 0, duration);
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
    Some((
        &clips.get(&handles[index]).expect("prepared sparse clip").0,
        time * resonance_content::animation::FRAME_HZ / resonance_content::ANIMATION_HZ,
        &spec.secondary_pose_nodes,
    ))
}

/// Evaluate sparse curves before blending and script adjustments.
pub(super) fn sample(
    state: State,
    art: Res<Art>,
    clips: Res<Assets<super::sparse_animation::Clip>>,
    mut rigs: Query<(&ActorPart, &mut Rig)>,
    mut nodes: Query<&mut Transform>,
    mut affine: ResMut<Locals>,
    mut applied: ResMut<super::field_audit::Applied>,
) {
    let world = &state.get().events.world;
    for (part, mut rig) in &mut rigs {
        rig.authored_channels.fill(0);
        let actor = &world.actors[&part.actor];
        let model = &art.models[&part.resource][part.part];
        for animation in actor
            .animation
            .iter()
            .chain(actor.scenery_animations.values())
        {
            let delay = actor.wings.as_ref().map_or(0., |w| {
                if let Some(entrance) =
                    w.entrance(part.pass, world.tick.saturating_sub(animation.start_tick))
                {
                    entrance.delay
                } else {
                    w.layer(part.pass, world.effect_tick).pose_delay
                }
            });
            let Some((clip, mut time, _)) = sample_clip(
                &model.spec,
                &model.clips,
                &clips,
                animation,
                actor.resource,
                world.tick,
                delay,
            ) else {
                continue;
            };
            let echo_entrance = actor
                .wings
                .as_ref()
                .and_then(|w| w.layer(part.pass, world.effect_tick).echo)
                .and_then(|echo| echo.entrance_weight(world.tick));
            if echo_entrance.is_some() {
                time = 0.;
            }
            for (i, mut pose) in rig
                .sample_tracks(clip, time)
                .expect("validated animation must evaluate")
            {
                let entity = rig.bones[i].0;
                if let Some(weight) = echo_entrance {
                    let rest = rig.bones[i].1;
                    pose = Frame::sample(rest.into(), 0, rig.bind_channels[i]).mix(
                        pose,
                        rest,
                        rig.bind_channels[i],
                        weight,
                    );
                }
                if let Ok(mut transform) = nodes.get_mut(entity) {
                    affine.set(entity, &mut transform, pose.pose);
                }
            }
            applied.ack(super::field_audit::Request::Animation {
                actor: part.actor,
                part: part.part,
                resource: animation.resource,
                slot: animation.slot,
            });
        }
    }
}

impl Rig {
    fn from_scene(
        root: Entity,
        count: usize,
        children: &Query<&Children>,
        nodes: &Query<(&Transform, &bevy::gltf::GltfExtras)>,
    ) -> anyhow::Result<Self> {
        let bones = super::sparse_animation::Binding::new(root, count, children, nodes)?.0;
        let mut rig = Self::new(bones);
        for &(entity, _) in &rig.bones {
            let extras: serde_json::Value = serde_json::from_str(&nodes.get(entity)?.1.value)?;
            if extras["resonance_camera_facing"] == true {
                rig.camera_facing.push(entity);
            }
        }
        Ok(rig)
    }

    pub(super) fn bind_scale(&self, entity: Entity) -> Option<Vec3> {
        self.bones
            .iter()
            .find(|(id, _)| *id == entity)
            .map(|(_, t)| t.scale)
    }

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
            camera_facing: Vec::new(),
            from: previous.clone(),
            presented: previous.clone(),
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

    fn sample_tracks(
        &mut self,
        motion: &resonance_content::animation::Motion,
        time: f32,
    ) -> anyhow::Result<Vec<(usize, Frame)>> {
        motion
            .tracks
            .iter()
            .map(|track| {
                let i = usize::from(track.bone);
                self.authored_channels[i] |= track.channels().0;
                Ok((
                    i,
                    Frame::sample(
                        super::sparse_animation::sample_track(track, time, self.bones[i].1)?,
                        self.authored_channels[i],
                        self.bind_channels[i],
                    ),
                ))
            })
            .collect()
    }

    fn blend_bone(&mut self, index: usize, pose: &mut Frame, weight: f32, animated: bool) {
        if !animated {
            *pose = self.presented[index];
        } else if weight < 1. {
            *pose = self.from[index].mix(
                *pose,
                self.bones[index].1,
                self.bind_channels[index],
                weight,
            );
        }
        self.presented[index] = *pose;
    }
}

pub(super) fn face_camera(
    state: State,
    rigs: Query<&Rig>,
    mut nodes: Query<&mut Transform>,
    mut affine: ResMut<Locals>,
) {
    let Some(camera) = &state.get().events.world.field_camera else {
        return;
    };
    let rotation = super::field_view::camera_transform(camera).rotation;
    for rig in &rigs {
        for &entity in &rig.camera_facing {
            if let Ok(mut transform) = nodes.get_mut(entity) {
                affine.face_camera(entity, &mut transform, rotation);
            }
        }
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
        let actor = &world.actors[&part.actor];
        let animation = actor.animation.as_ref();
        let key = animation.map(|a| (a.source, a.resource, a.slot, a.start_tick));
        if key != rig.clip {
            rig.from = rig.presented.clone();
            rig.clip = key;
        }
        let weight = animation.map_or(1., |a| {
            actor
                .wings
                .as_ref()
                .and_then(|w| w.entrance(part.pass, world.tick.saturating_sub(a.start_tick)))
                .map(|entrance| entrance.weight)
                .unwrap_or_else(|| a.blend_weight(world.tick))
        });
        for i in 0..rig.bones.len() {
            let entity = rig.bones[i].0;
            if let Ok(mut transform) = nodes.get_mut(entity) {
                let mut pose = Frame::sample(
                    affine.get(entity, *transform),
                    rig.authored_channels[i],
                    rig.bind_channels[i],
                );
                let animated = rig.authored_channels[i] != 0;
                rig.blend_bone(i, &mut pose, weight, animated);
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
    fn interrupted_blends_start_from_the_visible_pose_and_omitted_bones_stay_put() {
        let mut rig = Rig::new(vec![(Entity::PLACEHOLDER, Transform::IDENTITY)]);
        let target = Frame::from(Transform::from_xyz(10., 0., 0.));
        let mut pose = target;
        rig.blend_bone(0, &mut pose, 0.5, true);
        assert_eq!(pose.pose.global().translation().x, 5.);
        rig.from = rig.presented.clone();
        pose = Frame::from(Transform::from_xyz(-5., 0., 0.));
        rig.blend_bone(0, &mut pose, 0.5, true);
        assert_eq!(pose.pose.global().translation().x, 0.);
        rig.blend_bone(0, &mut pose, 1., false);
        assert_eq!(pose.pose.global().translation().x, 0.);
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
        world.spawn(Rig::new(vec![(entity, authored)]));
        world.get_mut::<Transform>(bone).unwrap().translation = Vec3::ZERO;
        world.run_system_once(restore).unwrap();
        assert_eq!(*world.get::<Transform>(bone).unwrap(), rest);
        assert_eq!(*world.get::<Transform>(mesh).unwrap(), Transform::IDENTITY);
    }
}
