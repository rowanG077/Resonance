//! Scene effects share particle births, analytic paths and bounded stage timing.
mod burst;
mod cloud;
mod column;
mod contract;
mod fire;
mod gathering;
mod mote;
mod native;
pub(crate) mod orbiting;
mod planes;
mod rays;
pub(crate) mod scatter;
mod seal;
mod stream;
use crate::effect::emission::normalized;
use crate::{
    Actor, GameWorld,
    effect::{
        BillboardController, BillboardEffect, Fade, NEUTRAL_TINT, RefractionImage, RefractionPulse,
        SpriteOrientation,
    },
};

pub(crate) const PHASE_PROPERTY: i32 = 33;

#[derive(Default)]
pub(crate) struct Assets {
    pub textures: [Option<(u32, u8)>; 2],
    pub rising_light_destination: Option<[f32; 3]>,
}

#[derive(Debug, Clone)]
pub(crate) struct Emitter {
    phase: u8,
    age: u32,
    kind: Kind,
}

#[derive(Debug, Clone)]
enum Kind {
    Burst(burst::Burst),
    Gathering {
        delay: u32,
        state: gathering::Gathering,
    },
    Seal(seal::Seal),
    Column(column::Column),
    Contract(contract::Contract),
    Mote(mote::Mote),
    Stream(stream::Stream),
    TwinTrail {
        sprite: BillboardEffect,
        radius: f32,
    },
    AttachedTrail {
        sprite: BillboardEffect,
        offset: f32,
        interval: u32,
    },
    LinearTrail {
        sprite: BillboardEffect,
        remaining: i32,
        target: [f32; 3],
        velocity: Option<[f32; 3]>,
    },
    ModelTrail {
        remaining: i32,
        target: [f32; 3],
        velocity: Option<[f32; 3]>,
    },
    ChargedTrail {
        size: f32,
        fade: f32,
        target: [f32; 3],
        flight: Option<Flight>,
    },
    Ray {
        palette: u16,
        size: f32,
        target: [f32; 3],
        velocity: Option<[f32; 3]>,
    },
    Flash {
        sparks: bool,
        lifetime: u32,
    },
    RadialSpray {
        palette: u16,
    },
    ChargedRay(rays::ChargedRay),
    Explosion(rays::Explosion),
    SoftBurst(rays::SoftBurst),
    Awakening {
        size: f32,
    },
    TwinGlow {
        size: i32,
        satellite_size: i32,
        angles: [f32; 2],
    },
    Shafts(rays::Shafts),
    Convergence(rays::Convergence),
    Rising(rays::Rising),
    RisingOrbs(rays::RisingOrbs),
    Bloom(rays::Bloom),
    Crown {
        palette: u16,
        radius: f32,
        spread: f32,
    },
    Aura {
        palette: i32,
        offset: f32,
    },
    Fire {
        size: f32,
        smoke: bool,
    },
    Glow {
        angle: f32,
        palette: i32,
        size: i32,
        retire_with_emitter: bool,
    },
    Portal {
        palette: i32,
        size: f32,
    },
    Charge {
        velocity: Option<[f32; 3]>,
        palette: i32,
        radius: i32,
        updates: i32,
        target: [f32; 3],
        travelling: bool,
    },
    Scatter(scatter::Scatter),
    Cloud(cloud::Cloud),
    Orbiting(orbiting::Orbiting),
    Cylinder(planes::Cylinder),
    Sheet(planes::Sheet),
    Travel {
        flight: Option<Flight>,
        texture: Option<(u32, u8)>,
        palette: i32,
        size: i32,
        burst_size: i32,
        fade: i32,
        target: [f32; 3],
        afterimages: bool,
    },
    Projectile {
        completed_phase: Option<u8>,
        color: ProjectileColor,
        path: Option<mote::Path>,
        launch: [f32; 3],
        curvature: i32,
        size: f32,
        target: [f32; 3],
        texture: Option<(u32, u8)>,
    },
    Fireball {
        velocity: Option<[f32; 3]>,
        size: f32,
        updates: u32,
        target: [f32; 3],
    },
    Quake,
    Inward {
        palette: i32,
        radius: i32,
        count: i32,
        curve: f32,
        size: i32,
        clear: i32,
        blend: i32,
    },
    Cardinal {
        angle: f32,
        count: i32,
    },
}

#[derive(Debug, Clone)]
enum ProjectileColor {
    White,
    Violet,
    Selected(u16),
    Changing { fade: f32 },
}

#[derive(Debug, Clone)]
struct Flight {
    velocity: [f32; 3],
    remaining: u32,
    heading: f32,
}

