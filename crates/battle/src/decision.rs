//! Native actor policy and idle scheduling.
use crate::{ActorId, Battle, Cue, PreparedBattle, Side};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeSet;

#[cfg(test)]
mod entry_target_tests;

#[derive(Debug, Clone, Copy)]
pub struct EntryChoice {
    pub actor: ActorId,
    pub strategy: crate::TargetPolicy,
}

#[derive(Debug, Clone, Copy)]
pub struct DecisionDefinition {
    pub idle_ticks: u32,
    pub idle_variation: u32,
}

pub(crate) const QUEUED_COMMAND_DELAY: u32 = 15;
const MIN_REACTION_DELAY: u32 = 15;
const REACTION_DELAY_DIVISOR: u32 = 8;

fn scaled_delay(roll: u16, upper: u32) -> u32 {
    (u64::from(roll) * u64::from(upper) / (u64::from(u16::MAX) + 1)) as u32
}

impl DecisionDefinition {
    pub(crate) fn idle_delay(self, roll: u16, after_hit: bool) -> u32 {
        let delay = self
            .idle_ticks
            .saturating_add(scaled_delay(roll, self.idle_variation));
        if after_hit {
            (delay / REACTION_DELAY_DIVISOR).max(MIN_REACTION_DELAY)
        } else {
            delay
        }
    }
}

impl PreparedBattle {
    pub fn initial_actors(&self) -> &[crate::Actor] {
        &self.actors
    }

    pub fn with_entry_choices(mut self, choices: Vec<EntryChoice>) -> Result<Self> {
        ensure!(
            choices.len() == self.actors.len(),
            "entry choice/actor count differs"
        );
        let mut seen = BTreeSet::new();
        for choice in &choices {
            ensure!(
                choice.actor.index() < self.actors.len() && seen.insert(choice.actor),
                "invalid entry choice actor"
            );
            ensure!(
                choice.strategy != crate::TargetPolicy::SelfTarget
                    || self.actors[choice.actor.index()].side == Side::Enemy,
                "only enemies can initially target themselves"
            );
        }
        let leader = self
            .actors
            .iter()
            .position(|actor| actor.side == Side::Party)
            .context("entry needs a party leader")?;
        let mut targets: Vec<_> = self
            .actors
            .iter()
            .map(|actor| crate::TargetActor {
                side: actor.side,
                position: actor.position,
                hp: actor.hp,
                hp_percent: actor.hp_percent(),
                flying: actor.movement.flying,
                casting: false,
                available: actor.available(),
                hidden: false,
                dying: actor.availability == crate::ActorAvailability::Dead,
                target: None,
            })
            .collect();
        let mut random = crate::Random::new(self.random_seed);
        // Establish the player leader first so followers observe its actual target.
        for index in
            std::iter::once(leader).chain((0..self.actors.len()).filter(|index| *index != leader))
        {
            let policy = if index == leader {
                crate::TargetPolicy::Nearest
            } else {
                choices
                    .iter()
                    .find(|choice| choice.actor.index() == index)
                    .unwrap()
                    .strategy
            };
            let target = crate::select_target(&targets, index, policy, &mut || random.next_u16())?
                .context("entry actor has no eligible target")?;
            targets[index].target = Some(target);
            self.targets[index] = ActorId(target as u8);
        }
        self.random_seed = random.state();
        for index in 0..self.actors.len() {
            let target = self.actors[self.targets[index].index()].position;
            let owner = &mut self.actors[index];
            let direction = crate::distance::normalize(std::array::from_fn(|axis| {
                target[axis] - owner.position[axis]
            }));
            owner.movement.direction = direction;
            owner.facing_direction = direction;
            crate::control::face(owner, direction, 0.);
        }
        Ok(self)
    }
}

impl Battle {
    pub(crate) fn initialize_entry_state(&mut self) -> Result<()> {
        for index in 0..self.actors.len() {
            if self.prepared.actor_setup[index].enemy_decision.is_some() {
                self.choose_enemy_action(ActorId(index as u8))?;
            }
        }
        for (runtime, setup) in self.runtime.iter_mut().zip(&self.prepared.actor_setup) {
            let variation = setup.decision.map_or(0, |decision| decision.idle_variation);
            runtime.idle_timer = if variation == 0 {
                0
            } else {
                scaled_delay(self.random.next_u16(), variation)
            };
        }
        Ok(())
    }

