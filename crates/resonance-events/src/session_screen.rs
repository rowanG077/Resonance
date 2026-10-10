//! Whole-session screens owned by presentation, outside the field dispatcher.
use crate::Operation;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Title,
    GameOver,
    Credits,
}

#[derive(Debug, Clone)]
pub struct Request {
    pub target: Target,
    pub operation: Operation,
}
