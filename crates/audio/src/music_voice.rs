//! A pitched music voice driven by bounded, typed instrument operations.
//! Inputs and output are values; there is no audio device or emulator state.
use crate::{
    control::{self, Combine, Term},
    data::{Interpolation, Note, Resources},
    dls, envelope, mix, modulation, pitch, resample,
};
use anyhow::{Context, Result, ensure};
mod execute;

pub(crate) enum HostRequest {
    Group { group: u8, kill: bool },
    Spawn { note: Note, instruction: u16 },
    Message { target: MessageTarget, value: i32 },
}

pub(crate) enum MessageTarget {
    Handle(u32),
    Macro(u16),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_vibrato_preserves_phase_and_modulation_rounding() {
        let tables = modulation::Tables {
            sine: std::array::from_fn(|i| i as i16 * 4),
            tremolo: [1.; 5],
        };
        let mut vibrato = modulation::Vibrato {
            depth_8: 25,
            scale_by_modulation: true,
            ..Default::default()
        };
        vibrato.oscillator.set(200, true);
        vibrato.oscillator.advance(50, &tables);
        assert_eq!(vibrato.pitch_offset(0), 0);
        assert_eq!(vibrato.pitch_offset(64 << 7), -3197);
        vibrato.oscillator.advance(100, &tables);
        assert_eq!(vibrato.pitch_offset(64 << 7), 3196);
        vibrato.depth_8 = 1021;
        vibrato.scale_by_modulation = false;
        for (reverse, offsets) in [(false, [174080, -174081, 0]), (true, [-174081, 174080, 0])] {
            vibrato.oscillator.set(45, reverse);
            for expected in offsets {
                vibrato.oscillator.advance(15, &tables);
                assert_eq!(vibrato.pitch_offset(0), expected);
            }
        }
    }

    #[test]
    fn volume_curves_keep_signed_fractional_segments_and_raw_endpoints() {
        let mut curve = crate::data::VolumeCurve([0; 128]);
        curve.0[..3].copy_from_slice(&[184, 11, 136]);
        curve.0[127] = 255;
        assert_eq!(curve.translate(0), 184 << 16);
        assert_eq!(curve.translate(32768), 6_389_760);
        assert_eq!(curve.translate(65536 + 16384), 2_768_896);
        assert_eq!(curve.translate(127 << 16), 255 << 16);
        let json = serde_json::to_value(curve).unwrap();
        assert_eq!(json.as_array().unwrap().len(), 128);
        assert_eq!(
            serde_json::from_value::<crate::data::VolumeCurve>(json)
                .unwrap()
                .0,
            curve.0
        );
        for length in [127, 129] {
            assert!(
                serde_json::from_value::<crate::data::VolumeCurve>(serde_json::json!(vec![
                    0;
                    length
                ]))
                .is_err()
            );
        }
    }

    #[test]
    fn pan_ramps_keep_fractional_steps_and_unclamped_endpoints() {
        let mut pan = PanRamp::new(10, 5, 3);
        assert_eq!(pan.value, 655360);
        for expected in [764588, 873814, 983040] {
            pan.advance(1);
            assert_eq!(pan.value, expected);
        }
        pan.advance(100);
        assert_eq!(pan.value, 15 << 16);
        let mut pan = PanRamp::new(100, -100, 3);
        pan.advance(1);
        assert_eq!(pan.value, 4369066);
        pan.advance(9);
        assert_eq!(pan.value, 0);
        let mut pan = PanRamp::new(0, -1, 0);
        assert_eq!(pan.value, 0);
        pan.advance(0);
        assert_eq!(pan.value, -(1 << 16));
        let mut pan = PanRamp::new(127, 10, 2);
        pan.advance(1);
        assert_eq!(pan.value, 127 << 16);
        pan.advance(1);
        assert_eq!(pan.value, 137 << 16);
    }

