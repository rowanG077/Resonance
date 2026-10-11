//! Attached models use the bone's rigid frame without changing script origins.
use crate::{GameWorld, ResourceLibrary, world::Attachment};
use anyhow::{Context, Result, ensure};
use resonance_content::animation::{Matrix, Transform, matrix_rotation, multiply};
use std::collections::BTreeMap;

#[derive(Debug)]
struct MissingOwner(i32);

impl std::fmt::Display for MissingOwner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "attachment owner {} is missing", self.0)
    }
}
impl std::error::Error for MissingOwner {}

struct Frames<'a> {
    world: &'a GameWorld,
    resources: &'a ResourceLibrary,
    roots: BTreeMap<i32, Matrix>,
}

impl Frames<'_> {
    fn root(&mut self, id: i32, depth: usize) -> Result<Matrix> {
        if let Some(matrix) = self.roots.get(&id) {
            return Ok(*matrix);
        }
        let actor = self.world.actors.get(&id).ok_or(MissingOwner(id))?;
        ensure!(depth < self.world.actors.len(), "cyclic model attachment");
        let local = actor.local_matrix();
        let root = match &actor.attachment {
            Some(attachment) => multiply(self.parent(attachment, depth + 1)?, local),
            None => local,
        };
        self.roots.insert(id, root);
        Ok(root)
    }

    fn parent(&mut self, attachment: &Attachment, depth: usize) -> Result<Matrix> {
        let root = self.root(attachment.actor, depth)?;
        let actor = &self.world.actors[&attachment.actor];
        let bone = multiply(
            root,
            self.resources
                .bone_matrix(actor, &attachment.bone, self.world.tick)?,
        );
        Ok(Transform {
            translation: bone[3][..3].try_into().unwrap(),
            rotation: matrix_rotation(bone)?,
            ..Default::default()
        }
        .matrix())
    }
}

