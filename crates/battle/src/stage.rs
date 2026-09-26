//! Stage color requests (45604), priority clocks (45DD8) and RGB approach (45514).
#[derive(Debug, Clone)]
pub(crate) struct StageColors {
    pub base: [u8; 4],
    pub models: [Option<[u8; 4]>; 4],
    pub channels: [Channel; 2],
}

#[derive(Debug, Clone)]
pub(crate) struct Channel {
    pub target: [u8; 4],
    pub remaining: u16,
    pub step: u8,
    pub active: bool,
}

impl Default for StageColors {
    fn default() -> Self {
        Self {
            base: [64, 64, 64, 255],
            models: [None; 4],
            channels: std::array::from_fn(|_| Channel {
                target: [64, 64, 64, 255],
                remaining: 0,
                step: 0,
                active: false,
            }),
        }
    }
}

impl StageColors {
    pub fn request(&mut self, index: usize, color: [u8; 4], duration: u16, step: u8) {
        if duration == 0 {
            for (i, model) in self.models.iter_mut().enumerate() {
                if let Some(model) = model {
                    *model = color;
                    // The original immediate branch clears flags by model
                    // slot, rather than by the requested color channel.
                    if let Some(channel) = self.channels.get_mut(i) {
                        channel.active = false;
                    }
                }
            }
            self.channels[index].remaining = 0;
            return;
        }
        let lower = self.channels[0].target;
        let channel = &mut self.channels[index];
        if index == 0 && channel.remaining != 0 {
            channel.remaining = channel.remaining.max(duration);
            for (target, color) in channel.target[..3].iter_mut().zip(color) {
                *target = (*target).min(color);
            }
        } else {
            channel.remaining = duration;
            if index == 1 {
                for i in 0..3 {
                    channel.target[i] = color[i].min(lower[i]);
                }
            } else {
                channel.target = color;
            }
        }
        channel.step = step;
        channel.active = true;
    }

    pub fn step(&mut self) {
        // Only the highest active timer advances, even on its expiry update.
        for channel in self.channels.iter_mut().rev() {
            if channel.active && channel.remaining != 0 {
                channel.remaining -= 1;
                if channel.remaining == 0 {
                    channel.active = false;
                    channel.target = [64, 64, 64, 255];
                }
                break;
            }
        }
        let (target, step) = self
            .channels
            .iter()
            .rev()
            .find(|c| c.active)
            .map_or((self.base, 2), |c| (c.target, c.step));
        for model in self.models.iter_mut().flatten() {
            for (value, target) in model[..3].iter_mut().zip(target) {
                if value.abs_diff(target) <= step {
                    *value = target;
                } else if *value < target {
                    *value += step;
                } else {
                    *value -= step;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_requests_merge_and_only_the_priority_timer_advances() {
        let mut stage = StageColors {
            models: [Some([64, 64, 64, 201]), None, None, None],
            ..Default::default()
        };
        stage.request(0, [48, 40, 32, 90], 3, 4);
        stage.request(0, [50, 20, 30, 7], 2, 8);
        assert_eq!(stage.channels[0].target, [48, 20, 30, 90]);
        stage.request(1, [24, 60, 12, 0], 2, 16);
        assert_eq!(stage.channels[1].target, [24, 20, 12, 255]);
        for (remaining, color) in [
            ([3, 1], [48, 48, 48, 201]),
            ([3, 0], [48, 40, 40, 201]),
            ([2, 0], [48, 32, 32, 201]),
            ([1, 0], [48, 24, 30, 201]),
            ([0, 0], [50, 26, 32, 201]),
        ] {
            stage.step();
            assert_eq!(stage.channels.each_ref().map(|c| c.remaining), remaining);
            assert_eq!(stage.models[0], Some(color));
        }
        assert!(stage.channels.iter().all(|c| !c.active));
        assert!(stage.channels.iter().all(|c| c.target == [64, 64, 64, 255]));
    }

    #[test]
    fn immediate_color_copies_alpha_and_clears_flags_for_present_model_slots() {
        let mut stage = StageColors {
            models: [Some([64; 4]), None, None, Some([64; 4])],
            ..Default::default()
        };
        stage.request(0, [24; 4], 5, 4);
        stage.request(1, [12; 4], 7, 4);
        stage.request(0, [99, 88, 77, 66], 0, 0);
        assert_eq!(
            stage.models,
            [Some([99, 88, 77, 66]), None, None, Some([99, 88, 77, 66])]
        );
        assert!(!stage.channels[0].active);
        assert_eq!(stage.channels[0].remaining, 0);
        assert!(stage.channels[1].active);
        assert_eq!(stage.channels[1].remaining, 7);
    }

    #[test]
    fn restored_stage_color_uses_metadata_and_clamps_without_changing_alpha() {
        let mut stage = StageColors {
            base: [0, 127, 255, 255],
            models: [Some([3, 126, 252, 33]), None, None, None],
            ..Default::default()
        };
        stage.step();
        assert_eq!(stage.models[0], Some([1, 127, 254, 33]));
        stage.step();
        assert_eq!(stage.models[0], Some([0, 127, 255, 33]));
        stage.step();
        assert_eq!(stage.models[0], Some([0, 127, 255, 33]));
    }
}
