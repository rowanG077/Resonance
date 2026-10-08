//! Scene effects share particle births, analytic paths and bounded stage timing.
mod fire;
mod native;
mod native_stream;
mod rays;
pub(crate) mod scatter;
mod stream;
use crate::effect::emission::normalized;
use crate::{
    Actor, GameWorld,
    effect::{
        BillboardController, BillboardEffect, Fade, NEUTRAL_TINT, RefractionImage, RefractionPulse,
        SpriteOrientation, emission::Emission,
    },
};

pub(crate) const PHASE_PROPERTY: i32 = 33;
const BURST_PARTICLES: usize = 80;
const BURST_LIFETIME: u32 = 120;

#[derive(Debug, Clone)]
pub(crate) struct Emitter {
    config: Config,
    inputs: native::Inputs,
    state: State,
}
pub(crate) enum ParticleCleanup {
    Keep,
    Fade { updates: u32 },
    Remove,
}
#[derive(Debug, Clone, Default)]
struct State {
    stage: u8,
    age: u32,
    origin: Option<[f32; 3]>,
    angle: f32,
    emitted: u32,
}
#[derive(Debug, Clone)]
enum Config {
    Stream(Box<stream::Stream>),
    Shafts(rays::Shafts),
    Convergence(rays::Convergence),
    Rising(rays::Rising),
    RisingOrbs(rays::RisingOrbs),
    Bloom(rays::Bloom),
    Crown {
        palette: i32,
        radius: f32,
        spread: f32,
    },
    Aura {
        palette: i32,
        offset: f32,
    },
    Fire {
        size: f32,
    },
    Gathering {
        delay: i32,
    },
    Glow {
        palette: i32,
        size: i32,
        retire_with_emitter: bool,
    },
    Portal {
        palette: i32,
        size: f32,
    },
    Charge {
        palette: i32,
        radius: i32,
        updates: u32,
        target: [f32; 3],
        travelling: bool,
    },
    Scatter(scatter::Scatter),
    Travel {
        sprite: u16,
        palette: i32,
        size: i32,
        burst_size: i32,
        fade: i32,
        target: [i32; 3],
        curvature: f32,
        afterimages: bool,
    },
    Projectile {
        size: f32,
        fade: f32,
        target: [f32; 3],
        texture: Option<(u32, u8)>,
    },
    Fireball {
        size: f32,
        updates: u32,
        target: [f32; 3],
    },
    Quake,
    Column {
        palette: i32,
        size: i32,
        layers: i32,
        alpha: i32,
        lighting: i32,
        spacing: i32,
        life: i32,
        growth: i32,
        expands: bool,
    },
    Contract {
        palette: i32,
        radius: i32,
        life: i32,
        interval: i32,
        angular_step: i32,
        width: i32,
        height: i32,
        alpha: i32,
        fade: i32,
        growth: i32,
    },
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
        count: i32,
    },
    Seal {
        palette: i32,
        opening: i32,
        pulse: i32,
        spark: i32,
    },
}

