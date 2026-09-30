//! Persistent native effect actors. Scenario scripts own their phases and lifetime.
// Numeric script properties are decoded here; emitters use named settings.
macro_rules! parameters {
    ($($field:ident = $property:literal),+ $(,)?) => { parameters!(Parameters { $($field = $property),+ }); };
    ($name:ident { $($field:ident = $property:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy)]
        struct $name { $( $field: i32, )+ }
        impl $name {
            fn read(arguments: &[i32]) -> Self {
                Self { $( $field: arguments[$property - 105], )+ }
            }
            fn property(&mut self, property: i32, value: Option<i32>) -> Result<i32, String> {
                let field = match property {
                    $( $property => &mut self.$field, )+
                    _ if (113..=122).contains(&property) => return Ok(0), // Unused script slots.
                    _ => return Err(format!("unsupported emitter property {property}")),
                };
                let previous = *field;
                if let Some(value) = value { *field = value; }
                Ok(previous)
            }
        }
    };
}

macro_rules! properties {
    ($target:expr, $property:expr, $value:expr; $( $id:pat => $field:ident ),+ $(,)?) => {
        match $property {
            $( $id => { let previous = $target.$field as i32; if let Some(value) = $value { $target.$field = value as _; } Ok(previous) }, )+
            _ if (113..=122).contains(&$property) => Ok(0),
            _ => Err(format!("unsupported emitter property {}", $property)),
        }
    };
}

mod beams;
mod bloom;
mod burst;
mod contracting;
mod converging;
mod gathering;
mod orbit;
mod quake;
mod rising;
mod scatter;
mod smoke;
mod splash;
mod trail;
mod veil;
use crate::{
    GameWorld,
    effect::{BILLBOARD_LIMIT, BillboardEffect, NEUTRAL_TINT, SpriteOrientation},
};

pub(crate) const PHASE_PROPERTY: i32 = 33;
fn phase(phase: &mut u16, value: Option<i32>) -> i32 {
    let previous = i32::from(*phase);
    if let Some(value) = value {
        *phase = value as u16;
    }
    previous
}

const MOTE_SPRITE: u16 = 10;
const SPEED_PERCENT: f32 = 100.;
const FULL_TURN: u32 = 360;
const DISC_SPRITE: u16 = 6;
const OPACITY: i32 = 8;
const TINT_RED: i32 = 42;
const RELEASE_LIFETIME: u32 = 300;
const RELEASE_ACCELERATION: f32 = 1.15;
const RELEASE_ALPHA_GAIN: i16 = 3;

#[derive(Debug, Clone)]
pub(crate) enum Emitter {
    Plume(Plume),
    LightColumn(Column),
    RisingMotes(Motes),
    Gathering(gathering::Gathering),
    Converging,
    Burst(burst::Burst),
    Contracting(contracting::Contracting),
    Scatter(scatter::Scatter),
    Smoke(smoke::Smoke),
    Veil(veil::Veil),
    Rising(rising::Rising),
    Beams(beams::Beams),
    Bloom(bloom::Bloom),
    Quake(quake::Quake),
    Orbit(orbit::Orbit),
    Splash(splash::Splash),
    Trail(trail::Trail),
}

