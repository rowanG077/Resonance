//! Decode effect timeline and modifier operands at the asset boundary.
use anyhow::{Context, Result, bail, ensure};
use resonance_content::battle_effect::{declaration::Declaration, *};
use std::collections::BTreeMap;
mod compile;
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum VectorTarget {
    Offset,
    Velocity,
    Angles,
    AngularVelocity,
    Orbit,
    OrbitVelocity,
    Size,
    SizeVelocity,
    SizeAcceleration,
}
#[derive(Debug, Clone, Copy)]
struct Repeat {
    count: u16,
    interval: u32,
}
#[derive(Default)]
struct InputTimeline {
    events: Vec<InputEvent>,
    end: u32,
}
struct InputEvent {
    at: u32,
    repeat: Option<Repeat>,
    operation: InputOperation,
}
#[derive(Clone)]
enum InputOperation {
    Spawn {
        particle: u8,
        blend: Option<u8>,
        palette: Option<u8>,
        edits: Vec<InputEdit>,
    },
    Sound {
        id: u16,
        priority: u8,
    },
    Shake {
        duration: u32,
        amplitude: u32,
    },
}

#[derive(Debug, Clone, PartialEq)]
enum Arithmetic {
    Set,
    Add,
    Subtract,
    Multiply,
    Divide,
}
#[derive(Debug, Clone, PartialEq)]
enum InputScalarValue {
    Literal { value: f32 },
    Scratch { index: u8 },
    Random { range: i16, scale: f32 },
}
#[derive(Debug, Clone, PartialEq)]
enum InputIntegerValue {
    Literal { value: i16 },
    Scratch { index: u8 },
    Random { range: u16 },
}
#[derive(Debug, Clone, Copy, PartialEq)]
enum InputScalarTarget {
    Vector { vector: VectorTarget, axis: u8 },
    SegmentAngleStep,
    Scratch { index: u8 },
}
#[derive(Debug, Clone, Copy, PartialEq)]
enum InputIntegerTarget {
    Scratch { index: u8 },
    Color { color: u8, channel: u8 },
    Uv { component: u8 },
    Brighten { channel: u8 },
    Fade { channel: u8 },
    BrightenUntil,
    GeometryCount,
    Phase,
    Palette,
    Model,
}
#[derive(Debug, Clone, PartialEq)]
enum InputEdit {
    Scalar {
        target: InputScalarTarget,
        arithmetic: Arithmetic,
        value: InputScalarValue,
    },
    Integer {
        target: InputIntegerTarget,
        arithmetic: Arithmetic,
        value: InputIntegerValue,
    },
    Polar {
        target: VectorTarget,
        angles: [f32; 3],
        radius: f32,
        radius_jitter: i16,
        angle_jitter: i16,
    },
    CullBack,
    ElementTint,
}

#[derive(Clone, Copy)]
pub(super) struct Record {
    pub age: i16,
    pub command: u8,
    pub argument: u8,
    pub operand: u16,
}
impl Record {
    pub fn from_bytes(b: [u8; 6]) -> Self {
        Self {
            age: i16::from_be_bytes([b[0], b[1]]),
            command: b[2],
            argument: b[3],
            operand: u16::from_be_bytes([b[4], b[5]]),
        }
    }
}

