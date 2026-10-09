//! Translate scenario constructors and properties into the live effect settings.
use super::{Emitter, Kind, PHASE_PROPERTY, ProjectileColor, stream};
use crate::effect::{BILLBOARD_LIMIT, BillboardEffect, Fade, SpriteOrientation};

macro_rules! setting {
    ($field:expr, $value:expr) => {
        setting!($field, $value, Ok::<i32, String>)
    };
    ($field:expr, $value:expr, $parse:expr) => {{
        let field = &mut $field;
        let previous = *field as i32;
        if let Some(value) = $value {
            *field = $parse(value)? as _;
        }
        Ok::<i32, String>(previous)
    }};
}

impl Emitter {
    pub fn from_native(a: &[i32], impact_texture: Option<(u32, u8)>) -> Result<Self, String> {
        if a.len() != 18 {
            return Err("invalid emitter arguments".into());
        }
        let kind = match a[5] {
            11 => Kind::Gathering {
                delay: 0,
                state: Default::default(),
            },
            49 => Kind::Seal(Default::default()),
            31 => Kind::Contract(Default::default()),
            29 => Kind::Burst(Default::default()),
            18 => Kind::Mote(Default::default()),
            23 | 63 => Kind::Column(super::column::Column {
                expands: a[5] == 63,
                ..Default::default()
            }),
            0..=3 => Kind::Fire {
                size: 0.,
                smoke: a[5] != 0,
            },
            9 | 17 | 20 | 21 | 24 | 25 | 36 | 39 | 41 | 44 | 54 | 57 | 62 | 64 | 75 => {
                Kind::Stream(stream::Stream::new(a[5]))
            }
            12 => Kind::Glow {
                angle: 0.,
                palette: 0,
                size: 0,
                retire_with_emitter: false,
            },
            13 => Kind::Scatter(Default::default()),
            59 => Kind::Cloud(Default::default()),
            14 => Kind::Portal {
                palette: 0,
                size: 0.,
            },
            15 | 30 => Kind::RisingOrbs(super::rays::RisingOrbs {
                drifting: a[5] == 30,
                ..Default::default()
            }),
            16 => Kind::Crown {
                palette: 0,
                radius: 0.,
                spread: 0.,
            },
            46 | 47 => Kind::Travel {
                flight: None,
                texture: impact_texture,
                palette: 33,
                size: 0,
                burst_size: 0,
                fade: -10,
                target: [0.; 3],
                afterimages: a[5] == 46,
            },
            19 => Kind::Charge {
                velocity: None,
                palette: 0,
                radius: 0,
                updates: 1,
                target: [0.; 3],
                travelling: false,
            },
            22 => Kind::Quake,
            26 => Kind::Shafts(Default::default()),
            27 => Kind::Convergence(Default::default()),
            28 => Kind::Aura {
                palette: 0,
                offset: 0.,
            },
            33 => Kind::TwinTrail {
                sprite: BillboardEffect {
                    fade: Fade::Linear(0.),
                    ..super::particle([0.; 3], 0, 0, 61)
                },
                radius: 0.,
            },
            32 => Kind::Ray {
                palette: 0,
                size: 0.,
                target: [0.; 3],
                velocity: None,
            },
            34 => Kind::Bloom(Default::default()),
            42 => Kind::TwinGlow {
                size: 0,
                satellite_size: 0,
                angles: [0.; 2],
            },
            43 => Kind::Explosion(Default::default()),
            45 => Kind::RadialSpray { palette: 0 },
            38 => Kind::Inward {
                palette: 0,
                radius: 0,
                count: 0,
                curve: 0.,
                size: 0,
                clear: 0,
                blend: 0,
            },
            48 => Kind::Rising(Default::default()),
            51 => Kind::Fireball {
                velocity: None,
                size: 0.,
                updates: 1,
                target: [0.; 3],
            },
            55 => Kind::AttachedTrail {
                sprite: super::particle([0.; 3], 0, 0, 1),
                offset: 0.,
                interval: 1,
            },
            50 => Kind::ModelTrail {
                remaining: 1,
                target: [0.; 3],
                velocity: None,
            },
            52 | 53 => Kind::Flash {
                sparks: a[5] == 52,
                lifetime: 11,
            },
            56 => Kind::LinearTrail {
                sprite: BillboardEffect {
                    size_delta: -0.75,
                    fade: Fade::Linear(0.),
                    ..super::particle([0.; 3], 0, 0, 61)
                },
                remaining: 1,
                target: [0.; 3],
                velocity: None,
            },
            60 => Kind::Cardinal {
                angle: 0.,
                count: 0,
            },
            10 | 37 | 61 | 66 => Kind::Projectile {
                completed_phase: match a[5] {
                    37 => None,
                    66 => Some(3),
                    _ => Some(2),
                },
                color: match a[5] {
                    10 => ProjectileColor::Violet,
                    37 => ProjectileColor::Selected(0),
                    _ => ProjectileColor::Changing { fade: 0. },
                },
                path: None,
                launch: [0.; 3],
                curvature: 0,
                size: 0.,
                target: [0.; 3],
                texture: if a[5] == 66 { impact_texture } else { None },
            },
            kind => return Err(format!("unsupported emitter {kind}")),
        };
        let mut emitter = Self {
            kind,
            phase: 0,
            age: 0,
        };
        for (slot, &value) in a[8..].iter().enumerate() {
            emitter.property(113 + slot as i32, Some(value))?;
        }
        Ok(emitter)
    }

