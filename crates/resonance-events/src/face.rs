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

impl EventRuntime {
    pub fn eye_frame(&self, actor: &Actor) -> u8 {
        let progress = self
            .memory()
            .read(appearance::ANGEL_PROGRESS, symphonia_script::Width::S32)
            .expect("angel progress is a valid global");
        appearance::forced_face(actor.resource, progress).unwrap_or_else(|| {
            match actor.appearance.face {
                Face::Frame(frame) => frame,
                Face::Blink => actor.appearance.eyes.map_or(0, |eyes| eyes.frame),
                Face::Disabled => 0,
            }
        })
    }
}

impl GameWorld {
    pub(crate) fn update_costumes(&mut self) {
        for actor in self.actors.values_mut() {
            actor.appearance.costume_frame =
                appearance::costume_frame(actor.resource, &self.event_flags);
        }
    }

    pub(crate) fn step_eyes(
        &mut self,
        resources: &ResourceLibrary,
        colette_progress: i32,
    ) -> Result<()> {
        for actor in self.actors.values_mut() {
            if !actor.visible || !resources.model(actor.resource).is_some_and(|m| m.has_eyes) {
                continue;
            }
            if appearance::forced_face(actor.resource, colette_progress).is_some() {
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
            }
        }
        Ok(())
    }
}
