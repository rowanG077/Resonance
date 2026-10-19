//! A prepared retreat consumes the preceding contact after follow-up selection.
use crate::{ActionExecution, ActionId, AttackPose, Battle, Cue, Sound, action::Sequence};
use anyhow::{Result, ensure};

#[derive(Debug, Clone, Copy)]
pub struct ContactRecovery {
    pub pose: Option<AttackPose>,
    pub movement: [Option<f32>; 2],
    pub braking: Option<f32>,
    pub pass_through_ticks: u8,
    pub voice: Option<Sound>,
    pub expressions: [u8; 4],
}

impl ContactRecovery {
    pub(crate) fn validate(self) -> Result<()> {
        ensure!(
            self.movement.into_iter().flatten().all(f32::is_finite)
                && self
                    .braking
                    .is_none_or(|value| value.is_finite() && value >= 0.),
            "invalid contact recovery movement"
        );
        Ok(())
    }
}

impl Sequence {
    pub(crate) fn contact_recovery_requested(&self) -> bool {
        self.contact_made
            && matches!(&self.definition.execution,
            ActionExecution::Attack(attack) if attack.contact_recovery.is_some())
    }
}

impl Battle {
    pub(crate) fn enter_contact_recovery(
        &mut self,
        id: ActionId,
        sequence: &mut Sequence,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        let ActionExecution::Attack(attack) = &sequence.definition.execution else {
            unreachable!("contact recovery belongs to an attack")
        };
        let recovery = attack.contact_recovery.unwrap();
        let remaining = u32::from(attack.recovery);
        sequence.recover(self, id, remaining, cues)?;
        sequence.combo = None;
        let actor = sequence.actor;
        let movement = &mut self.actors[actor.index()].movement;
        if let Some(value) = recovery.movement[0] {
            movement.forward = value;
        }
        if let Some(value) = recovery.movement[1] {
            movement.vertical = value;
        }
        if let Some(value) = recovery.braking {
            movement.braking = value;
        }
        self.runtime[actor.index()].body_bypass = recovery.pass_through_ticks != 0;
        self.runtime[actor.index()].body_bypass_ticks = recovery.pass_through_ticks;
        if let Some(pose) = recovery.pose {
            self.request_pose(
                actor,
                Some(pose.motion),
                crate::Pose {
                    frame: pose.start,
                    end: pose.end,
                    repeat: pose.repeat,
                    rate: pose.rate,
                    blend: pose.blend,
                    restart: false,
                    ..Default::default()
                },
            );
        }
        if let Some(sound) = recovery.voice {
            self.request_voice(actor, sound, crate::VoicePriority::Action);
        }
        self.restore_contact_recovery_expression(sequence);
        Ok(())
    }

