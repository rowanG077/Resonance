//! Actor-owned birds use the ordinary particle renderer and the stun controller's clock.
use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum StunPhase {
    Existing,
    Contacts,
}

impl Effects {
    pub fn with_head_anchors(mut self, anchors: Vec<crate::HeadAnchor>) -> Self {
        self.head_anchors = anchors;
        self
    }

    pub fn with_stun_particle(
        mut self,
        definition: Option<Arc<crate::ParticleDefinition>>,
    ) -> Result<Self> {
        if let Some(definition) = &definition {
            definition.validate()?;
            ensure!(
                definition.model.is_none() && definition.data.lifetime.is_none(),
                "stun birds require a controller-owned sprite particle"
            );
        }
        self.stun_particle = definition;
        Ok(self)
    }

    pub(super) fn update_stun(
        &mut self,
        frame: &BattleFrame,
        paused: bool,
        phase: StunPhase,
    ) -> Result<()> {
        let clearing = frame.recognized_result.is_some()
            || frame
                .cues
                .iter()
                .any(|cue| matches!(cue, Cue::ClearEffects | Cue::CombatRetired));
        self.stunned.retain(|actor, particle| {
            let live = !clearing
                && frame.actors.get(actor.index()).is_some_and(|owner| {
                    owner.available()
                        && owner.hp > 0
                        && owner.activity == crate::Activity::Stunned
                        && owner.stun_remaining > 0
                });
            if !live {
                self.particles.remove(particle);
            }
            live && self.particles.contains_key(particle)
        });
        let Some(definition) = self.stun_particle.clone().filter(|_| !clearing) else {
            return Ok(());
        };
        for (index, actor) in frame.actors.iter().enumerate() {
            if !actor.available()
                || actor.hp <= 0
                || actor.activity != crate::Activity::Stunned
                || actor.stun_remaining == 0
            {
                continue;
            }
            let owner = ActorId::from_index(index)?;
            let contact = frame
                .cues
                .iter()
                .any(|cue| matches!(cue, Cue::Hit { actor, stunned: true, .. } if *actor == owner));
            if phase == StunPhase::Contacts {
                if !contact || paused {
                    continue;
                }
                // A fresh stun replaces its birds after the ordinary particle visit.
                if let Some(particle) = self.stunned.remove(&owner) {
                    self.particles.remove(&particle);
                }
            }
            let Some(origin) = self.head_position(owner, frame)? else {
                continue;
            };
            if !self.stunned.contains_key(&owner) {
                // A cutscene can hold the old actor task after clearing combat effects.
                if paused || phase == StunPhase::Existing && contact {
                    continue;
                }
                let id =
                    self.spawn_particle(definition.clone(), owner, owner, origin, actor.heading)?;
                crate::particle::apply_scale(
                    &mut self.particles.get_mut(&id).unwrap().frame.state,
                    actor.effect_scale,
                )?;
                self.stunned.insert(owner, id);
            }
            let particle = self.particles.get_mut(&self.stunned[&owner]).unwrap();
            particle.frame.origin = origin;
            particle.frame.heading = actor.heading;
            let spin = if actor.stun_struggling { 3. } else { 1. };
            particle.frame.state.angular_velocity = definition
                .data
                .state
                .angular_velocity
                .map(|speed| speed * spin);
            let count = (actor.stun_remaining / 45 + 1).min(4) as u8;
            particle.frame.state.geometry_count = if count == 3 { 4 } else { count };
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
