use crate::{Actor, EventRuntime, Face, GameWorld, ResourceLibrary};
use anyhow::{Context, Result};
use resonance_content::{appearance, effect::BlinkCycle};

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EyeBlink {
    pub frame: u8,
    /// Next fixed update's sample; rendering retains the current frame.
    pub tick: u16,
}
impl EyeBlink {
    fn new(cycle: &BlinkCycle, random: u32) -> Self {
        Self {
            frame: 0,
            tick: (random as usize % cycle.frames.len()) as u16,
        }
    }
    fn step(&mut self, cycle: &BlinkCycle) {
        self.frame = cycle.frames[usize::from(self.tick)];
        self.tick = (usize::from(self.tick + 1) % cycle.frames.len()) as u16;
    }
}

impl Actor {
    pub(crate) fn scripted_eye_frame(&self) -> u8 {
        self.rendered_eye_frame
            .unwrap_or_else(|| match self.appearance.face {
                Face::Frame(frame) => frame,
                Face::Blink => self.appearance.eyes.map_or(0, |eyes| eyes.frame),
                Face::Disabled => 0,
            })
    }
}

impl EventRuntime {
    pub(crate) fn angel_progress(&self) -> i32 {
        self.memory()
            .read(appearance::ANGEL_PROGRESS, symphonia_script::Width::S32)
            .expect("angel progress is a valid global")
    }

    pub fn eye_frame(&self, actor: &Actor) -> u8 {
        let progress = if actor.sampled_field_model.is_some() {
            self.world.sampled_angel_progress
        } else {
            None
        }
        .unwrap_or_else(|| self.angel_progress());
        appearance::forced_face(actor.resource, progress)
            .unwrap_or_else(|| actor.scripted_eye_frame())
    }
}

impl GameWorld {
    pub(crate) fn update_costumes(&mut self, resources: &ResourceLibrary) {
        for (&id, actor) in &mut self.actors {
            let costume = self
                .party
                .as_ref()
                .and_then(|party| party.members.get(actor.resource.wrapping_sub(1) as usize))
                .map_or(0, |member| member.costume);
            if actor.appearance.costume != costume {
                Self::retain_sampled_actor(&mut self.retained_field_models, id, actor);
                actor.appearance.costume = costume;
                // Accessory node indices belong to the selected body.
                actor.appearance.hidden_nodes = resources
                    .model(actor.model_resource())
                    .map(|model| model.hidden_nodes.clone())
                    .unwrap_or_default();
            }
            actor.appearance.costume_frame =
                appearance::costume_frame(actor.resource, &self.event_flags);
            if let Some(name) = appearance::hidden_accessory(actor.resource, &self.event_flags)
                && let Some(model) = resources.model(actor.model_resource())
                && let Some(index) = model.names.iter().position(|node| node == name)
                && !actor.appearance.hidden_nodes.contains(&(index as u16))
            {
                Self::retain_sampled_actor(&mut self.retained_field_models, id, actor);
                actor.appearance.hidden_nodes.insert(index as u16);
            }
        }
    }

    pub(crate) fn step_eyes(
        &mut self,
        resources: &ResourceLibrary,
        colette_progress: i32,
    ) -> Result<()> {
        for (&id, actor) in &mut self.actors {
            if !actor.visible
                || !resources
                    .model(actor.model_resource())
                    .is_some_and(|m| m.has_eyes)
            {
                continue;
            }
            if appearance::forced_face(actor.resource, colette_progress).is_some() {
                if actor.sampled_field_model.is_some() && actor.appearance.eyes.is_some() {
                    actor.rendered_eye_frame = Some(actor.scripted_eye_frame());
                }
                actor.appearance.eyes = None;
                continue;
            }
            if matches!(actor.appearance.face, Face::Blink) {
                let cycle = resources
                    .blink
                    .as_ref()
                    .context("eye blink animation is not cooked")?;
                let eyes = actor.appearance.eyes.get_or_insert_with(|| {
                    EyeBlink::new(cycle, crate::world::random(&mut self.random_state))
                });
                eyes.step(cycle);
                // A bone or accessory edit retains the model through this render.
                // Its blink still advances with the actor that owns the eyes.
                if let Some(model) = self.retained_field_models.get_mut(&id)
                    && model.instance == actor.instance
                {
                    model.rendered_eye_frame = Some(actor.scripted_eye_frame());
                }
            }
        }
        Ok(())
    }
}
