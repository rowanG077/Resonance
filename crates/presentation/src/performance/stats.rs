use serde::Serialize;
use std::collections::VecDeque;

pub(super) const WINDOW_SECONDS: f64 = 30.;
const MAX_SAMPLES: usize = 16_384;

#[derive(Clone, Copy, Debug, Serialize)]
pub(super) struct Sample {
    pub frame: u64,
    pub elapsed_seconds: f64,
    pub frame_ms: f64,
    pub app_ms: f64,
}

#[derive(Debug, Serialize)]
pub(super) struct Summary {
    pub samples: usize,
    pub window_seconds: f64,
    pub fps: f64,
    pub mean_frame_ms: f64,
    pub mean_app_ms: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub p99_9_ms: f64,
    pub low_1_percent_fps: f64,
    pub low_0_1_percent_fps: f64,
    pub max_frame_ms: f64,
    pub frames_over_50_ms: usize,
    pub tail_warmed_up: bool,
}

#[derive(Default)]
pub(super) struct History(pub VecDeque<Sample>);

impl History {
    pub fn push(&mut self, sample: Sample) {
        if !sample.frame_ms.is_finite() || sample.frame_ms <= 0. {
            return;
        }
        while self.0.len() >= MAX_SAMPLES
            || self.0.front().is_some_and(|first| {
                sample.elapsed_seconds - first.elapsed_seconds > WINDOW_SECONDS
            })
        {
            self.0.pop_front();
        }
        self.0.push_back(sample);
    }

    pub fn summary(&self) -> Option<Summary> {
        if self.0.is_empty() {
            return None;
        }
        let mut times: Vec<_> = self.0.iter().map(|s| s.frame_ms).collect();
        times.sort_unstable_by(f64::total_cmp);
        // Nearest-rank quantiles. Low FPS is the reciprocal of the frame-time
        // percentile, not the average FPS of the slowest subset of frames.
        let percentile = |p: f64| times[(p * times.len() as f64).ceil() as usize - 1];
        let mean = times.iter().sum::<f64>() / times.len() as f64;
        Some(Summary {
            samples: times.len(),
            window_seconds: times.iter().sum::<f64>() / 1000.,
            fps: 1000. / mean,
            mean_frame_ms: mean,
            mean_app_ms: self.0.iter().map(|s| s.app_ms).sum::<f64>() / times.len() as f64,
            p50_ms: percentile(0.5),
            p95_ms: percentile(0.95),
            p99_ms: percentile(0.99),
            p99_9_ms: percentile(0.999),
            low_1_percent_fps: 1000. / percentile(0.99),
            low_0_1_percent_fps: 1000. / percentile(0.999),
            max_frame_ms: *times.last().unwrap(),
            frames_over_50_ms: times.iter().filter(|&&ms| ms > 50.).count(),
            tail_warmed_up: times.len() >= 1000,
        })
    }

    pub fn recent_fps(&self) -> Option<f64> {
        let end = self.0.back()?.elapsed_seconds;
        let (count, total) = self
            .0
            .iter()
            .rev()
            .take_while(|s| end - s.elapsed_seconds <= 1.)
            .fold((0, 0.), |(n, total), s| (n + 1, total + s.frame_ms));
        Some(1000. * f64::from(count) / total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_quantiles_expose_stalls_without_averaging_fps() {
        let mut history = History::default();
        let mut elapsed = 0.;
        for frame in 0..1000 {
            let frame_ms = match frame {
                0 => 100.,
                1 => 50.,
                2..=10 => 25.,
                _ => 10.,
            };
            elapsed += frame_ms / 1000.;
            history.push(Sample {
                frame,
                elapsed_seconds: elapsed,
                frame_ms,
                app_ms: 2.,
            });
        }
        let stats = history.summary().unwrap();
        assert_eq!(stats.p50_ms, 10.);
        assert_eq!(stats.p99_ms, 25.);
        assert_eq!(stats.p99_9_ms, 50.);
        assert_eq!(stats.low_0_1_percent_fps, 20.);
        assert_eq!(stats.max_frame_ms, 100.);
        assert_eq!(stats.frames_over_50_ms, 1);
        assert!(stats.tail_warmed_up);
        assert!((stats.fps - 1000. / 10.265).abs() < 1e-10);
    }

    #[test]
    fn rolling_window_expires_old_stalls_and_bounds_memory() {
        let mut history = History::default();
        assert!(history.summary().is_none());
        for frame in 0..20_000 {
            history.push(Sample {
                frame,
                elapsed_seconds: frame as f64 / 1000.,
                frame_ms: 1.,
                app_ms: 0.2,
            });
        }
        assert_eq!(history.0.len(), MAX_SAMPLES);
        history.push(Sample {
            frame: 20_000,
            elapsed_seconds: 60.,
            frame_ms: 16.,
            app_ms: 2.,
        });
        assert_eq!(history.0.len(), 1);
        assert_eq!(history.summary().unwrap().max_frame_ms, 16.);
    }
}
