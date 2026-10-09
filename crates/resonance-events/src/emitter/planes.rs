use super::{Births, normalized, particle};
use crate::effect::{BillboardController, Blend, Fade, SpriteOrientation};
use resonance_content::effect::sprite::{
    CYLINDER_RAY_SPRITE, LIGHT_SHEET_SPRITE, STATION_GLOW_SPRITE,
};

#[derive(Debug, Clone)]
pub(super) struct Cylinder {
    pub palette: u16,
    pub textures: [(u32, u8); 2],
}
impl Cylinder {
    pub fn emit(
        &self,
        phase: &mut u8,
        center: [f32; 3],
        born: u32,
        clock: u32,
        rng: &mut u32,
        out: &mut Births,
    ) {
        let textured = |index, lifetime, size, alpha, growth| {
            let mut p = particle(center, born, self.palette, lifetime);
            p.texture = Some(self.textures[index]);
            p.uv = Some([0., 0., 254. / 256., 254. / 256.]);
            p.blend = Some(Blend::Additive);
            p.size = [size; 2];
            p.rgba[3] = alpha;
            p.size_delta = growth;
            p.angular_velocity[2] = 1.;
            p
        };
        if *phase == 0 {
            if clock.is_multiple_of(10) {
                for (index, size, growth) in [(0, 400., 15.), (1, 250., 5.)] {
                    let mut p = textured(index, 16, size, 150, growth);
                    p.orientation = SpriteOrientation::World;
                    out.push(p);
                }
            }
            if clock.is_multiple_of(5) {
                let angle = (crate::world::random(rng) % 360) as f32;
                let (sin, cos) = angle.to_radians().sin_cos();
                let mut p = particle(center, born, 94, 17);
                p.recipe = CYLINDER_RAY_SPRITE;
                p.orientation = SpriteOrientation::World;
                p.position[0] -= sin * 125.;
                p.position[1] += cos * 125.;
                p.size = [40., 80.];
                p.rgba[3] = 125;
                p.fade = Fade::Linear(0.);
                p.rotation[2] = angle;
                p.anchor = resonance_content::effect::VerticalAnchor::Bottom;
                p.angular_velocity[0] = 5.;
                p.controller = Some(BillboardController::Stretch([0., 8.]));
                out.push(p);
            }
        } else if *phase == 1 {
            let mut p = textured(1, 61, 250., 5, 0.);
            p.position[2] += 150.;
            p.fade = Fade::Linear(10.);
            out.push(p);
            let mut glow = particle(center, born, self.palette, 61);
            glow.recipe = STATION_GLOW_SPRITE;
            glow.position[2] += 150.;
            glow.size = [150.; 2];
            glow.size_delta = 5.;
            glow.rgba[3] = 50;
            glow.fade = Fade::Linear(1. / 16.);
            out.push(glow);
            *phase = 2;
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(super) struct Sheet {
    pub burst: bool,
    pub palette: u16,
    pub offset: f32,
    pub lifetime: u32,
    pub size: [f32; 2],
    pub alpha: u8,
    pub fade: f32,
    pub growth: f32,
    pub rate: u32,
}
impl Sheet {
    pub fn emit(
        &self,
        phase: &mut u8,
        center: [f32; 3],
        born: u32,
        clock: u32,
        camera: [f32; 3],
        out: &mut Births,
    ) {
        let count = if self.burst {
            if *phase != 0 {
                return;
            }
            *phase = 1;
            self.rate
        } else {
            u32::from(clock.is_multiple_of(self.rate.max(1)))
        };
        let direction = normalized([camera[0], camera[1], 0.]);
        let position = std::array::from_fn(|i| center[i] - direction[i] * self.offset);
        for _ in 0..count {
            let mut p = particle(position, born, self.palette, self.lifetime);
            p.recipe = LIGHT_SHEET_SPRITE;
            p.size = self.size;
            p.rgba[3] = self.alpha;
            if self.fade != 0. {
                p.fade = Fade::Linear(self.fade);
            }
            p.controller = Some(BillboardController::Stretch([
                self.growth,
                self.growth * 4.,
            ]));
            out.push(p);
        }
    }
}
