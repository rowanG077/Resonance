//! Bind selected effect timelines to shared particle and audio resources.
use anyhow::{Context, Result, ensure};
use resonance_battle::{EffectBank, EffectDefinition, ParticleDefinition, ResourceId, Sound};
use resonance_content::{
    battle_effect::{EffectOperation, SourceBank, TINTS_PATH, Tints, declaration::Declaration},
    diagnostics::Diagnostics,
    prepared::Files,
};

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
pub(crate) mod cache;

pub fn load(files: &Files, path: &str) -> Result<Option<Arc<SourceBank>>> {
    let result =
        resonance_content::battle_effect::document::read(path, files.diagnostics(), |path| {
            files.read(path)
        });
    files.diagnostics().attempt(path, result.map(Arc::new))
}

pub fn load_tints(files: &Files) -> Result<Option<Tints>> {
    files
        .diagnostics()
        .attempt(TINTS_PATH, files.json(TINTS_PATH))
}

pub(super) fn validate_owner(
    resource: &super::EffectResource,
    model: Option<&resonance_battle::ModelDefinition>,
) -> Result<()> {
    let Some(bank) = &resource.bank else {
        return Ok(());
    };
    for &member in &resource.members {
        for event in bank.program(usize::from(member))? {
            if let EffectOperation::Spawn {
                particle, birth, ..
            } = &event.operation
            {
                let template = bank.particle(usize::from(*particle))?;
                for bone in [
                    birth.origin_bone,
                    birth
                        .attachment_bone
                        .or(template.follow_bone)
                        .or(template.owner_bone),
                ]
                .into_iter()
                .flatten()
                {
                    let model = model.context("effect attachment owner has no prepared body")?;
                    let bones = model
                        .weapons
                        .iter()
                        .find(|weapon| {
                            weapon.slot.is_some_and(|slot| u16::from(slot.0) == member)
                                && weapon.effect_interval.is_some()
                        })
                        .map_or(model.skeleton.bones.len(), |weapon| weapon.bone_count());
                    ensure!(
                        usize::from(bone) < bones,
                        "effect {}:{member} bone {bone} is outside owner model",
                        resource.resource
                    );
                }
            }
        }
    }
    Ok(())
}

/// One member's shared timeline. Its cues are bound by each selecting bank.
fn definition(
    source: &SourceBank,
    member: u16,
    particles: &mut BTreeMap<u8, Arc<ParticleDefinition>>,
) -> Result<EffectDefinition> {
    let mut definition = EffectDefinition {
        events: source.program(usize::from(member))?.to_vec().into(),
        ..Default::default()
    };
    for event in definition.events.iter() {
        if let EffectOperation::Spawn { particle, .. } = event.operation {
            if let std::collections::btree_map::Entry::Vacant(entry) = particles.entry(particle) {
                let data = source.particle(usize::from(particle))?.clone();
                let model = match &source.actors[usize::from(particle)] {
                    Declaration::ModelParticle { binding, .. } => Some(*binding),
                    _ => None,
                };
                entry.insert(Arc::new(ParticleDefinition {
                    member: u16::from(particle),
                    model,
                    data,
                }));
            }
            definition
                .particles
                .insert(particle, particles[&particle].clone());
        }
    }
    Ok(definition)
}

pub fn prepare(
    source: &SourceBank,
    resource: ResourceId,
    members: &[u16],
    models: BTreeMap<u8, resonance_battle::PreparedEffectModel>,
    sound: &mut impl FnMut(u16) -> Result<Option<Sound>>,
    diagnostics: &Diagnostics,
) -> Result<EffectBank> {
    let mut definitions = BTreeMap::new();
    let mut particles = BTreeMap::new();
    for member in members.iter().copied().collect::<BTreeSet<_>>() {
        let definition = definition(source, member, &mut particles).map(Arc::new);
        if let Some(definition) =
            diagnostics.attempt(&format!("battle effect {resource}:{member}"), definition)?
        {
            definitions.insert(member, definition);
        }
    }
    // Optional audio never discards an otherwise usable animation.
    EffectBank::new(resource, models, definitions, sound, diagnostics)
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::battle_effect::ScheduledEvent;

    #[test]
    fn unavailable_audio_takes_no_part_in_timeline_order_validation() -> Result<()> {
        let source = SourceBank {
            document: None,
            template_names: vec![],
            actors: vec![],
            art: None,
            programs: vec![vec![
                ScheduledEvent {
                    at: 100,
                    operation: EffectOperation::Sound { id: 7, priority: 0 },
                },
                ScheduledEvent {
                    at: 0,
                    operation: EffectOperation::Notice {
                        text: "Ready".into(),
                    },
                },
            ]],
        };
        for available in [false, true] {
            let bank = prepare(
                &source,
                ResourceId(77),
                &[0],
                BTreeMap::new(),
                &mut |_| Ok(available.then_some(Sound::Cue(7))),
                &Diagnostics::new(true),
            );
            assert_eq!(bank.is_ok(), !available);
        }
        Ok(())
    }

    #[test]
    fn unavailable_effects_and_sounds_leave_other_visuals_usable() -> Result<()> {
        let timeline = |operation| vec![ScheduledEvent { at: 0, operation }];
        let source = SourceBank {
            template_names: Vec::new(),
            document: None,
            programs: vec![
                timeline(EffectOperation::Sound { id: 7, priority: 0 }),
                timeline(EffectOperation::Unsupported {
                    reason: "unavailable animation".into(),
                }),
                vec![],
            ],
            actors: vec![],
            art: None,
        };
        let diagnostics = Diagnostics::default();
        let bank = prepare(
            &source,
            ResourceId(77),
            &[0, 1, 2, 2],
            BTreeMap::new(),
            &mut |_| Ok(None),
            &diagnostics,
        )?;
        assert!(bank.member(0).is_some() && bank.sound(7).is_none());
        assert!(bank.member(1).is_none());
        assert!(bank.member(2).is_some());
        assert_eq!(diagnostics.entries().len(), 1);
        assert!(
            prepare(
                &source,
                ResourceId(77),
                &[1],
                BTreeMap::new(),
                &mut |_| unreachable!(),
                &Diagnostics::new(true)
            )
            .is_err()
        );
        Ok(())
    }
}