    pub(crate) fn advance_ai(&mut self, actor: ActorId, cues: &mut Vec<Cue>) -> Result<()> {
        let index = actor.index();
        if self.phase() != crate::BattlePhase::Combat || self.actors[index].hit_stop > 0 {
            return Ok(());
        }
        let companion = self.actors[index].control == crate::Control::Auto
            && self.prepared.actor_setup[index].companion.is_some();
        let enemy = self.actors[index].control == crate::Control::Enemy
            && self.prepared.actor_setup[index].enemy_decision.is_some();
        if !companion && !enemy {
            return Ok(());
        }
        if !self.actor_command_ready(actor) {
            return Ok(());
        }
        if companion {
            self.advance_auto_charge(actor, cues)?;
        }
        if self.runtime[index].idle_timer > 0 {
            self.runtime[index].idle_timer -= 1;
            self.maintain_ai_idle(actor);
            return Ok(());
        }
        if companion {
            self.advance_companion_ai(actor, cues)
        } else {
            self.advance_enemy_ai(actor)
        }
    }

    pub(crate) fn maintain_ai_idle(&mut self, actor: ActorId) {
        let index = actor.index();
        self.actors[index].movement.forward = 0.;

        let turn_ticks = self.prepared.actor_setup[index].turn_ticks().unwrap_or(8);
        if let Some(target) = self.target(actor) {
            let direction = crate::distance::planar_direction(
                self.actors[target.index()].position,
                self.actors[index].position,
                self.actors[index].facing_direction,
            );
            self.actors[index].facing_direction = direction;
            crate::control::face(
                &mut self.actors[index],
                direction,
                180. / f32::from(turn_ticks),
            );
        }
    }

    pub(crate) fn decision_ready(&self, actor: ActorId) -> bool {
        self.terminal.result.is_none() && self.actor_command_ready(actor)
    }

    /// The actor can accept a new command when idle and free of active work.
    pub(crate) fn actor_command_ready(&self, actor: ActorId) -> bool {
        self.actors[actor.index()].available() && self.activity(actor) == crate::Activity::Idle
    }

    pub(crate) fn complete_ordinary_action(&mut self, actor: ActorId) -> Result<()> {
        self.actors[actor.index()].movement.direction = self.actors[actor.index()].facing_direction;
        if self.begin_recovery_return(actor)? {
            return Ok(());
        }
        self.enter_idle(actor);
        Ok(())
    }

    pub(crate) fn decision_target(
        &mut self,
        owner: ActorId,
        policy: crate::TargetPolicy,
    ) -> Result<Option<ActorId>> {
        let actors: Vec<_> = self
            .actors
            .iter()
            .enumerate()
            .map(|(index, a)| crate::TargetActor {
                side: a.side,
                position: a.position,
                hp: a.hp,
                hp_percent: a.hp_percent(),
                flying: a.movement.flying,
                casting: matches!(
                    self.activity(ActorId(index as u8)),
                    crate::Activity::Casting { .. }
                ),
                available: a.available(),
                hidden: false,
                dying: a.availability == crate::ActorAvailability::Dead,
                target: self.target(ActorId(index as u8)).map(ActorId::index),
            })
            .collect();
        let target = crate::select_target(&actors, owner.index(), policy, &mut || {
            self.random.next_u16()
        })?
        .map(|index| ActorId(index as u8));
        Ok(target)
    }

    pub(crate) fn set_decision_target(&mut self, owner: ActorId, target: ActorId) -> Result<()> {
        ensure!(
            target.index() < self.actors.len(),
            "invalid decision target"
        );
        self.runtime[owner.index()].target = target;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_policy_respects_idle_delay_pause_and_control_mode() -> Result<()> {
        let mut battle = crate::companion::tests::prepared()?.finish().unwrap();
        let owner = ActorId(2);
        battle.runtime[2].idle_timer = 2;
        battle.advance_ai(owner, &mut Vec::new())?;
        assert_eq!(battle.runtime[2].idle_timer, 1);
        assert_eq!(battle.activity(ActorId(2)), crate::Activity::Idle);
        battle.actors[2].hit_stop = 1;
        battle.advance_ai(owner, &mut Vec::new())?;
        assert_eq!(battle.runtime[2].idle_timer, 1);
        battle.actors[2].hit_stop = 0;
        battle.actors[2].control = crate::Control::Manual;
        battle.advance_ai(owner, &mut Vec::new())?;
        assert_eq!(battle.runtime[2].idle_timer, 1);
        Ok(())
    }

    #[test]
    fn idle_delays_are_bounded_and_shorter_after_hit_recovery() {
        let mut definition = DecisionDefinition {
            idle_ticks: 70_000,
            idle_variation: 70_000,
        };
        for roll in [0, u16::MAX] {
            assert!((70_000..140_000).contains(&definition.idle_delay(roll, false)));
        }
        definition.idle_ticks = u32::MAX;
        assert_eq!(definition.idle_delay(u16::MAX, false), u32::MAX);
        assert_eq!(
            definition.idle_delay(u16::MAX, true),
            u32::MAX / REACTION_DELAY_DIVISOR
        );
        definition.idle_ticks = 0;
        assert_eq!(definition.idle_delay(0, true), MIN_REACTION_DELAY);
    }
}
