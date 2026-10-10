//! Authored animation playback. Slots are resource-table byte offsets.

pub mod slot {
    pub const IDLE: u16 = 12;
    pub const TALK: u16 = 24;
    pub const WALK: u16 = 36;
    pub const RUN: u16 = 40;
    pub const TURN_RIGHT: u16 = 44;
    pub const TURN_LEFT: u16 = 48;
    pub const TALK_FALLBACK: u16 = 52;
    pub const STAGGER: u16 = 76;
    pub const EVENT_TALK: u16 = 112;
    pub const EVENT_IDLE: u16 = 116;
    pub const EVENT_WALK: u16 = 120;
    pub const EVENT_RUN: u16 = 124;
}
#[derive(Debug, Clone)]
pub struct Animation {
    pub resource: u32,
    pub source: AnimationSource,
    /// Position and rate use nominal 60-Hz cooked animation ticks.
    pub start_frame: f32,
    pub rate: f32,
    /// A script pause retains speed independently of ambient/event ownership.
    pub paused_rate: Option<f32>,
    pub loop_start: f32,
    pub blend_ticks: u32,
    pub duration_ticks: u32,
    pub slot: u16,
    pub start_tick: u32,
    /// Last seek or rate change, measured on the same paused playback clock.
    pub phase_tick: u32,
    pub repeat: bool,
    pub paused_at: Option<u32>,
    pub paused_ticks: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum AnimationSource {
    #[default]
    Model,
    Resource,
}
impl Animation {
    pub fn new(resource: u32, slot: u16, duration_ticks: u32, tick: u32) -> Self {
        Self {
            resource,
            source: AnimationSource::Model,
            slot,
            duration_ticks,
            start_tick: tick,
            phase_tick: tick,
            start_frame: 0.,
            rate: 1.,
            paused_rate: None,
            loop_start: 0.,
            blend_ticks: 0,
            repeat: true,
            paused_at: None,
            paused_ticks: 0,
        }
    }

    pub fn matches(&self, clip: &resonance_content::SceneClip, owner: u32) -> bool {
        clip.resource_slot == self.slot
            && match (self.source, clip.animation_resource) {
                (AnimationSource::Model, None) => owner == self.resource,
                (AnimationSource::Resource, Some(resource)) => resource == self.resource,
                _ => false,
            }
    }

