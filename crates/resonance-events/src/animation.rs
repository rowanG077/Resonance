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
}
#[derive(Debug, Clone)]
pub struct Animation {
    pub resource: u32,
    /// Position and rate use nominal 60-Hz cooked animation ticks.
    pub start_frame: f32,
    pub rate: f32,
    pub loop_start: f32,
    pub blend_ticks: u32,
    pub duration_ticks: u32,
    pub slot: u16,
    pub start_tick: u32,
    /// Explicit native binding evaluates once before the ordinary actor update.
    pub binding_updates: u32,
    /// Last seek/rate update; changing speed does not restart a cross-fade.
    pub phase_tick: u32,
    pub repeat: bool,
}
impl Animation {
    pub fn new(resource: u32, slot: u16, duration_ticks: u32, tick: u32) -> Self {
        Self {
            resource,
            slot,
            duration_ticks,
            start_tick: tick,
            phase_tick: tick,
            start_frame: 0.,
            rate: 1.,
            loop_start: 0.,
            blend_ticks: 0,
            binding_updates: 0,
            repeat: true,
        }
    }

    pub fn elapsed(&self, tick: u32, presentation_delay: u32) -> f32 {
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
}
