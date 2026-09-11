//! A pitched music voice driven by bounded, typed instrument operations.
//! Inputs and output are values; there is no audio device or emulator state.
use crate::{
    control::{self, Combine, Term},
    data::{Note, Resources},
    dls, envelope, mix, modulation, pitch, resample,
};
use anyhow::{Context, Result, ensure};
mod execute;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires locally cooked recovery cue; no audio device"]
    fn recovery_cue_fades_from_silence_to_its_scaled_velocity_in_400_ms() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/cooked");
        let package = crate::package::Package::load(&root, "audio/field-sound-132.json").unwrap();
        let crate::data::EventKind::Notes { voices, .. } = &package.score.first_events[0].kind
        else {
            panic!("missing recovery notes")
        };
        for &note in voices {
            let mut voice = Voice::new(&package.resources, &package.tables, note).unwrap();
            voice.commands().unwrap();
            assert_eq!(voice.volume, 0);
            for frame in 0..=12800 {
                voice.prepare_frame(Controls::default()).unwrap();
                if frame % 160 == 159 {
                    voice.mix_block(&mut [[[0; 2]; 3]; 160]).unwrap();
                }
                if frame == 6400 {
                    assert_eq!(voice.volume, 3_251_200);
                }
            }
            assert_eq!(voice.volume, 6_502_400);
            assert!(voice.volume_ramp.is_none());
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Tables {
    pub mix: mix::Tables,
    pub pitch: pitch::Tables,
    pub dls: dls::Tables,
    pub modulation: modulation::Tables,
    pub coefficients: resample::Coefficients,
}

impl Tables {
    pub fn validate(&self) -> Result<()> {
        self.mix.validate()?;
        self.pitch.validate()?;
        self.dls.validate()?;
        self.modulation.validate()
    }
}

#[derive(Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Controls {
    /// Output mode is runtime state; keep the score's authored pan intact.
    #[serde(skip)]
    pub mono: bool,
    pub group_volume: f32,
    pub volume: u8,
    pub expression: u8,
    pub pan: u8,
    pub post: [u8; 2],
    pub modulation: u16,
    pub pitch_bend: u16,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            mono: false,
            group_volume: 1.0,
            volume: 127,
            expression: 127,
            pan: mix::CENTER_PAN,
            post: [0; 2],
            modulation: 0,
            pitch_bend: 8192,
        }
    }
}

#[derive(Clone, Copy)]
enum Parameters {
    Ordinary(envelope::Parameters),
    Dls(dls::Parameters),
}
enum Envelope<'a> {
    Ordinary(envelope::Envelope),
    Dls(dls::Envelope<'a>),
}
impl<'a> Envelope<'a> {
    fn new(parameters: Parameters, tables: &'a Tables) -> Result<Self> {
        Ok(match parameters {
            Parameters::Ordinary(p) => Self::Ordinary(envelope::Envelope::new(p)),
            Parameters::Dls(p) => Self::Dls(dls::Envelope::new(p, &tables.dls)?),
        })
    }
    fn release(&mut self) {
        match self {
            Self::Ordinary(e) => e.release(),
            Self::Dls(e) => e.release(),
        }
    }
    fn next_gain(&mut self, block_start: bool) -> u16 {
        match self {
            Self::Ordinary(e) => e.next_gain_at(block_start),
            Self::Dls(e) => e.next_gain_at(block_start),
        }
    }
    fn is_done(&self) -> bool {
        match self {
            Self::Ordinary(e) => e.is_done(),
            Self::Dls(e) => e.is_done(),
        }
    }
}

#[derive(Default)]
struct Wait {
    until_ms: u32,
    key_off: bool,
    sample_end: bool,
}

