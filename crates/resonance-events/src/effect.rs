//! Billboard effects expressed as ordinary position, size, rotation and lifetime.
pub(crate) mod emission;
pub(crate) mod ring;
pub(crate) mod station;
pub(crate) const BILLBOARD_LIMIT: usize = 2048;
pub const NEUTRAL_TINT: u8 = 64;
pub const NEUTRAL_PALETTE: u8 = 0;
pub(crate) const GLOW_SPRITE: u16 = 0;
pub(crate) const STATION_GLOW_SPRITE: u16 = 4;
pub(crate) const STATION_HALO_SPRITE: u16 = 22;
pub(crate) const CAMERA_DISC_SPRITE: u16 = 5;
pub(crate) const WORLD_GLOW_SPRITE: u16 = 6;
pub(crate) const STAR_SPRITE: u16 = 7;
pub(crate) const SPINNING_STAR_SPRITE: u16 = 8;
pub(crate) const ORB_SPRITE: u16 = 10;
pub(crate) const RING_SPRITE: u16 = 41;
pub(crate) const ELECTRIC_SPARK_SPRITE: u16 = 42;
pub(crate) const ELECTRIC_ARC_SPRITE: u16 = 14;
pub(crate) const FLAME_SPRITE: u16 = 11;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Blend {
    Alpha,
    Additive,
    Subtractive,
}
impl TryFrom<i32> for Blend {
    type Error = String;
    fn try_from(value: i32) -> Result<Self, String> {
        match value & 3 {
            0 => Ok(Self::Alpha),
            1 => Ok(Self::Additive),
            2 => Ok(Self::Subtractive),
            _ => Err("inherited blend requires a sprite recipe".into()),
        }
    }
}
impl Blend {
    pub const ALL: [Self; 3] = [Self::Alpha, Self::Additive, Self::Subtractive];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StunEffect {
    None,
    Electric,
    Lightning,
    Ice,
    Darkness,
    TetheallaElectric,
}
impl StunEffect {
    pub const ALL: [Self; 6] = [
        Self::None,
        Self::Electric,
        Self::Lightning,
        Self::Ice,
        Self::Darkness,
        Self::TetheallaElectric,
    ];

    pub fn tint(self, tick: u32) -> Option<[u8; 3]> {
        match self {
            Self::Electric | Self::Lightning => Some(if tick % 10 < 5 {
                [128; 3]
            } else {
                [80, 64, 160]
            }),
            Self::None | Self::Ice | Self::Darkness | Self::TetheallaElectric => {
                Some([40, 40, 255])
            }
        }
    }
}

/// An expanding world-space ripple that refracts the scene behind its plane.
#[derive(Debug, Clone)]
pub struct RefractionPulse {
    pub draw_order: usize,
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
        // Sparks extend downward from their attachment point.
        let height = (64 + (self.random() & 15)) as f32;
        let rotation = std::array::from_fn(|_| self.random() as f32);
        self.emit_billboard(BillboardEffect {
            recipe: ELECTRIC_SPARK_SPRITE,
            orientation: SpriteOrientation::World,
            anchor: resonance_content::effect::VerticalAnchor::Top,
            palette: None,
            born: self.tick,
            lifetime: SPARK_LIFETIME,
            position,
            rotation,
            size: [width, height],
            rgba: [32, 32, 255, 247],
            fade: Fade::Linear(-8.),
            blend: Some(crate::effect::Blend::Additive),
            ..Default::default()
        })?;
        Ok(())
    }

