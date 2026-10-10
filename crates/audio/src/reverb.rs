//! Stereo reverb with authored predelay and retained filter tails.
use anyhow::{Result, ensure};
use std::collections::VecDeque;

// Leave time for any queued input, including a delay from a previous preset.
const MAX_PREDELAY_SECONDS: f32 = 0.1;
const TAIL_FADE_FRAMES: u32 = crate::SOURCE_RATE / 10;
const COMB_DELAYS: [usize; 2] = [1789, 1999];

/// Parameters: coloration, mix, decay seconds, damping, pre-delay seconds.
pub(crate) fn validate_parameters(parameters: [f32; 5]) -> Result<()> {
    for (value, (min, max)) in parameters.into_iter().zip([
        (0.0, 1.0),
        (0.0, 1.0),
        (0.01, 10.0),
        (0.0, 1.0),
        (0.0, MAX_PREDELAY_SECONDS),
    ]) {
        ensure!(
            value.is_finite() && (min..=max).contains(&value),
            "invalid standard-reverb parameter"
        );
    }
    Ok(())
}

struct Delay {
    data: Vec<f32>,
    write: usize,
    read: usize,
    last: f32,
}

impl Delay {
    fn new(delay: usize) -> Self {
        Self {
            data: vec![0.0; delay + 2],
            write: 0,
            read: 2,
            last: 0.0,
        }
    }
    fn advance(&mut self, input: f32) {
        self.data[self.write] = input;
        self.last = self.data[self.read];
        self.write = (self.write + 1) % self.data.len();
        self.read = (self.read + 1) % self.data.len();
    }
    fn comb(&mut self, input: f32, coefficient: f32) -> f32 {
        self.advance(coefficient.mul_add(self.last, input));
        self.last
    }
    fn all_pass(&mut self, input: f32, coefficient: f32) -> f32 {
        let value = coefficient.mul_add(self.last, input);
        // At unity feedback this filter is exactly a sign inversion. Avoid
        // cancellation between large retained values when rendering tiny input.
        let output = if coefficient == 1. {
            -input
        } else {
            (-coefficient).mul_add(value, self.last)
        };
        self.advance(value);
        output
    }
}

struct Channel {
    comb: [Delay; 2],
    all_pass: [Delay; 2],
    low_pass: f32,
    pending: VecDeque<f32>,
}

impl Channel {
    fn new() -> Self {
        Self {
            comb: COMB_DELAYS.map(Delay::new),
            all_pass: [Delay::new(433), Delay::new(149)],
            low_pass: 0.0,
            pending: VecDeque::new(),
        }
    }

    fn clear(&mut self) {
        for delay in self.comb.iter_mut().chain(&mut self.all_pass) {
            delay.data.fill(0.);
            delay.last = 0.;
        }
        self.low_pass = 0.;
        self.pending.clear();
    }
}

pub struct StandardReverb {
    channels: [Channel; 2],
    pre_delay: usize,
    coefficients: [f32; 2],
    coloration: f32,
    damping: f32,
    wet: f32,
    dry: f32,
    tail_frames: u32,
    remaining: u32,
}

/// One shared dry/auxiliary studio. Sources submit unclipped buses so their
/// effect tails survive source replacement and overlap before final clipping.
pub struct Studio {
    effects: [StandardReverb; 2],
    music_reverb: [f32; 5],
    sound_parameters: [f32; 5],
}
impl Studio {
    pub fn new(parameters: [[f32; 5]; 2]) -> Result<Self> {
        Ok(Self {
            effects: [
                StandardReverb::new(parameters[0])?,
                StandardReverb::new(parameters[1])?,
            ],
            music_reverb: parameters[0],
            sound_parameters: parameters[1],
        })
    }
    pub fn music_reverb(&self) -> [f32; 5] {
        self.music_reverb
    }

    /// No effect has a remaining audible tail. Dry input is never retained.
    pub fn is_silent(&self) -> bool {
        self.effects.iter().all(|effect| effect.remaining == 0)
    }

