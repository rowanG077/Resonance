//! Authored animation playback. Slots are resource-table byte offsets.
pub mod slot {
    pub const IDLE: u16 = 12;
    pub const TALK: u16 = 24;
    pub const WALK: u16 = 36;
    pub const RUN: u16 = 40;
    pub const TURN_RIGHT: u16 = 44;
    pub const TURN_LEFT: u16 = 48;
    pub const TALK_FALLBACK: u16 = 52;
    pub const EVENT_TALK: u16 = 112;
    pub const EVENT_IDLE: u16 = 116;
    pub const EVENT_WALK: u16 = 120;
    pub const EVENT_RUN: u16 = 124;
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingTiming {
    BeforeDraw,
    AfterDraw,
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
    /// Explicit native binding evaluates once before the ordinary actor update.
    pub binding_updates: u32,
    /// VM bindings can be observed by attachments before their first model draw.
    pub binding_timing: BindingTiming,
    /// Last seek/rate update; changing speed does not restart a cross-fade.
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
            binding_updates: 0,
            binding_timing: BindingTiming::BeforeDraw,
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
        let binding_age = tick
            .saturating_sub(self.start_tick)
            .saturating_add(self.binding_updates);
        let phase_age = tick.saturating_sub(self.phase_tick).saturating_add(
            if self.phase_tick == self.start_tick {
                self.binding_updates
            } else {
                0
            },
        );
        self.start_frame
            + binding_age
                .saturating_sub(self.blend_ticks.saturating_sub(1))
                .min(phase_age)
                .saturating_sub(presentation_delay) as f32
                * self.rate
    }
    /// Hold the new clip’s first sample while the old pose blends out.
    /// Blend weights progress from 1/(duration+1) to duration/(duration+1).
    pub fn blend_weight(&self, tick: u32) -> f32 {
        let tick = self.animation_tick(tick);
        let age = tick
            .saturating_sub(self.start_tick)
            .saturating_add(self.binding_updates);
        if age >= self.blend_ticks {
            1.
        } else {
            (age + 1) as f32 / (self.blend_ticks + 1) as f32
        }
    }
    pub fn sample(&self, tick: u32, presentation_delay: u32, duration: f32) -> f32 {
        let elapsed = self.elapsed(tick, presentation_delay).max(0.);
        if self.repeat && duration > self.loop_start && elapsed > duration {
            let phase = (elapsed - self.loop_start) % (duration - self.loop_start);
            if phase == 0. {
                duration
            } else {
                self.loop_start + phase
            }
        } else {
            elapsed.min(duration)
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

#[cfg(test)]
mod tests {
    use super::*;

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