    pub fn elapsed(&self, tick: u32, presentation_delay: u32) -> f32 {
        let tick = self.animation_tick(tick);
        let clip_age = tick
            .saturating_sub(self.start_tick)
            .saturating_sub(self.blend_ticks);
        let phase_age = tick.saturating_sub(self.phase_tick);
        self.start_frame
            + clip_age.min(phase_age).saturating_sub(presentation_delay) as f32 * self.rate
    }
    /// Blend from the current pose, then begin advancing the new clip.
    pub fn blend_weight(&self, tick: u32) -> f32 {
        if self.blend_ticks == 0 {
            1.
        } else {
            (self.animation_tick(tick).saturating_sub(self.start_tick) as f32
                / self.blend_ticks as f32)
                .min(1.)
        }
    }
    pub fn sample(&self, tick: u32, presentation_delay: u32, duration: f32) -> f32 {
        self.sample_elapsed(self.elapsed(tick, presentation_delay), duration)
    }
    fn sample_elapsed(&self, elapsed: f32, duration: f32) -> f32 {
        if self.repeat && duration > 0. && elapsed < 0. {
            elapsed.rem_euclid(duration)
        } else if self.repeat && duration > self.loop_start && elapsed > duration {
            let phase = (elapsed - self.loop_start) % (duration - self.loop_start);
            if phase == 0. {
                duration
            } else {
                self.loop_start + phase
            }
        } else {
            elapsed.clamp(0., duration)
        }
    }
    pub fn seek(&mut self, sample: f32, tick: u32) {
        self.start_frame = sample;
        self.phase_tick = self.animation_tick(tick);
    }
    pub fn script_pause(&mut self, paused: bool, tick: u32) {
        let position = self.sample(tick, 0, self.duration_ticks as f32);
        self.seek(position, tick);
        if paused {
            self.paused_rate.get_or_insert(self.rate);
            self.rate = 0.;
        } else if let Some(rate) = self.paused_rate.take() {
            self.rate = rate;
        }
    }
    pub fn script_rate(&self) -> f32 {
        self.paused_rate.unwrap_or(self.rate)
    }
    pub fn set_script_rate(&mut self, rate: f32, tick: u32) {
        self.seek(self.sample(tick, 0, self.duration_ticks as f32), tick);
        if let Some(paused) = &mut self.paused_rate {
            *paused = rate;
        } else {
            self.rate = rate;
        }
    }
    pub(crate) fn set_paused(&mut self, paused: bool, tick: u32) {
        let previous = tick.saturating_sub(1);
        if paused {
            self.paused_at.get_or_insert(previous);
        } else if let Some(start) = self.paused_at.take() {
            self.paused_ticks += previous.saturating_sub(start);
        }
    }
    fn animation_tick(&self, tick: u32) -> u32 {
        self.paused_at
            .unwrap_or(tick)
            .saturating_sub(self.paused_ticks)
    }
}

/// Inputs already sampled by the actor visit, before heading integration.
pub(crate) struct OrdinaryAnimation {
    pub movement_speed: Option<f32>,
    pub turn: f32,
    pub walking: bool,
    pub event_controlled: bool,
    pub player_locomotion: bool,
    pub dialogue: bool,
}
impl crate::Actor {
    pub(crate) fn select_ordinary_animation(
        &mut self,
        model: &crate::ModelResource,
        tick: u32,
        selection: OrdinaryAnimation,
        blend_ticks: Option<u32>,
    ) -> bool {
        let OrdinaryAnimation {
            movement_speed,
            turn,
            walking,
            event_controlled,
            player_locomotion,
            dialogue,
        } = selection;
        let requested = if let Some(speed) = movement_speed {
            let running = !walking && speed > 7.;
            let event_gait = if running {
                slot::EVENT_RUN
            } else {
                slot::EVENT_WALK
            };
            if event_controlled && model.clips.contains_key(&event_gait) {
                event_gait
            } else if running && model.clips.contains_key(&slot::RUN) {
                slot::RUN
            } else {
                slot::WALK
            }
        } else if turn != 0. && model.clips.contains_key(&slot::TURN_RIGHT) {
            if turn < 0. {
                slot::TURN_LEFT
            } else {
                slot::TURN_RIGHT
            }
        } else if dialogue {
            // Use the event conversation pose when the script owns the player.
            [
                if event_controlled {
                    slot::EVENT_TALK
                } else {
                    slot::TALK
                },
                slot::TALK_FALLBACK,
                slot::IDLE,
            ]
            .into_iter()
            .find(|slot| model.clips.contains_key(slot))
            .unwrap_or(self.idle_animation)
        } else if event_controlled && model.clips.contains_key(&slot::EVENT_IDLE) {
            slot::EVENT_IDLE
        } else {
            self.idle_animation
        };
        let slot = if model.clips.contains_key(&requested) {
            requested
        } else {
            slot::IDLE
        };
        let mut bound = false;
        if model.clips.contains_key(&slot)
            && self.animation.as_ref().is_none_or(|a| {
                a.slot != slot
                    || a.resource != self.resource
                    || a.source != AnimationSource::Model
                    || blend_ticks.is_some()
            })
        {
            bound = true;
            self.animation = Some(Animation {
                blend_ticks: blend_ticks.unwrap_or(
                    if matches!(slot, slot::TURN_RIGHT | slot::TURN_LEFT)
                        || player_locomotion && matches!(slot, slot::WALK | slot::RUN)
                    {
                        2
                    } else {
                        8
                    },
                ),
                repeat: !matches!(slot, slot::TURN_RIGHT | slot::TURN_LEFT),
                ..Animation::new(self.resource, slot, model.clips[&slot].duration_ticks, tick)
            });
        }
        if player_locomotion
            && let Some(motion) = &self.motion
            && let Some(animation) = &mut self.animation
            && matches!(slot, slot::WALK | slot::RUN)
        {
            // Scale player gait with movement speed. Script-directed movement
            // keeps its independent authored rate and blend duration.
            let rate = motion.speed / if slot == slot::WALK { 2. } else { 10. };
            if animation.rate != rate {
                animation.seek(
                    animation.sample(tick, 0, animation.duration_ticks as f32),
                    tick,
                );
                animation.rate = rate;
            }
        }
        bound
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blend_seek_rate_and_pause_share_one_clock() {
        let mut animation = Animation {
            blend_ticks: 4,
            repeat: false,
            ..Animation::new(1, 12, 100, 10)
        };
        assert_eq!(animation.blend_weight(10), 0.);
        assert_eq!(animation.blend_weight(12), 0.5);
        assert_eq!(animation.blend_weight(14), 1.);
        assert_eq!(animation.sample(14, 0, 100.), 0.);
        assert_eq!(animation.sample(18, 0, 100.), 4.);
        animation.seek(30., 18);
        animation.rate = 0.5;
        assert_eq!(animation.sample(18, 0, 100.), 30.);
        assert_eq!(animation.sample(22, 0, 100.), 32.);
        animation.set_paused(true, 23);
        let frozen = animation.sample(23, 0, 100.);
        assert_eq!(animation.sample(40, 0, 100.), frozen);
        animation.set_paused(false, 40);
        assert_eq!(animation.sample(40, 0, 100.), frozen + 0.5);
        animation.seek(99., 40);
        assert_eq!(animation.sample(44, 0, 100.), 100.);
    }

    #[test]
    fn reverse_loops_cross_zero_and_keep_moving_after_multiple_cycles() {
        let mut clip = Animation::new(1, 12, 100, 0);
        clip.start_frame = 10.;
        clip.rate = -2.;
        clip.loop_start = 20.;
        assert_eq!(clip.sample(5, 0, 100.), 0.);
        assert_eq!(clip.sample(6, 0, 100.), 98.);
        assert_eq!(clip.sample(55, 0, 100.), 0.);
        assert_eq!(clip.sample(106, 0, 100.), 98.);
        clip.repeat = false;
        assert_eq!(clip.sample(6, 0, 100.), 0.);
        assert_eq!(clip.sample(106, 0, 100.), 0.);
    }

    #[test]
    fn changing_speed_during_a_script_pause_keeps_the_pose_until_resume() {
        let mut clip = Animation::new(1, 12, 100, 0);
        clip.script_pause(true, 10);
        clip.set_script_rate(2., 20);
        assert_eq!(clip.script_rate(), 2.);
        assert_eq!(clip.sample(30, 0, 100.), 10.);
        clip.script_pause(false, 30);
        assert_eq!(clip.sample(35, 0, 100.), 20.);
        clip.script_pause(false, 35);
        assert_eq!(clip.sample(40, 0, 100.), 30.);
    }
}
