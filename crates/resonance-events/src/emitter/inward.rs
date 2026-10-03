//! Inward trails, then a radial burst.
use crate::effect::{
    BillboardController, BillboardEffect, Fade, RefractionImage, RefractionPulse, SpriteOrientation,
};

parameters! { palette = 113, radius = 114, count = 115, curve_angle = 116, size = 117, clear_on_remove = 121, blend = 122 }

#[derive(Debug, Clone)]
pub(crate) struct Inward {
    parameters: Parameters,
    phase: u16,
}

impl Inward {
    pub(super) fn from_native(a: &[i32]) -> Result<Self, String> {
        let result = Self {
            parameters: Parameters::read(a),
            phase: 0,
        };
        result.validate()?;
        Ok(result)
    }

    fn validate(&self) -> Result<(), String> {
        if !(0..=108).contains(&self.parameters.palette) || self.parameters.radius < 0 {
            return Err("invalid inward-trail palette or radius".into());
        }
        Ok(())
    }

    pub(super) fn property(&mut self, property: i32, value: Option<i32>) -> Result<i32, String> {
        if property == super::PHASE_PROPERTY {
            return Ok(super::phase(&mut self.phase, value));
        }
        let previous = self.parameters.property(property, value)?;
        self.validate()?;
        Ok(previous)
    }

    pub(super) fn preserves_particles(&self) -> bool {
        self.parameters.clear_on_remove != 1
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn particles(
        &mut self,
        owner: i32,
        center: [f32; 3],
        camera: [f32; 3],
        speed: i32,
        born: u32,
        random: &mut u32,
        out: &mut Vec<BillboardEffect>,
    ) -> Result<Option<RefractionPulse>, String> {
        let p = self.parameters;
        if self.phase == 0 {
            if speed <= 0 {
                return Err("inward-trail speed must be positive".into());
            }
            let axis = normalized(camera);
            let radial = [-axis[1], axis[0], axis[2]];
            for i in 0..p.count {
                let direction = rotated(radial, axis, (360 / p.count * i) as f32);
                let position = std::array::from_fn(|j| center[j] + direction[j] * p.radius as f32);
                let mut particle = super::particle(
                    position,
                    born,
                    palette(p.palette, random),
                    (p.radius / speed) as u32 + 1,
                );
                particle.owner = Some(owner);
                particle.recipe = 10;
                particle.field_lighting = true;
                particle.size = [p.size as f32; 2];
                particle.fade = Fade::Linear(0.);
                particle.blend_mode = (p.blend == 1).then_some(0);
                particle.controller = Some(BillboardController::Inward(Motion {
                    center,
                    remaining: p.radius,
                    step: speed,
                    angle: (p.curve_angle / 100) as f32,
                    palette: p.palette,
                    next_position: None,
                }));
                out.push(particle);
            }
            self.phase = 1;
        }
        if self.phase != 2 {
            return Ok(None);
        }
        self.phase = 3;
        // Executable constants 8035C294/318/31C/298 are 25/15/50/10.
        let ripple = RefractionPulse {
            operation: None,
            owner: Some(owner),
            image: RefractionImage::Ripple,
            palette: 0,
            orientation: SpriteOrientation::Camera,
            rotation: [0.; 3],
            position: center,
            born,
            lifetime: 120,
            size: 0.,
            growth: 25.,
            alpha: 255.,
            fade: Fade::tail(121),
        };
        let mut glow = super::particle(
            center,
            born,
            if p.palette >= 105 {
                65
            } else {
                p.palette as u16
            },
            121,
        );
        glow.owner = Some(owner);
        glow.recipe = 10;
        glow.field_lighting = true;
        glow.size_delta = 15.;
        glow.blend_mode = (p.blend == 1).then_some(0);
        out.push(glow);
        for _ in 0..250 {
            let mut particle = trail(
                center,
                50.,
                p.palette,
                (p.blend == 1).then_some(0),
                born,
                random,
            );
            particle.owner = Some(owner);
            particle.lifetime = 121;
            // Clearing flags5C disables the configured -5 alpha step and
            // selects the ordinary tail fade; flag0 also disables field fog.
            particle.fade = Fade::tail(121);
            particle.field_fog = false;
            let mut direction = [1.; 3];
            for axis in [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]] {
                direction = rotated(direction, axis, (crate::world::random(random) % 360) as f32);
            }
            particle.velocity = normalized(direction).map(|v| v * 10.);
            out.push(particle);
        }
        Ok(Some(ripple))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Motion {
    center: [f32; 3],
    remaining: i32,
    step: i32,
    angle: f32,
    palette: i32,
    next_position: Option<[f32; 3]>,
}

impl Motion {
    fn next(&mut self, position: [f32; 3], camera: [f32; 3]) -> [f32; 3] {
        let radial = normalized(std::array::from_fn(|i| position[i] - self.center[i]));
        let direction = rotated(radial, normalized(camera), self.angle);
        let result = std::array::from_fn(|i| self.center[i] + direction[i] * self.remaining as f32);
        self.remaining -= self.step;
        result
    }
}

impl BillboardEffect {
    pub(crate) fn advance_inward_trail(
        &mut self,
        camera: [f32; 3],
        born: u32,
        random: &mut u32,
        out: &mut Vec<Self>,
    ) {
        let Some(BillboardController::Inward(motion)) = &mut self.controller else {
            return;
        };
        if let Some(position) = motion.next_position {
            self.position = position;
        } else {
            out.push(trail(
                self.position,
                self.size[0],
                motion.palette,
                self.blend_mode,
                born,
                random,
            ));
            self.position = motion.next(self.position, camera);
        }
        out.push(trail(
            self.position,
            self.size[0],
            motion.palette,
            self.blend_mode,
            born,
            random,
        ));
        motion.next_position = Some(motion.next(self.position, camera));
    }
}

fn trail(
    position: [f32; 3],
    size: f32,
    color: i32,
    blend: Option<u8>,
    born: u32,
    random: &mut u32,
) -> BillboardEffect {
    let recipe = [10, 69, 12][crate::world::random(random) as usize % 3];
    let mut particle = super::particle(position, born, palette(color, random), 61);
    particle.recipe = recipe;
    particle.field_lighting = true;
    particle.size = [size; 2];
    particle.fade = Fade::Linear(-10.);
    particle.angular_velocity[2] = if crate::world::random(random) & 1 != 0 {
        3.
    } else {
        -3.
    };
    particle.blend_mode = blend;
    particle
}

fn palette(color: i32, random: &mut u32) -> u16 {
    if color < 105 {
        color as u16
    } else {
        [101, 85, 73, 89, 77, 97, 93][crate::world::random(random) as usize % 7]
            + (color - 105) as u16
    }
}

fn normalized(v: [f32; 3]) -> [f32; 3] {
    let length = v.iter().map(|v| v * v).sum::<f32>().sqrt();
    v.map(|v| if length == 0. { 0. } else { v / length })
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
