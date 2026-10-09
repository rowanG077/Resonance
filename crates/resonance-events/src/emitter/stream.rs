//! Particle births for continuous sprays. Motion and fading use ordinary billboards.
use super::{normalized, particle};
use crate::{
    Actor,
    effect::{BillboardEffect, Fade, SpriteOrientation},
    world::random,
};

#[derive(Debug, Clone)]
pub(crate) enum Stream {
    Splash {
        sprite: BillboardEffect,
        interval: u32,
        angle: u32,
        radius: u32,
        variation: u32,
        rise: f32,
        speed: f32,
        filled: bool,
    },
    Stars {
        filled: bool,
        sprite: BillboardEffect,
        interval: u32,
        radius: f32,
        variation: u32,
        speed_variation: u32,
    },
    TargetedStars {
        sprite: BillboardEffect,
        spread: u32,
        variation: u32,
        target: [f32; 3],
    },
    Rain,
    ConvergingShafts,
    Spiral,
    PlanarLights {
        sprite: BillboardEffect,
        radius: u32,
        variation: u32,
        interval: u32,
        spin: f32,
    },
    RisingCircle {
        radius: f32,
        size: f32,
        variation: u32,
        interval: u32,
        interval_variation: u32,
    },
    Lightning {
        sprite: BillboardEffect,
        radius: u32,
    },
    Flame {
        size: u32,
        palette: u16,
        tint: [u8; 3],
    },
    Embers {
        radius: u32,
        size: f32,
        variation: u32,
    },
    RisingSmoke {
        emitted: u32,
        remaining: u32,
        sprite: BillboardEffect,
        radius: [u32; 2],
        height_step: f32,
        size_variation: u32,
        alpha_variation: u32,
    },
    Smoke {
        emitted: u32,
        sprite: BillboardEffect,
        count: u32,
        limit: u32,
        radius: [u32; 2],
        height_step: f32,
        variation: u32,
    },
}

impl Stream {
    pub(super) fn new(kind: i32) -> Self {
        let mut sprite = particle([0.; 3], 0, 0, 1);
        match kind {
            9 | 75 => {
                sprite.recipe = crate::effect::STATION_GLOW_SPRITE;
                sprite.gravity = -0.98;
                sprite.fade = Fade::Linear(0.);
                Self::Splash {
                    sprite,
                    interval: 1,
                    angle: 1,
                    radius: 0,
                    variation: 0,
                    rise: 0.,
                    speed: 0.,
                    filled: kind == 75,
                }
            }
            44 | 54 => Self::Stars {
                filled: kind == 44,
                sprite,
                interval: 1,
                radius: 0.,
                variation: 0,
                speed_variation: 0,
            },
            36 => {
                sprite.rgba[3] = 150;
                Self::TargetedStars {
                    sprite,
                    spread: 0,
                    variation: 0,
                    target: [0.; 3],
                }
            }
            17 => Self::Rain,
            20 => Self::ConvergingShafts,
            21 => Self::Spiral,
            57 => {
                sprite.orientation = SpriteOrientation::World;
                Self::PlanarLights {
                    sprite,
                    radius: 0,
                    variation: 0,
                    interval: 1,
                    spin: 0.,
                }
            }
            64 => Self::RisingCircle {
                radius: 0.,
                size: 0.,
                variation: 0,
                interval: 1,
                interval_variation: 0,
            },
            39 => {
                sprite.recipe = crate::effect::LIGHTNING_BOLT_SPRITE;
                Self::Lightning { sprite, radius: 0 }
            }
            25 => Self::Flame {
                size: 0,
                palette: 0,
                tint: [0; 3],
            },
            62 => Self::Embers {
                radius: 0,
                size: 0.,
                variation: 0,
            },
            41 => {
                sprite.recipe = crate::effect::GLOW_SPRITE;
                sprite.palette = Some(33);
                Self::RisingSmoke {
                    emitted: 0,
                    remaining: 0,
                    sprite,
                    radius: [0; 2],
                    height_step: 0.,
                    size_variation: 0,
                    alpha_variation: 0,
                }
            }
            24 => {
                sprite.recipe = crate::effect::SMOKE_SPRITE;
                sprite.palette = Some(33);
                sprite.lifetime = 301;
                // The emitter uses a stationary puff from the smoke atlas.
                sprite.uv = Some([0., 64., 63., 127.].map(|v| v / 256.));
                sprite.fade = Fade::Linear(0.);
                Self::Smoke {
                    emitted: 0,
                    sprite,
                    count: 1,
                    limit: 0,
                    radius: [0; 2],
                    height_step: 0.,
                    variation: 0,
                }
            }
            _ => unreachable!(),
        }
    }