    pub fn property(&mut self, property: i32, value: Option<i32>) -> Result<i32, String> {
        if property == PHASE_PROPERTY {
            let previous = match &mut self.kind {
                Kind::Mote(mote) => {
                    let previous = i32::from(mote.script_phase);
                    if let Some(value) = value {
                        mote.script_phase = bounded(value, 0, 4)? as u8;
                    }
                    Ok(previous)
                }
                Kind::Gathering {
                    delay,
                    state: gathering,
                } => {
                    use super::gathering::Phase;
                    let previous = match gathering.phase {
                        Phase::Gathering => 0,
                        Phase::Charging(_) => 1,
                        Phase::Release => 2,
                        Phase::Done => 3,
                    };
                    if let Some(value) = value {
                        gathering.phase = match value {
                            0 => Phase::Gathering,
                            1 => Phase::Charging(*delay),
                            2 => Phase::Release,
                            3 => Phase::Done,
                            _ => return Err("invalid gathering phase".into()),
                        };
                    }
                    Ok(previous)
                }
                Kind::Seal(seal) => {
                    use super::seal::Phase;
                    let previous = seal.phase as i32;
                    if let Some(value) = value {
                        seal.phase = match value {
                            0 => Phase::Idle,
                            1 => Phase::Release,
                            2 => Phase::Pulsing,
                            3 => Phase::Collapse,
                            4 => Phase::Done,
                            _ => return Err("invalid seal phase".into()),
                        };
                    }
                    Ok(previous)
                }
                Kind::Column(column) => {
                    use super::column::Phase;
                    let previous = match column.phase {
                        Phase::Stacked => 0,
                        Phase::Releasing(_) => 1,
                        Phase::Done => 2,
                    };
                    if let Some(value) = value {
                        column.phase = match value {
                            0 => Phase::Stacked,
                            1 => Phase::Releasing(0),
                            2 => Phase::Done,
                            _ => return Err("invalid column phase".into()),
                        };
                    }
                    Ok(previous)
                }
                Kind::Contract(contract) => {
                    use super::contract::Phase;
                    let previous = contract.phase as i32;
                    if let Some(value) = value {
                        contract.set_phase(match value {
                            0 => Phase::Contracting,
                            1 => Phase::Waiting,
                            2 => Phase::Release,
                            3 => Phase::Done,
                            _ => return Err("invalid contraction phase".into()),
                        });
                    }
                    Ok(previous)
                }
                _ => setting!(self.phase, value, |v| bounded(v, 0, 4)),
            }?;
            if let Some(value) = value {
                self.age = 0;
                if value == 0 {
                    match &mut self.kind {
                        Kind::Travel { flight, .. } => *flight = None,
                        Kind::Projectile { path, .. } => *path = None,
                        Kind::Charge { velocity, .. }
                        | Kind::Fireball { velocity, .. }
                        | Kind::LinearTrail { velocity, .. }
                        | Kind::ModelTrail { velocity, .. } => *velocity = None,
                        Kind::Ray { velocity, .. } => *velocity = None,
                        Kind::Mote(mote) => mote.path = None,
                        Kind::Stream(stream::Stream::Smoke { emitted, .. }) => *emitted = 0,
                        _ => {}
                    }
                }
            }
            return Ok(previous);
        }
        const FIRST_PARAMETER: i32 = 113;
        let Some(slot @ 0..10) = property
            .checked_sub(FIRST_PARAMETER)
            .and_then(|i| usize::try_from(i).ok())
        else {
            return Ok(0);
        };
        match &mut self.kind {
            Kind::RadialSpray { palette: color } => match slot {
                0 => setting!(*color, value, palette_index),
                _ => Ok(0),
            },
            Kind::Explosion(s) => match slot {
                0 => setting!(s.palette, value, palette_index),
                1 => setting!(s.size, value),
                2 => setting!(s.variation, value, nonnegative),
                3 => setting!(s.count, value, count),
                4 => setting!(s.color_group, value),
                _ => Ok(0),
            },
            Kind::TwinGlow {
                size,
                satellite_size,
                angles,
            } => match slot {
                1 => setting!(*size, value),
                2 => setting!(*satellite_size, value),
                3..=4 => setting!(angles[slot - 3], value),
                _ => Ok(0),
            },
            Kind::Flash { sparks, lifetime } => {
                if *sparks && slot == 0 {
                    duration(lifetime, value)
                } else {
                    Ok(0)
                }
            }
            Kind::ModelTrail {
                remaining,
                target,
                velocity,
            } => match slot {
                1 => setting!(*remaining, value, nonnegative),
                4..=6 => displacement(target, velocity.as_mut(), slot - 4, value),
                _ => Ok(0),
            },
            Kind::Ray {
                palette: color,
                size,
                target,
                velocity,
            } => match slot {
                0 => setting!(*color, value, palette_index),
                1 => setting!(*size, value),
                4..=6 => displacement(target, velocity.as_mut(), slot - 4, value),
                _ => Ok(0),
            },
            Kind::LinearTrail {
                sprite,
                remaining,
                target,
                ..
            } => match slot {
                0 => color(sprite, value, palette_index),
                1 => diameter(sprite, value),
                2 => setting!(*remaining, value, nonnegative),
                3 => linear_fade(sprite, value, 1.),
                4..=6 => setting!(target[slot - 4], value),
                _ => Ok(0),
            },
            Kind::Burst(burst) => match slot {
                3 => setting!(burst.radius, value),
                4 => setting!(burst.spread, value, positive),
                5 => setting!(burst.tilt, value),
                7..=9 => setting!(burst.target[slot - 7], value),
                _ => Ok(0),
            },
            Kind::Mote(mote) => match slot {
                0 => setting!(mote.curvature, value),
                1 => setting!(mote.size, value),
                4..=6 => setting!(mote.target[slot - 4], value),
                _ => Ok(0),
            },
            Kind::Gathering { delay, .. } => match slot {
                0 => setting!(*delay, value, nonnegative),
                _ => Ok(0),
            },
            Kind::Column(column) => match slot {
                0 => setting!(column.palette, value, palette_index),
                1 => setting!(column.size, value),
                2 => setting!(column.layers, value, count),
                3 => setting!(column.alpha, value),
                5 => setting!(column.spacing, value),
                6 => setting!(column.lifetime, value, nonnegative),
                7 => setting!(column.growth, value),
                _ => Ok(0),
            },
            Kind::Contract(contract) => match slot {
                0 => setting!(contract.palette, value, palette_index),
                1 => setting!(contract.radius, value),
                2 => setting!(contract.lifetime, value, nonnegative),
                3 => setting!(contract.interval, value, positive),
                4 => setting!(contract.angular_step, value),
                5..=6 => setting!(contract.size[slot - 5], value),
                7 => setting!(contract.alpha, value),
                8 => setting!(contract.fade, value, |v: i32| Ok::<_, String>(i32::from(
                    v as i16
                ))),
                9 => setting!(contract.growth, value),
                _ => Ok(0),
            },
            Kind::Seal(seal) => match slot {
                0 => setting!(seal.palette, value, palette_group),
                1 => setting!(seal.size, value),
                2 => setting!(seal.spark_size, value),
                3 => setting!(seal.spark_lifetime, value, nonnegative),
                _ => Ok(0),
            },
            Kind::Stream(stream) => stream.property(slot, value),
            Kind::Fire { size, .. } => match slot {
                0 => setting!(*size, value, nonnegative),
                _ => Ok(0),
            },
            Kind::TwinTrail { sprite, radius, .. } => match slot {
                0 => color(sprite, value, palette),
                1 => setting!(*radius, value),
                2 => diameter(sprite, value),
                3 => {
                    let old = i32::from(sprite.rgba[3]) * 16;
                    if let Some(v) = value {
                        sprite.rgba[3] = (v / 16).clamp(0, 255) as u8;
                    }
                    Ok(old)
                }
                4 => linear_fade(sprite, value, 16.),
                _ => Ok(0),
            },
            Kind::AttachedTrail {
                sprite,
                offset,
                interval,
                ..
            } => match slot {
                0 => color(sprite, value, palette),
                1 => setting!(*offset, value),
                2 => lifetime(sprite, value),
                3 => diameter(sprite, value),
                4 => setting!(sprite.rgba[3], value, alpha),
                5 => fades(sprite, value),
                6 => setting!(sprite.size_delta, value),
                7 => setting!(*interval, value, positive),
                8 => {
                    let old = i32::from(matches!(sprite.orientation, SpriteOrientation::World));
                    if let Some(v) = value {
                        sprite.orientation = match v {
                            0 => SpriteOrientation::Camera,
                            1 => SpriteOrientation::World,
                            _ => return Err("invalid sprite orientation".into()),
                        };
                    }
                    Ok(old)
                }
                _ => Ok(0),
            },
            Kind::RisingOrbs(s) => match slot {
                0 => setting!(
                    s.palette,
                    value,
                    if s.drifting { palette } else { palette_index }
                ),
                1 => setting!(s.radius, value, nonnegative),
                2 => setting!(s.size, value),
                3 => setting!(s.variation, value, at_least_one),
                5 => setting!(s.speed_variation, value, at_least_one),
                8 => setting!(s.interval, value, at_least_one),
                9 => {
                    let old = i32::from(!s.preserve_particles);
                    if let Some(v) = value {
                        s.preserve_particles = !s.drifting && v != 1;
                    }
                    Ok(old)
                }
                _ => Ok(0),
            },
            Kind::Crown {
                palette: color,
                radius,
                spread,
                ..
            } => match slot {
                0 => setting!(*color, value, palette_index),
                1 => setting!(*radius, value),
                2 => setting!(*spread, value),
                _ => Ok(0),
            },
            Kind::Bloom(s) => match slot {
                0 => setting!(s.palette, value, palette_index),
                1 => setting!(s.lifetime, value, nonnegative),
                2 => setting!(s.count, value, count),
                3 => setting!(s.size[0], value),
                4 => setting!(s.variation[0], value, at_least_one),
                5 => setting!(s.size[1], value),
                6 => setting!(s.variation[1], value, at_least_one),
                _ => Ok(0),
            },
            Kind::Shafts(s) => match slot {
                0 => setting!(s.palette, value, palette),
                1 => setting!(s.interval, value, at_least_one),
                2 => setting!(s.radius, value),
                3 => setting!(s.size[0], value),
                4 => setting!(s.variation[0], value, at_least_one),
                5 => setting!(s.size[1], value),
                6 => setting!(s.variation[1], value, at_least_one),
                7 => setting!(s.tilt, value),
                8 => setting!(s.cluster, value, |v: i32| Ok::<_, String>(v.clamp(1, 100))),
                _ => Ok(0),
            },
            Kind::Convergence(s) => match slot {
                0 => setting!(s.palette, value, palette_index),
                1 => setting!(s.size, value),
                2 => setting!(s.growth, value),
                3 => setting!(s.radius, value, |v: i32| Ok::<_, String>(v.max(0))),
                4 => setting!(s.spread, value, at_least_one),
                _ => Ok(0),
            },
            Kind::Aura {
                palette: color,
                offset,
                ..
            } => match slot {
                0 => setting!(*color, value, palette),
                1 => setting!(*offset, value),
                _ => Ok(0),
            },
            Kind::Glow {
                palette: color,
                size,
                retire_with_emitter,
                ..
            } => match slot {
                0 => setting!(*color, value, palette_index),
                1 => setting!(*size, value),
                9 => flag(retire_with_emitter, value),
                _ => Ok(0),
            },
            Kind::Portal {
                palette: color,
                size,
                ..
            } => match slot {
                0 => setting!(*color, value, palette_index),
                1 => setting!(*size, value, nonnegative),
                _ => Ok(0),
            },
            Kind::Charge {
                palette: color,
                radius,
                updates,
                target,
                travelling,
                velocity,
            } => match slot {
                0 => setting!(*color, value, palette_index),
                1 => setting!(*updates, value, positive),
                2 => flag(travelling, value),
                3 => setting!(*radius, value),
                4..=6 => displacement(target, velocity.as_mut(), slot - 4, value),
                _ => Ok(0),
            },
            Kind::Cloud(s) => match slot {
                0 => setting!(s.palette, value, palette_group),
                1..=2 => setting!(s.size[slot - 1], value),
                3..=4 => setting!(s.lifetime[slot - 3], value, nonnegative),
                5..=6 => setting!(s.alpha[slot - 5], value, nonnegative),
                7..=8 => scaled(
                    &mut s.fade[slot - 7],
                    value.map(|v| i32::from(v as i16)),
                    16.,
                ),
                _ => Ok(0),
            },
            Kind::Scatter(s) => match slot {
                0 => setting!(s.palette, value, |v| bounded(
                    v,
                    0,
                    super::scatter::MAX_PALETTE
                )),
                1 => setting!(s.size[0], value),
                2 => setting!(s.variation[0], value, at_least_one),
                3 => setting!(s.size[1], value),
                4 => setting!(s.variation[1], value, at_least_one),
                5..=6 => duration(&mut s.lifetime[slot - 5], value),
                _ => Ok(0),
            },
            Kind::Travel {
                palette: color,
                size,
                burst_size,
                fade,
                target,
                flight,
                ..
            } => match slot {
                0 => setting!(*color, value, palette_index),
                1 => setting!(*size, value),
                2 => setting!(*burst_size, value),
                3 => setting!(*fade, value),
                4..=6 => displacement(
                    target,
                    flight.as_mut().map(|f| &mut f.velocity),
                    slot - 4,
                    value,
                ),
                _ => Ok(0),
            },
            Kind::Inward {
                palette: color,
                radius,
                count: amount,
                curve,
                size,
                clear,
                blend,
                ..
            } => match slot {
                0 => setting!(*color, value, palette),
                1 => setting!(*radius, value, nonnegative),
                2 => setting!(*amount, value, count),
                3 => {
                    let old = (*curve * 100.) as i32;
                    if let Some(v) = value {
                        *curve = (v / 100) as f32;
                    }
                    Ok(old)
                }
                4 => setting!(*size, value),
                8 => setting!(*clear, value),
                9 => setting!(*blend, value),
                _ => Ok(0),
            },
            Kind::Projectile {
                color,
                curvature,
                size,
                target,
                launch,
                ..
            } => match slot {
                0 => setting!(*curvature, value),
                1 if matches!(color, ProjectileColor::Selected(_)) => {
                    let ProjectileColor::Selected(color) = color else {
                        unreachable!()
                    };
                    setting!(*color, value, palette_index)
                }
                1 => setting!(*size, value, nonnegative),
                2 if matches!(color, ProjectileColor::Selected(_)) => {
                    setting!(*size, value, nonnegative)
                }
                3 => match color {
                    ProjectileColor::Changing { fade } => scaled(fade, value, 16.),
                    _ => Ok(0),
                },
                4..=6 => setting!(target[slot - 4], value),
                7..=9 => setting!(launch[slot - 7], value),
                _ => Ok(0),
            },
            Kind::Rising(s) => match slot {
                0 => setting!(s.palette, value, palette_index),
                1 => setting!(s.radius, value, nonnegative),
                2 => setting!(s.size[0], value),
                3 => setting!(s.variation[0], value, at_least_one),
                4 => setting!(s.size[1], value),
                5 => setting!(s.variation[1], value, at_least_one),
                6 => setting!(s.alpha, value, |v| bounded(v, 0, 255)),
                7 => setting!(s.rise, value, at_least_one),
                8 => {
                    let old = s.interval as i32;
                    if let Some(v) = value {
                        s.interval = v.unsigned_abs();
                    }
                    Ok(old)
                }
                9 => duration(&mut s.lifetime, value),
                _ => Ok(0),
            },
            Kind::Fireball {
                size,
                updates,
                target,
                velocity,
                ..
            } => match slot {
                0 => setting!(*size, value, nonnegative),
                1 => setting!(*updates, value, positive),
                4..=6 => displacement(target, velocity.as_mut(), slot - 4, value),
                _ => Ok(0),
            },
            Kind::Cardinal { count, .. } => match slot {
                0 => setting!(*count, value, |v| bounded(v, 0, 4)),
                _ => Ok(0),
            },
            Kind::Quake => Ok(0),
        }
    }
}

