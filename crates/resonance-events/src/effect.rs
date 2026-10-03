//! Billboard effects expressed as ordinary position, size, rotation and lifetime.
pub(crate) mod station;
pub(crate) const BILLBOARD_LIMIT: usize = 2048;
pub const NEUTRAL_TINT: u8 = 64;
pub const NEUTRAL_PALETTE: u8 = 0;
pub(crate) const GLOW_SPRITE: u16 = 0;
pub(crate) const STATION_GLOW_SPRITE: u16 = 4;
pub(crate) const CAMERA_DISC_SPRITE: u16 = 5;
pub(crate) const WORLD_GLOW_SPRITE: u16 = 6;
pub(crate) const STAR_SPRITE: u16 = 7;
pub(crate) const SPINNING_STAR_SPRITE: u16 = 8;
pub(crate) const ORB_SPRITE: u16 = 10;
pub(crate) const RING_SPRITE: u16 = 41;
pub(crate) const ELECTRIC_SPARK_SPRITE: u16 = 42;

/// A stationary origin whose effects share one authored task's lifetime.
pub(crate) struct EffectContext {
    pub task: i32,
    pub position: [f32; 3],
    pub operation: crate::Operation,
}

/// An enemy reaction survives the projectile that applied it.
#[derive(Debug, Clone, Copy)]
pub struct Stun {
    pub remaining: std::num::NonZeroU16,
    pub effect: StunEffect,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StunEffect {
    None,
    Electric,
    Lightning,
    Ice,
    Darkness,
}
impl StunEffect {
    pub fn tint(self) -> Option<[u8; 3]> {
        match self {
            Self::Electric => Some([128; 3]),
            // fn_800111D4's default stun mode includes ordinary fire. `None`
            // means no additional particle effect, not an unchanged model tint.
            Self::None | Self::Lightning | Self::Ice | Self::Darkness => Some([40, 40, 255]),
        }
    }
}

/// An expanding world-space ripple that refracts the scene behind its plane.
#[derive(Debug, Clone)]
pub struct RefractionPulse {
    pub operation: Option<crate::Operation>,
    pub owner: Option<i32>,
    pub image: RefractionImage,
    pub palette: u8,
    pub orientation: SpriteOrientation,
    pub rotation: [f32; 3],
    pub position: [f32; 3],
    pub born: u32,
    pub lifetime: u32,
    pub size: f32,
    pub growth: f32,
    pub alpha: f32,
    pub fade: Fade,
}
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum RefractionImage {
    Ripple,
    Air,
}
impl RefractionPulse {
    pub fn sample(&self, tick: u32) -> (f32, f32) {
        let age = tick.saturating_sub(self.born) as f32;
        (
            self.size + self.growth * age,
            self.fade.alpha(self.alpha, tick.saturating_sub(self.born)),
        )
    }
}

impl crate::GameWorld {
    pub(crate) fn emit_stun_effect(
        &mut self,
        id: i32,
        resources: &crate::ResourceLibrary,
    ) -> Result<(), String> {
        let actor = &self.actors[&id];
        if !actor
            .enemy
            .as_ref()
            .and_then(crate::Enemy::stun_effect)
            .is_some_and(|effect| matches!(effect, StunEffect::Electric | StunEffect::Lightning))
        {
            return Ok(());
        }
        let count = resources.model(actor.resource).map_or(0, |m| m.names.len());
        if count == 0 {
            return Ok(());
        }
        let node = self.random() as usize % count;
        let position = resources.attachment_point(&self.actors[&id], node, self.tick)?;
        const SPARK_LIFETIME: u32 = 9;
        let width = (16 + (self.random() & 15)) as f32;
        // Native draw mode 8 uses the bottom half of the quad with the full UVs.
        let height = (64 + (self.random() & 15)) as f32;
        let rotation = std::array::from_fn(|_| self.random() as f32);
        self.emit_billboard(BillboardEffect {
            recipe: ELECTRIC_SPARK_SPRITE,
            orientation: SpriteOrientation::World,
            anchor: resonance_content::effect::VerticalAnchor::LowerHalf,
            palette: Some(2),
            born: self.tick,
            lifetime: SPARK_LIFETIME,
            position,
            rotation,
            size: [width, height],
            rgba: [32, 32, 255, 255],
            fade: Fade::tail(SPARK_LIFETIME),
            blend_mode: Some(1),
            ..Default::default()
        })?;
        Ok(())
    }

