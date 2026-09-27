//! Ordered immediate model evaluations retain poses hidden by a later binding.
use super::*;
use crate::sparse_animation::{Clip, sample_track};
use anyhow::Result;

pub(crate) struct PoseUpdate {
    pub locals: BTreeMap<Entity, Pose>,
    pub yaw: f32,
    pub animated_roots: Vec<u16>,
    pub secondary_motion_disabled: bool,
}

impl Rig {
    pub(super) fn replay_bindings(
        &mut self,
        root: Entity,
        model: &resonance_content::ScenePart,
        handles: &[Handle<Clip>],
        clips: &Assets<Clip>,
        actor: &resonance_events::Actor,
        tick: u32,
    ) -> Result<()> {
        for binding in &actor.animation_bindings.updates {
            let mut poses: Vec<_> = self
                .bones
                .iter()
                .enumerate()
                .map(|(i, (_, rest))| Frame::sample((*rest).into(), 0, self.bind_channels[i]))
                .collect();
            let mut channels = vec![0; self.bones.len()];
            let mut animated_roots = Vec::new();
            for (layer, animation) in std::iter::once(&binding.animation)
                .chain(binding.scenery.values())
                .enumerate()
            {
                let Some(index) = model
                    .clips
                    .iter()
                    .position(|c| animation.matches(c, actor.resource))
                else {
                    continue;
                };
                let spec = &model.clips[index];
                if layer == 0 {
                    animated_roots.clone_from(&spec.secondary_pose_nodes);
                }
                let motion = &clips.get(&handles[index]).expect("prepared sparse clip").0;
                let time = animation.sample(
                    tick,
                    0,
                    spec.duration_seconds * resonance_content::ANIMATION_HZ,
                ) * resonance_content::animation::FRAME_HZ
                    / resonance_content::ANIMATION_HZ;
                for track in &motion.tracks {
                    let i = usize::from(track.bone);
                    channels[i] |= track.channels().0;
                    poses[i] = Frame::sample(
                        sample_track(track, time, self.bones[i].1)?,
                        channels[i],
                        self.bind_channels[i],
                    );
                }
            }
            self.from.clone_from(&self.previous);
            let weight = binding.animation.blend_weight(tick);
            for (i, pose) in poses.iter_mut().enumerate() {
                if weight < 1. {
                    *pose = self.from[i].mix(*pose, self.bones[i].1, self.bind_channels[i], weight);
                } else {
                    self.previous[i] = *pose;
                }
            }
            let mut affine = Locals::default();
            let mut transforms: Vec<_> = self.bones.iter().map(|(_, rest)| *rest).collect();
            for (i, pose) in poses.iter().enumerate() {
                affine.set(self.bones[i].0, &mut transforms[i], pose.pose);
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
        let handles: Vec<_> = [20., 40.]
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
        let spec = serde_json::from_value(serde_json::json!({
            "resource":1,"mesh":"test.glb","textures":[],"materials":[],
            "translation":[0.,0.,0.],"autoplay":false,"texture_animations":[],"bone_names":["socket"],
            "clips":[{"motion":"a.motion","resource_slot":12,"duration_seconds":1.},
                     {"motion":"b.motion","resource_slot":24,"duration_seconds":1.}]
        })).unwrap();
        let mut actor = resonance_events::Actor::new(1, [0.; 3]);
        actor.animation_bindings = AnimationBindings {
            tick: 1,
            updates: [(12, 0, 100.), (24, 1, 200.)]
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
        rig.replay_bindings(root, &spec, &handles, &clips, &actor, 1)
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
        assert_eq!(origins, [120., 230.]);
        assert_eq!(rig.presented[0].pose, rest.into());
        assert_eq!(rig.from[0].pose.global().translation().x, 20.);
    }
}
