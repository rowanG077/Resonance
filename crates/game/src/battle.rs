//! Battle sequence preparation. Filesystem integrity and rendering resource
//! preparation belong to the caller, before this batch can be activated.
mod advanced_ground_spells;
pub mod ai;
mod aqua_laser;
pub mod audio;
mod colette_spells;
pub mod command;
pub mod companion;
mod conditions;
pub mod contact_audio;
pub mod contact_feedback;
pub mod control;
mod corrine;
pub mod death;
mod defense_system;
pub mod effect_program;
mod elemental_spells;
mod enchant_seals;
pub mod encounter;
pub mod enemy;
mod enemy_stage;
pub mod entry;
pub mod escape;
pub mod event;
pub mod feedback;
pub mod fire_ball;
pub mod first_aid;
mod ground_pulse;
mod hit;
pub mod items;
mod lance;
pub mod learning;
pub mod lifecycle;
pub mod martial;
pub mod model;
pub mod normal;
pub mod nurse;
pub mod party;
mod pattern_spells;
mod photon;
mod prism;
pub mod profile;
mod projectile;
pub mod recoil;
pub mod resident_spells;
pub mod results;
pub mod rewards;
mod skill_scene;
pub mod stage;
mod stored_recovery;
pub mod stored_spells;
pub mod summons;
mod support_spells;
pub mod trail;
pub(crate) mod unison;
pub mod victory;
pub mod voice;
pub mod weapon;
use anyhow::{Context, Result, ensure};
pub use resonance_battle::{ActionDefinition, ActionDefinitions, ActionExecution, ActionKey};
use std::sync::Arc;

/// Asset members needed by the encounter and their ready presentation
/// resource generation.
#[derive(Debug, Clone)]
pub struct EffectResource {
    pub bank: Option<Arc<resonance_content::battle_effect::SourceBank>>,
    pub resource: resonance_battle::ResourceId,
    pub members: Vec<u16>,
    /// Verified model templates. Each particle owns its playback.
    pub models: std::collections::BTreeMap<u8, resonance_battle::PreparedEffectModel>,
}

/// Effect banks collected while resolving native action resources.
#[derive(Default)]
pub struct ActionResources {
    pub effects: std::collections::BTreeMap<resonance_battle::ResourceId, EffectResource>,
}

impl ActionResources {
    pub fn effect(&mut self, mut request: EffectResource) -> Result<()> {
        // Particles can emit another program from the same bank. Collect that
        // closure before either rendering or simulation selects its dependencies.
        // Program and particle validation still belongs to preparation below.
        if let Some(bank) = &request.bank {
            let mut next = 0;
            while let Some(&member) = request.members.get(next) {
                next += 1;
                for event in bank.programs.get(usize::from(member)).into_iter().flatten() {
                    if let resonance_content::battle_effect::EffectOperation::Spawn {
                        particle, ..
                    } = event.operation
                        && let Some(template) = bank
                            .actors
                            .get(usize::from(particle))
                            .and_then(|actor| actor.template().ok())
                    {
                        for child in [
                            template.bounce_effect,
                            template.floor_effect,
                            template.periodic_effect.map(|effect| effect.member),
                        ]
                        .into_iter()
                        .flatten()
                        {
                            if !request.members.contains(&child) {
                                request.members.push(child);
                            }
                        }
                    }
                }
            }
        }
        request.members.sort_unstable();
        request.members.dedup();
        if let Some(previous) = self.effects.get_mut(&request.resource) {
            ensure!(
                previous
                    .models
                    .iter()
                    .map(|(slot, model)| (slot, model.resource()))
                    .eq(request
                        .models
                        .iter()
                        .map(|(slot, model)| (slot, model.resource()))),
                "inconsistent effect resource {}",
                request.resource
            );
            previous.members.append(&mut request.members);
            previous.members.sort_unstable();
            previous.members.dedup();
        } else {
            self.effects.insert(request.resource, request);
        }
        Ok(())
    }

    /// Register one effect and return the matching runtime appearance.
    pub fn effect_member(
        &mut self,
        bank: &EffectResource,
        member: u16,
    ) -> Result<resonance_battle::EffectAppearance> {
        let mut request = bank.clone();
        request.members = vec![member];
        self.effect(request)?;
        Ok(resonance_battle::EffectAppearance {
            resource: bank.resource,
            member,
        })
    }

    pub fn prepare_effects(
        &self,
        sound: &mut impl FnMut(u16) -> Result<Option<resonance_battle::Sound>>,
        diagnostics: &resonance_content::diagnostics::Diagnostics,
    ) -> Result<Vec<resonance_battle::EffectBank>> {
        self.effects
            .values()
            .filter_map(|request| request.bank.as_ref().map(|bank| (request, bank)))
            .map(|(request, source)| {
                effect_program::prepare(
                    source,
                    request.resource,
                    &request.members,
                    request.models.clone(),
                    sound,
                    diagnostics,
                )
                .with_context(|| format!("prepare effect bank {}", request.resource))
            })
            .collect()
    }
}
