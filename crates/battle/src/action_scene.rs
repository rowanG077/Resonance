//! A prepared action reserves admissions using the existing action lifetime.
use crate::{ActionId, ActorId, Battle, Side};

#[derive(Debug, Clone, Copy)]
pub(crate) struct Reservation {
    pub action: ActionId,
    pub owner: ActorId,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ActionKey, ActionRequest, ActorSetup, AttackEvent, BattleInput, Cue, PreparedBattle,
        ProtectionMode, Rejection, Sound,
    };
    use anyhow::Result;

    fn request(actor: u8, action: usize, target: u8) -> ActionRequest {
        ActionRequest {
            actor: ActorId(actor),
            action: ActionKey(action),
            target: ActorId(target),
        }
    }
    fn fixture() -> Result<Battle> {
        let mut scene = crate::tests::action(6);
        crate::tests::attack_mut(&mut scene).recovery = 3;
        crate::tests::attack_mut(&mut scene).events = vec![(
            0,
            AttackEvent::ActionScene {
                party_protection: 30,
            },
        )];
        let mut ordinary = crate::tests::action(3);
        ordinary.tp_cost = 5;
        crate::tests::attack_mut(&mut ordinary).events =
            vec![(2, AttackEvent::Sound(Some(Sound::Cue(77))))];
        let mut prepared = PreparedBattle::new(
            [Side::Party, Side::Party, Side::Enemy, Side::Enemy]
                .into_iter()
                .map(|side| (crate::tests::actor(side), ActorSetup::default()))
                .collect(),
            vec![scene, ordinary].into(),
            3,
        )?;
        for actor in 0..4 {
            for action in 0..2 {
                crate::tests::assign_action(&mut prepared, actor, ActionKey(action));
            }
        }
        prepared.finish()
    }

    #[test]
    fn action_visibility_restores_the_carried_weapon_after_interruption() -> Result<()> {
        use std::sync::Arc;
        let mut action = crate::tests::action(20);
        crate::tests::attack_mut(&mut action).events = vec![(
            0,
            AttackEvent::ActionWeaponVisibility(vec![(Some(crate::WeaponSlot(0)), false)]),
        )];
        let mut body = crate::tests::model_definition(7, [(0, 30.)]);
        body.weapons.push(Arc::new(crate::WeaponDefinition {
            slot: Some(crate::WeaponSlot(0)),
            initially_visible: true,
            attachment: Some(0),
            effect_interval: None,
            layers: vec![crate::WeaponLayerDefinition {
                resource: crate::ResourceId(8),
                skeleton: body.skeleton.clone(),
                motions: body.motions.clone(),
                playback: crate::WeaponPlayback::Rigid,
                secondary_motion: vec![],
                texture_animation: None,
            }],
            links: vec![],
        }));
        let mut battle = PreparedBattle::new(
            [Side::Party, Side::Enemy]
                .into_iter()
                .map(|side| (crate::tests::actor(side), ActorSetup::default()))
                .collect(),
            vec![action].into(),
            3,
        )?
        .finish()?;
        battle.set_models(crate::tests::models(
            battle.actors(),
            [Some(Arc::new(body)), None],
        )?);
        battle.start_admitted_action(request(0, 0, 1), &mut vec![])?;
        let mut frame = battle.step(BattleInput::default())?;
        battle.advance_models(&mut frame, crate::BattleClock::Running, false)?;
        assert_eq!(frame.weapons.len(), 1);
        assert!(!frame.weapons[0].visible);
        battle.interrupt_actor(ActorId(0), &mut vec![]);
        let mut frame = battle.publish(vec![]);
        battle.advance_models(&mut frame, crate::BattleClock::Running, false)?;
        assert!(frame.weapons[0].visible);
        Ok(())
    }

    #[test]
    fn concurrent_scene_admission_preserves_in_flight_work_and_releases_on_recovery() -> Result<()>
    {
        let mut battle = fixture()?;
        battle.start_admitted_action(request(1, 1, 2), &mut vec![])?;
        let peer_tp = battle.actors[2].tp;
        let frame = battle.step(BattleInput {
            actions: vec![request(0, 0, 2), request(2, 1, 0)],
            ..Default::default()
        })?;
        assert!(frame.cues.iter().any(|cue| matches!(
            cue,
            Cue::Rejected {
                actor: ActorId(2),
                reason: Rejection::Busy
            }
        )));
        assert_eq!(battle.actors[2].tp, peer_tp);
        assert!(!battle.command_admission_allowed());
        assert_eq!(
            battle.actors[1].reaction.protection.mode,
            ProtectionMode::Escape
        );
        let mut prior_sound = false;
        for _ in 0..8 {
            let frame = battle.step(BattleInput::default())?;
            prior_sound |= frame.cues.iter().any(|cue| {
                matches!(
                    cue,
                    Cue::Sound {
                        sound: Sound::Cue(77),
                        ..
                    }
                )
            });
        }
        assert!(prior_sound);
        assert!(battle.action_scene.is_none());
        assert!(battle.command_admission_allowed());
        Ok(())
    }

    #[test]
    fn failed_scene_admission_does_not_reserve_the_next_concurrent_request() -> Result<()> {
        let mut battle = fixture()?;
        battle.actors[0].availability = crate::ActorAvailability::Petrified;
        let frame = battle.step(BattleInput {
            actions: vec![request(0, 0, 2), request(1, 0, 2)],
            ..Default::default()
        })?;
        assert!(frame.cues.iter().any(|cue| matches!(
            cue,
            Cue::Rejected {
                actor: ActorId(0),
                reason: Rejection::Petrified
            }
        )));
        assert_eq!(battle.action_scene.unwrap().owner, ActorId(1));
        Ok(())
    }

    #[test]
    fn enemy_reservation_keeps_party_admissions_and_all_termination_paths_release() -> Result<()> {
        for termination in 0..3 {
            let mut battle = fixture()?;
            battle.start_admitted_action(request(2, 0, 0), &mut vec![])?;
            assert!(battle.action_scene_blocks(ActorId(3)));
            assert!(!battle.action_scene_blocks(ActorId(0)));
            assert!(battle.start_actor_command(request(0, 1, 2), &mut vec![])?);
            let scene = battle.action_scene.unwrap();
            match termination {
                0 => battle.interrupt_actor(ActorId(2), &mut vec![]),
                1 => {
                    battle.defeat_actor(ActorId(2), &mut vec![]);
                }
                _ => battle.set_task(2, crate::state::ActorTask::None),
            }
            assert!(battle.action_scene.is_none(), "termination{termination}");
            battle.start_admitted_action(request(3, 0, 0), &mut vec![])?;
            battle.end_action_scene(scene.action);
            assert_eq!(battle.action_scene.unwrap().owner, ActorId(3));
        }
        Ok(())
    }
}

