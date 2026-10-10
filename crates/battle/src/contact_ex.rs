//! Shared contact and action-exit skill effects.
use crate::{ActorId, Battle, Cue, RecoveryKind, Side};
use anyhow::{Context, Result};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ContactEx {
    pub follow_up: bool,
    pub combo_hp: bool,
    pub combo_tp: bool,
    pub down_tp: bool,
    pub damage_tp: bool,
    pub reflect_damage: bool,
    pub hammer_revenge: bool,
    pub aid_revenge: bool,
}

/// A short, single-hit fall above the attacker's contact position. No homing.
fn hammer() -> Arc<crate::ProjectileDefinition> {
    Arc::new(crate::ProjectileDefinition {
        lifetime: Some(30),
        velocity: [0., -6., 0.],
        acceleration: [0.; 3],
        offset: [0.; 3],
        clamp_ground: false,
        active: None,
        birth: None,
        motion: Default::default(),
        effects: Default::default(),
        contact: Some(crate::ProjectileContact {
            hit: crate::HitRule {
                kind: crate::DamageKind::Thrust,
                arte: true,
                power: crate::Power::Percent(50),
                element: crate::HitElement::Neutral,
                overlimit_pause: false,
                prevents_defeat: false,
                guard: Default::default(),
                reaction: crate::ReactionRule {
                    hitstun: 20,
                    stun_chance: 25,
                    ..Default::default()
                },
                condition: None,
            },
            cooldown: 0,
            repeat_limit: 1,
            radius: 20.,
            height: 0.,
            shape: crate::HitShape::Sphere,
            offset: [0.; 3],
            radius_growth: 0.,
            height_growth: 0.,
            survives_contact: false,
            clashes: false,
        }),
    })
}

impl Battle {
    fn contact_ex_label(&self, actor: ActorId, cues: &mut Vec<Cue>) {
        let body = &self.actors[actor.index()];
        cues.push(Cue::ExSkillLabel {
            actor,
            position: std::array::from_fn(|i| body.position[i] + 2. * body.body.center_offset[i]),
        });
    }

    /// Finish after grounded or forced movement exit, before recovery can replace the combo.
    pub(crate) fn finish_combo_recovery(&mut self, victim: ActorId, cues: &mut Vec<Cue>) {
        if self.actors[victim.index()].equipment.reaction_ex.damage_tp {
            let amount = (i64::from(self.actors[victim.index()].reaction.combo_damage) * 3 / 100)
                .clamp(1, i64::from(i32::MAX)) as i32;
            let before = self.actors[victim.index()].tp;
            self.actors[victim.index()].recover_flat_tp(amount);
            cues.push(Cue::Recovered {
                actor: victim,
                kind: RecoveryKind::Tp,
                nominal: amount,
                applied: i32::from(self.actors[victim.index()].tp) - i32::from(before),
            });
            self.contact_ex_label(victim, cues);
        }
        if self.actors[victim.index()].side == Side::Enemy {
            let hits = self.actors[victim.index()].reaction.contributor_hits;
            // Only active party contributors receive their equipped combo recovery.
            for (index, &contributor_hits) in hits[..self.actors.len()].iter().enumerate() {
                let actor = &self.actors[index];
                let percent = i16::from(contributor_hits >> 1);
                if actor.side != Side::Party || !actor.available() || percent == 0 {
                    continue;
                }
                let id = ActorId(index as u8);
                let traits = actor.equipment.reaction_ex;
                if traits.combo_hp {
                    let actor = &mut self.actors[index];
                    let before = actor.hp;
                    let (hp, nominal) = actor.recovered_hp(i32::from(percent));
                    actor.hp = hp;
                    cues.push(Cue::Recovered {
                        kind: crate::RecoveryKind::Hp,
                        actor: id,
                        nominal,
                        applied: hp - before,
                    });
                }
                if traits.combo_tp {
                    let (tp, nominal) = self.actors[index].recovered_tp(percent);
                    cues.push(Cue::Recovered {
                        actor: id,
                        kind: RecoveryKind::Tp,
                        nominal,
                        applied: i32::from(tp) - i32::from(self.actors[index].tp),
                    });
                    self.actors[index].tp = tp;
                }
                if traits.combo_hp || traits.combo_tp {
                    self.contact_ex_label(id, cues);
                }
            }
        }
    }

