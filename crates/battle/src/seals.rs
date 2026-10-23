//! Seal contacts share their chance, timed stat reduction, and target feedback.
use crate::{ActorId, Battle, ContactSource, Cue, EffectAppearance, conditions::Condition};
use anyhow::{Result, ensure};

const STAT_REDUCTION_PERCENT: i16 = -10;
const BASE_SUCCESS_PERCENT: u16 = 25;
const LUCK_PER_PERCENT: u16 = 10;

#[derive(Debug, Clone, Copy)]
pub struct SealDefinition {
    pub condition: Condition,
    pub duration: u16,
    pub effect: EffectAppearance,
    pub tint: [u8; 3],
    /// Absolute seals always pass the chance check after a successful strike.
    pub guaranteed: bool,
}

impl SealDefinition {
    pub(crate) fn validate(self) -> Result<()> {
        ensure!(
            matches!(
                self.condition,
                Condition::DefenseDown | Condition::EvasionDown | Condition::AccuracyDown
            ) && self.duration > 0,
            "invalid seal condition"
        );
        Ok(())
    }
}

impl Battle {
    pub(crate) fn apply_recovery_down(
        &mut self,
        target: ActorId,
        color: [u8; 3],
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        let actor = self.actor(target)?;
        if !actor.available() {
            return Ok(());
        }
        let actor = &mut self.actors[target.index()];
        if actor
            .conditions
            .apply_hit(crate::HitCondition {
                condition: Condition::RecoveryDown,
                chance: 100,
                value: 0,
            })
            .is_none()
        {
            return Ok(());
        }
        let position = actor.condition_label_origin();
        cues.push(Cue::DeferredFlash {
            actor: target,
            color,
            fade_first: false,
        });
        cues.push(Cue::CustomLabel {
            actor: target,
            position,
            left: "RECOVER".into(),
            right: "DOWN".into(),
            duration: 45,
        });
        Ok(())
    }

    pub(crate) fn advance_projectile_seal(
        &mut self,
        id: crate::ProjectileId,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        let projectile = self.projectiles.get_mut(&id).unwrap();
        let owner = projectile.frame.owner;
        if let Some((target, seal)) = projectile.take_seal() {
            let prolonged = self.actor(owner)?.equipment.contact.prolong;
            self.attempt_seal(
                owner,
                Some(target),
                seal,
                prolonged,
                ContactSource::Projectile(id),
                cues,
            )?;
        }
        Ok(())
    }

    /// The caller supplies the successful melee victim or a projectile's contact victim.
    /// Released Pinion projectiles call this independently of their caster's action.
    /// Returns whether the chance check passed; immunity can still reject the stat change.
    pub(crate) fn attempt_seal(
        &mut self,
        owner: ActorId,
        target: Option<ActorId>,
        seal: SealDefinition,
        prolonged: bool,
        source: ContactSource,
        cues: &mut Vec<Cue>,
    ) -> Result<bool> {
        let Some(target) = target else {
            return Ok(false);
        };
        self.actor(target)?;
        let chance =
            (BASE_SUCCESS_PERCENT + self.actor(owner)?.equipment.luck / LUCK_PER_PERCENT).min(100);
        if !seal.guaranteed {
            #[cfg(any(test, feature = "test-support"))]
            let observed = if let Some(inputs) = &mut self.random_inputs {
                let (action, contact) = match source {
                    ContactSource::Melee { action, .. } => {
                        (action, crate::random_inputs::Contact::Attached)
                    }
                    ContactSource::Projectile(id) => (
                        self.projectiles[&id].action,
                        crate::random_inputs::Contact::Projectile(id.0),
                    ),
                    ContactSource::Weapon { .. } => {
                        anyhow::bail!("seal input requested by a detached weapon")
                    }
                };
                inputs.seal(
                    crate::random_inputs::Key {
                        clock: self.ledger.elapsed_ticks,
                        owner: owner,
                        target: target,
                        action: action.0,
                        contact,
                    },
                    seal.condition,
                    chance,
                )?
            } else {
                None
            };
            #[cfg(any(test, feature = "test-support"))]
            let roll = observed.unwrap_or_else(|| self.random.next_u16() % 100);
            #[cfg(not(any(test, feature = "test-support")))]
            let roll = self.random.next_u16() % 100;
            if roll >= chance {
                return Ok(false);
            }
        }
        let duration = u32::from(seal.duration);
        let duration = if prolonged {
            duration + duration / 2
        } else {
            duration
        };
        let recipient = &mut self.actors[target.index()];
        recipient.conditions.apply_stat_condition(
            seal.condition,
            STAT_REDUCTION_PERCENT,
            duration,
            false,
        );
        let origin = recipient.effect_origin();
        let label_position = recipient.condition_label_origin();
        let effect = crate::EffectRequest {
            owner,
            target,
            appearance: seal.effect,
            origin,
            heading: 0.,
            scale: 1.,
            follow: Some(crate::EffectFollow::Center(target)),
            tint: Default::default(),
        };
        if matches!(source, ContactSource::Melee { .. }) {
            cues.push(Cue::InitializedEffect(effect));
            cues.push(Cue::DeferredFlash {
                actor: target,
                color: seal.tint,
                fade_first: false,
            });
        } else {
            cues.push(Cue::Effect(effect));
            self.model_requests.push(crate::ModelRequest::Tint {
                actor: target,
                color: seal.tint,
            });
        }
        cues.push(Cue::ConditionLabel {
            actor: target,
            kind: crate::conditions::ConditionLabel::StatusDown,
            position: label_position,
        });
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ActionId, Side,
        random_inputs::{Contact, Key, RecordedInputs, Seal},
    };

