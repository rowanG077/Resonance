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

pub const VERSION: u32 = 1;
pub const COMMON: &str = "battle/effects/common/effects.json";
pub const TECHNIQUES: &str = "battle/effects/techniques/effects.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub version: u32,
    pub programs: String,
    pub actors: String,
    pub art: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artwork {
    pub version: u32,
    pub textures: BTreeMap<u8, Vec<EffectTexture>>,
    pub models: BTreeMap<u8, String>,
}

/// Assemble from already captured bytes. This also serves the runtime reader;
/// it never needs a synthetic legacy descriptor inserted into the file store.
pub fn read(
    path: &str,
    diagnostics: &Diagnostics,
    mut read: impl FnMut(&str) -> Result<Arc<[u8]>>,
) -> Result<SourceBank> {
    let definition: Definition = json(path, &mut read)?;
    version(definition.version, path)?;
    let programs = json(&definition.programs, &mut read)?;
    let actors = json(&definition.actors, &mut read)?;
    let art = definition
        .art
        .map(|path| -> Result<_> {
            let artwork: Artwork = json(&path, &mut read)?;
            version(artwork.version, &path)?;
            let mut models = BTreeMap::new();
            for (slot, path) in artwork.models {
                let candidate = crate::battle_model::document::read_model(&path, &mut read);
                if let Some(model) =
                    diagnostics.attempt(&format!("{path} model {slot}"), candidate)?
                {
                    models.insert(slot, model);
                }
            }
            for bank in artwork.textures.values() {
                for texture in bank {
                    texture.sampler.validate()?;
                    ensure!(!texture.images.is_empty(), "empty effect texture in {path}");
                    ensure!(
                        texture
                            .palette_sets
                            .iter()
                            .flatten()
                            .flatten()
                            .all(|&index| index < texture.images.len()),
                        "invalid effect palette in {path}"
                    );
                    for image in &texture.images {
                        image.validate()?;
                    }
                }
            }
            Ok(Art {
                source_sha256: String::new(),
                textures: artwork.textures,
                models,
            })
        })
        .transpose()?;
    Ok(SourceBank {
        document: Some(path.to_owned()),
        source_sha256: String::new(),
        programs,
        actors,
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
        for texture in art.textures.values().flatten() {
            for image in &texture.images {
                ensure!(!cancelled(), "battle effect preparation cancelled");
                diagnostics.attempt(path, snapshot.read(&image.path, ASSET_LIMIT))?;
            }
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

fn version(value: u32, path: &str) -> Result<()> {
    ensure!(
        value == VERSION,
        "unsupported battle effect document version in {path}"
    );
    Ok(())
}
