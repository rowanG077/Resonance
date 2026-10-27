//! Vehicle artwork clocks advance with travel, including its menu/battle pause.
use super::*;

pub(super) const WAKE_COUNT: usize = 24;
pub(super) const WAKE_RATE: f32 = 1. / 12.;
pub(super) const BARRIER_MODEL: u16 = 215;
pub(super) const PORTAL_MODEL: u16 = 216;
const BARRIER_DURATION: u32 = 80;

/// Horizontal cloud loops for the ordinary and endgame skies, in 60 Hz ticks.
pub(super) fn sky_scroll(variant: usize, texture: usize, tick: u32) -> f32 {
    const PERIODS: [&[u32]; 2] = [&[12288, 16384], &[12288, 12288, 8192]];
    let period = PERIODS[variant][texture];
    (tick % period) as f32 / period as f32
}

pub(super) fn portal_uv(age: u32, texture: usize) -> f32 {
    match texture {
        1 if (30..90).contains(&age) => (age - 30) as f32 / 60.,
        2 if age < 50 => age as f32 / 50.,
        _ => 0.,
    }
}

#[derive(Clone)]
pub(super) struct Barrier {
    pub position: Position,
    pub heading: f32,
    age: u32,
}
impl Barrier {
    pub fn uv_offset(&self) -> f32 {
        1. - self.age as f32 / BARRIER_DURATION as f32
    }
}

#[derive(Clone)]
pub(super) struct Wake {
    pub position: Position,
    pub age: u32,
    pub duration: u32,
}

#[derive(Clone)]
pub(super) struct Effects {
    pub(super) particles: super::particles::Particles,
    pub wakes: [Option<Wake>; WAKE_COUNT],
    pub barriers: Vec<Option<Barrier>>,
    cooldown: u8,
    random: u32,
}
impl Default for Effects {
    fn default() -> Self {
        Self {
            particles: Default::default(),
            wakes: std::array::from_fn(|_| None),
            barriers: Vec::new(),
            cooldown: 0,
            random: 1,
        }
    }
}
impl Effects {
    pub fn hit_barrier(&mut self, position: Position, heading: f32) {
        let barrier = Some(Barrier {
            position,
            heading,
            age: 0,
        });
        if let Some(slot) = self.barriers.iter_mut().find(|slot| slot.is_none()) {
            *slot = barrier;
        } else {
            self.barriers.push(barrier);
        }
    }

    pub fn step(
        &mut self,
        session: &game::Session,
        models: &BTreeMap<Model, Vec<ScenePart>>,
    ) -> Result<()> {
        self.particles.step(session, models)?;
        for slot in &mut self.barriers {
            if let Some(barrier) = slot {
                barrier.age += 1;
                if barrier.age >= BARRIER_DURATION {
                    *slot = None;
                }
            }
        }
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
            duration: (duration * resonance_content::ANIMATION_HZ / WAKE_RATE).ceil() as u32,
        });
        self.cooldown = 6;
        Ok(())
    }
    fn draw(&mut self) -> u32 {
        self.random = self.random.wrapping_mul(0x41c64e6d).wrapping_add(0x3039);
        self.random >> 16 & 0x7fff
    }
}
