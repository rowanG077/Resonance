//! Standard reverb from audio_filter_80150F20_whole.c (80150F20 / 80151530).
//! Uses the original fused filter operations and two-block auxiliary latency.
use anyhow::{Result, ensure};

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
        let output = (-coefficient).mul_add(value, self.last);
        self.advance(value);
        output
    }
}

struct Channel {
    comb: [Delay; 2],
    all_pass: [Delay; 2],
    low_pass: f32,
    pre_delay: Vec<f32>,
    pre_at: usize,
}

impl Channel {
    fn new(pre_delay: usize) -> Self {
        Self {
            comb: [Delay::new(1789), Delay::new(1999)],
            all_pass: [Delay::new(433), Delay::new(149)],
            low_pass: 0.0,
            pre_delay: vec![0.0; pre_delay],
            pre_at: 0,
        }
    }
}

pub struct StandardReverb {
    channels: [Channel; 2],
    coefficients: [f32; 2],
    coloration: f32,
    damping: f32,
    wet: f32,
    dry: f32,
    returned: Vec<[i32; 2]>,
    returned_at: usize,
}

/// One shared dry/auxiliary studio. Sources submit unclipped buses so their
/// effect tails survive source replacement and overlap before final clipping.
pub struct Studio {
    effects: [StandardReverb; 2],
}
impl Studio {
    pub fn new(parameters: [[f32; 5]; 2]) -> Result<Self> {
        Ok(Self {
            effects: [
                StandardReverb::new(parameters[0])?,
                StandardReverb::new(parameters[1])?,
            ],
        })
    }
    pub fn process(&mut self, buses: [[i32; 2]; 3]) -> [i32; 2] {
        let mut output = buses[0];
        for (effect, bus) in self.effects.iter_mut().zip(&buses[1..]) {
            for (channel, sample) in output.iter_mut().zip(effect.process(*bus)) {
                *channel += sample;
            }
        }
        output
    }
}

impl StandardReverb {
    /// Parameters: coloration, mix, decay seconds, damping, pre-delay seconds.
    pub fn new(parameters: [f32; 5]) -> Result<Self> {
        for (value, (min, max)) in parameters.into_iter().zip([
            (0.0, 1.0),
            (0.0, 1.0),
            (0.01, 10.0),
            (0.0, 1.0),
            (0.0, 0.1),
        ]) {
            ensure!(
                value.is_finite() && (min..=max).contains(&value),
                "invalid standard-reverb parameter"
            );
        }
        let [coloration, mix, time, damping, pre_delay] = parameters;
        let pre_delay = (32000.0 * pre_delay) as usize;
        // Original pointer wraps when the incremented pointer reaches count-1.
        ensure!(
            pre_delay == 0 || pre_delay >= 2,
            "nonzero reverb pre-delay must span at least two samples"
        );
        let wet = mix * 0.6;
        Ok(Self {
            channels: std::array::from_fn(|_| Channel::new(pre_delay)),
            coefficients: [1789, 1999].map(|delay| {
                10.0f64.powf(f64::from((delay * -3) as f32 / (32000.0 * time))) as f32
            }),
            coloration,
            damping: 1.0 - (0.05 + 0.8 * damping.max(0.05)),
            wet,
            dry: 0.6 - wet,
            // The original triple-buffered auxiliary bus returns two 160-frame
            // blocks later (salBuildCommandList's (salAuxFrame + 1) % 3).
            returned: vec![[0; 2]; 320],
            returned_at: 0,
        })
    }

    pub fn process(&mut self, input: [i32; 2]) -> [i32; 2] {
        let mut next = [0; 2];
        for (index, channel) in self.channels.iter_mut().enumerate() {
            let sample = input[index] as f32;
            let mut delayed = sample;
            if !channel.pre_delay.is_empty() {
                delayed = channel.pre_delay[channel.pre_at];
                channel.pre_delay[channel.pre_at] = sample;
                channel.pre_at += 1;
                if channel.pre_at == channel.pre_delay.len() - 1 {
                    channel.pre_at = 0;
                }
            }
            let comb = channel.comb[0].comb(delayed, self.coefficients[0])
                + channel.comb[1].comb(delayed, self.coefficients[1]);
            let low_pass = channel.all_pass[0].all_pass(comb, self.coloration) * 0.3;
            channel.low_pass = self.damping.mul_add(channel.low_pass, low_pass);
            let filtered = channel.all_pass[1].all_pass(channel.low_pass, self.coloration);
            next[index] = self.wet.mul_add(filtered, self.dry * sample) as i32;
        }
        let output = self.returned[self.returned_at];
        self.returned[self.returned_at] = next;
        self.returned_at = (self.returned_at + 1) % self.returned.len();
        output
    }
}

/// Mix stereo voice buses and retain the audible effect tail. Two decay periods
/// cover a 120 dB decay from the bounded PCM16 inputs; keep pre-delay and bus
/// latency as well, then remove only trailing frames that are exactly zero.
pub fn mix_studio(buses: &[Vec<i16>; 3], parameters: [[f32; 5]; 2]) -> Result<Vec<i16>> {
    let length = buses[0].len();
    ensure!(
        length.is_multiple_of(2) && buses.iter().all(|bus| bus.len() == length),
        "studio buses must have matching stereo lengths"
    );
    let mut studio = Studio::new(parameters)?;
    let tail_seconds = parameters
        .into_iter()
        .enumerate()
        .filter(|(bus, _)| buses[bus + 1].iter().any(|sample| *sample != 0))
        .map(|(_, p)| 2.0 * p[2] + p[4])
        .fold(0.0f32, f32::max);
    let frames = length / 2 + (tail_seconds * 32000.0).ceil() as usize + 320;
    let mut output = Vec::with_capacity(frames * 2);
    for frame in 0..frames {
        let input = buses.each_ref().map(|bus| {
            std::array::from_fn(|channel| {
                i32::from(bus.get(frame * 2 + channel).copied().unwrap_or(0))
            })
        });
        output.extend(
            studio
                .process(input)
                .map(|sample| sample.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16),
        );
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
    fn studio_retains_headroom_and_auxiliary_tail_after_sources_end() {
        let mut studio = Studio::new([[1., 0.5, 1., 0.8, 0.01]; 2]).unwrap();
        // Dry sources can exceed PCM16; a later source/voice may cancel them.
        assert_eq!(
            studio.process([[50000, -50000], [10000, -10000], [0; 2]]),
            [50000, -50000]
        );
        for _ in 1..320 {
            assert_eq!(studio.process([[0; 2]; 3]), [0; 2]);
        }
        // The source is gone, but its shared auxiliary return still arrives.
        assert_eq!(studio.process([[0; 2]; 3]), [3000, -3000]);
        let mut tail = 0;
        for _ in 0..32000 {
            tail += usize::from(studio.process([[0; 2]; 3]) != [0; 2]);
        }
        assert!(tail > 100, "effect tail disappeared with its source");
    }

    #[test]
    fn returns_dry_auxiliary_at_two_blocks_and_keeps_channels_separate() {
        let mut reverb = StandardReverb::new([1.0, 0.5, 1.0, 0.8, 0.01]).unwrap();
        assert_eq!(reverb.process([10000, -10000]), [0, 0]);
        for _ in 1..320 {
            assert_eq!(reverb.process([0; 2]), [0; 2]);
        }
        assert_eq!(reverb.process([0; 2]), [3000, -3000]);
        for _ in 0..100 {
            assert_eq!(reverb.process([0; 2]), [0; 2]);
        }
        assert!(StandardReverb::new([0.0, 0.0, f32::NAN, 0.0, 0.0]).is_err());
    }
}