#[derive(Debug, Clone)]
pub(crate) struct Column {
    release: Release,
    palette: i16,
    size: i16,
    layers: i16,
    alpha: i16,
    lighting_mode: i32,
    spacing: f32,
    release_timer: f32,
    release_growth: f32,
    phase: Phase,
    released: u16,
}
#[derive(Debug, Clone, Copy)]
enum Release {
    Rise,
    Expand,
}
#[derive(Debug, Clone, Copy)]
#[repr(u16)]
enum Phase {
    Holding,
    Releasing,
    Finished,
}
impl TryFrom<i32> for Phase {
    type Error = String;
    fn try_from(value: i32) -> Result<Self, String> {
        match value as u16 {
            0 => Ok(Self::Holding),
            1 => Ok(Self::Releasing),
            2 => Ok(Self::Finished),
            _ => Err("unsupported light column phase".into()),
        }
    }
}
impl Emitter {
    pub fn from_native(a: &[i32]) -> Result<Self, String> {
        let release = match a[5] {
            0 => return Ok(Self::Plume(Plume::new(PlumeKind::Flame, a[8] as i16)?)),
            1..=3 => return Ok(Self::Plume(Plume::new(PlumeKind::Mist, a[8] as i16)?)),
            9 => {
                return Ok(Self::Splash(splash::Splash::from_native(
                    a,
                    splash::Distribution::Ring,
                )?));
            }
            11 => return Ok(Self::Gathering(gathering::Gathering::from_native(a)?)),
            13 => return Ok(Self::Scatter(scatter::Scatter::from_native(a)?)),
            15 => return Ok(Self::RisingMotes(Motes::from_native(a)?)),
            16 => return Ok(Self::Burst(burst::Burst::from_native(a)?)),
            17 => return Ok(Self::Converging),
            22 => return Ok(Self::Quake(Default::default())),
            23 => Release::Rise,
            24 => return Ok(Self::Smoke(smoke::Smoke::from_native(a)?)),
            26 => {
                return Ok(Self::Beams(beams::Beams::from_native(
                    a,
                    beams::Kind::Shafts,
                )?));
            }
            27 => {
                return Ok(Self::Beams(beams::Beams::from_native(
                    a,
                    beams::Kind::Burst,
                )?));
            }
            28 => return Ok(Self::Bloom(bloom::Bloom::from_native(a)?)),
            30 => {
                return Ok(Self::Rising(rising::Rising::from_native(
                    a,
                    rising::Kind::Drifting,
                )?));
            }
            31 => return Ok(Self::Contracting(contracting::Contracting::from_native(a)?)),
            33 => return Ok(Self::Orbit(orbit::Orbit::from_native(a)?)),
            46 => return Ok(Self::Trail(trail::Trail::from_native(a)?)),
            54 => {
                return Ok(Self::Rising(rising::Rising::from_native(
                    a,
                    rising::Kind::Ascending,
                )?));
            }
            55 => return Ok(Self::Veil(veil::Veil::from_native(a)?)),
            63 => Release::Expand,
            75 => {
                return Ok(Self::Splash(splash::Splash::from_native(
                    a,
                    splash::Distribution::Spray,
                )?));
            }
            recipe => return Err(format!("unsupported effect emitter recipe {recipe}")),
        };
        let column = Column {
            release,
            palette: a[8] as i16,
            size: a[9] as i16,
            layers: a[10] as i16,
            alpha: a[11] as i16,
            lighting_mode: a[12],
            spacing: a[13] as f32,
            release_timer: a[14] as f32,
            release_growth: a[15] as f32,
            phase: Phase::Holding,
            released: 0,
        };
        column.validate()?;
        Ok(Self::LightColumn(column))
    }
    pub fn property(&mut self, property: i32, value: Option<i32>) -> Result<i32, String> {
        let column = match self {
            Self::Plume(plume) => return plume.property(property, value),
            Self::RisingMotes(motes) => return motes.property(property, value),
            Self::Gathering(gathering) => return gathering.property(property, value),
            Self::Scatter(scatter) => return scatter.property(property, value),
            Self::Smoke(smoke) => return smoke.property(property, value),
            Self::Burst(burst) => return burst.property(property, value),
            Self::Veil(veil) => return veil.property(property, value),
            Self::Rising(rising) => return rising.property(property, value),
            Self::Beams(beams) => return beams.property(property, value),
            Self::Bloom(bloom) => return bloom.property(property, value),
            Self::Contracting(contracting) => return contracting.property(property, value),
            Self::Converging => {
                return Err("converging streaks have no mutable recipe properties".into());
            }
            Self::Quake(quake) => return quake.property(property, value),
            Self::Orbit(orbit) => return orbit.property(property, value),
            Self::Splash(splash) => return splash.property(property, value),
            Self::Trail(trail) => return trail.property(property, value),
            Self::LightColumn(column) => column,
        };
        if property == PHASE_PROPERTY {
            let previous = column.phase as i32;
            if let Some(value) = value {
                column.phase = value.try_into()?;
            }
            return Ok(previous);
        }
        let previous = properties!(column, property, value;
            113 => palette, 114 => size, 115 => layers, 116 => alpha,
            117 => lighting_mode, 118 => spacing, 119 => release_timer, 120 => release_growth)?;
        column.validate()?;
        Ok(previous)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Plume {
    kind: PlumeKind,
    duration: i16,
}
#[derive(Debug, Clone, Copy)]
enum PlumeKind {
    Flame,
    Mist,
}
impl Plume {
    fn new(kind: PlumeKind, duration: i16) -> Result<Self, String> {
        if duration < 0 {
            return Err("negative plume duration".into());
        }
        Ok(Self { kind, duration })
    }
    fn interval(&self) -> u32 {
        match self.kind {
            PlumeKind::Flame => 4,
            PlumeKind::Mist => 8,
        }
    }
    fn property(&mut self, property: i32, value: Option<i32>) -> Result<i32, String> {
        const DURATION_PROPERTY: i32 = 113;
        if property != DURATION_PROPERTY {
            return Err("unsupported plume emitter property".into());
        }
        let previous = self.duration;
        if let Some(value) = value {
            *self = Self::new(self.kind, value as i16)?;
        }
        Ok(i32::from(previous))
    }
    fn particles(&self, position: [f32; 3], born: u32, random: &mut u32) -> Vec<BillboardEffect> {
        const DEFAULT_DURATION: i16 = 60;
        const RANDOM_MASK: u32 = 15;
        const RISE_SPEED: f32 = 2.;
        let duration = if self.duration == 0 {
            DEFAULT_DURATION
        } else {
            self.duration
        };
        let lifetime = duration.max(DEFAULT_DURATION) as u32;
        let count = match self.kind {
            PlumeKind::Flame => 2,
            PlumeKind::Mist => 1,
        };
        (0..count)
            .map(|index| {
                let inner = index == 1;
                let divisor = if inner { 2 } else { 1 };
                let size = (duration as u32 / divisor
                    + (crate::world::random(random) & RANDOM_MASK))
                    as f32;
                let speed = RISE_SPEED
                    + (crate::world::random(random) & RANDOM_MASK) as f32
                        / if inner { 32. } else { 16. };
                let drift = if inner {
                    0.
                } else {
                    (crate::world::random(random) & RANDOM_MASK) as f32 / 32.
                };
                BillboardEffect {
                    field_lighting: true,
                    recipe: crate::effect::GLOW_SPRITE,
                    palette: None,
                    lifetime: lifetime / divisor + 1,
                    position,
                    velocity: [drift, 0., speed],
                    angular_velocity: [0., 0., -3.],
                    size: [size; 2],
                    size_delta: match self.kind {
                        PlumeKind::Flame => {
                            if inner {
                                0.
                            } else {
                                -1.
                            }
                        }
                        PlumeKind::Mist => 1.,
                    },
                    rgba: match self.kind {
                        PlumeKind::Flame => [255, if inner { 255 } else { 10 }, 10, 255],
                        PlumeKind::Mist => [255, 255, 255, 127],
                    },
                    fade: crate::effect::Fade::tail(lifetime / divisor + 1),
                    blend_mode: Some(match self.kind {
                        PlumeKind::Flame => crate::model_particle::Blend::Additive,
                        PlumeKind::Mist => crate::model_particle::Blend::Alpha,
                    } as u8),
                    ..particle([0.; 3], born, 0, 1)
                }
            })
            .collect()
    }
}
impl Column {
    fn validate(&self) -> Result<(), String> {
        if self.layers > BILLBOARD_LIMIT as i16
            || !(0..resonance_content::effect::FIELD_PALETTE_COLORS as i16).contains(&self.palette)
        {
            return Err("invalid or unsupported light column parameters".into());
        }
        Ok(())
    }
    fn disc(
        &self,
        born: u32,
        mut position: [f32; 3],
        layer: i32,
        release: bool,
        tint: [u8; 4],
    ) -> BillboardEffect {
        position[2] += layer as f32 * self.spacing;
        let alpha = if tint[3] == u8::MAX {
            self.alpha
                .wrapping_mul(if release { RELEASE_ALPHA_GAIN } else { 1 }) as u8
        } else {
            tint[3]
        };
        BillboardEffect {
            field_lighting: self.lighting_mode & 1 != 0,
            recipe: DISC_SPRITE,
            orientation: SpriteOrientation::World,
            palette: Some(self.palette as u16),
            lifetime: if release { RELEASE_LIFETIME } else { 1 },
            position,
            velocity: [0., 0., if release { 1. } else { 0. }],
            acceleration: release.then_some(RELEASE_ACCELERATION),
            size: [self.size as f32; 2],
            rgba: [tint[0], tint[1], tint[2], alpha],
            fade: crate::effect::Fade::Linear(0.),
            ..particle([0.; 3], born, 0, 1)
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Motes {
    palette: i16,
    radius: i16,
    size: i16,
    size_spread: i16,
    lighting_mode: i32,
    speed_spread: f32,
    interval: f32,
    persistent: f32,
    speed: f32,
    phase: u16,
}
impl Motes {
    fn from_native(a: &[i32]) -> Result<Self, String> {
        let motes = Self {
            palette: a[8] as i16,
            radius: a[9] as i16,
            size: a[10] as i16,
            size_spread: a[11] as i16,
            lighting_mode: a[12],
            speed_spread: a[13] as f32,
            interval: a[16] as f32,
            persistent: a[17] as f32,
            speed: a[7] as f32,
            phase: 0,
        };
        motes.validate()?;
        Ok(motes)
    }
    fn validate(&self) -> Result<(), String> {
        if self.size_spread <= 0
            || self.speed_spread < 1.
            || self.interval < 1.
            || !(0..resonance_content::effect::FIELD_PALETTE_COLORS as i16).contains(&self.palette)
        {
            return Err("invalid rising-mote parameters".into());
        }
        Ok(())
    }
    fn property(&mut self, property: i32, value: Option<i32>) -> Result<i32, String> {
        let previous = properties!(self, property, value;
            PHASE_PROPERTY => phase, 113 => palette, 114 => radius, 115 => size, 116 => size_spread,
            117 => lighting_mode, 118 => speed_spread, 121 => interval, 122 => persistent)?;
        self.validate()?;
        Ok(previous)
    }
    fn particle(
        &self,
        owner: i32,
        mut position: [f32; 3],
        born: u32,
        random: &mut u32,
    ) -> BillboardEffect {
        let size =
            self.size as f32 + (crate::world::random(random) % self.size_spread as u32) as f32;
        let speed = (self.speed + (crate::world::random(random) % self.speed_spread as u32) as f32)
            / SPEED_PERCENT;
        let angle = (crate::world::random(random) % FULL_TURN) as f32;
        let (sin, cos) = angle.to_radians().sin_cos();
        position[0] += sin * self.radius as f32;
        position[1] -= cos * self.radius as f32;
        BillboardEffect {
            owner: (self.persistent == 0.).then_some(owner),
            field_lighting: self.lighting_mode & 1 != 0,
            recipe: MOTE_SPRITE,
            palette: Some(self.palette as u16),
            lifetime: RELEASE_LIFETIME,
            position,
            velocity: [0., 0., speed],
            size: [size; 2],
            fade: crate::effect::Fade::Linear(-1.),
            ..particle([0.; 3], born, 0, 1)
        }
    }
}

impl GameWorld {
    pub(crate) fn step_emitters(&mut self, effect_tick: u32) -> Result<(), String> {
        let mut births = Vec::new();
        let mut models = Vec::new();
        let mut ripples = Vec::new();
        let camera_direction = self.field_camera.as_ref().map_or([0., -1., 0.], |camera| {
            std::array::from_fn(|i| camera.position[i] - camera.target[i])
        });
        for &id in &self.actor_order {
            let Some(actor) = self.actors.get_mut(&id) else {
                continue;
            };
            if actor.appearance.model_hidden {
                continue;
            }
            let Some(emitter) = &actor.emitter else {
                continue;
            };
            if let Emitter::Splash(splash) = emitter {
                splash.particles(
                    actor,
                    self.tick,
                    effect_tick,
                    &mut self.random_state,
                    &mut births,
                );
                continue;
            }
            let emitter = actor.emitter.as_mut().unwrap();
            if let Emitter::Rising(rising) = emitter {
                if effect_tick.is_multiple_of(rising.interval()) {
                    let mut particle = rising.particle(
                        id,
                        actor.position,
                        actor.properties.get(&5).copied().unwrap_or(0) as f32,
                        self.tick,
                        &mut self.random_state,
                    );
                    if rising.inherits_appearance() {
                        inherit(&mut particle, &actor.properties, actor.blend);
                    }
                    births.push(particle);
                }
                continue;
            }
            if let Emitter::Veil(veil) = emitter {
                if effect_tick.is_multiple_of(veil.interval()) {
                    let mut particle =
                        veil.particle(id, actor.position, self.tick, &mut self.random_state);
                    inherit(&mut particle, &actor.properties, actor.blend);
                    births.push(particle);
                }
                continue;
            }
            if let Emitter::Scatter(scatter) = emitter {
                scatter.particles(
                    actor.position,
                    actor.properties.get(&5).copied().unwrap_or(0) as f32,
                    actor.blend,
                    self.tick,
                    effect_tick,
                    &mut self.random_state,
                    &mut births,
                );
                continue;
            }
            let column = match emitter {
                Emitter::Trail(trail) => {
                    models.extend(trail.particles(
                        &mut actor.position,
                        actor.resource,
                        actor.properties.get(&5).copied().unwrap_or(0) as f32,
                        self.tick,
                        &mut self.random_state,
                        &mut births,
                    )?);
                    continue;
                }
                Emitter::Burst(burst) => {
                    burst.particles(
                        actor.position,
                        self.tick,
                        &mut self.random_state,
                        &mut births,
                    );
                    continue;
                }
                Emitter::Smoke(smoke) => {
                    smoke.particles(
                        actor.position,
                        actor.properties.get(&5).copied().unwrap_or(0) as f32,
                        self.tick,
                        &mut self.random_state,
                        &mut births,
                    );
                    continue;
                }
                Emitter::Bloom(bloom) => {
                    bloom.particles(
                        actor.position,
                        camera_direction,
                        self.tick,
                        &mut self.random_state,
                        &mut births,
                    );
                    continue;
                }
                Emitter::Contracting(contracting) => {
                    contracting.particles(
                        &mut actor.position,
                        actor.properties.get(&5).copied().unwrap_or(0) as f32,
                        self.tick,
                        effect_tick,
                        &mut births,
                    );
                    continue;
                }
                Emitter::Beams(beams) => {
                    beams.particles(
                        actor.position,
                        self.tick,
                        effect_tick,
                        &mut self.random_state,
                        &mut births,
                    );
                    continue;
                }
                Emitter::Plume(plume) => {
                    if effect_tick.is_multiple_of(plume.interval()) {
                        births.extend(plume.particles(
                            actor.position,
                            self.tick,
                            &mut self.random_state,
                        ));
                    }
                    continue;
                }
                Emitter::RisingMotes(motes) => {
                    if effect_tick.is_multiple_of(motes.interval as u32) {
                        births.push(motes.particle(
                            id,
                            actor.position,
                            self.tick,
                            &mut self.random_state,
                        ));
                    }
                    continue;
                }
                Emitter::Gathering(gathering) => {
                    gathering.particles(
                        actor.position,
                        self.tick,
                        effect_tick,
                        &mut self.random_state,
                        &mut births,
                    );
                    continue;
                }
                Emitter::Converging => {
                    births.push(converging::particle(
                        actor.position,
                        self.tick,
                        &mut self.random_state,
                    ));
                    continue;
                }
                Emitter::Quake(quake) => {
                    let (ripple, shake) = quake.step(actor.position, self.tick, &mut births);
                    ripples.extend(ripple);
                    if let Some(amount) = shake {
                        self.field_camera
                            .get_or_insert_default()
                            .shake
                            .configure(amount, 0, 0);
                    }
                    continue;
                }
                Emitter::Orbit(orbit) => {
                    orbit.particles(
                        actor.position,
                        self.tick,
                        &mut self.random_state,
                        &mut births,
                    );
                    continue;
                }
                Emitter::Splash(_) => unreachable!(),
                Emitter::Scatter(_) => unreachable!(),
                Emitter::Veil(_) => unreachable!(),
                Emitter::Rising(_) => unreachable!(),
                Emitter::LightColumn(column) => column,
            };
            let layers = i32::from(column.layers)
                - match column.phase {
                    Phase::Holding => 0,
                    Phase::Releasing => i32::from(column.released),
                    Phase::Finished => continue,
                };
            let count = layers.max(0) as usize
                + usize::from(
                    matches!(column.phase, Phase::Releasing)
                        && matches!(column.release, Release::Rise),
                );
            if self.billboards.len() + births.len() + count > BILLBOARD_LIMIT {
                return Err("billboard effect limit exceeded".into());
            }
            let mut tint = [NEUTRAL_TINT; 4];
            for (channel, value) in tint[..3].iter_mut().enumerate() {
                *value = actor
                    .properties
                    .get(&(TINT_RED + channel as i32))
                    .copied()
                    .unwrap_or(NEUTRAL_TINT as i32) as u8;
            }
            tint[3] = actor
                .properties
                .get(&OPACITY)
                .copied()
                .unwrap_or(u8::MAX as i32) as u8;
            if matches!(column.release, Release::Expand) && matches!(column.phase, Phase::Releasing)
            {
                for layer in 0..layers {
                    let mut disc = column.disc(self.tick, actor.position, layer, false, tint);
                    disc.lifetime = (column.release_timer as u16 as u32) + 1;
                    disc.size_delta = column.release_growth;
                    disc.fade = crate::effect::Fade::tail(disc.lifetime);
                    births.push(disc);
                }
                column.phase = Phase::Finished;
                continue;
            }
            for layer in 0..layers {
                births.push(column.disc(self.tick, actor.position, layer, false, tint));
            }
            if matches!(column.phase, Phase::Releasing) {
                births.push(column.disc(self.tick, actor.position, layers, true, tint));
                if layers <= 0 {
                    column.phase = Phase::Finished;
                }
                column.released = column.released.saturating_add(1);
            }
        }
        let direction = self.field_camera.as_ref().map_or([0.; 3], |camera| {
            let delta = [
                camera.position[0] - camera.target[0],
                camera.position[1] - camera.target[1],
                0.,
            ];
            let length = delta[0].hypot(delta[1]);
            delta.map(|v| if length == 0. { 0. } else { v / length })
        });
        for mut effect in births {
            if let Some(crate::effect::BillboardController::CameraOffset {
                center, distance, ..
            }) = effect.controller
            {
                effect.position = std::array::from_fn(|i| center[i] + direction[i] * distance);
            }
            self.emit_billboard(effect)?;
        }
        for ripple in ripples {
            self.emit_refraction(ripple)?;
        }
        for model in models {
            self.emit_model_particle(model)?;
        }
        Ok(())
    }
}

fn inherit(
    particle: &mut BillboardEffect,
    properties: &std::collections::BTreeMap<i32, i32>,
    blend: Option<crate::model_particle::Blend>,
) {
    for (channel, color) in particle.rgba[..3].iter_mut().enumerate() {
        if let Some(value) = properties.get(&(42 + channel as i32))
            && *value as u8 != NEUTRAL_TINT
        {
            *color = *value as u8;
        }
    }
    if let Some(value) = properties.get(&8)
        && *value as u8 != 255
    {
        particle.rgba[3] = *value as u8;
    }
    if let Some(blend) = blend {
        particle.blend_mode = Some(blend as u8);
    }
}

fn particle(position: [f32; 3], born: u32, palette: u16, lifetime: u32) -> BillboardEffect {
    BillboardEffect {
        recipe: 0,
        palette: Some(palette),
        born,
        lifetime,
        position,
        size: [0.; 2],
        rgba: [NEUTRAL_TINT, NEUTRAL_TINT, NEUTRAL_TINT, 255],
        fade: crate::effect::Fade::tail(lifetime),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thoda_mist_rises_and_expands_after_its_emitter_is_removed() {
        let mut world = GameWorld::default();
        let mut actor = crate::Actor::new(0, [160., 5160., -1230.]);
        actor.emitter = Some(
            Emitter::from_native(&[
                400, 160, 5160, -1230, 0, 1, 0, 80, 160, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            ])
            .unwrap(),
        );
        world.insert_actor(400, actor);
        world.step_emitters(4).unwrap();
        assert!(world.billboards.is_empty());
        world.tick = 8;
        world.step_emitters(8).unwrap();
        assert_eq!(world.billboards.len(), 1);
        world.actors.remove(&400);
        world.step_emitters(16).unwrap();
        let mist = world.billboards.values_mut().next().unwrap();
        let size = mist.size[0];
        for _ in 0..16 {
            mist.step();
        }
        assert!(mist.position[2] >= -1198.);
        assert_eq!(mist.size, [size + 16.; 2]);
        assert_eq!(mist.rgba, [255, 255, 255, 127]);
        assert_eq!(
            mist.blend_mode,
            Some(crate::model_particle::Blend::Alpha as u8)
        );
        assert_eq!(mist.alpha(137), 127.);
        assert!(mist.alpha(145) < 127.);
        assert!(mist.alive(168));
        assert!(!mist.alive(169));
    }
}
