use super::*;
use crate::conditions::Condition;
use crate::{RecoveryKind, conditions::PreparedBuff};

struct Recovery {
    actor: ActorId,
    hp: Option<(i32, i32)>,
    tp: Option<(u16, i32)>,
}

struct LiveEffect {
    request: Release,
    policy: Policy,
    recoveries: Vec<Recovery>,
    buff: Option<(PreparedBuff, [f32; 3])>,
}

pub(super) fn percentage(mut percent: i16, compound: bool, sealed: bool) -> i16 {
    if compound {
        percent = percent.wrapping_add(percent / 4);
    }
    if sealed {
        percent >>= 1;
    }
    percent
}

fn actor_conditions_seal(actor: &crate::Actor) -> bool {
    actor
        .conditions
        .effective()
        .contains(Condition::ReduceItemEffect)
}

impl Battle {
    fn prepare_item_release(&self, request: Release) -> Result<Option<LiveEffect>> {
        let user = request.user;
        ensure!(
            self.actor(user)?.side == Side::Party && self.actor(user)?.available(),
            "item user is unavailable"
        );
        if !self.item_target_eligible(request.item, request.target)? {
            return Ok(None);
        }
        let policy = *self
            .item_policy(request.item)
            .context("unprepared battle item")?;
        let effect = policy.effect;
        let recipients: Vec<_> = if matches!(effect, Effect::PartyRecover { .. }) {
            self.actors
                .iter()
                .enumerate()
                .filter_map(|(index, actor)| {
                    (actor.side == Side::Party && actor.available()).then_some(ActorId(index as u8))
                })
                .collect()
        } else {
            vec![request.target]
        };
        let recovery = match effect {
            Effect::Recover { hp, tp } | Effect::PartyRecover { hp, tp } => Some((hp, tp, true)),
            Effect::FullRecovery => Some((100, 100, false)),
            Effect::Revive => Some((30, 15, true)),
            _ => None,
        };
        let mut recoveries = Vec::new();
        if let Some((hp_percent, tp_percent, modified)) = recovery {
            for &id in &recipients {
                let actor = self.actor(id)?;
                let source = if matches!(effect, Effect::PartyRecover { .. }) {
                    user
                } else {
                    id
                };
                let source = &self.actors[source.index()];
                let percent = |value| {
                    if modified {
                        percentage(
                            i16::from(value),
                            source.conditions.traits().extended_duration,
                            actor_conditions_seal(source),
                        )
                    } else {
                        i16::from(value)
                    }
                };
                let hp =
                    (hp_percent != 0).then(|| actor.recovered_hp(i32::from(percent(hp_percent))));
                let tp = (tp_percent != 0).then(|| actor.recovered_tp(percent(tp_percent)));
                recoveries.push(Recovery { actor: id, hp, tp });
            }
        }
        let buff = if let Effect::Buff(buff) = effect {
            let target = self.actor(request.target)?;
            Some((
                target.conditions.prepare_buff(buff, false),
                target.effect_origin(),
            ))
        } else {
            None
        };
        Ok(Some(LiveEffect {
            request,
            policy,
            recoveries,
            buff,
        }))
    }

    pub(super) fn release_item(
        &mut self,
        user: ActorId,
        provider: &mut dyn Provider,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        // Consume the reservation before preflight so failed releases cannot retry.
        let request = self
            .items
            .pending
            .take_if(|request| request.user == user)
            .context("item release has no pending reservation")?;
        let Some(effect) = self.prepare_item_release(request)? else {
            return Ok(());
        };
        let kind = effect.policy.effect;
        let discovered = if matches!(kind, Effect::Scan) {
            let mut loan = provider.acquire_scan(request)?;
            // Gameplay writes remain infallible until the loan is consumed.
            let learned = loan.scan();
            self.commit_item_effect(effect, cues);
            loan.consume();
            learned
        } else {
            let mut loan = provider.acquire_item(request)?;
            if effect.policy.records_gel_use {
                loan.record_gel_use();
            }
            self.commit_item_effect(effect, cues);
            loan.consume();
            false
        };
        self.ledger.item_consumed(user);
        cues.push(Cue::ItemReleased {
            user,
            target: request.target,
            effect: kind,
            discovered,
        });
        Ok(())
    }

    fn commit_item_effect(&mut self, effect: LiveEffect, cues: &mut Vec<Cue>) {
        let LiveEffect {
            request,
            policy,
            recoveries,
            buff,
        } = effect;
        if matches!(policy.effect, Effect::Revive) {
            self.enter_revival(request.target, cues);
        }
        for recovery in recoveries {
            let actor = &mut self.actors[recovery.actor.index()];
            if let Some((hp, nominal)) = recovery.hp {
                let applied = hp.wrapping_sub(actor.hp);
                actor.hp = hp;
                cues.push(Cue::Recovered {
                    kind: crate::RecoveryKind::Hp,
                    actor: recovery.actor,
                    nominal,
                    applied,
                });
            }
            if let Some((tp, nominal)) = recovery.tp {
                cues.push(Cue::Recovered {
                    actor: recovery.actor,
                    kind: RecoveryKind::Tp,
                    nominal,
                    applied: i32::from(tp) - i32::from(actor.tp),
                });
                actor.tp = tp;
            }
        }
        if let Effect::Cure(cure) = policy.effect {
            self.apply_cure(request.target, cure, cues);
        }
        if let Some((buff, position)) = buff {
            let actor = &mut self.actors[request.target.index()];
            if let Some(kind) = buff.commit(actor) {
                cues.push(Cue::ConditionLabel {
                    actor: request.target,
                    kind,
                    position,
                });
            }
        }
        if matches!(policy.effect, Effect::Scan) {
            self.items.revealed |= 1 << request.target.index();
            cues.push(Cue::EnemyScanned {
                actor: request.target,
            });
            self.ledger.enemy_was_scanned = true;
        }
        if matches!(policy.effect, Effect::AllDivide) {
            self.items.all_divide = true;
        }
        if matches!(policy.effect, Effect::Hourglass) {
            let user_side = self.actors[request.user.index()].side;
            for actor in &mut self.actors {
                if actor.side != user_side && actor.available() {
                    actor.time_stop = HOURGLASS_TICKS;
                }
            }
        }
        self.ledger.item_effect();
        self.items.cooldown = ITEM_COOLDOWN_TICKS;
        cues.push(Cue::ItemNotice {
            actor: request.user,
            item: request.item,
            duration: 90,
        });
    }
}
