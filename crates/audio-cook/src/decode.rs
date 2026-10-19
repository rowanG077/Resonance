//! Compile instrument bytes directly to the native audio command model.
use crate::{
    bank::{Bank, MissingObject, ObjectKind},
    read,
};
use anyhow::{Context, Result, bail, ensure};
use resonance_audio::data::{
    self, Arithmetic, Command, Comparison, ControlTarget, Controller, Envelope, Interpolation,
    Operand, Variable, VolumeCurve,
};
pub use resonance_audio::data::{PanAxis as Axis, Resources};
use std::collections::BTreeMap;

fn variable(controller: u8, index: u8) -> Result<Variable> {
    if controller == 0 {
        Ok(if index & 31 < 16 {
            Variable::Local(index & 15)
        } else {
            Variable::Global(index & 15)
        })
    } else {
        Ok(Variable::Controller(match index {
            0..=0x3f => Controller::Paired(index & 31),
            0x80 | 0x81 => Controller::PitchBend,
            0x84 | 0x85 => Controller::Surround,
            0x82 | 0xa0 => Controller::Lfo,
            _ => bail!("macro controller register {index} is not implemented"),
        }))
    }
}

pub fn programs(
    bank: &Bank<'_>,
    roots: impl IntoIterator<Item = u16>,
    sustains: &crate::parameters::Sustains,
) -> Result<Resources> {
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
            let command = command(bank, read::u32(bytes, 0)?, read::u32(bytes, 4)?, sustains)
                .with_context(|| format!("instrument {id}, instruction {pc}"))?;
            match command {
                Command::Jump { program, .. } | Command::KeyOffTrap { program, .. } => {
                    pending.push(program)
                }
                Command::RandomBranch { program, .. }
                | Command::SpawnMacro { program, .. }
                | Command::MessageTrap { program, .. }
                    if bank.object(ObjectKind::Macro, program).is_ok() =>
                {
                    pending.push(program)
                }
                Command::StartSample { sample } => {
                    if let std::collections::btree_map::Entry::Vacant(entry) =
                        resources.samples.entry(sample)
                    {
                        entry.insert(bank.sample(sample)?.into());
                    }
                }
                _ => {}
            }
            program.push(command);
        }
        resources.programs.insert(id, program);
    }
    resources.validate()?;
    Ok(resources)
}

