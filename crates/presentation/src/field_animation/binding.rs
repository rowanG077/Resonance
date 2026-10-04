//! Ordered immediate model evaluations retain poses hidden by a later binding.
use super::*;
use crate::sparse_animation::Clip;
use anyhow::Result;

pub(crate) struct PoseUpdate {
    pub locals: BTreeMap<Entity, Pose>,
    pub yaw: f32,
    pub animated_roots: Vec<u16>,
    pub secondary_motion_disabled: bool,
}

impl Rig {
    #[allow(clippy::too_many_arguments)] // Replay uses the same model, clock and camera as its draw.
    pub(super) fn replay_bindings(
        &mut self,
        root: Entity,
        model: &resonance_content::ScenePart,
        handles: &[Handle<Clip>],
        clips: &Assets<Clip>,
        actor: &resonance_events::Actor,
        tick: u32,
        camera: Quat,
    ) -> Result<()> {
        let displayed = self.presented.clone();
        let displayed_channels = self.authored_channels.clone();
        for binding in &actor.animation_bindings.updates {
            let mut poses: Vec<_> = self
                .bones
                .iter()
                .enumerate()
                .map(|(i, (_, rest))| Frame::sample((*rest).into(), 0, self.bind_channels[i]))
                .collect();
            self.authored_channels.fill(0);
            let mut animated_roots = Vec::new();
            for (layer, animation) in std::iter::once(&binding.animation)
                .chain(binding.scenery.values())
                .enumerate()
            {
                let Some((motion, time, roots)) =
                    sample_clip(model, handles, clips, animation, actor.resource, tick, 0.)
                else {
                    continue;
                };
                if layer == 0 {
                    animated_roots.extend_from_slice(roots);
                }
                for (i, pose) in self.sample_tracks(motion, time)? {
                    poses[i] = pose;
                }
            }
            self.from.clone_from(&self.previous);
            let weight = binding.animation.blend_weight(tick);
            for (i, pose) in poses.iter_mut().enumerate() {
                self.blend_bone(i, pose, weight, false, self.authored_channels[i] != 0);
            }
            let mut affine = Locals::default();
            let mut transforms: Vec<_> = self.bones.iter().map(|(_, rest)| *rest).collect();
            for (i, pose) in poses.iter().enumerate() {
                affine.set(self.bones[i].0, &mut transforms[i], pose.pose);
                if self.camera_facing.contains(&self.bones[i].0) {
                    affine.face_camera(self.bones[i].0, &mut transforms[i], camera);
                }
            }
            for adjustment in binding.adjustments.values() {
                let index = match &adjustment.bone {
                    resonance_events::BoneTarget::Index(i) => Some(usize::from(*i)),
                    resonance_events::BoneTarget::Name(name) => {
                        model.bone_names.iter().position(|n| n == name)
                    }
                };
                if let Some(i) = index.filter(|i| *i < self.bones.len()) {
                    crate::field_pose::adjust_bone(
                        self.bones[i].0,
                        &mut transforms[i],
                        &mut affine,
                        adjustment,
                        self.bones[i].1.scale,
                        tick,
                    );
                }
            }
            let [x, y, z] = binding.angles.map(f32::to_radians);
            let mut locals: BTreeMap<_, _> = self
                .bones
                .iter()
                .zip(transforms)
                .map(|(&(entity, _), transform)| (entity, affine.get(entity, transform)))
                .collect();
            locals.insert(
                root,
                Transform {
                    translation: Vec3::from_array(binding.position),
                    rotation: Quat::from_euler(EulerRot::ZYX, z, y, x),
                    scale: Vec3::from_array(binding.scale),
                }
                .into(),
            );
            self.bindings.push(PoseUpdate {
                locals,
                yaw: binding.angles[2],
                animated_roots,
                secondary_motion_disabled: binding.secondary_motion_disabled,
            });
            self.binding_pose = poses;
        }
        self.presented = displayed;
        self.authored_channels = displayed_channels;
        self.binding_tick = Some(tick);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_events::animation::{AnimationBinding, AnimationBindings};

    #[test]
    fn later_binding_blends_from_completed_intermediate_pose_and_keeps_each_transform() {
        let mut world = World::new();
        let root = world.spawn_empty().id();
        let bone = world.spawn_empty().id();
        let rest = Transform::from_xyz(10., 0., 0.);
        let mut rig = Rig::new(vec![(bone, rest)]);
        let mut clips = Assets::<Clip>::default();
        let mut handles: Vec<_> = [20., 40.]
            .map(|x| {
                let motion = serde_json::from_value(serde_json::json!({
                    "duration_frames":1., "tracks":[{"bone":0,"bind_channels":8,
                        "period_frames":1.,"times":[0.,1.],
                        "translation":{"interpolation":"linear","values":[[x,0.,0.],[x,0.,0.]]}}]
                }))
                .unwrap();
                clips.add(Clip(std::sync::Arc::new(motion)))
            })
            .into();
        // A later clip omits the socket: keep the intermediate binding pose,
        // even though it has never been displayed.
        handles.push(clips.add(Clip(std::sync::Arc::new(
            resonance_content::animation::Motion {
                duration_frames: 1.,
                tracks: Vec::new(),
            },
        ))));
        let spec = serde_json::from_value(serde_json::json!({
            "resource":1,"mesh":"test.glb","textures":[],"materials":[],
            "translation":[0.,0.,0.],"autoplay":false,"texture_animations":[],"bone_names":["socket"],
            "clips":[{"motion":"a.motion","resource_slot":12,"duration_seconds":1.},
                     {"motion":"b.motion","resource_slot":24,"duration_seconds":1.},
                     {"motion":"c.motion","resource_slot":36,"duration_seconds":1.}]
        })).unwrap();
        let mut actor = resonance_events::Actor::new(1, [0.; 3]);
        actor.animation_bindings = AnimationBindings {
            tick: 1,
            updates: [(12, 0, 100.), (24, 1, 200.), (36, 0, 300.)]
                .map(|(slot, blend_ticks, x)| AnimationBinding {
                    animation: resonance_events::Animation {
                        blend_ticks,
                        ..resonance_events::Animation::new(1, slot, 60, 1)
                    },
                    scenery: BTreeMap::new(),
                    position: [x, 0., 0.],
                    angles: [0.; 3],
                    scale: [1.; 3],
                    adjustments: BTreeMap::new(),
                    secondary_motion_disabled: false,
                })
                .into(),
        };
        rig.camera_facing.push(bone);
        rig.replay_bindings(
            root,
            &spec,
            &handles,
            &clips,
            &actor,
            1,
            Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
        )
        .unwrap();
        let origins: Vec<_> = rig
            .bindings
            .iter()
            .map(|p| {
                (p.locals[&root].global() * p.locals[&bone].global())
                    .translation()
                    .x
            })
            .collect();
        assert_eq!(origins, [120., 230., 330.]);
        for binding in &rig.bindings {
            assert!(
                binding.locals[&bone]
                    .global()
                    .affine()
                    .transform_vector3(Vec3::Y)
                    .distance(Vec3::Z)
                    < 0.0001
            );
        }
        assert_eq!(rig.presented[0].pose, rest.into());
        assert_eq!(rig.from[0].pose.global().translation().x, 20.);
    }
}
