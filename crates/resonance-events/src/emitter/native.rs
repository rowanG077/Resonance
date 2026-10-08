//! Decode scenario emitter arguments and property slots at the script boundary.
use super::{Config, Emitter, PHASE_PROPERTY, stream};
use crate::effect::BILLBOARD_LIMIT;
#[derive(Debug, Clone)]
pub(super) struct Inputs {
    preset: i32,
    sprite: u16,
    parameters: [i32; 10],
    impact_texture: Option<(u32, u8)>,
}

impl Emitter {
    pub fn from_native(a: &[i32], impact_texture: Option<(u32, u8)>) -> Result<Self, String> {
        if a.len() != 18 {
            return Err("invalid emitter arguments".into());
        }
        let inputs = Inputs {
            preset: a[5],
            impact_texture,
            sprite: a[4] as u16,
            parameters: a[8..].try_into().unwrap(),
        };
        Ok(Self {
            config: inputs.decode()?,
            inputs,
            state: Default::default(),
        })
    }
    pub fn property(&mut self, property: i32, value: Option<i32>) -> Result<i32, String> {
        if property == PHASE_PROPERTY {
            let previous = i32::from(self.state.stage);
            if let Some(value) = value {
                if !(0..=4).contains(&value) {
                    return Err("invalid emitter stage".into());
                }
                self.state.stage = value as u8;
                self.state.age = 0;
                if value == 0 {
                    self.state.origin = None;
                    self.state.emitted = 0;
                }
            }
            return Ok(previous);
        }
        const FIRST_PARAMETER: i32 = 113;
        let Some(index) = property
            .checked_sub(FIRST_PARAMETER)
            .and_then(|i| usize::try_from(i).ok())
            .filter(|i| *i < 10)
        else {
            return Ok(0);
        };
        let previous = self.inputs.parameters[index];
        if let Some(value) = value {
            let mut inputs = self.inputs.clone();
            inputs.parameters[index] = value;
            self.config = inputs.decode()?;
            self.inputs = inputs;
        }
        Ok(previous)
    }
}
impl Inputs {
    fn decode(&self) -> Result<Config, String> {
        let a = &self.parameters;
        Ok(match self.preset {
            0 => Config::Fire {
                size: nonnegative(a[0])? as f32,
            },
            1..=3 | 9 | 17 | 24 | 33 | 36 | 54 | 55 | 75 => {
                Config::Stream(Box::new(stream::Stream::decode(self.preset, *a)?))
            }
            15 | 30 => Config::RisingOrbs(super::rays::RisingOrbs {
                palette: palette(a[0])?,
                radius: nonnegative(a[1])? as f32,
                size: a[2] as f32,
                variation: a[3].max(1) as u32,
                lighting: a[4] & 1 != 0,
                speed_variation: a[5].max(1) as u32,
                alpha: if self.preset == 15 {
                    255
                } else {
                    a[6].clamp(0, 255) as u8
                },
                fade: if self.preset == 15 { -1. } else { a[7] as f32 },
                interval: a[8].max(1) as u32,
                preserve_particles: self.preset == 15 && a[9] != 1,
                drifting: self.preset == 30,
            }),
            16 => Config::Crown {
                palette: palette(a[0])?,
                radius: a[1] as f32,
                spread: a[2] as f32,
            },
            34 => Config::Bloom(super::rays::Bloom {
                palette: palette(a[0])?,
                lifetime: nonnegative(a[1])? as u32,
                count: bounded(a[2], 0, BILLBOARD_LIMIT as i32)? as u32,
                size: [a[3] as f32, a[5] as f32],
                variation: [a[4].max(1) as u32, a[6].max(1) as u32],
            }),
            26 => Config::Shafts(super::rays::Shafts {
                palette: palette(a[0])?,
                interval: a[1].max(1) as u32,
                radius: a[2] as f32,
                size: [a[3] as f32, a[5] as f32],
                variation: [a[4].max(1) as u32, a[6].max(1) as u32],
                tilt: a[7] as f32,
                cluster: a[8].clamp(1, 100) as u32,
            }),
            27 => Config::Convergence(super::rays::Convergence {
                palette: palette(a[0])?,
                size: a[1] as f32,
                growth: a[2] as f32,
                radius: a[3].max(0) as u32,
                spread: a[4].max(1) as u32,
            }),
            28 => Config::Aura {
                palette: palette(a[0])?,
                offset: a[1] as f32,
            },
            11 => Config::Gathering {
                delay: nonnegative(a[0])?,
            },
            12 => Config::Glow {
                palette: palette(a[0])?,
                size: a[1],
                retire_with_emitter: a[9] != 0,
            },
            14 => Config::Portal {
                palette: palette(a[0])?,
                size: nonnegative(a[1])? as f32,
            },
            19 => Config::Charge {
                palette: palette(a[0])?,
                radius: a[3],
                updates: bounded(a[1], 1, i32::MAX)? as u32,
                target: [a[4] as f32, a[5] as f32, a[6] as f32],
                travelling: a[2] != 0,
            },
            13 => Config::Scatter(super::scatter::Scatter {
                palette: palette(a[0])? as u16,
                size: [a[1] as f32, a[3] as f32],
                variation: [a[2].max(1) as u32, a[4].max(1) as u32],
                lifetime: [nonnegative(a[5])? as u32 + 1, nonnegative(a[6])? as u32 + 1],
            }),
            18 => Config::Travel {
                sprite: crate::effect::ORB_SPRITE,
                palette: 33,
                size: a[1],
                burst_size: 0,
                fade: -10,
                target: [a[4], a[5], a[6]],
                curvature: if a[0] == 0 { 1. } else { a[0] as f32 / 100. },
                afterimages: false,
            },
            22 => Config::Quake,
            23 | 63 => Config::Column {
                palette: palette(a[0])?,
                size: a[1],
                layers: bounded(a[2], 0, BILLBOARD_LIMIT as i32)?,
                alpha: a[3],
                lighting: a[4],
                spacing: a[5],
                life: nonnegative(a[6])?,
                growth: a[7],
                expands: self.preset == 63,
            },
            31 => Config::Contract {
                palette: palette(a[0])?,
                radius: a[1],
                life: nonnegative(a[2])?,
                interval: bounded(a[3], 1, i32::MAX)?,
                angular_step: a[4],
                width: a[5],
                height: a[6],
                alpha: a[7],
                fade: a[8],
                growth: a[9],
            },
            38 => Config::Inward {
                palette: palette(a[0])?,
                radius: nonnegative(a[1])?,
                count: bounded(a[2], 0, BILLBOARD_LIMIT as i32)?,
                curve: a[3] as f32 / 100.,
                size: a[4],
                clear: a[8],
                blend: a[9],
            },
            66 => Config::Projectile {
                size: nonnegative(a[1])? as f32,
                fade: a[3] as f32 / 16.,
                target: [a[4] as f32, a[5] as f32, a[6] as f32],
                texture: self.impact_texture,
            },
            46 | 47 => Config::Travel {
                sprite: if self.preset == 47 {
                    self.sprite
                } else {
                    crate::effect::ORB_SPRITE
                },
                palette: palette(a[0])?,
                size: a[1],
                burst_size: a[2],
                fade: a[3],
                target: [a[4], a[5], a[6]],
                curvature: 0.,
                afterimages: self.preset == 46,
            },
            49 => Config::Seal {
                palette: palette(a[0])?,
                opening: a[1],
                pulse: a[2],
                spark: a[3],
            },
            48 => Config::Rising(super::rays::Rising {
                palette: palette(a[0])?,
                radius: nonnegative(a[1])? as f32,
                size: [a[2] as f32, a[4] as f32],
                variation: [a[3].max(1) as u32, a[5].max(1) as u32],
                alpha: bounded(a[6], 0, 255)? as u8,
                rise: a[7].max(1) as u32,
                world: a[8] != 0,
                lifetime: nonnegative(a[9])? as u32 + 1,
            }),
            51 => Config::Fireball {
                size: nonnegative(a[0])? as f32,
                updates: bounded(a[1], 1, i32::MAX)? as u32,
                target: [a[4] as f32, a[5] as f32, a[6] as f32],
            },
            60 => Config::Cardinal {
                count: bounded(a[0], 0, 4)?,
            },
            id => return Err(format!("unsupported emitter {id}")),
        })
    }
}
fn bounded(value: i32, min: i32, max: i32) -> Result<i32, String> {
    (min..=max)
        .contains(&value)
        .then_some(value)
        .ok_or_else(|| "invalid emitter settings".into())
}
fn palette(value: i32) -> Result<i32, String> {
    const LAST_RANDOM_PALETTE: i32 = 108;
    bounded(value, 0, LAST_RANDOM_PALETTE)
}
fn nonnegative(value: i32) -> Result<i32, String> {
    bounded(value, 0, i32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ray_emitters_reject_invalid_palettes_without_losing_their_settings() {
        for preset in [26, 27] {
            let mut args = [0; 18];
            args[5] = preset;
            args[8] = 33;
            let mut emitter = Emitter::from_native(&args, None).unwrap();
            for palette in [-1, 109, i32::MAX] {
                args[8] = palette;
                assert!(Emitter::from_native(&args, None).is_err());
                assert!(emitter.property(113, Some(palette)).is_err());
                assert_eq!(emitter.property(113, None).unwrap(), 33);
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
        assert_eq!(output.particles.last().unwrap().rgba[3], 128);
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
    #[test]
    fn fire_size_can_change_and_emission_can_pause_and_restart() {
        let mut emitter = Emitter::from_native(
            &[500, 0, 0, 0, 0, 0, 0, 0, 60, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            None,
        )
        .unwrap();
        assert_eq!(emitter.property(113, Some(120)).unwrap(), 60);
        assert_eq!(emitter.property(113, None).unwrap(), 120);
        assert!(emitter.property(PHASE_PROPERTY, Some(-1)).is_err());
        let mut actor = crate::Actor::new(0, [0.; 3]);
        let mut random = 1;
        let mut output = super::super::Births::default();
        emitter
            .step(
                (500, [0.; 3]),
                &mut actor,
                2,
                4,
                [0.; 3],
                &mut random,
                &mut output,
            )
            .unwrap();
        assert!(!output.particles.is_empty());
        assert_eq!(output.particles.len(), 2);
        assert!(output.particles[0].size[0] >= 120.);
        assert!(output.particles[1].size[0] >= 60.);
        emitter.property(PHASE_PROPERTY, Some(2)).unwrap();
        output.particles.clear();
        for tick in 1..20 {
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
        assert!(output.particles.is_empty());
        emitter.property(PHASE_PROPERTY, Some(0)).unwrap();
        emitter
            .step(
                (500, [0.; 3]),
                &mut actor,
                22,
                24,
                [0.; 3],
                &mut random,
                &mut output,
            )
            .unwrap();
        assert!(!output.particles.is_empty());
    }
}
