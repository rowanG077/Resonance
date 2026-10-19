//! Hurricane Thrust adds a detached ground burst after its opening strike connects.
use super::{ActionDefinition, Sound, Technique};
use anyhow::{Context, Result};
use resonance_battle::{
    ActionExecution, AttackEvent, EffectAppearance, HitShape, ProjectileContact,
    ProjectileDefinition,
};
use std::sync::Arc;

impl Technique {
    pub(super) fn prepare_hurricane_thrust<
        S: FnMut(Sound) -> Result<Option<resonance_battle::Sound>>,
    >(
        self,
        resources: &mut crate::battle::encounter::resources::Resources<'_, S>,
        model: Option<&resonance_battle::ModelDefinition>,
        tp_cost: u16,
    ) -> Result<ActionDefinition> {
        let mut timeline = resources
            .voices
            .party_table()
            .martial_attack(self.catalogue, self.character as u8)?
            .clone();
        let hit = timeline
            .hurricane_hit
            .take()
            .context("missing Hurricane Thrust burst hit")?;
        let recoil = crate::battle::recoil::Parameters::load(resources.files)?;
        const BURST_EFFECT: u16 = 107;
        const BURST_TICK: u16 = 29;
        resources.effect(true, &[BURST_EFFECT])?;
        let mut definition = self.prepare_authored_attack(resources, model, tp_cost, timeline)?;
        let ActionExecution::Attack(attack) = &mut definition.execution else {
            unreachable!()
        };
        attack.events.push((
            BURST_TICK,
            AttackEvent::ContactBurst {
                definition: Arc::new(ProjectileDefinition {
                    lifetime: Some(2),
                    velocity: [0.; 3],
                    acceleration: [0.; 3],
                    offset: [0.; 3],
                    clamp_ground: false,
                    active: None,
                    birth: None,
                    motion: Default::default(),
                    effects: Default::default(),
                    contact: Some(ProjectileContact {
                        hit: crate::battle::hit::authored(&hit, &recoil)?,
                        cooldown: hit.cooldown,
                        repeat_limit: 0,
                        radius: 100.,
                        height: 50.,
                        shape: HitShape::Cylinder,
                        offset: [0.; 3],
                        radius_growth: 0.,
                        height_growth: 0.,
                        survives_contact: true,
                        clashes: false,
                    }),
                }),
                appearance: Some(EffectAppearance {
                    resource: resources.techniques.resource,
                    member: BURST_EFFECT,
                }),
            },
        ));
        attack.events.sort_by_key(|(at, _)| *at);
        Ok(definition)
    }
}
