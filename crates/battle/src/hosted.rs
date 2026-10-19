//! Formation participants carried by another actor's animated body.
use crate::{ActorId, Battle, Cue, MeleeAnchor, MotionBinding, PreparedBattle};
use anyhow::{Context, Result, ensure};
use resonance_content::battle_profile::HostedBody;

const RELEASE_FORWARD_SPEED: f32 = 12.;
const RELEASE_UPWARD_SPEED: f32 = 10.;
const RELEASE_BLEND_UPDATES: u8 = 4;

#[derive(Debug, Clone, Copy)]
pub struct HostedParticipant {
    pub actor: ActorId,
    pub host: ActorId,
    pub anchor: MeleeAnchor,
    pub release: HostedBody,
    pub released_hurt: [Option<MotionBinding>; 2],
}

impl PreparedBattle {
    pub fn with_hosted_participants(
        mut self,
        participants: Vec<HostedParticipant>,
    ) -> Result<Self> {
        let mut children = std::collections::BTreeSet::new();
        for child in &participants {
            ensure!(
                child.host.index() < child.actor.index()
                    && child.actor.index() < self.actors.len()
                    && self.actors[child.host.index()].side
                        == self.actors[child.actor.index()].side
                    && children.insert(child.actor),
                "invalid hosted participant"
            );
            self.resources.actor_setup[child.actor.index()].target_excluded = true;
        }
        self.resources.hosted_participants = participants;
        Ok(self)
    }
}

impl Battle {
    pub(crate) fn is_hosted(&self, actor: ActorId) -> bool {
        self.prepared
            .hosted_participants
            .iter()
            .any(|child| child.actor == actor)
    }

    pub(crate) fn hosted_positions(
        &self,
        placements: &[Option<resonance_content::animation::Matrix>],
    ) -> Result<Vec<(ActorId, [f32; 3])>> {
        self.prepared
            .hosted_participants
            .iter()
            .map(|child| {
                let models = self
                    .models
                    .as_ref()
                    .context("hosted participant has no model playback")?;
                let placement = placements
                    .get(child.host.index())
                    .copied()
                    .flatten()
                    .context("hosted participant has no host placement")?;
                Ok((
                    child.actor,
                    models.anchor_at(child.host, child.anchor, Some(placement))?,
                ))
            })
            .collect()
    }

    pub(crate) fn follow_hosted_participants(&mut self, positions: Vec<(ActorId, [f32; 3])>) {
        for (actor, position) in positions {
            self.actors[actor.index()].position = position;
        }
    }

    pub(crate) fn release_hosted_participants(&mut self, cues: &mut Vec<Cue>) -> Result<()> {
        let released: Vec<_> = self
            .prepared
            .hosted_participants
            .iter()
            .copied()
            .filter(|child| {
                let host = &self.actors[child.host.index()];
                let defeated = host.availability == crate::ActorAvailability::Dead;
                let hurt = defeated
                    || matches!(
                        self.runtime[child.host.index()].task(),
                        crate::state::ActorTask::Hurt { .. }
                            | crate::state::ActorTask::Knockdown(_)
                    );
                self.actors[child.actor.index()].available()
                    && (child.release.release_on_defeat && defeated
                        || hurt
                            && (child.release.release_on_hurt
                                || child.release.release_below_half_health
                                    && host.hp_percent() < 50))
            })
            .collect();
        for child in released {
            let position = self
                .models
                .as_ref()
                .context("hosted participant has no model playback")?
                .anchor(child.host, child.anchor)?;
            self.prepared
                .hosted_participants
                .retain(|link| link.actor != child.actor);
            self.prepared.actor_setup[child.actor.index()].target_excluded = false;
            self.models
                .as_mut()
                .unwrap()
                .release_hosted_body(child.actor, child.released_hurt)?;
            self.interrupt_actor(child.actor, cues);
            self.clear_technique_command(child.actor);
            self.enter_idle(child.actor);
            if let Some(items) = &mut self.prepared.items {
                items.targets[child.actor.index()].excluded = false;
            }
            let host_position = self.actors[child.host.index()].position;
            let actor = &mut self.actors[child.actor.index()];
            actor.position = position;
            actor.movement.steering.passable_for_allies = false;
            actor.movement.steering.push_obstacle_disabled = false;
            actor
                .reaction
                .protection
                .armor(crate::knockdown::GET_UP_DURATION);
            if child.release.launch_on_release {
                actor.movement.direction =
                    crate::distance::normalize(std::array::from_fn(|axis| {
                        position[axis] - host_position[axis]
                    }));
                actor.movement.forward = RELEASE_FORWARD_SPEED;
                actor.movement.vertical = RELEASE_UPWARD_SPEED;
            }
            self.set_task(child.actor.index(), crate::state::ActorTask::Released);
            if let Some(motion) = child.released_hurt[0] {
                self.model_requests.push(crate::ModelRequest::Play {
                    actor: child.actor,
                    motion,
                    pose: crate::Pose {
                        rate: 0.5,
                        blend: RELEASE_BLEND_UPDATES,
                        ..Default::default()
                    },
                });
            }
        }
        Ok(())
    }
}

#[cfg(feature = "test-support")]
impl Battle {
    /// Restore a host whose entire group was released and defeated before the fixture.
    pub fn restore_fixture_released_hosted(&mut self, host: ActorId) -> Result<()> {
        let children: Vec<_> = self
            .prepared
            .hosted_participants
            .iter()
            .copied()
            .filter(|child| child.host == host)
            .collect();
        ensure!(
            self.update == 0
                && !children.is_empty()
                && children.iter().all(|child| {
                    let actor = &self.actors[child.actor.index()];
                    actor.hp == 0 && actor.availability == crate::ActorAvailability::Dead
                }),
            "released fixture must precede updates and contain only defeated children"
        );
        let models = self
            .models
            .as_mut()
            .context("released fixture has no model playback")?;
        for child in children {
            self.prepared.actor_setup[child.actor.index()].target_excluded = false;
            models.release_hosted_body(child.actor, child.released_hurt)?;
            let actor = &mut self.actors[child.actor.index()];
            actor.movement.steering.passable_for_allies = false;
            actor.movement.steering.push_obstacle_disabled = false;
            if let Some(items) = &mut self.prepared.items {
                items.targets[child.actor.index()].excluded = false;
            }
        }
        self.prepared
            .hosted_participants
            .retain(|child| child.host != host);
        Ok(())
    }
}