impl stream::Stream {
    fn property(&mut self, slot: usize, value: Option<i32>) -> Result<i32, String> {
        use stream::Stream::*;
        match self {
            PlanarLights {
                sprite,
                radius,
                variation,
                interval,
                spin,
            } => match slot {
                0 => setting!(*radius, value, nonnegative),
                1 => lifetime(sprite, value),
                2 => diameter(sprite, value),
                3 => setting!(*variation, value, nonnegative),
                4 => setting!(sprite.rgba[3], value, alpha),
                5 => {
                    let old = if let Fade::Linear(fade) = sprite.fade {
                        fade as i32
                    } else {
                        0
                    };
                    if let Some(v) = value {
                        sprite.fade = if v == 0 {
                            Fade::tail(sprite.lifetime)
                        } else {
                            Fade::Linear(v.wrapping_mul(16) as i16 as f32 / 16.)
                        };
                    }
                    Ok(old)
                }
                6 => setting!(*interval, value, positive),
                7 => setting!(*spin, value),
                _ => Ok(0),
            },
            RisingCircle {
                radius,
                size,
                variation,
                interval,
                interval_variation,
            } => match slot {
                0 => setting!(*radius, value),
                1 => setting!(*size, value),
                2 => setting!(*variation, value, nonnegative),
                3 => setting!(*interval, value, nonnegative),
                4 => setting!(*interval_variation, value, nonnegative),
                _ => Ok(0),
            },
            Lightning { sprite, radius } => match slot {
                0 => color(sprite, value, palette_index),
                1 => lifetime(sprite, value),
                2 => setting!(*radius, value, nonnegative),
                _ => Ok(0),
            },
            Flame {
                size,
                palette: color,
                tint,
            } => match slot {
                0 => setting!(*size, value, nonnegative),
                1 => setting!(*color, value, palette_index),
                2..=4 => setting!(tint[slot - 2], value),
                _ => Ok(0),
            },
            Embers {
                radius,
                size,
                variation,
            } => match slot {
                0 => setting!(*radius, value, nonnegative),
                1 => setting!(*size, value),
                2 => setting!(*variation, value, nonnegative),
                _ => Ok(0),
            },
            Splash {
                sprite,
                interval,
                angle,
                radius,
                variation,
                rise,
                speed,
                ..
            } => match slot {
                0 => color(sprite, value, palette_index),
                1 => setting!(*radius, value, nonnegative),
                2 => lifetime(sprite, value),
                3 => setting!(*interval, value, positive),
                4 => setting!(*angle, value, positive),
                5 => diameter(sprite, value),
                6 => setting!(sprite.rgba[3], value, alpha),
                7 => setting!(*rise, value),
                8 => setting!(*speed, value),
                9 => setting!(*variation, value, nonnegative),
                _ => Ok(0),
            },
            Stars {
                filled,
                sprite,
                interval,
                radius,
                variation,
                speed_variation,
            } => match slot {
                0 => color(sprite, value, palette_index),
                1 => setting!(*radius, value, nonnegative),
                2 => diameter(sprite, value),
                3 => setting!(*variation, value, nonnegative),
                4 if *filled => lifetime(sprite, value),
                5 if *filled => setting!(*speed_variation, value, nonnegative),
                6 if *filled => setting!(sprite.rgba[3], value, alpha),
                7 if *filled => {
                    let old = match sprite.fade {
                        Fade::Linear(delta) => delta as i32,
                        _ => 0,
                    };
                    if let Some(value) = value {
                        sprite.fade = if value == 0 {
                            Fade::tail(sprite.lifetime)
                        } else {
                            Fade::Linear(value as i16 as f32)
                        };
                    }
                    Ok(old)
                }
                5 => {
                    let previous = setting!(*speed_variation, value, nonnegative)?;
                    sprite.rgba[3] = (*speed_variation).min(255) as u8;
                    Ok(previous)
                }
                6 => fades(sprite, value),
                7 => lifetime(sprite, value),
                8 => setting!(*interval, value, positive),
                _ => Ok(0),
            },
            TargetedStars {
                sprite,
                spread,
                variation,
                target,
            } => match slot {
                0 => color(sprite, value, palette_group),
                1 => setting!(*spread, value, nonnegative),
                2 => diameter(sprite, value),
                3 => lifetime(sprite, value),
                4..=6 => setting!(target[slot - 4], value),
                7 => setting!(*variation, value, nonnegative),
                _ => Ok(0),
            },
            Smoke {
                sprite,
                count,
                limit,
                radius,
                height_step,
                variation,
                ..
            } => match slot {
                0 => setting!(*limit, value, nonnegative),
                1 => setting!(*count, value, positive),
                2..=3 => setting!(radius[slot - 2], value, nonnegative),
                4 => setting!(*height_step, value),
                5 => diameter(sprite, value),
                6 => setting!(*variation, value, nonnegative),
                7 => setting!(sprite.rgba[3], value, alpha),
                9 => linear_fade(sprite, value, 1.),
                _ => Ok(0),
            },
            RisingSmoke {
                remaining,
                sprite,
                radius,
                height_step,
                size_variation,
                alpha_variation,
                ..
            } => match slot {
                0 => setting!(*remaining, value, nonnegative),
                1 => lifetime(sprite, value),
                2..=3 => setting!(radius[slot - 2], value, nonnegative),
                4 => setting!(*height_step, value),
                5 => diameter(sprite, value),
                6 => setting!(*size_variation, value, nonnegative),
                7 => setting!(sprite.rgba[3], value, alpha),
                8 => setting!(*alpha_variation, value, nonnegative),
                _ => Ok(0),
            },
            Rain | ConvergingShafts | Spiral => Ok(0),
        }
    }
}

