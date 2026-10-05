//! Decode scenario emitter arguments and property slots at the script boundary.
use super::{Emitter, Kind, PHASE_PROPERTY, stream};
use crate::effect::BILLBOARD_LIMIT;
#[derive(Debug, Clone)]
pub(super) struct Inputs {
    preset: i32,
    sprite: u16,
    parameters: [i32; 10],
}

impl Emitter {
    pub fn from_native(a: &[i32]) -> Result<Self, String> {
        if a.len() != 18 {
            return Err("invalid emitter arguments".into());
        }
        let inputs = Inputs {
            preset: a[5],
            sprite: a[4] as u16,
            parameters: a[8..].try_into().unwrap(),
        };
        Ok(Self {
            kind: inputs.decode()?,
            inputs,
            stage: 0,
            age: 0,
            origin: None,
        })
    }
    pub fn property(&mut self, property: i32, value: Option<i32>) -> Result<i32, String> {
        if property == PHASE_PROPERTY {
            let previous = i32::from(self.stage);
            if let Some(value) = value {
                if !(0..=4).contains(&value) {
                    return Err("invalid emitter stage".into());
                }
                self.stage = value as u8;
                self.age = 0;
                if value == 0 {
                    self.origin = None;
                    if let Kind::Stream(stream) = &mut self.kind {
                        stream.emitted = 0;
                    }
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
            let mut kind = inputs.decode()?;
            if let (Kind::Stream(old), Kind::Stream(new)) = (&self.kind, &mut kind) {
                new.angle = old.angle;
                new.emitted = old.emitted;
            }
            self.inputs = inputs;
            self.kind = kind;
        }
        Ok(previous)
    }
}
impl Inputs {
    fn decode(&self) -> Result<Kind, String> {
        let a = &self.parameters;
        let kind = match self.preset {
            0..=3 | 9 | 15..=17 | 24 | 26..=28 | 30 | 33 | 36 | 48 | 54 | 55 | 75 => {
                Kind::Stream(Box::new(stream::Stream::decode(self.preset, *a)?))
            }
            11 => Kind::Gathering { delay: a[0] },
            12 => Kind::Glow {
                palette: a[0],
                size: a[1],
            },
            19 => Kind::Charge {
                palette: a[0],
                radius: a[3],
            },
            13 => Kind::Scatter {
                palette: a[0],
                size: a[1],
                variation: a[2],
                mote_size: a[3],
                mote_variation: a[4],
                life: a[5],
                mote_life: a[6],
            },
            18 => Kind::Travel {
                sprite: crate::effect::ORB_SPRITE,
                palette: 33,
                size: a[1],
                burst_size: 0,
                fade: -10,
                target: [a[4], a[5], a[6]],
                curvature: if a[0] == 0 { 1. } else { a[0] as f32 / 100. },
                afterimages: false,
            },
            22 => Kind::Quake,
            23 | 63 => Kind::Column {
                palette: a[0],
                size: a[1],
                layers: a[2],
                alpha: a[3],
                lighting: a[4],
                spacing: a[5],
                life: a[6],
                growth: a[7],
                expands: self.preset == 63,
            },
            31 => Kind::Contract {
                target: None,
                palette: a[0],
                radius: a[1],
                life: a[2],
                interval: a[3],
                angular_step: a[4],
                width: a[5],
                height: a[6],
                alpha: a[7],
                fade: a[8],
                growth: a[9],
            },
            38 => Kind::Inward {
                palette: a[0],
                radius: a[1],
                count: a[2],
                curve: a[3] as f32 / 100.,
                size: a[4],
                clear: a[8],
                blend: a[9],
            },
            46 | 47 | 66 => Kind::Travel {
                sprite: if self.preset == 47 {
                    self.sprite
                } else {
                    crate::effect::ORB_SPRITE
                },
                palette: a[0],
                size: a[1],
                burst_size: a[2],
                fade: if self.preset == 66 { a[3] / 16 } else { a[3] },
                target: [a[4], a[5], a[6]],
                curvature: 0.,
                afterimages: self.preset == 46,
            },
            49 => Kind::Seal {
                palette: a[0],
                opening: a[1],
                pulse: a[2],
                spark: a[3],
            },
            60 => Kind::Cardinal { count: a[0] },
            id => return Err(format!("unsupported emitter {id}")),
        };
        kind.validate()?;
        Ok(kind)
    }
}
impl Kind {
    fn validate(&self) -> Result<(), String> {
        let valid = match self {
            Kind::Gathering { delay } => *delay >= 0,
            Kind::Cardinal { count } => (0..=4).contains(count),
            Kind::Column {
                palette,
                layers,
                life,
                ..
            } => color(*palette) && count(*layers) && *life >= 0,
            Kind::Inward {
                palette,
                radius,
                count: n,
                ..
            } => color(*palette) && *radius >= 0 && count(*n),
            Kind::Contract {
                palette,
                life,
                interval,
                ..
            } => color(*palette) && *life >= 0 && *interval > 0,
            Kind::Scatter {
                palette,
                life,
                mote_life,
                variation,
                mote_variation,
                ..
            } => {
                color(*palette)
                    && *life >= 0
                    && *mote_life >= 0
                    && *variation >= 0
                    && *mote_variation >= 0
            }
            Kind::Glow { palette, .. }
            | Kind::Travel { palette, .. }
            | Kind::Seal { palette, .. }
            | Kind::Charge { palette, .. } => color(*palette),
            _ => true,
        };
        if valid {
            Ok(())
        } else {
            Err("invalid emitter settings".into())
        }
    }
}
fn color(value: i32) -> bool {
    (0..=108).contains(&value)
}
fn count(value: i32) -> bool {
    (0..=BILLBOARD_LIMIT as i32).contains(&value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_count_updates_leave_a_usable_emitter() {
        let mut emitter =
            Emitter::from_native(&[500, 0, 0, 0, 0, 60, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0])
                .unwrap();
        for count in [-1, 5, i32::MAX] {
            assert!(emitter.property(113, Some(count)).is_err());
            assert_eq!(emitter.property(113, None).unwrap(), 4);
        }
    }
    #[test]
    fn stream_updates_are_atomic_and_release_can_be_restarted() {
        let mut emitter =
            Emitter::from_native(&[500, 0, 0, 0, 0, 0, 0, 0, 60, 0, 0, 0, 0, 0, 0, 0, 0, 0])
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
                0,
                [0.; 3],
                &mut random,
                &mut output,
            )
            .unwrap();
        assert!(!output.particles.is_empty());
        assert!(output.particles.iter().all(|p| p.lifetime == 120));
        emitter.property(PHASE_PROPERTY, Some(2)).unwrap();
        output.particles.clear();
        for tick in 1..20 {
            emitter
                .step(
                    (500, [0.; 3]),
                    &mut actor,
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
                20,
                [0.; 3],
                &mut random,
                &mut output,
            )
            .unwrap();
        assert!(!output.particles.is_empty());
    }
}
