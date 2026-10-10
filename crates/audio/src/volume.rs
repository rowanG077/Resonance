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
