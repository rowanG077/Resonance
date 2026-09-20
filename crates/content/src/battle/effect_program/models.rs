//! Validate every possible effect model before presentation preparation starts.
use super::*;
use crate::battle::visual::EffectModel;
use std::collections::BTreeMap;

impl BattleEffectPrograms {
    pub fn validate_models(&self, prepared: &[EffectModel]) -> Result<()> {
        let prepared: BTreeMap<_, _> = prepared.iter().map(|m| (m.binding, m)).collect();
        for actor in &self.actors {
            let Geometry::Model {
                animation,
                presentation,
                ..
            } = actor.geometry
            else {
                continue;
            };
            for binding in self.models_for(actor.id) {
                if binding == ModelRef::ColetteWeapon {
                    // Equipment is resolved at battle entry, then uses an already resident weapon.
                    ensure!(
                        animation.is_none() && !presentation.external_animation,
                        "dynamic Colette weapon cannot use an archive model animation"
                    );
                    continue;
                }
                let model = prepared
                    .get(&binding)
                    .with_context(|| format!("uncooked battle effect model {binding:?}"))?;
                if let Some(slot) = animation {
                    validate_clip(model, slot)?;
                }
                if !presentation.external_animation {
                    continue;
                }
                let rig = model.rig.as_ref().with_context(|| {
                    format!("effect model {binding:?} has no required external rig")
                })?;
                model.validate_pose_joints()?;
                for animation in
                    self.modifiers_for(actor)
                        .flatten()
                        .filter_map(|modifier| match modifier {
                            Modifier::PlayModelAnimation { animation }
                                if animation.model == binding.index() =>
                            {
                                Some(animation)
                            }
                            _ => None,
                        })
                {
                    validate_clip(model, animation.clip)?;
                    let clip = rig
                        .motions
                        .get(&u16::from(animation.clip))
                        .with_context(|| {
                            format!(
                                "effect model {binding:?} external rig lacks clip {}",
                                animation.clip
                            )
                        })?;
                    ensure!(
                        clip.duration_frames.is_finite()
                            && clip.duration_frames > 0.
                            && animation.rate.abs() <= clip.duration_frames,
                        "effect model {binding:?} has invalid external playback for clip {}",
                        animation.clip
                    );
                }
            }
        }
        Ok(())
    }
}

fn validate_clip(model: &EffectModel, slot: u8) -> Result<()> {
    ensure!(
        !model.model.parts.is_empty(),
        "effect model {:?} has no layers",
        model.binding
    );
    for (index, part) in model.model.parts.iter().enumerate() {
        ensure!(
            part.scene
                .clips
                .iter()
                .any(|clip| clip.resource_slot == u16::from(slot)
                    && clip.duration_seconds.is_finite()
                    && clip.duration_seconds > 0.),
            "effect model {:?} layer {index} lacks playable clip {slot}",
            model.binding
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests;
