//! Escape requests, success checks, and gauge updates.
//! Successful escape replaces party combat activity with departure.
use crate::{ActorId, Battle, BattlePhase, BattleResult, Control, PreparedBattle, Side, Sound};
use anyhow::{Context, Result, ensure};

pub const MAX_ESCAPE_GAUGE: i16 = 1024;

mod movement;
#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy)]
pub struct EscapeActorDefinition {
    pub actor: ActorId,
    pub request: Option<Sound>,
    pub cancel: Option<Sound>,
    pub success: Option<Sound>,
}

#[derive(Debug, Clone)]
pub struct EscapeDefinition {
    /// Prepared command capability, independent of request visibility.
    pub allowed: bool,
    /// Difference of whole-roster average levels, clamped to -8..8.
    pub level_difference: i8,
    /// Battle-wide equipment escape bonus.
    pub magic_mist: bool,
    pub actors: Vec<EscapeActorDefinition>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EscapeFrame {
    pub requested: bool,
    pub gauge: i16,
}

#[derive(Debug, Default)]
pub(crate) struct State {
    requested: bool,
    gauge: i16,
    /// The lifecycle recognizer and Battle::step share one actual update visit.
    recognized_update: Option<u64>,
    magic_mist: bool,
}

impl State {
    pub(crate) fn new(definition: Option<&EscapeDefinition>) -> Self {
        Self {
            magic_mist: definition.is_some_and(|definition| definition.magic_mist),
            ..Default::default()
        }
    }
}

impl PreparedBattle {
    pub fn with_escape(mut self, definition: EscapeDefinition) -> Self {
        self.resources.escape = Some(definition);
        self
    }

    pub(crate) fn validate_escape(&self) -> Result<()> {
        let Some(definition) = &self.resources.escape else {
            return Ok(());
        };
        ensure!(
            (-8..=8).contains(&definition.level_difference),
            "invalid escape level difference"
        );
        let mut seen = std::collections::BTreeSet::new();
        for row in &definition.actors {
            ensure!(seen.insert(row.actor), "duplicate escape actor");
            let actor = self
                .actors
                .get(row.actor.index())
                .context("unknown escape actor")?;
            ensure!(
                actor.side == Side::Party,
                "escape actor must be a party member"
            );
            ensure!(
                self.resources.actor_setup[row.actor.index()]
                    .control
                    .is_some(),
                "escape requires prepared movement control"
            );
            // Voice table/duration diagnostics belong to game preparation.
            // A tolerant missing duration remains zero; playback completion is
            // audio-owned, and this controller consumes only Sound.
        }
        ensure!(
            self.actors
                .iter()
                .enumerate()
                .all(|(index, actor)| actor.side != Side::Party
                    || seen.contains(&ActorId(index as u8))),
            "escape definitions must cover every party member"
        );
        Ok(())
    }
}

impl Battle {
    pub fn refresh_escape_magic_mist(&mut self, active: bool) {
        self.escape.magic_mist = active;
    }

    pub(crate) fn escape_actor_definition(&self, actor: ActorId) -> Option<&EscapeActorDefinition> {
        self.prepared
            .escape
            .as_ref()?
            .actors
            .iter()
            .find(|row| row.actor == actor)
    }

    pub fn escape_frame(&self) -> Option<EscapeFrame> {
        self.prepared.escape.as_ref()?;
        if !(self.phase() == BattlePhase::Combat
            || (self.phase() == BattlePhase::Ending
                && self.terminal.result == Some(BattleResult::Escaped)))
        {
            return None;
        }
        Some(EscapeFrame {
            requested: self.escape.requested,
            gauge: self.escape.gauge,
        })
    }

