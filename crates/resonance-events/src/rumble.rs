//! Simulation-clock controller feedback. Zero duration stops; -1 runs until stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Duration {
    Continuous,
    Until(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    Coast,
    Brake,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rumble {
    pub port: u8,
    pub duration: Duration,
    pub stop: Stop,
}
impl Rumble {
    pub fn new(port: i32, duration: i32, brake: bool, tick: u32) -> Result<Self, &'static str> {
        if !(0..4).contains(&port) {
            return Err("invalid controller port");
        }
        Ok(Self {
            port: port as u8,
            duration: if duration == -1 {
                Duration::Continuous
            } else {
                Duration::Until(tick.saturating_add(duration.max(0) as u32))
            },
            stop: if brake { Stop::Brake } else { Stop::Coast },
        })
    }
    pub fn remaining(self, tick: u32) -> Option<u32> {
        match self.duration {
            Duration::Continuous => None,
            Duration::Until(end) => Some(end.saturating_sub(tick)),
        }
    }
}
