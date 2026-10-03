//! Script-requested menus suspend their caller until the game service closes.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Shop(u8),
    Crafting(u8),
    Main,
}

impl TryFrom<i32> for Target {
    type Error = String;

    fn try_from(selector: i32) -> Result<Self, Self::Error> {
        match selector {
            0..52 => Ok(Self::Shop(selector as u8)),
            9995 => Ok(Self::Main),
            9999..=10020 => Ok(Self::Crafting((selector - 9999) as u8)),
            _ => Err(format!(
                "script menu selector {selector} is not implemented"
            )),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Request {
    pub target: Target,
    pub operation: crate::Operation,
}
