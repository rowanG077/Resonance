//! Named Defense System components and exact GLB attachment joints.
use super::*;
use crate::library::Mappings;

pub type DefenseSystem = defense::Definition<String>;

impl DefenseSystem {
    pub fn from_definition(value: defense::Definition, names: &Mappings) -> Result<Self> {
        value.map_enemies(|id| Ok(names.target(id.into())?.into()))
    }

    pub fn prepare(self, names: &Mappings) -> Result<defense::Definition> {
        let result = self.map_enemies(|name| Ok(u8::try_from(names.id(&name)?)?))?;
        result.validate()?;
        Ok(result)
    }
}