pub(crate) fn command(
    bank: &Bank<'_>,
    a: u32,
    b: u32,
    sustains: &crate::parameters::Sustains,
) -> Result<Command> {
    let a = a & !0x80; // The dispatch byte reserves its high bit.
    let milliseconds = b >> 8 & 1 != 0;
    Ok(match a as u8 {
        0 => Command::End,
        0x04 | 0x07 => {
            let duration = (b >> 16) as u16;
            let milliseconds = a as u8 == 7 || milliseconds;
            let random = a >> 16 & 1 != 0;
            let from_start = b & 1 != 0;
            let key_off = a >> 8 & 1 != 0;
            let sample_end = a >> 24 & 1 != 0;
            if random {
                ensure!(
                    milliseconds && !from_start,
                    "random waits require relative milliseconds"
                );
                Command::RandomWait {
                    upper_ms: duration,
                    key_off,
                    sample_end,
                }
            } else if milliseconds {
                Command::Wait {
                    milliseconds: (duration != u16::MAX).then_some(duration),
                    from_start,
                    key_off,
                    sample_end,
                }
            } else {
                ensure!(
                    !from_start,
                    "absolute beat waits require the global synthesizer clock"
                );
                Command::BeatWait {
                    ticks: (duration != u16::MAX).then_some(duration),
                    key_off,
                    sample_end,
                }
            }
        }
        0x05 => {
            let instruction = usize::from(b as u16);
            let count = (b >> 16) as u16;
            let key_off = a >> 8 & 1 != 0;
            let sample_end = a >> 24 & 1 != 0;
            if a >> 16 & 1 != 0 {
                Command::RandomLoop {
                    instruction,
                    count,
                    key_off,
                    sample_end,
                }
            } else {
                Command::Loop {
                    instruction,
                    count,
                    key_off,
                    sample_end,
                }
            }
        }
        0x06 => Command::Jump {
            program: (a >> 16) as u16,
            instruction: usize::from(b as u16),
        },
        0x08 => Command::SpawnMacro {
            program: (a >> 16) as u16,
            instruction: b as u16,
            key_offset: (a >> 8) as i8,
            priority: (b >> 16) as u8,
            max_voices: (b >> 24) as u8,
        },
        0x0c => {
            let bytes = bank.object(ObjectKind::Table, (a >> 8) as u16)?;
            let envelope = if a >> 24 == 0 {
                Envelope::Ordinary(crate::parameters::ordinary(bytes)?)
            } else {
                Envelope::Dls(crate::parameters::dls(bytes, sustains)?)
            };
            Command::Envelope { envelope }
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
                Command::SetVolume {
                    factor,
                    offset,
                    curve,
                    from_velocity: (b >> 8) as u8 != 0,
                }
            } else {
                ensure!(milliseconds, "beat-based volume fades are not implemented");
                Command::FadeVolume {
                    factor,
                    offset,
                    curve,
                    milliseconds: (b >> 16) as u16,
                    from_silence: a as u8 == 0x14,
                }
            }
        }
        0x0e | 0x15 => Command::PanRamp {
            axis: if a as u8 == 0x0e {
                Axis::Pan
            } else {
                Axis::Surround
            },
            initial: (a >> 8) as u8,
            delta: b as i8,
            milliseconds: (a >> 16) as u16,
        },
        // An invalid sample handle leaves the existing voice unchanged.
        0x10 if (a >> 8) as u16 == u16::MAX => Command::Noop,
        0x10 => {
            let sample = (a >> 8) as u16;
            let format = bank.sample_format(sample)?;
            ensure!(format == 0, "unsupported instrument sample format {format}");
            // ADPCM decoding begins at its first predictor block; byte offsets
            // in this operation do not select a different playback position.
            Command::StartSample { sample }
        }
        0x11 => Command::StopSample,
        0x12 => Command::Release,
        0x13 => Command::RandomBranch {
            minimum: (a >> 8) as u8,
            program: (a >> 16) as u16,
            instruction: b as u16 as usize,
        },
        0x17 => Command::RandomNote {
            low: (a >> 8) as u8,
            high: (a >> 24) as u8,
            cents: (a >> 16) as i8,
            random_cents: b as u8 != 0,
            relative: (b >> 8) as u8 != 0,
        },
        0x18 => {
            ensure!(
                b >> 16 == 0 || (b >> 8) & 1 != 0,
                "beat-based pitch-offset waits are not implemented"
            );
            Command::PitchOffset {
                from_original: a >> 24 != 0,
                semitones: (a >> 8) as i8,
                cents: (a >> 16) as i8,
                wait_ms: (b >> 16) as u16,
                from_start: b & 1 != 0,
            }
        }
        0x19 => {
            ensure!(
                b >> 16 == 0 || (b >> 8) & 1 != 0,
                "beat-based SetNote waits are not implemented"
            );
            Command::SetNote {
                key: ((a >> 8) & 127) as u8,
                cents: (a >> 16) as i8,
                wait_ms: (b >> 16) as u16,
                from_start: b & 1 != 0,
            }
        }
        0x1c => {
            let period_ms = (b >> 16) as u16;
            let semitones = (a >> 8) as i8;
            let cents = (a >> 16) as i8;
            let scale_by_modulation = a >> 24 & 3 != 0;
            let (depth_8, reverse) = if period_ms == 0 {
                (0, false)
            } else {
                ensure!(milliseconds, "beat-based vibrato is not implemented");
                let reverse = semitones < 0 || semitones == 0 && cents < 0;
                let (semitones, cents) = if reverse {
                    (semitones.unsigned_abs(), cents.unsigned_abs())
                } else if cents < 0 {
                    ((semitones - 1) as u8, (100 - i16::from(cents)) as u8)
                } else {
                    (semitones as u8, cents as u8)
                };
                (
                    u16::from(semitones) * 256 + u16::from(cents) * 256 / 100,
                    reverse,
                )
            };
            Command::Vibrato {
                period_ms,
                depth_8,
                reverse,
                scale_by_modulation,
            }
        }
        0x1d | 0x1e => {
            ensure!(
                (b >> 8) as u8 == 1 && b & 255 == 0,
                "unsupported pitch-sweep wait"
            );
            Command::PitchSweep {
                slot: if a as u8 == 0x1d {
                    data::SweepSlot::First
                } else {
                    data::SweepSlot::Second
                },
                step_hz: (a >> 16) as i16,
                period: (a >> 8) as u8,
                wait_ms: (b >> 16) as u16,
            }
        }
        0x20 => {
            let bytes = bank.pitch_envelope((a >> 8) as u16)?;
            let coarse = i32::from(b as i8) * 256;
            let fine = i32::from((b >> 8) as i8) * 256 / 100;
            Command::PitchEnvelope {
                envelope: crate::parameters::timing(bytes)?,
                sustain: crate::parameters::pitch_sustain(bytes, sustains)?,
                depth_8: (if coarse < 0 {
                    coarse - fine
                } else {
                    coarse + fine
                }) as i16,
            }
        }
        0x21 => Command::ScaleVolume {
            from_velocity: a >> 24 != 0,
            factor: (a >> 8) as u16,
        },
        0x22 => Command::ModulationDepth {
            semitones: (a >> 8) as i8,
            cents: (a >> 16) as i8,
        },
        0x23 => Command::Tremolo {
            scale: (a >> 8) as u16,
            modulation_scale: b as u16,
        },
        0x28 => {
            let program = (a >> 16) as u16;
            let instruction = (b & 0xffff) as usize;
            match (a >> 8) as u8 {
                0 => Command::KeyOffTrap {
                    program,
                    instruction,
                },
                2 => Command::MessageTrap {
                    program,
                    instruction,
                },
                slot => bail!("unsupported instrument trap slot {slot}"),
            }
        }
        0x29 => match (a >> 8) as u8 {
            0 => Command::ClearKeyOffTrap,
            2 => Command::ClearMessageTrap,
            slot => bail!("unsupported instrument trap slot {slot}"),
        },
        0x2a => Command::SendMessage {
            target: if (a >> 8) as u8 != 0 {
                data::MessageTarget::Handle(variable(0, b as u8)?)
            } else {
                ensure!(
                    a >> 16 != 0xffff,
                    "host message callbacks are not implemented"
                );
                data::MessageTarget::Macro((a >> 16) as u16)
            },
            value: variable(0, (b >> 8) as u8)?,
        },
        0x2b => Command::ReceiveMessage {
            destination: variable(0, (a >> 8) as u8)?,
        },
        0x2c => Command::VoiceHandle {
            destination: variable(0, (a >> 8) as u8)?,
            child: (a >> 16) as u8 != 0,
        },
        0x30 => Command::AddAge {
            value: (a >> 16) as i16,
        },
        0x31 => Command::SetAge {
            value: (a >> 16) as u16,
        },
        0x36 => Command::Priority {
            value: (a >> 8) as u8,
        },
        0x38 => Command::AgePeriod { milliseconds: b },
        0x40..=0x4c => {
            let coarse = i32::from((a >> 16) as i16) * 65536 / 100;
            let fine = i32::from((b >> 16) as i8) * 256 / 100;
            let scale = if coarse < 0 {
                coarse - fine
            } else {
                coarse + fine
            };
            let source = (a >> 8) as u8;
            ensure!(
                b as u8 == 0,
                "combined controller selectors are not implemented"
            );
            if (b >> 8) as u8 != 0 {
                let target = match a as u8 {
                    0x40 => ControlTarget::Volume,
                    0x41 => ControlTarget::Pan,
                    0x46 => ControlTarget::PostAuxiliaryA,
                    0x47 => ControlTarget::SurroundPan,
                    0x4a => ControlTarget::PreAuxiliaryA,
                    0x4b => ControlTarget::PreAuxiliaryB,
                    0x4c => ControlTarget::PostAuxiliaryB,
                    opcode => bail!("unsupported variable selector destination {opcode:#04x}"),
                };
                Command::SelectControl {
                    target,
                    source: if scale == 0 {
                        Operand::Constant(0)
                    } else {
                        Operand::Variable(variable(0, source)?)
                    },
                    scale,
                }
            } else {
                let input = match (a as u8, source, scale) {
                    (0x49, _, 0) => data::TremoloInput::Zero,
                    (0x49, 130, 65536) => data::TremoloInput::Lfo,
                    _ => bail!("unsupported controller selector"),
                };
                Command::TremoloInput { input }
            }
        }
        0x50 => {
            ensure!(
                (a >> 8) as u8 == 0 && b == 0,
                "only LFO 0 with default phase is implemented"
            );
            Command::Lfo {
                period_ms: (a >> 16) as u16,
            }
        }
        0x58 => Command::VolumeCurve {
            alternate: (a >> 8) as u8 != 0,
            interaural_delay: (a >> 16) as u8 != 0,
        },
        0x59 => Command::ExclusiveGroup {
            group: (a >> 8) as u8,
            kill: (a >> 16) as u8 != 0,
        },
        0x5a => {
            let mode = match (a >> 8) as u8 {
                0 => Interpolation::Polyphase,
                1 => Interpolation::Linear,
                2 => Interpolation::Direct,
                _ => bail!("unsupported interpolation mode"),
            };
            ensure!(((a >> 16) as u8) < 4, "invalid interpolation coefficients");
            Command::Interpolation {
                mode,
                coefficients: (a >> 16) as u8,
            }
        }
        0x60..=0x64 => Command::Calculate {
            destination: variable((a >> 8) as u8, (a >> 16) as u8)?,
            operation: match a as u8 {
                0x61 => Arithmetic::Subtract,
                0x62 => Arithmetic::Multiply,
                0x63 => Arithmetic::Divide,
                _ => Arithmetic::Add,
            },
            left: variable((a >> 24) as u8, b as u8)?,
            right: if a as u8 == 0x64 {
                Operand::Constant((b >> 8) as i16)
            } else {
                Operand::Variable(variable((b >> 8) as u8, (b >> 16) as u8)?)
            },
        },
        0x65 => Command::SetVariable {
            destination: variable((a >> 8) as u8, (a >> 16) as u8)?,
            value: b as i16,
        },
        0x70 | 0x71 => Command::Branch {
            comparison: if a as u8 == 0x70 {
                Comparison::Equal
            } else {
                Comparison::Less
            },
            left: variable((a >> 8) as u8, (a >> 16) as u8)?,
            right: variable((a >> 24) as u8, b as u8)?,
            invert: (b >> 8) as u8 != 0,
            instruction: (b >> 16) as usize,
        },
        opcode => bail!("unsupported instrument opcode {opcode:#04x}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parameters::test_sustains as sustains;

    #[test]
    fn unsupported_commands_report_their_source_location() -> Result<()> {
        let mut descriptor = [0; 32];
        descriptor[..4].copy_from_slice(&u32::MAX.to_be_bytes());
        let pool: Vec<_> = [
            16,
            0,
            0,
            0,
            32,
            7 << 16,
            0x00ffff10,
            0,
            4,
            10 << 16 | 1,
            0,
            0,
            u32::MAX,
        ]
        .into_iter()
        .flat_map(u32::to_be_bytes)
        .collect();
        let bank = Bank::from_sections([&descriptor, &pool, &[], &[]])?;
        let error = programs(&bank, [7], &sustains())
            .err()
            .context("absolute beat wait must fail compilation")?;
        let message = format!("{error:#}");
        assert!(message.contains("instrument 7, instruction 1"));
        assert!(message.contains("absolute beat waits require the global synthesizer clock"));
        assert!(command(&bank, 0x4b, 0x101, &sustains()).is_err());
        assert!(command(&bank, 0x42, 0x100, &sustains()).is_err());
        assert!(programs(&bank, [], &sustains()).is_err());
        Ok(())
    }

    #[test]
    fn modulation_commands_lower_depth_phase_and_constant_inputs() -> Result<()> {
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
            let actual = command(&bank, a, b, &sustains())?;
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
        for (word, expected) in [
            (0x0000_3249, data::TremoloInput::Zero),
            (0x0000_ff49, data::TremoloInput::Zero),
            (0x0064_8249, data::TremoloInput::Lfo),
        ] {
            let Command::TremoloInput { input } = command(&bank, word, 0, &sustains())? else {
                panic!("lost tremolo input")
            };
            assert_eq!(input, expected);
        }
        assert!(command(&bank, 0x0064_3249, 0, &sustains()).is_err());
        assert!(command(&bank, 0x0000_3249, 1, &sustains()).is_err());
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
            let lowered = command(&bank, a, b, &sustains())?;
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
            if opcode != 0x0d {
                assert!(command(&bank, a, b & !0x100, &sustains()).is_err());
            }
        }
        assert!(matches!(
            command(&bank, 89 << 24 | 0x0d, 0, &sustains())?,
            Command::SetVolume { curve: None, .. }
        ));
        pool.truncate(144);
        pool[16..20].copy_from_slice(&128u32.to_be_bytes());
        pool.extend_from_slice(&u32::MAX.to_be_bytes());
        let truncated = Bank::from_sections([&descriptor, &pool, &[], &[]])?;
        assert!(command(&truncated, 88 << 24 | 0x0d, 0, &sustains()).is_err());
        Ok(())
    }

    #[test]
    fn pan_ramps_decode_signed_deltas_and_both_axes() -> Result<()> {
        let mut descriptor = [0; 32];
        descriptor[..4].copy_from_slice(&u32::MAX.to_be_bytes());
        let bank = Bank::from_sections([&descriptor, &[], &[], &[]])?;
        for code in [0x0e, 0x15] {
            let result = command(&bank, 15000 << 16 | 98 << 8 | code, 0xbc, &sustains())?;
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
        Ok(())
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
        let compiled = programs(&bank, [7], &sustains())?;
        assert_eq!(compiled.programs.len(), 1);
        let commands = &compiled.programs[&7];
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

        let mut invalid_target = words;
        invalid_target[6] = 7 << 16 | 118 << 8 | 0x13;
        let pool = invalid_target
            .into_iter()
            .flat_map(u32::to_be_bytes)
            .collect::<Vec<_>>();
        let bank = Bank::from_sections([&descriptor, &pool, &[], &[]])?;
        let error = programs(&bank, [7], &sustains())
            .err()
            .context("out-of-range target must fail")?;
        let error = format!("{error:#}");
        assert!(error.contains("instrument 7 instruction 0"));
        assert!(error.contains("instrument conditional target is out of bounds"));
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
                assert!(programs(&bank, [7], &sustains()).is_err());
                continue;
            }
            let compiled = programs(&bank, [7], &sustains())?;
            assert_eq!(
                compiled.programs.keys().copied().collect::<Vec<_>>(),
                [7, 8]
            );
            let commands = &compiled.programs[&7];
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

            assert!(
                command(&bank, 0xffff_002a, 0, &sustains())
                    .unwrap_err()
                    .to_string()
                    .contains("host message")
            );
            assert!(command(&bank, 0x128, 0, &sustains()).is_err());
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
                assert!(programs(&bank, [7], &sustains()).is_err());
                continue;
            }
            let compiled = programs(&bank, [7], &sustains())?;
            assert_eq!(
                compiled.programs.keys().copied().collect::<Vec<_>>(),
                [7, 8]
            );
            let lowered = compiled.programs[&7][0];
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
    fn variable_commands_compile_signed_register_operations() -> Result<()> {
        let mut descriptor = [0; 32];
        descriptor[..4].copy_from_slice(&u32::MAX.to_be_bytes());
        let bank = Bank::from_sections([&descriptor, &[], &[], &[]])?;
        assert!(matches!(
            command(&bank, 0xf1 << 16 | 0x65, 0xfffe, &sustains())?,
            Command::SetVariable {
                destination: Variable::Global(1),
                value: -2
            }
        ));
        for (opcode, expected) in [
            (0x60, "add"),
            (0x61, "subtract"),
            (0x62, "multiply"),
            (0x63, "divide"),
        ] {
            let Command::Calculate {
                destination: Variable::Local(2),
                left: Variable::Local(1),
                right: Operand::Variable(Variable::Global(1)),
                operation,
            } = command(&bank, 2 << 16 | opcode, 0x11 << 16 | 1, &sustains())?
            else {
                panic!("lost register operands")
            };
            assert_eq!(serde_json::to_value(operation)?, expected);
        }
        assert!(matches!(
            command(&bank, 0x22 << 16 | 0x64, 0xfff9 << 8 | 0x10, &sustains())?,
            Command::Calculate {
                destination: Variable::Local(2),
                left: Variable::Global(0),
                right: Operand::Constant(-7),
                operation: Arithmetic::Add
            }
        ));
        assert!(matches!(
            command(&bank, 0x11 << 16 | 0x71, 5 << 16 | 1 << 8 | 2, &sustains())?,
            Command::Branch {
                comparison: Comparison::Less,
                left: Variable::Global(1),
                right: Variable::Local(2),
                invert: true,
                instruction: 5
            }
        ));
        for index in 0..64 {
            let command = command(
                &bank,
                u32::from(index) << 16 | 1 << 8 | 0x65,
                7001,
                &sustains(),
            )?;
            assert!(matches!(command,
                Command::SetVariable { destination: Variable::Controller(Controller::Paired(pair)), value: 7001 } if pair == index & 31));
            let resources = Resources {
                programs: BTreeMap::from([(1, vec![command])]),
                samples: BTreeMap::new(),
            };
            assert_eq!(resources.validate().is_ok(), index & 31 != 6);
        }
        for (selector, expected) in [
            (0x80, Controller::PitchBend),
            (0x81, Controller::PitchBend),
            (0x84, Controller::Surround),
            (0x85, Controller::Surround),
            (0x82, Controller::Lfo),
            (0xa0, Controller::Lfo),
        ] {
            assert_eq!(
                serde_json::to_value(variable(1, selector)?)?,
                serde_json::to_value(Variable::Controller(expected))?
            );
        }
        assert!(command(&bank, 0x83 << 16 | 1 << 8 | 0x65, 3, &sustains()).is_err());
        Ok(())
    }

    #[test]
    fn sample_starts_ignore_adpcm_offsets_and_reject_unsupported_formats() -> Result<()> {
        let mut descriptor = [0; 32];
        descriptor[..4].copy_from_slice(&u32::MAX.to_be_bytes());
        for format in [0, 1, 2] {
            let mut samples = [0; 36];
            samples[..2].copy_from_slice(&7u16.to_be_bytes());
            samples[16] = format;
            samples[32..].copy_from_slice(&u32::MAX.to_be_bytes());
            let bank = Bank::from_sections([&descriptor, &[], &samples, &[]])?;
            for mode in [0, 1, 2, 255] {
                for offset in [0, 1, u32::MAX] {
                    let result = command(&bank, mode << 24 | 7 << 8 | 0x10, offset, &sustains());
                    if format == 0 {
                        assert!(matches!(result?, Command::StartSample { sample: 7 }));
                    } else {
                        assert!(result.unwrap_err().to_string().contains("sample format"));
                    }
                }
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
            let compiled = programs(&bank, [7], &sustains())?;
            assert!(compiled.samples.is_empty());
            let commands = &compiled.programs[&7];
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
        Ok(())
    }

    #[test]
    #[ignore = "requires original se.snd; command and sound-route checks only, no samples or playback"]
    fn looping_sounds_repeat_their_existing_interpolation_selection() -> Result<()> {
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
                .map(|bytes| {
                    command(
                        &bank,
                        read::u32(bytes, 0)?,
                        read::u32(bytes, 4)?,
                        &sustains(),
                    )
                })
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
    fn signed_vibratos_lower_on_their_sound_entry_routes() -> Result<()> {
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
                        command(
                            &bank,
                            read::u32(bytes, 0)?,
                            read::u32(bytes, 4)?,
                            &sustains(),
                        )
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
    fn volume_curve_program_lowers_completely() -> Result<()> {
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
            .map(|bytes| {
                command(
                    &bank,
                    read::u32(bytes, 0)?,
                    read::u32(bytes, 4)?,
                    &sustains(),
                )
            })
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
    #[ignore = "requires both extracted disc sound banks; no audio output"]
    fn field_sound_programs_compile_completely() -> Result<()> {
        use std::{fs, path::Path};
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let mut sounds = 0;
        let mut commands = 0;
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
                    let compiled = programs(
                        &bank,
                        notes.into_iter().map(|note| note.macro_id),
                        &sustains(),
                    )
                    .with_context(|| format!("disc{disc}/{source} sound {id}"))?;
                    commands += compiled.programs.values().map(Vec::len).sum::<usize>();
                    sounds += 1;
                }
            }
        }
        ensure!(sounds > 0 && commands > 0, "sound corpus is empty");
        println!("Compiled {commands} commands in {sounds} field sound declarations");
        Ok(())
    }
}
