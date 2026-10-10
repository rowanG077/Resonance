//! Switching the playable character preserves the field position and follow view.
use crate::{Actor, GameWorld, ResourceLibrary, animation::slot};
use anyhow::{Context, Result, ensure};

impl GameWorld {
    pub(crate) fn select_party_member(
        &mut self,
        resources: &ResourceLibrary,
        id: i32,
    ) -> Result<()> {
        if id == self.controlled_actor {
            let actor = self
                .actors
                .get(&id)
                .context("controlled actor is missing")?;
            if actor.resource == id as u32 {
                return Ok(());
            }
        }
        self.replace_party_member(resources, id)
    }

    pub(crate) fn replace_party_member(
        &mut self,
        resources: &ResourceLibrary,
        id: i32,
    ) -> Result<()> {
        ensure!((1..=9).contains(&id), "invalid playable party member {id}");
        self.replace_controlled_model(resources, id, id as u32)
    }

    pub(crate) fn replace_controlled_model(
        &mut self,
        resources: &ResourceLibrary,
        id: i32,
        resource: u32,
    ) -> Result<()> {
        let model = resources
            .model(resource)
            .context("controlled actor model is not cooked")?;
        let idle = model
            .clips
            .get(&slot::IDLE)
            .context("party member idle animation is not cooked")?;
        let previous = self
            .actors
            .get_mut(&self.controlled_actor)
            .context("controlled actor is missing")?;
        let mut actor = Actor::new(resource, previous.position);
        actor.autonomy = Some(crate::Autonomy::new(crate::Behavior::Player, 0., [0.; 3]));
        // Model initialization evaluates its default clip before the first
        // activity selection; a later idle handler may choose the event pose.
        actor.animation = Some(crate::Animation {
            ..crate::Animation::new(resource, slot::IDLE, idle.duration_ticks, self.tick)
        });
        actor.face(previous.heading);
        actor.target_heading = previous.target_heading;
        actor
            .appearance
            .hidden_nodes
            .clone_from(&model.hidden_nodes);
        previous.visible = false;
        previous.autonomy = None;
        self.insert_actor(id, actor);
        if let Some(rig) = &mut self.field_camera {
            for camera in &mut rig.cameras {
                if camera.actor == self.controlled_actor {
                    camera.actor = id;
                }
            }
        }
        self.controlled_actor = id;
        Ok(())
    }
}
