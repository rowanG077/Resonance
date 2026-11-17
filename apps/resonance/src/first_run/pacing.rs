//! Measured per-pass costs, interpolated in inverse CPU count. Beyond the
//! measurements, a nonnegative serial + parallel/cores fit limits extrapolation.
use resonance_importer::ImportProgress;
use serde::Deserialize;
use std::{collections::BTreeMap, time::Instant};

#[derive(Deserialize)]
struct Profile {
    stages: Vec<Stage>,
}
#[derive(Deserialize)]
struct Stage {
    seconds: BTreeMap<usize, f64>,
}

fn cost(samples: &BTreeMap<usize, f64>, cores: usize) -> f64 {
    let cores = cores.max(1);
    if let Some(&seconds) = samples.get(&cores) {
        return seconds.max(0.001);
    }
    let lower = samples.range(..cores).next_back();
    let upper = samples.range(cores..).next();
    if let (Some((&a, &ta)), Some((&b, &tb))) = (lower, upper) {
        let t = (1. / cores as f64 - 1. / a as f64) / (1. / b as f64 - 1. / a as f64);
        return (ta + (tb - ta) * t).max(0.001);
    }
    if samples.len() < 2 {
        return samples.values().next().copied().unwrap_or(1.).max(0.001);
    }
    let n = samples.len() as f64;
    let x = samples.keys().map(|&c| 1. / c as f64).sum::<f64>() / n;
    let y = samples.values().sum::<f64>() / n;
    let variance = samples
        .keys()
        .map(|&c| (1. / c as f64 - x).powi(2))
        .sum::<f64>();
    let parallel = (samples
        .iter()
        .map(|(&c, &t)| (1. / c as f64 - x) * (t - y))
        .sum::<f64>()
        / variance)
        .max(0.);
    let serial = (y - parallel * x).max(0.);
    let predicted = serial + parallel / cores as f64;
    // Extrapolation cannot promise more than ideal linear speedup from the
    // closest measured point (nor invent a slowdown on a larger CPU budget).
    let (&nearest, &seconds) = lower.or(upper).unwrap();
    predicted
        .clamp(
            seconds.min(seconds * nearest as f64 / cores as f64),
            seconds.max(seconds * nearest as f64 / cores as f64),
        )
        .max(0.001)
}

pub struct Pacing {
    costs: Vec<f64>,
    stage: usize,
    started: Instant,
    observed: f64,
    predicted: f64,
    base: f64,
    running: bool,
    pub shown: f64,
}

impl Pacing {
    pub fn new(cores: usize) -> Self {
        let profile: Profile =
            serde_json::from_str(include_str!("../../assets/setup/import-profile.json"))
                .expect("bundled import profile");
        Self {
            costs: profile
                .stages
                .iter()
                .map(|s| cost(&s.seconds, cores))
                .collect(),
            stage: 0,
            started: Instant::now(),
            observed: 0.,
            predicted: 0.,
            base: 0.,
            running: false,
            shown: 0.,
        }
    }

    pub fn event(&mut self, event: &ImportProgress) {
        match *event {
            ImportProgress::Extracting { disc, files, total } => {
                self.base = if disc == 1 {
                    0.02 * files as f64 / total.max(1) as f64
                } else {
                    0.02 + 0.01 * files as f64 / total.max(1) as f64
                };
            }
            ImportProgress::Cooking { stage, total } => {
                if self.costs.len() != total {
                    self.costs = vec![1.; total];
                }
                if self.running {
                    self.observed += self.started.elapsed().as_secs_f64();
                    self.predicted += self.costs.get(self.stage).copied().unwrap_or(0.);
                }
                self.stage = stage
                    .saturating_sub(1)
                    .min(self.costs.len().saturating_sub(1));
                self.base =
                    0.03 + 0.95 * self.costs[..self.stage].iter().sum::<f64>() / self.total();
                self.started = Instant::now();
                self.running = true;
            }
            ImportProgress::Publishing => {
                self.base = 0.99;
                self.running = false;
            }
            // Only a successful child exit authorizes 100%, not a message.
            ImportProgress::Finished => {}
        }
    }

    fn total(&self) -> f64 {
        self.costs.iter().sum::<f64>().max(0.001)
    }

    pub fn update(&mut self, dt: f64, succeeded: bool) -> f64 {
        let mut target = self.base;
        if self.running
            && let Some(&cost) = self.costs.get(self.stage)
        {
            // Learn this machine's speed from completed passes, avoiding noisy
            // startup micro-stages. Never assume every CPU has the reference speed.
            let speed = if self.predicted > 2. {
                (self.observed / self.predicted).clamp(0.1, 10.)
            } else {
                1.
            };
            let fraction = (self.started.elapsed().as_secs_f64() / (cost * speed)).min(0.95);
            target += 0.95 * cost / self.total() * fraction;
        }
        target = if succeeded { 1. } else { target.min(0.99) };
        self.shown += (target - self.shown)
            .max(0.)
            .min(dt * if succeeded { 0.2 } else { 0.025 });
        self.shown
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn serial_and_parallel_stages_scale_independently_and_progress_waits_for_exit() {
        let serial = BTreeMap::from([(1, 20.), (2, 20.), (4, 20.), (8, 20.)]);
        let parallel = BTreeMap::from([(1, 88.), (2, 48.), (4, 28.), (8, 18.)]);
        assert!((cost(&serial, 16) - 20.).abs() < 0.001);
        assert!((cost(&parallel, 16) - 13.).abs() < 0.001);
        assert!((cost(&parallel, 3) - (8. + 80. / 3.)).abs() < 0.001);
        let mut pacing = Pacing::new(4);
        pacing.event(&ImportProgress::Publishing);
        pacing.event(&ImportProgress::Finished);
        assert_eq!(pacing.update(1000., false), 0.99);
        pacing.event(&ImportProgress::Extracting {
            disc: 1,
            files: 0,
            total: 1,
        });
        assert_eq!(pacing.update(1., false), 0.99);
        assert_eq!(pacing.update(1., true), 1.);
    }
}
