//! Effect emission and animated model parameters can drive contact recipes.
//! Keep those inputs while excluding colors, textures and audiovisual feedback.
use super::{models, record_json, version};
use crate::{
    battle_effect::{self as effects, declaration::Declaration, document},
    library::Snapshot,
    preparation::JSON_LIMIT,
};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use sha2::Sha256;

pub(super) fn capture(
    snapshot: &mut Snapshot,
    hash: &mut Sha256,
    name: &str,
    path: &str,
    cancelled: &impl Fn() -> bool,
) -> Result<()> {
    ensure!(!cancelled(), "effect identity preparation cancelled");
    let entry: document::Definition = snapshot.json(path, JSON_LIMIT)?;
    version(entry.version, document::VERSION, path)?;
    let programs: Vec<Vec<effects::ScheduledEvent>> = snapshot.json(&entry.programs, JSON_LIMIT)?;
    for (index, program) in programs.into_iter().enumerate() {
        ensure!(!cancelled(), "effect identity preparation cancelled");
        let mut events = Vec::new();
        for event in program {
            use effects::EffectOperation as Op;
            let value = match event.operation {
                Op::Spawn {
                    particle, birth, ..
                } => json!({
                    "kind": "spawn", "particle": particle, "birth": birth_projection(*birth)?,
                }),
                Op::SetMotion { spawn, motion } => json!({
                    "kind": "set_motion", "spawn": spawn, "motion": motion,
                }),
                Op::Unsupported { .. } => json!({"kind": "unsupported"}),
                Op::Sound { .. } | Op::Shake { .. } | Op::StageTint { .. } | Op::Notice { .. } => {
                    continue;
                }
            };
            events.push((event.at, value));
        }
        record_json(hash, &format!("{name}/program/{index}"), &events)?;
    }
    let actors: Vec<Declaration> = snapshot.json(&entry.actors, JSON_LIMIT)?;
    for (index, actor) in actors.into_iter().enumerate() {
        let value = match actor {
            Declaration::Particle { template, .. } => json!({
                "kind": "particle", "template": template_projection(template)?,
            }),
            Declaration::ModelParticle {
                template, binding, ..
            } => json!({
                "kind": "model", "template": template_projection(template)?, "binding": binding,
            }),
            Declaration::Unsupported { .. } => json!({"kind": "unsupported"}),
            Declaration::CameraShake { .. }
            | Declaration::StageTint { .. }
            | Declaration::Notice { .. } => json!({"kind": "presentation"}),
        };
        record_json(hash, &format!("{name}/particle/{index}"), &value)?;
    }
    models::effect_models(snapshot, hash, name, path, cancelled)
}

fn birth_projection(mut birth: effects::ParticleBirth) -> Result<Value> {
    birth
        .sums
        .retain(|sum| !matches!(sum.field, effects::BirthField::Uv(_)));
    let mut value = serde_json::to_value(birth)?;
    remove(
        &mut value,
        &[
            "color",
            "end_color",
            "uv",
            "brighten",
            "fade",
            "brighten_until",
            "fade_from",
            "palette",
            "alpha_palette",
            "cull_back",
            "element_tint",
            "ribbon_depth",
        ],
    );
    Ok(value)
}

fn template_projection(template: effects::ParticleTemplate) -> Result<Value> {
    let mut value = serde_json::to_value(template)?;
    remove(
        &mut value,
        &[
            "draw_after_target",
            "element_tint",
            "gradient",
            "fade",
            "fade_from",
            "uv_animation",
        ],
    );
    remove(
        &mut value["state"],
        &[
            "blend",
            "cull_back",
            "ribbon_depth",
            "colors",
            "brighten",
            "brighten_until",
            "uv",
            "palettes",
        ],
    );
    Ok(value)
}

fn remove(value: &mut Value, fields: &[&str]) {
    let value = value
        .as_object_mut()
        .expect("typed gameplay projection is an object");
    for field in fields {
        value.remove(*field);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn particle_colors_and_raster_settings_do_not_hide_motion_or_randomness_edits() -> Result<()> {
        let base = effects::ParticleBirth::default();
        let expected = birth_projection(base.clone())?;
        let mut painted = base;
        painted.cull_back = true;
        painted.element_tint = true;
        painted.ribbon_depth = true;
        painted.palette = Some(effects::ValueRange {
            min: 2.,
            max: 2.,
            step: 0.,
            draw: None,
            reverse: false,
        });
        painted.color[0] = Some(effects::ValueRange {
            min: 128.,
            max: 128.,
            step: 0.,
            draw: None,
            reverse: false,
        });
        assert_eq!(birth_projection(painted.clone())?, expected);
        painted.velocity[0] = Some(effects::ValueRange {
            min: 1.,
            max: 1.,
            step: 0.,
            draw: None,
            reverse: false,
        });
        assert_ne!(birth_projection(painted.clone())?, expected);
        painted.velocity[0] = None;
        painted.random_draws = 1;
        assert_ne!(birth_projection(painted)?, expected);
        Ok(())
    }
}
