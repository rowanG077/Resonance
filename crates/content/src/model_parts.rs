//! One model-local set of shared textures, motions and dynamics. Each ordered
//! part may override a value, including an explicit empty array. No file lookup
//! or recursive inheritance is involved; preparation receives ordinary parts.
use serde::{Deserialize, Serialize, de::DeserializeOwned, ser::SerializeMap};
use serde_json::{Map, Value};

const SETTINGS: [&str; 3] = ["textures", "clips", "secondary_motion"];

pub fn serialize<S: serde::Serializer, P: Serialize>(
    parts: &[P],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serialize_named(parts, "parts", serializer)
}

pub fn deserialize<'de, D: serde::Deserializer<'de>, P: DeserializeOwned>(
    deserializer: D,
) -> Result<Vec<P>, D::Error> {
    deserialize_named(deserializer, "parts")
}

fn serialize_named<S: serde::Serializer, P: Serialize>(
    parts: &[P],
    name: &str,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    // Preserve f32's short decimal spelling when building the shared document.
    let bytes = serde_json::to_vec(parts).map_err(serde::ser::Error::custom)?;
    let mut parts: Value = serde_json::from_slice(&bytes).map_err(serde::ser::Error::custom)?;
    let parts = parts.as_array_mut().expect("serialized part list");
    let mut shared = Map::new();
    if parts.len() > 1 {
        for key in SETTINGS {
            let Some(value) = parts[0].get(key) else {
                continue;
            };
            if value.as_array().is_some_and(|v| !v.is_empty())
                && parts.iter().all(|part| part.get(key) == Some(value))
            {
                shared.insert(key.into(), value.clone());
                for part in &mut *parts {
                    part.as_object_mut().expect("model part").remove(key);
                }
            }
        }
    }
    let mut map = serializer.serialize_map(None)?;
    if !shared.is_empty() {
        map.serialize_entry("shared", &shared)?;
    }
    map.serialize_entry(name, parts)?;
    map.end()
}

fn deserialize_named<'de, D: serde::Deserializer<'de>, P: DeserializeOwned>(
    deserializer: D,
    name: &str,
) -> Result<Vec<P>, D::Error> {
    // Typed fields let serde's flattened struct consume these keys. A generic
    // map would leave them visible to the parent's deny_unknown_fields check.
    #[derive(Deserialize)]
    struct Parts {
        #[serde(default)]
        shared: Map<String, Value>,
        parts: Vec<Map<String, Value>>,
    }
    #[derive(Deserialize)]
    struct Layers {
        #[serde(default)]
        shared: Map<String, Value>,
        layers: Vec<Map<String, Value>>,
    }
    let (shared, parts) = if name == "parts" {
        let value = Parts::deserialize(deserializer)?;
        (value.shared, value.parts)
    } else {
        let value = Layers::deserialize(deserializer)?;
        (value.shared, value.layers)
    };
    for key in shared.keys() {
        if !SETTINGS.contains(&key.as_str()) {
            return Err(serde::de::Error::custom(format!(
                "unknown shared model setting {key}"
            )));
        }
    }
    parts
        .into_iter()
        .map(|mut part| {
            for (key, value) in &shared {
                part.entry(key.clone()).or_insert_with(|| value.clone());
            }
            serde_json::from_value(Value::Object(part)).map_err(serde::de::Error::custom)
        })
        .collect()
}

