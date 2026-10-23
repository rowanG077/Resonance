use anyhow::{Context, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Character {
    Lloyd = 1,
    Colette,
    Genis,
    Raine,
    Sheena,
    Zelos,
    Presea,
    Regal,
    Kratos,
}

impl Character {
    pub const ALL: [Self; 9] = [
        Self::Lloyd,
        Self::Colette,
        Self::Genis,
        Self::Raine,
        Self::Sheena,
        Self::Zelos,
        Self::Presea,
        Self::Regal,
        Self::Kratos,
    ];
    pub fn from_index(index: usize) -> Result<Self> {
        Self::ALL
            .get(index)
            .copied()
            .with_context(|| format!("unknown party character index {index}"))
    }

    pub const fn index(self) -> usize {
        self as usize - 1
    }
}

impl TryFrom<u8> for Character {
    type Error = anyhow::Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        value
            .checked_sub(1)
            .and_then(|index| Self::ALL.get(usize::from(index)))
            .copied()
            .with_context(|| format!("unknown party character {value}"))
    }
}