    /// Apply a preset without interrupting the existing effect tail.
    pub fn set_music_reverb(&mut self, parameters: [f32; 5]) -> Result<()> {
        if self.music_reverb != parameters {
            validate_parameters(parameters)?;
            self.effects[0].configure(parameters);
            self.music_reverb = parameters;
        }
        Ok(())
    }

    /// Field ambience selects the sound bus preset independently of music.
    pub fn set_sound_preset(&mut self, preset: u8) -> Result<()> {
        let parameters = match preset {
            2 => [0.9, 0.6, 3.6, 0.6, 0.06],
            3 => [0.3, 0.6, 4.0, 0.4, 0.08],
            _ => [1.0, 0.5, 1.0, 0.8, 0.01],
        };
        if self.sound_parameters != parameters {
            self.effects[1] = StandardReverb::new(parameters)?;
            self.sound_parameters = parameters;
        }
        Ok(())
    }

    pub fn process(&mut self, buses: [[i32; 2]; 3]) -> [i32; 2] {
        let mut output = buses[0].map(i64::from);
        for (effect, bus) in self.effects.iter_mut().zip(&buses[1..]) {
            for (channel, sample) in output.iter_mut().zip(effect.process(*bus)) {
                *channel += i64::from(sample);
            }
        }
        output.map(|sample| sample.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32)
    }
}

impl StandardReverb {
    /// Parameters: coloration, mix, decay seconds, damping, pre-delay seconds.
    pub fn new(parameters: [f32; 5]) -> Result<Self> {
        validate_parameters(parameters)?;
        let mut effect = Self {
            channels: std::array::from_fn(|_| Channel::new()),
            pre_delay: 0,
            coefficients: [0.; 2],
            coloration: 0.,
            damping: 0.,
            wet: 0.,
            dry: 0.,
            tail_frames: 0,
            remaining: 0,
        };
        effect.configure(parameters);
        Ok(effect)
    }

    fn configure(&mut self, parameters: [f32; 5]) {
        let [coloration, mix, time, damping, pre_delay] = parameters;
        // Queued input keeps its arrival frame. Only new input uses this delay.
        self.pre_delay = (crate::SOURCE_RATE as f32 * pre_delay) as usize;
        self.coefficients = COMB_DELAYS
            .map(|delay| 10.0f32.powf(-3. * delay as f32 / (crate::SOURCE_RATE as f32 * time)));
        self.coloration = coloration;
        self.damping = 1.0 - (0.05 + 0.8 * damping.max(0.05));
        self.wet = mix * 0.6;
        self.dry = 0.6 - self.wet;
        // New input gets two decay periods, queued-input allowance and a final fade.
        // An existing tail keeps its deadline when the preset changes.
        self.tail_frames = ((2. * time + MAX_PREDELAY_SECONDS) * crate::SOURCE_RATE as f32).ceil()
            as u32
            + TAIL_FADE_FRAMES;
        if self.wet == 0. {
            self.clear();
        }
    }

    fn clear(&mut self) {
        for channel in &mut self.channels {
            channel.clear();
        }
        self.remaining = 0;
    }

    pub fn process(&mut self, input: [i32; 2]) -> [i32; 2] {
        if self.wet == 0. {
            return input.map(|sample| (self.dry * sample as f32) as i32);
        }
        if input != [0; 2] {
            self.remaining = self.tail_frames;
        } else if self.remaining == 0 {
            return [0; 2];
        } else {
            self.remaining -= 1;
        }
        let fade = (self.remaining as f32 / TAIL_FADE_FRAMES as f32).min(1.);
        let wet = self.wet * fade * fade;
        let mut next = [0; 2];
        for (index, channel) in self.channels.iter_mut().enumerate() {
            let sample = input[index] as f32;
            let mut delayed = channel.pending.pop_front().unwrap_or(0.);
            if self.pre_delay == 0 {
                delayed += sample;
            } else {
                if channel.pending.len() < self.pre_delay {
                    channel.pending.resize(self.pre_delay, 0.);
                }
                channel.pending[self.pre_delay - 1] += sample;
            }
            let comb = channel.comb[0].comb(delayed, self.coefficients[0])
                + channel.comb[1].comb(delayed, self.coefficients[1]);
            let low_pass = channel.all_pass[0].all_pass(comb, self.coloration) * 0.3;
            channel.low_pass = self.damping.mul_add(channel.low_pass, low_pass);
            let filtered = channel.all_pass[1].all_pass(channel.low_pass, self.coloration);
            next[index] = wet.mul_add(filtered, self.dry * sample) as i32;
        }
        if self.remaining == 0 {
            self.clear();
        }
        next
    }
}

