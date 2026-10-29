//! Shared atmosphere defaults are flat properties, applied only to opted-in effects.
use super::*;
use anyhow::Context;
use serde_json::{Map, Value};
type Properties = Map<String, Value>;

// JSON allows either 12 or 12.0 for the same field value. All numeric recipe
// types are f32 or at most 32-bit integers, so f64 preserves their comparison.
fn same_value(first: &Value, second: &Value) -> bool {
    match (first, second) {
        (Value::Number(a), Value::Number(b)) => a.as_f64() == b.as_f64(),
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same_value(a, b))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(key, value)| b.get(key).is_some_and(|other| same_value(value, other)))
        }
        _ => first == second,
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EnvironmentDefaults(pub BTreeMap<String, Properties>);
impl EnvironmentDefaults {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    fn apply(&self, kind: &str, local: Properties) -> Properties {
        let mut result = self.0.get(kind).cloned().unwrap_or_default();
        result.extend(local);
        result
    }
    fn compact(&self, kind: &str, mut local: Properties) -> Properties {
        if let Some(shared) = self.0.get(kind) {
            local.retain(|key, value| {
                key == "kind"
                    || !shared
                        .get(key)
                        .is_some_and(|other| same_value(value, other))
            });
        }
        if local.get("when").and_then(Value::as_str) == Some("always")
            && self
                .0
                .get(kind)
                .and_then(|values| values.get("when"))
                .is_none()
        {
            local.remove("when");
        }
        local
    }
    pub fn textures_mut(&mut self) -> impl Iterator<Item = (&str, &mut String)> {
        self.0.iter_mut().filter_map(|(kind, record)| {
            match record.get_mut("sprite")?.get_mut("texture")? {
                Value::String(path) => Some((kind.as_str(), path)),
                _ => None,
            }
        })
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentDocument {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    particles: Vec<Properties>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    wind: Option<Properties>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    heat_haze: Option<Properties>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lights: Option<Properties>,
}
impl EnvironmentDocument {
    pub fn is_empty(&self) -> bool {
        self.particles.is_empty()
            && self.wind.is_none()
            && self.heat_haze.is_none()
            && self.lights.is_none()
    }
    /// Only explicit local texture overrides; inherited textures belong to defaults.
    pub fn textures_mut(&mut self) -> impl Iterator<Item = &mut String> {
        self.particles
            .iter_mut()
            .chain(self.heat_haze.iter_mut())
            .filter_map(
                |record| match record.get_mut("sprite")?.get_mut("texture")? {
                    Value::String(path) => Some(path),
                    _ => None,
                },
            )
    }

    pub fn resolve(self, shared: &EnvironmentDefaults) -> Result<FieldEnvironment> {
        ensure!(
            shared.0.keys().all(|key| matches!(
                key.as_str(),
                "drifting" | "fire" | "snow" | "wind" | "heat_haze" | "lights"
            )),
            "unknown field environment defaults"
        );
        let mut particles = Vec::new();
        for record in self.particles {
            let kind = record
                .get("kind")
                .and_then(Value::as_str)
                .context("field emitter needs a kind")?
                .to_owned();
            particles.push(serde_json::from_value(Value::Object(
                shared.apply(&kind, record),
            ))?);
        }
        let resolve = |kind: &str, record: Option<Properties>| {
            record.map(|record| Value::Object(shared.apply(kind, record)))
        };
        let environment = FieldEnvironment {
            particles,
            wind: resolve("wind", self.wind)
                .map(serde_json::from_value)
                .transpose()?,
            heat_haze: resolve("heat_haze", self.heat_haze)
                .map(serde_json::from_value)
                .transpose()?,
            lights: resolve("lights", self.lights)
                .map(serde_json::from_value)
                .transpose()?,
        };
        environment.validate()?;
        Ok(environment)
    }
    pub fn from_environment(
        environment: &FieldEnvironment,
        shared: &EnvironmentDefaults,
    ) -> Result<Self> {
        // Retain f32's short decimal spelling when comparing authored values.
        let mut document: Self = serde_json::from_slice(&serde_json::to_vec(environment)?)?;
        for record in &mut document.particles {
            let kind = record["kind"]
                .as_str()
                .context("missing emitter kind")?
                .to_owned();
            *record = shared.compact(&kind, std::mem::take(record));
        }
        for (kind, record) in [
            ("wind", &mut document.wind),
            ("heat_haze", &mut document.heat_haze),
            ("lights", &mut document.lights),
        ] {
            if let Some(record) = record {
                *record = shared.compact(kind, std::mem::take(record));
            }
        }
        Ok(document)
    }
}

#[cfg(test)]
mod tests;