    #[test]
    fn sweep_expiry_discards_overshoot_and_zero_period_disables_only_its_slot() {
        let mut up = Sweep::new(1000, 2).unwrap();
        let mut down = Sweep::new(-1000, 2).unwrap();
        assert_eq!(up.advance(15), 120 << 16);
        assert_eq!(down.advance(15), -(120 << 16));
        assert_eq!(up.advance(15), 240 << 16);
        assert_eq!(up.advance(3), 0);
        assert_eq!(
            up.remaining,
            2 << 16,
            "period restarts, without carrying the extra millisecond"
        );
        assert_eq!(up.advance(1), 8 << 16);
        assert!(Sweep::new(1000, 0).is_none());
    }

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
            voice.commands(&mut Controls::default()).unwrap();
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
    /// MIDI controller pairs (MSB0..31 and LSB32..63), in fourteen-bit units.
    pub paired: [u16; 32],
    pub post: [u8; 2],
    pub pitch_bend: u16,
    pub surround: u16,
}

impl Default for Controls {
    fn default() -> Self {
        let mut paired = [0; 32];
        paired[7] = 127 << 7;
        paired[10] = u16::from(mix::CENTER_PAN) << 7;
        paired[11] = 127 << 7;
        Self {
            mono: false,
            group_volume: 1.0,
            paired,
            post: [0; 2],
            pitch_bend: 8192,
            surround: 8192,
        }
    }
}

impl Controls {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.paired
                .iter()
                .chain([&self.pitch_bend, &self.surround])
                .all(|&v| v < 16384)
                && self.post.iter().all(|&v| v < 128)
                && self.group_volume.is_finite()
                && (0.0..=1.0).contains(&self.group_volume),
            "invalid music channel controls"
        );
        Ok(())
    }
    pub fn set_coarse(&mut self, index: usize, value: u8) {
        self.paired[index] = (u16::from(value) << 7) | (self.paired[index] & 127);
    }
    pub(crate) fn child(&self) -> Self {
        let mut child = Self::default();
        for index in [7, 10] {
            child.paired[index] = self.paired[index];
        }
        child.post[0] = self.post[0];
        child.pitch_bend = self.pitch_bend;
        child.surround = self.surround;
        child.group_volume = self.group_volume;
        child.mono = self.mono;
        child
    }
}

#[derive(Clone, Copy)]
enum Parameters {
    Ordinary(envelope::Parameters),
    Dls(dls::Parameters),
}

struct PanRamp {
    value: i32,
    target: i32,
    step: i32,
    remaining_ms: u32,
}

struct VolumeRamp {
    /// Instant volume setters do not change a running fade's trajectory.
    current: i32,
    target: i32,
    step: i32,
}

impl VolumeRamp {
    fn new(current: u32, target: i32, milliseconds: u16) -> Self {
        Self {
            current: current as i32,
            target,
            step: (target - current as i32) / i32::from(milliseconds.max(1)),
        }
    }

    fn advance(&mut self, milliseconds: u32) -> u32 {
        let next = self.current + milliseconds as i32 * self.step;
        self.current = if self.step < 0 {
            next.max(self.target)
        } else {
            next.min(self.target)
        };
        self.current as u32
    }
}

impl PanRamp {
    fn new(initial: u8, delta: i8, milliseconds: u16) -> Self {
        let value = i32::from(initial) << 16;
        let delta = i32::from(delta) << 16;
        Self {
            value,
            target: value + delta,
            step: delta / i32::from(milliseconds.max(1)),
            remaining_ms: u32::from(milliseconds),
        }
    }

    fn advance(&mut self, milliseconds: u32) {
        if self.value != self.target {
            self.remaining_ms = self.remaining_ms.saturating_sub(milliseconds);
            self.value = if self.remaining_ms == 0 {
                self.target
            } else {
                (self.target - self.remaining_ms as i32 * self.step).clamp(0, 127 << 16)
            };
        }
    }
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
    until: u64,
    key_off: bool,
    sample_end: bool,
}

struct Sweep {
    value: i32,
    remaining: i32,
    period: u8,
    step: i32,
}

impl Sweep {
    fn new(step_hz: i16, period: u8) -> Option<Self> {
        let step = ((4096.0 * f32::from(step_hz)) / 32000.0) as i32;
        (period != 0).then_some(Self {
            value: 0,
            remaining: i32::from(period) << 16,
            period,
            step: step << 16,
        })
    }

