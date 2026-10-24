//! Editable title artwork, background selection and menu audio references.
use crate::{BootAssets, TitleAssets, TitleTexture, library::Snapshot, preparation};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const PATH: &str = "ui/title/title.json";
pub const BOOT: &str = "ui/boot/boot.json";
pub const VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub version: u32,
    pub day_field: u32,
    pub night_field: u32,
    pub textures: Vec<TitleTexture>,
    pub music: String,
    pub cues: String,
}

impl Definition {
    pub fn art(&self, game: &crate::library::Descriptor) -> Result<TitleAssets> {
        ensure!(self.version == VERSION, "unsupported named title version");
        crate::library::validate_path(&self.music)?;
        crate::library::validate_path(&self.cues)?;
        let art = TitleAssets {
            version: crate::CONTENT_VERSION,
            game_id: game.game.clone(),
            revision: game.revision.try_into()?,
            textures: self.textures.clone(),
        };
        art.validate()?;
        Ok(art)
    }
}

/// Cue names are the menu controller's interface; these are direct paths to
/// editable programs, not another ID catalogue or a dependency inventory.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cues<Entry = String> {
    pub version: u32,
    pub reverbs: [[f32; 5]; 2],
    pub cues: BTreeMap<String, Entry>,
}

pub fn capture(
    snapshot: &mut Snapshot,
    boot: bool,
    cancelled: &impl Fn() -> bool,
) -> Result<(Definition, TitleAssets, Option<BootAssets>)> {
    ensure!(!cancelled(), "title preparation cancelled");
    let definition: Definition = snapshot.json(PATH, preparation::JSON_LIMIT)?;
    let art = definition.art(snapshot.descriptor())?;
    for texture in &art.textures {
        ensure!(!cancelled(), "title preparation cancelled");
        snapshot.read(&texture.path, preparation::ASSET_LIMIT)?;
    }
    let boot = if boot {
        let boot: BootAssets = snapshot.json(BOOT, preparation::JSON_LIMIT)?;
        boot.validate()?;
        for texture in &boot.textures {
            ensure!(!cancelled(), "title preparation cancelled");
            snapshot.read(&texture.path, preparation::ASSET_LIMIT)?;
        }
        Some(boot)
    } else {
        None
    };
    ensure!(!cancelled(), "title preparation cancelled");
    Ok((definition, art, boot))
}
