//! UV motion with an explicit scene or effect clock.
use super::RenderValue;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldTextureAnimation {
    pub actor: RenderValue,
    pub texture: RenderValue,
    pub clock: TextureClock,
    pub motion: TextureMotion,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control: Option<TextureControl>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextureClock {
    Field,
    Effect,
    /// Effect time independent of the field's animation switch.
    UnconditionalEffect,
    /// A local counter advanced on selected effect ticks.
    EffectPulse {
        interval: u32,
        phase: u32,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        unconditional: bool,
    },
    /// Advance only while the target actor exists.
    Actor,
    /// An actor's animation independent of the field's animation switch.
    UnconditionalActor,
    /// Each existing actor advances a shared counter, in this order.
    SharedActors {
        actors: Vec<RenderValue>,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TextureControl {
    ResetUnless { setting: u8 },
    FinishWhen { setting: u8, tick: u32 },
    ClearWhenDisabled,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TextureMotion {
    /// Explicit frames support pauses, held endings and irregular atlas layouts.
    #[serde(rename = "timeline")]
    Sampled {
        #[serde(flatten)]
        animation: crate::TextureAnimation<()>,
    },
    Scroll {
        velocity: [f32; 2],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        wave: Option<FieldTextureWave>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        restart_ticks: Option<u32>,
    },
    Atlas {
        frames: u32,
        interval: u32,
        step: [f32; 2],
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldTextureWave {
    pub degrees_per_tick: f32,
    pub amplitude: f32,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub horizontal: bool,
}
impl FieldTextureAnimation {
    pub fn validate(&self) -> Result<()> {
        let shared = match &self.clock {
            TextureClock::SharedActors { actors } => {
                ensure!(!actors.is_empty(), "empty texture clock actor group");
                actors.as_slice()
            }
            _ => &[],
        };
        if let TextureClock::EffectPulse {
            interval, phase, ..
        } = self.clock
        {
            ensure!(
                interval > 0 && phase < interval,
                "invalid texture clock pulse"
            );
        }
        for target in [&self.actor, &self.texture].into_iter().chain(shared) {
            ensure!(
                !matches!(target, RenderValue::Setting(slot) | RenderValue::SettingOffset { slot, .. } if *slot > 7),
                "invalid texture target slot"
            );
        }
        if let Some(control) = &self.control {
            if let TextureControl::ResetUnless { setting }
            | TextureControl::FinishWhen { setting, .. } = control
            {
                ensure!(*setting <= 7, "invalid texture control slot");
            }
            ensure!(
                !matches!(
                    self.clock,
                    TextureClock::Effect | TextureClock::UnconditionalEffect
                ),
                "effect clock cannot be reset by a texture control"
            );
        }
        match &self.motion {
            TextureMotion::Sampled { animation } => animation.validate()?,
            TextureMotion::Scroll {
                velocity,
                wave,
                restart_ticks,
            } => {
                ensure!(
                    *restart_ticks != Some(0),
                    "invalid texture restart interval"
                );
                ensure!(
                    velocity.iter().all(|v| v.is_finite()),
                    "invalid texture velocity"
                );
                if let Some(wave) = wave {
                    ensure!(
                        wave.degrees_per_tick.is_finite() && wave.amplitude.is_finite(),
                        "invalid texture wave"
                    );
                }
            }
            TextureMotion::Atlas {
                frames,
                interval,
                step,
            } => ensure!(
                *frames > 0 && *interval > 0 && step.iter().all(|v| v.is_finite()),
                "invalid texture atlas"
            ),
        }
        Ok(())
    }
    pub fn offset(&self, field_tick: u64, effect_tick: u32) -> [f32; 2] {
        let tick = match &self.clock {
            TextureClock::Effect | TextureClock::UnconditionalEffect => u64::from(effect_tick),
            _ => field_tick,
        };
        match &self.motion {
            TextureMotion::Sampled { animation } => animation.offset(tick),
            TextureMotion::Scroll {
                velocity,
                wave,
                restart_ticks,
            } => {
                let tick = restart_ticks.map_or(tick, |period| {
                    if tick == 0 {
                        0
                    } else {
                        1 + (tick - 1) % u64::from(period)
                    }
                });
                let mut offset = velocity.map(|v| v * tick as f32);
                if let Some(wave) = wave {
                    offset[usize::from(!wave.horizontal)] +=
                        (tick as f32 * wave.degrees_per_tick).to_radians().sin() * wave.amplitude;
                }
                offset
            }
            TextureMotion::Atlas {
                frames,
                interval,
                step,
            } => step.map(|v| v * ((tick / u64::from(*interval)) % u64::from(*frames)) as f32),
        }
    }
}

/// Small simulation state shared by normal rendering and capture rendering.
#[derive(Debug, Clone, Default)]
pub struct TextureAnimationState {
    pub offset: [f32; 2],
    tick: u64,
}

impl TextureAnimationState {
    pub fn reset(&mut self, animation: &FieldTextureAnimation, effect_tick: u32) {
        if matches!(
            animation.clock,
            TextureClock::Effect | TextureClock::UnconditionalEffect
        ) {
            return;
        }
        self.tick = 0;
        self.offset = animation.offset(0, effect_tick);
    }

    pub fn advance(
        &mut self,
        animation: &FieldTextureAnimation,
        enabled: bool,
        field_tick: u64,
        effect_tick: u32,
        settings: &BTreeMap<i32, i32>,
        exists: impl Fn(i32) -> bool,
    ) {
        let actor = animation.actor.resolve(settings);
        let unconditional = matches!(
            animation.clock,
            TextureClock::UnconditionalActor
                | TextureClock::UnconditionalEffect
                | TextureClock::EffectPulse {
                    unconditional: true,
                    ..
                }
        );
        if !enabled && !unconditional {
            if matches!(animation.control, Some(TextureControl::ClearWhenDisabled)) {
                self.offset = [0.; 2];
            }
            return;
        }
        if let TextureClock::EffectPulse {
            interval, phase, ..
        } = animation.clock
            && effect_tick % interval != phase
        {
            return;
        }
        if matches!(
            animation.clock,
            TextureClock::Actor | TextureClock::UnconditionalActor
        ) && !exists(actor)
        {
            return;
        }
        if let Some(TextureControl::ResetUnless { setting }) = &animation.control
            && settings.get(&i32::from(*setting)).copied().unwrap_or(0) == 0
        {
            self.tick = 0;
            self.offset = animation.offset(0, effect_tick);
            return;
        }
        if let Some(TextureControl::FinishWhen { setting, tick }) = &animation.control
            && settings.get(&i32::from(*setting)).copied().unwrap_or(0) != 0
        {
            self.tick = self.tick.max(u64::from(*tick));
        }
        let mut later = 0;
        match &animation.clock {
            TextureClock::Field if animation.control.is_none() => self.tick = field_tick,
            TextureClock::SharedActors { actors } => {
                let mut passed_target = false;
                for target in actors {
                    let target = target.resolve(settings);
                    if exists(target) {
                        self.tick += 1;
                        later += u64::from(passed_target);
                    }
                    passed_target |= target == actor;
                }
            }
            _ => self.tick += 1,
        }
        self.offset = animation.offset(self.tick.saturating_sub(later), effect_tick);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scroll(actor: i32, clock: TextureClock) -> FieldTextureAnimation {
        FieldTextureAnimation {
            actor: RenderValue::Fixed(actor),
            texture: RenderValue::Fixed(0),
            clock,
            motion: TextureMotion::Scroll {
                velocity: [0.25, 0.],
                wave: None,
                restart_ticks: None,
            },
            control: None,
        }
    }

    #[test]
    fn shared_actor_clock_tracks_order_and_missing_actors() {
        let tracks = [1, 2].map(|actor| {
            scroll(
                actor,
                TextureClock::SharedActors {
                    actors: vec![RenderValue::Fixed(1), RenderValue::Fixed(2)],
                },
            )
        });
        let mut states = [
            TextureAnimationState::default(),
            TextureAnimationState::default(),
        ];
        for (present, expected) in [
            ([true, true], [0.25, 0.5]),
            ([true, true], [0.75, 1.]),
            ([true, false], [1.25, 1.25]),
            ([false, true], [1.25, 1.5]),
            ([false, false], [1.5, 1.5]),
        ] {
            for (track, state) in tracks.iter().zip(&mut states) {
                state.advance(track, true, 0, 0, &BTreeMap::new(), |actor| {
                    present[actor as usize - 1]
                });
            }
            assert_eq!(states.each_ref().map(|state| state.offset[0]), expected);
        }
    }

    #[test]
    fn finish_control_holds_the_final_frame_until_reset() {
        let mut track = scroll(1, TextureClock::Field);
        track.control = Some(TextureControl::FinishWhen {
            setting: 0,
            tick: 3,
        });
        track.motion = TextureMotion::Sampled {
            animation: crate::TextureAnimation {
                texture: (),
                delay_ticks: 0,
                loop_start: 3,
                offsets: vec![[0., 0.], [0., 0.25], [0., 0.5], [0., 0.75]],
            },
        };
        let mut state = TextureAnimationState::default();
        for (setting, expected) in [(0, 0.25), (1, 0.75), (0, 0.75)] {
            state.advance(&track, true, 0, 0, &[(0, setting)].into(), |_| true);
            assert_eq!(state.offset[1], expected);
        }
        state.reset(&track, 0);
        assert_eq!(state.offset, [0., 0.]);
        track.control = Some(TextureControl::ClearWhenDisabled);
        state.advance(&track, true, 0, 0, &BTreeMap::new(), |_| true);
        assert_eq!(state.offset, [0., 0.25]);
        state.advance(&track, false, 0, 0, &BTreeMap::new(), |_| true);
        assert_eq!(state.offset, [0., 0.]);
        state.advance(&track, true, 0, 0, &BTreeMap::new(), |_| true);
        assert_eq!(state.offset, [0., 0.5]);
    }

    #[test]
    fn unconditional_effect_wave_uses_effect_time_while_disabled() {
        let mut track = scroll(1, TextureClock::UnconditionalEffect);
        track.motion = TextureMotion::Scroll {
            velocity: [0.; 2],
            wave: Some(FieldTextureWave {
                degrees_per_tick: 90.,
                amplitude: 0.25,
                horizontal: true,
            }),
            restart_ticks: None,
        };
        let mut state = TextureAnimationState::default();
        state.advance(&track, false, 0, 1, &BTreeMap::new(), |_| false);
        assert_eq!(state.offset, [0.25, 0.]);
        state.reset(&track, 0);
        assert_eq!(state.offset, [0.25, 0.]);
    }

    #[test]
    fn effect_pulses_reset_the_local_counter_without_changing_global_phase() {
        let track = scroll(
            1,
            TextureClock::EffectPulse {
                interval: 2,
                phase: 1,
                unconditional: true,
            },
        );
        let mut state = TextureAnimationState::default();
        for tick in 1..=4 {
            state.advance(&track, false, 0, tick, &BTreeMap::new(), |_| false);
        }
        assert_eq!(state.offset, [0.5, 0.]);
        state.reset(&track, 4);
        state.advance(&track, false, 0, 6, &BTreeMap::new(), |_| false);
        assert_eq!(state.offset, [0., 0.]);
        state.advance(&track, false, 0, 7, &BTreeMap::new(), |_| false);
        assert_eq!(state.offset, [0.25, 0.]);
    }
}
