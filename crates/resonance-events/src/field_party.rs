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
            ensure!(self.actors.contains_key(&id), "controlled actor is missing");
            return Ok(());
        }
        ensure!((1..=9).contains(&id), "invalid playable party member {id}");
        let model = resources
            .model(id as u32)
            .context("party member model is not cooked")?;
        ensure!(
            model.clips.contains_key(&slot::IDLE),
            "party member idle animation is not cooked"
        );
        let previous = self
            .actors
            .get_mut(&self.controlled_actor)
            .context("controlled actor is missing")?;
        let mut actor = Actor::new(id as u32, previous.position);
        actor.autonomy = Some(crate::Autonomy::new(crate::Behavior::Player, 0., [0.; 3]));
        actor.face(previous.heading);
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
