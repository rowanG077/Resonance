//! Choice input owns confirm/cancel edges and repeats on gameplay updates.
use resonance_events::dialogue::{Choice, ChoiceExit, Selection};

#[derive(Debug, Clone, Copy, Default)]
pub struct ChoiceInput {
    /// -1 moves up; +1 moves down. Values outside that range are clamped.
    pub direction: i8,
    /// -1 selects the digit to the left; +1 selects the digit to the right.
    pub horizontal: i8,
    pub confirm: bool,
    pub cancel: bool,
}

#[derive(Default)]
pub struct ChoicePlayer {
    operation: Option<u64>,
    direction: [i8; 2],
    held_ticks: u16,
    ready_ticks: u16,
}

impl ChoicePlayer {
    /// Returns a completion reason and whether the cursor moved. Text must be
    /// fully revealed before choices accept input or count down their timeout.
    pub fn step(
        &mut self,
        choice: &mut Choice,
        input: ChoiceInput,
        text_ready: bool,
    ) -> (Option<ChoiceExit>, bool) {
        if self.operation != Some(choice.operation.id()) {
            *self = Self {
                operation: Some(choice.operation.id()),
                ..Self::default()
            };
        }
        if !text_ready || choice.operation.progress().outcome.is_some() {
            return (None, false);
        }
        let direction = [input.horizontal.clamp(-1, 1), input.direction.clamp(-1, 1)];
        let repeat = direction != [0, 0]
            && if direction != self.direction {
                self.held_ticks = 0;
                true
            } else {
                self.held_ticks = if self.held_ticks >= 24 {
                    20
                } else {
                    self.held_ticks + 1
                };
                self.held_ticks == 20
            };
        self.direction = direction;
        let old = choice.selection.value();
        let mut moved = false;
        if repeat {
            match &mut choice.selection {
                Selection::Lines(lines) => {
                    let count = i32::from(lines.last_line - lines.first_line) + 1;
                    lines.selected_line = lines.first_line
                        + (i32::from(lines.selected_line - lines.first_line)
                            + i32::from(direction[1]))
                        .rem_euclid(count) as u8;
                }
                Selection::Number(number) => {
                    let step = 10_i64.pow(u32::from(number.place));
                    if direction[1] != 0 && step <= i64::from(number.maximum) {
                        let value = i64::from(number.value);
                        let delta = -i64::from(direction[1]);
                        let candidate = if number.wrap_digits {
                            let digit = value / step % 10;
                            value + ((digit + delta).rem_euclid(10) - digit) * step
                        } else {
                            value + delta * step
                        };
                        let bounds = i64::from(number.minimum)..=i64::from(number.maximum);
                        if !number.wrap_digits || bounds.contains(&candidate) {
                            number.value = candidate.clamp(*bounds.start(), *bounds.end()) as i32;
                        }
                    }
                    if direction[0] != 0 {
                        number.place = (i32::from(number.place) - i32::from(direction[0]))
                            .rem_euclid(i32::from(number.digits))
                            as u8;
                        moved = number.digits > 1;
                    }
                }
            }
        }
        self.ready_ticks = self.ready_ticks.saturating_add(1);
        let reason = if input.confirm {
            Some(ChoiceExit::Confirm)
        } else if input.cancel && choice.cancel_allowed {
            Some(ChoiceExit::Cancel)
        } else if choice
            .timeout_ticks
            .is_some_and(|ticks| self.ready_ticks >= ticks)
        {
            Some(ChoiceExit::Timeout)
        } else {
            None
        };
        (reason, moved || choice.selection.value() != old)
    }
}
