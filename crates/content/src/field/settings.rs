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

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Context;
    use std::{fs, path::Path, sync::Arc};

    #[test]
    #[ignore = "requires installed fields and saved pre-migration metadata; no media conversion"]
    fn all_field_settings_preserve_saved_values_and_save_point_ownership() -> Result<()> {
        let assets =
            std::env::var_os("RESONANCE_TEST_ASSETS").context("set RESONANCE_TEST_ASSETS")?;
        let root = Path::new(&assets);
        let before = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../local/structure-review-20261027/field-settings/migration/before");
        let names: BTreeMap<u32, String> =
            serde_json::from_slice(&fs::read(root.join("fields/mappings.json"))?)?;
        let mut notices = 0;
        for (&id, name) in &names {
            let path = format!("fields/{name}");
            let definition: document::Definition =
                serde_json::from_slice(&fs::read(root.join(format!("{path}/field.json")))?)?;
            let field = definition.assemble_documents(id, &mut |p: &str| {
                Ok(Arc::<[u8]>::from(fs::read(root.join(p))?))
            })?;
            let mut old: serde_json::Value =
                serde_json::from_slice(&fs::read(before.join(format!("{path}/scene.json")))?)?;
            let saved = serde_json::from_slice::<serde_json::Value>(&serde_json::to_vec(&field)?)?;
            old["contact_shadow"]["texture"] = "effects/field/textures/field-effects.ktx2".into();
            for particle in old["particles"].as_object_mut().unwrap().values_mut() {
                particle["texture"] = "effects/field/textures/field-effects.ktx2".into();
            }
            assert_eq!(
                serde_json::to_value(&field.blink.frames)?,
                old["blink"]["frames"],
                "field {id}: blink"
            );
            for key in ["contact_shadow", "particles", "toon_ramp"] {
                assert_eq!(saved[key], old[key], "field {id}: {key}");
            }
            for key in [
                "save_point_tutorial",
                "save_point_unlock",
                "save_point_no_gem",
            ] {
                assert_eq!(
                    saved.get(key).cloned().unwrap_or(serde_json::json!([])),
                    old.get(key).cloned().unwrap_or(serde_json::json!([])),
                    "field {id}: {key}"
                );
            }
            notices += usize::from(!field.save_point_tutorial.is_empty());
        }
        assert_eq!((names.len(), notices), (501, 75));
        println!(
            "All 501 fields preserve shared appearance and particles; all 75 save-point owners retain their exact notices."
        );
        Ok(())
    }
}
