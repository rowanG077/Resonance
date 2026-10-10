//! Artwork for stored spell effects.
use crate::battle_model::ModelPart;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub fn path(technique: u16) -> String {
    format!("battle/scenes/{technique:03}.json")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scene {
    pub source_sha256: String,
    /// Standard source bank, consumed by the shared effect preparation path.
    pub effects: String,
    pub textures: Vec<Texture>,
    /// Original model slots. Aliases share physical assets, not playback state.
    pub models: BTreeMap<u8, ModelPart>,
    pub files: BTreeMap<String, crate::field_preload::File>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Texture {
    /// Palette pages retain their original order.
    pub images: Vec<crate::font::UiTexture>,
    pub sampler: crate::texture::Sampler,
}