fn flag(field: &mut bool, value: Option<i32>) -> Result<i32, String> {
    let previous = i32::from(*field);
    if let Some(v) = value {
        *field = v != 0;
    }
    Ok(previous)
}
fn scaled(field: &mut f32, value: Option<i32>, divisor: f32) -> Result<i32, String> {
    let previous = (*field * divisor) as i32;
    if let Some(v) = value {
        *field = v as f32 / divisor;
    }
    Ok(previous)
}
fn color(
    sprite: &mut BillboardEffect,
    value: Option<i32>,
    validate: fn(i32) -> Result<i32, String>,
) -> Result<i32, String> {
    setting!(*sprite.palette.as_mut().unwrap(), value, validate)
}
fn diameter(sprite: &mut BillboardEffect, value: Option<i32>) -> Result<i32, String> {
    let previous = setting!(sprite.size[0], value)?;
    sprite.size[1] = sprite.size[0];
    Ok(previous)
}
fn duration(field: &mut u32, value: Option<i32>) -> Result<i32, String> {
    let previous = field.saturating_sub(1) as i32;
    if let Some(v) = value {
        *field = nonnegative(v)? as u32 + 1;
    }
    Ok(previous)
}
fn lifetime(sprite: &mut BillboardEffect, value: Option<i32>) -> Result<i32, String> {
    let previous = duration(&mut sprite.lifetime, value)?;
    if matches!(sprite.fade, Fade::Tail { .. }) {
        sprite.fade = Fade::tail(sprite.lifetime);
    }
    Ok(previous)
}
fn fades(sprite: &mut BillboardEffect, value: Option<i32>) -> Result<i32, String> {
    let previous = i32::from(matches!(sprite.fade, Fade::Tail { .. }));
    if let Some(v) = value {
        sprite.fade = if v == 0 {
            Fade::Linear(0.)
        } else {
            Fade::tail(sprite.lifetime)
        };
    }
    Ok(previous)
}
fn linear_fade(
    sprite: &mut BillboardEffect,
    value: Option<i32>,
    divisor: f32,
) -> Result<i32, String> {
    let Fade::Linear(ref mut fade) = sprite.fade else {
        unreachable!()
    };
    scaled(fade, value.map(|v| i32::from(v as i16)), divisor)
}