    /// Initialize the bounded knockdown window and apply its recovery bonus once.
    pub(crate) fn initialize_knockdown_window(&mut self, index: usize, cues: &mut Vec<Cue>) {
        let actor = &mut self.actors[index];
        if actor.reaction.stagger.initialized {
            return;
        }
        let window = (60 - actor.reaction.combo_hits.max(0) / 2).clamp(1, 60) as u8;
        actor.reaction.stagger.window = window;
        if actor.equipment.reaction_ex.follow_up {
            actor.reaction.stagger.window = actor.reaction.stagger.window.saturating_add(5);
        }
        actor.reaction.stagger.initialized = true;
        if actor.equipment.reaction_ex.down_tp
            && self.random.next_u16() % 100 < actor.equipment.luck / 10 + 10
        {
            let id = ActorId(index as u8);
            self.contact_ex_label(id, cues);
            let (tp, nominal) = self.actors[index].recovered_tp(5);
            cues.push(Cue::Recovered {
                actor: id,
                kind: RecoveryKind::Tp,
                nominal,
                applied: i32::from(tp) - i32::from(self.actors[index].tp),
            });
            self.actors[index].tp = tp;
        }
    }

    /// Unblocked damage credits its contributor. Living recipients may retaliate.
    pub(crate) fn contact_retaliation(
        &mut self,
        owner: ActorId,
        victim: ActorId,
        hit: crate::HitResult,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        if !hit.is_unblocked_damage() {
            return Ok(());
        }
        let actor = &mut self.actors[victim.index()];
        let hits = &mut actor.reaction.contributor_hits[owner.index()];
        *hits = hits.saturating_add(1);
        if actor.hp == 0 {
            return Ok(());
        }
        let traits = actor.equipment.reaction_ex;
        let luck = actor.equipment.luck / 25;
        if traits.hammer_revenge
            && self.actors[owner.index()].available()
            && self.random.next_u16() % 100 < luck + 5
        {
            let action = crate::ActionId(self.next_action);
            self.next_action = self
                .next_action
                .checked_add(1)
                .context("retaliation action handle exhausted")?;
            let mut origin = self.actors[owner.index()].effect_origin();
            const DROP_HEIGHT: f32 = 80.;
            origin[1] += DROP_HEIGHT;
            let projectile = self.emit(hammer(), action, victim, owner, origin)?;
            cues.push(Cue::HammerRevenge { projectile });
            self.contact_ex_label(victim, cues);
        }
        if traits.aid_revenge && self.random.next_u16() % 100 < luck + 2 {
            const FIRST_AID_PERCENT: i16 = 20;
            self.recover(victim, FIRST_AID_PERCENT, cues)?;

            self.contact_ex_label(victim, cues);
        }
        if traits.reflect_damage
            && self.actors[owner.index()].hp > 1
            && self.random.next_u16() % 100 < luck + 5
        {
            self.contact_ex_label(victim, cues);
            let actor = &mut self.actors[owner.index()];
            // Reflect actual damage taken, leaving a living attacker with at least one HP.
            let amount = hit.hp_change.saturating_neg().min(actor.hp - 1);
            actor.hp -= amount;
            self.runtime[owner.index()].recovery.last_hit_damage = amount;
            self.runtime[owner.index()].recovery.last_hit_recovery = amount / 2;
            cues.push(Cue::IncidentalDamage {
                actor: owner,
                amount,
            });
            const REFLECTION_HITSTUN: u32 = 15;
            self.begin_hurt(owner, REFLECTION_HITSTUN, cues);
            self.model_requests.push(crate::ModelRequest::Hurt {
                actor: owner,
                alternate: true,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