pub struct Voice<'a> {
    resources: &'a Resources,
    tables: &'a Tables,
    pub macro_id: u16,
    pc: usize,
    instructions: u32,
    frame: u32,
    block_phase: u32,
    wait: Wait,
    key_off: bool,
    key_off_trap: Option<(u16, usize)>,
    released: bool,
    done: bool,
    original_key: u8,
    key: u8,
    cents: i8,
    velocity: u8,
    volume: u32,
    volume_ramp: Option<(i32, i32)>,
    auxiliary_override: [Option<u8>; 2],
    pitch_sweep: Option<(i32, i32, u8, i32)>,
    age_decay: u32,
    alternate_volume: bool,
    pan: u8,
    source_mode: u8,
    coefficient_set: usize,
    source: Option<(resample::SampleCursor<'a>, resample::Resampler<'a>)>,
    sample_finished: bool,
    parameters: Parameters,
    envelope: Envelope<'a>,
    pitch_envelope: Option<(dls::Envelope<'a>, i16)>,
    prepared_woke: Option<bool>,
    vibrato: modulation::Vibrato,
    lfo: modulation::Oscillator,
    tremolo: modulation::Tremolo,
    lfo_to_tremolo: bool,
    previous_controls: Option<Controls>,
    last_pitch_ms: u32,
    last_volume_ms: u32,
    priority: u8,
    pub(crate) exclusive_group: u8,
    pub(crate) group_request: Option<(u8, bool)>,
    priority_age: u32,
    gains: Option<[[mix::GainRamp; 2]; 3]>,
    targets: Option<[[u16; 2]; 3]>,
    changed: [bool; 3],
    pending: [(i16, u16); 160],
    pending_start: usize,
    pending_end: usize,
}

impl<'a> Voice<'a> {
    pub fn new(resources: &'a Resources, tables: &'a Tables, note: Note) -> Result<Self> {
        Self::new_at(resources, tables, note, 0)
    }

    /// Start a note on the sequencer's shared millisecond and DSP block clock.
    pub fn new_at(
        resources: &'a Resources,
        tables: &'a Tables,
        note: Note,
        start_frame: u32,
    ) -> Result<Self> {
        ensure!(
            start_frame.is_multiple_of(32),
            "note starts between control boundaries"
        );
        ensure!(
            note.key < 128 && note.velocity < 128 && note.pan < 128,
            "invalid music note"
        );
        let parameters = Parameters::Ordinary(envelope::Parameters::default());
        Ok(Self {
            resources,
            tables,
            macro_id: note.macro_id,
            pc: 0,
            instructions: 0,
            frame: 0,
            block_phase: start_frame % 160,
            wait: Wait::default(),
            key_off: false,
            key_off_trap: None,
            released: false,
            done: false,
            original_key: note.key,
            key: note.key,
            cents: 0,
            velocity: note.velocity,
            volume: u32::from(note.velocity) << 16,
            volume_ramp: None,
            auxiliary_override: [None; 2],
            pitch_sweep: None,
            age_decay: 8,
            alternate_volume: false,
            pan: note.pan,
            source_mode: 0,
            coefficient_set: 0,
            source: None,
            sample_finished: true,
            parameters,
            envelope: Envelope::new(parameters, tables)?,
            pitch_envelope: None,
            prepared_woke: None,
            vibrato: Default::default(),
            lfo: Default::default(),
            tremolo: Default::default(),
            lfo_to_tremolo: false,
            previous_controls: None,
            last_pitch_ms: 0,
            last_volume_ms: 0,
            priority: note.priority,
            exclusive_group: 0,
            group_request: None,
            priority_age: 60000,
            gains: None,
            targets: None,
            changed: [false; 3],
            pending: [(0, 0); 160],
            pending_start: (start_frame % 160) as usize,
            pending_end: (start_frame % 160) as usize,
        })
    }