    pub(super) fn particles(
        &mut self,
        (owner, center): (i32, [f32; 3]),
        actor: &Actor,
        born: u32,
        clock: u32,
        rng: &mut u32,
        out: &mut super::Births,
    ) {
        random(rng);
        let speed = actor.movement_speed() / 100.;
        let start = |sprite: &BillboardEffect| {
            let mut p = sprite.clone();
            p.position = center;
            p.born = born;
            p
        };
        match self {
            Self::ConvergingShafts if clock.is_multiple_of(2) => {
                const START_RADIUS: f32 = 650.;
                const ACCELERATION: f32 = 0.02;
                let mut p = particle(center, born, 46, 301);
                p.recipe = crate::effect::STREAK_SPRITE;
                p.orientation = SpriteOrientation::World;
                p.field_fog = false;
                p.size = [
                    (5 + random(rng) % 4) as f32,
                    (100 + random(rng) % 50) as f32,
                ];
                p.rgba[3] = 150;
                p.fade = Fade::Linear(-1.);
                let angle = (10 + random(rng) % 160) as f32;
                let (sin, cos) = angle.to_radians().sin_cos();
                let radial = [cos, 0., sin];
                p.rotation = [90., 90. - angle, 0.];
                p.position = std::array::from_fn(|i| center[i] + radial[i] * START_RADIUS);
                p.velocity = radial.map(|v| -v * (3. + ACCELERATION));
                p.controller = Some(crate::effect::BillboardController::Accelerate {
                    multiplier: 1.,
                    delta: radial.map(|v| -v * ACCELERATION),
                });
                out.push(p);
            }
            Self::Spiral if clock.is_multiple_of(5) => {
                let mut p = particle(center, born, 33 + (random(rng) % 28) as u16, 601);
                p.field_fog = false;
                p.size = [(45 + random(rng) % 45) as f32; 2];
                p.rgba[3] = 150;
                p.fade = Fade::Linear(0.);
                let mut origin = center;
                origin[2] += 5.;
                let mut orbit = super::Orbit::new(origin, [0., 0., 1.], 150., 1., 105., 15.);
                orbit.rise = 5.;
                p.controller = Some(crate::effect::BillboardController::Orbit(orbit));
                out.push(p);
            }
            Self::PlanarLights {
                sprite,
                radius,
                variation,
                interval,
                spin,
            } if clock.is_multiple_of(*interval) => {
                let mut p = start(sprite);
                let images = crate::effect::PLANE_LIGHT_SPRITES;
                p.recipe = images[random(rng) as usize % images.len()];
                p.palette = Some(77 + (random(rng) % 3) as u16);
                p.size = [p.size[0] + spread(rng, *variation) as f32; 2];
                p.rotation[2] = (random(rng) % 360) as f32;
                p.angular_velocity[2] = if random(rng).is_multiple_of(2) {
                    -*spin
                } else {
                    *spin
                };
                let (sin, cos) = ((random(rng) % 360) as f32).to_radians().sin_cos();
                let distance = spread(rng, *radius) as f32;
                p.position[0] += cos * distance;
                p.position[1] += sin * distance;
                out.push(p);
            }
            Self::RisingCircle {
                radius,
                size,
                variation,
                interval,
                interval_variation,
            } => {
                let interval = (*interval + spread(rng, *interval_variation)).max(1);
                if !clock.is_multiple_of(interval) {
                    return;
                }
                let mut p = particle(center, born, super::palette(108, rng), 76);
                p.recipe = crate::effect::RISING_LIGHT_SPRITE;
                p.size = [*size + spread(rng, *variation) as f32; 2];
                p.rgba[3] = 150;
                p.velocity[2] = actor.movement_speed();
                p.angular_velocity[2] = if random(rng).is_multiple_of(2) {
                    -2.
                } else {
                    2.
                };
                let (sin, cos) = ((random(rng) % 360) as f32).to_radians().sin_cos();
                p.position[0] += cos * *radius;
                p.position[1] += sin * *radius;
                out.push(p);
            }
            Self::Lightning { sprite, radius } if clock.is_multiple_of(5) => {
                let mut p = start(sprite);
                p.size = [(96 + random(rng) % 32) as f32, 1024.];
                p.rotation[1] = if random(rng).is_multiple_of(2) {
                    0.
                } else {
                    180.
                };
                let (sin, cos) = ((random(rng) % 360) as f32).to_radians().sin_cos();
                let radius = spread(rng, *radius) as f32;
                p.position[0] += cos * radius;
                p.position[1] += sin * radius;
                out.push(p);
            }
            Self::Flame {
                size,
                palette,
                tint,
            } if clock.is_multiple_of(4) => {
                if *size == 0 {
                    *size = 60;
                }
                let mut p = particle(center, born, *palette, (*size).max(60) + 1);
                p.owner = Some(owner);
                p.recipe = crate::effect::GLOW_SPRITE;
                p.blend = Some(actor.blend.unwrap_or(crate::effect::Blend::Additive));
                p.size = [(*size + random(rng) % 16) as f32; 2];
                p.size_delta = -1.;
                p.velocity = [
                    (random(rng) % 16) as f32 / 32.,
                    0.,
                    2. + (random(rng) % 16) as f32 / 16.,
                ];
                p.angular_velocity[2] = -3.;
                p.fade = Fade::Linear(0.);
                out.push_tinted(p, tint.map(|v| (v != 0).then_some(v)));
            }
            Self::Embers {
                radius,
                size,
                variation,
            } if clock % 4 >= 2 => {
                let color = if random(rng).is_multiple_of(2) {
                    80
                } else {
                    104
                };
                let mut p = particle(center, born, color, 301);
                p.field_fog = false;
                p.size = [*size + spread(rng, *variation) as f32; 2];
                p.fade = Fade::Linear(-2.);
                p.velocity[2] = ((random(rng) % 500 + 100) / 100) as f32;
                let (sin, cos) = (random(rng) as f32 % 360.).to_radians().sin_cos();
                let radius = spread(rng, *radius) as f32;
                p.position[0] += cos * radius;
                p.position[1] += sin * radius;
                out.push(p);
            }
            Self::Splash {
                sprite,
                interval,
                angle,
                radius,
                variation,
                rise,
                speed,
                filled,
            } if clock.is_multiple_of(*interval) => {
                for angle in (*angle..=360).step_by(*angle as usize) {
                    let mut p = start(sprite);
                    let distance = if *filled {
                        p.size = [p.size[0] + spread(rng, *variation) as f32; 2];
                        spread(rng, *radius)
                    } else {
                        *radius
                    } as f32;
                    let (sin, cos) = (angle as f32).to_radians().sin_cos();
                    p.position[0] += cos * distance;
                    p.position[1] += sin * distance;
                    let [vx, vy] = if *radius == 0 {
                        std::array::from_fn(|_| {
                            *speed - spread(rng, (*speed * 2. + 1.) as u32) as f32
                        })
                    } else {
                        [cos * *speed, sin * *speed]
                    };
                    p.velocity = [vx, vy, *rise + (random(rng) % 5) as f32];
                    super::inherit(&mut p, actor);
                    out.push(p);
                }
            }
            Self::Stars {
                filled,
                sprite,
                interval,
                radius,
                variation,
                speed_variation,
            } if clock.is_multiple_of(*interval) => {
                let mut p = start(sprite);
                let image = random(rng) % 3;
                p.recipe = if *filled {
                    [
                        crate::effect::ORB_SPRITE,
                        crate::effect::TRAIL_GLOW_SPRITE,
                        crate::effect::SEAL_SPARK_SPRITE,
                    ]
                } else {
                    [
                        crate::effect::ORB_SPRITE,
                        crate::effect::SEAL_SPARK_SPRITE,
                        crate::effect::SEAL_STAR_SPRITE,
                    ]
                }[image as usize];
                p.size = [p.size[0] + spread(rng, *variation) as f32; 2];
                p.velocity[2] = speed + spread(rng, *speed_variation) as f32 / 100.;
                if *filled || image != 0 {
                    p.angular_velocity[2] = spin(rng, if *filled { 5. } else { 2. });
                }
                let (sin, cos) = (random(rng) as f32 % 360.).to_radians().sin_cos();
                for (axis, direction) in [sin, -cos, 0.].into_iter().enumerate() {
                    let distance = if *filled {
                        spread(rng, *radius as u32) as f32
                    } else {
                        *radius
                    };
                    p.position[axis] += direction * distance;
                }
                out.push(p);
            }
            Self::TargetedStars {
                sprite,
                spread: radius,
                variation,
                target,
            } if clock.is_multiple_of(3) => {
                let mut p = start(sprite);
                let image = random(rng) % 3;
                p.recipe = if image == 0 {
                    crate::effect::ORB_SPRITE
                } else {
                    crate::effect::SEAL_SPARK_SPRITE
                };
                p.palette = Some(p.palette.unwrap() + (random(rng) % 4) as u16);
                let size = p.size[0] + spread(rng, *variation) as f32;
                p.size = [size * if image == 1 { 1.2 } else { 1. }; 2];
                let direction = std::array::from_fn(|i| {
                    target[i] - center[i] + *radius as f32
                        - spread(rng, radius.saturating_mul(2)) as f32
                });
                p.velocity = normalized(direction).map(|v| v * speed);
                p.angular_velocity[2] = spin(rng, 3.);
                out.push(p);
            }
            Self::Rain => {
                let yaw = 30. - (random(rng) % 60) as f32;
                let tilt = 25. - (random(rng) % 50) as f32;
                random(rng);
                let alpha = 100 + random(rng) % 50;
                let width = 10 + random(rng) % 10;
                let height = 200 + random(rng) % 300;
                let speed = 30. + (random(rng) % 20) as f32;
                let (sy, cy) = yaw.to_radians().sin_cos();
                let (st, ct) = tilt.to_radians().sin_cos();
                let direction = [-sy, st * cy, -ct * cy];
                let mut p = particle(
                    std::array::from_fn(|i| center[i] - direction[i] * 1000.),
                    born,
                    33,
                    121,
                );
                p.recipe = crate::effect::GLOW_SPRITE;
                p.orientation = SpriteOrientation::World;
                p.anchor = resonance_content::effect::VerticalAnchor::Bottom;
                p.rotation = [90., yaw, 0.];
                p.velocity = direction.map(|v| v * speed);
                p.size = [width as f32, height as f32];
                p.rgba[3] = alpha as u8;
                out.push(p);
            }
            Self::RisingSmoke {
                emitted,
                remaining,
                sprite,
                radius,
                height_step,
                size_variation,
                alpha_variation,
            } if *remaining > 0 => {
                let mut p = start(sprite);
                for (axis, &radius) in radius.iter().enumerate() {
                    if radius != 0 {
                        p.position[axis] +=
                            radius as f32 - spread(rng, radius.saturating_mul(2)) as f32;
                    }
                }
                p.position[2] += *emitted as f32 * *height_step;
                p.size_delta = 1. + (random(rng) % 2) as f32;
                p.rgba[3] = p.rgba[3].wrapping_add(spread(rng, *alpha_variation) as u8);
                p.size = [p.size[0] + spread(rng, *size_variation) as f32; 2];
                p.velocity[2] = speed;
                out.push(p);
                *emitted += 1;
                *remaining -= 1;
            }
            Self::Smoke {
                emitted,
                sprite,
                count,
                limit,
                radius,
                height_step,
                variation,
            } => {
                for _ in 0..(*count).min(limit.saturating_sub(*emitted)) {
                    let mut p = start(sprite);
                    const GRAY_INTENSITY: u32 = 175;
                    const GRAY_VARIATION: u32 = 100;
                    const TINT_SCALE: u32 = 3;
                    p.rgba[..3]
                        .fill(((GRAY_INTENSITY - random(rng) % GRAY_VARIATION) / TINT_SCALE) as u8);
                    for (axis, radius) in radius.iter().enumerate() {
                        p.position[axis] +=
                            *radius as f32 - spread(rng, radius.saturating_mul(2)) as f32;
                    }
                    p.position[2] += *emitted as f32 * *height_step;
                    p.size_delta = 1. + (random(rng) % 2) as f32;
                    p.rgba[3] = p.rgba[3].wrapping_add(random(rng) as u8);
                    p.size = [p.size[0] + spread(rng, *variation) as f32; 2];
                    let direction = [
                        50. - (random(rng) % 100) as f32,
                        50. - (random(rng) % 100) as f32,
                        (random(rng) % 100) as f32,
                    ];
                    p.velocity = normalized(direction).map(|v| v * speed);
                    out.push(p);
                    *emitted += 1;
                }
            }
            _ => {}
        }
    }
}

pub(super) fn spread(rng: &mut u32, range: u32) -> u32 {
    // Zero leaves the sample unbounded instead of suppressing its variation.
    let sample = random(rng);
    sample.checked_rem(range).unwrap_or(sample)
}
fn spin(rng: &mut u32, speed: f32) -> f32 {
    if random(rng).is_multiple_of(2) {
        -speed
    } else {
        speed
    }
}
