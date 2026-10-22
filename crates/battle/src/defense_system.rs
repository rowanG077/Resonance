//! A mechanical encounter shares deployment, attachments and core defeat.
use crate::{
    ActorId, Battle, Cue, MeleeAnchor, ModelRequest, MotionBinding, Pose, PreparedBattle, Side,
};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeSet;

const DEPLOYMENT_HP_PERCENT: i16 = 66;

#[derive(Debug, Clone, Copy)]
pub struct DefenseArm {
    pub actor: ActorId,
    pub anchor: MeleeAnchor,
}

#[derive(Debug, Clone)]
pub struct DefenseSystemDefinition {
    pub core: ActorId,
    /// Repair unit and its fixed arena position.
    pub repair: (ActorId, [f32; 3]),
    pub components: Vec<ActorId>,
    pub arms: [DefenseArm; 4],
    pub deployment: MotionBinding,
    pub deployment_ticks: u16,
    pub deployment_pose: Pose,
}

#[derive(Debug, Default)]
pub(crate) struct State {
    deployment_age: Option<u16>,
    cleaned_up: bool,
}

impl PreparedBattle {
    pub fn with_defense_system(mut self, definition: DefenseSystemDefinition) -> Result<Self> {
        ensure!(
            self.resources.defense_system.is_none(),
            "duplicate Defense System group"
        );
        ensure!(
            definition.deployment_ticks > 0
                && definition.deployment_pose.rate.is_finite()
                && definition.deployment_pose.rate > 0.,
            "empty Defense System deployment"
        );
        let mut actors = BTreeSet::new();
        for id in std::iter::once(definition.core).chain(definition.components.iter().copied()) {
            ensure!(actors.insert(id), "duplicate Defense System component");
            let actor = self
                .actors
                .get(id.index())
                .context("missing Defense System actor")?;
            ensure!(
                actor.side == Side::Enemy,
                "Defense System component is not an enemy"
            );
        }
        let mut arms = BTreeSet::new();
        for arm in &definition.arms {
            ensure!(
                arms.insert(arm.actor) && definition.components.contains(&arm.actor),
                "invalid Defense System arm membership"
            );
            ensure!(
                arm.anchor.resource == definition.deployment.model,
                "Defense System arm belongs to another body"
            );
        }
        let (repair, position) = definition.repair;
        ensure!(
            definition.components.contains(&repair)
                && !arms.contains(&repair)
                && position.iter().all(|value| value.is_finite()),
            "invalid Defense System repair anchor"
        );
        self.actors[definition.core.index()]
            .movement
            .turning_disabled = true;
        let heading = self.actors[definition.core.index()].heading;
        for arm in &definition.arms {
            self.resources.actor_setup[arm.actor.index()].target_excluded = true;
            let actor = &mut self.actors[arm.actor.index()];
            actor.heading = heading;
            actor.movement.fixed_height = true;
            if let Some(recovery) =
                &mut self.resources.actor_setup[arm.actor.index()].recovery_return
            {
                recovery.disabled = true;
            }
        }
        for (actor, target) in self.actors.iter().zip(&mut self.targets) {
            if actor.side == Side::Party && arms.contains(target) {
                *target = definition.core;
            }
        }
        if let Some(leader) = self
            .actors
            .iter()
            .position(|actor| actor.side == Side::Party)
        {
            self.targets[leader] = definition.core;
            let target = self.actors[definition.core.index()].position;
            let actor = &mut self.actors[leader];
            let direction = crate::distance::normalize(std::array::from_fn(|axis| {
                target[axis] - actor.position[axis]
            }));
            actor.movement.direction = direction;
            crate::control::face(actor, direction, 0.);
        }
        self.resources.defense_system = Some(definition);
        Ok(self)
    }
}

impl Battle {
    pub(crate) fn defense_arm_waiting(&self, actor: ActorId) -> bool {
        let Some(definition) = &self.prepared.defense_system else {
            return false;
        };
        definition.arms.iter().any(|arm| arm.actor == actor)
            && self
                .defense_system
                .deployment_age
                .is_none_or(|age| age < definition.deployment_ticks)
    }

