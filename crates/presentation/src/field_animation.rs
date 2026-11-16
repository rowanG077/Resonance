//! Blend skeletal poses before script adjustments and secondary motion.
#[cfg(test)]
mod camera_tests;
#[cfg(test)]
mod continuity_tests;
mod frame;
mod prediction;
use super::field_view::{ActorPart, Art, Part, State};
use super::sparse_animation::affine::{Locals, Pose};
use bevy::prelude::*;
use frame::Frame;
pub(super) use prediction::{prepare as prepare_prediction, present};
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
            let echo = sample
                .wings
                .and_then(|w| w.layer(sample.pass, sample.effect_tick).echo);
            let mut channels = vec![0; rig.bones.len()];
            for animation in start.animation.iter().chain(&start.scenery_animations) {
                if let Some(clip) = sample.clip(artwork, animation, echo) {
                    let tracks = sample_tracks(
                        &rig.bones,
                        &mut channels,
                        clip.motion,
                        clip.time,
                        clip.echo_weight,
                    )
                    .collect::<anyhow::Result<Vec<_>>>()
                    .expect("validated animation must evaluate");
                    for (bone, pose) in tracks {
                        rig.presented[bone] =
                            rig.blended_bone(bone, pose, rig.from[bone], weight, true);
                    }
                }
            }
            rig.sampled = channels.iter().any(|&channels| channels != 0);
        }
        commands.entity(root).insert(rig);
    }
}

struct SampledClip<'a> {
    animation: &'a resonance_events::Animation,
    motion: &'a resonance_content::animation::Motion,
    time: f32,
    roots: &'a [String],
    echo_weight: Option<f32>,
}

/// Predictions retain the current echo when its slot changes in the next tick.
/// All other clip timing and entrance rules are shared with committed sampling.
fn sample_clips<'a>(
    model: &'a crate::field_view::Part,
    actor: &'a resonance_events::Actor,
    pass: u8,
    world: &'a resonance_events::GameWorld,
    echo: Option<&'a resonance_events::WingEcho>,
) -> impl Iterator<Item = SampledClip<'a>> + 'a {
    let sample = ActorSample::current(actor, world, pass);
    actor
        .field_model_animation()
        .into_iter()
        .chain(actor.scenery_animations.values())
        .filter_map(move |animation| sample.clip(model, animation, echo))
}

fn sample_tracks<'a>(
    bones: &'a [(Entity, Transform)],
    channels: &'a mut [u8],
    motion: &'a resonance_content::animation::Motion,
    time: f32,
    echo_weight: Option<f32>,
) -> impl Iterator<Item = anyhow::Result<(usize, Frame)>> + 'a {
    motion.tracks.iter().map(move |track| {
        let i = usize::from(track.bone);
        let rest = bones[i].1;
        channels[i] |= track.channels().0;
        let mut frame = Frame::sample(
            super::sparse_animation::sample_track(track, time, rest)?,
            channels[i],
        );
        if let Some(weight) = echo_weight {
            frame = Frame::sample(rest.into(), 0).mix(frame, rest, weight);
        }
        Ok((i, frame))
    })
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

    fn clip(
        self,
        model: &'a Part,
        animation: &'a Animation,
        echo: Option<&resonance_events::WingEcho>,
    ) -> Option<SampledClip<'a>> {
        let delay = if echo.is_some() {
            -1.
        } else {
            self.wings
                .map_or(0., |w| w.layer(self.pass, self.effect_tick).pose_delay)
        };
        let echo_weight = echo.and_then(|e| e.entrance_weight(self.tick));
        let index = model
            .spec
            .clips
            .iter()
            .position(|clip| animation.matches(clip, self.resource))?;
        let spec = &model.spec.clips[index];
        let duration = spec.duration_seconds * resonance_content::ANIMATION_HZ;
        let mut time = animation.sample(self.tick, 0, duration);
        let delay = self
            .wings
            .and_then(|w| w.entrance(self.pass, self.tick.saturating_sub(animation.start_tick)))
            .map_or(delay, |entrance| entrance.delay);
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
        Some(SampledClip {
            animation,
            motion: &model.motions[index],
            time: if echo_weight.is_some() {
                0.
            } else {
                time * resonance_content::animation::FRAME_HZ / resonance_content::ANIMATION_HZ
            },
            roots: &spec.pose_chains,
            echo_weight,
        })
    }
}

