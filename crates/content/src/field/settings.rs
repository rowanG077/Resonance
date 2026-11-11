//! Shared field appearance and notices; scenes contain only local overrides.
use super::*;
use crate::{
    effect::{BlinkCycle, FieldEffects, FlutterRecipe},
    font::SystemText,
};

pub const PATH: &str = "fields/settings.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub effects: FieldEffects,
    pub blink: BlinkCycle,
    pub contact_shadow: ContactShadow,
    pub toon_ramp: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub particles: BTreeMap<i32, FlutterRecipe>,
    pub save_point: SavePointText,
    #[serde(default, skip_serializing_if = "EnvironmentDefaults::is_empty")]
    pub environment: EnvironmentDefaults,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavePointText {
    pub tutorial: SystemText,
    pub unlock: SystemText,
    pub no_gem: SystemText,
}

impl Settings {
    pub fn resolve_particles(
        &self,
        local: Option<BTreeMap<i32, FlutterRecipe>>,
    ) -> BTreeMap<i32, FlutterRecipe> {
        let mut particles = local.unwrap_or_else(|| self.particles.clone());
        for recipe in particles.values_mut() {
            if recipe.palette.is_empty() {
                recipe.palette.clone_from(&self.effects.palette);
            }
        }
        particles
    }

    pub fn from_field(effects: FieldEffects, field: &FieldAssets) -> Self {
        Self {
            effects,
            environment: EnvironmentDefaults::default(),
            blink: field.blink.clone(),
            contact_shadow: field.contact_shadow.clone(),
            toon_ramp: field.toon_ramp.clone(),
            particles: field.particles.clone(),
            save_point: SavePointText::from_field(field),
        }
    }
}

impl SavePointText {
    pub fn from_field(field: &FieldAssets) -> Self {
        Self {
            tutorial: SystemText(field.save_point_tutorial.clone()),
            unlock: SystemText(field.save_point_unlock.clone()),
            no_gem: SystemText(field.save_point_no_gem.clone()),
        }
    }
}