pub(super) fn bounded(value: i32, min: i32, max: i32) -> Result<i32, String> {
    (min..=max)
        .contains(&value)
        .then_some(value)
        .ok_or_else(|| "invalid emitter settings".into())
}
fn count(v: i32) -> Result<i32, String> {
    bounded(v, 0, BILLBOARD_LIMIT as i32)
}
fn positive(v: i32) -> Result<i32, String> {
    bounded(v, 1, i32::MAX)
}
fn at_least_one(v: i32) -> Result<i32, String> {
    Ok(v.max(1))
}
fn alpha(v: i32) -> Result<i32, String> {
    Ok(v.clamp(0, 255))
}
pub(super) fn palette(v: i32) -> Result<i32, String> {
    bounded(v, 0, 108)
}
pub(super) fn palette_index(v: i32) -> Result<i32, String> {
    bounded(
        v,
        0,
        resonance_content::effect::FIELD_PALETTE_COLORS as i32 - 1,
    )
}
pub(super) fn palette_group(v: i32) -> Result<i32, String> {
    bounded(
        v,
        0,
        resonance_content::effect::FIELD_PALETTE_COLORS as i32 - 4,
    )
}
pub(super) fn nonnegative(v: i32) -> Result<i32, String> {
    bounded(v, 0, i32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn emitters_reject_invalid_palettes_without_losing_their_settings() {
        for (preset, maximum) in [(13, 106), (26, 108), (27, 109), (36, 106), (49, 106)] {
            let mut args = [0; 18];
            args[5] = preset;
            args[8] = 33;
            let mut emitter = Emitter::from_native(&args, None).unwrap();
            for palette in [-1, maximum + 1, i32::MAX] {
                args[8] = palette;
                assert!(Emitter::from_native(&args, None).is_err());
                assert!(emitter.property(113, Some(palette)).is_err());
                assert_eq!(emitter.property(113, None).unwrap(), 33);
            }
            emitter.property(113, Some(maximum)).unwrap();
            let mut actor = crate::Actor::new(0, [0.; 3]);
            let mut random = 1;
            let mut output = super::super::Births::default();
            for tick in 0..30 {
                emitter
                    .step(
                        (500, [0.; 3]),
                        &mut actor,
                        tick,
                        tick,
                        [0.; 3],
                        &mut random,
                        &mut output,
                    )
                    .unwrap();
            }
            let mut world = crate::GameWorld::default();
            for birth in output.items {
                if let super::super::Birth::Sprite(particle, _) = birth {
                    world.emit_billboard(particle).unwrap();
                }
            }
        }
    }

    #[test]
    fn changing_seal_opacity_preserves_its_descent() {
        let mut emitter = Emitter::from_native(
            &[
                500, 0, 0, 0, 0, 31, 0, 0, 33, 20, 60, 1, 10, 20, 20, 255, 0, 0,
            ],
            None,
        )
        .unwrap();
        let mut actor = crate::Actor::new(0, [0.; 3]);
        actor.set_movement_speed(-2.);
        let mut random = 1;
        let mut output = super::super::Births::default();
        for tick in 0..=5 {
            if tick == 2 {
                emitter.property(120, Some(128)).unwrap();
            }
            emitter
                .step(
                    (500, actor.position),
                    &mut actor,
                    tick,
                    tick,
                    [0.; 3],
                    &mut random,
                    &mut output,
                )
                .unwrap();
        }
        assert_eq!(actor.position, [0., 0., -48.]);
        let super::super::Birth::Sprite(particle, _) = output.items.last().unwrap() else {
            panic!("expected a descending sprite");
        };
        assert_eq!(particle.rgba[3], 128);
    }

    #[test]
    fn invalid_count_updates_leave_a_usable_emitter() {
        let mut emitter = Emitter::from_native(
            &[500, 0, 0, 0, 0, 60, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            None,
        )
        .unwrap();
        for count in [-1, 5, i32::MAX] {
            assert!(emitter.property(113, Some(count)).is_err());
            assert_eq!(emitter.property(113, None).unwrap(), 4);
        }
    }
}

// Launch coordinates become per-update displacement while an effect is in flight.
fn displacement(
    target: &mut [f32; 3],
    velocity: Option<&mut [f32; 3]>,
    axis: usize,
    value: Option<i32>,
) -> Result<i32, String> {
    if let Some(velocity) = velocity {
        setting!(velocity[axis], value)
    } else {
        setting!(target[axis], value)
    }
}