/// Battle attachments call the same ordered parts `layers`.
pub mod layers {
    use super::*;
    pub fn serialize<S: serde::Serializer, P: Serialize>(
        parts: &[P],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serialize_named(parts, "layers", serializer)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>, P: DeserializeOwned>(
        deserializer: D,
    ) -> Result<Vec<P>, D::Error> {
        deserialize_named(deserializer, "layers")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{field::document::Model, model_preview::ModelPreview};

    fn model() -> Value {
        serde_json::json!({
            "shared": {
                "textures": ["model/art.ktx2"],
                "clips": [{"motion":"model/idle.glb","slots":[12,36]}],
                "secondary_motion": [{"name":"hair","joints":["hair-root","hair-tip"],"gravity":0.3,"damping":0.8}]
            },
            "parts": [
                {"mesh":"model/body.glb"},
                {"mesh":"model/outline.glb","resource":1,"outline_color":[0,0,0,127]}
            ]
        })
    }

    #[test]
    fn shared_settings_expand_without_changing_part_order_or_clip_targets() -> anyhow::Result<()> {
        let before = model();
        let prepared: Model = serde_json::from_value(before.clone())?;
        assert_eq!(prepared.parts.len(), 2);
        assert_eq!(prepared.parts[0].resource, 0);
        assert_eq!(prepared.parts[1].resource, 1);
        assert_eq!(prepared.parts[0].textures, prepared.parts[1].textures);
        assert_eq!(prepared.parts[0].clips, prepared.parts[1].clips);
        assert_eq!(
            prepared.parts[0]
                .clips
                .iter()
                .map(|c| c.resource_slot)
                .collect::<Vec<_>>(),
            [12, 36]
        );
        assert_eq!(
            prepared.parts[0].secondary_motion,
            prepared.parts[1].secondary_motion
        );
        assert_eq!(serde_json::to_value(prepared)?, before);
        Ok(())
    }

    #[test]
    fn edits_to_shared_settings_propagate_and_empty_overrides_stay_empty() -> anyhow::Result<()> {
        let mut source = model();
        source["shared"]["textures"][0] = "model/modded.ktx2".into();
        source["parts"][1]["clips"] = serde_json::json!([]);
        source["parts"][1]["secondary_motion"] = serde_json::json!([]);
        let prepared: Model = serde_json::from_value(source)?;
        assert!(
            prepared
                .parts
                .iter()
                .all(|p| p.textures == ["model/modded.ktx2"])
        );
        assert_eq!(prepared.parts[0].clips.len(), 2);
        assert!(prepared.parts[1].clips.is_empty());
        assert!(prepared.parts[1].secondary_motion.is_empty());
        let written = serde_json::to_value(&prepared)?;
        assert!(written["shared"].get("clips").is_none());
        assert!(written["shared"].get("secondary_motion").is_none());
        let roundtrip: Model = serde_json::from_value(written)?;
        assert!(roundtrip.parts[1].clips.is_empty());
        assert!(roundtrip.parts[1].secondary_motion.is_empty());
        Ok(())
    }

    #[test]
    fn previews_and_attachments_share_settings_but_keep_independent_placement() -> anyhow::Result<()>
    {
        let mut source = model();
        source["scale"] = 2.into();
        source["elevation"] = 3.into();
        source["parts"][1]["attached_to"] = "hand".into();
        let preview: ModelPreview = serde_json::from_value(source)?;
        assert_eq!(preview.scale, 2.);
        assert_eq!(preview.elevation, 3.);
        assert_eq!(preview.parts[1].attached_to.as_deref(), Some("hand"));
        let mut source = model();
        source["layers"] = source.as_object_mut().unwrap().remove("parts").unwrap();
        source["independent_layers"] = serde_json::json!([1]);
        let attachment: crate::battle_model::document::Model =
            serde_json::from_value(source.clone())?;
        assert_eq!(attachment.layers[1].scene.resource, 1);
        assert!(attachment.independent_layers.contains(&1));
        assert_eq!(serde_json::to_value(attachment)?, source);
        Ok(())
    }

    #[test]
    fn unrelated_values_and_unknown_shared_settings_are_rejected() {
        for value in [
            serde_json::json!({"unknown":[]}),
            serde_json::json!({"mesh":"model/body.glb"}),
        ] {
            let mut source = model();
            source["shared"] = value;
            assert!(serde_json::from_value::<Model>(source).is_err());
        }
        let mut source = model();
        source["unused"] = true.into();
        assert!(serde_json::from_value::<Model>(source).is_err());
    }
}

#[cfg(test)]
mod installed_tests;