    /// Run once per ordinary update, before individual enemy decisions.
    pub(crate) fn advance_defense_system(&mut self, cues: &mut Vec<Cue>) -> Result<()> {
        self.advance_defense_system_on_clock(cues, self.phase() == crate::BattlePhase::Combat)
    }

    fn advance_defense_system_on_clock(
        &mut self,
        cues: &mut Vec<Cue>,
        in_combat: bool,
    ) -> Result<()> {
        let Some(definition) = &self.prepared.defense_system else {
            return Ok(());
        };
        if self.defense_system.cleaned_up {
            return Ok(());
        }
        if self.actors[definition.core.index()].availability == crate::ActorAvailability::Dead {
            for actor in definition.components.clone() {
                self.release_capture_links(actor);
                self.cancel_pending_item(actor);
                self.clear_technique_command(actor);
                self.interrupt_actor(actor, cues);
                let component = &mut self.actors[actor.index()];
                component.hp = 0;
                component.availability = crate::ActorAvailability::Dead;
                component.guard.active = false;
                component.time_stop = 0;
                component.hit_stop = 0;
            }
            self.defense_system.cleaned_up = true;
            return Ok(());
        }
        let core = &self.actors[definition.core.index()];
        if !core.available() || core.hit_stop != 0 || core.time_stop != 0 {
            return Ok(());
        }
        if in_combat && let Some(age) = &mut self.defense_system.deployment_age {
            *age = age.saturating_add(1).min(definition.deployment_ticks);
        } else if self.defense_system.deployment_age.is_none()
            && core.hp_percent() <= DEPLOYMENT_HP_PERCENT
        {
            self.model_requests.push(ModelRequest::Play {
                actor: definition.core,
                motion: definition.deployment,
                pose: definition.deployment_pose,
            });
            self.defense_system.deployment_age = Some(0);
        }
        Ok(())
    }

    pub(crate) fn defense_arm_positions(&self) -> Result<Option<[[f32; 3]; 4]>> {
        let Some(definition) = &self.prepared.defense_system else {
            return Ok(None);
        };
        if self.defense_system.cleaned_up || !self.actors[definition.core.index()].available() {
            return Ok(None);
        }
        let models = self
            .models
            .as_ref()
            .context("Defense System has no model playback")?;
        let mut positions = [[0.; 3]; 4];
        for (position, arm) in positions.iter_mut().zip(definition.arms) {
            if self.actors[arm.actor.index()].available() {
                *position = models.anchor(definition.core, arm.anchor)?;
            }
        }
        Ok(Some(positions))
    }

    /// Preserve fixed components and prior arm anchors after ordinary movement and pushing.
    pub(crate) fn attach_defense_arms(&mut self, positions: Option<[[f32; 3]; 4]>) {
        let Some(definition) = self.prepared.defense_system.as_ref() else {
            return;
        };
        let (repair, position) = definition.repair;
        let heading = self.actors[definition.core.index()].heading;
        let actor = &mut self.actors[repair.index()];
        actor.position = position;
        actor.heading = heading;
        let Some(positions) = positions else {
            return;
        };
        let core = &mut self.actors[definition.core.index()];
        core.position = core.movement.steering.home;
        core.movement.forward = 0.;
        core.movement.vertical = 0.;
        for (arm, position) in definition.arms.into_iter().zip(positions) {
            if self.actors[arm.actor.index()].available() {
                let actor = &mut self.actors[arm.actor.index()];
                actor.position = position;
                actor.movement.vertical = 0.;
            }
        }
    }
}

#[cfg(feature = "test-support")]
impl Battle {
    /// Restore the shared phase together with the saved body poses.
    pub fn restore_fixture_defense_deployment(&mut self) -> Result<()> {
        let definition = self
            .prepared
            .defense_system
            .as_ref()
            .context("fixture has no Defense System")?;
        ensure!(
            self.actors[definition.core.index()].available(),
            "fixture core is unavailable"
        );
        self.defense_system = State::default();
        Ok(())
    }