impl Battle {
    pub(crate) fn action_scene_blocks(&self, actor: ActorId) -> bool {
        self.action_scene.is_some_and(|scene| {
            actor != scene.owner
                && (self.actors[scene.owner.index()].side == Side::Party
                    || self.actors[actor.index()].side == Side::Enemy)
        })
    }

    pub(crate) fn begin_action_scene(&mut self, action: ActionId, owner: ActorId, protection: u16) {
        self.action_scene = Some(Reservation { action, owner });
        self.target_selector = None;
        self.command_target = None;
        if self.actors[owner.index()].side != Side::Party {
            return;
        }
        for index in 0..self.actors.len() {
            let id = ActorId(index as u8);
            // Native scene entry drops queued party techniques, without
            // interrupting the action or spell that is already running.
            if self.actors[index].side == Side::Party {
                self.clear_technique_command(id);
            }
            if self.actors[index].side == Side::Party
                && self.actors[index].available()
                && !matches!(
                    self.activity(id),
                    crate::Activity::Hurt
                        | crate::Activity::KnockedDown
                        | crate::Activity::GettingUp
                )
            {
                self.actors[index]
                    .reaction
                    .protection
                    .protect_transition(u32::from(protection));
            }
        }
    }

    pub(crate) fn end_action_scene(&mut self, action: ActionId) {
        if self
            .action_scene
            .is_some_and(|scene| scene.action == action)
        {
            self.action_scene = None;
        }
    }
}
