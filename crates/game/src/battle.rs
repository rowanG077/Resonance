//! Battle sequence preparation. Filesystem integrity and rendering resource
//! preparation belong to the caller, before this batch can be activated.
pub mod ai;
pub mod audio;
pub mod command;
pub mod companion;
mod conditions;
pub mod contact_audio;
pub mod contact_feedback;
pub mod control;
pub mod death;
pub mod effect_program;
pub mod encounter;
pub mod enemy;
pub mod entry;
pub mod escape;
pub mod feedback;
pub mod fire_ball;
mod hit;
pub mod items;
pub mod learning;
pub mod lifecycle;
pub mod martial;
pub mod model;
pub mod normal;
pub mod party;
pub mod profile;
mod projectile;
pub mod recoil;
pub mod results;
pub mod rewards;
pub mod stage;
pub mod trail;
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
    pub resource: u32,
    pub members: Vec<u16>,
    /// Verified model templates. Each particle owns its playback.
    pub models: std::collections::BTreeMap<u8, resonance_battle::PreparedEffectModel>,
}

/// Effect banks collected while resolving native action resources.
#[derive(Default)]
pub struct ActionResources {
    pub effects: std::collections::BTreeMap<u32, EffectResource>,
}

impl ActionResources {
    pub fn effect(&mut self, request: EffectResource) -> Result<()> {
        require_effect(&mut self.effects, request)
    }
}

impl ActionResources {
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

fn require_effect(
    effects: &mut std::collections::BTreeMap<u32, EffectResource>,
    mut request: EffectResource,
) -> Result<()> {
    if let Some(previous) = effects.get_mut(&request.resource) {
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
        effects.insert(request.resource, request);
    }
    Ok(())
}