#[derive(Default)]
pub(super) struct Sample {
    poses: Vec<Frame>,
    channels: Vec<u8>,
}

/// Evaluate and blend in reusable storage, then publish each authored bone once.
pub(super) fn sample(
    state: State,
    art: Res<Art>,
    mut rigs: Query<(&ActorPart, &mut Rig)>,
    mut nodes: Query<&mut Transform>,
    mut affine: ResMut<Locals>,
    mut applied: ResMut<super::field_audit::Applied>,
    mut output: Local<Sample>,
) {
    let world = &state.get().events.world;
    for (part, mut rig) in &mut rigs {
        let actor = world.field_model_actor(part.actor).unwrap();
        let model = &art.models[&part.resource][part.part];
        let echo = actor
            .wings
            .as_ref()
            .and_then(|w| w.layer(part.pass, world.effect_tick).echo);
        rig.sample_pose(part, model, actor, world, echo, &mut output, |clip| {
            applied.ack(super::field_audit::Request::Animation {
                actor: part.actor,
                part: part.part,
                resource: clip.animation.resource,
                slot: clip.animation.slot,
            });
        });
        let key = actor
            .field_model_animation()
            .map(|a| (a.source, a.resource, a.slot, a.start_tick));
        if key != rig.clip {
            rig.start_blend();
            rig.clip = key;
        }
        rig.presented.clone_from(&output.poses);
        // The first visible sample has no earlier pose to blend from.
        if !rig.sampled {
            rig.from.clone_from(&output.poses);
        }
        rig.sampled |= output.channels.iter().any(|&channels| channels != 0);
        for (&(entity, rest), pose) in rig.bones.iter().zip(&output.poses) {
            if let Ok(mut transform) = nodes.get_mut(entity) {
                // Affine keys retain the authored TRS fallback for absolute
                // script setters; a previous rendered pose must not supply it.
                let mut local = rest;
                affine.set(entity, &mut local, pose.pose);
                transform.set_if_neq(local);
            }
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

    /// Committed and speculative poses share clip selection, omitted-channel
    /// behavior and blend sources. Sampling never changes the source rig.
    #[allow(clippy::too_many_arguments)]
    fn sample_pose(
        &self,
        part: &ActorPart,
        model: &crate::field_view::Part,
        actor: &resonance_events::Actor,
        world: &resonance_events::GameWorld,
        echo: Option<&resonance_events::WingEcho>,
        output: &mut Sample,
        mut sampled: impl FnMut(&SampledClip),
    ) {
        output.poses.clear();
        output.poses.extend(
            self.bones
                .iter()
                .map(|(_, rest)| Frame::sample((*rest).into(), 0)),
        );
        output.channels.resize(self.bones.len(), 0);
        output.channels.fill(0);
        for clip in sample_clips(model, actor, part.pass, world, echo) {
            for frame in sample_tracks(
                &self.bones,
                &mut output.channels,
                clip.motion,
                clip.time,
                clip.echo_weight,
            ) {
                let (i, frame) = frame.expect("validated animation must evaluate");
                output.poses[i] = frame;
            }
            sampled(&clip);
        }
        let key = actor
            .field_model_animation()
            .map(|a| (a.source, a.resource, a.slot, a.start_tick));
        let source = if key == self.clip {
            &self.from
        } else {
            &self.presented
        };
        let weight = self.blend_weight(actor, part.pass, world.tick);
        for (i, pose) in output.poses.iter_mut().enumerate() {
            // Match the authored channel mask after all overlapping controllers.
            *pose = self.blended_bone(
                i,
                Frame::sample(pose.pose, output.channels[i]),
                source[i],
                weight,
                output.channels[i] != 0,
            );
        }
    }

    fn start_blend(&mut self) {
        self.from.copy_from_slice(&self.presented);
    }

    /// The committed and predicted poses use the same omitted-channel rules.
    fn blended_bone(
        &self,
        index: usize,
        pose: Frame,
        source: Frame,
        weight: f32,
        animated: bool,
    ) -> Frame {
        // Cloth and hair resume their rest target when explicit motion ends;
        // carrying a previous clip's bend would pin the dynamics to that pose.
        if !animated && !self.secondary_bones.contains(&index) {
            self.presented[index]
        } else if weight < 1. {
            source.mix(pose, self.bones[index].1, weight)
        } else {
            pose
        }
    }

    fn blend_weight(&self, actor: &resonance_events::Actor, pass: u8, tick: u32) -> f32 {
        actor.field_model_animation().map_or(1., |animation| {
            actor
                .wings
                .as_ref()
                .and_then(|w| w.entrance(pass, tick.saturating_sub(animation.start_tick)))
                .map_or_else(
                    || {
                        if self.sampled {
                            animation.blend_weight(tick)
                        } else {
                            1.
                        }
                    },
                    |entrance| entrance.weight,
                )
        })
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn secondary_bones_return_to_rest_when_the_next_clip_omits_them() {
        let rest = Transform::from_xyz(0., 10., 0.);
        let mut rig = Rig::new(vec![(Entity::PLACEHOLDER, rest); 2]);
        rig.secondary_bones.insert(0);
        let folded = rest.with_rotation(Quat::from_rotation_x(1.));
        rig.presented.fill(Frame::from(folded));
        rig.start_blend();
        for (weight, angle) in [(0.5, 0.5), (1., 0.)] {
            for i in 0..2 {
                let pose =
                    rig.blended_bone(i, Frame::sample(rest.into(), 0), rig.from[i], weight, false);
                rig.presented[i] = pose;
                let expected = if i == 0 { angle } else { 1. };
                let rotation =
                    super::super::sparse_animation::affine::rotation(pose.pose.global().affine());
                assert!(rotation.angle_between(Quat::from_rotation_x(expected)) < 0.001);
                assert_eq!(pose.pose.global().translation(), rest.translation);
            }
        }
    }

    #[test]
    fn first_animation_has_no_unseen_bind_pose_to_blend_from() {
        let mut rig = Rig::new(vec![(Entity::PLACEHOLDER, Transform::IDENTITY)]);
        let mut actor = resonance_events::Actor::new(1, [0.; 3]);
        let mut animation = resonance_events::Animation::new(1, 0, 60, 10);
        animation.blend_ticks = 9;
        actor.animation = Some(animation);
        assert_eq!(rig.blend_weight(&actor, 0, 10), 1.);
        rig.sampled = true;
        assert_eq!(rig.blend_weight(&actor, 0, 10), 0.);
    }

    #[test]
    fn interrupted_blends_start_from_the_visible_pose_and_omitted_bones_stay_put() {
        let mut rig = Rig::new(vec![(Entity::PLACEHOLDER, Transform::IDENTITY)]);
        let target = Frame::from(Transform::from_xyz(10., 0., 0.));
        let mut pose = rig.blended_bone(0, target, rig.from[0], 0.5, true);
        rig.presented[0] = pose;
        assert_eq!(pose.pose.global().translation().x, 5.);
        rig.start_blend();
        pose = rig.blended_bone(
            0,
            Frame::from(Transform::from_xyz(-5., 0., 0.)),
            rig.from[0],
            0.5,
            true,
        );
        rig.presented[0] = pose;
        assert_eq!(pose.pose.global().translation().x, 0.);
        pose = rig.blended_bone(0, pose, rig.from[0], 1., false);
        assert_eq!(pose.pose.global().translation().x, 0.);
    }
}
