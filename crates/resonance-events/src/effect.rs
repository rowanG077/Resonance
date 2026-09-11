//! Billboard effects expressed as ordinary position, size, rotation and lifetime.

/// An expanding world-space ripple that refracts the scene behind its plane.
#[derive(Debug, Clone)]
pub struct RefractionPulse {
    pub position: [f32; 3],
    pub born: u32,
    pub lifetime: u32,
    pub size: f32,
    pub growth: f32,
    pub alpha: f32,
    pub fade: f32,
}
impl RefractionPulse {
    pub fn sample(&self, tick: u32) -> (f32, f32) {
        let age = tick.saturating_sub(self.born) as f32;
        (
            self.size + self.growth * age,
            (self.alpha - self.fade * age).max(self.fade),
        )
    }
}

impl crate::GameWorld {
    pub fn emit_particle(&mut self, mut particle: crate::Particle) -> Result<i32, String> {
        if self.particles.len() >= 2048 {
            return Err("particle pool exhausted".into());
        }
        let handle = self.allocate_effect()?;
        particle.handle = handle;
        particle.born = self.tick;
        self.particles.push(particle);
        Ok(handle)
    }
    pub fn emit_billboard(&mut self, effect: BillboardEffect) -> Result<i32, String> {
        if self.billboards.len() >= 2048 {
            return Err("billboard effect limit exceeded".into());
        }
        let handle = self.allocate_effect()?;
        self.billboards.insert(handle, effect);
        Ok(handle)
    }
    pub fn emit_refraction(&mut self, mut effect: RefractionPulse) -> Result<i32, String> {
        if self.refractions.len() >= 16 {
            return Err("refraction effect limit exceeded".into());
        }
        let handle = self.allocate_effect()?;
        effect.born = self.tick;
        self.refractions.insert(handle, effect);
        Ok(handle)
    }
    fn allocate_effect(&mut self) -> Result<i32, String> {
        self.next_particle = self
            .next_particle
            .checked_add(1)
            .ok_or("effect handle overflow")?;
        Ok(self.next_particle)
    }
}

/// Wind-blown leaves tumble around three world axes, rather than facing the camera.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Flutter {
    pub rotation: [f32; 3],
    fall_speed: f32,
    spin: f32,
    heading: f32,
    turn_after: i32,
    // Creation leaves motion untouched until the first particle update.
    #[serde(skip)]
    initial_fall_variation: Option<f32>,
}

/// Transient motion registered once by an oracle replay, never an ordinary save.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlutterOrigin {
    pub kind: i32,
    pub age: u32,
    pub lifetime: u32,
    pub position: [f32; 3],
    pub size: f32,
    /// Current scripted tint; effect properties can override the palette.
    pub rgba: [u8; 4],
    pub motion: Flutter,
}
impl FlutterOrigin {
    pub(crate) fn particle(
        &self,
        tick: u32,
        recipe: &resonance_content::effect::FlutterRecipe,
    ) -> anyhow::Result<crate::Particle> {
        anyhow::ensure!(
            self.age <= self.lifetime
                && self.age <= tick
                && self.lifetime <= 32767
                && self
                    .position
                    .iter()
                    .chain(&self.motion.rotation)
                    .all(|v| v.is_finite())
                && self.motion.heading.is_finite()
                && (0. ..=65535.).contains(&self.size)
                && (-36..=68).contains(&self.motion.turn_after)
                && self.motion.spin == recipe.spin
                && self.motion.fall_speed
                    >= recipe.fall_speed - 31. * recipe.fall_variation - 0.000001
                && self.motion.fall_speed <= recipe.fall_speed,
            "leaf origin is outside its cooked motion recipe"
        );
        Ok(crate::Particle {
            kind: self.kind,
            handle: 0,
            born: tick - self.age,
            lifetime: self.lifetime,
            position: self.position,
            velocity: [0.; 3],
            size: self.size,
            size_delta: 0.,
            rgba: self.rgba.map(f32::from),
            alpha_delta: 0.,
            flutter: Some(self.motion.clone()),
        })
    }
}
impl Flutter {
    pub(crate) fn new(recipe: &resonance_content::effect::FlutterRecipe) -> Self {
        Self {
            rotation: [0.; 3],
            fall_speed: recipe.fall_speed,
            spin: recipe.spin,
            heading: 0.,
            turn_after: 0,
            initial_fall_variation: Some(recipe.fall_variation),
        }
    }
    pub(crate) fn step(
        &mut self,
        position: &mut [f32; 3],
        tick: u32,
        random: &mut impl FnMut() -> u32,
    ) {
        if let Some(variation) = self.initial_fall_variation.take() {
            self.turn_after = (random() & 31) as i32 + 5;
            if random() & 1 != 0 {
                self.turn_after = -self.turn_after;
            }
            self.fall_speed -= (random() & 31) as f32 * variation;
            self.rotation = std::array::from_fn(|_| random() as f32);
        }
        self.turn_after -= 1;
        if self.turn_after < 0 {
            self.turn_after = (random() & 63) as i32 + 5;
            self.heading += (random() as i32 % 90 - 45) as f32;
            self.rotation[1] -= (random() & 3) as f32;
            self.rotation[0] += (random() & 3) as f32;
        }
        self.rotation[2] += self.spin;
        let sway = (tick as f32).to_radians().sin();
        let (sin, cos) = self.heading.to_radians().sin_cos();
        position[0] += sway * cos;
        position[1] += sway * sin;
        position[2] -= self.fall_speed;
    }
}

