//! Plain values and explicit live setting references, with no native enum wrappers.
use super::RenderValue;
use serde::{Deserialize, Serialize};
#[derive(Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub(super) enum Authored {
    Literal(i32),
    Setting {
        setting: u8,
        #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
        offset: i32,
    },
    Actor(Actor),
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Actor {
    Scenery,
}
impl From<Authored> for RenderValue {
    fn from(value: Authored) -> Self {
        match value {
            Authored::Literal(value) => Self::Fixed(value),
            Authored::Setting { setting, offset: 0 } => Self::Setting(setting),
            Authored::Setting { setting, offset } => Self::SettingOffset {
                slot: setting,
                offset,
            },
            Authored::Actor(Actor::Scenery) => Self::Fixed(super::scenery_actor(0)),
        }
    }
}
impl From<RenderValue> for Authored {
    fn from(value: RenderValue) -> Self {
        match value {
            RenderValue::Fixed(value) if value == super::scenery_actor(0) => {
                Self::Actor(Actor::Scenery)
            }
            RenderValue::Fixed(value) => Self::Literal(value),
            RenderValue::Setting(setting) => Self::Setting { setting, offset: 0 },
            RenderValue::SettingOffset { slot, offset } => Self::Setting {
                setting: slot,
                offset,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn rendering_values_are_direct_and_retain_live_wrapping_arithmetic() {
        for (source, expected) in [
            (json!(42), 42),
            (json!("scenery"), super::super::scenery_actor(0)),
            (json!({"setting":7}), i32::MAX),
            (json!({"setting":7,"offset":1}), i32::MIN),
        ] {
            let value: RenderValue = serde_json::from_value(source.clone()).unwrap();
            assert_eq!(
                value.resolve(&std::collections::BTreeMap::from([(7, i32::MAX)])),
                expected
            );
            assert_eq!(serde_json::to_value(value).unwrap(), source);
        }
        for invalid in [
            json!({"Fixed":42}),
            json!({"Setting":7}),
            json!({"setting":7,"offest":1}),
        ] {
            assert!(serde_json::from_value::<RenderValue>(invalid).is_err());
        }
    }
}