impl GameWorld {
    /// Scripts can remove a motion owner before replacing it. Presentation keeps
    /// the last attached pose then; an unknown position must not cull its child.
    pub(crate) fn attached_culling_position(
        &self,
        resources: &ResourceLibrary,
        id: i32,
    ) -> Result<Option<[f32; 3]>> {
        match self.attached_position(resources, id) {
            Ok(position) => Ok(Some(position)),
            Err(error) if error.is::<MissingOwner>() => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub(crate) fn attached_position(
        &self,
        resources: &ResourceLibrary,
        id: i32,
    ) -> Result<[f32; 3]> {
        let actor = &self.actors[&id];
        let root = Frames {
            world: self,
            resources,
            roots: BTreeMap::new(),
        }
        .root(id, 0)?;
        let frame = match resources
            .model(actor.model_resource())
            .and_then(|model| model.names.first())
        {
            Some(name) => multiply(root, resources.bone_matrix(actor, name, self.tick)?),
            None => root,
        };
        Ok(frame[3][..3].try_into().unwrap())
    }

    pub(crate) fn attachment_parent(
        &self,
        resources: &ResourceLibrary,
        attachment: &Attachment,
    ) -> Result<Matrix> {
        Frames {
            world: self,
            resources,
            roots: BTreeMap::new(),
        }
        .parent(attachment, 0)
    }

    pub(crate) fn update_collision_attachments(
        &mut self,
        resources: &ResourceLibrary,
    ) -> Result<()> {
        let mut frames = Frames {
            world: self,
            resources,
            roots: BTreeMap::new(),
        };
        let updates: Result<Vec<_>> = self
            .actors
            .iter()
            .filter(|(_, actor)| {
                actor.model_collision.is_some() && actor.attachment.is_some()
                    || actor.collision_parent.is_some()
            })
            .map(|(&id, actor)| {
                let frame = actor
                    .attachment
                    .as_ref()
                    .filter(|_| actor.model_collision.is_some())
                    .map(|attachment| frames.parent(attachment, 0))
                    .transpose()
                    .with_context(|| format!("actor {id} collision attachment"))?;
                Ok((id, frame))
            })
            .collect();
        for (id, frame) in updates? {
            self.actors.get_mut(&id).unwrap().collision_parent = frame;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Actor, Animation, AnimationClip, AttachmentPose, EventRuntime, ModelResource};
    use resonance_content::{
        animation::{Bone, Motion, Skeleton, TransformChannels},
        field::CollisionQuery,
    };
    use std::sync::Arc;

    fn culling_scene() -> EventRuntime {
        let skeleton = Arc::new(Skeleton {
            bones: vec![Bone {
                name: "socket".into(),
                parent: None,
                bind_channels: TransformChannels(0),
                bind: Transform::default(),
            }],
        });
        let resources = Arc::new(ResourceLibrary {
            models: [(
                1,
                ModelResource {
                    names: vec!["socket".into()],
                    attachments: crate::ModelAttachments {
                        skeleton: Some(skeleton),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )]
            .into(),
            ..Default::default()
        });
        let mut world = GameWorld::default();
        world.field_camera = Some(crate::camera::CameraRig {
            position: [0.; 3],
            target: [0., 1000., 0.],
            position_settled: true,
            target_settled: true,
            ..Default::default()
        });
        for (id, parent) in [(10, None), (20, Some(10)), (30, Some(20))] {
            let mut actor = Actor::new(1, [0.; 3]);
            actor.grounded = false;
            actor.attachment = parent.map(|actor| Attachment {
                actor,
                bone: "socket".into(),
            });
            if parent.is_none() {
                actor.position = [0., -1000., 0.];
                actor.appearance.model_hidden = true;
            }
            world.insert_actor(id, actor);
        }
        let program =
            symphonia_script::Program::decode(&[0, 4, 0, 0, 0, 0, 0, 0, 0x20, 0xff]).unwrap();
        EventRuntime::with_state(Arc::new(program), resources, world, Default::default()).unwrap()
    }

    #[test]
    fn culling_keeps_nested_children_when_a_motion_owner_is_temporarily_missing() {
        let mut events = culling_scene();
        events.step().unwrap();
        assert!(events.world.actors[&30].animation_culled);
        let mut owner = events.world.actors.remove(&10).unwrap();
        events.step().unwrap();
        assert!(!events.world.actors[&20].animation_culled);
        assert!(!events.world.actors[&30].animation_culled);
        // Collision/camera callers still require a resolvable position.
        assert!(
            events
                .world
                .attached_position(&events.resources, 30)
                .is_err()
        );
        owner.position = [0., 1000., 0.];
        events.world.insert_actor(10, owner);
        events.step().unwrap();
        assert!(!events.world.actors[&30].animation_culled);
        events.world.actors.get_mut(&10).unwrap().position = [0., -1000., 0.];
        events.step().unwrap();
        assert!(events.world.actors[&30].animation_culled);
    }

    #[test]
    fn culling_still_rejects_invalid_bones_and_attachment_cycles() {
        let mut events = culling_scene();
        let attachment = events
            .world
            .actors
            .get_mut(&20)
            .unwrap()
            .attachment
            .as_mut()
            .unwrap();
        attachment.bone = "missing".into();
        assert!(
            events
                .world
                .attached_culling_position(&events.resources, 30)
                .is_err()
        );
        let attachment = events
            .world
            .actors
            .get_mut(&20)
            .unwrap()
            .attachment
            .as_mut()
            .unwrap();
        attachment.bone = "socket".into();
        attachment.actor = 30;
        assert!(
            events
                .world
                .attached_culling_position(&events.resources, 30)
                .unwrap_err()
                .to_string()
                .contains("cyclic")
        );
    }

    #[test]
    fn nested_collision_follows_animated_bones_and_detaches_without_moving_script_coordinates() {
        let skeleton = Arc::new(Skeleton {
            bones: vec![Bone {
                name: "socket".into(),
                parent: None,
                bind_channels: TransformChannels(0),
                bind: Transform {
                    translation: [0., 5., 2.],
                    ..Default::default()
                },
            }],
        });
        let motion: Motion = serde_json::from_value(serde_json::json!({
            "duration_frames":2., "tracks":[{
                "bone":0, "bind_channels":0, "period_frames":2., "times":[0.,2.],
                "translation":{"interpolation":"linear","values":[[10.,0.,5.],[30.,0.,5.]]},
                "rotation":{"interpolation":"step","values":[[0.,0.,0.70710677,0.70710677],[0.,0.,0.70710677,0.70710677]]},
                "scale":{"interpolation":"linear","values":[[2.,2.,2.],[2.,2.,2.]]}
            }]
        })).unwrap();
        let model = ModelResource {
            names: vec!["socket".into()],
            clips: [(
                12,
                AnimationClip {
                    duration_ticks: 4,
                    attachments: Some(
                        AttachmentPose::new(skeleton.clone(), Arc::new(motion)).unwrap(),
                    ),
                },
            )]
            .into(),
            attachments: crate::ModelAttachments {
                skeleton: Some(skeleton),
                ..Default::default()
            },
            ..Default::default()
        };
        let resources = Arc::new(ResourceLibrary {
            models: [(1, model)].into(),
            ..Default::default()
        });
        let mut world = GameWorld::default();
        for (id, position, parent) in [
            (10, [100., 200., 0.], None),
            (20, [10., 0., 0.], Some(10)),
            (30, [0.; 3], Some(20)),
        ] {
            let mut actor = Actor::new(1, position);
            actor.face(90.);
            actor.grounded = false;
            actor.scripted_animation = true;
            actor.animation = Some(Animation::new(1, 12, 4, 0));
            actor.attachment = parent.map(|actor| Attachment {
                actor,
                bone: "socket".into(),
            });
            if parent.is_some() {
                actor.scale_percent[0] = 400;
            }
            world.insert_actor(id, actor);
        }
        world.actors.get_mut(&30).unwrap().model_collision = Some(
            resonance_content::test_support::solid_box([-1.; 3], [1.; 3]),
        );
        let program =
            symphonia_script::Program::decode(&[0, 4, 0, 0, 0, 0, 0, 0, 0x20, 0xff]).unwrap();
        let mut events =
            EventRuntime::with_state(Arc::new(program), resources, world, Default::default())
                .unwrap();
        let child = &events.world.actors[&30];
        assert!(child.contains_solid([90., 173., 10.], CollisionQuery::All));
        assert!(!child.contains_solid([91.5, 170., 10.], CollisionQuery::All));
        assert!(!child.contains_solid([0.; 3], CollisionQuery::All));
        events.step().unwrap();
        events.step().unwrap();
        let child = &events.world.actors[&30];
        assert!(child.contains_solid([90., 143., 10.], CollisionQuery::All));
        assert!(!child.contains_solid([90., 173., 10.], CollisionQuery::All));
        assert_eq!(child.position, [0.; 3]);
        for id in [10, 20] {
            events.world.actors.get_mut(&id).unwrap().animation = None;
        }
        events.step().unwrap();
        assert!(events.world.actors[&30].contains_solid([95., 205., 4.], CollisionQuery::All));
        events.world.actors.get_mut(&30).unwrap().attachment = None;
        events.step().unwrap();
        let child = &events.world.actors[&30];
        assert!(child.contains_solid([0.; 3], CollisionQuery::All));
        assert!(!child.contains_solid([90., 143., 10.], CollisionQuery::All));
    }
}