#[cfg(test)]
mod flutter_tests {
    use super::*;

    #[test]
    fn leaf_motion_matches_consecutive_dolphin_observations() {
        // Three consecutive field-332 observations, registered to the wind clock.
        let motion = Flutter {
            rotation: [29949., 21033., 23432.922],
            fall_speed: 1.74,
            spin: 0.2,
            heading: -37.,
            turn_after: 10,
            initial_fall_variation: None,
        };
        let mut world = crate::GameWorld {
            tick: 100,
            ..Default::default()
        };
        world.particles.push(crate::Particle {
            kind: 25,
            handle: 1,
            born: 100,
            lifetime: 150,
            position: [2815.3848, 979.9171, 105.99975],
            velocity: [0.; 3],
            size: 25.,
            size_delta: 0.,
            rgba: [13., 60., 4., 255.],
            alpha_delta: 0.,
            flutter: Some(motion),
        });
        let program =
            symphonia_script::Program::decode(&[0, 4, 0, 0, 0, 0, 0, 0, 0x20, 0xff]).unwrap();
        let mut events = crate::EventRuntime::with_state(
            std::sync::Arc::new(program),
            Default::default(),
            world,
            Default::default(),
        )
        .unwrap();
        let seed = events.world.random_state;
        for (tick, expected, angle) in [
            (36917, [2815.1514, 980.0931, 104.25975], 23433.121),
            (36918, [2814.9045, 980.27905, 102.51975], 23433.32),
            (36919, [2814.6445, 980.475, 100.779755], 23433.52),
        ] {
            events
                .step_with_motion(tick, |_| Ok(()), |_, _, _, _| {}, |_| Ok(()))
                .unwrap();
            let particle = &events.world.particles[0];
            for (actual, expected) in particle.position.into_iter().zip(expected) {
                assert!((actual - expected).abs() < 0.0003, "{actual} != {expected}");
            }
            assert_eq!(
                particle.flutter.as_ref().unwrap().rotation,
                [29949., 21033., angle]
            );
        }
        assert_eq!(events.world.tick, 103);
        assert_eq!(events.world.random_state, seed);
        let particle = &mut events.world.particles[0];
        particle.position = [2898.4536, 872.2535, 55.520428];
        particle.flutter = Some(Flutter {
            rotation: [23037., 18576., 25593.305],
            fall_speed: 1.84,
            spin: 0.2,
            heading: -49.,
            turn_after: 0,
            initial_fall_variation: None,
        });
        events.world.random_state = 594934361;
        events
            .step_with_motion(37561, |_| Ok(()), |_, _, _, _| {}, |_| Ok(()))
            .unwrap();
        let particle = &events.world.particles[0];
        for (actual, expected) in particle
            .position
            .into_iter()
            .zip([2898.4985, 871.39746, 53.680428])
        {
            assert!((actual - expected).abs() < 0.0003);
        }
        let motion = particle.flutter.as_ref().unwrap();
        assert_eq!((motion.heading, motion.turn_after), (-87., 29));
        assert_eq!(motion.rotation, [23039., 18576., 25593.504]);
        assert_eq!(events.world.random_state, 1417863957);
        assert_eq!(particle.alpha(219), 255.);
        assert_eq!(particle.alpha(220), 247.);
        assert_eq!(particle.alpha(250), 7.);
        assert!(particle.alive(250));
        assert!(!particle.alive(251));
    }
}