    pub(crate) fn step_ring_stations(&mut self) -> Result<(), String> {
        // fn_8007BF58 / fn_8007C964: the ring pedestal spins beneath four
        // short-lived glows. Its script owns the selected ring power.
        let stations: Vec<_> = self
            .actors
            .values_mut()
            .filter(|a| a.ring_station && a.visible)
            .map(|actor| {
                actor.face((self.tick % 360) as f32);
                let mut position = actor.position;
                position[2] += (self.tick as f32).to_radians().sin() * 10. + 150.;
                let rgb = actor.station_color();
                (position, rgb)
            })
            .collect();
        for (position, rgb) in stations {
            for (index, (recipe, lifetime, base, mask, alpha, fade, rotation, blend)) in [
                (4, 1, 48, 7, 64, -16., 0., None),
                (4, 2, 40, 3, 128, -64., 0., Some(0)),
                (22, 4, 80, 3, 255, -48., self.tick as f32 * 4., None),
                (22, 4, 80, 3, 255, -48., -(self.tick as f32) * 8., None),
            ]
            .into_iter()
            .enumerate()
            {
                let size = (base + (self.random() & mask)) as f32;
                let color = if index < 2 { rgb } else { [NEUTRAL_TINT; 3] };
                self.emit_billboard(BillboardEffect {
                    field_lighting: true,
                    palette: (index >= 2).then_some(0),
                    recipe,
                    born: self.tick,
                    lifetime,
                    position,
                    rotation: [rotation, 0., 0.],
                    size: [size; 2],
                    rgba: [color[0], color[1], color[2], alpha],
                    fade: Fade::Linear(fade),
                    blend_mode: blend,
                    ..Default::default()
                })?;
            }
        }
        Ok(())
    }
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
        if self.billboards.len() >= BILLBOARD_LIMIT {
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
    pub(crate) fn allocate_effect(&mut self) -> Result<i32, String> {
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
    pub(crate) fn initialize(&mut self, random: &mut impl FnMut() -> u32) {
        if let Some(variation) = self.initial_fall_variation.take() {
            self.turn_after = (random() & 31) as i32 + 5;
            if random() & 1 != 0 {
                self.turn_after = -self.turn_after;
            }
            self.fall_speed -= (random() & 31) as f32 * variation;
            self.rotation = std::array::from_fn(|_| random() as f32);
        }
    }
    pub(crate) fn step(
        &mut self,
        position: &mut [f32; 3],
        tick: u32,
        random: &mut impl FnMut() -> u32,
    ) {
        self.initialize(random);
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
                .step_with_motion(tick, |_| Ok(()), |_, _, _, _, _| {}, |_| Ok(()))
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
            .step_with_motion(37561, |_| Ok(()), |_, _, _, _, _| {}, |_| Ok(()))
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
            5 => {
                // fn_8004F1AC rotates (0, -10000, 0) by Rx * Rz.
                let (sx, cx) = (value[0] as f32).to_radians().sin_cos();
                let (sz, cz) = (value[2] as f32).to_radians().sin_cos();
                self.position =
                    LightPosition::Relative([10000. * sz, -10000. * cz * cx, -10000. * cz * sx]);
            }
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
    pub operation: Option<crate::Operation>,
    pub owner: Option<i32>,
    pub field_lighting: bool,
    pub field_fog: bool,
    pub recipe: u16,
    pub orientation: SpriteOrientation,
    pub anchor: resonance_content::effect::VerticalAnchor,
    /// Original palette index; neutral RGB channels preserve its color.
    pub palette: Option<u16>,
    pub born: u32,
    pub lifetime: u32,
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub acceleration: Option<f32>,
    pub(crate) controller: Option<BillboardController>,
    pub gravity: f32,
    pub rotation: [f32; 3],
    pub angular_velocity: [f32; 3],
    pub size: [f32; 2],
    pub size_delta: f32,
    pub rgba: [u8; 4],
    pub fade: Fade,
    pub blend_mode: Option<u8>,
}
#[derive(Debug, Clone, Copy)]
pub enum SpriteOrientation {
    Camera,
    World,
}

#[derive(Debug, Clone)]
pub(crate) enum BillboardController {
    Cardinal(crate::emitter::cardinal::CardinalMotion),
    Inward(crate::emitter::inward::Motion),
    Flutter(Flutter),
    RisingWander {
        direction: [f32; 3],
        speed: f32,
    },
    CameraOffset {
        emitter: i32,
        center: [f32; 3],
        distance: f32,
    },
    Wander {
        direction: [f32; 3],
        speed: f32,
        gravity: f32,
    },
    Spiral {
        center: [f32; 3],
        radius: f32,
    },
    Directed {
        direction: [f32; 3],
        speed: f32,
        gravity: f32,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum Fade {
    Linear(f32),
    /// fn_80086654's fixed-point light shaft: rise, then reverse its alpha step.
    RiseFall {
        rise_ticks: u32,
        step: f32,
    },
    /// Fade to zero by expiry, starting when selected by a script.
    Proportional {
        after: u32,
        lifetime: u32,
    },
    /// Fade by eight alpha units per tick near expiry.
    Tail {
        after: u32,
    },
}
impl Fade {
    pub const fn tail(lifetime: u32) -> Self {
        const TAIL_UPDATES: u32 = 32;
        Self::Tail {
            after: lifetime.saturating_sub(TAIL_UPDATES),
        }
    }
    fn alpha(self, alpha: f32, age: u32) -> f32 {
        match self {
            Self::Linear(delta) => (alpha + delta * age as f32).floor(),
            Self::RiseFall { rise_ticks, step } => {
                let ramp = if age <= rise_ticks {
                    age.max(1) as f32
                } else {
                    (2 * rise_ticks + 1) as f32 - age as f32
                };
                (ramp * step).floor()
            }
            Self::Proportional { after, lifetime } => {
                let duration = lifetime.saturating_sub(after).max(1);
                alpha * (1. - age.saturating_sub(after) as f32 / duration as f32).clamp(0., 1.)
            }
            Self::Tail { after } => (alpha - 8. * age.saturating_sub(after) as f32).max(0.),
        }
    }
}
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct Paralysis {
    pub actor: i32,
    pub frame: u8,
}
impl Default for BillboardEffect {
    fn default() -> Self {
        Self {
            operation: None,
            owner: None,
            field_lighting: false,
            field_fog: true,
            recipe: 0,
            orientation: SpriteOrientation::Camera,
            anchor: resonance_content::effect::VerticalAnchor::Center,
            palette: None,
            born: 0,
            lifetime: 0,
            position: [0.; 3],
            velocity: [0.; 3],
            acceleration: None,
            controller: None,
            gravity: 0.,
            rotation: [0.; 3],
            angular_velocity: [0.; 3],
            size: [0.; 2],
            size_delta: 0.,
            rgba: [NEUTRAL_TINT, NEUTRAL_TINT, NEUTRAL_TINT, 255],
            fade: Fade::Linear(0.),
            blend_mode: None,
        }
    }
}

impl BillboardEffect {
    pub(crate) fn advance(&mut self, tick: u32, random: &mut u32) {
        if let Some(controller) = &mut self.controller {
            let wandering = matches!(controller, BillboardController::Wander { .. });
            match controller {
                BillboardController::Cardinal(motion) => motion.advance(&mut self.position),
                BillboardController::Inward(_) => return,
                BillboardController::Flutter(flutter) => {
                    flutter.step(&mut self.position, tick, &mut || {
                        crate::world::random(random)
                    });
                    self.rotation = flutter.rotation;
                }
                BillboardController::RisingWander { direction, speed } => {
                    for value in &mut direction[..2] {
                        *value += if crate::world::random(random) & 1 != 0 {
                            2.5
                        } else {
                            -2.5
                        };
                    }
                    let length = direction.iter().map(|v| v * v).sum::<f32>().sqrt();
                    self.velocity = direction.map(|v| v / length * *speed);
                }
                BillboardController::CameraOffset { .. } => {}
                BillboardController::Wander {
                    direction,
                    speed,
                    gravity,
                }
                | BillboardController::Directed {
                    direction,
                    speed,
                    gravity,
                } => {
                    let moving_axis = if wandering {
                        Some(if crate::world::random(random) & 1 == 0 {
                            (0, 2)
                        } else {
                            (2, 0)
                        })
                    } else {
                        None
                    };
                    // fn_800863B4 perturbs the X/Z direction before normalizing.
                    if let Some((source, target)) = moving_axis
                        && direction[source] != 0.
                    {
                        direction[target] += if crate::world::random(random) & 1 != 0 {
                            2.
                        } else {
                            -2.
                        };
                    }
                    let length = direction.iter().map(|v| v * v).sum::<f32>().sqrt();
                    self.velocity = direction.map(|v| {
                        if length == 0. {
                            0.
                        } else {
                            v / length * *speed
                        }
                    });
                    direction[2] += *gravity;
                }
                BillboardController::Spiral { center, radius } => {
                    *radius += 1.;
                    let [x, _, z] = std::array::from_fn(|i| self.position[i] - center[i]);
                    let angle = z.atan2(x) - 0.1_f32.to_radians();
                    self.position = [
                        center[0] + angle.cos() * *radius,
                        center[1],
                        center[2] + angle.sin() * *radius,
                    ];
                }
            }
        }
        self.step();
    }
    pub fn poison(position: [f32; 3], size: f32, speed: f32, born: u32) -> Self {
        Self {
            operation: None,
            owner: None,
            field_lighting: true,
            field_fog: true,
            orientation: crate::effect::SpriteOrientation::Camera,
            anchor: resonance_content::effect::VerticalAnchor::Center,
            palette: None,
            controller: None,
            acceleration: None,
            gravity: 0.,
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
            fade: Fade::Linear(0.),
            blend_mode: None,
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
            operation: None,
            owner: None,
            field_lighting: true,
            field_fog: true,
            orientation: crate::effect::SpriteOrientation::Camera,
            anchor: resonance_content::effect::VerticalAnchor::Center,
            palette: None,
            controller: None,
            acceleration: None,
            gravity: 0.,
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
            fade: Fade::Linear(0.),
            blend_mode: None,
        }
    }

    pub fn step(&mut self) {
        if let Some(gain) = self.acceleration {
            self.velocity = self.velocity.map(|v| v * gain);
        }
        for i in 0..3 {
            self.position[i] += self.velocity[i];
            self.rotation[i] += self.angular_velocity[i];
        }
        self.velocity[2] += self.gravity;
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
        self.fade
            .alpha(f32::from(self.rgba[3]), tick.saturating_sub(self.born))
    }
}
