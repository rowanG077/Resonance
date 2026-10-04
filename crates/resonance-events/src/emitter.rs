//! Scene effects share particle births, analytic paths and bounded stage timing.
mod native;
mod native_stream;
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
const SPEED_PROPERTY: i32 = 5;
const OPACITY: i32 = 8;
const TINT_RED: i32 = 42;
const BURST_PARTICLES: usize = 80;
const BURST_LIFETIME: u32 = 120;

#[derive(Debug, Clone)]
pub(crate) struct Emitter {
    kind: Kind,
    inputs: native::Inputs,
    stage: u8,
    age: u32,
    origin: Option<[f32; 3]>,
}
#[derive(Debug, Clone)]
enum Kind {
    Stream(Box<stream::Stream>),
    Gathering {
        delay: i32,
    },
    Charge {
        palette: i32,
        radius: i32,
    },
    Scatter {
        palette: i32,
        size: i32,
        variation: i32,
        mote_size: i32,
        mote_variation: i32,
        life: i32,
        mote_life: i32,
    },
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
        target: Option<[f32; 3]>,
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
    pub(crate) fn aim_at(&mut self, position: [f32; 3]) {
        if let Kind::Contract { target, .. } = &mut self.kind {
            *target = Some(position);
        }
    }
    pub(crate) fn camera_offset(&self) -> Option<f32> {
        if let Kind::Stream(stream) = &self.kind {
            stream.camera_offset()
        } else {
            None
        }
    }
    pub(crate) fn preserves_particles_on_despawn(&self) -> bool {
        match &self.kind {
            Kind::Stream(stream) => stream.preserves_particles(),
            Kind::Inward { clear, .. } => *clear != 1,
            _ => false,
        }
    }
    fn step(
        &mut self,
        owner: i32,
        actor: &mut Actor,
        born: u32,
        camera: [f32; 3],
        random: &mut u32,
        output: &mut Births,
    ) -> Result<(), String> {
        let tick = self.age;
        let mut stage = self.stage;
        let center = actor.position;
        let speed = actor.properties.get(&SPEED_PROPERTY).copied().unwrap_or(0) as f32;
        let out = &mut output.particles;
        match &mut self.kind {
            Kind::Stream(stream) => {
                const START: u8 = 0;
                const EMIT: u8 = 1;
                if matches!(stage, START | EMIT) {
                    stream.particles(
                        owner,
                        center,
                        camera,
                        &actor.properties,
                        actor.blend,
                        born,
                        tick,
                        random,
                        out,
                    )?;
                    stage = EMIT;
                }
            }
            Kind::Travel {
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
                let start = *self.origin.get_or_insert(center);
                let target = target.map(|v| v as f32);
                let delta: [f32; 3] = std::array::from_fn(|i| target[i] - start[i]);
                let distance = delta.iter().map(|v| v * v).sum::<f32>().sqrt();
                if !*afterimages && self.age > 0 && self.age as f32 * speed >= distance {
                    return Ok(());
                }
                let t = ((self.age + 1) as f32 * speed / distance.max(speed)).min(1.);
                actor.position = std::array::from_fn(|i| start[i] + delta[i] * t);
                actor.position[2] += distance * *curvature * t * (1. - t);
                let mut glow = particle(actor.position, born, *color as u16, 30);
                glow.recipe = *sprite;
                glow.size = [*size as f32; 2];
                glow.fade = Fade::Linear(*fade as f32);
                out.push(glow);
                if *afterimages {
                    if self.age == 0 || t == 1. {
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
                    if self.age.is_multiple_of(3) && actor.resource != 0 {
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
            Kind::Gathering { delay } => {
                const GATHER: u8 = 0;
                const CHARGE: u8 = 1;
                const BURST: u8 = 2;
                const DONE: u8 = 3;
                let size = (self.age as f32 * 0.8).min(200.);
                if stage == CHARGE && self.age >= *delay as u32 {
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
            Kind::Charge {
                palette: color,
                radius,
            } => {
                if stage < 2 && *radius > 0 {
                    let mut glow = particle(center, born, *color as u16, 1);
                    glow.owner = Some(owner);
                    glow.size = [*radius as f32 * 2.; 2];
                    glow.rgba[3] = 160;
                    out.push(glow.clone());
                    glow.recipe = crate::effect::ELECTRIC_ARC_SPRITE;
                    glow.size = [*radius as f32 * 3.; 2];
                    glow.rotation[2] = crate::world::random_unit(random) * 360.;
                    out.push(glow);
                }
            }
            Kind::Scatter {
                palette: color,
                size,
                variation,
                mote_size,
                mote_variation,
                life,
                mote_life,
            } if stage != 1 && tick.is_multiple_of(6) => {
                const SCATTER: u8 = 0;
                const ORBIT: u8 = 2;
                for (image, size, variation, life) in [
                    (crate::effect::STAR_SPRITE, *size, *variation, *life),
                    (
                        crate::effect::ORB_SPRITE,
                        *mote_size,
                        *mote_variation,
                        *mote_life,
                    ),
                ] {
                    let mut p = particle(center, born, *color as u16, life.clamp(1, 180) as u32);
                    p.owner = Some(owner);
                    p.recipe = image;
                    p.size = [size as f32 * 0.5; 2];
                    p.rgba[3] = 64;
                    p.fade = Fade::Proportional {
                        after: 0,
                        lifetime: p.lifetime,
                    };
                    p.velocity[2] = if stage == SCATTER { 0. } else { 0.8 };
                    p.blend_mode = actor.blend.map(|b| b as u8);
                    if stage == ORBIT {
                        p.controller = Some(BillboardController::Orbit(Orbit::new(
                            center,
                            [0., 1., 0.],
                            1.,
                            1.,
                            self.age as f32 * 4.,
                            1.,
                        )));
                    }
                    Emission {
                        particle: p,
                        count: 1,
                        spread: 0.,
                        speed: speed / 10.,
                        size_variation: variation as f32 * 0.5,
                    }
                    .emit(random, out);
                }
            }
            Kind::Cardinal { count } if stage == 0 => {
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
            Kind::Inward {
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
                        mote.blend_mode = (*blend == 1).then_some(0);
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
            Kind::Seal {
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
            Kind::Contract {
                target,
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
                let radius_initial = *radius as f32;
                const CONTRACT: u8 = 0;
                const WAIT: u8 = 1;
                const EXPAND: u8 = 2;
                const DONE: u8 = 3;
                if stage == CONTRACT && tick.is_multiple_of(*interval as u32) {
                    let radius = (*radius as f32 - self.age as f32 * 4. / *interval as f32).max(0.);
                    for arm in 0..4 {
                        let angle = (self.age as f32 * *angular_step as f32 + arm as f32 * 90.)
                            .to_radians();
                        let mut p = particle(center, born, *color as u16, *life as u32 + 1);
                        p.position[0] += angle.cos() * radius;
                        p.position[1] += angle.sin() * radius;
                        p.size = [*width as f32, *height as f32];
                        p.rgba[3] = *alpha as u8;
                        p.fade = Fade::Linear(*fade as f32);
                        out.push(p);
                    }
                    let start = *self.origin.get_or_insert(center);
                    if let Some(target) = target {
                        let progress = 1. - radius / radius_initial.max(1.);
                        actor.position =
                            std::array::from_fn(|i| start[i] + (target[i] - start[i]) * progress);
                    } else {
                        actor.position[2] += speed;
                    }
                    if radius == 0. {
                        stage = WAIT;
                    }
                } else if stage == EXPAND {
                    let mut p = particle(center, born, *color as u16, 30);
                    p.size = [*width as f32; 2];
                    p.rgba[3] = 96;
                    p.size_delta = *growth as f32 / 4.;
                    out.push(p);
                    stage = DONE;
                }
            }
            Kind::Column {
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
                if stage >= DONE {
                    return Ok(());
                }
                let remaining = if stage == RELEASE && !*expands {
                    layers.saturating_sub(self.age as i32)
                } else {
                    *layers
                };
                for layer in 0..remaining.max(0) {
                    let released = stage == RELEASE && (*expands || layer == remaining - 1);
                    let lifetime = if released {
                        if *expands { *life as u32 } else { 120 }
                    } else {
                        1
                    };
                    let mut p = particle(center, born, *color as u16, lifetime.max(1));
                    p.recipe = crate::effect::WORLD_GLOW_SPRITE;
                    p.orientation = SpriteOrientation::World;
                    p.position[2] += layer as f32 * *spacing as f32;
                    p.size = [*size as f32; 2];
                    p.rgba[3] = *alpha as u8;
                    p.field_lighting = *lighting & 1 != 0;
                    if released {
                        if *expands {
                            p.size_delta = *growth as f32;
                        } else {
                            p.velocity[2] = 5.;
                        }
                    }
                    inherit(&mut p, &actor.properties, actor.blend);
                    out.push(p);
                }
                if stage == RELEASE && (*expands || remaining <= 1) {
                    stage = DONE;
                }
            }
            Kind::Quake => {
                const COLUMN: u8 = 1;
                const SHAKE: u8 = 2;
                const DONE: u8 = 3;
                const COLUMN_TICKS: u32 = 20;
                const SHAKE_TICKS: u32 = 120;
                if self.age < COLUMN_TICKS {
                    let mut p = particle(center, born, 30, 21);
                    p.recipe = crate::effect::RING_SPRITE;
                    p.orientation = SpriteOrientation::World;
                    p.position[2] += (COLUMN_TICKS - self.age) as f32 * 6.;
                    p.size = [150.; 2];
                    out.push(p);
                    stage = COLUMN;
                } else if self.age <= COLUMN_TICKS + SHAKE_TICKS {
                    if self.age == COLUMN_TICKS {
                        output.ripples.push(ripple(center, born, None));
                    }
                    output.shake = Some((COLUMN_TICKS + SHAKE_TICKS - self.age) as f32 / 10.);
                    stage = if self.age == COLUMN_TICKS + SHAKE_TICKS {
                        DONE
                    } else {
                        SHAKE
                    };
                }
            }
            Kind::Cardinal { .. } | Kind::Scatter { .. } => {}
        }
        self.stage = stage;
        self.age = self.age.saturating_add(1);
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
    pub(crate) fn step_emitters(&mut self) -> Result<(), String> {
        let mut output = Births::default();
        let camera = self.field_camera.as_ref().map_or([0., -1., 0.], |c| {
            std::array::from_fn(|i| c.position[i] - c.target[i])
        });
        for &id in &self.actor_order {
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
                id,
                actor,
                self.tick,
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
                }) => p.position = std::array::from_fn(|i| center[i] + direction[i] * distance),
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
fn inherit(
    particle: &mut BillboardEffect,
    properties: &std::collections::BTreeMap<i32, i32>,
    blend: Option<crate::model_particle::Blend>,
) {
    for (channel, color) in particle.rgba[..3].iter_mut().enumerate() {
        if let Some(value) = properties.get(&(TINT_RED + channel as i32))
            && *value as u8 != NEUTRAL_TINT
        {
            *color = *value as u8;
        }
    }
    if let Some(value) = properties.get(&OPACITY)
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
