//! Named battle effect banks. Programs and particle declarations are editable
//! semantic data; artwork is reached through explicit model and image references.
use super::{Art, EffectTexture, SourceBank};
use crate::{
    diagnostics::Diagnostics,
    library::Snapshot,
    preparation::{ASSET_LIMIT, JSON_LIMIT},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};

pub const COMMON: &str = "battle/effects/common/effects.json";
pub const TECHNIQUES: &str = "battle/effects/techniques/effects.json";

mod programs;
mod templates;
#[cfg(test)]
mod tests;
pub use templates::STUN_BIRD;

#[derive(Debug, Clone, Deserialize)]
#[serde(try_from = "Authored")]
pub struct Definition {
    pub programs: Vec<Vec<super::ScheduledEvent>>,
    pub template_names: Vec<String>,
    pub templates: Vec<super::declaration::Declaration>,
    pub artwork: Option<Artwork>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Authored {
    #[serde(default, skip_serializing_if = "programs::Programs::is_empty")]
    programs: programs::Programs,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    templates: BTreeMap<String, super::declaration::Declaration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    artwork: Option<Artwork>,
}
impl Serialize for Definition {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let authored = || -> Result<Authored> {
            templates::validate(&self.template_names, self.templates.len())?;
            Ok(Authored {
                programs: programs::Programs::from_runtime(&self.programs, &self.template_names)?,
                templates: self
                    .template_names
                    .iter()
                    .cloned()
                    .zip(self.templates.iter().cloned())
                    .collect(),
                artwork: self.artwork.clone(),
            })
        };
        authored()
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
}
impl TryFrom<Authored> for Definition {
    type Error = anyhow::Error;
    fn try_from(authored: Authored) -> Result<Self> {
        let (template_names, templates): (Vec<_>, Vec<_>) = authored.templates.into_iter().unzip();
        templates::validate(&template_names, templates.len())?;
        Ok(Self {
            programs: authored.programs.prepare(&template_names)?,
            template_names,
            templates,
            artwork: authored.artwork,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artwork {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[serde(with = "super::labels::atlas::bindings")]
    pub textures: BTreeMap<u8, Vec<EffectTexture>>,
    // Decode each optional visual independently, so one malformed edit does not
    // prevent healthy model slots from loading in normal diagnostic mode.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub models: BTreeMap<u8, serde_json::Value>,
}

impl Artwork {
    pub fn from_models(
        textures: BTreeMap<u8, Vec<EffectTexture>>,
        models: BTreeMap<u8, crate::battle_model::document::Model>,
    ) -> Result<Self> {
        Ok(Self {
            textures,
            // Preserve the short decimal spelling of f32 material settings.
            models: serde_json::from_slice(&serde_json::to_vec(&models)?)?,
        })
    }
}

/// Assemble from already captured bytes. This also serves the runtime reader;
/// it never needs a synthetic legacy descriptor inserted into the file store.
pub fn read(
    path: &str,
    diagnostics: &Diagnostics,
    mut read: impl FnMut(&str) -> Result<Arc<[u8]>>,
) -> Result<SourceBank> {
    let definition: Definition = json(path, &mut read)?;
    let art = definition
        .artwork
        .map(|mut artwork| -> Result<_> {
            let mut models = BTreeMap::new();
            for (slot, value) in artwork.models {
                let candidate =
                    serde_json::from_value::<crate::battle_model::document::Model>(value)
                        .map_err(anyhow::Error::from)
                        .and_then(|model| model.prepare(&mut read));
                if let Some(model) =
                    diagnostics.attempt(&format!("{path} model {slot}"), candidate)?
                {
                    models.insert(slot, model);
                }
            }
            for bank in artwork.textures.values_mut() {
                for texture in bank {
                    for image in &mut texture.images {
                        diagnostics
                            .attempt(path, crate::texture::images::prepare(image, &mut read))?;
                    }
                }
            }
            Ok(Art {
                textures: artwork.textures,
                models,
            })
        })
        .transpose()?;
    Ok(SourceBank {
        document: Some(path.to_owned()),
        programs: definition.programs,
        template_names: definition.template_names,
        actors: definition.templates,
        art,
    })
}

pub fn capture(
    snapshot: &mut Snapshot,
    path: &str,
    diagnostics: &Diagnostics,
    cancelled: &impl Fn() -> bool,
) -> Result<SourceBank> {
    let result = read(path, diagnostics, |resource| {
        ensure!(!cancelled(), "battle effect preparation cancelled");
        snapshot.read(resource, ASSET_LIMIT)
    });
    ensure!(!cancelled(), "battle effect preparation cancelled");
    let bank = result?;
    if let Some(art) = &bank.art {
        for texture in art
            .models
            .values()
            .flat_map(|model| &model.layers)
            .flat_map(|layer| &layer.scene.textures)
        {
            ensure!(!cancelled(), "battle effect preparation cancelled");
            diagnostics.attempt(path, snapshot.read(texture, ASSET_LIMIT))?;
        }
    }
    ensure!(!cancelled(), "battle effect preparation cancelled");
    Ok(bank)
}

fn json<T: serde::de::DeserializeOwned>(
    path: &str,
    read: &mut impl FnMut(&str) -> Result<Arc<[u8]>>,
) -> Result<T> {
    crate::library::validate_path(path)?;
    let bytes = read(path)?;
    ensure!(
        bytes.len() <= JSON_LIMIT,
        "battle document exceeds size limit: {path}"
    );
    serde_json::from_slice(&bytes).with_context(|| format!("decode battle document {path}"))
}