/// Per-character light colors and transitions.
#[derive(Debug, Clone, PartialEq)]
pub struct CharacterLight {
    pub position: LightPosition,
    pub strength: u8,
    pub shade: [u8; 3],
    pub bright: [u8; 3],
    pub color_step: u16,
    pub mode: u8,
}
#[derive(Debug, Clone, PartialEq)]
pub enum LightPosition {
    Relative([f32; 3]),
    World([f32; 3]),
    Actor { id: i32, height: f32 },
}
impl Default for CharacterLight {
    fn default() -> Self {
        Self {
            position: LightPosition::Relative([60000., -60000., 140.]),
            strength: 192,
            shade: [48; 3],
            bright: [64; 3],
            color_step: 4,
            mode: 0,
        }
    }
}
impl CharacterLight {
    pub(crate) fn set(&mut self, operation: i32, value: [i32; 3]) -> Result<(), String> {
        let color = || value.map(|v| (v / 4) as u8);
        match operation {
            0 => {
                self.bright = color();
                self.shade = self.bright.map(|v| (f32::from(v) * 0.75) as u8);
            }
            1 => self.mode = value[0] as u8,
            2 => self.strength = value[0] as u8,
            3 => self.position = LightPosition::Relative(value.map(|v| v as f32)),
            4 => self.position = LightPosition::World(value.map(|v| v as f32)),
            6 => {
                self.position = LightPosition::Actor {
                    id: value[0],
                    height: value[1] as f32,
                }
            }
            7 => self.bright = color(),
            8 => self.shade = color(),
            9 => self.color_step = value[0] as u16,
            _ => return Err("character light operation is not implemented".into()),
        }
        Ok(())
    }
    /// Approach each color channel by the configured step.
    pub fn approach(&mut self, target: &Self) {
        let step = i32::from(target.color_step);
        let approach = |current: [u8; 3], next: [u8; 3]| {
            std::array::from_fn(|i| {
                let delta = i32::from(next[i]) - i32::from(current[i]);
                (i32::from(current[i]) + delta.clamp(-step, step)) as u8
            })
        };
        let shade = approach(self.shade, target.shade);
        let bright = approach(self.bright, target.bright);
        *self = target.clone();
        self.shade = shade;
        self.bright = bright;
    }
}

#[derive(Debug, Clone)]
pub struct BillboardEffect {
    pub recipe: u16,
    pub born: u32,
    pub lifetime: u32,
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub rotation: [f32; 3],
    pub angular_velocity: [f32; 3],
    pub size: [f32; 2],
    pub size_delta: f32,
    pub rgba: [u8; 4],
    pub alpha_delta: f32,
}
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct Paralysis {
    pub actor: i32,
    pub frame: u8,
}
impl BillboardEffect {
    pub fn poison(position: [f32; 3], size: f32, speed: f32, born: u32) -> Self {
        Self {
            recipe: 10,
            born,
            lifetime: 21,
            position,
            velocity: [0., 0., speed],
            rotation: [0.; 3],
            angular_velocity: [0.; 3],
            size: [size; 2],
            size_delta: 0.,
            rgba: [13, 63, 4, 255],
            alpha_delta: 0.,
        }
    }

    pub fn rising_spark(
        position: [f32; 3],
        size: f32,
        speed: f32,
        born: u32,
        effect_tick: u32,
    ) -> Self {
        Self {
            recipe: 8,
            born,
            // Include the birth pose and the final timer-zero pose.
            lifetime: 61,
            position,
            velocity: [0., 0., speed],
            rotation: [0., 0., (effect_tick & 127) as f32],
            angular_velocity: [0., 0., 1.],
            size: [size; 2],
            size_delta: 0.,
            rgba: [64, 64, 64, 255],
            alpha_delta: 0.,
        }
    }

    pub fn step(&mut self) {
        for i in 0..3 {
            self.position[i] += self.velocity[i];
            self.rotation[i] += self.angular_velocity[i];
        }
        for size in &mut self.size {
            *size += self.size_delta;
        }
    }
    pub fn alive(&self, tick: u32) -> bool {
        tick.saturating_sub(self.born) < self.lifetime
            && self.size.iter().all(|s| *s >= 0.)
            && self.alpha(tick) >= 0.
    }
    pub fn alpha(&self, tick: u32) -> f32 {
        f32::from(self.rgba[3]) + self.alpha_delta * tick.saturating_sub(self.born) as f32
    }
}
