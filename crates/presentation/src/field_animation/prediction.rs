//! Predict authored skeletons and one secondary-physics update in scratch memory.
//! Committed blend sources and solver velocities never receive rendered poses.
use super::*;
use crate::sparse_animation::affine::{Helper, PoseEndpoint, PosePair, PoseSpace, apply_poses};
use std::collections::BTreeMap;

impl Rig {
    fn next_pose(
        &self,
        part: &ActorPart,
        model: &crate::field_view::Part,
        actor: &resonance_events::Actor,
        world: &resonance_events::GameWorld,
        echo: Option<&resonance_events::WingEcho>,
        camera: Option<Quat>,
    ) -> (BTreeMap<Entity, Pose>, Locals, Vec<String>) {
        let mut sample = Sample::default();
        let mut roots = Vec::new();
        self.sample_pose(part, model, actor, world, echo, &mut sample, |clip| {
            if actor
                .field_model_animation()
                .is_some_and(|a| std::ptr::eq(a, clip.animation))
            {
                roots.extend_from_slice(clip.roots);
            }
        });
        let mut locals = BTreeMap::new();
        let mut adjustments = Locals::default();
        let translated = crate::field_pose::has_relative_translation(actor);
        for (i, &(entity, rest)) in self.bones.iter().enumerate() {
            let mut transform = rest;
            adjustments.set(entity, &mut transform, sample.poses[i].pose);
            if self.camera_facing.contains(&entity)
                && let Some(rotation) = camera
            {
                adjustments.face_camera(entity, &mut transform, rotation);
            }
            if translated {
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

#[allow(clippy::type_complexity)]
pub(crate) fn prepare(
    state: State,
    art: Res<Art>,
    mut prediction: ResMut<crate::field_view::Prediction>,
    rigs: Query<(
        Entity,
        &ActorPart,
        Option<&Rig>,
        Option<&crate::secondary_motion::Rig>,
    )>,
    helper: Helper,
) {
    let _timer = crate::performance::smoothing_profile::Timer::start(1);
    if prediction.field().is_none() || prediction.poses().is_some() {
        return;
    }
    let camera = state
        .get()
        .events
        .world
        .field_camera
        .as_ref()
        .map(|current| {
            crate::field_view::camera_transform(prediction.camera(current).unwrap_or(current))
                .rotation
        });
    let result = prediction.scratch(|next, _| {
        let mut output = Vec::new();
        let world = &next.events.world;
        for (root, part, rig, secondary) in &rigs {
            let Some(current) = state.get().events.world.field_model_actor(part.actor) else {
                continue;
            };
            let Some(actor) = world.field_model_actor(part.actor) else {
                continue;
            };
            if !part.prepared || !crate::field_view::prediction::continuous_pose(current, actor) {
                continue;
            }
            // Echo slots shift when an older echo expires. Keep this draw's
            // snapshot while advancing its age; never morph it into its neighbor.
            let echo = current.wings.as_ref().and_then(|w| {
                w.layer(part.pass, state.get().events.world.effect_tick)
                    .echo
            });
            let target_root =
                crate::field_view::actor_transform(actor, part.actor, part.pass, world, echo);
            if let Ok(current_root) = helper.local(root) {
                output.push(PoseEndpoint {
                    entity: root,
                    poses: PosePair::new(current_root, target_root.into()),
                    space: PoseSpace::Local,
                });
            }
            let Some(rig) = rig.filter(|_| !current.animation_culled) else {
                continue;
            };
            let model = &art.models[&part.resource][part.part];
            // Outline skeletons receive the final primary pose in finish_models().
            if model.spec.outline_color.is_some() {
                continue;
            }
            let (mut locals, adjustments, animated_roots) =
                rig.next_pose(part, model, actor, world, echo, camera);
            locals.insert(root, target_root.into());
            let empty = BTreeMap::new();
            let mut future_cache = bevy::ecs::entity::EntityHashMap::default();
            let mut dynamics = BTreeMap::new();
            let mut driven = BTreeMap::new();
            if let Some(secondary) = secondary
                && let Some(pose) = secondary.predict(
                    actor,
                    world,
                    world.tick,
                    target_root.scale,
                    &animated_roots,
                    |entity| {
                        Some((
                            helper
                                .predicted_global(
                                    entity,
                                    &locals,
                                    &adjustments,
                                    &empty,
                                    Some(&mut future_cache),
                                )
                                .ok()?,
                            helper.predicted_has_affine(entity, &locals),
                        ))
                    },
                )
            {
                pose.bind(secondary, &mut dynamics, &mut driven);
            }
            output.extend(
                helper
                    .pose_endpoints(
                        rig.bones.iter().map(|&(entity, _)| entity),
                        &locals,
                        &adjustments,
                        &dynamics,
                        &driven,
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
    }
}

#[allow(clippy::type_complexity)]
pub(crate) fn present<const FUTURE: bool>(
    prediction: Res<crate::field_view::Prediction>,
    mut workspace: Local<crate::sparse_animation::affine::PoseWorkspace>,
    mut transforms: ParamSet<(Helper, (Query<&mut Transform>, ResMut<Locals>))>,
) {
    if let Some(poses) = prediction.poses() {
        let _timer = crate::performance::smoothing_profile::Timer::start(1);
        apply_poses(
            poses,
            if FUTURE { 1. } else { prediction.alpha },
            &mut transforms,
            &mut workspace,
        );
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
        let pose = crate::field_view::actor_transform(actor, 90026, 0, &events.world, Some(&echo));
        assert_eq!(pose.translation.x, 16.);
        assert!(pose.scale.abs_diff_eq(Vec3::new(1.32, 1.32, 1.8), 0.00001));
        assert_eq!(echo.rgba(events.world.tick)[3], 0);
    }
}
