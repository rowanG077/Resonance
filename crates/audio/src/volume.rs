//! Linear fades on the native source PCM clock.
use anyhow::{Context, Result, ensure};

pub fn frames_from_millis(milliseconds: u64) -> Result<u64> {
    Ok(milliseconds
        .checked_mul(u64::from(crate::SOURCE_RATE))
        .context("duration exceeds the audio clock")?
        .div_ceil(1000))
}

/// Integer control interpolation with an exact endpoint and no accumulated rounding.
pub struct Ramp {
    initial: i32,
    target: i32,
    elapsed: u64,
    duration: u64,
}

impl Ramp {
    pub fn new(initial: i32, target: i32, duration_frames: u64) -> Self {
        Self {
            initial,
            target,
            elapsed: 0,
            duration: duration_frames,
        }
    }

    pub fn value(&self) -> i32 {
        if self.finished() {
            return self.target;
        }
        (i128::from(self.initial)
            + (i128::from(self.target) - i128::from(self.initial)) * i128::from(self.elapsed)
                / i128::from(self.duration)) as i32
    }

    pub fn advance(&mut self, frames: u64) {
        self.elapsed = self.elapsed.saturating_add(frames).min(self.duration);
    }

    pub fn finished(&self) -> bool {
        self.elapsed >= self.duration
    }
}

#[derive(Clone, Copy)]
pub struct Fade {
    initial: f32,
    target: f32,
    elapsed: u64,
    duration: u64,
}

impl Fade {
    pub fn new(initial: f32, target: f32, duration_ms: u64) -> Result<Self> {
        Self::from_frames(initial, target, frames_from_millis(duration_ms)?)
    }

    pub fn from_frames(initial: f32, target: f32, duration: u64) -> Result<Self> {
        ensure!(
            [initial, target]
                .into_iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(&v)),
            "invalid group volume"
        );
        Ok(Self {
            initial,
            target,
            elapsed: 0,
            duration,
        })
    }

    pub fn value(&self) -> f32 {
        self.value_at(0)
    }

    /// Preview an offset from the current position without advancing playback.
    pub fn value_at(&self, frames: u64) -> f32 {
        let elapsed = self.elapsed.saturating_add(frames);
        if elapsed >= self.duration {
            return self.target;
        }
        let progress = elapsed as f64 / self.duration as f64;
        (f64::from(self.initial) + (f64::from(self.target) - f64::from(self.initial)) * progress)
            as f32
    }

    pub fn advance(&mut self, frames: u64) {
        self.elapsed = self.elapsed.saturating_add(frames).min(self.duration);
    }
}

/// Native title sessions and diagnostic previews fade their final mixed output.
pub const TITLE_STARTUP_MS: u64 = 100;

#[derive(Clone, Copy)]
pub struct ControlFade {
    value: f32,
    previous: f32,
    target: f32,
    progress: f32,
    step: f32,
}

impl ControlFade {
    /// Convert seven-bit volume using a single-precision reciprocal.
    pub fn to_control(initial: f32, amount: u8, duration_ms: u16) -> Result<Self> {
        ensure!(amount <= 127, "invalid group volume control");
        Self::new(initial, f32::from(amount) * (1.0 / 127.0), duration_ms)
    }

    pub fn new(initial: f32, target: f32, duration_ms: u16) -> Result<Self> {
        ensure!(
            [initial, target]
                .into_iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(&v)),
            "invalid group volume"
        );
        Ok(Self {
            value: if duration_ms == 0 { target } else { initial },
            previous: initial,
            target,
            progress: if duration_ms == 0 { 0.0 } else { 1.0 },
            step: if duration_ms == 0 {
                0.0
            } else {
                1280.0 / (f32::from(duration_ms) * 256.0)
            },
        })
    }

    pub fn value(&self) -> f32 {
        self.value
    }

    pub fn advance_block(&mut self) {
        if self.progress > 0.0 {
            // Keep the multiply and subtraction separate for single-precision rounding.
            let delta = self.target - self.previous;
            let weighted = self.progress * delta;
            self.value = self.target - weighted;
            self.progress -= self.step;
            if self.progress <= 0.0 {
                self.value = self.target;
            }
        }
    }
}

/// Two group fades sampled before each millisecond's voice controls.
pub struct Startup {
    master: ControlFade,
    sequence: ControlFade,
}

impl Startup {
    pub fn new(master_ms: u16, sequence_ms: u16, lead_ms: u16) -> anyhow::Result<Self> {
        anyhow::ensure!(
            lead_ms.is_multiple_of(5),
            "master fade lead must align to a five-ms block"
        );
        let mut master = ControlFade::new(0.0, 1.0, master_ms)?;
        let sequence = ControlFade::new(0.0, 1.0, sequence_ms)?;
        for _ in 0..lead_ms / 5 {
            master.advance_block();
        }
        Ok(Self { master, sequence })
    }

