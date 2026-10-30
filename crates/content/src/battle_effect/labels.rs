//! Readable authoring names for established draw modes and artwork scopes.
//! Runtime request numbers remain local to the existing effect preparation.
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;

pub mod blend {
    use super::*;
    #[derive(Clone, Copy, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    enum Mode {
        Alpha,
        Additive,
        Subtractive,
    }
    impl TryFrom<u8> for Mode {
        type Error = &'static str;
        fn try_from(value: u8) -> Result<Self, Self::Error> {
            match value {
                0 => Ok(Self::Alpha),
                1 => Ok(Self::Additive),
                2 => Ok(Self::Subtractive),
                _ => Err("unsupported particle blend"),
            }
        }
    }
    impl From<Mode> for u8 {
        fn from(value: Mode) -> Self {
            match value {
                Mode::Alpha => 0,
                Mode::Additive => 1,
                Mode::Subtractive => 2,
            }
        }
    }
    pub fn serialize<S: Serializer>(value: &u8, serializer: S) -> Result<S::Ok, S::Error> {
        Mode::try_from(*value)
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u8, D::Error> {
        Ok(Mode::deserialize(deserializer)?.into())
    }
    pub mod optional {
        use super::*;
        pub fn serialize<S: Serializer>(
            value: &Option<u8>,
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            value
                .map(Mode::try_from)
                .transpose()
                .map_err(serde::ser::Error::custom)?
                .serialize(serializer)
        }
        pub fn deserialize<'de, D: Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Option<u8>, D::Error> {
            Ok(Option::<Mode>::deserialize(deserializer)?.map(Into::into))
        }
    }
}

pub mod atlas {
    use super::*;
    fn name(slot: u8) -> String {
        match slot {
            2 => "actor".into(),
            6 => "scene".into(),
            9 => "arena".into(),
            14 => "actor-secondary".into(),
            _ => format!("bank-{slot}"),
        }
    }
    fn slot(name: &str) -> Result<u8, String> {
        match name {
            "actor" => Ok(2),
            "scene" => Ok(6),
            "arena" => Ok(9),
            "actor-secondary" => Ok(14),
            _ => name
                .strip_prefix("bank-")
                .and_then(|v| v.parse().ok())
                .ok_or_else(|| format!("unknown effect atlas {name}")),
        }
    }
    pub fn serialize<S: Serializer>(value: &u8, serializer: S) -> Result<S::Ok, S::Error> {
        name(*value).serialize(serializer)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u8, D::Error> {
        slot(&String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
    pub mod bindings {
        use super::*;
        pub fn serialize<T: Serialize, S: Serializer>(
            value: &BTreeMap<u8, T>,
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            value
                .iter()
                .map(|(&key, value)| (name(key), value))
                .collect::<BTreeMap<_, _>>()
                .serialize(serializer)
        }
        pub fn deserialize<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
            deserializer: D,
        ) -> Result<BTreeMap<u8, T>, D::Error> {
            let mut result = BTreeMap::new();
            for (name, value) in BTreeMap::<String, T>::deserialize(deserializer)? {
                let key = slot(&name).map_err(serde::de::Error::custom)?;
                if result.insert(key, value).is_some() {
                    return Err(serde::de::Error::custom("duplicate effect atlas binding"));
                }
            }
            Ok(result)
        }
    }
}
