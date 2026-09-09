//! Choice input owns confirm/cancel edges and repeats on gameplay updates.
use resonance_events::dialogue::{Choice, ChoiceExit};

#[derive(Debug, Clone, Copy, Default)]
pub struct ChoiceInput {
    /// -1 moves up; +1 moves down. Values outside that range are clamped.
    pub direction: i8,
    pub confirm: bool,
    pub cancel: bool,
}

#[derive(Default)]
pub struct ChoicePlayer {
    operation: Option<u64>,
    direction: i8,
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
        let direction = input.direction.clamp(-1, 1);
        let repeat = direction != 0
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
        let old = choice.selected_line;
        if repeat {
            let count = i32::from(choice.last_line - choice.first_line) + 1;
            choice.selected_line = choice.first_line
                + (i32::from(choice.selected_line - choice.first_line) + i32::from(direction))
                    .rem_euclid(count) as u8;
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
        (reason, choice.selected_line != old)
    }
}