impl Emitter {
    pub(crate) fn camera_offset(&self) -> Option<f32> {
        match &self.kind {
            Kind::AttachedTrail { offset, .. } | Kind::Aura { offset, .. } => Some(*offset),
            _ => None,
        }
    }
    pub(crate) fn clear_particles(&self) -> bool {
        match &self.kind {
            Kind::RisingOrbs(effect) => !effect.preserve_particles,
            Kind::Inward { clear, .. } => *clear == 1,
            Kind::Stream(stream::Stream::Flame { cleanup, .. }) => *cleanup,
            Kind::Cloud(cloud) => cloud.cleanup,
            _ => true,
        }
    }
    fn step(
        &mut self,
        (owner, center): (i32, [f32; 3]),
        actor: &mut Actor,
        born: u32,
        clock: u32,
        camera: [f32; 3],
        random: &mut u32,
        out: &mut Births,
    ) -> Result<(), String> {
        let speed = actor.movement_speed();
        let phase = &mut self.phase;
        let tick = self.age;
        match &mut self.kind {
            Kind::SoftBurst(burst) => {
                crate::world::random(random);
                if *phase == 0 {
                    burst.emit(center, born, camera, speed, random, out);
                    *phase = 1;
                }
            }
            Kind::Awakening { size } => {
                rays::awakening(phase, size, center, born, clock, random, out);
            }
            Kind::ChargedRay(ray) => {
                ray.emit(phase, (owner, center), born, clock, speed, random, out)?;
            }
            Kind::Cylinder(cylinder) => {
                crate::world::random(random);
                cylinder.emit(phase, center, born, clock, random, out);
            }
            Kind::Sheet(sheet) => {
                crate::world::random(random);
                sheet.emit(phase, center, born, clock, camera, out);
            }
            Kind::Flash { sparks, lifetime } => {
                crate::world::random(random);
                if *phase == 0 {
                    let mut flash = particle(center, born, 33, *lifetime);
                    flash.field_fog = false;
                    flash.size = [100.; 2];
                    flash.size_delta = 40.;
                    if *sparks {
                        flash.fade = Fade::Linear(0.);
                        out.push(flash.clone());
                        out.push(flash);
                        rays::flash_sparks(center, born, camera, random, out);
                    } else {
                        flash.rgba = [10, 200, 200, 255];
                        flash.blend = Some(crate::effect::Blend::Subtractive);
                        out.push(flash);
                    }
                    *phase = 1;
                }
            }
            Kind::LinearTrail {
                sprite,
                remaining,
                target,
                velocity,
            } => {
                crate::world::random(random);
                if *phase >= 2 {
                    return Ok(());
                }
                let velocity = velocity.get_or_insert_with(|| {
                    std::array::from_fn(|i| (target[i] - center[i]) / (*remaining).max(1) as f32)
                });
                let mut trail = sprite.clone();
                trail.position = center;
                trail.born = born;
                out.push(trail);
                if *remaining >= 0 {
                    actor.position = std::array::from_fn(|i| center[i] + velocity[i]);
                    *remaining -= 1;
                }
                *phase = if *remaining < 0 { 2 } else { 1 };
            }
            Kind::Burst(burst) => burst.emit(center, born, phase, random, out),
            Kind::RadialSpray { palette } => {
                rays::spray(center, born, camera, *palette, random, out)
            }
            Kind::Explosion(explosion) => {
                crate::world::random(random);
                if *phase == 0 {
                    explosion.emit(center, born, actor, random, out);
                    *phase = 1;
                }
            }
            Kind::TwinGlow {
                size,
                satellite_size,
                angles,
            } => {
                crate::world::random(random);
                let mut core = particle(center, born, 50, 2);
                core.owner = Some(owner);
                core.field_fog = false;
                core.size = [*size as f32; 2];
                core.rgba[3] = 100;
                core.fade = Fade::Linear(0.);
                out.push(core);
                if *phase == 0 {
                    for (color, radial, sign) in [(35, [1., 0., 0.], 1.), (34, [0., 1., 0.], -1.)] {
                        let radial = rotated(
                            rotated(radial, [0., 0., 1.], angles[0] * sign),
                            [0., 1., 0.],
                            angles[1] * sign,
                        );
                        let position =
                            std::array::from_fn(|i| center[i] + radial[i] * (*size / 5) as f32);
                        let mut glow = particle(position, born, color, 61);
                        glow.owner = Some(owner);
                        glow.field_fog = false;
                        glow.size = [if *satellite_size == 0 {
                            *size / 10
                        } else {
                            *satellite_size
                        } as f32; 2];
                        glow.rgba[3] = 160;
                        glow.fade = Fade::Linear(-4.);
                        out.push(glow);
                    }
                    angles[0] += 5.;
                    angles[1] += (crate::world::random(random) % 2) as f32;
                }
            }
            Kind::Ray {
                palette,
                size,
                target,
                velocity,
            } => {
                crate::world::random(random);
                let velocity = velocity.get_or_insert_with(|| {
                    normalized(std::array::from_fn(|i| target[i] - center[i])).map(|v| v * speed)
                });
                let mut glow = particle(center, born, *palette, 61);
                glow.field_fog = false;
                glow.size = [*size; 2];
                glow.size_delta = -0.75;
                glow.fade = Fade::Linear(-10.);
                glow.blend = Some(crate::effect::Blend::Additive);
                out.push(glow.clone());
                glow.recipe = crate::effect::ELECTRIC_ARC_SPRITE;
                glow.size = [*size * 0.75; 2];
                glow.position =
                    center.map(|v| v + 15. - (crate::world::random(random) % 30) as f32);
                glow.angular_velocity[2] = if crate::world::random(random).is_multiple_of(2) {
                    -100.
                } else {
                    100.
                };
                out.push(glow);
                actor.position = std::array::from_fn(|i| center[i] + velocity[i]);
                *phase = 1;
            }
            Kind::Cloud(cloud) => {
                cloud.emit((owner, center), born, clock, *phase, actor, random, out)
            }
            Kind::Orbiting(orbiting) => {
                orbiting.emit((owner, center), actor, phase, born, clock, random, out)?
            }
            Kind::ModelTrail {
                remaining,
                target,
                velocity,
            } => {
                crate::world::random(random);
                let velocity = velocity.get_or_insert_with(|| {
                    std::array::from_fn(|i| (target[i] - center[i]) / (*remaining).max(1) as f32)
                });
                if *remaining >= 0 {
                    actor.position = std::array::from_fn(|i| center[i] + velocity[i]);
                    *remaining -= 1;
                }
                if *phase < 2 {
                    out.push_model(crate::model_particle::ModelParticle::streak(
                        actor.resource,
                        actor.position,
                        velocity[1].atan2(velocity[0]).to_degrees(),
                    ));
                    *phase = if *remaining <= 0 { 2 } else { 1 };
                }
            }
            Kind::ChargedTrail {
                size,
                fade,
                target,
                flight,
            } => {
                crate::world::random(random);
                if *phase == 0 {
                    let mut glow = particle(center, born, palette(108, random), 87);
                    glow.size = [(10 + crate::world::random(random) % 10) as f32; 2];
                    glow.angular_velocity[2] = if crate::world::random(random).is_multiple_of(2) {
                        -3.
                    } else {
                        3.
                    };
                    glow.blend = Some(crate::effect::Blend::Subtractive);
                    let mut direction = [1.; 3];
                    for axis in [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]] {
                        direction =
                            rotated(direction, axis, (crate::world::random(random) % 360) as f32);
                    }
                    glow.position = std::array::from_fn(|i| center[i] + direction[i] * 100.);
                    glow.velocity = normalized(direction).map(|v| -v * 2.);
                    out.push(glow);
                } else if *phase < 3 {
                    if speed <= 0. {
                        return Err("charged trail needs positive flight speed".into());
                    }
                    let flight = flight.get_or_insert_with(|| {
                        let delta = std::array::from_fn(|i| target[i] - center[i]);
                        Flight {
                            velocity: normalized(delta).map(|v| v * speed),
                            remaining: (delta.iter().map(|v| v * v).sum::<f32>().sqrt() / speed)
                                as u32,
                            heading: 0.,
                        }
                    });
                    let mut glow = particle(center, born, palette(108, random), 61);
                    glow.size = [*size; 2];
                    glow.fade = Fade::Linear(*fade);
                    glow.size_delta = -10.;
                    glow.blend = Some(crate::effect::Blend::Subtractive);
                    out.push(glow);
                    *phase = if flight.remaining == 0 { 3 } else { 2 };
                    if flight.remaining > 0 {
                        actor.position = std::array::from_fn(|i| center[i] + flight.velocity[i]);
                        flight.remaining -= 1;
                    }
                }
            }
            Kind::Mote(mote) => return mote.emit(actor, born, random, out),
            Kind::Gathering { state, .. } => {
                state.emit(center, born, clock, random, out);
                return Ok(());
            }
            Kind::Seal(seal) => {
                seal.emit(center, born, clock, random, out);
                return Ok(());
            }
            Kind::Column(column) => {
                column.emit(center, actor, born, random, out);
                return Ok(());
            }
            Kind::Contract(contract) => {
                contract.emit(center, actor, born, clock, random, out);
                return Ok(());
            }
            Kind::Fire { size, smoke, .. } => {
                crate::world::random(random);
                fire::emit(center, born, clock, *size, *smoke, random, out);
            }
            Kind::Projectile {
                completed_phase,
                color,
                path,
                launch,
                curvature,
                size,
                target,
                texture,
                ..
            } => {
                crate::world::random(random);
                let through = matches!(color, ProjectileColor::White);
                if !through && completed_phase.is_some_and(|done| *phase >= done) {
                    return Ok(());
                }
                if speed <= 0. {
                    return Err("projectile needs positive speed".into());
                }
                let path = path.get_or_insert_with(|| {
                    mote::Path::new(center, *target, *launch, speed, *curvature)
                });
                let emitting = !path.arrived();
                actor.position = path.advance();
                if emitting {
                    let (color, fade) = match color {
                        ProjectileColor::White => (33, -10.),
                        ProjectileColor::Violet => (65, -10.),
                        ProjectileColor::Selected(color) => (*color, -10.),
                        ProjectileColor::Changing { fade } => (palette(108, random), *fade),
                    };
                    let mut trail = particle(actor.position, born, color, 61);
                    trail.field_fog = false;
                    trail.size = [*size; 2];
                    trail.fade = Fade::Linear(fade);
                    out.push(trail);
                }
                if path.arrived() {
                    if through && emitting {
                        rays::impact_spheres(actor.position, born, texture.unwrap(), random, out);
                    } else if !through && let Some(texture) = texture {
                        out.push(arrival_flash(
                            actor.position,
                            born,
                            path.impact_rotation(),
                            *texture,
                        ));
                    }
                    *phase = completed_phase.unwrap_or(1);
                } else {
                    *phase = 1;
                }
            }
            Kind::Fireball {
                size,
                updates,
                target,
                velocity,
                ..
            } => {
                // Advance the emitter's shared random phase before its sparks.
                crate::world::random(random);
                if *phase >= 2 || *updates == 0 {
                    return Ok(());
                }
                let velocity = velocity.get_or_insert_with(|| {
                    std::array::from_fn(|i| (target[i] - center[i]) / *updates as f32)
                });
                for i in 0..3 {
                    actor.position[i] += velocity[i];
                }
                let core = BillboardEffect {
                    recipe: crate::effect::STATION_GLOW_SPRITE,
                    born,
                    lifetime: 2,
                    position: actor.position,
                    size: [*size * 0.4; 2],
                    rgba: [63, 32, 16, 255],
                    ..Default::default()
                };
                out.push(core);
                for _ in 0..2 {
                    let green = if crate::world::random(random).is_multiple_of(2) {
                        16
                    } else {
                        32
                    };
                    let position = std::array::from_fn(|i| {
                        actor.position[i] + 10. - (crate::world::random(random) % 21) as f32
                    });
                    let diameter = *size + (crate::world::random(random) % 16) as f32;
                    let angle = (crate::world::random(random) % 360) as f32;
                    out.push(BillboardEffect {
                        recipe: crate::effect::GLOW_SPRITE,
                        blend: Some(crate::effect::Blend::Additive),
                        born,
                        lifetime: 59,
                        position,
                        size: [diameter; 2],
                        rotation: [0., 0., angle],
                        rgba: [63, green, 16, 175],
                        fade: Fade::Linear(-15.),
                        ..Default::default()
                    });
                }
                *updates -= 1;
                *phase = if *updates == 0 { 2 } else { 1 };
            }
            Kind::Shafts(shafts) => {
                if clock.is_multiple_of(shafts.interval) {
                    shafts.emit(center, born, random, out);
                } else {
                    crate::world::random(random);
                }
            }
            Kind::Convergence(burst) => {
                crate::world::random(random);
                if *phase == 0 {
                    burst.emit(center, born, random, out);
                    *phase = 1;
                }
            }
            Kind::Rising(lights) => lights.emit(center, born, clock, camera, random, out),
            Kind::RisingOrbs(orbs) => {
                if let Some(target) = orbs.destination
                    && *phase != 0
                {
                    crate::world::random(random);
                    if *phase == 1 {
                        out.release = Some(Release::Toward(owner, target));
                        *phase = 2;
                    }
                } else {
                    orbs.emit(center, born, clock, (owner, actor), random, out);
                }
            }
            Kind::Aura {
                palette, offset, ..
            } => {
                rays::aura(center, born, tick, camera, *palette, *offset, random, out);
            }
            Kind::Bloom(bloom) => {
                crate::world::random(random);
                if *phase == 0 {
                    bloom.emit(center, born, speed, random, out);
                    *phase = 1;
                }
            }
            Kind::Crown {
                palette,
                radius,
                spread,
                ..
            } => {
                crate::world::random(random);
                if *phase == 0 {
                    rays::crown(center, born, *palette, *radius, *spread, random, out);
                    *phase = 1;
                }
            }
            Kind::Stream(stream) => {
                stream.particles((owner, center), actor, born, clock, random, out);
            }
            Kind::TwinTrail { sprite, radius, .. } => {
                crate::world::random(random);
                if tick > 0 && *phase < 2 {
                    let (sin, cos) = (tick as f32 * speed / 100.).to_radians().sin_cos();
                    for side in [1., -1.] {
                        let mut p = sprite.clone();
                        p.position = center;
                        p.born = born;
                        p.palette = p.palette.map(|color| palette(i32::from(color), random));
                        p.position[0] += *radius * cos;
                        p.position[1] += *radius * sin * side;
                        out.push(p);
                    }
                }
            }
            Kind::AttachedTrail {
                sprite,
                offset,
                interval,
                ..
            } => {
                crate::world::random(random);
                *phase = (*phase).max(1);
                if clock.is_multiple_of(*interval) {
                    let mut p = sprite.clone();
                    p.position = center;
                    p.born = born;
                    p.palette = p.palette.map(|color| palette(i32::from(color), random));
                    if matches!(p.orientation, SpriteOrientation::World) {
                        p.rotation =
                            std::array::from_fn(|_| (crate::world::random(random) % 360) as f32);
                    }
                    p.controller = Some(BillboardController::CameraOffset {
                        emitter: Some((owner, actor.instance)),
                        center,
                        distance: *offset,
                    });
                    inherit(&mut p, actor);
                    p.step();
                    out.push(p);
                }
            }
            Kind::Travel {
                texture,
                palette: color,
                size,
                burst_size,
                fade,
                target,
                afterimages,
                flight,
            } => {
                const TRAVEL: u8 = 1;
                const DONE: u8 = 3;
                crate::world::random(random);
                if *phase >= DONE {
                    return Ok(());
                }
                if speed <= 0. {
                    return Err("travelling effect needs positive speed".into());
                }
                if flight.is_none() && *afterimages {
                    travel_burst(center, born, *color as u16, *burst_size as f32, random, out);
                }
                let flight = flight.get_or_insert_with(|| {
                    let delta = std::array::from_fn(|i| target[i] - center[i]);
                    let distance = delta.iter().map(|v| v * v).sum::<f32>().sqrt();
                    Flight {
                        velocity: normalized(delta).map(|v| v * speed),
                        remaining: (distance / speed) as u32,
                        heading: delta[0].atan2(-delta[1]).to_degrees(),
                    }
                });
                let mut glow = particle(actor.position, born, *color as u16, 61);
                glow.size = [*size as f32; 2];
                glow.size_delta = -0.75;
                glow.fade = Fade::Linear(*fade as f32);
                out.push(glow);
                let arrived = flight.remaining == 0;
                if !arrived {
                    for i in 0..3 {
                        actor.position[i] += flight.velocity[i];
                    }
                }
                if *afterimages && flight.remaining.is_multiple_of(3) && actor.resource != 0 {
                    out.push_model(crate::model_particle::ModelParticle::afterimage(
                        actor.resource,
                        actor.position,
                        flight.heading,
                    ));
                }
                if arrived {
                    if *afterimages {
                        travel_burst(center, born, *color as u16, *burst_size as f32, random, out);
                    } else if let Some(texture) = texture {
                        out.push(arrival_flash(
                            actor.position,
                            born,
                            -flight.heading,
                            *texture,
                        ));
                    }
                    *phase = DONE;
                } else {
                    flight.remaining -= 1;
                    *phase = TRAVEL;
                }
            }
            Kind::Glow {
                palette,
                size,
                retire_with_emitter,
                angle,
                ..
            } => {
                let mut glow = particle(center, born, *palette as u16, 2);
                glow.size = [*size as f32; 2];
                glow.rgba[3] = 200;
                glow.fade = Fade::Linear(0.);
                out.push(glow);
                let (sin, cos) = (tick as f32 * 5.).to_radians().sin_cos();
                let (tilt_sin, tilt_cos) = angle.to_radians().sin_cos();
                crate::world::random(random);
                for offset in [
                    [cos * tilt_cos, sin, -cos * tilt_sin],
                    [sin * tilt_cos, cos, sin * tilt_sin],
                ] {
                    let color = 65 + crate::world::random(random) % 39;
                    let mut mote = particle(center, born, color as u16, 61);
                    mote.owner = retire_with_emitter.then_some(owner);
                    mote.position =
                        std::array::from_fn(|i| center[i] + offset[i] * (*size / 5) as f32);
                    mote.size = [(*size / 10) as f32; 2];
                    mote.rgba[3] = 160;
                    mote.fade = Fade::Linear(-4.);
                    out.push(mote);
                }
                *angle += (crate::world::random(random) % 2) as f32;
            }
            Kind::Portal { palette, size, .. } => {
                crate::world::random(random);
                let mut emit = |recipe, diameter, alpha, rotation, orientation| {
                    let mut p = particle(center, born, *palette as u16, 2);
                    p.recipe = recipe;
                    p.size = [diameter; 2];
                    p.rgba[3] = alpha;
                    p.rotation = rotation;
                    p.orientation = orientation;
                    p.fade = Fade::Linear(0.);
                    out.push(p);
                };
                emit(
                    crate::effect::ORB_SPRITE,
                    *size - 30.,
                    200,
                    [0.; 3],
                    SpriteOrientation::Camera,
                );
                for direction in [1., -1.] {
                    emit(
                        crate::effect::STAR_SPRITE,
                        *size + 5.,
                        40,
                        [0., 0., tick as f32 * 4. * direction],
                        SpriteOrientation::Camera,
                    );
                }
                for axis in [2, 1] {
                    for tilt in [-45., 45.] {
                        let mut rotation = [if axis == 2 { 90. } else { 0. }, 0., 0.];
                        rotation[axis] = tilt + tick as f32 * 3.;
                        emit(
                            crate::effect::WORLD_GLOW_SPRITE,
                            *size + 35.,
                            40,
                            rotation,
                            SpriteOrientation::World,
                        );
                    }
                }
            }
            Kind::Charge {
                palette: color,
                radius,
                updates,
                target,
                travelling,
                velocity,
                ..
            } => {
                crate::world::random(random);
                let motion = velocity.get_or_insert_with(|| {
                    std::array::from_fn(|i| (target[i] - center[i]) / (*updates).max(1) as f32)
                });
                if *updates >= 0 {
                    for i in 0..3 {
                        actor.position[i] += motion[i];
                    }
                    *updates -= 1;
                }
                if *phase >= 2 {
                    return Ok(());
                }
                let spin = if clock.is_multiple_of(2) { -3. } else { 3. };
                for (recipe, diameter, variation, lifetime, alpha, rotation_speed, palette) in [
                    (crate::effect::ORB_SPRITE, *radius, 16, 2, 255, spin, None),
                    (
                        crate::effect::ORB_SPRITE,
                        *radius * 2,
                        16,
                        4,
                        224,
                        0.,
                        Some(*color as u16),
                    ),
                    (
                        crate::effect::ELECTRIC_ARC_SPRITE,
                        *radius / 2 * 5,
                        32,
                        9,
                        224,
                        spin,
                        Some(*color as u16),
                    ),
                ]
                .into_iter()
                .take(if clock.is_multiple_of(2) { 3 } else { 2 })
                {
                    let size = diameter as f32 + (crate::world::random(random) % variation) as f32;
                    let angle = (crate::world::random(random) % 256) as f32;
                    out.push(BillboardEffect {
                        recipe,
                        born,
                        lifetime,
                        palette,
                        owner: (!*travelling).then_some(owner),
                        position: actor.position,
                        size: [size; 2],
                        size_delta: -3.,
                        rotation: [0., 0., angle],
                        angular_velocity: [0., 0., rotation_speed],
                        rgba: [64, 64, 64, alpha],
                        fade: Fade::tail(lifetime),
                        ..Default::default()
                    });
                }
                *phase = if *travelling && *updates <= 0 { 2 } else { 1 };
            }
            Kind::Scatter(scatter) => {
                scatter.emit(center, born, clock, speed / 10., *phase == 1, random, out);
            }
            Kind::Cardinal {
                count, angle: turn, ..
            } => {
                const TURN_PER_BATCH: f32 = 30.;
                crate::world::random(random);
                if *phase == 0 && clock.is_multiple_of(2) {
                    for (color, angle) in [(101, 0.), (77, 180.), (93, 90.), (32, 270.)]
                        .into_iter()
                        .take(*count as usize)
                    {
                        let dark = color == 32;
                        let color = color
                            + if dark {
                                0
                            } else {
                                (crate::world::random(random) % 4) as u16
                            };
                        let mut mote = particle(center, born, color, 181);
                        mote.field_fog = false;
                        mote.rgba[3] = 200;
                        mote.fade = Fade::tail(180);
                        mote.blend = dark.then_some(crate::effect::Blend::Alpha);
                        mote.size = [25.; 2];
                        let mut orbit = Orbit::new(
                            center,
                            [0., 0., 1.],
                            50.,
                            0.,
                            angle + 3. - *turn - actor.heading,
                            3.,
                        );
                        orbit.rise = 8.;
                        mote.controller = Some(BillboardController::Orbit(orbit));
                        out.push(mote);
                    }
                    *turn += TURN_PER_BATCH;
                }
            }
            Kind::Inward {
                palette: color,
                radius,
                count,
                curve,
                size,
                clear: _,
                blend,
                ..
            } => {
                const CONVERGE: u8 = 0;
                const WAIT: u8 = 1;
                const BURST: u8 = 2;
                const DONE: u8 = 3;
                crate::world::random(random);
                if *phase == CONVERGE {
                    if speed < 0. {
                        return Err("inward effect needs nonnegative speed".into());
                    }
                    let duration = if speed == 0. {
                        0
                    } else {
                        (*radius as f32 / speed) as u32
                    };
                    let axis = normalized(camera);
                    for index in 0..*count {
                        let mut mote =
                            particle(center, born, palette(*color, random), duration + 1);
                        // The leading orb stays upright; its afterimages spin independently.
                        crate::world::random(random);
                        mote.owner = Some(owner);
                        mote.owner_tail = crate::effect::OwnerTail::Fade(40);
                        mote.recipe = crate::effect::ORB_SPRITE;
                        mote.size = [*size as f32; 2];
                        mote.fade = Fade::Linear(0.);
                        mote.blend = (*blend == 1).then_some(crate::effect::Blend::Alpha);
                        let mut orbit = Orbit::new(
                            center,
                            axis,
                            *radius as f32,
                            -speed,
                            -index as f32 * 360. / *count as f32 - *curve,
                            -*curve,
                        );
                        orbit.radial = [axis[1], -axis[0], -axis[2]];
                        orbit.trail_palette = Some(*color);
                        mote.controller = Some(BillboardController::Orbit(orbit));
                        out.push(mote);
                    }
                    *phase = WAIT;
                } else if *phase == BURST {
                    inward_release(center, born, *color, owner, *blend == 1, random, out);
                    *phase = DONE;
                }
            }
            Kind::Quake => {
                crate::world::random(random);
                const COLUMN: u8 = 1;
                const SHAKE: u8 = 2;
                const DONE: u8 = 3;
                const COLUMN_TICKS: u32 = 20;
                const SHAKE_TICKS: u32 = 120;
                if (COLUMN_TICKS - 1..COLUMN_TICKS + SHAKE_TICKS).contains(&tick) {
                    out.shake = Some((COLUMN_TICKS + SHAKE_TICKS - tick - 1) as f32 / 10.);
                }
                if tick < COLUMN_TICKS {
                    let mut p = particle(center, born, 30, 21);
                    p.recipe = crate::effect::RING_SPRITE;
                    p.orientation = SpriteOrientation::World;
                    p.position[2] += (COLUMN_TICKS - tick) as f32 * 6.;
                    p.size = [150.; 2];
                    p.rgba[3] = (135 + 6 * tick) as u8;
                    out.push(p);
                    if tick + 1 == COLUMN_TICKS {
                        out.push_refraction(RefractionPulse {
                            draw_order: 0,
                            operation: None,
                            owner: None,
                            image: RefractionImage::Ripple,
                            palette: 0,
                            orientation: SpriteOrientation::World,
                            angular_velocity: [0.; 3],
                            rotation: [0., 0., 90.],
                            rotation_order: Default::default(),
                            position: [center[0], center[1], center[2] + 4.],
                            velocity: [0.; 3],
                            speed: 0.,
                            normalize_velocity: false,
                            born,
                            lifetime: 41,
                            size: 30.,
                            growth: 20.,
                            alpha: 192.,
                            fade: Fade::tail(41),
                        });
                    }
                    *phase = COLUMN;
                } else if tick <= COLUMN_TICKS + SHAKE_TICKS {
                    *phase = if tick == COLUMN_TICKS + SHAKE_TICKS {
                        DONE
                    } else {
                        SHAKE
                    };
                }
            }
        }
        self.age = self.age.saturating_add(1);
        Ok(())
    }
}