/// Mix stereo voice buses and finish the same bounded, faded tail as live playback.
/// Trim trailing silence without shortening the supplied input.
pub fn mix_studio<S: Copy + Into<i32>>(
    buses: &[Vec<S>; 3],
    parameters: [[f32; 5]; 2],
) -> Result<Vec<i16>> {
    let length = buses[0].len();
    ensure!(
        length.is_multiple_of(2) && buses.iter().all(|bus| bus.len() == length),
        "studio buses must have matching stereo lengths"
    );
    let mut studio = Studio::new(parameters)?;
    let mut output = Vec::with_capacity(length);
    let append = |output: &mut Vec<i16>, frame: [i32; 2]| {
        output.extend(
            frame.map(|sample| sample.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16),
        );
    };
    for frame in 0..length / 2 {
        let input = buses
            .each_ref()
            .map(|bus| std::array::from_fn(|channel| bus[frame * 2 + channel].into()));
        append(&mut output, studio.process(input));
    }
    while !studio.is_silent() {
        append(&mut output, studio.process([[0; 2]; 3]));
    }
    let audible = output
        .chunks_exact(2)
        .rposition(|frame| frame != [0, 0])
        .map_or(1, |at| at + 1);
    output.truncate((audible * 2).max(length));
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simultaneous_buses_preserve_cancellation_and_clamp_after_summing() {
        let full = [i32::MAX, i32::MIN];
        for second_aux in [full, [i32::MIN, i32::MAX]] {
            let mut studio = Studio::new([[0., 0., 1., 0., 0.]; 2]).unwrap();
            // Exercise both saturating sums and cancellation after an
            // intermediate sum exceeds i32, in both stereo polarities.
            assert_eq!(studio.process([full, full, second_aux]), full);
        }
    }

    #[test]
    fn music_reverb_changes_preserve_filter_tails_and_the_other_effect() {
        let initial = [0.8, 0.7, 3.6, 0.6, 0.08];
        let changed = [0.7, 0.7, 2.5, 0.6, 0.05];
        let parameters = [initial, [1., 0.5, 1., 0.8, 0.01]];
        let mut studio = Studio::new(parameters).unwrap();
        let mut reference = Studio::new(parameters).unwrap();
        for i in 0..6007 {
            let buses = [[0; 2], [10000 + i % 97, -2000], [-3000, 5000 + i % 53]];
            assert_eq!(studio.process(buses), reference.process(buses));
        }
        assert!(
            studio
                .set_music_reverb([f32::NAN, 0.7, 2.5, 0.6, 0.05])
                .is_err()
        );
        assert_eq!(studio.music_reverb(), initial);
        studio.set_music_reverb(initial).unwrap();
        // Invalid and unchanged settings must leave the audible tail alone.
        let mut audible = false;
        // Drain the pre-delay too, so the later check exercises filter history.
        for _ in 0..4000 {
            let expected = reference.process([[0; 2]; 3]);
            audible |= expected != [0; 2];
            assert_eq!(studio.process([[0; 2]; 3]), expected);
        }
        assert!(audible);
        studio.set_music_reverb(changed).unwrap();
        assert_eq!(studio.music_reverb(), changed);
        let mut music_tail = false;
        for _ in 0..6400 {
            music_tail |= studio.effects[0].process([0; 2]) != [0; 2];
            assert_eq!(
                studio.effects[1].process([0; 2]),
                reference.effects[1].process([0; 2])
            );
        }
        assert!(music_tail, "preset change erased the filter tail");
    }

    #[test]
    fn predelay_changes_preserve_queued_audio_and_apply_to_new_input() {
        let parameters = |frames: usize| {
            [
                0.7,
                1.,
                0.05,
                0.6,
                (frames as f32 + 0.25) / crate::SOURCE_RATE as f32,
            ]
        };
        let changes = [(1, 4), (2, 16), (3, 0), (4, 8)];
        let mut changed = StandardReverb::new(parameters(8)).unwrap();
        let mut reference = StandardReverb::new(parameters(0)).unwrap();
        let mut scheduled = [[0; 2]; 32];
        let mut delay = 8;
        for frame in 0..scheduled.len() {
            if let Some(&(_, frames)) = changes.iter().find(|(at, _)| *at == frame) {
                delay = frames;
                changed.configure(parameters(delay));
            }
            let input = if frame < 5 {
                [100000 + frame as i32 * 10000, -100000]
            } else {
                [0; 2]
            };
            if input != [0; 2] {
                for (queued, sample) in scheduled[frame + delay].iter_mut().zip(input) {
                    *queued += sample;
                }
            }
            assert_eq!(changed.process(input), reference.process(scheduled[frame]));
        }
        let mut audible = false;
        for _ in 0..crate::SOURCE_RATE / 10 {
            let output = changed.process([0; 2]);
            assert_eq!(output, reference.process([0; 2]));
            audible |= output != [0; 2];
        }
        assert!(audible, "queued input never reached the wet output");
    }

    #[test]
    fn offline_tail_includes_delayed_input_and_decays() {
        let parameters = [[0.7, 1., 0.4, 0.6, 0.1]; 2];
        let buses = [vec![0i16; 2], vec![i16::MAX; 2], vec![0; 2]];
        let output = mix_studio(&buses, parameters).unwrap();
        let delay = (0.1 * crate::SOURCE_RATE as f32) as usize;
        assert!(output[..delay * 2].iter().all(|&sample| sample == 0));
        let first = output.iter().position(|&sample| sample != 0).unwrap();
        let peak = |samples: &[i16]| {
            samples
                .iter()
                .map(|sample| sample.unsigned_abs())
                .max()
                .unwrap()
        };
        assert!(peak(&output[first..first + 1000]) > peak(&output[output.len() - 1000..]));
        assert!(output.len() <= 2 * (crate::SOURCE_RATE as usize + 1));
    }

    #[test]
    fn wide_bus_tail_fades_to_silence_within_its_duration() {
        let mut studio = Studio::new([[0.99, 1., 0.01, 0.6, 0.1]; 2]).unwrap();
        studio.process([[0; 2], [i32::MAX, i32::MIN], [0; 2]]);
        let mut tail = Vec::new();
        // 20 ms of decay, 100 ms for queued input and a 100 ms fade.
        let limit = (0.22 * crate::SOURCE_RATE as f32).ceil() as usize + 1;
        while !studio.is_silent() {
            assert!(tail.len() < limit);
            tail.push(studio.process([[0; 2]; 3]));
        }
        let peak = tail
            .iter()
            .flatten()
            .map(|sample| sample.unsigned_abs())
            .max()
            .unwrap();
        assert!(peak > 0, "delayed wet input must be heard");
        assert!(
            tail[tail.len() - 32..]
                .iter()
                .flatten()
                .all(|sample| sample.unsigned_abs() < peak / 20)
        );
        assert_eq!(tail.last(), Some(&[0; 2]));
        for _ in 0..crate::SOURCE_RATE / 10 {
            assert_eq!(studio.process([[0; 2]; 3]), [0; 2]);
        }
    }

    #[test]
    fn auxiliary_dry_return_is_immediate_and_keeps_channels_separate() {
        let mut reverb = StandardReverb::new([1.0, 0.5, 1.0, 0.8, 0.01]).unwrap();
        assert_eq!(reverb.process([10000, -10000]), [3000, -3000]);
        for _ in 0..100 {
            assert_eq!(reverb.process([0; 2]), [0; 2]);
        }
        assert!(StandardReverb::new([0.0, 0.0, f32::NAN, 0.0, 0.0]).is_err());
    }
}