    pub fn validate_escape_toggle(&self, actor: ActorId) -> Result<()> {
        ensure!(
            self.phase() == BattlePhase::Combat,
            "escape request outside combat"
        );
        let definition = self.prepared.escape.as_ref().context("unprepared escape")?;
        ensure!(
            definition.allowed,
            "escape is restricted for this encounter"
        );
        ensure!(
            self.escape_actor_definition(actor).is_some(),
            "unknown escape command owner"
        );
        Ok(())
    }

    pub fn toggle_escape(&mut self, actor: ActorId) -> Result<bool> {
        self.validate_escape_toggle(actor)?;
        let definition = *self.escape_actor_definition(actor).unwrap();
        let requested = !self.escape.requested;
        if !requested {
            self.ledger.escape_cancellations =
                self.ledger.escape_cancellations.saturating_add(1).min(3);
        }
        if self.actors[actor.index()].available()
            && let Some(line) = if requested {
                definition.request
            } else {
                definition.cancel
            }
        {
            self.request_voice(actor, line, crate::VoicePriority::Reaction);
        }
        self.escape.requested = requested;
        Ok(requested)
    }

    /// Resolve terminal outcomes before gauge decay and the escape threshold check. Querying
    /// alone does not advance the gauge.
    pub fn recognize_update(&mut self) -> Option<BattleResult> {
        if let Some(result) = self.recognize_result() {
            return Some(result);
        }
        if self.prepared.escape.is_none()
            || self.phase() != BattlePhase::Combat
            || self.is_paused()
            || self.escape.recognized_update == Some(self.update)
        {
            return None;
        }
        self.escape.recognized_update = Some(self.update);
        if self.escape.gauge >= MAX_ESCAPE_GAUGE {
            self.escape.gauge = MAX_ESCAPE_GAUGE;
            // The phase and all earlier terminal cases were checked above.
            self.recognize_escape(false)
                .expect("checked ordinary escape phase");
            return self.recognize_result();
        }
        self.escape.gauge = self.escape.gauge.saturating_sub(2).max(0);
        None
    }

    pub(crate) fn advance_escape(&mut self) {
        let Some(definition) = &self.prepared.escape else {
            return;
        };
        if !self.escape.requested || self.phase() != BattlePhase::Combat || self.is_paused() {
            return;
        }
        if definition.allowed {
            let skill = self.actors.iter().any(|actor| {
                actor.side == Side::Party && actor.available() && actor.equipment.quick_escape
            });
            let bonus = i16::from(self.escape.magic_mist) * 4 + i16::from(skill) * 4;
            let rate = (i16::from(definition.level_difference) + 7 + bonus).clamp(3, 24);
            self.escape.gauge = self
                .escape
                .gauge
                .saturating_add(rate)
                .clamp(0, MAX_ESCAPE_GAUGE);
        }
    }

    /// Run once when results are recognized, before the shared world update.
    pub(crate) fn enter_escape(&mut self) {
        self.escape.requested = false;
        self.timed_hold = None;
        self.clear_hourglass();
        for actor in &mut self.actors {
            if actor.availability != crate::ActorAvailability::Petrified {
                actor.hit_stop = 0;
            }
        }
        if !self.forced_escape() {
            for actor in &mut self.actors {
                if actor.side == Side::Party && actor.available() {
                    actor.reaction.protection.escape();
                }
            }
            let party = |(_, actor): &(usize, &crate::Actor)| actor.side == Side::Party;
            let leader = self
                .actors
                .iter()
                .enumerate()
                .filter(party)
                .find(|(_, actor)| actor.control != Control::Auto)
                .or_else(|| self.actors.iter().enumerate().find(party))
                .map(|(index, _)| ActorId(index as u8));
            if let Some(leader) = leader {
                self.ledger.ordinary_escape = Some(leader);
                if self.actors[leader.index()].available()
                    && let Some(line) = self
                        .escape_actor_definition(leader)
                        .and_then(|row| row.success)
                {
                    self.request_voice(leader, line, crate::VoicePriority::Announcement);
                }
            }
            self.ledger.adjust(0);
        }
    }
}