enum Birth {
    Sprite(BillboardEffect, [Option<u8>; 3]),
    Model(crate::model_particle::ModelParticle),
    Refraction(RefractionPulse),
}

#[derive(Default)]
struct Births {
    items: Vec<Birth>,
    shake: Option<f32>,
    release: Option<Release>,
}
#[derive(Clone, Copy)]
enum Release {
    Guided(i32),
    Toward(i32, [f32; 3]),
}
impl Births {
    fn push(&mut self, sprite: BillboardEffect) {
        self.items.push(Birth::Sprite(sprite, [None; 3]));
    }
    fn push_tinted(&mut self, sprite: BillboardEffect, tint: [Option<u8>; 3]) {
        self.items.push(Birth::Sprite(sprite, tint));
    }
    fn push_model(&mut self, model: crate::model_particle::ModelParticle) {
        self.items.push(Birth::Model(model));
    }
    fn push_refraction(&mut self, pulse: RefractionPulse) {
        self.items.push(Birth::Refraction(pulse));
    }
}
impl GameWorld {
    pub(crate) fn step_emitter(
        &mut self,
        id: i32,
        resources: &crate::ResourceLibrary,
    ) -> Result<(), String> {
        let mut output = Births::default();
        let camera = self.field_camera.as_ref().map_or([0., -1., 0.], |c| {
            std::array::from_fn(|i| c.position[i] - c.target[i])
        });
        {
            let Some(actor) = self
                .actors
                .get(&id)
                .filter(|actor| actor.emitter.is_some() && !actor.appearance.model_hidden)
            else {
                return Ok(());
            };
            let center = if actor.attachment.is_some() {
                self.attached_position(resources, id)
                    .map_err(|e| e.to_string())?
            } else {
                actor.position
            };
            let actor = self.actors.get_mut(&id).unwrap();
            let mut emitter = actor.emitter.take().unwrap();
            let result = emitter.step(
                (id, center),
                actor,
                self.tick,
                self.effect_tick,
                camera,
                &mut self.random_state,
                &mut output,
            );
            actor.emitter = Some(emitter);
            result?;
        }
        let direction = normalized([camera[0], camera[1], 0.]);
        for birth in output.items {
            match birth {
                Birth::Sprite(mut p, tint) => {
                    match &p.controller {
                        Some(BillboardController::Stretch(growth)) => {
                            for (size, growth) in p.size.iter_mut().zip(growth) {
                                *size += *growth;
                            }
                            p.step();
                        }
                        Some(BillboardController::Orbit(orbit)) => p.position = orbit.position(0),
                        Some(BillboardController::CameraOffset {
                            center, distance, ..
                        }) => {
                            p.position =
                                std::array::from_fn(|i| center[i] - direction[i] * distance);
                        }
                        _ => {}
                    }
                    let id = self.emit_billboard(p)?;
                    for (channel, color) in self.billboards.get_mut(&id).unwrap().rgba[..3]
                        .iter_mut()
                        .zip(tint)
                    {
                        if let Some(color) = color {
                            *channel = color;
                        }
                    }
                }
                Birth::Model(p) => {
                    self.emit_model_particle(p, self.tick)?;
                }
                Birth::Refraction(p) => {
                    self.emit_refraction(p)?;
                }
            }
        }
        if let Some(release) = output.release {
            let (Release::Guided(owner) | Release::Toward(owner, _)) = release;
            for p in self
                .billboards
                .values_mut()
                .filter(|p| p.owner == Some(owner))
            {
                match (release, &mut p.controller) {
                    (Release::Guided(_), Some(BillboardController::Guided(guided))) => {
                        guided.release()
                    }
                    (
                        Release::Toward(_, target),
                        Some(BillboardController::Drift { speed, .. }),
                    ) => {
                        if *speed <= 0. {
                            return Err("light convergence needs positive speed".into());
                        }
                        let delta = std::array::from_fn(|i| target[i] - p.position[i]);
                        p.velocity = normalized(delta).map(|v| v * *speed);
                        let lifetime =
                            (delta.iter().map(|v| v * v).sum::<f32>().sqrt() / *speed) as u32 + 1;
                        p.rgba[3] = p.alpha(self.tick).clamp(0., 255.) as u8;
                        p.born = self.tick;
                        p.lifetime = lifetime;
                        if matches!(p.fade, Fade::Tail { .. }) {
                            p.fade = Fade::tail(lifetime);
                        }
                        p.controller = None;
                    }
                    _ => {}
                }
            }
        }
        if let Some(amount) = output.shake {
            self.field_camera
                .get_or_insert_default()
                .shake
                // Expire the shake if this emitter stops updating.
                .configure(amount, 0, 1);
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Orbit {
    center: [f32; 3],
    axis: [f32; 3],
    radial: [f32; 3],
    radius: f32,
    radial_speed: f32,
    angle: f32,
    angular_speed: f32,
    rise: f32,
    pub trail_palette: Option<i32>,
}
impl Orbit {
    pub(crate) fn trail_origin(&self) -> [f32; 3] {
        let direction = rotated(self.radial, self.axis, self.angle - self.angular_speed);
        std::array::from_fn(|i| self.center[i] + direction[i] * self.radius)
    }
    fn new(
        center: [f32; 3],
        axis: [f32; 3],
        radius: f32,
        radial_speed: f32,
        angle: f32,
        angular_speed: f32,
    ) -> Self {
        let radial = normalized(if axis[2].abs() > 0.9 {
            [axis[2], 0., -axis[0]]
        } else {
            [-axis[1], axis[0], 0.]
        });
        Self {
            center,
            axis,
            radial,
            radius,
            radial_speed,
            angle,
            angular_speed,
            rise: 0.,
            trail_palette: None,
        }
    }
    pub fn position(&self, age: u32) -> [f32; 3] {
        let age = age as f32;
        let radial = rotated(
            self.radial,
            self.axis,
            self.angle + age * self.angular_speed,
        );
        let radius = (self.radius + age * self.radial_speed).max(0.);
        std::array::from_fn(|i| {
            self.center[i] + radial[i] * radius + if i == 2 { age * self.rise } else { 0. }
        })
    }
}
fn arrival_flash(
    position: [f32; 3],
    born: u32,
    rotation: f32,
    texture: (u32, u8),
) -> BillboardEffect {
    let mut flash = particle(position, born, 33, 16);
    flash.texture = Some(texture);
    flash.blend = Some(crate::effect::Blend::Additive);
    flash.uv = Some([0., 0., 254. / 256., 254. / 256.]);
    flash.orientation = SpriteOrientation::World;
    flash.rotation = [90., 0., rotation];
    flash.size = [0.; 2];
    flash.size_delta = 25.;
    flash.rgba[3] = 100;
    flash
}

fn travel_burst(
    center: [f32; 3],
    born: u32,
    color: u16,
    size: f32,
    random: &mut u32,
    out: &mut Births,
) {
    const PARTICLES: usize = 20;
    for _ in 0..PARTICLES {
        let direction = std::array::from_fn(|_| 50. - (crate::world::random(random) % 100) as f32);
        let mut p = particle(center, born, color, 61);
        p.velocity = normalized(direction).map(|v| v * 3.);
        p.size = [size; 2];
        p.rgba[3] = 200;
        out.push(p);
    }
}

fn inward_release(
    center: [f32; 3],
    born: u32,
    color: i32,
    owner: i32,
    alpha_blend: bool,
    random: &mut u32,
    output: &mut Births,
) {
    const SPARKS: usize = 250;
    const LIFETIME: u32 = 121;
    const SPARK_SIZE: f32 = 50.;
    const SPEED: f32 = 10.;
    const SPRITES: [u16; 3] = [
        crate::effect::ORB_SPRITE,
        crate::effect::SEAL_SPARK_SPRITE,
        crate::effect::TRAIL_GLOW_SPRITE,
    ];
    output.push_refraction(RefractionPulse {
        draw_order: 0,
        operation: None,
        owner: Some(owner),
        image: RefractionImage::Ripple,
        palette: 0,
        orientation: SpriteOrientation::Camera,
        angular_velocity: [0.; 3],
        rotation: [0.; 3],
        rotation_order: Default::default(),
        position: center,
        velocity: [0.; 3],
        speed: 0.,
        normalize_velocity: false,
        born,
        lifetime: LIFETIME,
        size: 0.,
        growth: 25.,
        alpha: 255.,
        fade: Fade::tail(LIFETIME),
    });
    let mut glow = particle(
        center,
        born,
        if color < 105 { color as u16 } else { 33 },
        LIFETIME,
    );
    glow.owner = Some(owner);
    glow.blend = alpha_blend.then_some(crate::effect::Blend::Alpha);
    glow.size_delta = 15.;
    output.push(glow);
    for _ in 0..SPARKS {
        let recipe = SPRITES[crate::world::random(random) as usize % SPRITES.len()];
        let mut spark = particle(center, born, palette(color, random), LIFETIME);
        spark.owner = Some(owner);
        spark.field_fog = false;
        spark.blend = alpha_blend.then_some(crate::effect::Blend::Alpha);
        spark.recipe = recipe;
        spark.angular_velocity[2] = if crate::world::random(random).is_multiple_of(2) {
            -3.
        } else {
            3.
        };
        let mut direction = [1.; 3];
        for axis in [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]] {
            direction = rotated(direction, axis, (crate::world::random(random) % 360) as f32);
        }
        spark.velocity = normalized(direction).map(|v| v * SPEED);
        spark.size = [SPARK_SIZE; 2];
        output.push(spark);
    }
}
pub(crate) fn palette(color: i32, random: &mut u32) -> u16 {
    const RANDOM_PALETTES: [u16; 7] = [101, 85, 73, 89, 77, 97, 93];
    if color < 105 {
        color as u16
    } else {
        RANDOM_PALETTES[crate::world::random(random) as usize % RANDOM_PALETTES.len()]
            + (color - 105) as u16
    }
}
fn rotated(v: [f32; 3], axis: [f32; 3], degrees: f32) -> [f32; 3] {
    let (sin, cos) = degrees.to_radians().sin_cos();
    let cross = [
        axis[1] * v[2] - axis[2] * v[1],
        axis[2] * v[0] - axis[0] * v[2],
        axis[0] * v[1] - axis[1] * v[0],
    ];
    let dot = axis.iter().zip(v).map(|(a, b)| a * b).sum::<f32>();
    std::array::from_fn(|i| v[i] * cos + cross[i] * sin + axis[i] * dot * (1. - cos))
}
fn inherit(particle: &mut BillboardEffect, actor: &Actor) {
    for (color, value) in particle.rgba[..3].iter_mut().zip(actor.tint) {
        if value != NEUTRAL_TINT {
            *color = value;
        }
    }
    if actor.opacity != 255 {
        particle.rgba[3] = actor.opacity;
    }
    if let Some(blend) = actor.blend {
        particle.blend = Some(blend);
    }
}

fn particle(position: [f32; 3], born: u32, palette: u16, lifetime: u32) -> BillboardEffect {
    BillboardEffect {
        recipe: crate::effect::ORB_SPRITE,
        palette: Some(palette),
        born,
        lifetime,
        position,
        fade: Fade::tail(lifetime),
        ..Default::default()
    }
}
