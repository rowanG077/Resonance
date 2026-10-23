//! Seal follow-ups retain the character's authored pose and contact sequence.
use super::{PreparedAttack, Technique};
use crate::battle::encounter::resources::Resources;
use anyhow::{Context, Result, bail};
use resonance_battle::{
    AttackEvent, EffectAppearance, RecoveryKind, SealDefinition, conditions::Condition,
};
use std::sync::Arc;

// The prepared timeline includes the opening pose's four blend updates.
const FOLLOW_UP_TICK: u16 = 19;
const PINION_RELEASE_TICK: u16 = 44;
const PINION_PROJECTILE: usize = 12;
const STAT_REDUCTION_DURATION: u16 = 900;

impl Resources<'_> {
    pub(crate) fn seal(&mut self, catalogue: u16) -> Result<SealDefinition> {
        let (condition, effect) = match catalogue {
            126..=128 => (Condition::DefenseDown, 134),
            129..=131 => (Condition::EvasionDown, 59),
            132..=134 => (Condition::AccuracyDown, 135),
            _ => bail!("unsupported stat-reduction seal"),
        };
        self.effect(true, &[effect])?;
        let [red, green, blue, _] = self
            .tints
            .context("missing seal feedback colors")?
            .actors
            .debuff;
        Ok(SealDefinition {
            condition,
            duration: STAT_REDUCTION_DURATION,
            effect: EffectAppearance {
                resource: self.techniques.resource,
                member: effect,
            },
            tint: [red, green, blue],
            guaranteed: matches!(catalogue, 128 | 131 | 134),
        })
    }
}

impl Technique {
    pub(super) fn prepare_seal(
        self,
        resources: &mut Resources<'_>,
        model: Option<&resonance_battle::ModelDefinition>,
    ) -> Result<PreparedAttack> {
        let timeline = self.authored_timeline(resources.voices.party_table())?;
        let mut definition = self.prepare_authored_attack(resources, model, timeline.clone())?;
        match self.catalogue {
            125 => {}
            126..=134 => {
                let seal = resources.seal(self.catalogue)?;
                definition
                    .events
                    .push((FOLLOW_UP_TICK, AttackEvent::Seal(seal)));
                if matches!(self.catalogue, 127 | 130 | 133) {
                    let hit = timeline
                        .callback_hits
                        .get(&1)
                        .context("missing Pinion Seal projectile hit")?;
                    let recoil = resources.recoil;
                    let row = resources
                        .projectiles
                        .records
                        .get(PINION_PROJECTILE)
                        .context("missing Pinion Seal projectile")?;
                    let mut projectile = resources.prepared.projectile(
                        resources.files,
                        row,
                        crate::battle::hit::authored(hit, recoil)?,
                        &[(1, &resources.techniques)],
                    )?;
                    let prepared = Arc::make_mut(&mut projectile);
                    prepared.effects.seal = Some(seal);
                    prepared
                        .contact
                        .as_mut()
                        .context("Pinion Seal projectile has no contact")?
                        .cooldown = hit.cooldown;
                    definition
                        .events
                        .push((PINION_RELEASE_TICK, AttackEvent::Projectile(projectile)));
                }
            }
            135 | 136 => {
                definition.events.push((
                    FOLLOW_UP_TICK,
                    AttackEvent::Drain {
                        kind: if self.catalogue == 135 {
                            RecoveryKind::Hp
                        } else {
                            RecoveryKind::Tp
                        },
                        percent: if self.catalogue == 135 { 10 } else { 3 },
                    },
                ));
            }
            _ => bail!("unsupported seal follow-up"),
        }
        definition.events.sort_by_key(|(at, _)| *at);
        Ok(definition)
    }
}
