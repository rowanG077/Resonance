//! Blend skeletal poses before script adjustments and secondary motion.
#[cfg(test)]
mod camera_tests;
#[cfg(test)]
mod continuity_tests;
mod frame;
use super::field_view::{ActorPart, Art, Part, State};
use super::sparse_animation::affine::{Locals, Pose};
use bevy::prelude::*;
use frame::Frame;
use resonance_events::{Actor, Animation, Wings};
use std::collections::BTreeSet;

#[derive(Component)]
pub(super) struct Rig {
    binding_key: std::sync::Arc<()>,
    /// At least one animated pose has been evaluated.
    pub(super) sampled: bool,
    bones: Vec<(Entity, Transform)>,
    camera_facing: Vec<Entity>,
    secondary_bones: BTreeSet<usize>,
    from: Vec<Frame>,
    presented: Vec<Frame>,
    authored_channels: Vec<u8>,
    clip: Option<(resonance_events::animation::AnimationSource, u32, u16, u32)>,
}

pub(super) fn bind(
    mut commands: Commands,
    state: State,
    art: Res<Art>,
    roots: Query<(Entity, &ActorPart, &crate::model_asset::Instance), Without<Rig>>,
) {
    for (root, part, model) in &roots {
        if !part.prepared {
            continue;
        }
        let artwork = &art.models[&part.resource][part.part];
        let spec = &artwork.spec;
        let mut rig = Rig::new(
            model
                .binding(spec.bone_names.len())
                .expect("prepared animation skeleton must contain every bone")
                .0,
        );
        rig.camera_facing = model.facing();
        rig.secondary_bones.extend(
            spec.secondary_motion
                .prepare(&spec.bone_names)
                .expect("prepared dynamics must reference existing bones")
                .into_iter()
                .flat_map(|chain| {
                    chain
                        .joints
                        .into_iter()
                        .map(|joint| usize::from(joint.node))
                }),
        );
        if let Some(start) = state.get().events.world.field_model_start(part.actor) {
            let sample = ActorSample {
                resource: start.resource,
                wings: start.wings.as_ref(),
                tick: start.tick,
                effect_tick: start.effect_tick,
                pass: part.pass,
            };
            let weight = sample.wing_weight(start.animation.as_ref()).unwrap_or(1.);
            for clip in start.animation.iter().chain(&start.scenery_animations) {
                if let Some(tracks) = sample.tracks(&mut rig, artwork, clip) {
                    for (bone, mut pose) in tracks {
                        rig.blend_bone(bone, &mut pose, weight, true);
                    }
                }
            }
            rig.sampled = rig.authored_channels.iter().any(|&channels| channels != 0);
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
    model: &'a Part,
    animation: &Animation,
    resource: u32,
    tick: u32,
    delay: f32,
) -> Option<(&'a resonance_content::animation::Motion, f32)> {
    let index = model
        .spec
        .clips
        .iter()
        .position(|clip| animation.matches(clip, resource))?;
    let spec = &model.spec.clips[index];
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
        &model.motions[index],
        time * resonance_content::animation::FRAME_HZ / resonance_content::ANIMATION_HZ,
    ))
}

#[derive(Clone, Copy)]
struct ActorSample<'a> {
    resource: u32,
    wings: Option<&'a Wings>,
    tick: u32,
    effect_tick: u32,
    pass: u8,
}

impl<'a> ActorSample<'a> {
    fn current(actor: &'a Actor, world: &resonance_events::GameWorld, pass: u8) -> Self {
        Self {
            resource: actor.resource,
            wings: actor.wings.as_ref(),
            tick: world.tick,
            effect_tick: world.effect_tick,
            pass,
        }
    }

    fn wing_weight(self, animation: Option<&Animation>) -> Option<f32> {
        self.wings?
            .entrance(self.pass, self.tick.saturating_sub(animation?.start_tick))
            .map(|entrance| entrance.weight)
    }

    fn tracks(
        self,
        rig: &mut Rig,
        model: &Part,
        animation: &Animation,
    ) -> Option<Vec<(usize, Frame)>> {
        let layer = self.wings.map(|w| w.layer(self.pass, self.effect_tick));
        let delay = self
            .wings
            .and_then(|w| w.entrance(self.pass, self.tick.saturating_sub(animation.start_tick)))
            .map_or_else(
                || layer.as_ref().map_or(0., |layer| layer.pose_delay),
                |entrance| entrance.delay,
            );
        let (clip, mut time) = sample_clip(model, animation, self.resource, self.tick, delay)?;
        let echo = layer
            .and_then(|layer| layer.echo)
            .and_then(|echo| echo.entrance_weight(self.tick));
        if echo.is_some() {
            time = 0.;
        }
        let mut tracks = rig
            .sample_tracks(clip, time)
            .expect("validated animation must evaluate");
        if let Some(weight) = echo {
            for (i, pose) in &mut tracks {
                let rest = rig.bones[*i].1;
                *pose = Frame::sample(rest.into(), 0).mix(*pose, rest, weight);
            }
        }
        Some(tracks)
    }
}

