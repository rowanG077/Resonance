//! Typed instrument operations compiled by the importer.
use super::{Envelope, Parameters, Voice, Wait};
use crate::{
    data::{Command, Envelope as Definition, Interpolation},
    modulation, resample,
};
use anyhow::{Context, Result, ensure};

impl Voice<'_> {
    pub(super) fn commands(&mut self) -> Result<()> {
        while !self.done && self.ready() {
            self.instructions += 1;
            ensure!(
                self.instructions <= 65536,
                "music program instruction budget exhausted"
            );
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
                Command::End => self.done = true,
                Command::Jump {
                    program,
                    instruction,
                } => {
                    self.macro_id = program;
                    self.pc = instruction;
                }
                Command::Envelope { envelope } => {
                    self.parameters = match envelope {
                        Definition::Ordinary(p) => Parameters::Ordinary(p),
                        Definition::Dls(p) => Parameters::Dls(p.resolve(
                            &self.tables.dls,
                            self.velocity,
                            self.original_key,
                        )?),
                    };
                }
                Command::PitchEnvelope {
                    envelope,
                    sustain,
                    depth_8,
                } => {
                    let mut parameters =
                        envelope.resolve(&self.tables.dls, self.velocity, self.original_key)?;
                    parameters.sustain = 193
                        - u16::from(self.tables.dls.inverse[usize::from((sustain >> 2).min(1023))]);
                    self.pitch_envelope = Some((
                        crate::dls::Envelope::new(parameters, &self.tables.dls)?,
                        depth_8,
                    ));
                }
                Command::StartSample { sample } => {
                    let cursor = resample::SampleCursor::new(self.resources.sample(sample)?)?;
                    let ratio = self.tables.pitch.ratio(
                        (i32::from(self.key) << 16) + (i32::from(self.cents) << 16) / 100,
                        cursor.sample(),
                    )?;
                    let mode = match self.source_mode {
                        0 => resample::Mode::Polyphase(
                            &self.tables.coefficients.0[self.coefficient_set],
                        ),
                        1 => resample::Mode::Linear,
                        2 => resample::Mode::Direct,
                        _ => unreachable!(),
                    };
                    self.source = Some((cursor, resample::Resampler::new(mode, ratio)?));
                    self.sample_finished = false;
                    self.envelope = Envelope::new(self.parameters, self.tables)?;
                    self.released = false;
                }
                Command::StopSample => {
                    self.source = None;
                    self.sample_finished = true;
                }
                Command::Release => {
                    if !self.released {
                        self.envelope.release();
                        if let Some((envelope, _)) = &mut self.pitch_envelope {
                            envelope.release();
                        }
                        self.released = true;
                    }
                }
                Command::PitchOffset {
                    from_original,
                    semitones,
                    cents,
                } => {
                    let base = if from_original {
                        self.original_key
                    } else {
                        self.key
                    };
                    self.key = (i16::from(base) + i16::from(semitones)).clamp(0, 127) as u8;
                    self.cents = cents;
                }
                Command::SetNote { key, cents } => {
                    ensure!(key < 128, "invalid instrument key");
                    self.key = key;
                    self.cents = cents;
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
                    self.volume =
                        ((start >> 5).wrapping_mul(u32::from(factor)) >> 7).min(127 << 16);
                }
                Command::SetVolume {
                    factor,
                    offset,
                    from_velocity,
                } => {
                    let start = if from_velocity {
                        u32::from(self.velocity) << 16
                    } else {
                        self.volume
                    };
                    self.volume = (start * u32::from(factor) / 127 + (u32::from(offset) << 16))
                        .min(127 << 16);
                }
                Command::FadeVolume {
                    factor,
                    offset,
                    milliseconds,
                    from_silence,
                } => {
                    let target = (((self.volume * u32::from(factor)) >> 7)
                        + (u32::from(offset) << 16))
                        .min(127 << 16) as i32;
                    if from_silence {
                        self.volume = 0;
                    }
                    self.volume_ramp = Some((
                        target,
                        (target - self.volume as i32) / i32::from(milliseconds.max(1)),
                    ));
                }
                Command::Auxiliary { bus, value } => {
                    ensure!(bus < 2 && value < 128, "invalid auxiliary control");
                    self.auxiliary_override[usize::from(bus)] = Some(value);
                }
                Command::SetAge { value } => self.priority_age = u32::from(value),
                Command::AddAge { value } => {
                    self.priority_age =
                        (self.priority_age as i32 + i32::from(value)).clamp(0, 65535) as u32
                }
                Command::AgePeriod { milliseconds } => {
                    self.age_decay = (self.priority_age << 7)
                        .checked_div(milliseconds)
                        .unwrap_or(0);
                }
                Command::PitchSweep {
                    step_hz,
                    period,
                    wait_ms,
                } => {
                    let step = ((4096.0 * f32::from(step_hz)) / 32000.0) as i32;
                    self.pitch_sweep =
                        (period != 0).then_some((0, i32::from(period) << 16, period, step << 16));
                    self.wait.until_ms = self.frame / 32 + u32::from(wait_ms);
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
                    let base = if from_start { 0 } else { self.frame / 32 };
                    self.wait = Wait {
                        until_ms: milliseconds
                            .map_or(u32::MAX, |duration| base + u32::from(duration)),
                        key_off,
                        sample_end,
                    };
                }
                Command::Priority { value } => self.priority = value,
                Command::ExclusiveGroup { group, kill } => {
                    self.exclusive_group = group;
                    self.group_request = (group != 0).then_some((group, kill));
                }
                Command::VolumeCurve { alternate } => self.alternate_volume = alternate,
                Command::Interpolation { mode, coefficients } => {
                    ensure!(
                        self.source.is_none(),
                        "changing an active source mode is not implemented"
                    );
                    ensure!(
                        coefficients < 4,
                        "invalid interpolation coefficient selection"
                    );
                    self.source_mode = match mode {
                        Interpolation::Polyphase => 0,
                        Interpolation::Linear => 1,
                        Interpolation::Direct => 2,
                    };
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
                    semitones,
                    cents,
                    scale_by_modulation,
                } => {
                    ensure!(semitones < 128 && cents < 128, "invalid vibrato depths");
                    self.vibrato.depth_8 =
                        i32::from(semitones) * 256 + i32::from(cents) * 256 / 100;
                    self.vibrato.scale_by_modulation = scale_by_modulation;
                    self.vibrato.oscillator.set(period_ms, false);
                }
                Command::Lfo { period_ms } => self.lfo.set_lfo(period_ms),
                Command::TremoloFromLfo => self.lfo_to_tremolo = true,
                Command::Tremolo {
                    scale,
                    modulation_scale,
                } => self.tremolo = modulation::Tremolo::new(scale, modulation_scale),
            }
        }
        Ok(())
    }
}
