//! Cyclone lifts its target, then strikes again while the target is airborne.
use super::{PreparedAttack, Technique};
use crate::battle::encounter::resources::Resources;
use anyhow::{Context, Result};
use resonance_battle::{
    AttackEvent, AttackOrigin, EffectAppearance, HitShape, ProjectileDefinition,
};
use std::sync::Arc;

impl Technique {
    pub(super) fn prepare_force_seal(
        self,
        resources: &mut Resources<'_>,
        model: Option<&resonance_battle::ModelDefinition>,
    ) -> Result<PreparedAttack> {
        const TALISMANS: u16 = 85;
        resources.effect(true, &[TALISMANS])?;
        let mut definition = self.prepare_authored(resources, model)?;
        let [red, green, blue, _] = resources
            .tints
            .context("missing Force Seal feedback colors")?
            .actors
            .debuff;
        for origin in [AttackOrigin::ActorRoot, AttackOrigin::TargetRoot] {
            definition.events.push((
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
        definition.events.push((
            64,
            AttackEvent::RecoveryDown {
                color: [red, green, blue],
            },
        ));
        definition.events.sort_by_key(|(at, _)| *at);
        Ok(definition)
    }

    pub(super) fn prepare_cyclone_seal(
        self,
        resources: &mut Resources<'_>,
        model: Option<&resonance_battle::ModelDefinition>,
    ) -> Result<PreparedAttack> {
        let timeline = self.authored_timeline(resources.voices.party_table())?;
        let hits = timeline.callback_hits.clone();
        const MARKER: u16 = 115;
        const CONTACT_EFFECTS: [u16; 2] = [116, 117];
        resources.effect(true, &[MARKER, CONTACT_EFFECTS[0], CONTACT_EFFECTS[1]])?;
        let recoil = resources.recoil;
        let mut definition = self.prepare_authored_attack(resources, model, timeline)?;
        definition.events.push((
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
            definition.events.push((
                at,
                AttackEvent::CycloneContact {
                    definition: Arc::new(ProjectileDefinition {
                        clamp_ground: true,
                        ..super::stationary_contact(
                            hit,
                            recoil,
                            CONTACT_LIFETIME,
                            HitShape::Cylinder,
                            [CONTACT_SIZE; 2],
                        )?
                    }),
                    appearance: EffectAppearance {
                        resource: resources.techniques.resource,
                        member: CONTACT_EFFECTS[index],
                    },
                    opening: index == 0,
                },
            ));
        }
        definition.events.sort_by_key(|(at, _)| *at);
        Ok(definition)
    }
}
