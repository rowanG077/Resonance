//! Hurricane Thrust adds a detached ground burst after its opening strike connects.
use super::{PreparedAttack, Sound, Technique};
use anyhow::{Context, Result};
use resonance_battle::{AttackEvent, EffectAppearance, HitShape};
use std::sync::Arc;

impl Technique {
    pub(super) fn prepare_hurricane_thrust<
        S: FnMut(Sound) -> Result<Option<resonance_battle::Sound>>,
    >(
        self,
        resources: &mut crate::battle::encounter::resources::Resources<'_, S>,
        model: Option<&resonance_battle::ModelDefinition>,
    ) -> Result<PreparedAttack> {
        let mut timeline = self.authored_timeline(resources.voices.party_table())?;
        let hit = timeline
            .hurricane_hit
            .take()
            .context("missing Hurricane Thrust burst hit")?;
        let recoil = resources.recoil;
        const BURST_EFFECT: u16 = 107;
        const BURST_TICK: u16 = 29;
        resources.effect(true, &[BURST_EFFECT])?;
        let mut definition = self.prepare_authored_attack(resources, model, timeline)?;
        definition.events.push((
            BURST_TICK,
            AttackEvent::ContactBurst {
                definition: Arc::new(super::stationary_contact(
                    &hit,
                    recoil,
                    2,
                    HitShape::Cylinder,
                    [100., 50.],
                )?),
                appearance: Some(EffectAppearance {
                    resource: resources.techniques.resource,
                    member: BURST_EFFECT,
                }),
            },
        ));
        definition.events.sort_by_key(|(at, _)| *at);
        Ok(definition)
    }
}
