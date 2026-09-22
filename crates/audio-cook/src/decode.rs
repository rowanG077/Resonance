//! Decode authored instrument operations independently of mixer support.
//! All sample payloads become PCM; references and operands become typed data.
use crate::{
    bank::{Bank, MissingObject, ObjectKind, Sample},
    compile, read,
};
use anyhow::{Context, Result, ensure};
pub use resonance_audio::data::PanAxis as Axis;
use resonance_audio::data::{
    Arithmetic, Command, Comparison, ControlTarget, Controller, Operand, Variable, VolumeCurve,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", content = "instruction", rename_all = "snake_case")]
pub enum Instruction {
    Mixer(Command),
    Native(Native),
}

impl Instruction {
    /// Lower supported operands without changing instruction indices or discarding
    /// the richer physical record. Unsupported operands remain a preparation error.
    pub fn mixer(self) -> Result<Command> {
        let operation = match self {
            Self::Mixer(command) => return Ok(command),
            Self::Native(operation) => operation,
        };
        Ok(match operation {
            Native::Noop => Command::Noop,
            Native::SetVariable { destination, value } => Command::SetVariable {
                destination: destination.destination()?,
                value,
            },
            Native::Calculate {
                destination,
                operation,
                left,
                right,
            } => Command::Calculate {
                destination: destination.destination()?,
                operation,
                left: left.lower()?,
                right: match right {
                    VariableOperand::Variable(value) => Operand::Variable(value.lower()?),
                    VariableOperand::Constant(value) => Operand::Constant(value),
                },
            },
            Native::Branch {
                comparison,
                left,
                right,
                invert,
                instruction,
            } => Command::Branch {
                comparison,
                left: left.lower()?,
                right: right.lower()?,
                invert,
                instruction,
            },
            Native::PanRamp {
                axis,
                initial,
                delta,
                milliseconds,
            } => Command::PanRamp {
                axis,
                initial,
                delta,
                milliseconds,
            },
            Native::Wait {
                duration,
                clock,
                random: false,
                from_start,
                key_off,
                sample_end,
            } => {
                let duration = (duration != u16::MAX).then_some(duration);
                match clock {
                    Clock::Milliseconds => Command::Wait {
                        milliseconds: duration,
                        from_start,
                        key_off,
                        sample_end,
                    },
                    Clock::Beats => {
                        ensure!(
                            !from_start,
                            "absolute beat waits require the global synthesizer clock"
                        );
                        Command::BeatWait {
                            ticks: duration,
                            key_off,
                            sample_end,
                        }
                    }
                }
            }
            Native::Wait {
                duration,
                clock: Clock::Milliseconds,
                random: true,
                from_start: false,
                key_off,
                sample_end,
            } => Command::RandomWait {
                upper_ms: duration,
                key_off,
                sample_end,
            },
            Native::Loop {
                instruction,
                count,
                random: false,
                key_off,
                sample_end,
            } => Command::Loop {
                instruction,
                count,
                key_off,
                sample_end,
            },
            Native::Loop {
                instruction,
                count,
                random: true,
                key_off,
                sample_end,
            } => {
                ensure!(count != 0, "random loop requires a nonzero bound");
                Command::RandomLoop {
                    instruction,
                    count,
                    key_off,
                    sample_end,
                }
            }
            Native::RandomBranch {
                minimum,
                program,
                instruction,
            } => Command::RandomBranch {
                minimum,
                program,
                instruction,
            },
            Native::RandomNote {
                low,
                high,
                cents,
                random_cents,
                relative,
            } => Command::RandomNote {
                low,
                high,
                cents,
                random_cents,
                relative,
            },
            // Ordinary ADPCM starts from its first predictor block regardless
            // of the requested offset. Other hardware encodings have different
            // offset rules and require their own sample recovery.
            Native::StartSample {
                sample, format: 0, ..
            } if sample != u16::MAX => Command::StartSample { sample },
            Native::SetVolume {
                factor,
                offset,
                curve,
                from_velocity,
            } => Command::SetVolume {
                factor,
                offset,
                curve,
                from_velocity,
            },
            Native::FadeVolume {
                factor,
                offset,
                curve,
                duration,
                clock: Clock::Milliseconds,
                from_silence,
            } => Command::FadeVolume {
                factor,
                offset,
                curve,
                milliseconds: duration,
                from_silence,
            },
            // A zero period disables vibrato before the source reads either depth.
            Native::Vibrato {
                duration: 0,
                scale_by_modulation,
                ..
            } => Command::Vibrato {
                period_ms: 0,
                depth_8: 0,
                reverse: false,
                scale_by_modulation,
            },
            Native::Vibrato {
                duration,
                clock: Clock::Milliseconds,
                semitones,
                cents,
                scale_by_modulation,
            } => {
                let reverse = semitones < 0 || semitones == 0 && cents < 0;
                let (semitones, cents) = if reverse {
                    (semitones.unsigned_abs(), cents.unsigned_abs())
                } else if cents < 0 {
                    // The stored fine byte can exceed 127; playback reads it unsigned.
                    ((semitones - 1) as u8, (100 - i16::from(cents)) as u8)
                } else {
                    (semitones as u8, cents as u8)
                };
                Command::Vibrato {
                    period_ms: duration,
                    depth_8: u16::from(semitones) * 256 + u16::from(cents) * 256 / 100,
                    reverse,
                    scale_by_modulation,
                }
            }
            Native::SelectController {
                destination,
                source,
                scale_16,
                combine: 0,
                variable: true,
            } => Command::SelectControl {
                target: match destination {
                    Destination::Volume => ControlTarget::Volume,
                    Destination::Pan => ControlTarget::Pan,
                    Destination::SurroundPan => ControlTarget::SurroundPan,
                    Destination::Reverb => ControlTarget::PostAuxiliaryA,
                    Destination::PreAuxiliaryA => ControlTarget::PreAuxiliaryA,
                    Destination::PreAuxiliaryB => ControlTarget::PreAuxiliaryB,
                    Destination::PostAuxiliaryB => ControlTarget::PostAuxiliaryB,
                    unsupported => {
                        anyhow::bail!("unsupported variable selector destination: {unsupported:?}")
                    }
                },
                // A zero scale never needs the variable's value or shared state.
                source: if scale_16 == 0 {
                    Operand::Constant(0)
                } else {
                    Operand::Variable(VariableSource::Register(source).lower()?)
                },
                scale: scale_16,
            },
            Native::SelectController {
                destination: Destination::Tremolo,
                source: 130,
                scale_16: 65536,
                combine: 0,
                variable: false,
            } => Command::TremoloFromLfo,
            unsupported => anyhow::bail!("unsupported mixer operation: {unsupported:?}"),
        })
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Clock {
    Milliseconds,
    Beats,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Destination {
    Volume,
    Pan,
    PitchBend,
    Modulation,
    Sustain,
    Portamento,
    Reverb,
    SurroundPan,
    Doppler,
    Tremolo,
    PreAuxiliaryA,
    PreAuxiliaryB,
    PostAuxiliaryB,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SampleOffset {
    Constant,
    InverseVolume,
    Volume,
    /// The native default branch resets the requested offset to zero.
    Unrecognized(u8),
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VariableSource {
    Register(u8),
    Controller(u8),
}

impl VariableSource {
    fn read(controller: u8, index: u8) -> Self {
        if controller == 0 {
            Self::Register(index)
        } else {
            Self::Controller(index)
        }
    }

    fn lower(self) -> Result<Variable> {
        match self {
            Self::Register(index) => Ok(if index & 31 < 16 {
                Variable::Local(index & 15)
            } else {
                Variable::Global(index & 15)
            }),
            Self::Controller(index) => Ok(Variable::Controller(match index {
                0..=0x3f => Controller::Paired(index & 31),
                0x80 | 0x81 => Controller::PitchBend,
                0x84 | 0x85 => Controller::Surround,
                0x82 | 0xa0 => Controller::Lfo,
                _ => anyhow::bail!("macro controller register {index} is not implemented"),
            })),
        }
    }
    fn destination(self) -> Result<Variable> {
        let variable = self.lower()?;
        ensure!(
            !matches!(variable, Variable::Controller(Controller::Paired(6))),
            "RPN data-entry controller writes are not implemented"
        );
        Ok(variable)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VariableOperand {
    Variable(VariableSource),
    Constant(i16),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum Native {
    Noop,
    SetVariable {
        destination: VariableSource,
        value: i16,
    },
    Calculate {
        destination: VariableSource,
        #[serde(rename = "arithmetic")]
        operation: Arithmetic,
        left: VariableSource,
        right: VariableOperand,
    },
    Branch {
        comparison: Comparison,
        left: VariableSource,
        right: VariableSource,
        invert: bool,
        instruction: usize,
    },
    Wait {
        duration: u16,
        clock: Clock,
        random: bool,
        from_start: bool,
        key_off: bool,
        sample_end: bool,
    },
    Loop {
        instruction: usize,
        count: u16,
        random: bool,
        key_off: bool,
        sample_end: bool,
    },
    RandomBranch {
        minimum: u8,
        program: u16,
        instruction: usize,
    },
    StartSample {
        sample: u16,
        format: u8,
        offset: u32,
        offset_mode: SampleOffset,
    },
    PanRamp {
        axis: Axis,
        initial: u8,
        delta: i8,
        milliseconds: u16,
    },
    RandomNote {
        low: u8,
        high: u8,
        cents: i8,
        random_cents: bool,
        relative: bool,
    },
    Vibrato {
        duration: u16,
        clock: Clock,
        semitones: i8,
        cents: i8,
        scale_by_modulation: bool,
    },
    SelectController {
        destination: Destination,
        source: u8,
        scale_16: i32,
        combine: u8,
        variable: bool,
    },
    SetVolume {
        factor: u8,
        offset: u8,
        curve: Option<VolumeCurve>,
        from_velocity: bool,
    },
    FadeVolume {
        factor: u8,
        offset: u8,
        curve: Option<VolumeCurve>,
        duration: u16,
        clock: Clock,
        from_silence: bool,
    },
}

pub struct Resources {
    pub programs: BTreeMap<u16, Vec<Instruction>>,
    pub samples: BTreeMap<u16, Arc<Sample>>,
}

pub fn programs(bank: &Bank<'_>, roots: impl IntoIterator<Item = u16>) -> Result<Resources> {
    let mut pending: Vec<_> = roots.into_iter().collect();
    let mut resources = Resources {
        programs: BTreeMap::new(),
        samples: BTreeMap::new(),
    };
    while let Some(id) = pending.pop() {
        if resources.programs.contains_key(&id) {
            continue;
        }
        let bytes = bank.object(ObjectKind::Macro, id)?;
        ensure!(
            !bytes.is_empty() && bytes.len().is_multiple_of(8) && bytes.len() <= 65536 * 8,
            "invalid instrument {id} length"
        );
        let mut program = Vec::new();
        for (pc, bytes) in bytes.chunks_exact(8).enumerate() {
            let instruction = command(bank, read::u32(bytes, 0)?, read::u32(bytes, 4)?)
                .with_context(|| format!("instrument {id}, instruction {pc}"))?;
            match &instruction {
                Instruction::Mixer(
                    Command::Jump { program, .. } | Command::KeyOffTrap { program, .. },
                ) => pending.push(*program),
                Instruction::Mixer(
                    Command::RandomBranch { program, .. }
                    | Command::SpawnMacro { program, .. }
                    | Command::MessageTrap { program, .. },
                )
                | Instruction::Native(Native::RandomBranch { program, .. })
                    if bank.object(ObjectKind::Macro, *program).is_ok() =>
                {
                    pending.push(*program)
                }
                Instruction::Mixer(Command::StartSample { sample })
                | Instruction::Native(Native::StartSample { sample, .. }) => {
                    if let std::collections::btree_map::Entry::Vacant(entry) =
                        resources.samples.entry(*sample)
                    {
                        entry.insert(bank.sample(*sample)?.into());
                    }
                }
                _ => {}
            }
            program.push(instruction);
        }
        resources.programs.insert(id, program);
    }
    for (&id, program) in &resources.programs {
        for instruction in program {
            let target = match instruction {
                Instruction::Mixer(
                    Command::Jump {
                        program,
                        instruction,
                    }
                    | Command::KeyOffTrap {
                        program,
                        instruction,
                    },
                ) => Some((*program, *instruction)),
                Instruction::Mixer(Command::RandomBranch {
                    program,
                    instruction,
                    ..
                })
                | Instruction::Mixer(Command::MessageTrap {
                    program,
                    instruction,
                })
                | Instruction::Native(Native::RandomBranch {
                    program,
                    instruction,
                    ..
                }) if resources.programs.contains_key(program) => Some((*program, *instruction)),
                Instruction::Mixer(
                    Command::Loop { instruction, .. }
                    | Command::RandomLoop { instruction, .. }
                    | Command::Branch { instruction, .. },
                )
                | Instruction::Native(
                    Native::Loop { instruction, .. } | Native::Branch { instruction, .. },
                ) => Some((id, *instruction)),
                Instruction::Mixer(Command::SpawnMacro {
                    program,
                    instruction,
                    ..
                }) if resources.programs.contains_key(program) => {
                    Some((*program, usize::from(*instruction)))
                }
                _ => None,
            };
            if let Some((target, pc)) = target {
                ensure!(
                    resources
                        .programs
                        .get(&target)
                        .is_some_and(|program| pc < program.len()),
                    "instrument {id} has invalid branch target {target}:{pc}"
                );
            }
        }
    }
    Ok(resources)
}

pub(crate) fn command(bank: &Bank<'_>, a: u32, b: u32) -> Result<Instruction> {
    let a = a & !0x80; // The dispatch byte reserves its high bit.
    let clock = if b >> 8 & 1 != 0 {
        Clock::Milliseconds
    } else {
        Clock::Beats
    };
    let native = match a as u8 {
        0x60..=0x64 => Native::Calculate {
            destination: VariableSource::read((a >> 8) as u8, (a >> 16) as u8),
            operation: match a as u8 {
                0x61 => Arithmetic::Subtract,
                0x62 => Arithmetic::Multiply,
                0x63 => Arithmetic::Divide,
                _ => Arithmetic::Add,
            },
            left: VariableSource::read((a >> 24) as u8, b as u8),
            right: if a as u8 == 0x64 {
                VariableOperand::Constant((b >> 8) as i16)
            } else {
                VariableOperand::Variable(VariableSource::read((b >> 8) as u8, (b >> 16) as u8))
            },
        },
        0x65 => Native::SetVariable {
            destination: VariableSource::read((a >> 8) as u8, (a >> 16) as u8),
            value: b as i16,
        },
        0x70 | 0x71 => Native::Branch {
            comparison: if a as u8 == 0x70 {
                Comparison::Equal
            } else {
                Comparison::Less
            },
            left: VariableSource::read((a >> 8) as u8, (a >> 16) as u8),
            right: VariableSource::read((a >> 24) as u8, b as u8),
            invert: (b >> 8) as u8 != 0,
            instruction: (b >> 16) as usize,
        },
        0x04 | 0x07 => Native::Wait {
            duration: (b >> 16) as u16,
            clock: if a as u8 == 7 {
                Clock::Milliseconds
            } else {
                clock
            },
            random: a >> 16 & 1 != 0,
            from_start: b & 1 != 0,
            key_off: a >> 8 & 1 != 0,
            sample_end: a >> 24 & 1 != 0,
        },
        0x05 => Native::Loop {
            instruction: b as u16 as usize,
            count: (b >> 16) as u16,
            random: a >> 16 & 1 != 0,
            key_off: a >> 8 & 1 != 0,
            sample_end: a >> 24 & 1 != 0,
        },
        // An invalid sample handle leaves the existing voice unchanged.
        0x10 if (a >> 8) as u16 == u16::MAX => Native::Noop,
        0x10 => Native::StartSample {
            sample: (a >> 8) as u16,
            format: bank.sample_format((a >> 8) as u16)?,
            offset: b,
            offset_mode: match a >> 24 {
                0 => SampleOffset::Constant,
                1 => SampleOffset::InverseVolume,
                2 => SampleOffset::Volume,
                mode => SampleOffset::Unrecognized(mode as u8),
            },
        },
        0x13 => Native::RandomBranch {
            minimum: (a >> 8) as u8,
            program: (a >> 16) as u16,
            instruction: b as u16 as usize,
        },
        0x0e | 0x15 => Native::PanRamp {
            axis: if a as u8 == 0x0e {
                Axis::Pan
            } else {
                Axis::Surround
            },
            initial: (a >> 8) as u8,
            delta: b as i8,
            milliseconds: (a >> 16) as u16,
        },
        0x17 => Native::RandomNote {
            low: (a >> 8) as u8,
            high: (a >> 24) as u8,
            cents: (a >> 16) as i8,
            random_cents: b as u8 != 0,
            relative: (b >> 8) as u8 != 0,
        },
        0x1c => Native::Vibrato {
            duration: (b >> 16) as u16,
            clock,
            semitones: (a >> 8) as i8,
            cents: (a >> 16) as i8,
            scale_by_modulation: a >> 24 & 3 != 0,
        },
        0x40..=0x4c => {
            let destination = match a as u8 {
                0x40 => Destination::Volume,
                0x41 => Destination::Pan,
                0x42 => Destination::PitchBend,
                0x43 => Destination::Modulation,
                0x44 => Destination::Sustain,
                0x45 => Destination::Portamento,
                0x46 => Destination::Reverb,
                0x47 => Destination::SurroundPan,
                0x48 => Destination::Doppler,
                0x49 => Destination::Tremolo,
                0x4a => Destination::PreAuxiliaryA,
                0x4b => Destination::PreAuxiliaryB,
                _ => Destination::PostAuxiliaryB,
            };
            let coarse = i32::from((a >> 16) as i16) * 65536 / 100;
            let fine = i32::from((b >> 16) as i8) * 256 / 100;
            Native::SelectController {
                destination,
                source: (a >> 8) as u8,
                scale_16: if coarse < 0 {
                    coarse - fine
                } else {
                    coarse + fine
                },
                combine: b as u8,
                variable: (b >> 8) as u8 != 0,
            }
        }
        0x0d | 0x0f | 0x14 => {
            let curve_id = ((a >> 24) | ((b & 255) << 8)) as u16;
            let curve = if curve_id == u16::MAX {
                None
            } else {
                match bank.volume_curve(curve_id) {
                    Ok(values) => Some(VolumeCurve(values.try_into()?)),
                    Err(error) if error.downcast_ref::<MissingObject>().is_some() => None,
                    Err(error) => return Err(error),
                }
            };
            let factor = (a >> 8) as u8;
            let offset = (a >> 16) as u8;
            if a as u8 == 0x0d {
                Native::SetVolume {
                    factor,
                    offset,
                    curve,
                    from_velocity: (b >> 8) as u8 != 0,
                }
            } else {
                Native::FadeVolume {
                    factor,
                    offset,
                    curve,
                    duration: (b >> 16) as u16,
                    clock,
                    from_silence: a as u8 == 0x14,
                }
            }
        }
        _ => return Ok(Instruction::Mixer(compile::command(bank, a, b)?)),
    };
    Ok(Instruction::Native(native))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_vibrato_lowers_normalized_depth_and_phase() -> Result<()> {
        let mut descriptor = [0; 32];
        descriptor[..4].copy_from_slice(&u32::MAX.to_be_bytes());
        let bank = Bank::from_sections([&descriptor, &[], &[], &[]])?;
        for (semitones, cents, expected_depth, expected_reverse) in [
            (0i8, 10i8, 25, false),
            (0, -10, 25, true),
            (3, -99, 1021, false),
            (-3, -99, 1021, true),
            (-3, 99, 1021, true),
            (-128, -128, 33095, true),
            (127, -128, 32839, false),
            (1, -128, 583, false),
            (1, -1, 258, false),
            (100, 99, 25853, false),
        ] {
            let a = 2 << 24 | u32::from(cents as u8) << 16 | u32::from(semitones as u8) << 8 | 0x1c;
            let b = 200 << 16 | 1 << 8;
            for actual in [
                command(&bank, a, b)?.mixer()?,
                compile::command(&bank, a, b)?,
            ] {
                let Command::Vibrato {
                    period_ms: 200,
                    depth_8,
                    reverse,
                    scale_by_modulation: true,
                } = actual
                else {
                    panic!("lost authored vibrato operands: {actual:?}");
                };
                assert_eq!((depth_8, reverse), (expected_depth, expected_reverse));
            }
        }
        Ok(())
    }

    #[test]
    fn volume_curves_lower_for_set_and_both_fades() -> Result<()> {
        let mut descriptor = [0; 32];
        descriptor[..4].copy_from_slice(&u32::MAX.to_be_bytes());
        let mut pool = [0u32, 16, 0, 0, 136, 88 << 16]
            .into_iter()
            .flat_map(u32::to_be_bytes)
            .collect::<Vec<_>>();
        let levels: Vec<_> = (0..128u8).map(|index| 255 - index).collect();
        pool.extend_from_slice(&levels);
        pool.extend_from_slice(&u32::MAX.to_be_bytes());
        let bank = Bank::from_sections([&descriptor, &pool, &[], &[]])?;
        for opcode in [0x0d, 0x0f, 0x14] {
            let a = 88 << 24 | 3 << 16 | 50 << 8 | opcode;
            let b = 10000 << 16 | 1 << 8;
            for lowered in [
                command(&bank, a, b)?.mixer()?,
                compile::command(&bank, a, b)?,
            ] {
                let curve = match lowered {
                    Command::SetVolume {
                        factor: 50,
                        offset: 3,
                        curve,
                        from_velocity: true,
                    } if opcode == 0x0d => curve,
                    Command::FadeVolume {
                        factor: 50,
                        offset: 3,
                        curve,
                        milliseconds: 10000,
                        from_silence,
                    } if from_silence == (opcode == 0x14) => curve,
                    _ => panic!("lost authored volume operands: {lowered:?}"),
                };
                assert_eq!(curve.unwrap().0.as_slice(), levels);
            }
            if opcode != 0x0d {
                assert!(command(&bank, a, b & !0x100)?.mixer().is_err());
            }
        }
        assert!(matches!(
            command(&bank, 89 << 24 | 0x0d, 0)?.mixer()?,
            Command::SetVolume { curve: None, .. }
        ));
        pool.truncate(144);
        pool[16..20].copy_from_slice(&128u32.to_be_bytes());
        pool.extend_from_slice(&u32::MAX.to_be_bytes());
        let truncated = Bank::from_sections([&descriptor, &pool, &[], &[]])?;
        assert!(command(&truncated, 88 << 24 | 0x0d, 0).is_err());
        Ok(())
    }

    #[test]
    fn pan_ramps_decode_signed_deltas_and_both_axes() -> Result<()> {
        let mut descriptor = [0; 32];
        descriptor[..4].copy_from_slice(&u32::MAX.to_be_bytes());
        let bank = Bank::from_sections([&descriptor, &[], &[], &[]])?;
        for code in [0x0e, 0x15] {
            for result in [
                command(&bank, 15000 << 16 | 98 << 8 | code, 0xbc)?.mixer()?,
                compile::command(&bank, 15000 << 16 | 98 << 8 | code, 0xbc)?,
            ] {
                let Command::PanRamp {
                    axis,
                    initial: 98,
                    delta: -68,
                    milliseconds: 15000,
                } = result
                else {
                    panic!("lost authored pan operands: {result:?}");
                };
                assert_eq!(matches!(axis, Axis::Pan), code == 0x0e);
            }
        }
        Ok(())
    }

    #[test]
    fn lowering_rejects_unimplemented_operands_and_keeps_disabled_vibrato() {
        let unsupported = [
            Native::Wait {
                duration: 10,
                clock: Clock::Beats,
                random: true,
                from_start: false,
                key_off: false,
                sample_end: false,
            },
            Native::Wait {
                duration: 10,
                clock: Clock::Milliseconds,
                random: true,
                from_start: true,
                key_off: false,
                sample_end: false,
            },
            Native::Wait {
                duration: 10,
                clock: Clock::Beats,
                random: false,
                from_start: true,
                key_off: false,
                sample_end: false,
            },
            Native::Loop {
                instruction: 0,
                count: 0,
                random: true,
                key_off: false,
                sample_end: false,
            },
            Native::StartSample {
                sample: 1,
                format: 1,
                offset: 1,
                offset_mode: SampleOffset::Constant,
            },
            Native::StartSample {
                sample: 1,
                format: 2,
                offset: 0,
                offset_mode: SampleOffset::Volume,
            },
            Native::FadeVolume {
                factor: 127,
                offset: 0,
                curve: None,
                duration: 10,
                clock: Clock::Beats,
                from_silence: false,
            },
            Native::Vibrato {
                duration: 10,
                clock: Clock::Beats,
                semitones: 1,
                cents: 0,
                scale_by_modulation: false,
            },
            Native::SelectController {
                destination: Destination::PreAuxiliaryB,
                source: 0,
                scale_16: 1,
                combine: 1,
                variable: true,
            },
        ];
        for operation in unsupported {
            assert!(Instruction::Native(operation).mixer().is_err());
        }
        assert!(matches!(
            Instruction::Native(Native::Vibrato {
                duration: 0,
                clock: Clock::Beats,
                semitones: -1,
                cents: -1,
                scale_by_modulation: true,
            })
            .mixer()
            .unwrap(),
            Command::Vibrato {
                period_ms: 0,
                depth_8: 0,
                reverse: false,
                scale_by_modulation: true,
            }
        ));
    }

    #[test]
    fn random_operands_lower_and_missing_conditional_targets_remain_legal() -> Result<()> {
        let mut descriptor = [0; 32];
        descriptor[..4].copy_from_slice(&u32::MAX.to_be_bytes());
        let words = [
            16,
            0,
            0,
            0,
            40,
            7 << 16,
            8 << 16 | 118 << 8 | 0x13,
            9,
            66 << 24 | 0xf9 << 16 | 53 << 8 | 0x17,
            0x0101,
            0x01010105,
            5 << 16 | 3,
            0,
            0,
            u32::MAX,
        ];
        let pool = words
            .into_iter()
            .flat_map(u32::to_be_bytes)
            .collect::<Vec<_>>();
        let bank = Bank::from_sections([&descriptor, &pool, &[], &[]])?;
        let physical = programs(&bank, [7])?;
        assert_eq!(physical.programs.len(), 1);
        let lowered = physical.programs[&7]
            .iter()
            .cloned()
            .map(Instruction::mixer)
            .collect::<Result<Vec<_>>>()?;
        let compiled = compile::programs(&bank, [7])?;
        for commands in [&lowered, &compiled.programs[&7]] {
            assert!(matches!(
                commands.as_slice(),
                [
                    Command::RandomBranch {
                        minimum: 118,
                        program: 8,
                        instruction: 9
                    },
                    Command::RandomNote {
                        low: 53,
                        high: 66,
                        cents: -7,
                        relative: true,
                        random_cents: true
                    },
                    Command::RandomLoop {
                        instruction: 3,
                        count: 5,
                        key_off: true,
                        sample_end: true
                    },
                    Command::End,
                ]
            ));
        }
        Ok(())
    }

    #[test]
    fn handle_messages_lower_selectors_and_optional_trap_dependencies() -> Result<()> {
        use resonance_audio::data::{
            MessageTarget,
            Variable::{Global, Local},
        };
        let mut descriptor = [0; 32];
        descriptor[..4].copy_from_slice(&u32::MAX.to_be_bytes());
        for entry in [1, 2] {
            let root = [
                (8 << 16 | 2 << 8 | 0x28, entry),
                (9 << 16 | 2 << 8 | 0x28, 0xffff_ffff),
                (7 << 16 | 0xf1 << 8 | 0xac, 0),
                (0xff << 8 | 0x2b, 0),
                (0xff << 8 | 0x2a, 0xe1 << 8 | 0xf2),
                (123 << 16 | 0x2a, 0xf0 << 8),
                (2 << 8 | 0x29, 0),
                (0, 0),
            ];
            let mut words = vec![16, 0, 0, 0, (8 + root.len() * 8) as u32, 7 << 16];
            words.extend(root.into_iter().flat_map(|(a, b)| [a, b]));
            words.extend([24, 8 << 16, 0, 0, 0, 0, u32::MAX]);
            let pool: Vec<_> = words.into_iter().flat_map(u32::to_be_bytes).collect();
            let bank = Bank::from_sections([&descriptor, &pool, &[], &[]])?;
            if entry == 2 {
                assert!(programs(&bank, [7]).is_err());
                assert!(compile::programs(&bank, [7]).is_err());
                continue;
            }
            let physical = programs(&bank, [7])?;
            let compiled = compile::programs(&bank, [7])?;
            assert_eq!(
                compiled.programs.keys().copied().collect::<Vec<_>>(),
                [7, 8]
            );
            let lowered = physical.programs[&7]
                .iter()
                .cloned()
                .map(Instruction::mixer)
                .collect::<Result<Vec<_>>>()?;
            for commands in [&lowered, &compiled.programs[&7]] {
                assert!(matches!(
                    commands.as_slice(),
                    [
                        Command::MessageTrap {
                            program: 8,
                            instruction: 1
                        },
                        Command::MessageTrap {
                            program: 9,
                            instruction: 65535
                        },
                        Command::VoiceHandle {
                            destination: Global(1),
                            child: true
                        },
                        Command::ReceiveMessage {
                            destination: Global(15)
                        },
                        Command::SendMessage {
                            target: MessageTarget::Handle(Global(2)),
                            value: Local(1)
                        },
                        Command::SendMessage {
                            target: MessageTarget::Macro(123),
                            value: Global(0)
                        },
                        Command::ClearMessageTrap,
                        Command::End,
                    ]
                ));
                let json = serde_json::to_value(commands)?;
                let decoded: Vec<Command> = serde_json::from_value(json.clone())?;
                assert_eq!(serde_json::to_value(decoded)?, json);
            }
            assert!(
                compile::command(&bank, 0xffff_002a, 0)
                    .unwrap_err()
                    .to_string()
                    .contains("host message")
            );
            assert!(compile::command(&bank, 0x128, 0).is_err());
        }
        Ok(())
    }

    #[test]
    fn child_macros_keep_operands_entry_points_and_optional_dependency_closure() -> Result<()> {
        let mut descriptor = [0; 32];
        descriptor[..4].copy_from_slice(&u32::MAX.to_be_bytes());
        for entry in [1, 2] {
            let words = [
                16,
                0,
                0,
                0,
                32,
                7 << 16,
                8 << 16 | 0x81 << 8 | 0x88,
                3 << 24 | 17 << 16 | entry,
                9 << 16 | 127 << 8 | 8,
                255 << 24 | 255 << 16 | 65535,
                0,
                0,
                24,
                8 << 16,
                0,
                0,
                0,
                0,
                u32::MAX,
            ];
            let pool: Vec<_> = words.into_iter().flat_map(u32::to_be_bytes).collect();
            let bank = Bank::from_sections([&descriptor, &pool, &[], &[]])?;
            if entry == 2 {
                assert!(programs(&bank, [7]).is_err());
                assert!(compile::programs(&bank, [7]).is_err());
                continue;
            }
            let physical = programs(&bank, [7])?;
            let compiled = compile::programs(&bank, [7])?;
            assert_eq!(
                physical.programs.keys().copied().collect::<Vec<_>>(),
                [7, 8]
            );
            assert_eq!(
                compiled.programs.keys().copied().collect::<Vec<_>>(),
                [7, 8]
            );
            for lowered in [
                physical.programs[&7][0].clone().mixer()?,
                compiled.programs[&7][0],
            ] {
                assert!(matches!(
                    lowered,
                    Command::SpawnMacro {
                        program: 8,
                        instruction: 1,
                        key_offset: -127,
                        priority: 17,
                        max_voices: 3
                    }
                ));
            }
            assert!(matches!(
                compiled.programs[&7][1],
                Command::SpawnMacro {
                    program: 9,
                    instruction: 65535,
                    key_offset: 127,
                    priority: 255,
                    max_voices: 255
                }
            ));
        }
        Ok(())
    }

    #[test]
    fn variable_commands_keep_raw_selectors_and_lower_signed_register_operations() -> Result<()> {
        let mut descriptor = [0; 32];
        descriptor[..4].copy_from_slice(&u32::MAX.to_be_bytes());
        let bank = Bank::from_sections([&descriptor, &[], &[], &[]])?;
        let physical = command(&bank, 0xf1 << 16 | 0x65, 0xfffe)?;
        assert!(matches!(
            &physical,
            Instruction::Native(Native::SetVariable {
                destination: VariableSource::Register(0xf1),
                value: -2
            })
        ));
        let restored: Instruction = serde_json::from_value(serde_json::to_value(&physical)?)?;
        assert!(matches!(
            restored.mixer()?,
            Command::SetVariable {
                destination: Variable::Global(1),
                value: -2
            }
        ));
        for (opcode, name) in [
            (0x60, "add"),
            (0x61, "subtract"),
            (0x62, "multiply"),
            (0x63, "divide"),
        ] {
            let physical = command(&bank, 2 << 16 | opcode, 0x11 << 16 | 1)?;
            let physical: Instruction = serde_json::from_value(serde_json::to_value(physical)?)?;
            for result in [
                physical.mixer()?,
                compile::command(&bank, 2 << 16 | opcode, 0x11 << 16 | 1)?,
            ] {
                let json = serde_json::to_value(result)?;
                assert_eq!(json["operation"], "calculate");
                assert_eq!(json["arithmetic"], name);
                let result: Command = serde_json::from_value(json)?;
                let Command::Calculate {
                    destination: Variable::Local(2),
                    left: Variable::Local(1),
                    right: Operand::Variable(Variable::Global(1)),
                    operation,
                } = result
                else {
                    panic!("lost register operands")
                };
                assert_eq!(serde_json::to_value(operation)?, name);
            }
        }
        assert!(matches!(
            command(&bank, 0x22 << 16 | 0x64, 0xfff9 << 8 | 0x10)?.mixer()?,
            Command::Calculate {
                destination: Variable::Local(2),
                left: Variable::Global(0),
                right: Operand::Constant(-7),
                operation: Arithmetic::Add
            }
        ));
        assert!(matches!(
            command(&bank, 0x11 << 16 | 0x71, 5 << 16 | 1 << 8 | 2)?.mixer()?,
            Command::Branch {
                comparison: Comparison::Less,
                left: Variable::Global(1),
                right: Variable::Local(2),
                invert: true,
                instruction: 5
            }
        ));
        let controller = command(&bank, 7 << 16 | 1 << 8 | 0x65, 3)?;
        assert!(matches!(
            &controller,
            Instruction::Native(Native::SetVariable {
                destination: VariableSource::Controller(7),
                ..
            })
        ));
        assert!(matches!(
            controller.mixer()?,
            Command::SetVariable {
                destination: Variable::Controller(Controller::Paired(7)),
                value: 3,
            }
        ));
        for index in 0..64 {
            let source = VariableSource::Controller(index).lower()?;
            assert!(
                matches!(source, Variable::Controller(Controller::Paired(pair)) if pair == index & 31)
            );
            let lowered = command(&bank, u32::from(index) << 16 | 1 << 8 | 0x65, 7001)?.mixer();
            if index & 31 == 6 {
                assert!(lowered.is_err());
            } else {
                assert!(
                    matches!(lowered?, Command::SetVariable { destination: Variable::Controller(Controller::Paired(pair)), value: 7001 } if pair == index & 31)
                );
            }
        }
        for (selector, expected) in [
            (0x80, Controller::PitchBend),
            (0x81, Controller::PitchBend),
            (0x84, Controller::Surround),
            (0x85, Controller::Surround),
            (0x82, Controller::Lfo),
            (0xa0, Controller::Lfo),
        ] {
            let actual = VariableSource::Controller(selector).lower()?;
            assert_eq!(
                serde_json::to_value(actual)?,
                serde_json::to_value(Variable::Controller(expected))?
            );
        }
        assert!(command(&bank, 6 << 16 | 1 << 8 | 0x65, 3)?.mixer().is_err());
        assert!(
            command(&bank, 0x83 << 16 | 1 << 8 | 0x65, 3)?
                .mixer()
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn ordinary_adpcm_offsets_are_retained_but_hardware_starts_at_zero() -> Result<()> {
        for offset_mode in [
            SampleOffset::Constant,
            SampleOffset::Volume,
            SampleOffset::InverseVolume,
            SampleOffset::Unrecognized(255),
        ] {
            for offset in [0, 1, u32::MAX] {
                let physical = Instruction::Native(Native::StartSample {
                    sample: 7,
                    format: 0,
                    offset,
                    offset_mode,
                });
                let restored: Instruction =
                    serde_json::from_value(serde_json::to_value(&physical)?)?;
                assert!(
                    matches!(&restored, Instruction::Native(Native::StartSample { offset: value, .. }) if *value == offset)
                );
                assert!(matches!(
                    restored.mixer()?,
                    Command::StartSample { sample: 7 }
                ));
            }
        }
        Ok(())
    }

    #[test]
    fn invalid_sample_noop_ignores_offsets_without_removing_branch_slots() -> Result<()> {
        let mut descriptor = [0; 32];
        descriptor[..4].copy_from_slice(&u32::MAX.to_be_bytes());
        for mode in [0, 1, 2, 255] {
            let words = [
                16,
                0,
                0,
                0, // Macro directory.
                40,
                7 << 16, // Macro 7, four instructions.
                mode << 24 | 0x00ffff10,
                u32::MAX,
                7 << 16 | 0x06,
                3, // Skip the second no-op.
                0x00ffff10,
                0,
                0,
                0,
                u32::MAX,
            ];
            let pool = words
                .into_iter()
                .flat_map(u32::to_be_bytes)
                .collect::<Vec<_>>();
            let bank = Bank::from_sections([&descriptor, &pool, &[], &[]])?;
            let physical = programs(&bank, [7])?;
            assert!(physical.samples.is_empty());
            let lowered = physical.programs[&7]
                .iter()
                .cloned()
                .map(Instruction::mixer)
                .collect::<Result<Vec<_>>>()?;
            let compiled = compile::programs(&bank, [7])?;
            assert!(compiled.samples.is_empty());
            for commands in [&lowered, &compiled.programs[&7]] {
                assert!(matches!(
                    commands.as_slice(),
                    [
                        Command::Noop,
                        Command::Jump {
                            program: 7,
                            instruction: 3,
                        },
                        Command::Noop,
                        Command::End
                    ]
                ));
            }
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires original se.snd; command and sound-route checks only, no samples or playback"]
    fn original_looping_sounds_repeat_their_existing_interpolation_selection() -> Result<()> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../local/extracted/disc1/files/S/se.snd");
        let bytes = std::fs::read(path)?;
        let bank = Bank::parse(&bytes)?;
        for (sound, program, selection, loop_at) in [
            (398, 444, 3, 11),
            (256, 525, 4, 9),
            (256, 526, 3, 9),
            (177, 831, 3, 9),
            (385, 831, 3, 9),
            (280, 849, 4, 7),
        ] {
            let sound = bank.sound(sound)?;
            let voices = crate::instrument::resolve(
                &bank,
                crate::bank::Page {
                    object: sound.object,
                    priority: sound.priority,
                    max_voices: sound.max_voices,
                },
                sound.key,
                sound.volume,
                sound.pan,
            )?;
            assert!(voices.iter().any(|voice| voice.macro_id == program));
            let commands = bank
                .object(ObjectKind::Macro, program)?
                .chunks_exact(8)
                .map(|bytes| command(&bank, read::u32(bytes, 0)?, read::u32(bytes, 4)?)?.mixer())
                .collect::<Result<Vec<_>>>()?;
            assert!(
                matches!(commands[loop_at], Command::Loop { instruction, count, .. }
                if instruction <= selection && count > 0)
            );
            assert!(matches!(
                commands[selection],
                Command::Interpolation {
                    mode: resonance_audio::data::Interpolation::Polyphase,
                    coefficients: 2,
                }
            ));
            let retained = &commands[selection + 1..loop_at];
            assert!(
                retained
                    .iter()
                    .any(|op| matches!(op, Command::StartSample { .. }))
            );
            assert!(
                !retained
                    .iter()
                    .any(|op| matches!(op, Command::StopSample | Command::Interpolation { .. }))
            );
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires both original disc sound banks; command lowering only, no samples or audio output"]
    fn original_signed_vibratos_lower_on_their_sound_entry_routes() -> Result<()> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        for disc in [1, 2] {
            for (file, sound, program, period, depth, reverse, scaled) in [
                ("se_ev07.snd", 500, 281, 200, 25, true, true),
                ("se.snd", 416, 486, 45, 1021, false, false),
            ] {
                let bytes = std::fs::read(root.join(format!("disc{disc}/files/S/{file}")))?;
                let bank = Bank::parse(&bytes)?;
                let sound = bank.sound(sound)?;
                let voices = crate::instrument::resolve(
                    &bank,
                    crate::bank::Page {
                        object: sound.object,
                        priority: sound.priority,
                        max_voices: sound.max_voices,
                    },
                    sound.key,
                    sound.volume,
                    sound.pan,
                )?;
                assert!(voices.iter().any(|voice| voice.macro_id == program));
                let commands = bank
                    .object(ObjectKind::Macro, program)?
                    .chunks_exact(8)
                    .map(|bytes| {
                        command(&bank, read::u32(bytes, 0)?, read::u32(bytes, 4)?)?.mixer()
                    })
                    .collect::<Result<Vec<_>>>()?;
                assert!(
                    matches!(commands[5], Command::Vibrato { period_ms, depth_8, reverse: phase, scale_by_modulation }
                    if (period_ms, depth_8, phase, scale_by_modulation) == (period, depth, reverse, scaled))
                );
            }
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires original se.snd; parses commands without samples or audio output"]
    fn original_volume_curve_program_lowers_completely() -> Result<()> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../local/extracted/disc1/files/S/se.snd");
        let bytes = std::fs::read(path)?;
        let bank = Bank::parse(&bytes)?;
        for id in [178, 387] {
            assert_eq!(bank.sound(id)?.object, 840);
        }
        let commands = bank
            .object(ObjectKind::Macro, 840)?
            .chunks_exact(8)
            .map(|bytes| command(&bank, read::u32(bytes, 0)?, read::u32(bytes, 4)?)?.mixer())
            .collect::<Result<Vec<_>>>()?;
        let Command::FadeVolume {
            curve: Some(curve),
            factor: 50,
            milliseconds: 10000,
            ..
        } = commands[12]
        else {
            panic!("missing authored custom fade");
        };
        assert_eq!(&curve.0[..8], &[184, 11, 136, 19, 153, 9, 244, 1]);
        assert!(matches!(
            commands[10],
            Command::KeyOffTrap {
                instruction: 22,
                ..
            }
        ));
        assert!(matches!(
            commands[11],
            Command::Wait {
                milliseconds: None,
                key_off: true,
                ..
            }
        ));
        Ok(())
    }

    #[test]
    #[ignore = "requires both extracted discs and cooked music setups; no audio output"]
    fn original_field_and_music_programs_lower_completely() -> Result<()> {
        use std::{collections::BTreeSet, fs, path::Path};
        #[derive(Deserialize)]
        struct Programs {
            programs: BTreeMap<u16, Vec<Instruction>>,
        }

        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let cooked = std::env::var_os("RESONANCE_TEST_ASSETS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| local.join("all-assets"));
        let mut failures = BTreeMap::<String, BTreeSet<String>>::new();
        let mut count = 0;
        let mut check = |source: &str, programs: BTreeMap<u16, Vec<Instruction>>| {
            for (id, program) in programs {
                for (pc, instruction) in program.into_iter().enumerate() {
                    count += 1;
                    if let Err(error) = instruction.mixer() {
                        failures
                            .entry(error.to_string())
                            .or_default()
                            .insert(format!("{source} {id}:{pc}"));
                    }
                }
            }
        };
        let mut setups = 0;
        for directory in fs::read_dir(cooked.join("audio/songs"))? {
            for entry in fs::read_dir(directory?.path())? {
                let path = entry?.path();
                if path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with("setup-"))
                {
                    let programs: Programs = serde_json::from_slice(&fs::read(&path)?)?;
                    check(
                        &path
                            .file_name()
                            .context("missing setup filename")?
                            .to_string_lossy(),
                        programs.programs,
                    );
                    setups += 1;
                }
            }
        }
        ensure!(
            setups > 100,
            "incomplete declared music setup corpus: {setups}"
        );
        let mut sounds = 0;
        for disc in [1, 2] {
            let sources = local.join(format!("extracted/disc{disc}/files/S"));
            let resident_bytes = fs::read(sources.join("se.snd"))?;
            let resident = Bank::parse(&resident_bytes)?;
            let instrument_bytes = fs::read(sources.join("inst.snd"))?;
            let instruments = Bank::parse(&instrument_bytes)?;
            for source in std::iter::once("se.snd".to_string())
                .chain((0..8).map(|index| format!("se_ev{index:02}.snd")))
            {
                let bytes = fs::read(sources.join(&source))?;
                let mut bank = Bank::parse(&bytes)?;
                bank.inherit_objects(&resident);
                bank.inherit_objects(&instruments);
                bank.inherit_samples(&instruments);
                bank.inherit_samples(&resident);
                for id in bank.sound_ids() {
                    let sound = bank.sound(id)?;
                    let notes = crate::instrument::resolve(
                        &bank,
                        crate::bank::Page {
                            object: sound.object,
                            priority: sound.priority,
                            max_voices: sound.max_voices,
                        },
                        sound.key,
                        sound.volume,
                        sound.pan,
                    )?;
                    check(
                        &format!("disc{disc}/{source} sound {id}"),
                        programs(&bank, notes.into_iter().map(|note| note.macro_id))?.programs,
                    );
                    sounds += 1;
                }
            }
        }
        println!(
            "Checked {count} commands in {setups} music setups and {sounds} field sound declarations"
        );
        for (error, locations) in &failures {
            eprintln!(
                "{error}: {} occurrences; {:?}",
                locations.len(),
                locations.iter().take(8).collect::<Vec<_>>()
            );
        }
        ensure!(
            failures.is_empty(),
            "{} unsupported operation forms",
            failures.len()
        );
        Ok(())
    }

    #[test]
    #[ignore = "requires original sound banks; parses authored commands without sample decoding or audio output"]
    fn original_sound_commands_lower_like_the_existing_compiler() -> Result<()> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../local/extracted/disc1/files/S");
        let mut sources = std::fs::read_dir(&root)?
            .map(|entry| Ok(entry?.path()))
            .collect::<Result<Vec<_>>>()?;
        sources.retain(|path| path.extension().is_some_and(|extension| extension == "snd"));
        sources.sort();
        let bytes = sources
            .iter()
            .map(std::fs::read)
            .collect::<std::io::Result<Vec<_>>>()?;
        let residents = bytes
            .iter()
            .map(|bytes| Bank::parse(bytes))
            .collect::<Result<Vec<_>>>()?;
        let common = &residents[sources
            .iter()
            .position(|path| path.ends_with("se.snd"))
            .unwrap()];
        let instruments = &residents[sources
            .iter()
            .position(|path| path.ends_with("inst.snd"))
            .unwrap()];
        let mut compared = 0;
        let mut lowered = 0;
        let mut noops = 0;
        let mut random = 0;
        let mut pans = [0; 2];
        let mut curves = 0;
        for (path, bytes) in sources.iter().zip(&bytes) {
            let mut bank = Bank::parse(bytes)?;
            let programs: Vec<_> = bank.object_ids(ObjectKind::Macro).collect();
            bank.inherit_objects(common);
            bank.inherit_objects(instruments);
            bank.inherit_unique_objects(&residents);
            bank.inherit_samples(instruments);
            bank.inherit_samples(common);
            for id in programs {
                for (pc, bytes) in bank
                    .object(ObjectKind::Macro, id)?
                    .chunks_exact(8)
                    .enumerate()
                {
                    let a = read::u32(bytes, 0)? & !0x80;
                    let b = read::u32(bytes, 4)?;
                    let Ok(expected) = compile::command(&bank, a, b) else {
                        continue;
                    };
                    let decoded = command(&bank, a, b)?;
                    noops += usize::from(matches!(decoded, Instruction::Native(Native::Noop)));
                    lowered += usize::from(matches!(decoded, Instruction::Native(_)));
                    let actual = decoded
                        .mixer()
                        .with_context(|| format!("{} instrument {id}:{pc}", path.display()))?;
                    random += usize::from(matches!(
                        actual,
                        Command::RandomNote { .. }
                            | Command::RandomBranch { .. }
                            | Command::RandomLoop { .. }
                    ));
                    if let Command::PanRamp { axis, .. } = actual {
                        pans[usize::from(matches!(axis, Axis::Surround))] += 1;
                    }
                    curves += usize::from(matches!(
                        actual,
                        Command::SetVolume { curve: Some(_), .. }
                            | Command::FadeVolume { curve: Some(_), .. }
                    ));
                    assert_eq!(
                        format!("{actual:?}"),
                        format!("{expected:?}"),
                        "{} instrument {id}:{pc}",
                        path.display()
                    );
                    compared += 1;
                }
            }
        }
        ensure!(
            compared > 1000
                && lowered > 100
                && noops > 0
                && random > 0
                && curves > 0
                && pans.iter().all(|&count| count > 0),
            "incomplete original command corpus"
        );
        println!(
            "Compared {compared} authored commands, including {lowered} native-record lowerings, {random} random note/branch/loop operations, {noops} no-ops, pan/surround ramps {pans:?} and {curves} custom volume curves"
        );
        Ok(())
    }
}