impl Emitter {
    pub(crate) fn camera_offset(&self) -> Option<f32> {
        match &self.config {
            Config::Stream(stream) => stream.camera_offset,
            Config::Aura { offset, .. } => Some(*offset),
            _ => None,
        }
    }
    pub(crate) fn particle_cleanup(&self) -> ParticleCleanup {
        match &self.config {
            Config::RisingOrbs(orbs) if orbs.preserve_particles => ParticleCleanup::Keep,
            Config::Inward { clear, .. } if *clear != 1 => ParticleCleanup::Fade { updates: 40 },
            _ => ParticleCleanup::Remove,
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
        output: &mut Births,
    ) -> Result<(), String> {
        let tick = self.state.age;
        let mut stage = self.state.stage;
        let speed = actor.movement_speed();
        let out = &mut output.particles;
        match &self.config {
            Config::Fire { size } => {
                if stage < 2 {
                    fire::emit(center, born, clock, *size, random, out);
                }
            }
            Config::Projectile {
                size,
                fade,
                target,
                texture,
            } => {
                crate::world::random(random);
                if stage >= 3 {
                    return Ok(());
                }
                if speed <= 0. {
                    return Err("projectile needs positive speed".into());
                }
                let delta = std::array::from_fn(|i| target[i] - actor.position[i]);
                let distance = delta.iter().map(|v| v * v).sum::<f32>().sqrt();
                if distance > speed {
                    let step = normalized(delta).map(|v| v * speed);
                    for i in 0..3 {
                        actor.position[i] += step[i];
                    }
                    let mut trail = particle(actor.position, born, palette(108, random), 61);
                    trail.size = [*size; 2];
                    trail.fade = Fade::Linear(*fade);
                    out.push(trail);
                }
                let remaining = std::array::from_fn::<_, 3, _>(|i| target[i] - actor.position[i]);
                if remaining.iter().map(|v| v * v).sum::<f32>() <= speed * speed {
                    if let Some(texture) = texture {
                        out.push(BillboardEffect {
                            recipe: crate::effect::ORB_SPRITE,
                            texture: Some(*texture),
                            uv: Some([0., 0., 254. / 256., 254. / 256.]),
                            orientation: SpriteOrientation::World,
                            rotation: [90., 0., 0.],
                            born,
                            lifetime: 16,
                            position: actor.position,
                            size: [0.; 2],
                            size_delta: size / 2.,
                            rgba: [63, 63, 63, 100],
                            fade: Fade::tail(16),
                            ..Default::default()
                        });
                    }
                    stage = 3;
                }
            }
            Config::Fireball {
                size,
                updates,
                target,
            } => {
                // Advance the emitter's shared random phase before its sparks.
                crate::world::random(random);
                if tick >= *updates {
                    return Ok(());
                }
                let start = *self.state.origin.get_or_insert(center);
                for i in 0..3 {
                    actor.position[i] += (target[i] - start[i]) / *updates as f32;
                }
                let core = BillboardEffect {
                    owner: Some(owner),
                    recipe: crate::effect::STATION_GLOW_SPRITE,
                    born,
                    lifetime: 2,
                    position: actor.position,
                    size: [size * 0.4; 2],
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
                    let diameter = size + (crate::world::random(random) % 16) as f32;
                    let angle = (crate::world::random(random) % 360) as f32;
                    out.push(BillboardEffect {
                        owner: Some(owner),
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
                stage = 1;
            }
            Config::Shafts(shafts) => {
                if stage < 2 && clock.is_multiple_of(shafts.interval) {
                    shafts.emit(center, born, random, out);
                    stage = 1;
                } else {
                    crate::world::random(random);
                }
            }
            Config::Convergence(burst) => {
                crate::world::random(random);
                if stage == 0 {
                    burst.emit(center, born, random, out);
                    stage = 1;
                }
            }
            Config::Rising(lights) => lights.emit(center, born, camera, random, out),
            Config::RisingOrbs(orbs) => {
                orbs.emit(center, born, clock, (owner, actor), random, out);
            }
            Config::Aura { palette, offset } => {
                rays::aura(
                    center, born, tick, owner, camera, *palette, *offset, random, out,
                );
            }
            Config::Bloom(bloom) => {
                crate::world::random(random);
                if stage == 0 {
                    bloom.emit(center, born, speed, random, out);
                    stage = 1;
                }
            }
            Config::Crown {
                palette,
                radius,
                spread,
            } => {
                crate::world::random(random);
                if stage == 0 {
                    rays::crown(center, born, *palette, *radius, *spread, random, out);
                    stage = 1;
                }
            }
            Config::Stream(stream) => {
                const START: u8 = 0;
                const EMIT: u8 = 1;
                if matches!(stage, START | EMIT) {
                    stream.particles(
                        &mut self.state,
                        owner,
                        center,
                        actor,
                        born,
                        tick,
                        random,
                        out,
                    );
                    stage = EMIT;
                }
            }
            Config::Travel {
                sprite,
                palette: color,
                size,
                burst_size,
                fade,
                target,
                curvature,
                afterimages,
            } => {
                const TRAVEL: u8 = 1;
                const DONE: u8 = 3;
                if stage >= DONE {
                    return Ok(());
                }
                if speed <= 0. {
                    return Err("travelling effect needs positive speed".into());
                }
                let start = *self.state.origin.get_or_insert(center);
                let target = target.map(|v| v as f32);
                let delta: [f32; 3] = std::array::from_fn(|i| target[i] - start[i]);
                let distance = delta.iter().map(|v| v * v).sum::<f32>().sqrt();
                if !*afterimages && self.state.age > 0 && self.state.age as f32 * speed >= distance
                {
                    return Ok(());
                }
                let t = ((self.state.age + 1) as f32 * speed / distance.max(speed)).min(1.);
                actor.position = std::array::from_fn(|i| start[i] + delta[i] * t);
                actor.position[2] += distance * *curvature * t * (1. - t);
                let mut glow = particle(actor.position, born, *color as u16, 30);
                glow.recipe = *sprite;
                glow.size = [*size as f32; 2];
                glow.fade = Fade::Linear(*fade as f32);
                out.push(glow);
                if *afterimages {
                    if self.state.age == 0 || t == 1. {
                        burst(
                            actor.position,
                            born,
                            *color,
                            *burst_size as f32,
                            None,
                            random,
                            out,
                        );
                    }
                    if self.state.age.is_multiple_of(3) && actor.resource != 0 {
                        output
                            .models
                            .push(crate::model_particle::ModelParticle::afterimage(
                                actor.resource,
                                actor.position,
                                delta[0].atan2(-delta[1]).to_degrees(),
                            ));
                    }
                }
                stage = if t == 1. && *afterimages {
                    DONE
                } else {
                    stage.max(TRAVEL)
                };
            }
            Config::Gathering { delay } => {
                const GATHER: u8 = 0;
                const CHARGE: u8 = 1;
                const BURST: u8 = 2;
                const DONE: u8 = 3;
                let size = (self.state.age as f32 * 0.8).min(200.);
                if stage == CHARGE && self.state.age >= *delay as u32 {
                    stage = BURST;
                }
                if matches!(stage, GATHER | CHARGE) {
                    let mut glow = particle(center, born, 9, 2);
                    glow.size = [size; 2];
                    glow.rgba[3] = 100;
                    out.push(glow);
                    if stage == GATHER {
                        let direction = normalized(std::array::from_fn(|_| {
                            crate::world::random_unit(random) * 2. - 1.
                        }));
                        let mut star = particle(
                            std::array::from_fn(|i| center[i] + direction[i] * 100.),
                            born,
                            9,
                            50,
                        );
                        star.recipe = crate::effect::STAR_SPRITE;
                        star.size = [25.; 2];
                        star.velocity = direction.map(|v| -v * 2.);
                        out.push(star);
                    }
                } else if stage == BURST {
                    burst(center, born, 9, size.max(25.), None, random, out);
                    stage = DONE;
                }
            }
            Config::Glow {
                palette,
                size,
                retire_with_emitter,
            } => {
                let mut glow = particle(center, born, *palette as u16, 2);
                glow.size = [*size as f32; 2];
                glow.field_lighting = false;
                glow.rgba[3] = 200;
                glow.fade = Fade::Linear(0.);
                out.push(glow);
                let (sin, cos) = (tick as f32 * 5.).to_radians().sin_cos();
                let (tilt_sin, tilt_cos) = self.state.angle.to_radians().sin_cos();
                crate::world::random(random);
                for offset in [
                    [cos * tilt_cos, sin, -cos * tilt_sin],
                    [sin * tilt_cos, cos, sin * tilt_sin],
                ] {
                    let color = 65 + crate::world::random(random) % 39;
                    let mut mote = particle(center, born, color as u16, 60);
                    mote.owner = retire_with_emitter.then_some(owner);
                    mote.position =
                        std::array::from_fn(|i| center[i] + offset[i] * *size as f32 / 5.);
                    mote.size = [*size as f32 / 10.; 2];
                    mote.rgba[3] = 160;
                    mote.fade = Fade::Linear(-4.);
                    out.push(mote);
                }
                self.state.angle += (crate::world::random(random) % 2) as f32;
            }
            Config::Portal { palette, size } => {
                crate::world::random(random);
                let mut emit = |recipe, diameter, alpha, rotation, orientation| {
                    let mut p = particle(center, born, *palette as u16, 2);
                    p.recipe = recipe;
                    p.size = [diameter; 2];
                    p.rgba[3] = alpha;
                    p.rotation = rotation;
                    p.orientation = orientation;
                    p.field_lighting = false;
                    p.fade = Fade::Linear(0.);
                    out.push(p);
                };
                // A bright core, counter-rotating stars and two crossed pairs of discs.
                emit(
                    crate::effect::ORB_SPRITE,
                    size - 30.,
                    200,
                    [0.; 3],
                    SpriteOrientation::Camera,
                );
                for direction in [1., -1.] {
                    emit(
                        crate::effect::STAR_SPRITE,
                        size + 5.,
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
                            size + 35.,
                            40,
                            rotation,
                            SpriteOrientation::World,
                        );
                    }
                }
            }
            Config::Charge {
                palette: color,
                radius,
                updates,
                target,
                travelling,
            } => {
                crate::world::random(random);
                if stage >= 2 || (*travelling && tick >= *updates) {
                    return Ok(());
                }
                let start = *self.state.origin.get_or_insert(center);
                if *travelling {
                    for i in 0..3 {
                        actor.position[i] += (target[i] - start[i]) / *updates as f32;
                    }
                }
                let spin = if clock.is_multiple_of(2) { -3. } else { 3. };
                for (recipe, diameter, variation, lifetime, alpha, rotation_speed, palette) in [
                    (crate::effect::ORB_SPRITE, *radius, 16, 2, 255, spin, None),
                    (
                        crate::effect::ORB_SPRITE,
                        radius * 2,
                        16,
                        4,
                        224,
                        0.,
                        Some(*color as u16),
                    ),
                    (
                        crate::effect::ELECTRIC_ARC_SPRITE,
                        radius / 2 * 5,
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
                        owner: (!travelling).then_some(owner),
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
            }
            Config::Scatter(scatter) => {
                if stage != 1 {
                    scatter.emit(center, born, clock, speed / 10., random, out);
                }
            }
            Config::Cardinal { count } if stage == 0 => {
                const DONE: u8 = 1;
                for (index, color) in [101, 77, 93, 32]
                    .into_iter()
                    .take(*count as usize)
                    .enumerate()
                {
                    let mut mote = particle(center, born, color, 180);
                    mote.owner = Some(owner);
                    mote.size = [25.; 2];
                    let mut orbit = Orbit::new(
                        center,
                        [0., 0., 1.],
                        50.,
                        0.,
                        index as f32 * 90. - actor.heading,
                        3.,
                    );
                    orbit.rise = 8.;
                    mote.controller = Some(BillboardController::Orbit(orbit));
                    out.push(mote);
                }
                stage = DONE;
            }
            Config::Inward {
                palette: color,
                radius,
                count,
                curve,
                size,
                blend,
                ..
            } => {
                const CONVERGE: u8 = 0;
                const WAIT: u8 = 1;
                const BURST: u8 = 2;
                const DONE: u8 = 3;
                if stage == CONVERGE {
                    if speed <= 0. {
                        return Err("inward effect needs positive speed".into());
                    }
                    for index in 0..*count {
                        let mut mote = particle(
                            center,
                            born,
                            palette(*color, random),
                            (*radius as f32 / speed).max(1.) as u32,
                        );
                        mote.owner = Some(owner);
                        mote.recipe = crate::effect::ORB_SPRITE;
                        mote.size = [*size as f32; 2];
                        mote.blend = (*blend == 1).then_some(crate::effect::Blend::Alpha);
                        let mut orbit = Orbit::new(
                            center,
                            normalized(camera),
                            *radius as f32,
                            -speed,
                            index as f32 * 360. / *count as f32,
                            *curve,
                        );
                        orbit.trail = true;
                        mote.controller = Some(BillboardController::Orbit(orbit));
                        out.push(mote);
                    }
                    stage = WAIT;
                } else if stage == BURST {
                    burst(center, born, *color, *size as f32, Some(owner), random, out);
                    output.ripples.push(ripple(center, born, Some(owner)));
                    stage = DONE;
                }
            }
            Config::Seal {
                palette: color,
                opening,
                pulse,
                spark,
            } => {
                const OPEN: u8 = 0;
                const PULSE: u8 = 1;
                const SUSTAIN: u8 = 2;
                const CLOSE: u8 = 3;
                const DONE: u8 = 4;
                let (size, life, count) = match stage {
                    OPEN => (*opening as f32, 2, 0),
                    PULSE => {
                        stage = SUSTAIN;
                        (*pulse as f32, BURST_LIFETIME, BURST_PARTICLES)
                    }
                    SUSTAIN => (*pulse as f32, 30, 0),
                    CLOSE => {
                        stage = DONE;
                        (*pulse as f32 * 5., 300, 0)
                    }
                    _ => return Ok(()),
                };
                if count != 0 {
                    burst(
                        center,
                        born,
                        *color,
                        *spark as f32,
                        Some(owner),
                        random,
                        out,
                    );
                }
                let mut glow = particle(center, born, *color as u16, life);
                glow.owner = Some(owner);
                glow.size = [size; 2];
                glow.rgba[3] = 150;
                out.push(glow);
            }
            Config::Contract {
                palette: color,
                radius,
                life,
                interval,
                angular_step,
                width,
                height,
                alpha,
                fade,
                growth,
            } => {
                crate::world::random(random);
                const CONTRACT: u8 = 0;
                const WAIT: u8 = 1;
                const EXPAND: u8 = 2;
                const DONE: u8 = 3;
                if stage == CONTRACT && tick.is_multiple_of(*interval as u32) {
                    let radius = (*radius as f32 - tick as f32 * 4. / *interval as f32).max(0.);
                    const COLORS: [[u8; 3]; 4] =
                        [[64, 64, 64], [255, 64, 64], [64, 64, 255], [64, 255, 255]];
                    for (arm, rgb) in COLORS.into_iter().enumerate() {
                        self.state.angle += *angular_step as f32;
                        let angle = (self.state.angle + arm as f32 * 90.).to_radians();
                        let mut p = particle(actor.position, born, 0, *life as u32 + 1);
                        p.palette = None;
                        p.position[0] += angle.cos() * radius;
                        p.position[1] += angle.sin() * radius;
                        p.size = [*width as f32, *height as f32];
                        p.rgba = [rgb[0], rgb[1], rgb[2], *alpha as u8];
                        p.fade = Fade::Linear(*fade as f32);
                        out.push(p);
                        actor.position[2] += speed;
                    }
                    if radius == 0. {
                        stage = WAIT;
                    }
                } else if stage == EXPAND {
                    let mut sphere = particle(center, born, *color as u16, *life as u32 + 1);
                    sphere.recipe = crate::effect::STATION_GLOW_SPRITE;
                    sphere.size_delta = *growth as f32;
                    sphere.fade = Fade::Linear(-10.);
                    out.extend([sphere.clone(), sphere]);
                    stage = DONE;
                }
            }
            Config::Column {
                palette: color,
                size,
                layers,
                alpha,
                lighting,
                spacing,
                life,
                growth,
                expands,
            } => {
                const RELEASE: u8 = 1;
                const DONE: u8 = 2;
                const ACCELERATION: f32 = 1.15;
                crate::world::random(random);
                if stage >= DONE {
                    return Ok(());
                }
                let remaining = if stage == RELEASE && !*expands {
                    layers.saturating_sub(self.state.age as i32)
                } else {
                    *layers
                };
                let count = remaining + i32::from(stage == RELEASE && !*expands);
                for layer in 0..count.max(0) {
                    let released = stage == RELEASE && (*expands || layer == remaining);
                    let lifetime = if released {
                        if *expands { *life as u32 } else { 301 }
                    } else {
                        2
                    };
                    let mut p = particle(center, born, *color as u16, lifetime.max(1));
                    p.recipe = crate::effect::WORLD_GLOW_SPRITE;
                    p.orientation = SpriteOrientation::World;
                    p.position[2] += layer as f32 * *spacing as f32;
                    p.size = [*size as f32; 2];
                    p.rgba[3] = (if released && !*expands {
                        alpha * 3
                    } else {
                        *alpha
                    })
                    .clamp(0, 255) as u8;
                    p.field_lighting = *lighting & 1 != 0;
                    if released {
                        if *expands {
                            p.size_delta = *growth as f32;
                        } else {
                            p.position[2] += ACCELERATION;
                            p.velocity[2] = ACCELERATION;
                            p.controller = Some(BillboardController::Accelerate {
                                multiplier: ACCELERATION,
                            });
                        }
                    } else {
                        p.fade = Fade::Linear(0.);
                    }
                    inherit(&mut p, actor);
                    out.push(p);
                }
                if stage == RELEASE && (*expands || remaining <= 0) {
                    stage = DONE;
                }
            }
            Config::Quake => {
                const COLUMN: u8 = 1;
                const SHAKE: u8 = 2;
                const DONE: u8 = 3;
                const COLUMN_TICKS: u32 = 20;
                const SHAKE_TICKS: u32 = 120;
                if self.state.age < COLUMN_TICKS {
                    let mut p = particle(center, born, 30, 21);
                    p.recipe = crate::effect::RING_SPRITE;
                    p.orientation = SpriteOrientation::World;
                    p.position[2] += (COLUMN_TICKS - self.state.age) as f32 * 6.;
                    p.size = [150.; 2];
                    out.push(p);
                    stage = COLUMN;
                } else if self.state.age <= COLUMN_TICKS + SHAKE_TICKS {
                    if self.state.age == COLUMN_TICKS {
                        output.ripples.push(ripple(center, born, None));
                    }
                    output.shake = Some((COLUMN_TICKS + SHAKE_TICKS - self.state.age) as f32 / 10.);
                    stage = if self.state.age == COLUMN_TICKS + SHAKE_TICKS {
                        DONE
                    } else {
                        SHAKE
                    };
                }
            }
            Config::Cardinal { .. } => {}
        }
        self.state.stage = stage;
        self.state.age = self.state.age.saturating_add(1);
        Ok(())
    }
}

#[derive(Default)]
struct Births {
    particles: Vec<BillboardEffect>,
    models: Vec<crate::model_particle::ModelParticle>,
    ripples: Vec<RefractionPulse>,
    shake: Option<f32>,
}
impl GameWorld {
    pub(crate) fn step_emitters(
        &mut self,
        resources: &crate::ResourceLibrary,
    ) -> Result<(), String> {
        let mut output = Births::default();
        let camera = self.field_camera.as_ref().map_or([0., -1., 0.], |c| {
            std::array::from_fn(|i| c.position[i] - c.target[i])
        });
        for &id in &self.actor_order {
            let Some(actor) = self.actors.get(&id) else {
                continue;
            };
            if actor.emitter.is_none() {
                continue;
            }
            let center = if actor.attachment.is_some() {
                self.attached_position(resources, id)
                    .map_err(|e| e.to_string())?
            } else {
                actor.position
            };
            let Some(actor) = self
                .actors
                .get_mut(&id)
                .filter(|a| !a.appearance.model_hidden)
            else {
                continue;
            };
            let Some(mut emitter) = actor.emitter.take() else {
                continue;
            };
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
        for mut p in output.particles {
            match &p.controller {
                Some(BillboardController::Orbit(orbit)) => p.position = orbit.position(0),
                Some(BillboardController::CameraOffset {
                    center, distance, ..
                }) => p.position = std::array::from_fn(|i| center[i] - direction[i] * distance),
                _ => {}
            }
            self.emit_billboard(p)?;
        }
        for p in output.models {
            self.emit_model_particle(p)?;
        }
        for p in output.ripples {
            self.emit_refraction(p)?;
        }
        if let Some(amount) = output.shake {
            self.field_camera
                .get_or_insert_default()
                .shake
                .configure(amount, 0, 0);
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
    pub trail: bool,
}
impl Orbit {
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
            trail: false,
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
fn burst(
    center: [f32; 3],
    born: u32,
    color: i32,
    size: f32,
    owner: Option<i32>,
    random: &mut u32,
    out: &mut Vec<BillboardEffect>,
) {
    let mut p = particle(center, born, palette(color, random), BURST_LIFETIME);
    p.owner = owner;
    p.field_fog = false;
    p.size = [size; 2];
    Emission {
        particle: p,
        count: BURST_PARTICLES,
        spread: 0.,
        speed: 6.,
        size_variation: 4.,
    }
    .emit(random, out);
}
fn ripple(position: [f32; 3], born: u32, owner: Option<i32>) -> RefractionPulse {
    RefractionPulse {
        draw_order: 0,
        operation: None,
        owner,
        image: RefractionImage::Ripple,
        palette: 0,
        orientation: SpriteOrientation::Camera,
        rotation: [0.; 3],
        position,
        born,
        lifetime: BURST_LIFETIME,
        size: 1.,
        growth: 20.,
        alpha: 192.,
        fade: Fade::tail(BURST_LIFETIME),
    }
}
fn palette(color: i32, random: &mut u32) -> u16 {
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
        field_lighting: true,
        palette: Some(palette),
        born,
        lifetime,
        position,
        fade: Fade::tail(lifetime),
        ..Default::default()
    }
}
