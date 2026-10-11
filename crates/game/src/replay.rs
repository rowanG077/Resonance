//! Development input fixtures expressed in native title updates.
use crate::MenuInput;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TitleReplay {
    pub version: u32,
    pub inputs: Vec<InputSpan>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputSpan {
    /// First affected update, starting at one.
    pub tick: u32,
    pub duration: u32,
    pub buttons: Vec<Button>,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Button {
    Up,
    Down,
    Accept,
}

impl TitleReplay {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.version == 1 && self.inputs.len() <= 10000,
            "unsupported title replay"
        );
        for span in &self.inputs {
            anyhow::ensure!(
                span.tick > 0
                    && span.duration > 0
                    && !span.buttons.is_empty()
                    && span
                        .tick
                        .checked_add(span.duration)
                        .is_some_and(|end| end <= 1_000_000),
                "invalid input span"
            );
        }
        Ok(())
    }

    pub fn held(&self, tick: u32) -> MenuInput {
        let mut held = MenuInput::default();
        for span in &self.inputs {
            if tick >= span.tick && tick - span.tick < span.duration {
                for button in &span.buttons {
                    match button {
                        Button::Up => held.up = true,
                        Button::Down => held.down = true,
                        Button::Accept => held.accept = true,
                    }
                }
            }
        }
        held.reveal = held.up || held.down || held.accept;
        held
    }
}
