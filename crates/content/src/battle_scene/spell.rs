//! Parameters for the native fixed-pattern spell strategy, not an instruction stream.
use super::Scene;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SpellRecipe {
    FixedPattern(PatternSpell),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatternSpell {
    /// Active callback ticks; transition, initialization and retirement stay native.
    pub lifetime: u16,
    pub origin: PatternOrigin,
    pub heading_offset: f32,
    pub startup_member: u16,
    pub startup_deferred: bool,
    pub contacts: Vec<PatternContact>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PatternOrigin {
    SceneGround { toward_caster: f32 },
    SceneFixed { position: [f32; 3] },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatternContact {
    /// Creation age in the active native callback, before contact initialization.
    pub tick: u16,
    pub projectile: u8,
    pub hit_variant: u8,
    pub effect_member: u16,
    /// Authored point before rotation by the captured heading.
    pub offset: [f32; 3],
}

impl SpellRecipe {
    pub fn validate(&self, scene: &Scene) -> Result<()> {
        let Self::FixedPattern(pattern) = self;
        pattern.validate()?;
        for contact in &pattern.contacts {
            scene
                .projectiles
                .get(usize::from(contact.projectile))
                .context("spell recipe projectile is missing")?
                .hit_variant(usize::from(contact.hit_variant))?;
        }
        Ok(())
    }
}

impl PatternSpell {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (46..=3600).contains(&self.lifetime),
            "invalid pattern spell lifetime"
        );
        ensure!(
            self.heading_offset.is_finite() && self.heading_offset.abs() <= 360.,
            "invalid pattern spell heading"
        );
        match self.origin {
            PatternOrigin::SceneGround { toward_caster } => ensure!(
                toward_caster.is_finite() && (0. ..=10000.).contains(&toward_caster),
                "invalid pattern spell ground offset"
            ),
            PatternOrigin::SceneFixed { position } => validate_point(position)?,
        }
        ensure!(
            self.startup_member > 0,
            "pattern spell startup effect is disabled"
        );
        ensure!(
            (1..=64).contains(&self.contacts.len()),
            "invalid pattern spell contact count"
        );
        ensure!(
            self.contacts
                .windows(2)
                .all(|pair| pair[0].tick < pair[1].tick),
            "pattern spell contacts are not strictly ordered"
        );
        for contact in &self.contacts {
            ensure!(
                contact.tick < self.lifetime,
                "pattern spell contact exceeds lifetime"
            );
            ensure!(
                contact.effect_member > 0,
                "pattern spell contact effect is disabled"
            );
            validate_point(contact.offset)?;
        }
        Ok(())
    }
}

fn validate_point(point: [f32; 3]) -> Result<()> {
    ensure!(
        point
            .iter()
            .all(|value| value.is_finite() && value.abs() <= 100000.),
        "invalid pattern spell point"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pattern() -> PatternSpell {
        PatternSpell {
            lifetime: 100,
            origin: PatternOrigin::SceneGround { toward_caster: 1. },
            heading_offset: 20.,
            startup_member: 1,
            startup_deferred: true,
            contacts: vec![PatternContact {
                tick: 15,
                projectile: 1,
                hit_variant: 0,
                effect_member: 2,
                offset: [0.; 3],
            }],
        }
    }

    #[test]
    fn fixed_pattern_rejects_invalid_timing_and_nonfinite_geometry() {
        let mut recipe = pattern();
        assert!(recipe.validate().is_ok());
        recipe.contacts.push(recipe.contacts[0].clone());
        assert!(recipe.validate().is_err());
        recipe.contacts[1].tick = recipe.lifetime;
        assert!(recipe.validate().is_err());
        recipe.contacts[1].tick -= 1;
        assert!(recipe.validate().is_ok());
        recipe.contacts[0].offset[2] = f32::NAN;
        assert!(recipe.validate().is_err());
        recipe.contacts[0].offset[2] = 0.;
        recipe.heading_offset = f32::INFINITY;
        assert!(recipe.validate().is_err());
    }

    #[test]
    fn current_scene_reader_rejects_missing_version_and_projectile_bindings() {
        let mut entry = serde_json::json!({
            "version": 1, "rules":"rules.json", "effects":"effects.json",
            "cutin":null, "weapon":null,
        });
        let documents = [
            (
                "rules.json",
                serde_json::json!({
                    "version":1, "projectiles":[], "actions":{}, "actions_error":null,
                    "duration":0, "stage_color":null, "camera":null, "strike_points":[],
                    "recovery_radius":null, "spell_recipe":SpellRecipe::FixedPattern(pattern()),
                }),
            ),
            (
                "effects.json",
                serde_json::json!({
                    "version":1, "programs":"programs.json", "actors":"actors.json", "art":null,
                }),
            ),
            ("programs.json", serde_json::json!([])),
            ("actors.json", serde_json::json!([])),
        ]
        .into_iter()
        .collect::<std::collections::BTreeMap<_, _>>();
        let read = |entry: &serde_json::Value| {
            super::super::document::read(
                "scene.json",
                &crate::diagnostics::Diagnostics::new(true),
                |path| {
                    let value = if path == "scene.json" {
                        entry
                    } else {
                        documents.get(path).context("missing fixture document")?
                    };
                    Ok(serde_json::to_vec(value)?.into())
                },
            )
        };
        assert!(format!("{:#}", read(&entry).unwrap_err()).contains("projectile is missing"));
        entry.as_object_mut().unwrap().remove("version");
        assert!(read(&entry).is_err());
    }
}
