//! Decode effect timeline and modifier operands at the asset boundary.
use anyhow::{Context, Result, bail, ensure};
use resonance_content::battle_effect::{declaration::Declaration, *};
use std::collections::BTreeMap;
mod compile;
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum VectorTarget {
    Offset,
    Velocity,
    Acceleration,
    Angles,
    AngularVelocity,
    SegmentOffset,
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
        origin_bone: Option<u8>,
        inherit_previous: bool,
        retained: bool,
        blend: Option<u8>,
        palette: Option<u8>,
        edits: Vec<InputEdit>,
    },
    SetMotion {
        retained: u8,
        motion: Box<MotionChange>,
    },
    Sound {
        id: u16,
        priority: u8,
    },
    Shake {
        duration: u32,
        amplitude: u32,
    },
    StageTint {
        color: [u8; 3],
        duration: u16,
    },
    Notice {
        text: String,
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
    ScalarHigh { index: u8 },
    RollHigh,
    Color { color: u8, channel: u8 },
    Uv { component: u8 },
    Brighten { channel: u8 },
    Fade { channel: u8 },
    BrightenUntil,
    Lifetime,
    FadeFrom,
    GeometryCount,
    AttachmentIndex,
    Phase,
    Palette,
    PalettePair,
    Model,
    ModelAndAnimation { animation: u8 },
}
#[derive(Debug, Clone, PartialEq)]
enum InputEdit {
    ModelAnimation(ModelAnimation),
    HeldModelFrame {
        scalar: u8,
    },
    Forward {
        target: VectorTarget,
        value: f32,
    },
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
    RibbonDepth,
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

#[cfg(test)]
pub(super) fn decode(
    records: &[Record],
    actors: &[Declaration],
    retained: &[bool],
    modifiers: &BTreeMap<u16, Vec<u16>>,
    animation_bytes: &[u8],
) -> Vec<ScheduledEvent> {
    decode_grouped(
        records,
        &Default::default(),
        actors,
        retained,
        modifiers,
        animation_bytes,
    )
}

pub(super) fn decode_grouped(
    records: &[Record],
    inherited: &std::collections::BTreeSet<usize>,
    actors: &[Declaration],
    retained: &[bool],
    modifiers: &BTreeMap<u16, Vec<u16>>,
    animation_bytes: &[u8],
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
        let operation = match operation(
            record,
            actors,
            modifiers,
            animation_bytes.get(usize::from(record.command)).copied(),
        )
        .map(|mut operation| {
            if let InputOperation::Spawn {
                particle,
                retained: keep,
                inherit_previous,
                ..
            } = &mut operation
            {
                *inherit_previous = inherited.contains(&index);
                *keep = retained
                    .get(usize::from(*particle))
                    .copied()
                    .unwrap_or(false);
            }
            operation
        }) {
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
    animation_byte: Option<u8>,
) -> Result<InputOperation> {
    if record.command == 252 {
        return Ok(InputOperation::Sound {
            id: u16::from(record.argument),
            priority: record.operand as u8,
        });
    }
    if record.command < 252
        && let Some(Declaration::Notice { text }) = actors.get(record.command as usize)
    {
        return Ok(InputOperation::Notice { text: text.clone() });
    }
    if record.command < 252
        && let Some(Declaration::StageTint { color, duration }) =
            actors.get(record.command as usize)
    {
        return Ok(InputOperation::StageTint {
            color: *color,
            duration: *duration,
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
    if record.command == 253 {
        let mut offset = [None; 3];
        let mut size = [None; 3];
        let mut animation = None;
        let mut velocity = [None; 3];
        let mut velocity_add: Option<[f32; 3]> = None;
        let mut acceleration = [None; 3];
        let mut angular_velocity = [None; 3];
        let mut forward_motion: Option<ForwardMotion> = None;
        let mut velocity_scale = None;
        let mut size_velocity = [None; 3];
        let mut size_acceleration = [None; 3];
        for edit in edits(words)? {
            if let InputEdit::ModelAnimation(playback) = edit {
                ensure!(
                    animation.replace(playback).is_none(),
                    "multiple delayed model animations"
                );
                continue;
            }
            if let InputEdit::Forward { target, value } = edit {
                let motion = forward_motion.get_or_insert_default();
                match target {
                    VectorTarget::Velocity => motion.velocity = Some(value),
                    VectorTarget::Acceleration => motion.acceleration = Some(value),
                    _ => bail!("unsupported forward motion destination"),
                }
                continue;
            }
            let InputEdit::Scalar {
                target: InputScalarTarget::Vector { vector, axis },
                arithmetic,
                value: InputScalarValue::Literal { value },
            } = edit
            else {
                bail!("unsupported delayed particle motion edit");
            };
            let axis = usize::from(axis);
            match (vector, arithmetic) {
                (VectorTarget::Offset, Arithmetic::Set) => offset[axis] = Some(value),
                (VectorTarget::Size, Arithmetic::Set) => size[axis] = Some(value),
                (VectorTarget::Velocity, Arithmetic::Multiply) => {
                    velocity_scale.get_or_insert([1.; 3])[axis] *= value;
                    if let Some(add) = &mut velocity_add {
                        add[axis] *= value;
                    }
                }
                (VectorTarget::Velocity, Arithmetic::Add) => {
                    velocity_add.get_or_insert([0.; 3])[axis] += value;
                }
                (VectorTarget::Velocity, Arithmetic::Set) => {
                    velocity[axis] = Some(value);
                    if let Some(add) = &mut velocity_add {
                        add[axis] = 0.;
                    }
                    if let Some(scale) = &mut velocity_scale {
                        scale[axis] = 1.;
                    }
                }
                (VectorTarget::Acceleration, Arithmetic::Set) => acceleration[axis] = Some(value),
                (VectorTarget::AngularVelocity, Arithmetic::Set) => {
                    angular_velocity[axis] = Some(value)
                }
                (VectorTarget::SizeVelocity, Arithmetic::Set) => size_velocity[axis] = Some(value),
                (VectorTarget::SizeAcceleration, Arithmetic::Set) => {
                    size_acceleration[axis] = Some(value)
                }
                _ => bail!("unsupported delayed particle motion destination"),
            }
        }
        if let Some(forward) = forward_motion {
            ensure!(
                forward.velocity.is_none()
                    || (velocity.iter().all(Option::is_none)
                        && velocity_add.is_none()
                        && velocity_scale.is_none()),
                "mixed forward and component velocity edits"
            );
            ensure!(
                forward.acceleration.is_none() || acceleration.iter().all(Option::is_none),
                "mixed forward and component acceleration edits"
            );
        }
        return Ok(InputOperation::SetMotion {
            retained: record.argument,
            motion: Box::new(MotionChange {
                offset,
                size,
                animation,
                velocity,
                velocity_add,
                acceleration,
                angular_velocity,
                forward_motion,
                velocity_scale,
                size_velocity,
                size_acceleration,
            }),
        });
    }
    ensure!(record.argument < 128, "unsupported effect origin selector");
    let origin_bone = (record.argument != 0).then_some(record.argument);
    if origin_bone.is_some() {
        let (template, static_model) = match actors.get(usize::from(record.command)) {
            Some(Declaration::Particle { template, .. }) => (template, false),
            Some(Declaration::ModelParticle {
                template, binding, ..
            }) if binding.animation.is_none()
                && !binding.program_animation
                && binding.attachment.is_none() =>
            {
                (template, true)
            }
            _ => anyhow::bail!("bone birth requires an ordinary particle or a static model"),
        };
        ensure!(
            !template.follow_origin
                && !template.follow_orientation
                && (template.follow_bone.is_none()
                    || (static_model && template.follow_bone == origin_bone))
                && template.owner_bone.is_none(),
            "bone birth with a live attachment is unsupported"
        );
    }
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
        origin_bone,
        inherit_previous: false,
        retained: false,
        blend,
        palette,
        edits: edits_for(
            words,
            actors.get(usize::from(record.command)),
            animation_byte,
        )?,
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
        (0x4c, VectorTarget::Acceleration),
        (0x58, VectorTarget::Angles),
        (0x64, VectorTarget::AngularVelocity),
        (0x70, VectorTarget::SegmentOffset),
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
    edits_for(words, None, None)
}

fn edits_for(
    words: &[u16],
    declaration: Option<&Declaration>,
    animation_byte: Option<u8>,
) -> Result<Vec<InputEdit>> {
    let mut out = Vec::new();
    let mut at = 0;
    loop {
        let opcode = *words.get(at).context("unterminated particle edits")?;
        if opcode == 65535 {
            ensure!(at + 1 == words.len(), "data follows particle edit end");
            return Ok(out);
        }
        if opcode == 26 {
            let pair = words
                .get(at..at + 12)
                .context("truncated held model frame")?;
            ensure!(
                pair[..4] == [26, 0x40, 0, 0]
                    && (0x7ff8..0x7ffc).contains(&pair[4])
                    && pair[5..] == [0, 27, 0x40, 0, 0, 0, 0]
                    && matches!(declaration,
                        Some(Declaration::ModelParticle { template, binding, .. })
                        if template.owner_bone.is_some()
                            && binding.slot == 0
                            && binding.animation == Some(0)
                            && binding.repeat
                            && !binding.program_animation
                            && binding.attachment.is_none()),
                "unsupported held model frame operands or binding"
            );
            out.push(InputEdit::HeldModelFrame {
                scalar: (pair[4] - 0x7ff8) as u8,
            });
            at += 12;
            continue;
        }
        let count = match opcode {
            1 | 22 => 8,
            0 | 2..=6 | 10..=12 | 21 | 23..=25 => 4,
            7 | 8 | 13..=15 | 18 | 30 => 6,
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
        // This auxiliary literal has no draw or motion effect on the admitted bone ring.
        if opcode == 7 && target == 0x80 {
            ensure!(
                row[4] == 0
                    && row[5] == 0
                    && matches!(declaration, Some(Declaration::Particle { template, visual })
                        if template.owner_bone.is_some()
                            && matches!(visual.geometry, visual::ParticleShape::Ring { .. })),
                "unsupported particle auxiliary scalar"
            );
            float(2)?;
            continue;
        }
        if opcode == 10 && target == 0x8d {
            let Some(Declaration::ModelParticle { template, .. }) = declaration else {
                bail!("texture frame requires a model particle");
            };
            ensure!(
                template.uv_animation.is_none()
                    && template.state.uv[3] > 0
                    && row[2] < template.state.uv[3] as u16
                    && row[3] == 0,
                "model texture frame is outside its prepared rows"
            );
            out.push(InputEdit::Integer {
                target: InputIntegerTarget::Uv { component: 1 },
                arithmetic: Arithmetic::Set,
                value: InputIntegerValue::Literal {
                    value: row[2] as i16,
                },
            });
            continue;
        }
        if opcode == 30 {
            ensure!(
                row[4] == 0 && row[5] == 0,
                "dynamic forward motion is unsupported"
            );
            let target = match target {
                0x40 => VectorTarget::Velocity,
                0x4c => VectorTarget::Acceleration,
                _ => bail!("unsupported forward motion destination"),
            };
            out.push(InputEdit::Forward {
                target,
                value: float(2)?,
            });
            continue;
        }
        if opcode == 22 {
            ensure!(
                target == 0x5c
                    && row[2] <= u8::MAX as u16
                    && row[4] <= u8::MAX as u16
                    && matches!(row[6], 0 | 8)
                    && row[7] == 0,
                "unsupported model animation start"
            );
            if let Some(declaration) = declaration {
                ensure!(
                    matches!(declaration, Declaration::ModelParticle { binding, .. } if binding.program_animation),
                    "model animation start requires program-owned playback"
                );
            }
            let playback = ModelAnimation {
                model: row[2] as u8,
                clip: row[3],
                frame: 0.,
                blend_ticks: row[4] as u8,
                rate: if row[5] == 0 {
                    0.5
                } else {
                    f32::from(row[5]) * 0.01
                },
                repeat: row[6] == 0,
            };
            playback.validate()?;
            out.push(InputEdit::ModelAnimation(playback));
            continue;
        }
        if opcode == 1 {
            let InputScalarTarget::Vector { vector, axis: 0 } = scalar_target(target)? else {
                bail!("unsupported particle vector destination");
            };
            for axis in 0..3 {
                out.push(InputEdit::Scalar {
                    target: InputScalarTarget::Vector { vector, axis },
                    arithmetic: Arithmetic::Set,
                    value: InputScalarValue::Literal {
                        value: float(2 + usize::from(axis) * 2)?,
                    },
                });
            }
            continue;
        }
        if opcode == 18 {
            let color = match target {
                0x18 => 0,
                0x20 => 1,
                _ => bail!("unsupported particle color destination"),
            };
            for channel in 0..4 {
                let value = row[2 + usize::from(channel)] as i16;
                ensure!(
                    value < 0x7ff8,
                    "dynamic packed particle color is unsupported"
                );
                out.push(InputEdit::Integer {
                    target: InputIntegerTarget::Color { color, channel },
                    arithmetic: Arithmetic::Set,
                    value: InputIntegerValue::Literal { value },
                });
            }
            continue;
        }
        if opcode == 21 {
            let flags = u32::from(row[2]) << 16 | u32::from(row[3]);
            ensure!(flags & !0x10400020 == 0, "unsupported particle flags");
            if flags & 0x10000000 != 0 {
                out.push(InputEdit::CullBack);
            }
            if flags & 0x400000 != 0 {
                out.push(InputEdit::ElementTint);
            }
            if flags & 0x20 != 0 {
                ensure!(
                    matches!(declaration, Some(Declaration::Particle { template, .. })
                        if matches!(template.state.geometry, ParticleGeometry::Ribbon { .. })),
                    "depth modifier requires a ribbon"
                );
                out.push(InputEdit::RibbonDepth);
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
            (0 | 2 | 3 | 5, 0x7ff8..=0x7ffb) => InputIntegerTarget::ScalarHigh {
                index: (target - 0x7ff8) as u8,
            },
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
            (10 | 12 | 23..=25, 0x2c..=0x2f) => InputIntegerTarget::Fade {
                channel: (target - 0x2c) as u8,
            },
            (10, 0x30) => InputIntegerTarget::BrightenUntil,
            (0 | 3..=6, 0x10) => InputIntegerTarget::Lifetime,
            (10 | 12 | 23, 0x31) => InputIntegerTarget::FadeFrom,
            (10 | 12, 0x12) => InputIntegerTarget::GeometryCount,
            (10 | 12, 0x13) => InputIntegerTarget::Phase,
            (10 | 12, 3) => InputIntegerTarget::Palette,
            (3, 3) => InputIntegerTarget::PalettePair,
            (3, 0x60) if row[3] == 0 => InputIntegerTarget::RollHigh,
            (10 | 12, 0xd4) => InputIntegerTarget::Model,
            (3, 0xd4) => InputIntegerTarget::ModelAndAnimation {
                animation: animation_byte
                    .context("packed model edit lacks its initial animation byte")?,
            },
            (10, 0x8c) => InputIntegerTarget::AttachmentIndex,
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
                4 | 23 => Arithmetic::Subtract,
                5 | 24 => Arithmetic::Multiply,
                6 | 25 => Arithmetic::Divide,
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
    fn particle_birth_preserves_the_body_bone_selector_and_schedule() -> Result<()> {
        let mut bytes = [0; 352];
        bytes[0] = 8;
        bytes[19] = 1;
        bytes[50] = 255;
        let mut actors = vec![crate::battle_effect::declaration::read(&bytes, &[])?];
        let record = |age, argument| Record {
            age,
            command: 0,
            argument,
            operand: 0,
        };
        let records = [
            record(0, 0),
            record(18, 49),
            record(48, 49),
            Record {
                age: 50,
                command: 254,
                argument: 0,
                operand: 0,
            },
        ];
        let events = decode(&records, &actors, &[false], &BTreeMap::new(), &[]);
        let births: Vec<_> = events
            .iter()
            .map(|event| match &event.operation {
                EffectOperation::Spawn { birth, .. } => (event.at, birth.origin_bone),
                _ => panic!("particle birth expected"),
            })
            .collect();
        assert_eq!(births, [(0, None), (18, Some(49)), (48, Some(49))]);
        assert!(operation(&record(18, 250), &actors, &BTreeMap::new(), None).is_err());
        actors[0].template_mut()?.follow_origin = true;
        assert!(operation(&record(18, 49), &actors, &BTreeMap::new(), None).is_err());
        bytes[0] = 3;
        actors[0] = crate::battle_effect::declaration::read(&bytes, &[])?;
        assert!(matches!(
            operation(&record(18, 49), &actors, &BTreeMap::new(), None)?,
            InputOperation::Spawn {
                origin_bone: Some(49),
                ..
            }
        ));
        actors[0].template_mut()?.follow_bone = Some(49);
        assert!(operation(&record(18, 49), &actors, &BTreeMap::new(), None).is_ok());
        assert!(operation(&record(18, 48), &actors, &BTreeMap::new(), None).is_err());
        let Declaration::ModelParticle { binding, .. } = &mut actors[0] else {
            panic!("model particle expected");
        };
        binding.animation = Some(0);
        assert!(operation(&record(18, 49), &actors, &BTreeMap::new(), None).is_err());
        for (flags, original, selected, drawing) in [
            (0x4000_0000_u32, 37, 54, true),
            (0x2400_0000_u32, 14, 8, false),
        ] {
            let mut bytes = [0; 352];
            bytes[0] = 4;
            bytes[2] = 2;
            bytes[20..24].copy_from_slice(&flags.to_be_bytes());
            bytes[50] = 255;
            bytes[140] = original;
            let actors = [crate::battle_effect::declaration::read(&bytes, &[])?];
            let mut changed = record(0, 0);
            changed.operand = 1;
            let events = decode(
                &[record(0, 0), changed],
                &actors,
                &[false],
                &BTreeMap::from([(1, vec![10, 0x8c, selected, 0, u16::MAX])]),
                &[],
            );
            let bindings: Vec<_> = events
                .iter()
                .map(|event| match &event.operation {
                    EffectOperation::Spawn { birth, .. } => {
                        assert_eq!(birth.origin_bone, None);
                        assert_eq!(birth.effect_group, None);
                        birth.attachment_bone
                    }
                    _ => panic!("particle birth expected"),
                })
                .collect();
            assert_eq!(bindings, [None, Some(selected as u8)]);
            let template = actors[0].template()?;
            assert_eq!(template.owner_bone, drawing.then_some(original));
            assert_eq!(template.follow_bone, (!drawing).then_some(original));
        }
        Ok(())
    }

    #[test]
    fn retained_forward_motion_preserves_authored_edits() -> Result<()> {
        let row = |operand| Record {
            age: 235,
            command: 253,
            argument: 2,
            operand,
        };
        let modifiers = BTreeMap::from([
            (1, vec![8, 0x44, 0x4170, 0, 0, 0, u16::MAX]),
            (
                2,
                vec![
                    30,
                    0x40,
                    0x4130,
                    0,
                    0,
                    0,
                    30,
                    0x4c,
                    0xbe80,
                    0,
                    0,
                    0,
                    u16::MAX,
                ],
            ),
            (
                3,
                vec![
                    7,
                    0x40,
                    0,
                    0,
                    0,
                    0,
                    7,
                    0x44,
                    0,
                    0,
                    0,
                    0,
                    7,
                    0x48,
                    0,
                    0,
                    0,
                    0,
                    7,
                    0x4c,
                    0,
                    0,
                    0,
                    0,
                    7,
                    0x50,
                    0,
                    0,
                    0,
                    0,
                    7,
                    0x54,
                    0,
                    0,
                    0,
                    0,
                    u16::MAX,
                ],
            ),
        ]);
        let InputOperation::SetMotion { motion, .. } = operation(&row(1), &[], &modifiers, None)?
        else {
            panic!("motion")
        };
        assert_eq!(motion.velocity_add, Some([0., 15., 0.]));
        let InputOperation::SetMotion { motion, .. } = operation(&row(2), &[], &modifiers, None)?
        else {
            panic!("forward motion")
        };
        assert_eq!(
            motion.forward_motion,
            Some(ForwardMotion {
                velocity: Some(11.),
                acceleration: Some(-0.25)
            })
        );
        let InputOperation::SetMotion { motion, .. } = operation(&row(3), &[], &modifiers, None)?
        else {
            panic!("stop motion")
        };
        assert_eq!(motion.velocity, [Some(0.); 3]);
        assert_eq!(motion.acceleration, [Some(0.); 3]);
        Ok(())
    }

    #[test]
    fn retained_model_animation_uses_authored_playback() -> Result<()> {
        let mut row = [0; 352];
        row[0] = 3;
        row[50] = 255;
        row[20..24].copy_from_slice(&0x40020_u32.to_be_bytes());
        let declaration = crate::battle_effect::declaration::read(&row, &[])?;
        let records = [
            Record {
                age: 75,
                command: 0,
                argument: 0,
                operand: 1,
            },
            Record {
                age: 110,
                command: 253,
                argument: 0,
                operand: 2,
            },
            Record {
                age: 120,
                command: 254,
                argument: 0,
                operand: 0,
            },
        ];
        let modifiers = BTreeMap::from([
            (1, vec![22, 0x5c, 0, 1, 0, 0, 8, 0, u16::MAX]),
            (
                2,
                vec![
                    22,
                    0x5c,
                    0,
                    2,
                    2,
                    40,
                    8,
                    0,
                    7,
                    0x38,
                    0,
                    0,
                    0,
                    0,
                    7,
                    0x44,
                    0x4170,
                    0,
                    0,
                    0,
                    u16::MAX,
                ],
            ),
        ]);
        let events = decode(&records, &[declaration.clone()], &[true], &modifiers, &[]);
        let EffectOperation::Spawn { birth, .. } = &events[0].operation else {
            panic!("model birth expected")
        };
        assert_eq!(birth.model, Some(ValueRange::fixed(0.)));
        assert_eq!(
            birth.animation.unwrap(),
            ModelAnimation {
                model: 0,
                clip: 1,
                frame: 0.,
                blend_ticks: 0,
                rate: 0.5,
                repeat: false
            }
        );
        let EffectOperation::SetMotion { spawn, motion } = &events[1].operation else {
            panic!("retained model edit expected")
        };
        assert_eq!(*spawn, 0);
        assert_eq!(motion.offset, [None, Some(0.), None]);
        assert_eq!(motion.velocity, [None, Some(15.), None]);
        let animation = motion.animation.unwrap();
        assert_eq!(
            (animation.clip, animation.blend_ticks, animation.repeat),
            (2, 2, false)
        );
        assert!((animation.rate - 0.4).abs() < 0.00001);

        let mut blended = declaration;
        let Declaration::ModelParticle { visual, .. } = &mut blended else {
            panic!("model declaration")
        };
        visual.blend = 1;
        let blended_events = decode(&records, &[blended], &[true], &modifiers, &[]);
        let EffectOperation::Spawn { birth, .. } = &blended_events[0].operation else {
            panic!("model birth expected")
        };
        assert_eq!(birth.animation.unwrap().frame, 0.5);

        let mut held = crate::battle_effect::declaration::read(&row, &[])?;
        let Declaration::ModelParticle {
            template, binding, ..
        } = &mut held
        else {
            panic!("model declaration")
        };
        template.owner_bone = Some(10);
        binding.animation = Some(0);
        binding.repeat = true;
        binding.program_animation = false;
        let words = vec![
            26,
            0x40,
            0,
            0,
            0x7ff8,
            0,
            27,
            0x40,
            0,
            0,
            0,
            0,
            8,
            0x7ff8,
            0x3f80,
            0,
            0,
            0,
            u16::MAX,
        ];
        let records = [
            Record {
                age: 0,
                command: 255,
                argument: 3,
                operand: 5,
            },
            Record {
                age: 0,
                command: 0,
                argument: 0,
                operand: 1,
            },
            Record {
                age: 15,
                command: 254,
                argument: 0,
                operand: 0,
            },
        ];
        let events = decode(
            &records,
            std::slice::from_ref(&held),
            &[false],
            &BTreeMap::from([(1, words.clone())]),
            &[],
        );
        let mut poses = Vec::new();
        for event in events {
            let EffectOperation::Spawn { birth, .. } = event.operation else {
                panic!("held model birth expected")
            };
            assert_eq!(birth.model, Some(ValueRange::fixed(0.)));
            let animation = birth.animation.unwrap();
            assert_eq!(
                (animation.clip, animation.rate, animation.repeat),
                (0, 0., true)
            );
            poses.push((event.at, animation.frame));
        }
        assert_eq!(poses, [(0, 0.), (5, 1.), (10, 2.)]);
        for (index, value) in [(1, 0x41), (4, 0x7ffc), (8, 0x3f80)] {
            let mut unsupported = words.clone();
            unsupported[index] = value;
            assert!(edits_for(&unsupported, Some(&held), None).is_err());
        }
        assert!(edits_for(&words[6..], Some(&held), None).is_err());
        Ok(())
    }

    #[test]
    fn delayed_motion_selects_retained_births() {
        let row = |age, command, operand| Record {
            age,
            command,
            argument: 0,
            operand,
        };
        let records = [
            row(12, 0, 0),
            row(20, 0, 0),
            row(20, 1, 0),
            row(25, 0, 0),
            row(58, 253, 1),
            row(59, 254, 0),
        ];
        let modifiers = [(
            1,
            vec![
                7,
                0xc4,
                0x40a0,
                0,
                0,
                0,
                7,
                0xd0,
                0x3f80,
                0,
                0,
                0,
                7,
                0x44,
                0xbf00,
                0,
                0,
                0,
                7,
                0xb4,
                0x4200,
                0,
                0,
                0,
                7,
                0xb0,
                0x4200,
                0,
                0,
                0,
                u16::MAX,
            ],
        )]
        .into();
        for (retained, selector, expected) in [
            ([false, true], 0, 2),
            ([true, false], 0, 0),
            ([true, false], 1, 1),
            ([true, true], 2, 2),
        ] {
            let mut records = records;
            records[4].argument = selector;
            let events = decode(&records, &[], &retained, &modifiers, &[]);
            assert_eq!(
                events.last(),
                Some(&ScheduledEvent {
                    at: 58,
                    operation: EffectOperation::SetMotion {
                        spawn: expected,
                        motion: Box::new(MotionChange {
                            size: [Some(32.), Some(32.), None],
                            velocity: [None, Some(-0.5), None],
                            size_velocity: [None, None, Some(5.)],
                            size_acceleration: [None, None, Some(1.)],
                            ..Default::default()
                        }),
                    },
                })
            );
        }
        let events = decode(&records, &[], &[false, false], &modifiers, &[]);
        assert!(matches!(
            events[0].operation,
            EffectOperation::Unsupported { .. }
        ));
        let mut records = records;
        records[4].argument = 1;
        let events = decode(&records, &[], &[false, true], &modifiers, &[]);
        assert!(matches!(
            events[0].operation,
            EffectOperation::Unsupported { .. }
        ));
    }

    #[test]
    fn model_texture_frame_selects_a_checked_uv_row() -> Result<()> {
        let mut row = [0; 352];
        row[0] = 3;
        row[50] = 255;
        row[140] = 2;
        let declaration = crate::battle_effect::declaration::read(&row, &[])?;
        assert_eq!(declaration.template()?.state.uv, [0, 0, 0, 2]);
        let events = decode(
            &[
                Record {
                    age: 0,
                    command: 0,
                    argument: 0,
                    operand: 1,
                },
                Record {
                    age: 0,
                    command: 254,
                    argument: 0,
                    operand: 0,
                },
            ],
            std::slice::from_ref(&declaration),
            &[false],
            &[(1, vec![10, 0x8d, 1, 0, u16::MAX])].into(),
            &[],
        );
        let EffectOperation::Spawn { birth, .. } = &events[0].operation else {
            panic!("checked model row birth");
        };
        assert_eq!(birth.uv[1], Some(ValueRange::fixed(1.)));
        assert!(edits_for(&[10, 0x8d, 2, 0, u16::MAX], Some(&declaration), None).is_err());
        row[140] = 0;
        let plain = crate::battle_effect::declaration::read(&row, &[])?;
        assert!(edits_for(&[10, 0x8d, 0, 0, u16::MAX], Some(&plain), None).is_err());
        row[140] = 2;
        row[141] = 2;
        assert!(
            crate::battle_effect::declaration::read(&row, &[])?
                .template()
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn pollen_color_assignment_matches_recorded_components() -> Result<()> {
        let decoded = edits(&[18, 0x18, 48, 32, 128, 128, u16::MAX])?;
        assert_eq!(decoded.len(), 4);
        for (channel, value) in [48, 32, 128, 128].into_iter().enumerate() {
            assert_eq!(
                decoded[channel],
                InputEdit::Integer {
                    target: InputIntegerTarget::Color {
                        color: 0,
                        channel: channel as u8
                    },
                    arithmetic: Arithmetic::Set,
                    value: InputIntegerValue::Literal { value },
                }
            );
        }
        assert!(edits(&[18, 0x1a, 48, 32, 128, 128, u16::MAX]).is_err());
        Ok(())
    }

    #[test]
    fn arena_vector_assignment_uses_existing_scalar_settings() -> Result<()> {
        let values = [-600_f32, 200., -1400.];
        let mut words = vec![1, 0x34];
        for value in values {
            words.extend([(value.to_bits() >> 16) as u16, value.to_bits() as u16]);
        }
        words.push(u16::MAX);
        let decoded = edits(&words)?;
        assert_eq!(decoded.len(), 3);
        for (axis, value) in values.into_iter().enumerate() {
            assert_eq!(
                decoded[axis],
                InputEdit::Scalar {
                    target: InputScalarTarget::Vector {
                        vector: VectorTarget::Offset,
                        axis: axis as u8
                    },
                    arithmetic: Arithmetic::Set,
                    value: InputScalarValue::Literal { value },
                }
            );
        }
        words[1] = 0x38;
        assert!(edits(&words).is_err());
        words[1] = 0x34;
        words[2] = (f32::NAN.to_bits() >> 16) as u16;
        assert!(edits(&words).is_err());
        Ok(())
    }

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
                    operation(&record, &[], &modifiers, None).is_ok(),
                    value <= maximum,
                    "target {target}, value {value}"
                );
            }
        }
    }
}