    /// The sequencer delivers note-off at a millisecond boundary.
    pub fn key_off(&mut self) -> Result<()> {
        ensure!(
            self.frame.is_multiple_of(32),
            "note-off must be on a control boundary"
        );
        self.key_off = true;
        if let Some((id, pc)) = self.key_off_trap {
            self.macro_id = id;
            self.pc = pc;
            self.wait = Wait::default();
        }
        Ok(())
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    pub(crate) fn kill(&mut self) {
        self.done = true;
        self.source = None;
        self.sample_finished = true;
    }

    pub(crate) fn prepare_commands(&mut self) -> Result<()> {
        if self.frame.is_multiple_of(32) {
            self.prepared_woke = Some(self.ready());
            self.commands()?;
        }
        Ok(())
    }

    pub(crate) fn retained_lfo(&self) -> u32 {
        self.lfo.counter()
    }

    pub(crate) fn restore_lfo(&mut self, counter: u32) {
        self.lfo.restore_counter(counter);
    }

    pub(crate) fn priority(&self) -> u32 {
        (u32::from(self.priority) << 24) | self.priority_age
    }

    pub(crate) fn source_active(&self) -> bool {
        self.source.is_some() && !self.sample_finished && !self.done
    }

    pub(crate) fn waits_for_sample_end(&self) -> bool {
        self.wait.sample_end
    }

    fn sample_ended(&self) -> bool {
        // Publish envelope completion after collecting the whole control block.
        self.sample_finished
    }

    fn ready(&self) -> bool {
        self.frame / 32 >= self.wait.until_ms
            || (self.wait.key_off && self.key_off)
            || (self.wait.sample_end && self.sample_ended())
    }

    /// Prepare one source sample; mix on shared block boundaries. Volume can
    /// change later in a block while pitch and envelope retain their control phase.
    pub fn prepare_frame(&mut self, controls: Controls) -> Result<()> {
        ensure!(
            [
                controls.volume,
                controls.expression,
                controls.pan,
                controls.post[0],
                controls.post[1]
            ]
            .into_iter()
            .all(|v| v < 128)
                && controls.pitch_bend < 16384
                && controls.modulation < 16384
                && controls.group_volume.is_finite()
                && (0.0..=1.0).contains(&controls.group_volume),
            "invalid music channel controls"
        );
        if self.done {
            return Ok(());
        }
        let phase = ((self.frame + self.block_phase) % 160) as usize;
        ensure!(
            phase == self.pending_end,
            "music block must be mixed before preparing the next block"
        );
        let pitch_envelope = self.pitch_envelope.as_mut().map_or(0, |(envelope, depth)| {
            (i32::from(*depth)
                * i32::from(
                    envelope.next_gain_at((self.frame + self.block_phase).is_multiple_of(160)),
                ))
                >> 7
        });
        if self.frame.is_multiple_of(32) {
            // Resuming a macro wakes both scheduled controls.
            let woke = if let Some(woke) = self.prepared_woke.take() {
                woke
            } else {
                let woke = self.ready();
                self.commands()?;
                woke
            };
            if self.done {
                return Ok(());
            }
            let now = self.frame / 32;
            let pitch_dirty = self.previous_controls.is_none_or(|p| {
                p.pitch_bend != controls.pitch_bend || p.modulation != controls.modulation
            });
            if now - self.last_pitch_ms >= 15 || pitch_dirty || woke {
                let delta = now - self.last_pitch_ms;
                self.last_pitch_ms = now;
                self.lfo.advance(delta, &self.tables.modulation);
                self.vibrato
                    .oscillator
                    .advance(delta, &self.tables.modulation);
                let sweep = if let Some((value, remaining, period, step)) = &mut self.pitch_sweep {
                    *remaining -= (delta * 4096) as i32;
                    if *remaining <= 0 {
                        *remaining = i32::from(*period) << 16;
                        *value = 0;
                    } else {
                        *value =
                            value.wrapping_add((*step >> 12).wrapping_mul((delta * 256) as i32));
                    }
                    *value >> 16
                } else {
                    0
                };
                if let Some((cursor, source)) = &mut self.source {
                    let pitch = (i32::from(self.key) << 16)
                        + (i32::from(self.cents) << 16) / 100
                        + (i32::from(controls.pitch_bend) - 8192) * 2 * 8
                        + pitch_envelope
                        + self.vibrato.pitch_offset(controls.modulation);
                    let ratio = self.tables.pitch.ratio(pitch, cursor.sample())?;
                    source.set_ratio(
                        (i64::from(ratio) + i64::from(sweep) * 16).clamp(0, 0x3fff0) as u32,
                    )?;
                }
            }
            // Sample group gain on the next shared control block; a group change
            // alone does not wake the voice at the next millisecond.
            let channel_dirty = self.previous_controls.is_none_or(|mut previous| {
                previous.group_volume = controls.group_volume;
                previous != controls
            });
            if (self.frame + self.block_phase).is_multiple_of(160) || channel_dirty || woke {
                // Age allocation priority by eight units per millisecond, starting at 60000.
                self.priority_age = self
                    .priority_age
                    .saturating_sub((now - self.last_volume_ms) * self.age_decay);
                if let Some((end, step)) = self.volume_ramp {
                    let next = self.volume as i32 + (now - self.last_volume_ms) as i32 * step;
                    self.volume = (if step < 0 {
                        next.max(end)
                    } else {
                        next.min(end)
                    }) as u32;
                    if self.volume as i32 == end {
                        self.volume_ramp = None;
                    }
                }
                self.last_volume_ms = now;
                let controller = control::evaluate(&[
                    Term {
                        value: i16::from(controls.volume) << 7,
                        signed: false,
                        scale: 65536,
                        combine: Combine::Set,
                    },
                    Term {
                        value: i16::from(controls.expression) << 7,
                        signed: false,
                        scale: 65536,
                        combine: Combine::Multiply,
                    },
                ])?;
                let pan = if controls.mono {
                    mix::CENTER_PAN
                } else {
                    (i16::from(self.pan) + i16::from(controls.pan) - i16::from(mix::CENTER_PAN))
                        .clamp(0, 127) as u8
                };
                let lfo = if self.lfo_to_tremolo {
                    self.lfo.value
                } else {
                    0
                };
                let scale = self
                    .tremolo
                    .gain(lfo, controls.modulation, &self.tables.modulation);
                let targets = self.tables.mix.gains_for(mix::Parameters {
                    volume: self.volume,
                    controller,
                    pan,
                    post: std::array::from_fn(|i| {
                        u16::from(self.auxiliary_override[i].unwrap_or(controls.post[i])) << 7
                    }),
                    scale,
                    group_volume: controls.group_volume,
                    aux_a: 128,
                    alternate: self.alternate_volume,
                });
                if let Some(previous) = self.targets {
                    for (bus, target) in targets.iter().enumerate() {
                        self.changed[bus] |= previous[bus] != *target;
                    }
                }
                self.targets = Some(targets);
            }
            self.previous_controls = Some(controls);
        }
        let sample = self.source.as_mut().map_or(0, |(cursor, source)| {
            source.next_sample(|| cursor.next_sample())
        });
        let envelope = self
            .envelope
            .next_gain((self.frame + self.block_phase).is_multiple_of(160));
        self.pending[phase] = (sample, envelope);
        self.pending_end += 1;
        self.frame += 1;
        Ok(())
    }

    /// Add the prepared block to the studio buses, using its final targets.
    /// This collects control decisions without keeping runtime DSP structures.
    pub fn mix_block(&mut self, output: &mut [[[i32; 2]; 3]; 160]) -> Result<()> {
        if self.pending_start == self.pending_end {
            return Ok(());
        }
        let targets = self.targets.context("missing music gain targets")?;
        if let Some(gains) = &mut self.gains {
            for (bus, channels) in gains.iter_mut().enumerate() {
                for (channel, gain) in channels.iter_mut().enumerate() {
                    gain.set_target_changed(targets[bus][channel], self.changed[bus]);
                }
            }
        } else {
            self.gains = Some(targets.map(|bus| bus.map(mix::GainRamp::new)));
        }
        let gains = self.gains.as_mut().unwrap();
        for (frame, &(sample, envelope)) in output[self.pending_start..self.pending_end]
            .iter_mut()
            .zip(&self.pending[self.pending_start..self.pending_end])
        {
            for (bus, channels) in frame.iter_mut().zip(gains.iter_mut()) {
                for (output, gain) in bus.iter_mut().zip(channels) {
                    *output += i32::from(mix::apply(sample, envelope, gain.next_gain()));
                }
            }
        }
        self.changed = [false; 3];
        self.pending_start = 0;
        self.pending_end = 0;
        if self.envelope.is_done()
            || self
                .source
                .as_ref()
                .is_none_or(|(cursor, _)| cursor.is_done())
        {
            self.sample_finished = true;
        }
        Ok(())
    }
}
