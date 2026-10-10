//! A pitched music voice driven by bounded, typed instrument operations.
//! Inputs and output are values; there is no audio device or emulator state.
use crate::{
    BLOCK_FRAMES, CONTROL_FRAMES, control,
    data::{ControlTarget, Interpolation, Note, Operand, Resources, TremoloInput},
    dls, envelope, mix, modulation, pitch, resample,
};
use anyhow::{Result, ensure};
mod execute;

/// Maximum authored instructions a voice may execute in one control quantum.
pub(crate) const INSTRUCTION_BUDGET: usize = 65_536;

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
    fn vibrato_uses_source_frames_and_respects_depth_and_direction() {
        let tables = modulation::Tables {
            sine: std::array::from_fn(|i| i as i16 * 4),
            tremolo: [1.; 5],
        };
        let period = crate::volume::frames_from_millis(200).unwrap();
        for reverse in [false, true] {
            let mut vibrato = modulation::Vibrato {
                depth_8: 256,
                scale_by_modulation: true,
                ..Default::default()
            };
            vibrato.oscillator.set(200, reverse);
            vibrato.oscillator.advance(period / 4, &tables);
            let peak = vibrato.pitch_offset(127 << 7);
            assert_eq!(vibrato.pitch_offset(0), 0);
            assert_eq!(peak.is_negative(), reverse);
            assert!(peak.abs() > 60_000 && peak.abs() <= 65_536);
            let half = vibrato.pitch_offset(64 << 7);
            assert!(half.abs() < peak.abs());
            vibrato.oscillator.advance(period - period / 4, &tables);
            assert_eq!(vibrato.pitch_offset(127 << 7), 0);
        }
    }

    #[test]
    fn random_notes_stay_in_authored_bounds_and_replay_reproducibly() {
        use crate::data::Command;
        let (_, _, tables) = crate::package::tests::playback_data();
        for (low, high, relative, expected) in [
            (53, 67, false, 53..=67),
            (7, 7, true, 53..=67),
            (67, 53, false, 53..=67),
        ] {
            let bank = Resources {
                samples: Default::default(),
                programs: std::collections::BTreeMap::from([(
                    1,
                    vec![
                        Command::RandomNote {
                            low,
                            high,
                            cents: 0,
                            random_cents: true,
                            relative,
                        },
                        Command::End,
                    ],
                )]),
            };
            let replay = || {
                let random = crate::sequence::shared::Control::default();
                (0..64)
                    .map(|_| {
                        let mut voice = Voice::new_at(
                            &bank,
                            &tables,
                            Note {
                                macro_id: 1,
                                key: 60,
                                velocity: 100,
                                pan: 64,
                                priority: 1,
                                max_voices: 1,
                            },
                            0,
                            random.clone(),
                        )
                        .unwrap();
                        crate::music_voice::test_frame(&mut voice, Controls::default()).unwrap();
                        assert!(expected.contains(&voice.key));
                        assert!((-100..=100).contains(&voice.cents));
                        assert!(voice.is_done());
                        (voice.key, voice.cents)
                    })
                    .collect::<Vec<_>>()
            };
            let notes = replay();
            assert_eq!(notes, replay());
            assert!(notes.windows(2).any(|pair| pair[0] != pair[1]));
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
    fn pan_ramps_use_elapsed_frames_and_preserve_authored_endpoints() {
        for (initial, delta, milliseconds) in [(10, 5, 3), (100, -100, 3), (127, 10, 2)] {
            let mut pan = PanRamp::new(initial, delta, milliseconds);
            let duration = crate::volume::frames_from_millis(u64::from(milliseconds)).unwrap();
            let initial = i32::from(initial) << 16;
            let target = initial + (i32::from(delta) << 16);
            assert_eq!(pan.value(), initial);
            for _ in 0..duration {
                pan.advance();
            }
            assert_eq!(pan.value(), target);
            pan.advance();
            assert_eq!(pan.value(), target);
        }
        assert_eq!(PanRamp::new(0, -1, 0).value(), -(1 << 16));
    }

    #[test]
    fn sweeps_preserve_small_rates_and_period_remainder() {
        for rate in [1, 1000, i16::MAX] {
            let mut up = Sweep::new(rate, 64).unwrap();
            let mut down = Sweep::new(-rate, 64).unwrap();
            assert_eq!(up.value(), 0);
            let half = up.duration / 2;
            up.advance(half);
            down.advance(half);
            assert!(up.value() > 0);
            assert_eq!(up.value(), -down.value());
            up.advance(up.duration - half);
            assert_eq!(up.value(), 0);
            up.advance(up.duration + half);
            assert_eq!(up.value(), -down.value());
            up.advance(u64::MAX);
            assert!(up.elapsed < up.duration);
        }
        assert!(Sweep::new(1000, 0).is_none());
    }

    #[test]
    #[ignore = "requires locally cooked battle audio; no audio device"]
    fn authored_battle_cues_finish_samples_and_release_loops() {
        use crate::sequence::{LiveControls, shared::Synthesizer, stream::Stream};
        let root = std::env::var_os("RESONANCE_TEST_ASSETS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets")
            });
        let descriptor: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("battle/audio.json")).unwrap())
                .unwrap();
        // These authored cues cover staggered envelopes and a looping sample
        // whose program ends after fading its volume to silence.
        for cue in [73, 80] {
            let path = descriptor["assets"]["sounds"][cue.to_string()]["path"]
                .as_str()
                .unwrap();
            let loaded = std::sync::Arc::new(crate::package::Package::load(&root, path).unwrap());
            let synth = Synthesizer::default();
            let stream = Stream::in_synthesizer(loaded, false, &synth).unwrap();
            let mut audible = false;
            let mut completed = false;
            for frame in 0..5 * 32000 {
                // Cue 80 also has sustained layers awaiting a caller key-off.
                if cue == 80 && frame == 2 * 32000 {
                    stream
                        .set_shared_controls(
                            [LiveControls {
                                release: true,
                                ..Default::default()
                            }; crate::CONTROLS_PER_BLOCK],
                        )
                        .unwrap();
                }
                synth.advance().unwrap();
                let Some(frame) = stream.shared_frame() else {
                    completed = true;
                    break;
                };
                audible |= frame.iter().flatten().any(|sample| sample.abs() > 100);
            }
            assert!(audible, "cue {cue} must contain audible samples");
            assert!(
                completed,
                "cue {cue} must complete after its authored lifetime and caller release"
            );
            assert_eq!(synth.unread_frame(), [[0; 2]; 3]);
        }
    }

    #[test]
    #[ignore = "requires locally cooked recovery cue; no audio device"]
    fn recovery_cue_fades_from_silence_to_its_scaled_velocity_in_400_ms() {
        let root = std::env::var_os("RESONANCE_TEST_ASSETS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets")
            });
        let field: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("fields/map-340-audio.json")).unwrap())
                .unwrap();
        let path = field["sounds"]["132"]["path"].as_str().unwrap();
        let package = crate::package::Package::load(&root, path).unwrap();
        let crate::data::EventKind::Notes { voices, .. } = &package.score().first_events[0].kind
        else {
            panic!("missing recovery notes")
        };
        for &note in voices {
            let mut voice = Voice::new(package.resources(), package.tables(), note).unwrap();
            voice
                .commands(&mut Controls::default(), &mut { INSTRUCTION_BUDGET })
                .unwrap();
            assert_eq!(voice.volume, 0);
            let duration = crate::volume::frames_from_millis(400).unwrap();
            for frame in 0..=duration {
                crate::music_voice::test_frame(&mut voice, Controls::default()).unwrap();
                if frame % BLOCK_FRAMES as u64 == BLOCK_FRAMES as u64 - 1 {
                    voice.mix_block(&mut [[[0; 2]; 3]; BLOCK_FRAMES]);
                }
                if frame == duration / 2 {
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
    pub dls: dls::Tables,
    pub modulation: modulation::Tables,
    pub coefficients: resample::Coefficients,
}

impl Tables {
    pub fn validate(&self) -> Result<()> {
        self.mix.validate()?;
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

struct PanRamp(crate::volume::Ramp);

impl PanRamp {
    fn new(initial: u8, delta: i8, milliseconds: u16) -> Self {
        let initial = i32::from(initial) << 16;
        Self(crate::volume::Ramp::new(
            initial,
            initial + (i32::from(delta) << 16),
            crate::volume::frames_from_millis(u64::from(milliseconds)).unwrap(),
        ))
    }
    fn value(&self) -> i32 {
        self.0.value()
    }
    fn advance(&mut self) {
        self.0.advance(1);
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
    fn next_gain(&mut self) -> u16 {
        match self {
            Self::Ordinary(e) => e.next_gain(),
            Self::Dls(e) => e.next_gain(),
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
    elapsed: u64,
    duration: u64,
    frequency: i64,
}

// New voices gradually lose age-based allocation preference over 7.5 seconds.
const INITIAL_PRIORITY_AGE: u16 = 60_000;
const DEFAULT_PRIORITY_AGE_MS: u64 = 7_500;

impl Sweep {
    fn new(step_hz: i16, period: u8) -> Option<Self> {
        // Authored sweep periods count 16-millisecond frequency steps.
        let duration = crate::volume::frames_from_millis(u64::from(period) * 16).unwrap();
        (period != 0).then_some(Self {
            elapsed: 0,
            duration,
            frequency: i64::from(step_hz) * i64::from(period),
        })
    }

    /// Signed 16.16 source increment, before addition to the note's pitch.
    fn value(&self) -> i64 {
        (i128::from(self.frequency) * 65536 * i128::from(self.elapsed)
            / (i128::from(crate::SOURCE_RATE) * i128::from(self.duration))) as i64
    }

    fn advance(&mut self, frames: u64) {
        self.elapsed =
            ((u128::from(self.elapsed) + u128::from(frames)) % u128::from(self.duration)) as u64;
    }
}

pub(crate) struct Voice<'a> {
    resources: &'a Resources,
    tables: &'a Tables,
    pub macro_id: u16,
    pc: usize,
    loop_remaining: u16,
    frame: u64,
    /// Absolute source-frame timestamps retain deadlines independently of control scheduling.
    started_at: u64,
    macro_started_at: Option<u64>,
    wait_reference: u64,
    bpm_1024: u32,
    random: crate::sequence::shared::Control,
    wait: Wait,
    key_off: bool,
    key_off_trap: Option<(u16, usize)>,
    message_trap: Option<(u16, usize)>,
    messages: std::collections::VecDeque<i32>,
    done: bool,
    original_key: u8,
    key: u8,
    cents: i8,
    velocity: u8,
    volume: u32,
    volume_control: Option<u16>,
    selectors: [Option<(Operand, i32)>; ControlTarget::COUNT],
    volume_ramp: Option<crate::volume::Ramp>,
    auxiliary_override: [Option<u8>; 2],
    pitch_sweeps: [Option<Sweep>; 2],
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
    vibrato: modulation::Vibrato,
    lfo: modulation::Oscillator,
    tremolo: modulation::Tremolo,
    tremolo_input: TremoloInput,
    validated_controls: Option<Controls>,
    effective_controls: Controls,
    priority: u8,
    pub(crate) exclusive_group: u8,
    pub(crate) host_request: Option<HostRequest>,
    variables: [i32; 16],
    pub(crate) original_macro: u16,
    pub(crate) handle: u32,
    pub(crate) last_child: u32,
    priority_age: crate::volume::Ramp,
    /// Source frames to fade an authored age to zero; zero holds the age.
    age_period_frames: u64,
    tremolo_scale: f32,
    pending: [Option<crate::sequence::BusFrame>; BLOCK_FRAMES],
    last_mix: crate::sequence::BusFrame,
    playing: bool,
    release: crate::release::Release,
}

/// Unit tests of voice arithmetic can step without constructing a score.
#[cfg(test)]
pub(crate) fn test_frame(voice: &mut Voice<'_>, mut controls: Controls) -> Result<()> {
    voice.prepare_commands(&mut controls, &mut { INSTRUCTION_BUDGET })?;
    voice.prepare_frame(controls, 0)
}

impl<'a> Voice<'a> {
    #[cfg(test)]
    pub(crate) fn new(resources: &'a Resources, tables: &'a Tables, note: Note) -> Result<Self> {
        Self::new_at(resources, tables, note, 0, Default::default())
    }

    /// Start a note on the sequencer's shared control and sample clock.
    pub(crate) fn new_at(
        resources: &'a Resources,
        tables: &'a Tables,
        note: Note,
        start_frame: u64,
        random: crate::sequence::shared::Control,
    ) -> Result<Self> {
        ensure!(
            start_frame.is_multiple_of(CONTROL_FRAMES as u64),
            "note starts between control boundaries"
        );
        ensure!(
            note.key < 128 && note.velocity < 128 && note.pan < 128,
            "invalid music note"
        );
        let parameters = Parameters::Ordinary(envelope::Parameters::default());
        let age_frames = crate::volume::frames_from_millis(DEFAULT_PRIORITY_AGE_MS)?;
        Ok(Self {
            resources,
            tables,
            macro_id: note.macro_id,
            pc: 0,
            loop_remaining: 0,
            frame: 0,
            started_at: start_frame,
            wait_reference: start_frame,
            macro_started_at: Some(start_frame),
            bpm_1024: 120 * 1024,
            random,
            wait: Wait::default(),
            key_off: false,
            key_off_trap: None,
            message_trap: None,
            messages: Default::default(),
            done: false,
            original_key: note.key,
            key: note.key,
            cents: 0,
            velocity: note.velocity,
            volume: u32::from(note.velocity) << 16,
            volume_control: None,
            selectors: [None; ControlTarget::COUNT],
            volume_ramp: None,
            auxiliary_override: [None; 2],
            pitch_sweeps: [None, None],
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
            vibrato: Default::default(),
            lfo: Default::default(),
            tremolo: Default::default(),
            tremolo_input: TremoloInput::default(),
            validated_controls: None,
            effective_controls: Controls::default(),
            priority: note.priority,
            exclusive_group: 0,
            host_request: None,
            variables: [0; 16],
            original_macro: note.macro_id,
            handle: u32::MAX,
            last_child: u32::MAX,
            priority_age: crate::volume::Ramp::new(i32::from(INITIAL_PRIORITY_AGE), 0, age_frames),
            age_period_frames: age_frames,
            tremolo_scale: 1.,
            pending: [None; BLOCK_FRAMES],
            last_mix: [[0; 2]; 3],
            playing: false,
            release: Default::default(),
        })
    }

    /// The sequencer delivers note-off at a control boundary.
    pub fn key_off(&mut self) -> Result<()> {
        ensure!(
            self.frame.is_multiple_of(CONTROL_FRAMES as u64),
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
        self.done && !self.source_active()
    }

    pub(crate) fn send_message(&mut self, value: i32) -> Result<()> {
        ensure!(
            self.messages.len() < crate::sequence::MESSAGE_BUDGET,
            "audio pending-message budget exceeded"
        );
        self.messages.push_back(value);
        if let Some((program, instruction)) = self.message_trap.take() {
            self.macro_id = program;
            self.pc = instruction;
            self.wake();
        }
        Ok(())
    }

    /// Beat waits sample the current tempo when issued; an existing deadline is retained.
    pub fn set_tempo(&mut self, bpm_1024: u32) {
        self.bpm_1024 = bpm_1024;
    }

    pub(crate) fn commands_ready(&self) -> bool {
        !self.done && self.ready()
    }

    fn wake(&mut self) {
        self.wait = Wait::default();
        self.wait_reference = self.now();
    }

    fn wake_on_sample_end(&mut self) {
        if !self.done && self.wait.sample_end && self.sample_finished {
            self.wake();
        }
    }

    /// A finite sample may finish after its program. A looping sample instead
    /// releases its authored envelope when the program ends.
    fn finish_program(&mut self) {
        self.done = true;
        if self
            .source
            .as_ref()
            .is_some_and(|(cursor, _)| cursor.sample().loop_length > 0)
        {
            self.release_envelopes();
        }
    }

    fn release_envelopes(&mut self) {
        self.envelope.release();
        if let Some((envelope, _)) = &mut self.pitch_envelope {
            envelope.release();
        }
    }

    pub(crate) fn kill(&mut self) {
        self.break_source();
        self.done = true;
    }

    fn break_source(&mut self) {
        self.source = None;
        self.sample_finished = true;
    }

    pub(crate) fn output_pending(&self) -> bool {
        self.playing || self.release.active() || self.pending.iter().any(Option::is_some)
    }

    pub(crate) fn child(&self, note: Note, instruction: u16, frame: u64) -> Result<Self> {
        let mut child = Self::new_at(
            self.resources,
            self.tables,
            note,
            frame,
            self.random.clone(),
        )?;
        // Note velocity selects envelope timing; inherited gain may be louder.
        child.volume = self.volume;
        child.pc = usize::from(instruction);
        child.interaural_delay = self.interaural_delay;
        child.macro_started_at = None;
        Ok(child)
    }

    pub(crate) fn prepare_commands(
        &mut self,
        controls: &mut Controls,
        fuel: &mut usize,
    ) -> Result<()> {
        if self.frame.is_multiple_of(CONTROL_FRAMES as u64) {
            if self.macro_started_at.is_none() {
                let now = self.now();
                self.macro_started_at = Some(now);
                self.wait_reference = now;
            }
            self.commands(controls, fuel)?;
        }
        Ok(())
    }

    pub(crate) fn allocation_priority(&self) -> (u8, u32) {
        (self.priority, self.priority_age.value() as u32)
    }

    fn set_age(&mut self, age: i32) {
        let age = age.clamp(0, i32::from(u16::MAX));
        let target = if self.age_period_frames == 0 { age } else { 0 };
        self.priority_age = crate::volume::Ramp::new(age, target, self.age_period_frames);
    }

    fn set_age_period(&mut self, milliseconds: u32) -> Result<()> {
        let age = self.priority_age.value();
        self.age_period_frames = crate::volume::frames_from_millis(u64::from(milliseconds))?;
        self.set_age(age);
        Ok(())
    }

    pub(crate) fn source_active(&self) -> bool {
        self.source.is_some() && !self.sample_finished
    }

    fn sample_ended(&self) -> bool {
        self.sample_finished
    }

    fn ready(&self) -> bool {
        self.now() >= self.wait.until
            || (self.wait.key_off && self.key_off)
            || (self.wait.sample_end && self.sample_ended())
    }

    fn now(&self) -> u64 {
        self.started_at + self.frame
    }

    /// Queue fully gained PCM. The external pan offset uses fourteen-bit controller units
    /// and is applied after authored selectors. Later controls cannot change queued samples.
    pub(crate) fn prepare_frame(&mut self, controls: Controls, pan_offset: i16) -> Result<()> {
        if self.validated_controls != Some(controls) {
            controls.validate()?;
            self.validated_controls = Some(controls);
        }
        if self.done && !self.source_active() {
            return Ok(());
        }
        let phase = (self.now() % BLOCK_FRAMES as u64) as usize;
        let pitch_envelope = self.pitch_envelope.as_mut().map_or(0, |(envelope, depth)| {
            (i32::from(*depth) * i32::from(envelope.next_gain())) >> 7
        });
        if self.frame.is_multiple_of(CONTROL_FRAMES as u64) {
            let sweep: i64 = self.pitch_sweeps.iter().flatten().map(Sweep::value).sum();
            if let Some((cursor, source)) = &mut self.source {
                let pitch = (i32::from(self.key) << 16)
                    + (i32::from(self.cents) << 16) / 100
                    + (i32::from(controls.pitch_bend) - 8192) * 2 * 8
                    + pitch_envelope
                    + self.vibrato.pitch_offset(controls.paired[1]);
                let ratio = pitch::ratio(pitch, cursor.sample())?;
                source.set_ratio((i64::from(ratio) + sweep).clamp(0, i64::from(u32::MAX)) as u32);
            }
            self.effective_controls = controls;
            self.tremolo_scale = self.tremolo.gain(
                match self.tremolo_input {
                    TremoloInput::Midpoint => 8192,
                    TremoloInput::Zero => 0,
                    TremoloInput::Lfo => (i32::from(self.lfo.value) * 2 + 8192) as u16,
                },
                controls.paired[1],
                &self.tables.modulation,
            )?;
        }
        let controls = self.effective_controls;
        if let Some(ramp) = &mut self.volume_ramp {
            self.volume = ramp.value() as u32;
            if ramp.finished() {
                self.volume_ramp = None;
            }
        }
        let controller = self
            .volume_control
            .unwrap_or_else(|| control::volume(controls.paired[7], controls.paired[11]));
        let controller = self.control(ControlTarget::Volume, controller, &controls)?;
        let pan_control = self.control(ControlTarget::Pan, controls.paired[10], &controls)?;
        let pan = if controls.mono {
            u32::from(mix::CENTER_PAN) << 16
        } else {
            (self.pan[0].value()
                + ((i32::from(pan_control) + i32::from(pan_offset)
                    - (i32::from(mix::CENTER_PAN) << 7))
                    << 9))
                .clamp(0, 127 << 16) as u32
        };
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
            pre: [
                self.control(ControlTarget::PreAuxiliaryA, 0, &controls)?,
                self.control(ControlTarget::PreAuxiliaryB, 0, &controls)?,
            ],
            post: [
                self.control(
                    ControlTarget::PostAuxiliaryA,
                    u16::from(self.auxiliary_override[0].unwrap_or(controls.post[0])) << 7,
                    &controls,
                )?,
                self.control(
                    ControlTarget::PostAuxiliaryB,
                    u16::from(self.auxiliary_override[1].unwrap_or(controls.post[1])) << 7,
                    &controls,
                )?,
            ],
            scale: self.tremolo_scale,
            group_volume: controls.group_volume,
            aux_a: 128,
            alternate: self.alternate_volume,
            interaural_delay: self.delay.is_some(),
        });
        if let Some((cursor, source)) = &mut self.source {
            let enveloped = if self.envelope.is_done() {
                0
            } else {
                let sample = source.next_sample(cursor);
                let envelope = self.envelope.next_gain();
                ((i64::from(sample) * i64::from(envelope)) >> 15) as i16
            };
            let samples = self
                .delay
                .as_mut()
                .map_or([enveloped; 2], |delay| delay.next(enveloped));
            self.pending[phase] = Some(targets.map(|bus| {
                std::array::from_fn(|channel| {
                    i32::from(mix::apply(samples[channel], 32768, bus[channel]))
                })
            }));
            let input_finished =
                (cursor.is_done() && !source.has_pending()) || self.envelope.is_done();
            if input_finished && self.delay.as_ref().is_none_or(|delay| !delay.has_pending()) {
                self.break_source();
            }
        }
        self.wake_on_sample_end();
        if let Some(ramp) = &mut self.volume_ramp {
            ramp.advance(1);
        }
        for pan in &mut self.pan {
            pan.advance();
        }
        for sweep in self.pitch_sweeps.iter_mut().flatten() {
            sweep.advance(1);
        }
        self.priority_age.advance(1);
        self.lfo.advance(1, &self.tables.modulation);
        self.vibrato.oscillator.advance(1, &self.tables.modulation);
        self.frame += 1;
        Ok(())
    }

    /// Mix queued samples, then fade a stopped source completely to silence.
    pub(crate) fn mix_block(&mut self, output: &mut [[[i32; 2]; 3]; BLOCK_FRAMES]) {
        for (frame, samples) in output.iter_mut().zip(&mut self.pending) {
            if let Some(samples) = samples.take() {
                self.last_mix = samples;
                for (bus, samples) in frame.iter_mut().zip(samples) {
                    for (output, sample) in bus.iter_mut().zip(samples) {
                        *output += sample;
                    }
                }
                self.playing = true;
            } else if self.playing {
                self.release = crate::release::Release::new(self.last_mix, 0);
                self.playing = false;
            }
            self.release.mix(frame);
        }
    }
}
