//! Rheiard world changes retain the source scene until its departure finishes.
use super::World;

const DEPARTURE_FADE_TICKS: u32 = 128;
const ARRIVAL_FADE_TICKS: u32 = 16;
const HOLD_TICKS: u32 = 3;
const EFFECT_TICKS: u32 = 120;

#[derive(Debug, Clone, Copy)]
pub enum Portal {
    Departure { destination: World, ticks: u32 },
    Arrival { ticks: u32 },
}

impl Portal {
    /// White overlay opacity, independent of the destination's preparation time.
    pub fn fade(&self) -> u8 {
        match *self {
            Self::Departure { ticks, .. } => (ticks * 2).min(255) as u8,
            Self::Arrival { ticks } => 255u32.saturating_sub(ticks * 16) as u8,
        }
    }

    /// Riders and their vehicles dissolve together before the whiteout.
    pub fn vehicle_opacity(&self) -> u8 {
        match self {
            Self::Departure { .. } => {
                const START: u8 = 80;
                const END: u8 = 110;
                (u16::from(END - self.fade().clamp(START, END)) * 255 / u16::from(END - START))
                    as u8
            }
            Self::Arrival { .. } => 255,
        }
    }

    /// All active riders share the same departure-effect age.
    pub fn effect_ticks(&self) -> Option<u32> {
        match *self {
            Self::Departure { ticks, .. } if ticks < EFFECT_TICKS => Some(ticks),
            _ => None,
        }
    }

    pub(super) fn destination(&self) -> Option<World> {
        match *self {
            Self::Departure { destination, ticks }
                if ticks >= DEPARTURE_FADE_TICKS + HOLD_TICKS =>
            {
                Some(destination)
            }
            _ => None,
        }
    }

    /// Departure remains owned while the destination is being prepared.
    pub(super) fn step(&mut self) -> bool {
        match self {
            Self::Departure { ticks, .. } => {
                *ticks = (*ticks + 1).min(DEPARTURE_FADE_TICKS + HOLD_TICKS);
                false
            }
            Self::Arrival { ticks } => {
                *ticks += 1;
                *ticks >= ARRIVAL_FADE_TICKS + HOLD_TICKS
            }
        }
    }
}
