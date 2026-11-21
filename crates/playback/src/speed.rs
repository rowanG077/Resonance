/// Discrete playback rates shared by the game clock and audio transport.
/// Tenths represent every supported rate exactly, without fractional drift.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Speed {
    Tenth,
    Fifth,
    Half,
    #[default]
    Normal,
    Double,
}

impl Speed {
    pub(crate) const fn tenths(self) -> u64 {
        match self {
            Self::Tenth => 1,
            Self::Fifth => 2,
            Self::Half => 5,
            Self::Normal => 10,
            Self::Double => 20,
        }
    }

    pub fn factor(self) -> f64 {
        self.tenths() as f64 / 10.
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Tenth => "1/10X",
            Self::Fifth => "1/5X",
            Self::Half => "1/2X",
            Self::Normal => "1X",
            Self::Double => "2X",
        }
    }
}