    fn advance(&mut self, milliseconds: u32) -> i32 {
        self.remaining -= (milliseconds * 4096) as i32;
        if self.remaining <= 0 {
            self.remaining = i32::from(self.period) << 16;
            self.value = 0;
        } else {
            self.value = self
                .value
                .wrapping_add((self.step >> 12).wrapping_mul((milliseconds * 256) as i32));
        }
        self.value
    }
}

pub struct Voice<'a> {
    resources: &'a Resources,
    tables: &'a Tables,
    pub macro_id: u16,
    pc: usize,
    loop_remaining: u16,
    frame: u32,
    block_phase: u32,
    /// Synthesizer timestamps use 1/256 ms, retaining fractional beat-wait deadlines.
    started_at: u64,
    macro_started_at: Option<u64>,
    wait_reference: u64,
    bpm_1024: u32,
    random: Option<crate::sequence::shared::Control>,
    wait_order: u64,
    runnable: Option<u64>,
    source_change: Option<(u64, bool)>,
    stopped_subframe: Option<u32>,
    wait: Wait,
    key_off: bool,
    key_off_trap: Option<(u16, usize)>,
    message_trap: Option<(u16, usize)>,
    messages: std::collections::VecDeque<i32>,
    released: bool,
    done: bool,
    original_key: u8,
    key: u8,
    cents: i8,
    velocity: u8,
    volume: u32,
    volume_control: Option<u16>,
    volume_ramp: Option<VolumeRamp>,
    auxiliary_override: [Option<u8>; 2],
    pitch_sweeps: [Option<Sweep>; 2],
    age_decay: u16,
    alternate_volume: bool,
    interaural_delay: bool,
    delay: Option<mix::StereoDelay>,
    pan: [PanRamp; 2],
    source_mode: Interpolation,
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
    pub(crate) host_request: Option<HostRequest>,
    variables: [i32; 16],
    pub(crate) original_macro: u16,
    pub(crate) handle: u32,
    pub(crate) last_child: u32,
    priority_age: u32,
    priority_revision: u64,
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
        start_frame: u64,
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
            loop_remaining: 0,
            frame: 0,
            block_phase: (start_frame % 160) as u32,
            started_at: start_frame
                .checked_mul(8)
                .context("music start time overflow")?,
            wait_reference: start_frame * 8,
            macro_started_at: Some(start_frame * 8),
            bpm_1024: 120 * 1024,
            random: None,
            wait_order: 0,
            runnable: None,
            source_change: None,
            stopped_subframe: None,
            wait: Wait::default(),
            key_off: false,
            key_off_trap: None,
            message_trap: None,
            messages: Default::default(),
            released: false,
            done: false,
            original_key: note.key,
            key: note.key,
            cents: 0,
            velocity: note.velocity,
            volume: u32::from(note.velocity) << 16,
            volume_control: None,
            volume_ramp: None,
            auxiliary_override: [None; 2],
            pitch_sweeps: [None, None],
            age_decay: 1024,
            alternate_volume: false,
            interaural_delay: false,
            delay: None,
            pan: [PanRamp::new(note.pan, 0, 0), PanRamp::new(0, 0, 0)],
            source_mode: Interpolation::Polyphase,
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
            host_request: None,
            variables: [0; 16],
            original_macro: note.macro_id,
            handle: u32::MAX,
            last_child: u32::MAX,
            priority_age: 60000 << 15,
            priority_revision: 0,
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
        if let Some((id, pc)) = self.key_off_trap.take() {
            self.macro_id = id;
            self.pc = pc;
            self.wake();
        } else if self.wait.key_off {
            self.wake();
        }
        Ok(())
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    pub(crate) fn send_message(&mut self, value: i32) {
        if self.messages.len() == 4 {
            return;
        }
        self.messages.push_back(value);
        if let Some((program, instruction)) = self.message_trap.take() {
            self.macro_id = program;
            self.pc = instruction;
            self.wake();
        }
    }

    pub(crate) fn mailbox_state(&self) -> (u8, bool) {
        (self.messages.len() as u8, self.message_trap.is_some())
    }

    /// Beat waits sample the current tempo when issued; an existing deadline is retained.
    pub fn set_tempo(&mut self, bpm_1024: u32) {
        self.bpm_1024 = bpm_1024;
    }

    pub(crate) fn set_random(&mut self, random: crate::sequence::shared::Control) {
        self.runnable = Some(random.schedule());
        self.random = Some(random);
    }

    pub(crate) fn wake_order(&self) -> Option<crate::sequence::shared::Wake> {
        use crate::sequence::shared::Wake;
        if self.done {
            None
        } else if let Some(order) = self.runnable {
            Some(Wake::Runnable(order))
        } else {
            (self.now() >= self.wait.until).then_some(Wake::Timer {
                deadline: self.wait.until,
                order: self.wait_order,
            })
        }
    }

    fn wake(&mut self) {
        if self.runnable.is_none() {
            self.runnable = self.random.as_ref().map(|random| random.schedule());
            self.wait = Wait::default();
            self.wait_reference = self.now();
            if self.prepared_woke.is_some() {
                self.prepared_woke = Some(true);
            }
        }
    }

    pub(crate) fn wake_timer(&mut self) {
        if self.runnable.is_none() && self.now() >= self.wait.until {
            let deadline = self.wait.until;
            self.wake();
            self.wait_reference = deadline;
        }
    }

    pub(crate) fn sample_end_callback(&mut self) {
        if !self.done && self.wait.sample_end && self.sample_finished {
            self.wake();
        }
    }

    pub(crate) fn defer_commands(&mut self) {
        self.prepared_woke = Some(false);
    }

    fn changed_source(&mut self, start: bool) {
        if let Some((_, pending_start)) = &mut self.source_change {
            *pending_start |= start;
        } else {
            self.source_change = self
                .random
                .as_ref()
                .map(|random| (random.schedule(), start));
        }
    }

    pub(crate) fn take_source_change(&mut self) -> Option<(u64, bool)> {
        self.source_change.take()
    }

    pub(crate) fn kill(&mut self) {
        self.done = true;
        self.source = None;
        self.sample_finished = true;
    }

    pub(crate) fn child(&self, note: Note, instruction: u16, frame: u64) -> Result<Self> {
        // The constructor copies the current scalar bytes, including values
        // outside MIDI's seven-bit input range after volume/pan automation.
        let mut child = Self::new_at(
            self.resources,
            self.tables,
            Note {
                velocity: 0,
                pan: 0,
                ..note
            },
            frame,
        )?;
        child.velocity = note.velocity;
        child.volume = u32::from(note.velocity) << 16;
        child.pan[0] = PanRamp::new(note.pan, 0, 0);
        child.pc = usize::from(instruction);
        child.interaural_delay = self.interaural_delay;
        child.macro_started_at = None;
        child.defer_commands();
        Ok(child)
    }

    pub(crate) fn prepare_commands(&mut self, controls: &mut Controls) -> Result<()> {
        if self.frame.is_multiple_of(32) {
            if self.macro_started_at.is_none() {
                let now = self.now();
                self.macro_started_at = Some(now);
                self.wait_reference = now;
            }
            self.prepared_woke = Some(self.ready());
            self.commands(controls)?;
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
        (u32::from(self.priority) << 24) | (self.priority_age >> 15)
    }

    pub(crate) fn allocation_priority(&self) -> (u8, u32, u64) {
        (self.priority, self.priority_age, self.priority_revision)
    }

    fn set_priority(&mut self, priority: u8) {
        if self.priority != priority {
            self.priority = priority;
            self.priority_revision += 1;
        }
    }

    pub(crate) fn source_active(&self) -> bool {
        self.source.is_some() && !self.sample_finished && !self.done
    }

    pub(crate) fn studio_active(&self) -> bool {
        !self.done
            && (self.source_active() || self.stopped_subframe.is_some_and(|phase| phase != 0))
    }

    pub(crate) fn waits_for_sample_end(&self) -> bool {
        self.wait.sample_end
    }

    fn sample_ended(&self) -> bool {
        // Publish envelope completion after collecting the whole control block.
        self.sample_finished
    }

    fn ready(&self) -> bool {
        self.now() >= self.wait.until
            || (self.wait.key_off && self.key_off)
            || (self.wait.sample_end && self.sample_ended())
    }

    fn now(&self) -> u64 {
        self.started_at + u64::from(self.frame) * 8
    }

    /// Prepare one source sample; mix on shared block boundaries. Volume can
    /// change later in a block while pitch and envelope retain their control phase.
    pub fn prepare_frame(&mut self, mut controls: Controls) -> Result<()> {
        controls.validate()?;
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
                self.commands(&mut controls)?;
                woke
            };
            if self.done {
                return Ok(());
            }
            let now = self.frame / 32;
            let pitch_dirty = self.previous_controls.is_none_or(|p| {
                p.pitch_bend != controls.pitch_bend || p.paired[1] != controls.paired[1]
            });
            if now - self.last_pitch_ms >= 15 || pitch_dirty || woke {
                let delta = now - self.last_pitch_ms;
                self.last_pitch_ms = now;
                for pan in &mut self.pan {
                    pan.advance(delta);
                }
                self.lfo.advance(delta, &self.tables.modulation);
                self.vibrato
                    .oscillator
                    .advance(delta, &self.tables.modulation);
                let sweep = self
                    .pitch_sweeps
                    .iter_mut()
                    .flatten()
                    .fold(0i32, |value, sweep| {
                        value.wrapping_add(sweep.advance(delta))
                    })
                    >> 16;
                if let Some((cursor, source)) = &mut self.source {
                    let pitch = (i32::from(self.key) << 16)
                        + (i32::from(self.cents) << 16) / 100
                        + (i32::from(controls.pitch_bend) - 8192) * 2 * 8
                        + pitch_envelope
                        + self.vibrato.pitch_offset(controls.paired[1]);
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
                    .saturating_sub((now - self.last_volume_ms) * 256 * u32::from(self.age_decay));
                if let Some(ramp) = &mut self.volume_ramp {
                    self.volume = ramp.advance(now - self.last_volume_ms);
                    if ramp.current == ramp.target {
                        self.volume_ramp = None;
                    }
                }
                self.last_volume_ms = now;
                let controller = self.volume_control.map_or_else(
                    || {
                        control::evaluate(&[
                            Term {
                                value: controls.paired[7] as i16,
                                signed: false,
                                scale: 65536,
                                combine: Combine::Set,
                            },
                            Term {
                                value: controls.paired[11] as i16,
                                signed: false,
                                scale: 65536,
                                combine: Combine::Multiply,
                            },
                        ])
                    },
                    Ok,
                )?;
                let pan = if controls.mono {
                    u32::from(mix::CENTER_PAN) << 16
                } else {
                    (self.pan[0].value
                        + ((i32::from(controls.paired[10]) - (i32::from(mix::CENTER_PAN) << 7))
                            << 9))
                        .clamp(0, 127 << 16) as u32
                };
                // Surround ramps retain state; stereo and mono output use only this axis.
                let lfo = if self.lfo_to_tremolo {
                    self.lfo.value
                } else {
                    0
                };
                let scale = self
                    .tremolo
                    .gain(lfo, controls.paired[1], &self.tables.modulation);
                if let Some(delay) = &mut self.delay {
                    let left = self
                        .tables
                        .mix
                        .spatial
                        .as_ref()
                        .expect("validated spatial audio")
                        .left_delay[(pan >> 16) as usize];
                    delay.shift = [left, 32 - left];
                }
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
                    interaural_delay: self.delay.is_some(),
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
        self.stopped_subframe = None;
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
            let (samples, envelope) = if let Some(delay) = &mut self.delay {
                let enveloped = ((i64::from(sample) * i64::from(envelope)) >> 15) as i16;
                (delay.next(enveloped), 32768)
            } else {
                ([sample; 2], envelope)
            };
            for (bus, channels) in frame.iter_mut().zip(gains.iter_mut()) {
                for ((output, gain), sample) in bus.iter_mut().zip(channels).zip(samples) {
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
