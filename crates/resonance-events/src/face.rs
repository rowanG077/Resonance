use crate::{Face, GameWorld, ResourceLibrary};
use anyhow::{Context, Result};
use resonance_content::effect::BlinkCycle;

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
            tick: cycle.initial_tick + (random % u32::from(cycle.initial_spread)) as u16,
        }
    }
    fn step(&mut self, cycle: &BlinkCycle) {
        self.frame = cycle.frames[usize::from(self.tick)];
        self.tick = (usize::from(self.tick + 1) % cycle.frames.len()) as u16;
    }
}

impl GameWorld {
    pub(crate) fn step_eyes(&mut self, resources: &ResourceLibrary) -> Result<()> {
        for actor in self.actors.values_mut() {
            if !actor.visible || !resources.model(actor.resource).is_some_and(|m| m.has_eyes) {
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