    pub(crate) fn step_ring_stations(&mut self) -> Result<(), String> {
        let stations: Vec<_> = self
            .actors
            .values_mut()
            .filter(|a| a.ring_station && a.visible)
            .map(|actor| {
                actor.face((self.tick % 360) as f32);
                let mut position = actor.position;
                position[2] += (self.tick as f32).to_radians().sin() * 10. + 150.;
                let rgb = actor.tint;
                (position, rgb)
            })
            .collect();
        let turn = self.effect_tick as f32 * 4.;
        for (position, rgb) in stations {
            let tint = |alpha| [rgb[0], rgb[1], rgb[2], alpha];
            let neutral = |alpha| [NEUTRAL_TINT, NEUTRAL_TINT, NEUTRAL_TINT, alpha];
            for (recipe, lifetime, base, mask, rgba, fade, rotation, blend) in [
                (STATION_GLOW_SPRITE, 2, 48, 7, tint(48), -16., 0., None),
                (
                    STATION_GLOW_SPRITE,
                    3,
                    40,
                    3,
                    tint(64),
                    -64.,
                    0.,
                    Some(Blend::Alpha),
                ),
                (
                    STATION_HALO_SPRITE,
                    5,
                    80,
                    3,
                    neutral(207),
                    -48.,
                    turn,
                    None,
                ),
                (
                    STATION_HALO_SPRITE,
                    5,
                    80,
                    3,
                    neutral(207),
                    -48.,
                    -turn * 2.,
                    None,
                ),
            ] {
                let size = (base + (self.random() & mask)) as f32;
                self.emit_billboard(BillboardEffect {
                    field_lighting: true,
                    palette: (recipe == STATION_HALO_SPRITE).then_some(u16::from(NEUTRAL_PALETTE)),
                    recipe,
                    born: self.tick,
                    lifetime,
                    position,
                    rotation: [0., 0., rotation],
                    size: [size; 2],
                    rgba,
                    fade: Fade::Linear(fade),
                    blend,
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
    pub fn emit_billboard(&mut self, mut effect: BillboardEffect) -> Result<i32, String> {
        effect.draw_order = self.effect_draw_order()?;
        let handle = self.allocate_effect()?;
        self.billboards.insert(handle, effect);
        Ok(handle)
    }
    pub fn emit_refraction(&mut self, mut effect: RefractionPulse) -> Result<i32, String> {
        if self.refractions.len() >= 16 {
            return Err("refraction effect limit exceeded".into());
        }
        effect.draw_order = self.effect_draw_order()?;
        let handle = self.allocate_effect()?;
        effect.born = self.tick;
        self.refractions.insert(handle, effect);
        Ok(handle)
    }
    fn effect_draw_order(&self) -> Result<usize, String> {
        let mut occupied = [false; BILLBOARD_LIMIT];
        for order in self
            .billboards
            .values()
            .map(|p| p.draw_order)
            .chain(self.refractions.values().map(|p| p.draw_order))
        {
            occupied[order] = true;
        }
        occupied
            .iter()
            .position(|used| !used)
            .ok_or_else(|| "billboard effect limit exceeded".into())
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
    fn leaves_fall_sway_and_spin() {
        let mut motion = Flutter {
            rotation: [0.; 3],
            fall_speed: 2.,
            spin: 0.2,
            heading: 0.,
            turn_after: 10,
            initial_fall_variation: None,
        };
        let mut position = [0., 0., 100.];
        let mut seed = 1;
        for tick in 1..=30 {
            motion.step(&mut position, tick, &mut || crate::world::random(&mut seed));
        }
        assert!(position[2] < 100.);
        assert!(position[0].hypot(position[1]) > 0.);
        assert!(motion.rotation[2] > 0.);
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
    /// Vacated positions are reused in the shared update and translucent draw order.
    pub draw_order: usize,
    pub operation: Option<crate::Operation>,
    pub owner: Option<i32>,
    pub field_lighting: bool,
    pub field_fog: bool,
    pub recipe: u16,
    /// Starting age within the sprite's texture animation.
    pub texture_phase: u32,
    /// Optional atlas rectangle for effects that use a fixed crop.
    pub uv: Option<[f32; 4]>,
    /// Optional scene texture resource and image index.
    pub texture: Option<(u32, u8)>,
    pub orientation: SpriteOrientation,
    pub anchor: resonance_content::effect::VerticalAnchor,
    /// Palette index; neutral RGB channels preserve its color.
    pub palette: Option<u16>,
    pub intensity: f32,
    pub born: u32,
    pub lifetime: u32,
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub(crate) controller: Option<BillboardController>,
    pub gravity: f32,
    pub rotation: [f32; 3],
    pub angular_velocity: [f32; 3],
    pub size: [f32; 2],
    pub size_delta: f32,
    pub rgba: [u8; 4],
    pub fade: Fade,
    pub blend: Option<Blend>,
}
#[derive(Debug, Clone, Copy)]
pub enum SpriteOrientation {
    Camera,
    World,
}

#[derive(Debug, Clone)]
pub(crate) enum BillboardController {
    Orbit(crate::emitter::Orbit),
    Wander {
        direction: [f32; 3],
        speed: f32,
    },
    Drift {
        direction: [f32; 3],
        speed: f32,
    },
    Accelerate {
        multiplier: f32,
    },
    Flutter(Flutter),
    CameraOffset {
        emitter: i32,
        center: [f32; 3],
        distance: f32,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum Fade {
    Linear(f32),
    RiseFall {
        rise_ticks: u32,
        step: f32,
    },
    /// Fade to zero by expiry, starting when selected by a script.
    Proportional {
        after: u32,
        lifetime: u32,
    },
    /// Fade by eight alpha units per tick, retaining the last positive level until expiry.
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
                    age as f32
                } else {
                    (2 * rise_ticks + 1) as f32 - age as f32
                };
                (ramp * step).floor()
            }
            Self::Proportional { after, lifetime } => {
                let duration = lifetime.saturating_sub(after).max(1);
                alpha * (1. - age.saturating_sub(after) as f32 / duration as f32).clamp(0., 1.)
            }
            Self::Tail { after } => {
                let minimum = if alpha > 0. {
                    (alpha - 1.).rem_euclid(8.) + 1.
                } else {
                    0.
                };
                (alpha - 8. * age.saturating_sub(after) as f32).max(minimum)
            }
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
            draw_order: 0,
            operation: None,
            owner: None,
            field_lighting: false,
            field_fog: true,
            recipe: 0,
            texture_phase: 1,
            uv: None,
            texture: None,
            orientation: SpriteOrientation::Camera,
            anchor: resonance_content::effect::VerticalAnchor::Center,
            palette: None,
            intensity: 1.,
            born: 0,
            lifetime: 0,
            position: [0.; 3],
            velocity: [0.; 3],
            controller: None,
            gravity: 0.,
            rotation: [0.; 3],
            angular_velocity: [0.; 3],
            size: [0.; 2],
            size_delta: 0.,
            rgba: [NEUTRAL_TINT, NEUTRAL_TINT, NEUTRAL_TINT, 255],
            fade: Fade::Linear(0.),
            blend: None,
        }
    }
}

impl BillboardEffect {
    pub fn poison(position: [f32; 3], size: f32, speed: f32, born: u32) -> Self {
        Self {
            field_lighting: true,
            recipe: ORB_SPRITE,
            born,
            lifetime: 21,
            position,
            velocity: [0., 0., speed],
            size: [size; 2],
            rgba: [13, 63, 4, 255],
            ..Default::default()
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
            field_lighting: true,
            recipe: SPINNING_STAR_SPRITE,
            born,
            lifetime: 61,
            position,
            velocity: [0., 0., speed],
            rotation: [0., 0., (effect_tick & 127) as f32],
            angular_velocity: [0., 0., 1.],
            size: [size; 2],
            ..Default::default()
        }
    }

    pub fn step(&mut self) {
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

impl crate::GameWorld {
    pub(crate) fn step_wandering_billboards(&mut self) {
        let mut effects: Vec<_> = self.billboards.values_mut().collect();
        effects.sort_unstable_by_key(|effect| effect.draw_order);
        for effect in effects {
            let change: fn(&mut [f32; 3], &mut u32) = match &effect.controller {
                Some(BillboardController::Wander { .. }) => crate::emitter::scatter::wander,
                Some(BillboardController::Drift { .. }) => crate::emitter::scatter::drift,
                _ => continue,
            };
            if let Some(
                BillboardController::Wander { direction, speed }
                | BillboardController::Drift { direction, speed },
            ) = &mut effect.controller
            {
                if effect.born == self.tick {
                    change(direction, &mut self.random_state);
                    let velocity = emission::normalized(*direction).map(|v| v * *speed);
                    for i in 0..3 {
                        effect.position[i] += velocity[i];
                    }
                }
                change(direction, &mut self.random_state);
                effect.velocity = emission::normalized(*direction).map(|v| v * *speed);
            }
        }
    }
    pub(crate) fn step_billboards(&mut self, effect_tick: u32) -> Result<(), String> {
        self.billboards.retain(|_, effect| {
            effect.alive(self.tick) && effect.owner.is_none_or(|id| self.actors.contains_key(&id))
        });
        let direction = self.field_camera.as_ref().map_or([0.; 3], |camera| {
            let delta = [
                camera.position[0] - camera.target[0],
                camera.position[1] - camera.target[1],
                0.,
            ];
            let length = delta[0].hypot(delta[1]);
            delta.map(|v| if length == 0. { 0. } else { v / length })
        });
        let mut trails = Vec::new();
        for effect in self.billboards.values_mut() {
            if self.tick <= effect.born {
                continue;
            }
            if matches!(&effect.controller, Some(BillboardController::Orbit(orbit)) if orbit.trail)
            {
                let mut trail = effect.clone();
                trail.controller = None;
                trail.owner = None;
                trail.born = self.tick;
                trail.lifetime = 12;
                trail.rgba[3] /= 3;
                trail.size_delta = -trail.size[0] / trail.lifetime as f32;
                trail.fade = Fade::Proportional {
                    after: 0,
                    lifetime: trail.lifetime,
                };
                trails.push(trail);
            }
            match &mut effect.controller {
                Some(BillboardController::Orbit(orbit)) => {
                    effect.position = orbit.position(self.tick.saturating_sub(effect.born));
                }
                Some(BillboardController::Flutter(flutter)) => {
                    flutter.step(&mut effect.position, effect_tick, &mut || {
                        crate::world::random(&mut self.random_state)
                    });
                    effect.rotation = flutter.rotation;
                }
                Some(BillboardController::CameraOffset {
                    emitter,
                    center,
                    distance,
                }) => {
                    if let Some(offset) = self
                        .actors
                        .get(emitter)
                        .and_then(|a| a.emitter.as_ref())
                        .and_then(crate::emitter::Emitter::camera_offset)
                    {
                        *distance = offset;
                    }
                    effect.position = std::array::from_fn(|i| center[i] - direction[i] * *distance);
                }
                Some(BillboardController::Accelerate { multiplier }) => {
                    effect.velocity.iter_mut().for_each(|v| *v *= *multiplier);
                }
                Some(BillboardController::Wander { .. } | BillboardController::Drift { .. })
                | None => {}
            }
            effect.step();
        }
        for trail in trails {
            self.emit_billboard(trail)?;
        }
        Ok(())
    }
}
