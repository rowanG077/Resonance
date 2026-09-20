//! Physical modifier operands retain inactive selectors and alignment storage.
use crate::read::{FloatOperand, u16 as half, u32 as word};
use anyhow::{Context, Result, bail};
use resonance_content::battle::effect_inventory::{EffectArithmetic, EffectIntegerWidth};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Floating {
    pub literal: FloatOperand,
    pub selector: i16,
    pub storage: u16,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Operation {
    Integer {
        width: EffectIntegerWidth,
        operation: EffectArithmetic,
        value: i16,
        storage: u16,
    },
    Float {
        operation: EffectArithmetic,
        value: Floating,
    },
    RandomInteger {
        modulus: i16,
        storage: u16,
    },
    RandomFloat {
        literal: i16,
        selector: i16,
    },
    SetVector {
        value: [FloatOperand; 3],
    },
    RandomPolarVector {
        angles: [FloatOperand; 3],
        radius: FloatOperand,
        radius_jitter: i16,
        angle_jitter: i16,
    },
    RotateVector {
        axis: i16,
        angle: i16,
    },
    PolarVector {
        radius: i16,
        angle: i16,
    },
    SetColor {
        color: [i16; 4],
    },
    ClearFlags {
        flags: u32,
    },
    SetFlags {
        flags: u32,
    },
    TranslateVertices {
        translation: [FloatOperand; 3],
    },
    ModelAnimation {
        model: i16,
        clip: i16,
        blend_ticks: i16,
        rate_percent: i16,
        flags: u16,
        storage: u16,
    },
    AnimationPosition {
        value: Floating,
    },
    AnimationRate {
        value: Floating,
    },
    EmitterAxisVector {
        axis: u8,
        value: Floating,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Instruction {
    /// Signed offset from the output, or 0x7ff8..0x7fff temporary selector.
    /// Some operations ignore this operand; physical decoding does not bind it.
    pub destination: i16,
    pub operation: Operation,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Record {
    End,
    Instruction { instruction: Instruction },
}

pub(crate) fn read(bytes: &[u8], at: usize) -> Result<(Record, usize)> {
    let opcode = half(bytes, at)?;
    if opcode == u16::MAX {
        return Ok((Record::End, at + 2));
    }
    let short = |offset| half(bytes, at + offset).map(|value| value as i16);
    let literal = |offset| word(bytes, at + offset).map(FloatOperand::from_bits);
    let vector = |offset| -> Result<_> {
        Ok([literal(offset)?, literal(offset + 4)?, literal(offset + 8)?])
    };
    let floating = || -> Result<_> {
        Ok(Floating {
            literal: literal(4)?,
            selector: short(8)?,
            storage: half(bytes, at + 10)?,
        })
    };
    let arithmetic = |opcode| match opcode {
        0 | 7 | 10 => EffectArithmetic::Set,
        3 | 8 | 12 => EffectArithmetic::Add,
        4 | 13 | 23 => EffectArithmetic::Subtract,
        5 | 14 | 24 => EffectArithmetic::Multiply,
        _ => EffectArithmetic::Divide,
    };
    let (operation, size) = match opcode {
        0 | 3..=6 | 10 | 12 | 23..=25 => (
            Operation::Integer {
                width: if matches!(opcode, 10 | 12 | 23..=25) {
                    EffectIntegerWidth::Byte
                } else {
                    EffectIntegerWidth::Halfword
                },
                operation: arithmetic(opcode),
                value: short(4)?,
                storage: half(bytes, at + 6)?,
            },
            8,
        ),
        1 => (Operation::SetVector { value: vector(4)? }, 16),
        2 => (
            Operation::RandomInteger {
                modulus: short(4)?,
                storage: half(bytes, at + 6)?,
            },
            8,
        ),
        7 | 8 | 13..=15 => (
            Operation::Float {
                operation: arithmetic(opcode),
                value: floating()?,
            },
            12,
        ),
        9 => (
            Operation::RandomPolarVector {
                angles: vector(4)?,
                radius: literal(16)?,
                radius_jitter: short(20)?,
                angle_jitter: short(22)?,
            },
            24,
        ),
        11 => (
            Operation::RandomFloat {
                literal: short(4)?,
                selector: short(6)?,
            },
            8,
        ),
        16 => (
            Operation::RotateVector {
                axis: short(4)?,
                angle: short(6)?,
            },
            8,
        ),
        17 => (
            Operation::PolarVector {
                radius: short(4)?,
                angle: short(6)?,
            },
            8,
        ),
        18 => (
            Operation::SetColor {
                color: [short(4)?, short(6)?, short(8)?, short(10)?],
            },
            12,
        ),
        19 => (
            Operation::ClearFlags {
                flags: word(bytes, at + 4)?,
            },
            8,
        ),
        20 => (
            Operation::TranslateVertices {
                translation: vector(4)?,
            },
            16,
        ),
        21 => (
            Operation::SetFlags {
                flags: word(bytes, at + 4)?,
            },
            8,
        ),
        22 => (
            Operation::ModelAnimation {
                model: short(4)?,
                clip: short(6)?,
                blend_ticks: short(8)?,
                rate_percent: short(10)?,
                flags: half(bytes, at + 12)?,
                storage: half(bytes, at + 14)?,
            },
            16,
        ),
        26 => (Operation::AnimationPosition { value: floating()? }, 12),
        27 => (Operation::AnimationRate { value: floating()? }, 12),
        28..=30 => (
            Operation::EmitterAxisVector {
                axis: (opcode - 28) as u8,
                value: floating()?,
            },
            12,
        ),
        _ => bail!("unknown authored effect modifier {opcode} at {at:#x}; record width is unknown"),
    };
    bytes
        .get(at..at + size)
        .context("truncated authored effect modifier")?;
    Ok((
        Record::Instruction {
            instruction: Instruction {
                destination: short(2)?,
                operation,
            },
        },
        at + size,
    ))
}

#[cfg(test)]
impl Record {
    pub(super) fn bytes(&self) -> Vec<u8> {
        let Self::Instruction { instruction } = self else {
            return u16::MAX.to_be_bytes().to_vec();
        };
        let halfwords = |values: &[i16]| {
            values
                .iter()
                .flat_map(|value| value.to_be_bytes())
                .collect::<Vec<_>>()
        };
        let vector = |values: &[FloatOperand]| {
            values
                .iter()
                .flat_map(|value| value.bits().to_be_bytes())
                .collect::<Vec<_>>()
        };
        let floating = |value: &Floating| {
            [
                value.literal.bits().to_be_bytes().as_slice(),
                &value.selector.to_be_bytes(),
                &value.storage.to_be_bytes(),
            ]
            .concat()
        };
        let arithmetic = |operation: EffectArithmetic, opcodes: [u16; 5]| {
            opcodes[match operation {
                EffectArithmetic::Set => 0,
                EffectArithmetic::Add => 1,
                EffectArithmetic::Subtract => 2,
                EffectArithmetic::Multiply => 3,
                EffectArithmetic::Divide => 4,
            }]
        };
        let (opcode, operands) = match &instruction.operation {
            Operation::Integer {
                width,
                operation,
                value,
                storage,
            } => (
                arithmetic(
                    *operation,
                    match width {
                        EffectIntegerWidth::Byte => [10, 12, 23, 24, 25],
                        EffectIntegerWidth::Halfword => [0, 3, 4, 5, 6],
                    },
                ),
                halfwords(&[*value, *storage as i16]),
            ),
            Operation::Float { operation, value } => {
                (arithmetic(*operation, [7, 8, 13, 14, 15]), floating(value))
            }
            Operation::RandomInteger { modulus, storage } => {
                (2, halfwords(&[*modulus, *storage as i16]))
            }
            Operation::RandomFloat { literal, selector } => (11, halfwords(&[*literal, *selector])),
            Operation::SetVector { value } => (1, vector(value)),
            Operation::RandomPolarVector {
                angles,
                radius,
                radius_jitter,
                angle_jitter,
            } => (
                9,
                [
                    vector(angles),
                    vector(std::slice::from_ref(radius)),
                    halfwords(&[*radius_jitter, *angle_jitter]),
                ]
                .concat(),
            ),
            Operation::RotateVector { axis, angle } => (16, halfwords(&[*axis, *angle])),
            Operation::PolarVector { radius, angle } => (17, halfwords(&[*radius, *angle])),
            Operation::SetColor { color } => (18, halfwords(color)),
            Operation::ClearFlags { flags } => (19, flags.to_be_bytes().to_vec()),
            Operation::SetFlags { flags } => (21, flags.to_be_bytes().to_vec()),
            Operation::TranslateVertices { translation } => (20, vector(translation)),
            Operation::ModelAnimation {
                model,
                clip,
                blend_ticks,
                rate_percent,
                flags,
                storage,
            } => (
                22,
                halfwords(&[
                    *model,
                    *clip,
                    *blend_ticks,
                    *rate_percent,
                    *flags as i16,
                    *storage as i16,
                ]),
            ),
            Operation::AnimationPosition { value } => (26, floating(value)),
            Operation::AnimationRate { value } => (27, floating(value)),
            Operation::EmitterAxisVector { axis, value } => {
                (28 + u16::from(*axis), floating(value))
            }
        };
        [
            opcode.to_be_bytes().as_slice(),
            &instruction.destination.to_be_bytes(),
            &operands,
        ]
        .concat()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_modifier_layouts_preserve_inactive_operands_and_float_bits() -> Result<()> {
        // Native cursor increments, independent of the record decoder.
        let sizes = [
            8, 16, 8, 8, 8, 8, 8, 12, 12, 24, 8, 8, 8, 12, 12, 12, 8, 8, 12, 8, 16, 8, 16, 8, 8, 8,
            12, 12, 12, 12, 12,
        ];
        for (opcode, size) in sizes.into_iter().enumerate() {
            let mut bytes: Vec<_> = (0..size).map(|index| (index * 53 + 29) as u8).collect();
            bytes[..2].copy_from_slice(&(opcode as u16).to_be_bytes());
            bytes[2..4].copy_from_slice(&(-31i16).to_be_bytes());
            if matches!(opcode, 1 | 7..=9 | 13..=15 | 20 | 26..=30) {
                bytes[4..8].copy_from_slice(&0x7fa12345u32.to_be_bytes());
            }
            if matches!(opcode, 7 | 8 | 13..=15 | 26..=30) {
                bytes[8..10].copy_from_slice(&0x7ff8u16.to_be_bytes());
            }
            let (record, next) = read(&bytes, 0)?;
            assert_eq!(next, size);
            let published: Record = serde_json::from_value(serde_json::to_value(record)?)?;
            assert_eq!(published.bytes(), bytes, "opcode {opcode}");
            assert!(read(&bytes[..size - 1], 0).is_err());
        }
        for selector in [i16::MIN, -1, 0, 1, 0x7ff7, 0x7ff8, 0x7ffb, 0x7ffc, 0x7fff] {
            for bits in [
                0x80000000u32,
                0x7f800000,
                0xff800000,
                0x7fa12345,
                0x3fc00000,
            ] {
                let bytes = [
                    7u16.to_be_bytes().as_slice(),
                    &(-1i16).to_be_bytes(),
                    &bits.to_be_bytes(),
                    &selector.to_be_bytes(),
                    &[0xa5, 0x5a],
                ]
                .concat();
                let (record, _) = read(&bytes, 0)?;
                let restored: Record = serde_json::from_value(serde_json::to_value(record)?)?;
                assert_eq!(restored.bytes(), bytes);
            }
            let bytes = [
                11u16.to_be_bytes().as_slice(),
                &[0xff, 0xff, 0xa5, 0x5a],
                &selector.to_be_bytes(),
            ]
            .concat();
            assert_eq!(read(&bytes, 0)?.0.bytes(), bytes);
        }
        assert_eq!(read(&[255, 255], 0)?.0.bytes(), [255, 255]);
        assert!(read(&[0, 31, 0, 0], 0).is_err());
        assert!(read(&[255], 0).is_err());
        // Selected execution reads a literal only when the selector chooses it.
        let mut stream = vec![0; 4];
        stream.extend([0, 7, 0, 0x40]);
        stream.extend(0x7fa12345u32.to_be_bytes());
        stream.extend([0x7f, 0xf8, 0xa5, 0x5a, 255, 255]);
        let selected = super::super::decode_modifiers(&stream, 4, false)?;
        assert!(matches!(
            selected.as_slice(),
            [super::super::Modifier::Float {
                value: super::super::FloatValue::Temporary(0),
                ..
            }]
        ));
        stream[12..14].fill(0);
        assert!(read(&stream, 4).is_ok());
        assert!(super::super::decode_modifiers(&stream, 4, false).is_err());
        Ok(())
    }
}