    /// Replay the encounter controller during the saved noncombat entry visits.
    pub fn advance_fixture_defense_deployment(
        &mut self,
        frame: &mut crate::BattleFrame,
    ) -> Result<()> {
        self.advance_defense_system_on_clock(&mut frame.cues, frame.combat_active)?;
        let positions = self.defense_arm_positions()?;
        self.attach_defense_arms(positions);
        for (frame_actor, actor) in frame.actors.iter_mut().zip(&self.actors) {
            frame_actor.state = actor.clone();
        }
        frame.model_requests.append(&mut self.model_requests);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_components_and_deployed_arms_survive_ordinary_movement() -> Result<()> {
        let core = ActorId(1);
        let mut participants: Vec<_> = std::iter::once(crate::tests::actor(Side::Party))
            .chain((0..6).map(|_| crate::tests::actor(Side::Enemy)))
            .map(|actor| (actor, Default::default()))
            .collect();
        participants[core.index()].0.position = [850., 0., 0.];
        participants[core.index()].0.heading = -90.;
        participants[core.index()].0.hp = 67;
        let anchor = MeleeAnchor {
            resource: 7,
            bone: 0,
        };
        let repair = ActorId(6);
        let repair_position = [200., 0., 0.];
        participants[repair.index()].0.position = repair_position;
        let definition = DefenseSystemDefinition {
            core,
            repair: (repair, repair_position),
            components: (2..7).map(ActorId).collect(),
            arms: std::array::from_fn(|index| DefenseArm {
                actor: ActorId(index as u8 + 2),
                anchor,
            }),
            deployment: MotionBinding { model: 7, clip: 32 },
            deployment_ticks: 120,
            deployment_pose: Pose {
                rate: 0.5,
                blend: 0,
                ..Default::default()
            },
        };
        let prepared = PreparedBattle::new(participants, Default::default(), 1)?
            .with_entry_choices(
                (0..7)
                    .map(|actor| crate::EntryChoice {
                        actor: ActorId(actor),
                        strategy: crate::TargetPolicy::Nearest,
                    })
                    .collect(),
            )?;
        assert_eq!(prepared.targets[0], ActorId(2));
        let seed = prepared.random_seed;
        let prepared = prepared.with_defense_system(definition)?;
        assert_eq!(prepared.targets[0], core);
        assert_eq!(prepared.random_seed, seed);
        #[cfg(feature = "test-support")]
        let prepared = {
            let mut prepared = prepared.configure_fixture(|actors| {
                actors[core.index()].0.position = [0.; 3];
                Ok(())
            })?;
            assert_eq!(prepared.actors[core.index()].position, [0.; 3]);
            assert_eq!(
                prepared.actors[core.index()].movement.steering.home,
                [850., 0., 0.]
            );
            prepared.actors[core.index()].position = [850., 0., 0.];
            prepared
        };
        let mut battle = prepared.with_arena_boundary().finish()?;
        assert_eq!(battle.target(ActorId(0)), Some(core));
        for policy in [crate::TargetPolicy::Nearest, crate::TargetPolicy::Spread] {
            assert!(matches!(
                battle.decision_target(ActorId(0), policy)?,
                Some(target) if target == core || target == repair
            ));
        }
        let mut model = crate::tests::model_definition(7, [(0, 30.), (32, 60.)]);
        model.skeleton.bones[0].bind.translation = [185., 0., 400.];
        // Deployment moves the anchor; attachments retain the completed prior pose.
        model
            .motions
            .get_mut(&32)
            .unwrap()
            .tracks
            .push(resonance_content::animation::Track {
                bone: 0,
                bind_channels: resonance_content::animation::TransformChannels(8),
                period_frames: 60.,
                times: vec![0., 60.],
                translation: Some(resonance_content::animation::VectorCurve {
                    interpolation: resonance_content::animation::VectorInterpolation::Linear,
                    values: vec![[185., 0., 400.], [245., 0., 400.]],
                    incoming: vec![],
                    outgoing: vec![],
                    ease: vec![],
                }),
                scale: None,
                rotation: None,
                euler_degrees: None,
                matrices: None,
            });
        battle.set_models(crate::Models::new(
            battle.actors(),
            vec![
                None,
                Some(std::sync::Arc::new(model)),
                None,
                None,
                None,
                None,
                None,
            ],
            Default::default(),
            resonance_content::diagnostics::Diagnostics::new(true),
        )?);
        for update in 0..123 {
            if update == 1 {
                battle.actors[core.index()].hp = 66;
            }
            battle.actors[core.index()].movement.direction = [1., 0., 0.];
            battle.actors[core.index()].movement.forward = 5.;
            for arm in &mut battle.actors[2..6] {
                arm.movement.direction = [1., 0., 0.];
                arm.movement.forward = 5.;
                arm.movement.vertical = -74.;
                arm.movement.gravity = -1.;
                arm.heading = 45.;
            }
            let unit = &mut battle.actors[repair.index()];
            unit.movement.direction = [1., 0., 0.];
            unit.movement.forward = 5.;
            unit.heading = 45.;
            let expected = battle.models().unwrap().anchor(core, anchor)?;
            let mut frame = battle.step(crate::BattleInput::default())?;
            assert_eq!(frame.actors[core.index()].position, [850., 0., 0.]);
            assert_eq!(frame.actors[core.index()].heading, -90.);
            assert_eq!(frame.actors[core.index()].movement.forward, 0.);
            assert_eq!(frame.actors[repair.index()].position, repair_position);
            assert_eq!(frame.actors[repair.index()].heading, -90.);
            for arm in &frame.actors[2..6] {
                assert_eq!(arm.position, expected);
                assert_eq!(arm.heading, 45.);
                assert_eq!(arm.movement.vertical, 0.);
            }
            battle.advance_models(&mut frame, crate::BattleClock::Running, true)?;
            assert_eq!(frame.models[0].clip, if update == 0 { 0 } else { 32 });
            assert_eq!(battle.defense_arm_waiting(ActorId(2)), update <= 120);
            for arm in (2..6).map(ActorId) {
                assert!(!battle.target_available(ActorId(0), arm));
            }
        }
        // Defeating the closest helper must not redirect the party to an arm.
        battle.runtime[0].set_target(repair);
        battle.actors[repair.index()].availability = crate::ActorAvailability::Dead;
        battle.retarget_after_defeat(repair)?;
        assert_eq!(battle.target(ActorId(0)), Some(core));
        assert_eq!(
            battle.decision_target(ActorId(0), crate::TargetPolicy::Spread)?,
            Some(core)
        );

        // A carried creature inherits its host's entire pose, then moves freely
        // after that host is hurt. Its own heading and formation position do not
        // offset the attachment.
        let child = ActorId(2);
        let mut participants: Vec<_> = [Side::Party, Side::Enemy, Side::Enemy]
            .into_iter()
            .map(|side| (crate::tests::actor(side), Default::default()))
            .collect();
        participants[0].0.position = [-600., 0., 0.];
        participants[1].0.position = [100., 0., 0.];
        participants[2].0.position = [500., 0., 300.];
        participants[2].0.heading = 65.;
        participants[2].0.body.scale = 2.;
        let mut battle = PreparedBattle::new(participants, Default::default(), 1)?
            .with_hosted_participants(vec![crate::HostedParticipant {
                actor: child,
                host: core,
                anchor,
                release: resonance_content::battle_profile::HostedBody {
                    release_on_hurt: true,
                    release_below_half_health: false,
                    release_on_defeat: false,
                    launch_on_release: true,
                },
                released_hurt: [Some(MotionBinding { model: 8, clip: 3 }); 2],
            }])?
            .finish()?;
        let mut host_model = crate::tests::model_definition(7, [(0, 30.)]);
        host_model.skeleton.bones[0].bind.translation = [50., 0., 100.];
        host_model
            .motions
            .get_mut(&0)
            .unwrap()
            .tracks
            .push(resonance_content::animation::Track {
                bone: 0,
                bind_channels: resonance_content::animation::TransformChannels(8),
                period_frames: 30.,
                times: vec![0., 30.],
                translation: Some(resonance_content::animation::VectorCurve {
                    interpolation: resonance_content::animation::VectorInterpolation::Linear,
                    values: vec![[50., 0., 100.], [50., 0., 130.]],
                    incoming: vec![],
                    outgoing: vec![],
                    ease: vec![],
                }),
                scale: None,
                rotation: None,
                euler_degrees: None,
                matrices: None,
            });
        battle.set_models(crate::Models::new(
            battle.actors(),
            vec![
                None,
                Some(std::sync::Arc::new(host_model)),
                Some(std::sync::Arc::new(crate::tests::model_definition(
                    8,
                    [(0, 30.), (3, 10.)],
                ))),
            ],
            Default::default(),
            resonance_content::diagnostics::Diagnostics::new(true),
        )?);
        for _ in 0..3 {
            assert!(!battle.target_available(ActorId(0), child));
            let prior_anchor = battle.models().unwrap().anchor(core, anchor)?;
            battle.actors[core.index()].position[0] += 7.;
            battle.actors[core.index()].heading += 15.;
            battle.actors[child.index()].movement.forward = 8.;
            let mut frame = battle.step(crate::BattleInput::default())?;
            let position = frame.actors[child.index()].position;
            assert_eq!(
                [position[0], position[2]],
                [prior_anchor[0], prior_anchor[2]]
            );
            assert!((position[1] - prior_anchor[1] - 0.5).abs() < 0.0001);
            battle.advance_models(&mut frame, crate::BattleClock::Running, true)?;
            let host = &frame.models[0];
            assert_eq!(
                frame.models[1].world,
                resonance_content::animation::multiply(host.world, host.bones[0])
            );
        }
        battle.begin_hurt(core, 20, &mut Vec::new());
        let mut frame = battle.step(crate::BattleInput::default())?;
        assert_eq!(
            frame.actors[child.index()].activity,
            crate::Activity::Recovering
        );
        assert_eq!(frame.actors[child.index()].movement.forward, 12.);
        assert_eq!(frame.actors[child.index()].movement.vertical, 10.);
        assert!(battle.target_available(ActorId(0), child));
        battle.advance_models(&mut frame, crate::BattleClock::Running, true)?;
        assert_eq!(frame.models[1].clip, 3);
        let position = frame.actors[child.index()].position;
        let next = battle.step(crate::BattleInput::default())?;
        assert_ne!(next.actors[child.index()].position, position);
        assert!(next.actors[child.index()].position[1] > position[1]);
        let hp = battle.actors[child.index()].hp;
        for _ in 0..60 {
            let mut frame = battle.step(crate::BattleInput::default())?;
            battle.advance_models(&mut frame, crate::BattleClock::Running, true)?;
            assert_eq!(frame.actors[child.index()].hp, hp);
            assert!(!matches!(
                frame.actors[child.index()].activity,
                crate::Activity::KnockedDown | crate::Activity::GettingUp
            ));
            if frame.actors[child.index()].activity == crate::Activity::Idle {
                break;
            }
        }
        assert_eq!(battle.activity(child), crate::Activity::Idle);
        assert_eq!(battle.actors[child.index()].position[1], 0.);
        Ok(())
    }

    #[test]
    fn core_cleanup_retires_components_without_individual_defeat_events() -> Result<()> {
        let core = ActorId(1);
        let participants = std::iter::once(crate::tests::actor(Side::Party))
            .chain((0..8).map(|_| crate::tests::actor(Side::Enemy)))
            .map(|actor| (actor, Default::default()))
            .collect();
        let definition = DefenseSystemDefinition {
            core,
            repair: (ActorId(6), [200., 0., 0.]),
            components: (2..9).map(ActorId).collect(),
            arms: std::array::from_fn(|index| DefenseArm {
                actor: ActorId(index as u8 + 2),
                anchor: MeleeAnchor {
                    resource: 7,
                    bone: index as u16,
                },
            }),
            deployment: MotionBinding { model: 7, clip: 32 },
            deployment_ticks: 120,
            deployment_pose: Pose {
                rate: 0.5,
                blend: 0,
                ..Default::default()
            },
        };
        let mut battle = PreparedBattle::new(participants, Default::default(), 1)?
            .with_defense_system(definition)?
            .finish()?;
        let party_hp = battle.actors[0].hp;
        battle.actors[core.index()].availability = crate::ActorAvailability::Dead;
        let mut cues = Vec::new();
        battle.advance_defense_system(&mut cues)?;
        assert!(
            battle.actors[2..]
                .iter()
                .all(|actor| actor.hp == 0 && actor.availability == crate::ActorAvailability::Dead)
        );
        assert!(!cues.iter().any(|cue| matches!(cue, Cue::Defeated { .. })));
        assert_eq!(battle.actors[0].hp, party_hp);
        let first = cues.len();
        battle.advance_defense_system(&mut cues)?;
        assert_eq!(cues.len(), first);
        Ok(())
    }
}
