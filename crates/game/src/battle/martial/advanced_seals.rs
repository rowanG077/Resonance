//! Cyclone lifts its target, then strikes again while the target is airborne.
use super::{ActionDefinition, Sound, Technique};
use anyhow::{Context, Result};
use resonance_battle::{
    ActionExecution, AttackEvent, AttackOrigin, EffectAppearance, HitShape, ProjectileContact,
    ProjectileDefinition,
};
use std::sync::Arc;

impl Technique {
    pub(super) fn prepare_force_seal<S: FnMut(Sound) -> Result<Option<resonance_battle::Sound>>>(
        self,
        resources: &mut crate::battle::encounter::resources::Resources<'_, S>,
        model: Option<&resonance_battle::ModelDefinition>,
        tp_cost: u16,
    ) -> Result<ActionDefinition> {
        const TALISMANS: u16 = 85;
        resources.effect(true, &[TALISMANS])?;
        let mut definition = self.prepare_authored(resources, model, tp_cost)?;
        let [red, green, blue, _] = resources
            .tints
            .context("missing Force Seal feedback colors")?
            .actors
            .debuff;
        let ActionExecution::Attack(attack) = &mut definition.execution else {
            unreachable!()
        };
        for origin in [AttackOrigin::ActorRoot, AttackOrigin::TargetRoot] {
            attack.events.push((
                0,
                AttackEvent::InitializedEffect {
                    appearance: EffectAppearance {
                        resource: resources.techniques.resource,
                        member: TALISMANS,
                    },
                    origin,
                },
            ));
        }
        // Sixty action updates follow the opening pose's four blend updates.
        attack.events.push((
            64,
            AttackEvent::RecoveryDown {
                color: [red, green, blue],
            },
        ));
        attack.events.sort_by_key(|(at, _)| *at);
        Ok(definition)
    }

    pub(super) fn prepare_cyclone_seal<
        S: FnMut(Sound) -> Result<Option<resonance_battle::Sound>>,
    >(
        self,
        resources: &mut crate::battle::encounter::resources::Resources<'_, S>,
        model: Option<&resonance_battle::ModelDefinition>,
        tp_cost: u16,
    ) -> Result<ActionDefinition> {
        let timeline = resources
            .voices
            .party_table()
            .martial_attack(self.catalogue, self.character as u8)?
            .clone();
        let hits = timeline.callback_hits.clone();
        const MARKER: u16 = 115;
        const CONTACT_EFFECTS: [u16; 2] = [116, 117];
        resources.effect(true, &[MARKER, CONTACT_EFFECTS[0], CONTACT_EFFECTS[1]])?;
        let recoil = crate::battle::recoil::Parameters::load(resources.files)?;
        let mut definition = self.prepare_authored_attack(resources, model, tp_cost, timeline)?;
        let ActionExecution::Attack(attack) = &mut definition.execution else {
            unreachable!()
        };
        attack.events.push((
            0,
            AttackEvent::MarkCycloneTarget(EffectAppearance {
                resource: resources.techniques.resource,
                member: MARKER,
            }),
        ));
        const CONTACT_LIFETIME: u32 = 8;
        const CONTACT_SIZE: f32 = 100.;
        // The opening pose blends for four updates before the contact clock starts.
        for (index, at) in [49, 94].into_iter().enumerate() {
            let hit = hits
                .get(&(index as u8))
                .context("missing Cyclone Seal contact")?;
            attack.events.push((
                at,
                AttackEvent::CycloneContact {
                    definition: Arc::new(ProjectileDefinition {
                        lifetime: Some(CONTACT_LIFETIME),
                        velocity: [0.; 3],
                        acceleration: [0.; 3],
                        offset: [0.; 3],
                        clamp_ground: true,
                        active: None,
                        birth: None,
                        motion: Default::default(),
                        effects: Default::default(),
                        contact: Some(ProjectileContact {
                            hit: crate::battle::hit::authored(hit, &recoil)?,
                            cooldown: hit.cooldown,
                            repeat_limit: 0,
                            radius: CONTACT_SIZE,
                            height: CONTACT_SIZE,
                            shape: HitShape::Cylinder,
                            offset: [0.; 3],
                            radius_growth: 0.,
                            height_growth: 0.,
                            survives_contact: true,
                            clashes: false,
                        }),
                    }),
                    appearance: EffectAppearance {
                        resource: resources.techniques.resource,
                        member: CONTACT_EFFECTS[index],
                    },
                    opening: index == 0,
                },
            ));
        }
        attack.events.sort_by_key(|(at, _)| *at);
        Ok(definition)
    }
}
