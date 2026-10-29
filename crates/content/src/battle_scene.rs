//! Artwork and contact metadata for stored battle scenes.
use crate::battle_model::Weapon;
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub mod document;
mod spell;
pub use spell::{PatternContact, PatternOrigin, PatternSpell, SpellRecipe};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, bound(deserialize = "W: Deserialize<'de>"))]
pub struct Scene<W = Weapon> {
    /// Standard source bank, consumed by the shared effect preparation path.
    pub effects: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cutin: Option<Texture>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weapon: Option<W>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projectiles: Vec<SceneProjectile>,
    /// Participant action tracks, in the scene's authored variant order.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub actions: BTreeMap<u8, SceneAction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actions_error: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spell_recipe: Option<SpellRecipe>,
}

impl<W> Scene<W> {
    /// Only the attachment binding differs between authored and prepared scenes.
    pub fn with_weapon<T>(self, weapon: Option<T>) -> Scene<T> {
        Scene {
            effects: self.effects,
            cutin: self.cutin,
            weapon,
            projectiles: self.projectiles,
            actions: self.actions,
            actions_error: self.actions_error,
            duration: self.duration,
            stage_color: self.stage_color,
            camera: self.camera,
            strike_points: self.strike_points,
            recovery_radius: self.recovery_radius,
            spell_recipe: self.spell_recipe,
        }
    }
}

impl Scene {
    pub fn validate(&self) -> Result<()> {
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
    #[serde(with = "crate::texture::images")]
    pub images: Vec<crate::font::UiTexture>,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub sampler: crate::texture::Sampler,
}
