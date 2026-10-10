//! Bind selected effect timelines to shared particle and audio resources.
use anyhow::Result;
use resonance_battle::{EffectBank, EffectDefinition, ParticleDefinition, Sound};
use resonance_content::{
    battle_effect::{EffectOperation, SourceBank, TINTS_PATH, Tints, declaration::Declaration},
    diagnostics::Diagnostics,
    prepared::Files,
};

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

pub fn load(files: &Files, path: &str) -> Result<Option<Arc<SourceBank>>> {
    files
        .diagnostics()
        .attempt(path, files.json(path).map(Arc::new))
}

pub fn load_tints(files: &Files) -> Result<Option<Tints>> {
    files
        .diagnostics()
        .attempt(TINTS_PATH, files.json(TINTS_PATH))
}

pub fn prepare(
    source: &SourceBank,
    resource: u32,
    members: &[u16],
    models: BTreeMap<u8, resonance_battle::PreparedEffectModel>,
    sound: &mut impl FnMut(u16) -> Result<Option<Sound>>,
    diagnostics: &Diagnostics,
) -> Result<EffectBank> {
    let mut definitions = BTreeMap::new();
    let mut particles = BTreeMap::new();
    let mut sounds = BTreeMap::new();
    for member in members.iter().copied().collect::<BTreeSet<_>>() {
        let definition = (|| -> Result<_> {
            let mut definition = EffectDefinition {
                events: source.program(usize::from(member))?.to_vec(),
                ..Default::default()
            };
            for event in &definition.events {
                match event.operation {
                    EffectOperation::Spawn { particle, .. } => {
                        if let std::collections::btree_map::Entry::Vacant(entry) =
                            particles.entry(particle)
                        {
                            let data = source.particle(usize::from(particle))?.clone();
                            let model = match &source.actors[usize::from(particle)] {
                                Declaration::ModelParticle { binding, .. } => Some(*binding),
                                _ => None,
                            };
                            entry.insert(Arc::new(ParticleDefinition {
                                resource,
                                member: u16::from(particle),
                                model,
                                data,
                            }));
                        }
                        definition
                            .particles
                            .insert(particle, Arc::clone(&particles[&particle]));
                    }
                    EffectOperation::Sound { id, .. } if id != 0 => {
                        if let std::collections::btree_map::Entry::Vacant(entry) = sounds.entry(id)
                        {
                            entry.insert(sound(id)?);
                        }
                        if let Some(binding) = sounds[&id] {
                            definition.sounds.insert(id, binding);
                        }
                    }
                    _ => {}
                }
            }
            // Missing audio does not discard an otherwise usable animation.
            definition.events.retain(|event| match event.operation {
                EffectOperation::Sound { id, .. } => definition.sounds.contains_key(&id),
                _ => true,
            });
            Ok(Arc::new(definition))
        })();
        if let Some(definition) =
            diagnostics.attempt(&format!("battle effect {resource}:{member}"), definition)?
        {
            definitions.insert(member, definition);
        }
    }
    EffectBank::new(resource, models, definitions, diagnostics)
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::battle_effect::ScheduledEvent;

    #[test]
    fn unavailable_effects_and_sounds_leave_other_visuals_usable() -> Result<()> {
        let timeline = |operation| vec![ScheduledEvent { at: 0, operation }];
        let source = SourceBank {
            source_sha256: "fixture".into(),
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
            77,
            &[0, 1, 2, 2],
            BTreeMap::new(),
            &mut |_| Ok(None),
            &diagnostics,
        )?;
        assert!(bank.member(0).unwrap().events.is_empty());
        assert!(bank.member(1).is_none());
        assert!(bank.member(2).is_some());
        assert_eq!(diagnostics.entries().len(), 1);
        assert!(
            prepare(
                &source,
                77,
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
