//! Predict authored skeletons and one secondary-physics update in scratch memory.
//! Committed blend sources and solver velocities never receive rendered poses.
use super::*;
use crate::sparse_animation::affine::{Helper, PoseEndpoint, PosePair};
use std::collections::BTreeMap;

impl Rig {
    fn next_pose(
        &self,
        part: &ActorPart,
        model: &crate::field_view::Part,
        actor: &resonance_events::Actor,
        world: &resonance_events::GameWorld,
        clips: &Assets<crate::sparse_animation::Clip>,
        echo: Option<&resonance_events::WingEcho>,
    ) -> (BTreeMap<Entity, Pose>, Locals, Vec<u16>) {
        let mut frames: Vec<_> = self
            .bones
            .iter()
            .enumerate()
            .map(|(i, (_, rest))| Frame::sample((*rest).into(), 0, self.bind_channels[i]))
            .collect();
        let mut channels = vec![0; frames.len()];
        let mut roots = Vec::new();
        for animation in actor
            .animation
            .iter()
            .chain(actor.scenery_animations.values())
        {
            let layer = actor
                .wings
                .as_ref()
                .map(|w| w.layer(part.pass, world.effect_tick));
            let entrance = actor.wings.as_ref().and_then(|w| {
                w.entrance(part.pass, world.tick.saturating_sub(animation.start_tick))
            });
            let delay = entrance.as_ref().map_or_else(
                || {
                    if echo.is_some() {
                        -1.
                    } else {
                        layer.as_ref().map_or(0., |l| l.pose_delay)
                    }
                },
                |e| e.delay,
            );
            let Some((clip, mut time, animated_roots)) = sample_clip(
                &model.spec,
                &model.clips,
                clips,
                animation,
                actor.resource,
                world.tick,
                delay,
            ) else {
                continue;
            };
            if actor
                .animation
                .as_ref()
                .is_some_and(|a| std::ptr::eq(a, animation))
            {
                roots.extend_from_slice(animated_roots);
            }
            let echo_weight = echo.and_then(|e| e.entrance_weight(world.tick));
            if echo_weight.is_some() {
                time = 0.;
            }
            for track in &clip.tracks {
                let i = usize::from(track.bone);
                let rest = self.bones[i].1;
                channels[i] |= track.channels().0;
                let mut frame = Frame::sample(
                    crate::sparse_animation::sample_track(track, time, rest)
                        .expect("validated motion"),
                    channels[i],
                    self.bind_channels[i],
                );
                if let Some(weight) = echo_weight {
                    frame = Frame::sample(rest.into(), 0, self.bind_channels[i]).mix(
                        frame,
                        rest,
                        self.bind_channels[i],
                        weight,
                    );
                }
                frames[i] = frame;
            }
        }
        let animation = actor.animation.as_ref();
        let key = animation.map(|a| (a.source, a.resource, a.slot, a.start_tick));
        let source = if key == self.clip {
            &self.from
        } else {
            &self.presented
        };
        let weight = self.blend_weight(actor, part.pass, world.tick);
        let mut locals = BTreeMap::new();
        let mut adjustments = Locals::default();
        for (i, &(entity, rest)) in self.bones.iter().enumerate() {
            let pose = self.blended_bone(i, frames[i], source[i], weight, channels[i] != 0);
            let mut transform = rest;
            adjustments.set(entity, &mut transform, pose.pose);
            if self.camera_facing.contains(&entity)
                && let Some(camera) = &world.field_camera
            {
                adjustments.face_camera(
                    entity,
                    &mut transform,
                    crate::field_view::camera_transform(camera).rotation,
                );
            }
            if actor
                .appearance
                .bone_adjustments
                .values()
                .any(|a| a.translation.is_some())
            {
                adjustments.translation_boundary(entity);
            }
            for adjustment in actor.appearance.bone_adjustments.values() {
                let matches = match &adjustment.bone {
                    resonance_events::BoneTarget::Index(index) => usize::from(*index) == i,
                    resonance_events::BoneTarget::Name(name) => {
                        model.spec.bone_names.get(i) == Some(name)
                    }
                };
                if matches {
                    crate::field_pose::adjust_bone(
                        entity,
                        &mut transform,
                        &mut adjustments,
                        adjustment,
                        rest.scale,
                        world.tick,
                    );
                }
            }
            locals.insert(entity, adjustments.get(entity, transform));
        }
        (locals, adjustments, roots)
    }
}

fn root_pose(
    actor: &resonance_events::Actor,
    id: i32,
    pass: u8,
    world: &resonance_events::GameWorld,
    echo: Option<&resonance_events::WingEcho>,
) -> Transform {
    let layer = actor
        .wings
        .as_ref()
        .map(|w| w.layer(pass, world.effect_tick));
    let [x, y, z] = echo.map_or_else(
        || {
            [
                actor.tilt_degrees()[0],
                actor.tilt_degrees()[1],
                actor.appearance.fixed_heading.unwrap_or(actor.heading),
            ]
        },
        |e| e.angles,
    );
    let mut scale =
        Vec3::from_array(echo.map_or_else(|| actor.model_scale(), |e| e.scale(world.tick)))
            * layer.as_ref().map_or(1., |l| l.scale);
    if id == world.controlled_actor {
        scale *= world.player_size.model_scale();
    }
    Transform {
        translation: Vec3::from_array(echo.map_or_else(|| actor.visual_position(), |e| e.position)),
        rotation: Quat::from_rotation_z(z.to_radians())
            * Quat::from_rotation_y(y.to_radians())
            * Quat::from_rotation_x(x.to_radians()),
        scale,
    }
}

