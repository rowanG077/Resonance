use super::*;
use resonance_content::battle_effect::ParticleBirth;
use std::collections::BTreeSet;

/// Observed random inputs and allocation identity for one authored emission.
pub struct FixtureEffectBirth {
    pub owner: ActorId,
    pub appearance: EffectAppearance,
    pub spawn: usize,
    pub particle: u8,
    pub first_draw: usize,
    pub birth: ParticleBirth,
    pub samples: Vec<f32>,
    pub model_ordinal: Option<usize>,
    pub heading: Option<f32>,
}

#[derive(Default)]
pub(super) struct Births {
    scopes: BTreeSet<(ActorId, EffectAppearance, u8)>,
    pending: Vec<FixtureEffectBirth>,
    pub(super) identities: BTreeMap<ParticleId, usize>,
}

impl Births {
    pub(super) fn take(
        &mut self,
        active: &Active,
        particle: u8,
        birth: &ParticleBirth,
    ) -> Result<Option<FixtureEffectBirth>> {
        let owner = active.actor;
        let appearance = active.appearance;
        let spawn = active.spawned.len();
        let first_draw = active.samples.len();
        let heading = active.heading;
        if !self.scopes.contains(&(owner, appearance, particle)) {
            return Ok(None);
        }
        let index = self
            .pending
            .iter()
            .position(|input| {
                input.owner == owner
                    && input.appearance == appearance
                    && input.spawn == spawn
                    && input.particle == particle
                    && input
                        .heading
                        .is_none_or(|value| (value - heading).abs() < 0.001)
            })
            .context("extra recorded effect birth")?;
        let input = self.pending.swap_remove(index);
        ensure!(
            input.first_draw == first_draw && &input.birth == birth,
            "recorded effect birth identity or authored distribution differs"
        );
        Ok(Some(input))
    }
}

impl Effects {
    pub fn observe_fixture_births(
        &mut self,
        scopes: &[(ActorId, EffectAppearance, u8)],
    ) -> Result<()> {
        ensure!(
            self.recorded_births.scopes.is_empty(),
            "effect inputs already installed"
        );
        for &(owner, appearance, particle) in scopes {
            ensure!(
                self.banks
                    .get(&appearance.resource)
                    .and_then(|bank| bank.member(appearance.member))
                    .is_some(),
                "recorded effect program is not prepared"
            );
            self.recorded_births
                .scopes
                .insert((owner, appearance, particle));
        }
        Ok(())
    }

    pub fn fixture_birth_inputs(&mut self, inputs: Vec<FixtureEffectBirth>) -> Result<()> {
        self.finish_fixture_birth_inputs()?;
        for input in &inputs {
            ensure!(
                input.model_ordinal.is_none_or(|ordinal| ordinal > 0)
                    && input.heading.is_none_or(f32::is_finite)
                    && input.samples.len() == input.birth.random_draws as usize
                    && input
                        .samples
                        .iter()
                        .all(|value| value.is_finite() && (0. ..1.).contains(value)),
                "invalid recorded effect samples"
            );
        }
        self.recorded_births.pending = inputs;
        Ok(())
    }

    pub fn finish_fixture_birth_inputs(&self) -> Result<()> {
        ensure!(
            self.recorded_births.pending.is_empty(),
            "unconsumed recorded effect births"
        );
        Ok(())
    }

    pub fn fixture_model_ordinal(&self, particle: ParticleId) -> Option<usize> {
        self.recorded_births.identities.get(&particle).copied()
    }
}
