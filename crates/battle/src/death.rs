//! Actor availability is the original lifecycle state, independently of HP and
//! motion completion (C988, 28DF4, 2E3F4). Authored controllers own clip selection.
use crate::{ActionPhase, Activity, Actor, ActorId, Battle, Cue, PreparedBattle, Side};
use anyhow::{Context, Result, ensure};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ActorAvailability {
    Absent,
    Petrified,
    Dead,
    #[default]
    Active,
}

impl Actor {
    pub fn available(&self) -> bool {
        self.availability == ActorAvailability::Active
    }
}

/// Already compiled actor callbacks and descriptor behavior, selected before
/// activation. Initial KO enters the ordinary callback without death side effects.
#[derive(Debug, Clone, Copy)]
pub struct DeathBinding {
    pub fall: u16,
    pub initial: u16,
    pub wait_for_motion: bool,
    pub integrate: bool,
    pub darken_immediately: bool,
    /// Ordinary (non-launched) revival; the original absent slot leaves playback.
    pub revival_motion: Option<crate::MotionBinding>,
}

/// Resolved common resources; the contact site owns dispatch, after the death
/// initializer and before the kill ledger and ordinary impact tail.
#[derive(Debug, Clone, Copy)]
pub struct DeathEffect {
    pub appearance: crate::EffectAppearance,
    pub sound: crate::SoundBinding,
}

/// Relationship selection is prepared from persistent party data. Availability,
/// one shared random variant and voice arbitration remain live battle state.
#[derive(Debug, Clone, Copy)]
pub struct AllyDeathReaction {
    pub voices: [Option<crate::VoiceLine>; 2],
    pub priority: u8,
    pub overlimit_gain: u16,
}

#[derive(Debug, Clone)]
pub struct DeathFeedback {
    pub enemy: Option<DeathEffect>,
    /// Victim, then recipient, both in actor roster order.
    pub allies: Vec<Vec<Option<AllyDeathReaction>>>,
}

impl PreparedBattle {
    pub fn with_death_feedback(mut self, feedback: DeathFeedback) -> Result<Self> {
        ensure!(
            feedback.allies.len() == self.actors.len(),
            "death feedback/actor count differs"
        );
        if let Some(effect) = feedback.enemy {
            ensure!(
                self.effects
                    .get(&effect.appearance.resource)
                    .is_some_and(|bank| bank.members.contains_key(&effect.appearance.member)),
                "unprepared death effect"
            );
        }
        for (victim, recipients) in feedback.allies.iter().enumerate() {
            ensure!(
                recipients.len() == self.actors.len(),
                "death recipient/actor count differs"
            );
            for (recipient, reaction) in recipients.iter().enumerate() {
                if let Some(reaction) = reaction {
                    ensure!(
                        self.actors[victim].side == Side::Party
                            && self.actors[recipient].side == Side::Party,
                        "ally death reaction requires party actors"
                    );
                    ensure!(
                        matches!(reaction.priority, 1 | 3) && reaction.overlimit_gain <= 3825,
                        "invalid ally death reaction"
                    );
                }
            }
        }
        self.death_feedback = Some(feedback);
        Ok(self)
    }

    pub fn with_deaths(mut self, bindings: Vec<Option<DeathBinding>>) -> Result<Self> {
        ensure!(
            bindings.len() == self.actors.len(),
            "death binding/actor count differs"
        );
        for (index, binding) in bindings.iter().enumerate() {
            let Some(binding) = binding else { continue };
            for id in [binding.fall, binding.initial] {
                let definition = self
                    .actions
                    .iter()
                    .find(|a| a.id == id)
                    .context("unprepared death controller")?;
                ensure!(
                    definition.phase == ActionPhase::Controller && definition.tp_cost == 0,
                    "death needs an actor controller without a cost"
                );
            }
            if let Some(motion) = binding.revival_motion {
                ensure!(
                    self.models[index]
                        .as_ref()
                        .is_some_and(|m| m.definition.resource == motion.model
                            && m.definition.motions.contains_key(&motion.clip)),
                    "unprepared revival motion"
                );
            }
            ensure!(
                !binding.wait_for_motion || self.models[index].is_some(),
                "death completion requires an actor model"
            );
        }
        self.deaths = bindings;
        Ok(self)
    }
}

