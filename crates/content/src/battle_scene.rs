//! Artwork and contact metadata for stored battle scenes.
use crate::battle_model::{ModelPart, Weapon};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub mod document;
mod spell;
pub use spell::{PatternContact, PatternOrigin, PatternSpell, SpellRecipe};

/// Version 1 makes the migrated spell recipe part of the scene publication.
pub const VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize)]
pub struct Scene {
    pub version: u32,
    pub source_sha256: String,
    /// Standard source bank, consumed by the shared effect preparation path.
    pub effects: String,
    pub textures: Vec<Texture>,
    /// Authored model slots. Aliases share physical assets, not playback state.
    pub models: BTreeMap<u8, ModelPart>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cutin: Option<Texture>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weapon: Option<Weapon>,
    #[serde(default)]
    pub projectiles: Vec<SceneProjectile>,
    /// Participant action tracks, in the scene's authored variant order.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub actions: BTreeMap<u8, SceneAction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actions_error: Option<String>,
    #[serde(default)]
    pub duration: u16,
    /// Scenery RGB uses 64 as neutral; actor lighting is independent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage_color: Option<[u8; 3]>,
    /// Active-scene framing constrains ordinary tracking after the caster close-up.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera: Option<CameraBounds>,
    /// Ordered strike locations for scenes with a fixed spatial pattern.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub strike_points: Vec<[f32; 3]>,
    /// Ground radius for recovery scenes that select nearby recipients.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery_radius: Option<f32>,
    /// Native strategies consume these parameters after resolving all dependencies.
    pub spell_recipe: Option<SpellRecipe>,
}

impl Scene {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == VERSION,
            "unsupported battle scene version; recook assets"
        );
        if let Some(recipe) = &self.spell_recipe {
            recipe.validate(self)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SceneAction {
    Prepared(Box<crate::battle_martial::Attack>),
    Unsupported { reason: String },
}

impl SceneAction {
    pub fn prepared(&self) -> Result<&crate::battle_martial::Attack> {
        match self {
            Self::Prepared(attack) => Ok(attack),
            Self::Unsupported { reason } => bail!("scene action is not prepared: {reason}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CameraBounds {
    pub minimum_distance: f32,
    pub minimum_elevation: f32,
}

impl CameraBounds {
    pub fn validate(self) -> Result<()> {
        ensure!(
            self.minimum_distance.is_finite()
                && self.minimum_distance >= 0.
                && self.minimum_elevation.is_finite()
                && (0. ..90.).contains(&self.minimum_elevation),
            "invalid scene camera bounds"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneProjectile {
    pub projectile: crate::battle_projectile::Projectile,
    pub hit: Option<crate::battle_action::ActionHit>,
    /// Authored contact variants, bound to this template's damage kind and recoil.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hits: Vec<Option<crate::battle_action::ActionHit>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub hit_errors: BTreeMap<usize, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unsupported_reason: Option<String>,
}

impl SceneProjectile {
    pub fn hit(&self) -> Result<&crate::battle_action::ActionHit> {
        if let Some(reason) = &self.unsupported_reason {
            bail!("scene contact is not prepared: {reason}");
        }
        self.hit.as_ref().context("scene projectile has no contact")
    }

    pub fn hit_variant(&self, index: usize) -> Result<&crate::battle_action::ActionHit> {
        if let Some(reason) = self.hit_errors.get(&index) {
            bail!("scene contact variant {index} is not prepared: {reason}");
        }
        self.hits
            .get(index)
            .and_then(Option::as_ref)
            .with_context(|| format!("scene projectile has no contact variant {index}"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Texture {
    /// Palette pages retain their original order.
    pub images: Vec<crate::font::UiTexture>,
    pub sampler: crate::texture::Sampler,
}
