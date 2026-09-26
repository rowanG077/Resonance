//! Vehicle artwork clocks advance with travel, including its menu/battle pause.
use super::*;

pub(super) const WAKE_COUNT: usize = 24;
pub(super) const WAKE_RATE: f32 = 1. / 12.;

pub(super) struct Wake {
    pub position: Position,
    pub age: u32,
    pub duration: u32,
}

pub(super) struct Effects {
    pub wakes: [Option<Wake>; WAKE_COUNT],
    cooldown: u8,
    random: u32,
}
impl Default for Effects {
    fn default() -> Self {
        Self {
            wakes: std::array::from_fn(|_| None),
            cooldown: 0,
            random: 1,
        }
    }
}
impl Effects {
    pub fn step(
        &mut self,
        session: &game::Session,
        models: &BTreeMap<Model, Vec<ScenePart>>,
    ) -> Result<()> {
        if session.travel.state().mount != Mount::Ship {
            self.wakes = std::array::from_fn(|_| None);
            self.cooldown = 0;
            return Ok(());
        }
        if !session.player_has_control() {
            return Ok(());
        }
        for slot in &mut self.wakes {
            if let Some(wake) = slot {
                wake.age += 1;
                if wake.age >= wake.duration {
                    *slot = None;
                }
            }
        }
        if self.cooldown != 0 {
            self.cooldown -= 1;
            return Ok(());
        }
        let Some(index) = self.wakes.iter().position(Option::is_none) else {
            return Ok(());
        };
        let duration = models[&Model::Actor(213)]
            .iter()
            .flat_map(|part| &part.clips)
            .find(|clip| clip.resource_slot == 0)
            .context("ship wake animation is missing")?
            .duration_seconds;
        // The native pool emits every seven updates, even when idling. Keep
        // cosmetic jitter separate from the persistent battle formation RNG.
        let x = self.draw() % 200;
        let z = self.draw() % 400;
        let state = session.travel.state();
        let position = state.position.translated([
            0.6 * (x as f32 - 100.) * state.camera_yaw.sin(),
            0.6 * (z as f32 - 300.) * state.camera_yaw.cos(),
            -state.position.map()[2],
        ])?;
        self.wakes[index] = Some(Wake {
            position,
            age: 0,
            duration: (duration * resonance_game::clock::UPDATE_HZ as f32 / WAKE_RATE).ceil()
                as u32,
        });
        self.cooldown = 6;
        Ok(())
    }
    fn draw(&mut self) -> u32 {
        self.random = self.random.wrapping_mul(0x41c64e6d).wrapping_add(0x3039);
        self.random >> 16 & 0x7fff
    }
}