impl Battle {
    /// Result performance uses this only for native active Over Limit state4.
    /// A merely full, inactive gauge does not satisfy that source condition.
    pub fn end_overlimit(&mut self, id: ActorId) -> Result<()> {
        self.actor(id)?;
        let actor = &mut self.actors[id.index()];
        if actor.overlimit_active {
            actor.overlimit = 0;
            actor.overlimit_active = false;
        }
        Ok(())
    }

    fn react_to_ally_death(&mut self, victim: ActorId) {
        // 28EC8 is unconditional for a new party death, even if no ally qualifies
        // or every voice is disabled, blocked, or absent in the original table.
        let variant = usize::from(self.random.next() & 1);
        let Some(feedback) = &self.prepared.death_feedback else {
            return;
        };
        for (index, reaction) in feedback.allies[victim.index()].iter().enumerate() {
            let Some(reaction) = reaction else { continue };
            let actor = &mut self.actors[index];
            if !actor.available() {
                continue;
            }
            // 19F0C precedes 71D90, so voice rejection cannot suppress this gain.
            if !actor.overlimit_active {
                actor.overlimit = (actor.overlimit + reaction.overlimit_gain).min(1000);
            }
            if let Some(line) = reaction.voices[variant] {
                self.voices[index].request(line.sound, reaction.priority);
            }
        }
    }

    pub(crate) fn death_contact_feedback(
        &mut self,
        id: ActorId,
        action: crate::ActionId,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        let actor = &self.actors[id.index()];
        if actor.side != Side::Enemy {
            return Ok(());
        }
        let Some(effect) = self.prepared.death_feedback.as_ref().and_then(|f| f.enemy) else {
            return Ok(());
        };
        let position = actor.body.audio_position;
        self.show_effect(
            crate::effect::Spawn {
                action,
                scene: None,
                owner: id,
                target: id,
                appearance: effect.appearance,
                origin: actor.body.center,
                heading: actor.heading,
                follow: Some(crate::effect::Follow::Center(id)),
                scale: actor.effect_scale,
                late: false,
                tint: Default::default(),
            },
            cues,
        )?;
        cues.push(Cue::Sound {
            actor: id,
            sound: effect.sound,
            position,
            priority: 1,
        });
        Ok(())
    }

    pub(crate) fn death_appearance(&self, id: ActorId, visible: &mut bool, tint: &mut [u8; 4]) {
        let actor = &self.actors[id.index()];
        if actor.availability == ActorAvailability::Dead {
            *tint = actor.body.tint;
            *visible &= tint[3] != 0;
        }
    }

    fn start_death_controller(
        &mut self,
        actor: ActorId,
        initial: bool,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        let Some(binding) = self.prepared.deaths[actor.index()] else {
            return Ok(());
        };
        let action = if initial {
            binding.initial
        } else {
            binding.fall
        };
        let definition = self
            .prepared
            .actions
            .iter()
            .position(|a| a.id == action)
            .context("missing death controller")?;
        let (id, mut sequence) = self.allocate_sequence(definition, actor, actor)?;
        // The contact initializer binds its motion immediately, after this
        // update's model sample. Initial KO starts on its first actor visit.
        if !initial {
            crate::script::step_sequence(self, id, &mut sequence, cues)?;
            sequence.age += 1;
        }
        self.sequences.insert(id, sequence);
        cues.push(Cue::Started { action: id, actor });
        Ok(())
    }

    pub(crate) fn initialize_dead_controller(
        &mut self,
        actor: ActorId,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        if self.actors[actor.index()].availability == ActorAvailability::Dead
            && self.prepared.deaths[actor.index()].is_some()
            && !self
                .sequences
                .values()
                .any(|s| s.actor == actor && s.definition.phase == ActionPhase::Controller)
        {
            self.start_death_controller(actor, true, cues)?;
        }
        Ok(())
    }

