//! Typed instrument operations compiled by the importer.
use super::{Envelope, HostRequest, PanRamp, Parameters, Sweep, Voice, Wait};
use crate::{
    data::{
        Command, Comparison, Controller, Envelope as Definition, Interpolation, Operand, PanAxis,
        SweepSlot, Variable,
    },
    modulation, resample,
};
use anyhow::{Context, Result, ensure};

impl Voice<'_> {
    pub(super) fn commands(
        &mut self,
        controls: &mut super::Controls,
        fuel: &mut usize,
    ) -> Result<()> {
        while !self.done && self.ready() {
            if self.wait.until != 0 {
                self.wait_reference = self.wait.until.min(self.now());
            }
            ensure!(
                *fuel > 0,
                "music program {} instruction budget exhausted at {}",
                self.macro_id,
                self.pc
            );
            *fuel -= 1;
            let command = *self
                .resources
                .program(self.macro_id)?
                .get(self.pc)
                .with_context(|| {
                    format!("music program {} instruction {}", self.macro_id, self.pc)
                })?;
            self.pc += 1;
            self.wait = Wait::default();
            match command {
                Command::Noop => {}
                Command::End => self.finish_program(),
                Command::SetVariable { destination, value } => {
                    self.set_variable(destination, i32::from(value), controls)?
                }
                Command::VoiceHandle { destination, child } => {
                    self.set_variable(
                        destination,
                        if child { self.last_child } else { self.handle } as i32,
                        controls,
                    )?;
                }
                Command::SendMessage { target, value } => {
                    let target = match target {
                        crate::data::MessageTarget::Handle(variable) => {
                            super::MessageTarget::Handle(self.variable(variable, controls)? as u32)
                        }
                        crate::data::MessageTarget::Macro(program) => {
                            super::MessageTarget::Macro(program)
                        }
                    };
                    self.host_request = Some(HostRequest::Message {
                        target,
                        value: self.variable(value, controls)?,
                    });
                    break;
                }
                Command::ReceiveMessage { destination } => {
                    let value = self.messages.pop_front().unwrap_or(0);
                    self.set_variable(destination, value, controls)?;
                }
                Command::MessageTrap {
                    program,
                    instruction,
                } => {
                    if self.resources.programs.contains_key(&program) {
                        self.message_trap = Some((program, instruction));
                    }
                }
                Command::ClearMessageTrap => self.message_trap = None,
                Command::Calculate {
                    destination,
                    operation,
                    left,
                    right,
                } => {
                    let left = self.variable(left, controls)? as i16;
                    let right = match right {
                        Operand::Variable(variable) => self.variable(variable, controls)? as i16,
                        Operand::Constant(value) => value,
                    };
                    self.set_variable(
                        destination,
                        i32::from(operation.evaluate(left, right)),
                        controls,
                    )?;
                }
                Command::Branch {
                    comparison,
                    left,
                    right,
                    invert,
                    instruction,
                } => {
                    let (left, right) = (
                        self.variable(left, controls)?,
                        self.variable(right, controls)?,
                    );
                    let passes = match comparison {
                        Comparison::Equal => left == right,
                        Comparison::Less => left < right,
                    };
                    if passes != invert {
                        self.pc = instruction;
                    }
                }
                Command::SpawnMacro {
                    program,
                    instruction,
                    key_offset,
                    priority,
                    max_voices,
                } => {
                    self.last_child = u32::MAX;
                    if self.resources.programs.contains_key(&program) {
                        self.host_request = Some(HostRequest::Spawn {
                            note: crate::data::Note {
                                macro_id: program,
                                key: (i16::from(self.original_key) + i16::from(key_offset))
                                    .clamp(0, 127) as u8,
                                velocity: (self.volume >> 16).min(127) as u8,
                                pan: (self.pan[0].value() >> 16).clamp(0, 127) as u8,
                                priority,
                                max_voices,
                            },
                            instruction,
                        });
                        break;
                    }
                }
                Command::Jump {
                    program,
                    instruction,
                } => {
                    self.macro_id = program;
                    self.pc = instruction;
                }
                Command::RandomBranch {
                    minimum,
                    program,
                    instruction,
                } => {
                    if self.random.next() as u8 >= minimum
                        && self.resources.programs.contains_key(&program)
                    {
                        self.macro_id = program;
                        self.pc = instruction;
                    }
                }
                Command::Loop {
                    instruction,
                    count,
                    key_off,
                    sample_end,
                }
                | Command::RandomLoop {
                    instruction,
                    count,
                    key_off,
                    sample_end,
                } => {
                    match self.loop_remaining {
                        0 => {
                            self.loop_remaining = if matches!(command, Command::RandomLoop { .. }) {
                                ensure!(count != 0, "random loop requires a nonzero bound");
                                self.random.below(count)
                            } else {
                                count
                            }
                        }
                        u16::MAX => {}
                        _ => self.loop_remaining -= 1,
                    }
                    if self.loop_remaining != 0 {
                        if key_off && self.key_off || sample_end && self.sample_ended() {
                            self.loop_remaining = 0;
                        } else {
                            self.pc = instruction;
                        }
                    }
                }
                Command::Envelope { envelope } => {
                    self.parameters = match envelope {
                        Definition::Ordinary(p) => Parameters::Ordinary(p),
                        Definition::Dls(p) => {
                            Parameters::Dls(p.resolve(self.velocity, self.original_key)?)
                        }
                    };
                    self.envelope = Envelope::new(self.parameters, self.tables)?;
                }
                Command::PitchEnvelope {
                    envelope,
                    sustain,
                    depth_8,
                } => {
                    let parameters = envelope.resolve(sustain, self.velocity, self.original_key)?;
                    self.pitch_envelope = Some((
                        crate::dls::Envelope::new(parameters, &self.tables.dls)?,
                        depth_8,
                    ));
                }
                Command::StartSample { sample } => {
                    ensure!(
                        !self.interaural_delay || self.tables.mix.spatial.is_some(),
                        "instrument requires uncooked spatial audio tables"
                    );
                    self.delay = self.interaural_delay.then(Default::default);
                    let cursor = resample::SampleCursor::new(self.resources.sample(sample)?)?;
                    let ratio = crate::pitch::ratio(
                        (i32::from(self.key) << 16) + (i32::from(self.cents) << 16) / 100,
                        cursor.sample(),
                    )?;
                    let mode = match self.source_mode {
                        Interpolation::Polyphase => resample::Mode::Polyphase(
                            &self.tables.coefficients.0[self.coefficient_set],
                        ),
                        Interpolation::Linear => resample::Mode::Linear,
                        Interpolation::Direct => resample::Mode::Direct,
                    };
                    self.source = Some((cursor, resample::Resampler::new(mode, ratio)));
                    self.sample_finished = false;
                    self.envelope = Envelope::new(self.parameters, self.tables)?;
                }
                Command::StopSample => {
                    self.break_source();
                }
                Command::Release => self.release_envelopes(),
                Command::PitchOffset {
                    from_original,
                    semitones,
                    cents,
                    wait_ms,
                    from_start,
                } => {
                    let base = if from_original {
                        self.original_key
                    } else {
                        self.key
                    };
                    self.key = (i16::from(base) + i16::from(semitones)).clamp(0, 127) as u8;
                    self.cents = cents;
                    self.pitch_wait(wait_ms, from_start);
                }
                Command::SetNote {
                    key,
                    cents,
                    wait_ms,
                    from_start,
                } => {
                    ensure!(key < 128, "invalid instrument key");
                    self.key = key;
                    self.cents = cents;
                    self.pitch_wait(wait_ms, from_start);
                }
                Command::RandomNote {
                    low,
                    high,
                    cents,
                    random_cents,
                    relative,
                } => {
                    let (low, high) = if relative {
                        (
                            self.key.saturating_sub(low),
                            self.key.saturating_add(high).min(127),
                        )
                    } else {
                        (low.min(high), high.max(low))
                    };
                    let random = &self.random;
                    self.cents = if random_cents {
                        (random.below(201) as i16 - 100) as i8
                    } else {
                        cents
                    };
                    self.key = ((u16::from(low)
                        + random.below(u16::from(high) - u16::from(low) + 1))
                        & 127) as u8;
                }
                Command::PanRamp {
                    axis,
                    initial,
                    delta,
                    milliseconds,
                } => {
                    self.pan[match axis {
                        PanAxis::Pan => 0,
                        PanAxis::Surround => 1,
                    }] = PanRamp::new(initial, delta, milliseconds);
                }
                Command::ScaleVolume {
                    from_velocity,
                    factor,
                } => {
                    let start = if from_velocity {
                        u32::from(self.velocity) << 16
                    } else {
                        self.volume
                    };
                    self.volume = scale_volume(start, factor);
                }
                Command::SetVolume {
                    factor,
                    offset,
                    curve,
                    from_velocity,
                } => {
                    let start = if from_velocity {
                        u32::from(self.velocity) << 16
                    } else {
                        self.volume
                    };
                    self.volume = (start * u32::from(factor) / 127 + (u32::from(offset) << 16))
                        .min(127 << 16);
                    if let Some(curve) = curve {
                        self.volume = curve.translate(self.volume);
                    }
                    self.volume_ramp = None;
                }
                Command::FadeVolume {
                    factor,
                    offset,
                    curve,
                    milliseconds,
                    from_silence,
                } => {
                    let target = (((self.volume * u32::from(factor)) >> 7)
                        + (u32::from(offset) << 16))
                        .min(127 << 16);
                    let target = curve.map_or(target, |curve| curve.translate(target)) as i32;
                    if from_silence {
                        self.volume = 0;
                    }
                    self.volume_ramp = Some(crate::volume::Ramp::new(
                        self.volume as i32,
                        target,
                        crate::volume::frames_from_millis(u64::from(milliseconds))?,
                    ));
                }
                Command::Auxiliary { bus, value } => {
                    ensure!(bus < 2 && value < 128, "invalid auxiliary control");
                    self.auxiliary_override[usize::from(bus)] = Some(value);
                }
                Command::VolumeControl { value } => self.volume_control = Some(value),
                Command::SelectControl {
                    target,
                    source,
                    scale,
                } => {
                    self.selectors[target as usize] = Some((source, scale));
                }
                Command::SetAge { value } => self.set_age(i32::from(value)),
                Command::AddAge { value } => {
                    self.set_age(self.priority_age.value() + i32::from(value));
                }
                Command::AgePeriod { milliseconds } => self.set_age_period(milliseconds)?,
                Command::PitchSweep {
                    slot,
                    step_hz,
                    period,
                    wait_ms,
                } => {
                    self.pitch_sweeps[match slot {
                        SweepSlot::First => 0,
                        SweepSlot::Second => 1,
                    }] = Sweep::new(step_hz, period);
                    self.pitch_wait(wait_ms, false);
                }
                Command::KeyOffTrap {
                    program,
                    instruction,
                } => {
                    self.key_off_trap = Some((program, instruction));
                }
                Command::ClearKeyOffTrap => self.key_off_trap = None,
                Command::Wait {
                    milliseconds,
                    from_start,
                    key_off,
                    sample_end,
                } => {
                    if milliseconds != Some(0)
                        && !(key_off && self.key_off)
                        && !(sample_end && self.sample_ended())
                    {
                        let base = if from_start {
                            self.macro_started_at.unwrap()
                        } else {
                            self.now()
                        };
                        self.wait = Wait {
                            until: milliseconds
                                .map(|duration| {
                                    crate::volume::frames_from_millis(u64::from(duration)).and_then(
                                        |frames| {
                                            base.checked_add(frames)
                                                .context("wait deadline overflow")
                                        },
                                    )
                                })
                                .transpose()?
                                .unwrap_or(u64::MAX),
                            key_off,
                            sample_end,
                        };
                    }
                }
                Command::BeatWait {
                    ticks,
                    key_off,
                    sample_end,
                } => {
                    if ticks != Some(0)
                        && !(key_off && self.key_off)
                        && !(sample_end && self.sample_ended())
                    {
                        let until = if let Some(ticks) = ticks {
                            // Scores use 384 ticks per quarter note. Sample tempo when issued.
                            let duration =
                                (u64::from(ticks) * 60 * 1024 * u64::from(crate::SOURCE_RATE))
                                    .div_ceil(u64::from(self.bpm_1024) * 384);
                            self.wait_reference
                                .checked_add(duration)
                                .context("beat wait overflow")?
                        } else {
                            u64::MAX
                        };
                        self.wait = Wait {
                            until,
                            key_off,
                            sample_end,
                        };
                    }
                }
                Command::RandomWait {
                    upper_ms,
                    key_off,
                    sample_end,
                } => {
                    if upper_ms != 0
                        && !(key_off && self.key_off)
                        && !(sample_end && self.sample_ended())
                    {
                        let duration = self.random.below(upper_ms);
                        self.wait = Wait {
                            until: self.now()
                                + crate::volume::frames_from_millis(u64::from(duration))?,
                            key_off,
                            sample_end,
                        };
                    }
                }
                Command::Priority { value } => self.priority = value,
                Command::ExclusiveGroup { group, kill } => {
                    self.exclusive_group = group;
                    self.host_request = (group != 0).then_some(HostRequest::Group { group, kill });
                    if group != 0 {
                        break;
                    }
                }
                Command::VolumeCurve {
                    alternate,
                    interaural_delay,
                } => {
                    self.alternate_volume = alternate;
                    self.interaural_delay = interaural_delay;
                }
                Command::Interpolation { mode, coefficients } => {
                    ensure!(
                        coefficients < 4,
                        "invalid interpolation coefficient selection"
                    );
                    // Loops can repeat the current selection without resetting source state.
                    ensure!(
                        self.source.is_none()
                            || (self.source_mode, self.coefficient_set)
                                == (mode, usize::from(coefficients)),
                        "changing an active source mode is not implemented"
                    );
                    self.source_mode = mode;
                    self.coefficient_set = usize::from(coefficients);
                }
                Command::ModulationDepth { semitones, cents } => {
                    let coarse = i32::from(semitones) * 256;
                    let fine = i32::from(cents) * 256 / 100;
                    self.vibrato.modulation_depth_8 = (if coarse < 0 {
                        coarse - fine
                    } else {
                        coarse + fine
                    }) as i16;
                }
                Command::Vibrato {
                    period_ms,
                    depth_8,
                    reverse,
                    scale_by_modulation,
                } => {
                    self.vibrato.depth_8 = i32::from(depth_8);
                    self.vibrato.scale_by_modulation = scale_by_modulation;
                    self.vibrato.oscillator.set(period_ms, reverse);
                }
                Command::Lfo { period_ms } => self.lfo.set(period_ms, false),
                Command::TremoloInput { input } => self.tremolo_input = input,
                Command::Tremolo {
                    scale,
                    modulation_scale,
                } => self.tremolo = modulation::Tremolo::new(scale, modulation_scale),
            }
        }
        Ok(())
    }

    fn pitch_wait(&mut self, milliseconds: u16, from_start: bool) {
        if milliseconds != 0 {
            self.wait.until = if milliseconds == u16::MAX {
                u64::MAX
            } else {
                crate::volume::frames_from_millis(u64::from(milliseconds)).unwrap()
                    + if from_start {
                        self.macro_started_at.unwrap()
                    } else {
                        self.now()
                    }
            };
        }
    }

    pub(super) fn control(
        &self,
        target: crate::data::ControlTarget,
        default: u16,
        controls: &super::Controls,
    ) -> Result<u16> {
        let Some((source, scale)) = self.selectors[target as usize] else {
            return Ok(default);
        };
        let value = match source {
            Operand::Variable(variable) => self.variable(variable, controls)? as i16,
            Operand::Constant(value) => value,
        };
        Ok(crate::control::signed(value, scale))
    }

    fn variable(&self, variable: Variable, controls: &super::Controls) -> Result<i32> {
        Ok(match variable {
            Variable::Controller(controller) => i32::from(match controller {
                Controller::Paired(index) => *controls
                    .paired
                    .get(usize::from(index))
                    .context("invalid paired controller")?,
                Controller::PitchBend => controls.pitch_bend,
                Controller::Surround => controls.surround,
                Controller::Lfo => (i32::from(self.lfo.value) * 2 + 8192) as u16,
            }),
            Variable::Local(index) => *self
                .variables
                .get(usize::from(index))
                .context("invalid local macro variable")?,
            Variable::Global(index) => {
                ensure!(index < 16, "invalid global macro variable");
                self.random.variable(index)
            }
        })
    }

    fn set_variable(
        &mut self,
        variable: Variable,
        value: i32,
        controls: &mut super::Controls,
    ) -> Result<()> {
        match variable {
            Variable::Controller(controller) => {
                let value = (value as i16).clamp(0, 16383) as u16;
                match controller {
                    Controller::Paired(index) => {
                        ensure!(
                            index != 6,
                            "RPN data-entry controller writes are not implemented"
                        );
                        *controls
                            .paired
                            .get_mut(usize::from(index))
                            .context("invalid paired controller")? = value;
                    }
                    Controller::PitchBend => controls.pitch_bend = value,
                    Controller::Surround => controls.surround = value,
                    Controller::Lfo => {}
                }
            }
            Variable::Local(index) => {
                *self
                    .variables
                    .get_mut(usize::from(index))
                    .context("invalid local macro variable")? = value
            }
            Variable::Global(index) => {
                ensure!(index < 16, "invalid global macro variable");
                self.random.set_variable(index, value);
            }
        }
        Ok(())
    }
}

fn scale_volume(volume: u32, factor: u16) -> u32 {
    ((u64::from(volume >> 5) * u64::from(factor)) >> 7).min(127 << 16) as u32
}

#[cfg(test)]
mod tests {
    use super::scale_volume;

    #[test]
    fn volume_scaling_preserves_ordinary_gain_and_saturates_monotonically() {
        let maximum = 127 << 16;
        for volume in [0, 1 << 16, 64 << 16, maximum] {
            assert_eq!(scale_volume(volume, 0), 0);
            assert_eq!(scale_volume(volume, 2048), volume / 2);
            assert_eq!(scale_volume(volume, 4096), volume);
            let mut previous = 0;
            for factor in 0..=u16::MAX {
                let scaled = scale_volume(volume, factor);
                assert!((previous..=maximum).contains(&scaled));
                previous = scaled;
            }
        }
        assert_eq!(scale_volume(maximum, u16::MAX), maximum);
    }
}
