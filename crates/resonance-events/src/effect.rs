//! Billboard effects expressed as ordinary position, size, rotation and lifetime.
pub(crate) mod emission;
pub(crate) mod property;
pub(crate) mod ring;
pub(crate) mod station;
pub(crate) const BILLBOARD_LIMIT: usize = 2048;
pub const NEUTRAL_TINT: u8 = 64;
pub const NEUTRAL_PALETTE: u8 = 0;
pub(crate) use resonance_content::effect::sprite::*;

#[derive(Clone, Debug)]
pub struct Palette(pub Vec<[u8; 4]>);
impl Default for Palette {
    fn default() -> Self {
        Self(vec![
            [NEUTRAL_TINT, NEUTRAL_TINT, NEUTRAL_TINT, 255];
            resonance_content::effect::FIELD_PALETTE_COLORS
        ])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Blend {
    Alpha,
    Additive,
    Subtractive,
    Previous,
}
impl TryFrom<i32> for Blend {
    type Error = String;
    fn try_from(value: i32) -> Result<Self, String> {
        match value & 3 {
            0 => Ok(Self::Alpha),
            1 => Ok(Self::Additive),
            2 => Ok(Self::Subtractive),
            _ => Err("inherited blend requires a preceding draw".into()),
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
    /// Cleanup group; preserved tails remain in this group after the actor leaves.
    pub owner: Option<i32>,
    pub image: RefractionImage,
    pub palette: u8,
    pub orientation: SpriteOrientation,
    pub rotation: [f32; 3],
    pub rotation_order: RotationOrder,
    pub angular_velocity: [f32; 3],
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub speed: f32,
    pub normalize_velocity: bool,
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
    pub(crate) fn step(&mut self, tick: u32) -> bool {
        if tick > self.born {
            let delta = motion_delta(self.velocity, self.speed, self.normalize_velocity);
            for (position, velocity) in self.position.iter_mut().zip(delta) {
                *position += velocity;
            }
            self.size += self.growth;
            for (rotation, velocity) in self.rotation.iter_mut().zip(self.angular_velocity) {
                *rotation += velocity;
            }
        }
        self.alive(tick)
    }

    fn alive(&self, tick: u32) -> bool {
        tick.saturating_sub(self.born) < self.lifetime && self.size >= 0. && self.alpha(tick) >= 0.
    }

    pub fn alpha(&self, tick: u32) -> f32 {
        if tick < self.born {
            0.
        } else {
            self.fade.alpha(self.alpha, tick - self.born)
        }
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
        let count = resources
            .model(actor.model_resource())
            .map_or(0, |m| m.names.len());
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
        let mut transfers = std::mem::take(&mut self.station_transfers);
        for transfer in &mut transfers {
            transfer.update(self)?;
        }
        let stations: Vec<_> = self
            .actors
            .values_mut()
            .filter(|a| a.ring_station)
            .map(|actor| {
                actor.visible = true;
                actor.opacity = 64;
                let angle = (self.effect_tick % 360) as f32;
                actor.face(angle);
                let mut position = actor.position;
                position[2] += angle.to_radians().sin() * 10. + 150.;
                let rgb = actor.tint;
                (position, rgb)
            })
            .collect();
        let turn = self.effect_tick as f32 * 4.;
        for (position, rgb) in stations {
            let tint = |alpha| [rgb[0], rgb[1], rgb[2], alpha];
            let neutral = |alpha| [NEUTRAL_TINT, NEUTRAL_TINT, NEUTRAL_TINT, alpha];
            for (recipe, lifetime, base, mask, rgba, fade, rotation, blend) in [
                (STATION_GLOW_SPRITE, 2, 48, 7, tint(64), -16., 0., None),
                (
                    STATION_GLOW_SPRITE,
                    3,
                    40,
                    3,
                    tint(128),
                    -64.,
                    0.,
                    Some(Blend::Alpha),
                ),
                (
                    STATION_HALO_SPRITE,
                    5,
                    80,
                    3,
                    neutral(255),
                    -48.,
                    turn,
                    None,
                ),
                (
                    STATION_HALO_SPRITE,
                    5,
                    80,
                    3,
                    neutral(255),
                    -48.,
                    -turn * 2.,
                    None,
                ),
            ] {
                let size = (base + (self.random() & mask)) as f32;
                self.emit_billboard(BillboardEffect {
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
        for transfer in &transfers {
            transfer.draw(self)?;
        }
        transfers.retain(|transfer| transfer.operation.is_pending());
        self.station_transfers = transfers;
        Ok(())
    }
    pub fn emit_billboard(&mut self, mut effect: BillboardEffect) -> Result<i32, String> {
        if let Some(index) = effect.palette.take() {
            let palette = self
                .effect_palette
                .0
                .get(usize::from(index))
                .ok_or("effect palette is outside the field palette")?;
            for (color, &inherited) in effect.rgba[..3].iter_mut().zip(palette) {
                if *color == NEUTRAL_TINT {
                    *color = inherited;
                }
            }
        }
        effect.draw_order = self.effect_draw_order(|_| effect.born)?;
        let handle = self.allocate_effect()?;
        self.billboards.insert(handle, effect);
        Ok(handle)
    }
    pub fn emit_refraction(&mut self, mut effect: RefractionPulse) -> Result<i32, String> {
        effect.draw_order = self.effect_draw_order(|_| effect.born)?;
        let handle = self.allocate_effect()?;
        self.refractions.insert(handle, effect);
        Ok(handle)
    }
    fn effect_draw_order(&self, tick: impl Fn(usize) -> u32) -> Result<usize, String> {
        let mut occupied = [false; BILLBOARD_LIMIT];
        // Deferred births may reuse particles that finish drawing this update.
        for order in self
            .billboards
            .values()
            .filter(|p| p.alive(tick(p.draw_order)))
            .map(|p| p.draw_order)
            .chain(
                self.refractions
                    .values()
                    .filter(|p| p.alive(tick(p.draw_order)))
                    .map(|p| p.draw_order),
            )
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
#[derive(Debug, Clone)]
pub(crate) struct Flutter {
    fall_speed: f32,
    initial_variation: Option<f32>,
    spin: f32,
    heading: f32,
    turn_after: i32,
}

impl Flutter {
    pub(crate) fn pending(recipe: &resonance_content::effect::FlutterRecipe, rising: bool) -> Self {
        let direction = if rising { -1. } else { 1. };
        Self {
            fall_speed: recipe.fall_speed * direction,
            initial_variation: Some(recipe.fall_variation * direction),
            spin: recipe.spin,
            heading: 0.,
            turn_after: 0,
        }
    }
    fn initialize(&mut self, rotation: &mut [f32; 3], random: &mut impl FnMut() -> u32) {
        let Some(variation) = self.initial_variation.take() else {
            return;
        };
        let mut turn_after = (random() & 31) as i32 + 5;
        if random() & 1 != 0 {
            turn_after = -turn_after;
        }
        self.turn_after = turn_after;
        self.fall_speed -= (random() & 31) as f32 * variation;
        *rotation = std::array::from_fn(|_| random() as f32);
    }
    pub(crate) fn step(
        &mut self,
        position: &mut [f32; 3],
        rotation: &mut [f32; 3],
        tick: u32,
        random: &mut impl FnMut() -> u32,
    ) {
        self.initialize(rotation, random);
        self.turn_after -= 1;
        if self.turn_after < 0 {
            self.turn_after = (random() & 63) as i32 + 5;
            self.heading += (random() as i32 % 90 - 45) as f32;
            rotation[1] -= (random() & 3) as f32;
            rotation[0] += (random() & 3) as f32;
        }
        rotation[2] += self.spin;
        // Integrate wind over the interval beginning at the preceding update.
        let sway = (tick.wrapping_sub(1) as f32).to_radians().sin();
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
    fn reusing_a_particle_slot_preserves_random_motion_order() {
        let flutter = BillboardEffect {
            lifetime: 30,
            controller: Some(BillboardController::Flutter(Flutter {
                fall_speed: 2.,
                initial_variation: None,
                spin: 0.2,
                heading: 0.,
                turn_after: 0,
            })),
            ..Default::default()
        };
        let mut ordered = crate::GameWorld::default();
        ordered.emit_billboard(flutter.clone()).unwrap();
        ordered.emit_billboard(flutter.clone()).unwrap();
        let mut reused = crate::GameWorld::default();
        let hole = reused
            .emit_billboard(BillboardEffect {
                lifetime: 1,
                ..Default::default()
            })
            .unwrap();
        reused.emit_billboard(flutter.clone()).unwrap();
        reused.billboards.remove(&hole);
        reused.emit_billboard(flutter).unwrap();
        for tick in 1..=20 {
            for world in [&mut ordered, &mut reused] {
                world.tick = tick;
                world.step_billboards(tick);
            }
            for expected in ordered.billboards.values() {
                let actual = reused
                    .billboards
                    .values()
                    .find(|p| p.draw_order == expected.draw_order)
                    .unwrap();
                assert_eq!(actual.position, expected.position);
                assert_eq!(actual.rotation, expected.rotation);
            }
        }
    }

    #[test]
    fn leaves_fall_sway_and_spin() {
        let mut motion = Flutter {
            fall_speed: 2.,
            initial_variation: None,
            spin: 0.2,
            heading: 0.,
            turn_after: 10,
        };
        let mut position = [0., 0., 100.];
        let mut rotation = [0.; 3];
        let mut seed = 1;
        for tick in 1..=30 {
            motion.step(&mut position, &mut rotation, tick, &mut || {
                crate::world::random(&mut seed)
            });
        }
        assert!(position[2] < 100.);
        assert!(position[0].hypot(position[1]) > 0.);
        assert!(rotation[2] > 0.);
        rotation = [90.; 3];
        motion.step(&mut position, &mut rotation, 31, &mut || {
            crate::world::random(&mut seed)
        });
        assert!(
            rotation[2] > 90.,
            "flutter continues from the changed rotation"
        );
    }
}

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
    /// Cleanup group; preserved tails remain in this group after the actor leaves.
    pub owner: Option<i32>,
    pub owner_tail: OwnerTail,
    pub field_fog: bool,
    pub recipe: u16,
    /// Starting age within the sprite's texture animation.
    pub texture_phase: u32,
    /// Atlas rectangle for a fixed crop.
    pub uv: Option<[f32; 4]>,
    /// Scene texture resource and image index.
    pub texture: Option<(u32, u8)>,
    pub orientation: SpriteOrientation,
    pub anchor: resonance_content::effect::VerticalAnchor,
    /// Birth recipe: resolves neutral RGB channels before the particle is stored.
    pub palette: Option<u16>,
    pub born: u32,
    pub lifetime: u32,
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub speed: f32,
    pub normalize_velocity: bool,
    pub(crate) controller: Option<BillboardController>,
    pub gravity: f32,
    pub rotation: [f32; 3],
    pub rotation_order: RotationOrder,
    pub angular_velocity: [f32; 3],
    pub size: [f32; 2],
    pub size_delta: f32,
    pub rgba: [u8; 4],
    /// A direct opacity write affects one presentation while the fade continues.
    pub(crate) alpha_override: Option<(u32, u8)>,
    pub fade: Fade,
    pub blend: Option<Blend>,
}

#[derive(Debug, Clone, Copy, Default)]
pub enum OwnerTail {
    #[default]
    Keep,
    Fade(u32),
}
#[derive(Debug, Clone, Copy)]
pub enum SpriteOrientation {
    Camera,
    World,
}

#[derive(Debug, Clone, Copy, Default)]
pub enum RotationOrder {
    #[default]
    Zyx,
    Zxy,
    Xyz,
    Xzy,
    Yxz,
    Yzx,
}

#[derive(Debug, Clone)]
pub(crate) enum BillboardController {
    Orbit(crate::emitter::Orbit),
    Scatter {
        direction: [f32; 3],
        speed: f32,
        planar: bool,
        wandering: bool,
    },
    Drift {
        direction: [f32; 3],
        speed: f32,
        spatial: bool,
    },
    Accelerate {
        multiplier: f32,
        delta: [f32; 3],
    },
    Flutter(Flutter),
    TextureStrip {
        columns: u32,
        ticks: u32,
    },
    CameraOffset {
        emitter: Option<(i32, u64)>,
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
    /// Reduce the current byte opacity according to the remaining lifetime.
    Proportional,
    /// Fade by eight alpha units per tick, retaining the last positive level until expiry.
    Tail {
        after: u32,
    },
}
impl Fade {
    pub(crate) const TAIL_UPDATES: u32 = 32;

    pub const fn tail(lifetime: u32) -> Self {
        Self::Tail {
            after: lifetime.saturating_sub(Self::TAIL_UPDATES),
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
            Self::Proportional => alpha,
            Self::Tail { after } => Self::tail_alpha(alpha, age.saturating_sub(after)),
        }
    }

    pub(crate) fn tail_alpha(alpha: f32, updates: u32) -> f32 {
        const STEP: f32 = 8.;
        let minimum = if alpha > 0. {
            (alpha - 1.).rem_euclid(STEP) + 1.
        } else {
            0.
        };
        (alpha - STEP * updates as f32).max(minimum)
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
            owner_tail: OwnerTail::Keep,
            field_fog: true,
            recipe: 0,
            texture_phase: 1,
            uv: None,
            texture: None,
            orientation: SpriteOrientation::Camera,
            anchor: resonance_content::effect::VerticalAnchor::Center,
            palette: None,
            born: 0,
            lifetime: 0,
            position: [0.; 3],
            velocity: [0.; 3],
            speed: 0.,
            normalize_velocity: false,
            controller: None,
            gravity: 0.,
            rotation: [0.; 3],
            rotation_order: RotationOrder::default(),
            angular_velocity: [0.; 3],
            size: [0.; 2],
            size_delta: 0.,
            rgba: [NEUTRAL_TINT, NEUTRAL_TINT, NEUTRAL_TINT, 255],
            alpha_override: None,
            fade: Fade::Linear(0.),
            blend: None,
        }
    }
}

impl BillboardEffect {
    pub fn poison(position: [f32; 3], size: f32, speed: f32, born: u32) -> Self {
        Self {
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
        let delta = motion_delta(self.velocity, self.speed, self.normalize_velocity);
        for (i, delta) in delta.into_iter().enumerate() {
            self.position[i] += delta;
            self.rotation[i] += self.angular_velocity[i];
        }
        self.velocity[2] += self.gravity;
        let aspect = if matches!(self.controller, Some(BillboardController::Flutter(_)))
            && self.size[0] > 0.
        {
            self.size[1] / self.size[0]
        } else {
            1.
        };
        self.size[0] += self.size_delta;
        self.size[1] += self.size_delta * aspect;
    }
    fn alive(&self, tick: u32) -> bool {
        tick.saturating_sub(self.born) < self.lifetime
            && self.size.iter().all(|s| *s >= 0.)
            && self.alpha(tick) >= 0.
    }
    pub fn alpha(&self, tick: u32) -> f32 {
        if let Some((at, alpha)) = self.alpha_override
            && tick == at
        {
            return f32::from(alpha);
        }
        self.fade
            .alpha(f32::from(self.rgba[3]), tick.saturating_sub(self.born))
    }
}

fn motion_delta(velocity: [f32; 3], speed: f32, normalized: bool) -> [f32; 3] {
    if normalized {
        emission::normalized(velocity).map(|component| component * speed)
    } else {
        velocity
    }
}

impl crate::GameWorld {
    pub(crate) fn initialize_billboards(&mut self) {
        let mut effects: Vec<_> = self
            .billboards
            .values_mut()
            .filter(|effect| {
                effect.born <= self.tick
                    && matches!(&effect.controller, Some(BillboardController::Flutter(f)) if f.initial_variation.is_some())
            })
            .collect();
        effects.sort_unstable_by_key(|effect| effect.draw_order);
        for effect in effects {
            if let Some(BillboardController::Flutter(flutter)) = &mut effect.controller {
                flutter.initialize(&mut effect.rotation, &mut || {
                    crate::world::random(&mut self.random_state)
                });
            }
        }
    }

    pub(crate) fn step_wandering_billboards(&mut self) {
        let mut effects: Vec<_> = self.billboards.values_mut().collect();
        effects.sort_unstable_by_key(|effect| effect.draw_order);
        for effect in effects {
            type Change = fn(&mut [f32; 3], &mut u32);
            let (direction, speed, change, rise) = match &mut effect.controller {
                Some(BillboardController::Scatter {
                    direction,
                    speed,
                    planar,
                    wandering,
                }) => (
                    direction,
                    *speed,
                    wandering.then_some(crate::emitter::scatter::wander as Change),
                    f32::from(*planar) * crate::emitter::scatter::RISE_PER_TICK,
                ),
                Some(BillboardController::Drift {
                    direction,
                    speed,
                    spatial,
                }) => (
                    direction,
                    *speed,
                    Some(if *spatial {
                        crate::emitter::scatter::diffuse as Change
                    } else {
                        crate::emitter::scatter::drift as Change
                    }),
                    0.,
                ),
                _ => continue,
            };
            let born = effect.born == self.tick;
            if born && let Some(change) = change {
                change(direction, &mut self.random_state);
                let velocity = emission::normalized(*direction).map(|v| v * speed);
                for i in 0..3 {
                    effect.position[i] += velocity[i];
                }
            }
            if !born || change.is_some() {
                direction[2] += rise;
                if let Some(change) = change {
                    change(direction, &mut self.random_state);
                }
            }
            effect.velocity = emission::normalized(*direction).map(|v| v * speed);
        }
    }
    pub(crate) fn step_billboards(&mut self, effect_tick: u32) {
        let direction = self.field_camera.as_ref().map_or([0.; 3], |camera| {
            let delta = [
                camera.position[0] - camera.target[0],
                camera.position[1] - camera.target[1],
                0.,
            ];
            let length = delta[0].hypot(delta[1]);
            delta.map(|v| if length == 0. { 0. } else { v / length })
        });
        self.billboards.retain(|_, effect| effect.alive(self.tick));
        let mut effects: Vec<_> = self.billboards.values_mut().collect();
        effects.sort_unstable_by_key(|effect| effect.draw_order);
        for effect in effects {
            if self.tick <= effect.born {
                continue;
            }
            if matches!(effect.fade, Fade::Proportional) {
                let remaining = effect.lifetime.saturating_sub(self.tick - effect.born);
                if remaining > 1 {
                    effect.rgba[3] -= (u32::from(effect.rgba[3]) / remaining) as u8;
                }
            }
            match &mut effect.controller {
                Some(BillboardController::Orbit(orbit)) => {
                    effect.position = orbit.position(self.tick.saturating_sub(effect.born));
                }
                Some(BillboardController::Flutter(flutter)) => {
                    flutter.step(
                        &mut effect.position,
                        &mut effect.rotation,
                        effect_tick,
                        &mut || crate::world::random(&mut self.random_state),
                    );
                }
                Some(BillboardController::TextureStrip { columns, ticks }) => {
                    let left = (self.tick - effect.born) / *ticks % *columns;
                    let width = 1. / *columns as f32;
                    effect.uv = Some([
                        left as f32 * width,
                        0.,
                        (left + 1) as f32 * width - 1. / 256.,
                        254. / 256.,
                    ]);
                }
                Some(BillboardController::CameraOffset {
                    emitter,
                    center,
                    distance,
                }) => {
                    effect.position = std::array::from_fn(|i| center[i] - direction[i] * *distance);
                    if let Some(offset) = emitter
                        .and_then(|(id, instance)| {
                            self.actors.get(&id).filter(|a| a.instance == instance)
                        })
                        .and_then(|a| a.emitter.as_ref())
                        .and_then(crate::emitter::Emitter::camera_offset)
                    {
                        *distance = offset;
                    }
                }
                Some(BillboardController::Accelerate { multiplier, delta }) => {
                    for (velocity, delta) in effect.velocity.iter_mut().zip(delta) {
                        *velocity = *velocity * *multiplier + *delta;
                    }
                }
                Some(BillboardController::Scatter { .. } | BillboardController::Drift { .. })
                | None => {}
            }
            effect.step();
        }
        self.billboards
            .retain(|_, effect| effect.size.iter().all(|size| *size >= 0.));
    }

    pub(crate) fn emit_orbit_trails(&mut self) -> Result<(), String> {
        let mut trails = Vec::new();
        let mut effects: Vec<_> = self.billboards.values().collect();
        effects.sort_unstable_by_key(|effect| effect.draw_order);
        for effect in effects {
            let Some(BillboardController::Orbit(orbit)) = &effect.controller else {
                continue;
            };
            let Some(palette) = orbit.trail_palette else {
                continue;
            };
            for batch in 0..if self.tick == effect.born { 2 } else { 1 } {
                let mut trail = effect.clone();
                trail.controller = None;
                trail.owner = None;
                if self.tick == effect.born && batch == 0 {
                    trail.position = orbit.trail_origin();
                }
                trail.recipe = [ORB_SPRITE, SEAL_SPARK_SPRITE, TRAIL_GLOW_SPRITE]
                    [crate::world::random(&mut self.random_state) as usize % 3];
                trail.palette = Some(crate::emitter::palette(palette, &mut self.random_state));
                trail.rgba[..3].fill(NEUTRAL_TINT);
                let spin = if crate::world::random(&mut self.random_state) % 2 == 0 {
                    -3.
                } else {
                    3.
                };
                trail.born = self.tick;
                trail.lifetime = 61;
                trail.rgba[3] = 255;
                trail.rotation = [0.; 3];
                trail.angular_velocity[2] = spin;
                trail.fade = Fade::Linear(-10.);
                trails.push((trail, effect.draw_order));
            }
        }
        for (mut trail, head) in trails {
            // Reuse finished slots as drawing advances. A slot already drawn
            // this frame presents its new afterimage on the next frame.
            let order = self.effect_draw_order(|order| self.tick + u32::from(order < head))?;
            trail.born += u32::from(order <= head);
            let handle = self.emit_billboard(trail)?;
            self.billboards.get_mut(&handle).unwrap().draw_order = order;
        }
        Ok(())
    }
}
