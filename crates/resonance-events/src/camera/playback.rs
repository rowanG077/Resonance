use crate::CameraTrack;
use resonance_content::CameraKey;

impl CameraTrack {
    pub fn new(resource: u32, tick: u32) -> Self {
        Self {
            resource,
            start_tick: tick,
            start_frame: 0.,
            rate: 0.5,
            playing: true,
            repeat: false,
            completed: false,
            target_actor: 0,
            target_offset: [0.; 3],
        }
    }

    fn elapsed(&self, tick: u32) -> f32 {
        self.start_frame
            + if self.playing {
                tick.saturating_sub(self.start_tick) as f32 * self.rate
            } else {
                0.
            }
    }

    pub fn finished(&self, tick: u32, duration: f32) -> bool {
        self.completed || self.elapsed(tick) > duration
    }

    pub fn frame(&self, tick: u32, duration: f32) -> f32 {
        let frame = self.elapsed(tick);
        if duration > 0. && (frame < 0. || self.repeat && frame > duration) {
            let wrapped = frame.rem_euclid(duration);
            if wrapped == 0. && frame > 0. {
                duration
            } else {
                wrapped
            }
        } else {
            frame.clamp(0., duration)
        }
    }

    pub(crate) fn seek(&mut self, frame: f32, tick: u32) {
        self.start_frame = frame;
        self.start_tick = tick;
    }

    pub(crate) fn retime(&mut self, tick: u32, duration: f32) -> f32 {
        let frame = self.frame(tick, duration);
        self.completed = self.finished(tick, duration);
        if !self.repeat && self.elapsed(tick) > duration {
            self.playing = false;
        }
        self.seek(frame, tick);
        frame
    }

    pub(crate) fn configure(
        &mut self,
        operation: i32,
        value: i32,
        tick: u32,
        duration: f32,
    ) -> Result<i32, String> {
        let frame = self.retime(tick, duration);
        Ok(match operation {
            0 => {
                self.playing = true;
                0
            }
            1 | 2 => {
                self.playing = false;
                if operation == 2 {
                    self.seek(0., tick);
                }
                0
            }
            3 => {
                self.seek((value as f32).clamp(0., duration), tick);
                0
            }
            4 => {
                self.rate = value as f32 / 100.;
                0
            }
            5 => {
                self.repeat = value != 0;
                0
            }
            6 => frame as i32,
            7 => duration as i32,
            8 => self.rate as i32,
            9 => i32::from(self.repeat),
            10 => i32::from(self.completed),
            11 => {
                self.target_actor = value;
                value
            }
            12..=14 => {
                self.target_offset[(operation - 12) as usize] = value as f32;
                value
            }
            _ => return Err("unknown camera track operation".into()),
        })
    }

    pub fn sample(&self, tick: u32, keys: &[CameraKey]) -> Option<([f32; 3], [f32; 3])> {
        let time = self.frame(tick, keys.last()?.time);
        let right = keys
            .partition_point(|key| key.time < time)
            .min(keys.len() - 1);
        let a = &keys[right.saturating_sub(1)];
        let b = &keys[right];
        let fraction = if a.time == b.time {
            0.
        } else {
            (time - a.time) / (b.time - a.time)
        };
        let interpolate =
            |a: [f32; 3], b: [f32; 3]| std::array::from_fn(|i| a[i] + (b[i] - a[i]) * fraction);
        Some((
            interpolate(a.position, b.position),
            interpolate(a.target, b.target),
        ))
    }
}
