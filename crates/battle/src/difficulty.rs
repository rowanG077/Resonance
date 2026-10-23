#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Difficulty {
    Normal = 0,
    Hard = 1,
    Mania = 2,
}

impl TryFrom<u8> for Difficulty {
    type Error = anyhow::Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Normal),
            1 => Ok(Self::Hard),
            2 => Ok(Self::Mania),
            _ => anyhow::bail!("invalid battle difficulty {value}"),
        }
    }
}
