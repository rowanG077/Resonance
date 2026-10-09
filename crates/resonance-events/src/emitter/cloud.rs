//! A diffuse cloud can rise or release expanding rings without replacing its particles.
use super::{Births, Orbit, particle};
use crate::{
    Actor,
    effect::{BillboardController, Blend, Fade, GLOW_SPRITE},
    world::random,
};

#[derive(Debug, Clone, Default)]
pub(super) struct Cloud {
    pub palette: u16,
    pub size: [f32; 2],
    pub lifetime: [u32; 2],
    pub alpha: [u32; 2],
    pub fade: [f32; 2],
    turn: f32,
}

impl Cloud {
    pub fn emit(
        &mut self,
        center: [f32; 3],
        born: u32,
        clock: u32,
        phase: u8,
        actor: &Actor,
        rng: &mut u32,
        out: &mut Births,
    ) {
        random(rng);
        if phase > 2 {
            return;
        }
        let release = phase == 2;
        let periodic = clock.is_multiple_of(3);
        let speed = if phase == 1 {
            0.8
        } else {
            actor.movement_speed() / 10.
        };
        let direction = |rng: &mut u32| std::array::from_fn(|_| 90. - (random(rng) % 180) as f32);
        let glows = match (release, periodic) {
            (true, true) => 5,
            (true, false) => 0,
            (false, _) => 1,
        };
        for spoke in 0..glows {
            let lifetime = if release {
                self.alpha[0]
            } else {
                self.lifetime[0]
            } + 1;
            let mut glow = particle(center, born, self.palette, lifetime);
            glow.recipe = GLOW_SPRITE;
            glow.field_fog = false;
            glow.blend = Some(actor.blend.unwrap_or(Blend::Additive));
            glow.size = [self.size[usize::from(release)]; 2];
            glow.rgba[3] = if release {
                50
            } else {
                self.alpha[0].min(255) as u8
            };
            if release {
                let mut orbit = Orbit::new(
                    center,
                    [0., 1., 0.],
                    1.,
                    1.,
                    1. - self.turn - spoke as f32 * 72.,
                    0.1,
                );
                orbit.radial = [1., 0., 0.];
                glow.position = orbit.position(0);
                glow.controller = Some(BillboardController::Orbit(orbit));
                glow.size_delta = 0.25;
            } else {
                let mut direction = direction(rng);
                if phase == 0 {
                    direction.swap(1, 2);
                }
                random(rng);
                glow.controller = Some(BillboardController::Scatter {
                    direction,
                    speed,
                    planar: phase == 1,
                    wandering: false,
                });
                glow.size_delta = if random(rng).is_multiple_of(2) {
                    -0.25
                } else {
                    0.25
                };
                if self.fade[0] != 0. {
                    glow.fade = Fade::Linear(self.fade[0]);
                }
            }
            glow.rotation[2] = (random(rng) % 360) as f32;
            out.push(glow);
        }
        if !periodic {
            return;
        }
        if release {
            self.turn += 4.;
        }
        let color = if actor.blend.is_some() {
            super::palette(108, rng)
        } else {
            self.palette + (random(rng) % 4) as u16
        };
        let lifetime = if release {
            self.alpha[1]
        } else {
            self.lifetime[1]
        } + 1;
        let mut mote = particle(center, born, color, lifetime);
        mote.field_fog = false;
        mote.size = [if release {
            self.lifetime[0] as f32
        } else {
            self.size[1]
        }; 2];
        mote.rgba[3] = if release {
            150
        } else {
            self.alpha[1].min(255) as u8
        };
        mote.blend = actor.blend.map(|_| Blend::Subtractive);
        let direction = direction(rng);
        let sample = random(rng);
        mote.controller = Some(match phase {
            0 => BillboardController::Drift {
                direction,
                speed,
                spatial: true,
            },
            _ => BillboardController::Scatter {
                direction,
                speed: if release {
                    random(rng);
                    speed
                } else {
                    speed + f32::from(sample % 12 >= 10)
                },
                planar: !release,
                wandering: release,
            },
        });
        out.push(mote);
    }
}