#[allow(clippy::type_complexity)]
pub(crate) fn present(
    state: State,
    art: Res<Art>,
    clips: Res<Assets<crate::sparse_animation::Clip>>,
    mut prediction: ResMut<crate::field_view::Prediction>,
    rigs: Query<(
        Entity,
        &ActorPart,
        Option<&Rig>,
        Option<&crate::secondary_motion::Rig>,
    )>,
    mut transforms: ParamSet<(Helper, (Query<&mut Transform>, ResMut<Locals>))>,
) {
    let _timer = crate::performance::smoothing_profile::Timer::start(1);
    if prediction.field().is_none() {
        return;
    }
    if prediction.poses().is_none() {
        let helper = transforms.p0();
        let result = prediction.scratch(|next, _| {
            let mut output = Vec::new();
            let world = &next.events.world;
            for (root, part, rig, secondary) in &rigs {
                let Some(current) = state.get().events.world.actors.get(&part.actor) else {
                    continue;
                };
                let Some(actor) = world.actors.get(&part.actor) else {
                    continue;
                };
                if !part.prepared
                    || !crate::field_view::prediction::continuous_actor(current, actor)
                {
                    continue;
                }
                // Echo slots shift when an older echo expires. Keep this draw's
                // snapshot while advancing its age; never morph it into its neighbor.
                let echo = current.wings.as_ref().and_then(|w| {
                    w.layer(part.pass, state.get().events.world.effect_tick)
                        .echo
                });
                let target_root = root_pose(actor, part.actor, part.pass, world, echo);
                if let Ok(current_root) = helper.local(root) {
                    output.push(PoseEndpoint {
                        entity: root,
                        poses: PosePair::new(current_root, target_root.into()),
                        global: false,
                    });
                }
                let Some(rig) = rig.filter(|_| !current.animation_culled) else {
                    continue;
                };
                let model = &art.models[&part.resource][part.part];
                // Outline skeletons receive the final primary pose in outlines().
                if model.spec.outline_color.is_some() {
                    continue;
                }
                let (mut locals, adjustments, animated_roots) =
                    rig.next_pose(part, model, actor, world, &clips, echo);
                locals.insert(root, target_root.into());
                let empty = BTreeMap::new();
                let mut future_cache = bevy::ecs::entity::EntityHashMap::default();
                let dynamics = secondary
                    .and_then(|s| {
                        s.predict(
                            actor,
                            world.tick,
                            target_root.scale,
                            &animated_roots,
                            |entity| {
                                Some((
                                    helper
                                        .predicted_global_cached(
                                            entity,
                                            &locals,
                                            &adjustments,
                                            &empty,
                                            &mut future_cache,
                                        )
                                        .ok()?,
                                    helper.predicted_has_affine(entity, &locals),
                                ))
                            },
                        )
                    })
                    .unwrap_or_default();
                output.extend(
                    helper
                        .pose_endpoints(
                            rig.bones.iter().map(|&(entity, _)| entity),
                            &locals,
                            &adjustments,
                            &dynamics,
                        )
                        .filter_map(Result::ok),
                );
            }
            output
        });
        let Some(output) = result else {
            return;
        };
        if let Err(error) = prediction.retain_poses(&output) {
            prediction.report_error(&error);
            return;
        }
    }
    let (mut nodes, mut affine) = transforms.p1();
    for &endpoint in prediction.poses().unwrap() {
        endpoint.apply(prediction.alpha, &mut nodes, &mut affine);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expiring_wing_echo_keeps_its_snapshot_when_the_slots_shift() {
        let bytes: Vec<_> = [4u16, 0, 0, 0, 0x20ff]
            .into_iter()
            .flat_map(u16::to_be_bytes)
            .collect();
        let mut events = resonance_events::EventRuntime::new(
            std::sync::Arc::new(symphonia_script::Program::decode(&bytes).unwrap()),
            Default::default(),
        )
        .unwrap();
        let mut actor = resonance_events::Actor::new(10, [0.; 3]);
        actor.set_wings(resonance_events::WingStyle::Echo);
        events.world.insert_actor(90026, actor);
        for tick in 1..=23 {
            events.world.actors.get_mut(&90026).unwrap().position[0] = tick as f32 * 2.;
            events.step().unwrap();
        }
        let echo = events.world.actors[&90026]
            .wings
            .as_ref()
            .unwrap()
            .layer(0, events.world.effect_tick)
            .echo
            .unwrap()
            .clone();
        assert_eq!(echo.position[0], 16.);
        events.step().unwrap();
        let actor = &events.world.actors[&90026];
        let replacement = actor
            .wings
            .as_ref()
            .unwrap()
            .layer(0, events.world.effect_tick)
            .echo
            .unwrap();
        assert_eq!(replacement.position[0], 32.);
        let pose = root_pose(actor, 90026, 0, &events.world, Some(&echo));
        assert_eq!(pose.translation.x, 16.);
        assert!(pose.scale.abs_diff_eq(Vec3::new(1.32, 1.32, 1.8), 0.00001));
        assert_eq!(echo.rgba(events.world.tick)[3], 0);
    }
}