    pub(crate) fn restore_contact_recovery_expression(&mut self, sequence: &Sequence) {
        if let ActionExecution::Attack(attack) = &sequence.definition.execution
            && let Some(recovery) = attack.contact_recovery
        {
            self.model_requests.push(crate::ModelRequest::Expression {
                actor: sequence.actor,
                layers: recovery.expressions,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ActionKey, ActionRequest, ActorId, BattleInput, Control, Side};
    use std::sync::Arc;

    #[test]
    fn contact_recovery_waits_for_hitstop_and_yields_to_followup() -> Result<()> {
        for (connects, guarded, paused, followup) in [
            (false, false, false, false),
            (true, false, false, false),
            (true, true, false, false),
            (true, false, true, false),
            (true, false, false, true),
        ] {
            let mut owner = crate::tests::actor(Side::Enemy);
            owner.movement.direction = owner.facing_direction;
            let mut target = crate::tests::actor(Side::Party);
            target.control = Control::Manual;
            target.position = [0., 0., if connects { 3. } else { 1000. }];
            target.heading = 180.;
            target.facing_direction = [0., 0., -1.];
            target.body.collider = Some(crate::Collider::sphere(1.));
            let mut prepared = crate::tests::prepared(vec![owner, target], 10);
            let attack =
                crate::tests::attack_mut(Arc::make_mut(&mut prepared.resources.actions.entries[0]));
            attack.recovery = 4;
            attack.chain_at = Some(1);
            attack.events = vec![(
                1,
                crate::AttackEvent::Move {
                    forward: Some(9.),
                    vertical: None,
                },
            )];
            attack.contact_recovery = Some(ContactRecovery {
                pose: None,
                movement: [Some(-4.), None],
                braking: Some(1.),
                pass_through_ticks: 4,
                voice: None,
                expressions: [0; 4],
            });
            if followup {
                let successor = prepared.resources.actions.insert(crate::tests::action(10));
                crate::tests::assign_action(&mut prepared, 0, successor);
                let choices = &mut prepared.resources.actor_setup[0]
                    .enemy_decision
                    .as_mut()
                    .unwrap()
                    .choices;
                choices[0].followup_chance = 100;
                choices[1].chain_group = 1;
                choices[1].requirements = Default::default();
            }
            let mut battle = prepared.finish()?;
            battle.step(BattleInput {
                actions: vec![ActionRequest {
                    actor: ActorId(0),
                    action: ActionKey(0),
                    target: ActorId(1),
                }],
                ..Default::default()
            })?;
            let id = *battle.runtime[0].task().action().unwrap().0;
            battle.actors[1].guard.active = guarded;
            battle.actors[1].guard.break_pressure = 31;
            let mut hit = crate::tests::contact_projectile(false, true)
                .contact
                .unwrap()
                .hit;
            hit.power = crate::Power::Fixed(5);
            let melee = crate::MeleeDefinition {
                hit,
                volume: crate::MeleeVolume::Fixed {
                    offset: [0.; 3],
                    radius: 5.,
                    half_height: 5.,
                },
                trail: None,
            };
            let mut contacts = crate::contact::Contacts::default();
            contacts.melee(ActorId(0), id, &melee, &[])?;
            let mut cues = Vec::new();
            contacts.resolve(&mut battle, &mut cues, &[])?;
            let result = cues.iter().find_map(|cue| match cue {
                Cue::Hit { result, .. } => Some(result),
                _ => None,
            });
            assert_eq!(result.is_some(), connects);
            if let Some(result) = result {
                assert_eq!(
                    matches!(result.guard, crate::GuardResult::Blocked { .. }),
                    guarded
                );
            }
            battle.actors[0].hit_stop = if paused { 2 } else { 0 };
            let before = battle.actors[0].position;
            if paused {
                for remaining in [1, 0] {
                    battle.step(BattleInput::default())?;
                    assert_eq!(battle.actors[0].hit_stop, remaining);
                    assert_eq!(battle.activity(ActorId(0)), crate::Activity::Action);
                    assert_eq!(battle.actors[0].position, before);
                    assert_eq!(battle.runtime[0].body_bypass_ticks, 0);
                }
            }
            battle.step(BattleInput::default())?;
            if followup {
                assert_eq!(
                    battle.runtime[0].task().action().unwrap().1.action,
                    ActionKey(1)
                );
                assert_eq!(battle.runtime[0].body_bypass_ticks, 0);
            } else if connects {
                assert_eq!(battle.activity(ActorId(0)), crate::Activity::Recovering);
                assert_eq!(battle.actors[0].movement.forward, -3.);
                assert_eq!(battle.actors[0].position[2] - before[2], -4.);
                assert_eq!(battle.runtime[0].body_bypass_ticks, 3);
                assert!(battle.runtime[0].task().action().unwrap().1.combo.is_none());
            } else {
                assert_eq!(battle.activity(ActorId(0)), crate::Activity::Action);
                assert_eq!(battle.runtime[0].body_bypass_ticks, 0);
            }
        }
        Ok(())
    }
}