    #[test]
    fn observed_seal_attempt_applies_only_its_hit_recipient_and_consumes_one_draw() {
        for (percentile, applied) in [(24, true), (25, false)] {
            let mut owner = crate::tests::actor(Side::Party);
            owner.equipment.luck = 0;
            let mut battle =
                crate::tests::prepared(vec![owner, crate::tests::actor(Side::Enemy)], 120)
                    .finish()
                    .unwrap();
            let action = ActionId(battle.next_action);
            battle
                .install_random_inputs(
                    10,
                    RecordedInputs {
                        seals: Some(vec![Seal {
                            key: Key {
                                clock: 2,
                                owner: ActorId(0),
                                target: ActorId(1),
                                action: 1,
                                contact: Contact::Attached,
                            },
                            condition: Condition::DefenseDown,
                            chance_percent: 25,
                            percentile,
                        }]),
                        ..Default::default()
                    },
                )
                .unwrap();
            battle.ledger.elapsed_ticks = 10;
            battle
                .verify_random_input_admission(ActorId(0), action)
                .unwrap();
            battle.ledger.elapsed_ticks = 12;
            let seal = SealDefinition {
                condition: Condition::DefenseDown,
                duration: 900,
                effect: EffectAppearance {
                    resource: crate::ResourceId(0),
                    member: 1,
                },
                tint: [255, 128, 128],
                guaranteed: false,
            };
            let source = ContactSource::Melee {
                actor: ActorId(0),
                action,
            };
            let mut cues = Vec::new();
            assert!(
                !battle
                    .attempt_seal(ActorId(0), None, seal, false, source, &mut cues)
                    .unwrap()
            );
            assert!(battle.finish_random_inputs().is_err());
            assert_eq!(
                battle
                    .attempt_seal(ActorId(0), Some(ActorId(1)), seal, false, source, &mut cues)
                    .unwrap(),
                applied
            );
            assert_eq!(
                battle.actors[1]
                    .conditions
                    .effective()
                    .contains(Condition::DefenseDown),
                applied
            );
            assert!(
                !battle.actors[0]
                    .conditions
                    .effective()
                    .contains(Condition::DefenseDown)
            );
            assert_eq!(
                cues.iter()
                    .filter(|cue| matches!(
                        cue,
                        Cue::ConditionLabel {
                            actor: ActorId(1),
                            ..
                        }
                    ))
                    .count(),
                usize::from(applied)
            );
            battle.finish_random_inputs().unwrap();
            assert!(
                battle
                    .attempt_seal(ActorId(0), Some(ActorId(1)), seal, false, source, &mut cues)
                    .is_err()
            );
        }
    }
}
