//! Names for the battle body's established motion requests. Local action slots
//! remain explicit; the model's animation bindings still map requests to GLBs.
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};

const ROLES: &[(u16, &str)] = &[
    (0, "idle"),
    (1, "walk"),
    (2, "guard"),
    (3, "hurt"),
    (4, "guard-and-alternate-hurt"),
    (5, "rising"),
    (6, "falling"),
    (7, "defeated"),
    (8, "knocked-down"),
    (9, "revival"),
    (10, "get-up"),
    (11, "chant"),
    (12, "cast"),
    (13, "cast-opening"),
    (14, "jump"),
    (15, "airborne"),
    (16, "landing"),
    (17, "breakfall"),
    (18, "stop"),
    (19, "run"),
    (20, "item"),
    (21, "stunned"),
    (24, "entry"),
    (26, "weak-idle"),
    (27, "taunt"),
    (28, "backstep"),
    (29, "acquisition"),
];

pub fn name(slot: u16) -> String {
    ROLES
        .iter()
        .find(|(id, _)| *id == slot)
        .map_or_else(|| format!("slot-{slot}"), |(_, name)| (*name).into())
}
fn parse(name: &str) -> Option<u16> {
    ROLES
        .iter()
        .find(|(_, role)| *role == name)
        .map(|(id, _)| *id)
        .or_else(|| name.strip_prefix("slot-")?.parse().ok())
}

#[derive(Clone, Copy)]
struct Selector(u16);
impl Serialize for Selector {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&name(self.0))
    }
}
impl<'de> Deserialize<'de> for Selector {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let name = String::deserialize(d)?;
        parse(&name)
            .map(Self)
            .ok_or_else(|| D::Error::custom(format!("unknown body motion {name}")))
    }
}
pub fn serialize<S: Serializer>(slot: &u16, s: S) -> Result<S::Ok, S::Error> {
    Selector(*slot).serialize(s)
}
pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u16, D::Error> {
    Ok(Selector::deserialize(d)?.0)
}
pub mod byte {
    use super::*;
    pub fn serialize<S: Serializer>(slot: &u8, s: S) -> Result<S::Ok, S::Error> {
        Selector(u16::from(*slot)).serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u8, D::Error> {
        u8::try_from(Selector::deserialize(d)?.0).map_err(D::Error::custom)
    }
}
pub mod optional_byte {
    use super::*;
    pub fn serialize<S: Serializer>(slot: &Option<u8>, s: S) -> Result<S::Ok, S::Error> {
        slot.map(|n| Selector(u16::from(n))).serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u8>, D::Error> {
        Option::<Selector>::deserialize(d)?
            .map(|n| u8::try_from(n.0).map_err(D::Error::custom))
            .transpose()
    }
}
pub mod pair {
    use super::*;
    pub fn serialize<S: Serializer>(slots: &(u16, u16), s: S) -> Result<S::Ok, S::Error> {
        [Selector(slots.0), Selector(slots.1)].serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<(u16, u16), D::Error> {
        let [a, b] = <[Selector; 2]>::deserialize(d)?;
        Ok((a.0, b.0))
    }
}
pub mod pairs {
    use super::*;
    pub fn serialize<S: Serializer>(slots: &[(u16, u16)], s: S) -> Result<S::Ok, S::Error> {
        slots
            .iter()
            .map(|&(a, b)| [Selector(a), Selector(b)])
            .collect::<Vec<_>>()
            .serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<(u16, u16)>, D::Error> {
        Ok(Vec::<[Selector; 2]>::deserialize(d)?
            .into_iter()
            .map(|[a, b]| (a.0, b.0))
            .collect())
    }
}