pub(super) fn decode(
    records: &[Record],
    actors: &[Declaration],
    modifiers: &BTreeMap<u16, Vec<u16>>,
) -> Vec<ScheduledEvent> {
    let mut timeline = InputTimeline::default();
    let mut index = 0;
    let mut at = 0;
    while let Some(record) = records.get(index) {
        at = at.max(record.age.max(0) as u32);
        if record.command == 254 {
            timeline.end = at;
            break;
        }
        let repeat = (record.command == 255).then(|| Repeat {
            count: u16::from(record.argument.max(1)),
            interval: (record.operand as i16).max(0) as u32,
        });
        let record = if repeat.is_some() {
            index += 1;
            &records[index]
        } else {
            record
        };
        let operation = match operation(record, actors, modifiers) {
            Ok(operation) => operation,
            Err(error) => {
                return vec![ScheduledEvent {
                    at,
                    operation: EffectOperation::Unsupported {
                        reason: error.to_string(),
                    },
                }];
            }
        };
        timeline.events.push(InputEvent {
            at,
            repeat,
            operation,
        });
        index += 1;
    }
    compile::timeline(timeline, actors)
}
fn operation(
    record: &Record,
    actors: &[Declaration],
    modifiers: &BTreeMap<u16, Vec<u16>>,
) -> Result<InputOperation> {
    ensure!(
        record.command != 253,
        "delayed particle edits are unsupported"
    );
    if record.command == 252 {
        return Ok(InputOperation::Sound {
            id: u16::from(record.argument),
            priority: record.operand as u8,
        });
    }
    if record.command < 252
        && let Some(Declaration::CameraShake {
            duration,
            amplitude,
        }) = actors.get(record.command as usize)
    {
        return Ok(InputOperation::Shake {
            duration: *duration,
            amplitude: *amplitude,
        });
    }
    let words = if record.operand == 0 {
        &[65535][..]
    } else {
        modifiers
            .get(&record.operand)
            .context("unbound particle edits")?
    };
    ensure!(record.argument == 0, "effect attachment is not supported");
    let mut words = words;
    let (mut blend, mut palette) = (None, None);
    while words.first() == Some(&10) && matches!(words.get(1), Some(1 | 3)) {
        let row = words.get(..4).context("truncated particle appearance")?;
        ensure!(
            (row[2] as i16) < 0x7ff8,
            "dynamic particle appearance is unsupported"
        );
        let value = u8::try_from(row[2]).context("particle appearance value exceeds byte range")?;
        if row[1] == 1 {
            ensure!(value <= 1, "unsupported particle blend");
            blend = Some(value);
        } else {
            palette = Some(value);
        }
        words = &words[4..];
    }
    ensure!(
        !matches!(
            actors.get(record.command as usize),
            Some(Declaration::ModelParticle { .. })
        ) || (blend.is_none() && palette.is_none()),
        "model appearance overrides are unsupported"
    );
    Ok(InputOperation::Spawn {
        particle: record.command,
        blend,
        palette,
        edits: edits(words)?,
    })
}
fn integer(value: u16) -> Result<InputIntegerValue> {
    Ok(if value as i16 >= 0x7ffc {
        InputIntegerValue::Scratch {
            index: (value - 0x7ffc) as u8,
        }
    } else {
        ensure!(
            (value as i16) < 0x7ff8,
            "float to integer particle operand is unsupported"
        );
        InputIntegerValue::Literal {
            value: value as i16,
        }
    })
}
fn scalar_target(target: u16) -> Result<InputScalarTarget> {
    let vectors = [
        (0x34, VectorTarget::Offset),
        (0x40, VectorTarget::Velocity),
        (0x58, VectorTarget::Angles),
        (0x64, VectorTarget::AngularVelocity),
        (0x98, VectorTarget::Orbit),
        (0xa4, VectorTarget::OrbitVelocity),
        (0xb0, VectorTarget::Size),
        (0xbc, VectorTarget::SizeVelocity),
        (0xc8, VectorTarget::SizeAcceleration),
    ];
    for (base, vector) in vectors {
        if (base..=base + 8).contains(&target) && target.is_multiple_of(4) {
            return Ok(InputScalarTarget::Vector {
                vector,
                axis: ((target - base) / 4) as u8,
            });
        }
    }
    match target {
        0x84 => Ok(InputScalarTarget::SegmentAngleStep),
        0x7ff8..=0x7ffb => Ok(InputScalarTarget::Scratch {
            index: (target - 0x7ff8) as u8,
        }),
        _ => bail!("unsupported particle scalar destination"),
    }
}
fn edits(words: &[u16]) -> Result<Vec<InputEdit>> {
    let mut out = Vec::new();
    let mut at = 0;
    loop {
        let opcode = *words.get(at).context("unterminated particle edits")?;
        if opcode == 65535 {
            ensure!(at + 1 == words.len(), "data follows particle edit end");
            return Ok(out);
        }
        let count = match opcode {
            0 | 2 | 3 | 5 | 10..=12 | 21 => 4,
            7 | 8 | 13..=15 => 6,
            9 => 12,
            _ => bail!("unsupported particle operation {opcode}"),
        };
        let row = words
            .get(at..at + count)
            .context("truncated particle edit")?;
        at += count;
        let target = row[1];
        let float = |i| -> Result<f32> {
            let value = f32::from_bits(u32::from(row[i]) << 16 | u32::from(row[i + 1]));
            ensure!(value.is_finite(), "invalid particle operand");
            Ok(value)
        };
        if opcode == 21 {
            let flags = u32::from(row[2]) << 16 | u32::from(row[3]);
            ensure!(flags & !0x10400000 == 0, "unsupported particle flags");
            if flags & 0x10000000 != 0 {
                out.push(InputEdit::CullBack);
            }
            if flags & 0x400000 != 0 {
                out.push(InputEdit::ElementTint);
            }
            continue;
        }
        if opcode == 9 {
            out.push(InputEdit::Polar {
                target: match target {
                    0x34 => VectorTarget::Offset,
                    0x40 => VectorTarget::Velocity,
                    _ => bail!("unsupported polar destination"),
                },
                angles: [float(2)?, float(4)?, float(6)?],
                radius: float(8)?,
                radius_jitter: row[10] as i16,
                angle_jitter: row[11] as i16,
            });
            continue;
        }
        if matches!(opcode, 7 | 8 | 11 | 13..=15) {
            let value = if opcode == 11 {
                ensure!(
                    row[2] != 0 && (row[3] as i16) < 0x7ff8,
                    "invalid random particle range"
                );
                InputScalarValue::Random {
                    range: row[2] as i16,
                    scale: 0.1,
                }
            } else if row[4] as i16 >= 0x7ff8 {
                ensure!(row[4] < 0x7ffc, "integer to scalar operand unsupported");
                InputScalarValue::Scratch {
                    index: (row[4] - 0x7ff8) as u8,
                }
            } else {
                InputScalarValue::Literal { value: float(2)? }
            };
            out.push(InputEdit::Scalar {
                target: scalar_target(target)?,
                arithmetic: match opcode {
                    8 => Arithmetic::Add,
                    13 => Arithmetic::Subtract,
                    14 => Arithmetic::Multiply,
                    15 => Arithmetic::Divide,
                    _ => Arithmetic::Set,
                },
                value,
            });
            continue;
        }
        let destination = match (opcode, target) {
            (0 | 2 | 3 | 5, 0x7ffc..=0x7fff) => InputIntegerTarget::Scratch {
                index: (target - 0x7ffc) as u8,
            },
            (0 | 3, 0x08..=0x0e) if target.is_multiple_of(2) => InputIntegerTarget::Uv {
                component: ((target - 8) / 2) as u8,
            },
            (0, 0x18..=0x26) if target.is_multiple_of(2) => InputIntegerTarget::Color {
                color: ((target - 0x18) / 8) as u8,
                channel: ((target % 8) / 2) as u8,
            },
            (10, 0x28..=0x2b) => InputIntegerTarget::Brighten {
                channel: (target - 0x28) as u8,
            },
            (10, 0x2c..=0x2f) => InputIntegerTarget::Fade {
                channel: (target - 0x2c) as u8,
            },
            (10, 0x30) => InputIntegerTarget::BrightenUntil,
            (10 | 12, 0x12) => InputIntegerTarget::GeometryCount,
            (10 | 12, 0x13) => InputIntegerTarget::Phase,
            (12, 3) => InputIntegerTarget::Palette,
            (10, 0xd4) => InputIntegerTarget::Model,
            _ => bail!("unsupported particle integer destination"),
        };
        let value = if opcode == 2 {
            ensure!(
                row[2] > 0 && row[2] < 0x7ff8,
                "invalid particle random range"
            );
            InputIntegerValue::Random { range: row[2] }
        } else {
            integer(row[2])?
        };
        out.push(InputEdit::Integer {
            target: destination,
            arithmetic: match opcode {
                3 | 12 => Arithmetic::Add,
                5 => Arithmetic::Multiply,
                _ => Arithmetic::Set,
            },
            value,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appearance_values_are_checked_before_narrowing() {
        let record = Record {
            age: 0,
            command: 0,
            argument: 0,
            operand: 1,
        };
        for (target, maximum) in [(1, 1), (3, 255)] {
            for value in [0, 1, 2, 255, 256, 257, u16::MAX] {
                let modifiers = BTreeMap::from([(1, vec![10, target, value, 0, u16::MAX])]);
                assert_eq!(
                    operation(&record, &[], &modifiers).is_ok(),
                    value <= maximum,
                    "target {target}, value {value}"
                );
            }
        }
    }
}
