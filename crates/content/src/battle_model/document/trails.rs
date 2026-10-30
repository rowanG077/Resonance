//! Explicit ribbon bindings and a shared, editable material palette.
use super::*;
use std::sync::Arc;

pub const MATERIALS: &str = "battle/trails.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrailDefinition {
    /// Ordered points across the ribbon; repeated joints are permitted.
    pub joints: Vec<String>,
    pub material: TrailStyle,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TrailStyle {
    Named(String),
    Inline(TrailMaterial),
}

impl TrailDefinition {
    pub fn prepare(
        self,
        skeleton: &Skeleton,
        mut read: impl FnMut(&str) -> Result<Arc<[u8]>>,
    ) -> Result<Trail> {
        ensure!(
            (2..=3).contains(&self.joints.len()),
            "trail requires two or three joints"
        );
        let bones = self
            .joints
            .iter()
            .map(|name| {
                let mut matching = skeleton
                    .bones
                    .iter()
                    .enumerate()
                    .filter(|(_, bone)| &bone.name == name);
                let (index, _) = matching
                    .next()
                    .with_context(|| format!("trail refers to absent joint {name}"))?;
                ensure!(
                    matching.next().is_none(),
                    "trail refers to ambiguous joint {name}"
                );
                Ok(u16::try_from(index)?)
            })
            .collect::<Result<_>>()?;
        let material = match self.material {
            TrailStyle::Inline(material) => material,
            TrailStyle::Named(name) => {
                let bytes = read(MATERIALS)?;
                ensure!(
                    bytes.len() <= JSON_LIMIT,
                    "trail materials exceed size limit"
                );
                let materials: BTreeMap<String, TrailMaterial> = serde_json::from_slice(&bytes)
                    .with_context(|| format!("decode {MATERIALS}"))?;
                *materials
                    .get(&name)
                    .with_context(|| format!("unknown trail material {name}"))?
            }
        };
        ensure!(
            material.color.iter().all(|v| v.is_finite() && *v >= 0.),
            "invalid weapon trail color"
        );
        Ok(Trail { bones, material })
    }
}

pub(super) fn prepare_trails(
    definitions: BTreeMap<u8, TrailDefinition>,
    parts: &BTreeMap<u8, ModelPart>,
    diagnostics: &Diagnostics,
    mut read: impl FnMut(&str) -> Result<Arc<[u8]>>,
) -> Result<BTreeMap<u8, Trail>> {
    let mut trails = BTreeMap::new();
    for (slot, definition) in definitions {
        let result = (|| {
            let part = parts
                .get(&slot)
                .with_context(|| format!("trail refers to absent part {slot}"))?;
            definition.prepare(&part.rig.skeleton, &mut read)
        })();
        if let Some(trail) = diagnostics.attempt("battle weapon trail", result)? {
            trails.insert(slot, trail);
        }
    }
    Ok(trails)
}