/// Evaluate sparse curves before blending and script adjustments.
pub(super) fn sample(
    state: State,
    art: Res<Art>,
    mut rigs: Query<(&ActorPart, &mut Rig)>,
    mut nodes: Query<&mut Transform>,
    mut affine: ResMut<Locals>,
    mut applied: ResMut<super::field_audit::Applied>,
) {
    let world = &state.get().events.world;
    for (part, mut rig) in &mut rigs {
        rig.authored_channels.fill(0);
        let actor = world.field_model_actor(part.actor).unwrap();
        let sample = ActorSample::current(actor, world, part.pass);
        let model = &art.models[&part.resource][part.part];
        for animation in actor
            .field_model_animation()
            .into_iter()
            .chain(actor.scenery_animations.values())
        {
            let Some(tracks) = sample.tracks(&mut rig, model, animation) else {
                continue;
            };
            for (i, pose) in tracks {
                let entity = rig.bones[i].0;
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
    #[cfg(test)]
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

    /// Identity of the skeleton binding, independent of sampled animation.
    pub(super) fn binding_key(&self) -> &std::sync::Arc<()> {
        &self.binding_key
    }

    pub(super) fn bones(&self) -> &[(Entity, Transform)] {
        &self.bones
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
        let rest: Vec<_> = bones
            .iter()
            .map(|&(_, transform)| Frame::sample(transform.into(), 0))
            .collect();
        Self {
            binding_key: Default::default(),
            sampled: false,
            authored_channels: vec![0; bones.len()],
            bones,
            camera_facing: Vec::new(),
            secondary_bones: BTreeSet::new(),
            from: rest.clone(),
            presented: rest,
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
                    ),
                ))
            })
            .collect()
    }

    fn start_blend(&mut self) {
        self.from.copy_from_slice(&self.presented);
    }

    fn blend_bone(&mut self, index: usize, pose: &mut Frame, weight: f32, animated: bool) {
        // Cloth and hair resume their rest target when explicit motion ends;
        // carrying a previous clip's bend would pin the dynamics to that pose.
        if !animated && !self.secondary_bones.contains(&index) {
            *pose = self.presented[index];
        } else if weight < 1. {
            *pose = self.from[index].mix(*pose, self.bones[index].1, weight);
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
        let actor = world.field_model_actor(part.actor).unwrap();
        let animation = actor.field_model_animation();
        let key = animation.map(|a| (a.source, a.resource, a.slot, a.start_tick));
        if key != rig.clip {
            rig.start_blend();
            rig.clip = key;
        }
        let sample = ActorSample::current(actor, world, part.pass);
        let weight = sample.wing_weight(animation).unwrap_or_else(|| {
            animation
                .filter(|_| rig.sampled)
                .map_or(1., |a| a.blend_weight(world.tick))
        });
        for i in 0..rig.bones.len() {
            let entity = rig.bones[i].0;
            if let Ok(mut transform) = nodes.get_mut(entity) {
                let mut pose =
                    Frame::sample(affine.get(entity, *transform), rig.authored_channels[i]);
                let animated = rig.authored_channels[i] != 0;
                rig.blend_bone(i, &mut pose, weight, animated);
                // A newly loaded rig has no previous visible pose to blend from.
                // Retain its first sample even if the clip starts out paused.
                if !rig.sampled {
                    rig.from[i] = pose;
                }
                affine.set(entity, &mut transform, pose.pose);
            }
        }
        // Setup can render a rig before its first clip is selected. Its bind
        // pose must not become the starting pose for that clip's transition.
        rig.sampled |= rig.authored_channels.iter().any(|&channels| channels != 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn secondary_bones_return_to_rest_when_the_next_clip_omits_them() {
        let rest = Transform::from_xyz(0., 10., 0.);
        let mut rig = Rig::new(vec![(Entity::PLACEHOLDER, rest); 2]);
        rig.secondary_bones.insert(0);
        let folded = rest.with_rotation(Quat::from_rotation_x(1.));
        for i in 0..2 {
            rig.blend_bone(i, &mut Frame::from(folded), 1., true);
        }
        rig.start_blend();
        for (weight, angle) in [(0.5, 0.5), (1., 0.)] {
            for i in 0..2 {
                let mut pose = Frame::sample(rest.into(), 0);
                rig.blend_bone(i, &mut pose, weight, false);
                let expected = if i == 0 { angle } else { 1. };
                let rotation =
                    super::super::sparse_animation::affine::rotation(pose.pose.global().affine());
                assert!(rotation.angle_between(Quat::from_rotation_x(expected)) < 0.001);
                assert_eq!(pose.pose.global().translation(), rest.translation);
            }
        }
    }

    #[test]
    fn interrupted_blends_start_from_the_visible_pose_and_omitted_bones_stay_put() {
        let mut rig = Rig::new(vec![(Entity::PLACEHOLDER, Transform::IDENTITY)]);
        let target = Frame::from(Transform::from_xyz(10., 0., 0.));
        let mut pose = target;
        rig.blend_bone(0, &mut pose, 0.5, true);
        assert_eq!(pose.pose.global().translation().x, 5.);
        rig.start_blend();
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