    pub(crate) fn advance_dead_motion(&mut self, index: usize) {
        let actor = &mut self.actors[index];
        if self.prepared.deaths[index]
            .as_ref()
            .is_none_or(|d| d.integrate)
        {
            actor
                .movement
                .integrate_along(&mut actor.position, [0.; 2], actor.reaction.direction);
            actor
                .movement
                .brake(actor.position[1], actor.activity, false, false);
        }
        crate::movement::floor(actor);
        if self.prepared.deaths[index].is_some_and(|d| d.darken_immediately) {
            // 2503C, checked instructions2589c..258d0: 12 alpha per visit.
            actor.body.tint[3] = actor.body.tint[3].saturating_sub(12);
        }
    }

    pub(crate) fn enter_death(&mut self, id: ActorId, cues: &mut Vec<Cue>) -> Result<()> {
        if self.actors[id.index()].availability == ActorAvailability::Dead {
            return Ok(());
        }
        self.interrupt_actor(id, cues);
        self.clear_actor_particles(id, cues);
        if self.actors[id.index()].side == Side::Party {
            let first = self.actors.iter().position(|a| a.side == Side::Party) == Some(id.index());
            self.ledger.death(id, first);
            self.react_to_ally_death(id);
        }
        let actor = &mut self.actors[id.index()];
        actor.reset_hud_targets();
        actor.availability = ActorAvailability::Dead;
        actor.activity = Activity::Defeated;
        self.contact_feedback[id.index()].clear_flash();
        actor.petrified = false;
        actor.guard.active = false;
        actor.reaction.protection = Default::default();
        actor.overlimit = 0;
        actor.overlimit_active = false;
        self.ledger.record_combo_damage(actor.reaction.combo_damage);
        actor.reaction.combo_hits = 0;
        actor.reaction.combo_damage = 0;
        if self.prepared.deaths[id.index()].is_some_and(|binding| binding.darken_immediately) {
            actor.body.tint = [0, 0, 0, 248];
        }
        if actor.side == Side::Enemy {
            self.last_defeated = Some(id);
            self.death_wait_consumed = false;
        }
        self.start_death_controller(id, false, cues)
    }

    /// Ordinary 27F68 branch: revival opens availability immediately, before
    /// the ten-update get-up controller or its motion completes. Merely writing
    /// HP (including result HP) never has this effect.
    pub(crate) fn revive_percent(
        &mut self,
        id: ActorId,
        percent: i16,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        ensure!(
            (0..=100).contains(&percent),
            "revival percent must be in 0..100"
        );
        let actor = self.actor(id)?;
        if actor.availability == ActorAvailability::Dead {
            ensure!(
                actor.reaction.recoil.kind == crate::RecoilKind::Normal,
                "launched revival is not prepared"
            );
            let definition =
                self.prepared.deaths[id.index()].context("revival controller is not prepared")?;
            ensure!(
                self.models[id.index()].is_some(),
                "revival requires an actor model"
            );
            self.interrupt_actor(id, cues);
            if let Some(motion) = definition.revival_motion {
                self.models[id.index()]
                    .as_mut()
                    .unwrap()
                    .play(motion, 0., 0.5, false, 8)?;
            }
            let actor = &mut self.actors[id.index()];
            actor.reaction.protection.recover();
            actor.reaction.remaining = 10;
            actor.availability = ActorAvailability::Active;
            actor.activity = Activity::GettingUp;
        }
        if percent != 0 {
            self.recover_vitals(id, percent, false, cues)?;
        }
        Ok(())
    }

    /// 381C only delays the banner for the last victim's explicit descriptor flag.
    pub fn victory_death_waiting(&self) -> bool {
        !self.death_wait_consumed
            && self.last_defeated.is_some_and(|id| {
                self.prepared.deaths[id.index()].is_some_and(|binding| binding.wait_for_motion)
            })
    }

    /// 2828 consumes that one descriptor flag when its own motion completes.
    /// Other actors' unfinished motions do not delay the result transition.
    pub fn victory_death_ready(&mut self) -> Result<bool> {
        if !self.victory_death_waiting() {
            return Ok(true);
        }
        let id = self.last_defeated.context("missing final victim")?;
        let ready = self.models[id.index()]
            .as_ref()
            .context("missing final victim model")?
            .finished();
        if ready {
            self.death_wait_consumed = true;
        }
        Ok(ready)
    }
}

#[cfg(test)]
mod tests;
