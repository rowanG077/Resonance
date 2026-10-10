use crate::{ActorId, Battle, Cue, Side};
use anyhow::{Result, ensure};

const MAX_CHARGE: u16 = 1000;
const ACTIVE_DURATION: u16 = 1000;
const EXTENDED_DURATION: u16 = 1250;

/// Charge is consumed on activation; remaining duration never becomes saved charge.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OverLimit {
    charge: u16,
    remaining: u16,
}

impl OverLimit {
    pub fn new(charge: u16) -> Result<Self> {
        ensure!(charge <= MAX_CHARGE, "Over Limit charge exceeds 1000");
        Ok(Self {
            charge,
            remaining: 0,
        })
    }

    pub fn active(remaining: u16) -> Result<Self> {
        ensure!(
            remaining <= EXTENDED_DURATION,
            "Over Limit duration exceeds 1250"
        );
        Ok(Self {
            charge: 0,
            remaining,
        })
    }

    pub fn charge(self) -> u16 {
        self.charge
    }

    pub fn remaining(self) -> u16 {
        self.remaining
    }

    pub fn is_active(self) -> bool {
        self.remaining != 0
    }

    pub fn saved_percent(self) -> u8 {
        (self.charge / 10) as u8
    }

    fn drain(&mut self, amount: u16) {
        self.remaining = self.remaining.saturating_sub(amount);
    }
}

/// Freeze gameplay for a short effect; an optional rescue actor keeps animating.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Hold {
    pub remaining: u16,
    pub actor: Option<ActorId>,
}

impl Battle {
    pub fn timed_hold_remaining(&self) -> Option<u16> {
        self.timed_hold.map(|hold| hold.remaining)
    }

    pub fn is_paused(&self) -> bool {
        self.target_selector_active() || self.timed_hold.is_some()
    }

    pub(crate) fn gain_overlimit(&mut self, id: ActorId, delta: i8) {
        let actor = &mut self.actors[id.index()];
        if actor.overlimit.is_active() {
            return;
        }
        let mut gain =
            i32::from(delta) * i32::from(self.prepared.actor_setup[id.index()].overlimit_gain);
        if self.prepared.overlimit_boosted_gain {
            gain = gain * 3 / 2;
        }
        actor.overlimit.charge =
            (i32::from(actor.overlimit.charge) + gain).clamp(0, i32::from(MAX_CHARGE)) as u16;
    }

    pub(crate) fn request_timed_hold(&mut self, remaining: u16, actor: Option<ActorId>) {
        if remaining == 0 {
            return;
        }
        self.timed_hold = Some(Hold {
            remaining: self
                .timed_hold
                .map_or(remaining, |old| old.remaining.max(remaining)),
            actor: actor.or(self.timed_hold.and_then(|old| old.actor)),
        });
    }

    pub(crate) fn advance_timed_hold(&mut self) {
        if let Some(hold) = &mut self.timed_hold {
            hold.remaining = hold.remaining.saturating_sub(1);
            if hold.remaining == 0 {
                self.timed_hold = None;
            }
        }
    }

    pub(crate) fn pause_overlimit_contact(&mut self, target: ActorId, eligible: bool) {
        if eligible && self.actors[target.index()].overlimit.is_active() {
            self.request_timed_hold(2, None);
        }
    }

    pub(crate) fn contact_overlimit(&mut self, target: ActorId, gain_eligible: bool) {
        let actor = &mut self.actors[target.index()];
        if actor.overlimit.is_active() {
            actor.overlimit.drain(5);
        } else if gain_eligible {
            self.gain_overlimit(target, 1);
        }
    }

    pub(crate) fn advance_overlimit(&mut self, index: usize, cues: &mut Vec<Cue>) {
        if self.phase() != crate::BattlePhase::Combat {
            return;
        }
        let actor = &self.actors[index];
        let active = actor.overlimit.is_active();
        let entering = actor.available()
            && !active
            && actor.overlimit.charge == MAX_CHARGE
            && !self
                .actors
                .iter()
                .any(|other| other.side == actor.side && other.overlimit.is_active());
        if !active && !entering {
            return;
        }
        let id = ActorId(index as u8);
        let position =
            std::array::from_fn(|axis| actor.position[axis] + 2. * actor.body.center_offset[axis]);
        if entering {
            let actor = &mut self.actors[index];
            actor.overlimit = OverLimit {
                charge: 0,
                remaining: if self.prepared.actor_setup[index].extended_overlimit {
                    EXTENDED_DURATION
                } else {
                    ACTIVE_DURATION
                },
            };
            self.request_timed_hold(45, None);
            cues.push(Cue::OverLimitEntered {
                actor: id,
                position,
            });
        } else {
            let actor = &mut self.actors[index];
            actor.overlimit.drain(2);
        }
    }

    pub fn normalize_result_overlimit(&mut self) -> Result<()> {
        ensure!(
            self.phase() == crate::BattlePhase::Results,
            "Over Limit result normalization outside results"
        );
        for actor in &mut self.actors {
            if actor.side == Side::Party
                && (actor.overlimit.is_active() || actor.overlimit.charge == MAX_CHARGE)
            {
                actor.overlimit = OverLimit::default();
            }
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests;