    pub fn value_at(&mut self, frame: u64) -> f32 {
        let value = self.sequence.value() * self.master.value();
        if frame.is_multiple_of(160) {
            self.master.advance_block();
            self.sequence.advance_block();
        }
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fades_are_monotonic_and_finish_at_the_requested_duration() {
        for milliseconds in [1, 17, TITLE_STARTUP_MS, 2000, 120_000] {
            for (initial, target) in [(0., 1.), (1., 0.)] {
                let mut fade = Fade::new(initial, target, milliseconds).unwrap();
                let duration = frames_from_millis(milliseconds).unwrap();
                assert_eq!(fade.value(), initial);
                assert!((fade.value_at(duration / 2) - 0.5).abs() <= 1.0 / duration as f32);
                let mut previous = initial;
                for _ in 0..duration.div_ceil(32) {
                    fade.advance(32);
                    let value = fade.value();
                    assert!((value - previous) * (target - initial) >= 0.);
                    previous = value;
                }
                assert_eq!(fade.value(), target);
                fade.advance(u64::MAX);
                assert_eq!(fade.value(), target);
            }
        }
        assert_eq!(Fade::new(1., 0.25, 0).unwrap().value(), 0.25);
        assert!(Fade::new(f32::NAN, 1., 20).is_err());
    }

    #[test]
    fn interrupted_fade_starts_from_the_current_volume() {
        let mut fade = Fade::new(0., 1., 2000).unwrap();
        fade.advance(frames_from_millis(1500).unwrap());
        assert_eq!(fade.value(), 0.75);
        let mut replacement = Fade::new(fade.value(), 0., 250).unwrap();
        assert_eq!(replacement.value(), fade.value());
        replacement.advance(frames_from_millis(250).unwrap() / 2);
        assert!((replacement.value() - 0.375).abs() < 0.0001);
        replacement.advance(frames_from_millis(250).unwrap().div_ceil(2));
        assert_eq!(replacement.value(), 0.);
    }

    #[test]
    fn integer_fades_reach_small_and_large_targets_at_the_declared_frame() {
        for (initial, target, ms) in [
            (127 << 16, 0, 2000),
            (33026, 65536, 60000),
            (65536, 33026, 60000),
        ] {
            let duration = frames_from_millis(ms).unwrap();
            let mut ramp = Ramp::new(initial, target, duration);
            let mut previous = initial;
            for _ in 0..16 {
                let value = ramp.value();
                assert!(
                    (i64::from(value) - i64::from(previous)) * i64::from(target - initial) >= 0
                );
                previous = value;
                ramp.advance(duration / 17);
            }
            ramp.advance(duration - duration / 17 * 16 - 1);
            assert!(!ramp.finished());
            ramp.advance(1);
            assert!(ramp.finished());
            assert_eq!(ramp.value(), target);
            ramp.advance(u64::MAX);
            assert_eq!(ramp.value(), target);
        }
        assert_eq!(Ramp::new(0, 127, 0).value(), 127);
    }
}

#[cfg(test)]
mod startup_tests {
    use super::ControlFade as Fade;

    #[test]
    fn five_millisecond_fade_keeps_first_step_and_exact_endpoint() {
        let mut fade = Fade::new(0.0, 1.0, 100).unwrap();
        assert_eq!(fade.value(), 0.0);
        fade.advance_block();
        assert_eq!(fade.value(), 0.0);
        fade.advance_block();
        assert_eq!(fade.value(), 1.0 - 0.95_f32);
        for _ in 2..20 {
            fade.advance_block();
        }
        assert_eq!(fade.value(), 1.0);
        fade.advance_block();
        assert_eq!(fade.value(), 1.0);
        assert_eq!(Fade::new(1.0, 0.25, 0).unwrap().value(), 0.25);
        assert!(Fade::new(f32::NAN, 1.0, 20).is_err());
    }

    #[test]
    fn classroom_music_fade_reaches_the_observed_volume_and_endpoint() {
        // GQSEAF lesson-three checkpoint, VI 17986, volume groups 23..30.
        // The target amount was 72, then the script requested zero over 2 s.
        let initial = Fade::to_control(1., 72, 0).unwrap().value();
        assert_eq!(initial, 0.566_929_1);
        let mut fade = Fade::to_control(initial, 0, 2000).unwrap();
        for _ in 0..241 {
            fade.advance_block();
        }
        assert_eq!(fade.value(), 0.226_771_97);
        // Single-precision accumulation needs the 401st update to cross
        // zero for this duration.
        for _ in 241..401 {
            fade.advance_block();
        }
        assert_eq!(fade.value(), 0.);
        assert!(Fade::to_control(1., 128, 10).is_err());
    }
}
