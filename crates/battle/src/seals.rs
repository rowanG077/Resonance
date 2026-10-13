//! Seal contacts share their chance, timed stat reduction, and target feedback.
use crate::{ActorId, Battle, Cue, EffectAppearance, conditions::Condition};
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
        let position =
            std::array::from_fn(|i| actor.position[i] + 2. * actor.body.center_offset[i]);
        self.model_requests.push(crate::ModelRequest::Tint {
            actor: target,
            color,
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
            self.attempt_seal(owner, Some(target), seal, prolonged, cues)?;
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
        cues: &mut Vec<Cue>,
    ) -> Result<bool> {
        let Some(target) = target else {
            return Ok(false);
        };
        self.actor(target)?;
        let chance =
            (BASE_SUCCESS_PERCENT + self.actor(owner)?.equipment.luck / LUCK_PER_PERCENT).min(100);
        if !seal.guaranteed && self.random.next_u16() % 100 >= chance {
            return Ok(false);
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
        let label_position =
            std::array::from_fn(|i| recipient.position[i] + 2. * recipient.body.center_offset[i]);
        cues.push(Cue::Effect(crate::EffectRequest {
            owner,
            target,
            appearance: seal.effect,
            origin,
            heading: 0.,
            scale: 1.,
            follow: Some(crate::EffectFollow::Center(target)),
            tint: Default::default(),
        }));
        self.model_requests.push(crate::ModelRequest::Tint {
            actor: target,
            color: seal.tint,
        });
        cues.push(Cue::ConditionLabel {
            actor: target,
            kind: crate::conditions::ConditionLabel::StatusDown,
            position: label_position,
        });
        Ok(true)
    }
}
