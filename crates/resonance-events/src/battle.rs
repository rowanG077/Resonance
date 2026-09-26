//! Field-to-battle handoff. The field keeps its VM and clocks while the game
//! prepares and runs the encounter, then resumes the original caller once.
use crate::Operation;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefeatPolicy {
    GameOver,
    ResumeEvent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Setup {
    pub encounter: u16,
    pub arena: u16,
    pub defeat: DefeatPolicy,
    /// An explicit music selection; None uses the current world's battle theme.
    pub music: Option<u16>,
}

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Escaped = 1,
    Victory = 2,
    Defeat = 3,
}

impl TryFrom<i32> for Outcome {
    type Error = String;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Escaped),
            2 => Ok(Self::Victory),
            3 => Ok(Self::Defeat),
            _ => Err(format!("invalid battle result {value}")),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Request {
    pub setup: Setup,
    pub(crate) operation: Operation,
}

impl Request {
    pub fn id(&self) -> u64 {
        self.operation.id()
    }

    pub fn is_pending(&self) -> bool {
        self.operation.is_pending()
    }

    /// Complete after writeback and results presentation. Fatal defeat retires
    /// the field instead, cancelling its caller and outstanding callbacks.
    pub fn complete(&self, outcome: Outcome) -> Result<(), String> {
        if outcome == Outcome::Defeat && self.setup.defeat == DefeatPolicy::GameOver {
            return Err("battle defeat requires game over".into());
        }
        self.operation.complete(Some(outcome as i32))
    }
}
