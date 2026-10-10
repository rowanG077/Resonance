//! A result meal is a game transaction; notices display its completed outcome.
use super::{Candidate, ResultNotice};
use anyhow::{Context, Result, ensure};
use resonance_battle::{Battle, BattlePhase, Side};

struct Reload {
    actor: resonance_battle::ActorId,
    member: usize,
    max_hp: i32,
    max_tp: u16,
    conditions: resonance_battle::conditions::Conditions,
    overlimit: resonance_battle::OverLimit,
}

impl Candidate {
    pub(super) fn cook(&mut self, battle: &mut Battle) -> Result<()> {
        if self.party.cooking.full {
            return Ok(());
        }
        ensure!(
            battle.phase() == BattlePhase::Results,
            "cooking outside results"
        );
        ensure!(self.results.is_some(), "cooking before rewards");

        let mut party = self.snapshot_party(battle)?;
        let mut random = self.gameplay_random;
        let recipe = party.cooking.recipe;
        let Ok(meal) = party.cook(&self.menus, || random.next_u32()) else {
            return Ok(());
        };
        let character = party.cooking.chef + 1;
        let reloads = self
            .setup
            .actors
            .iter()
            .map(|&(actor, character)| {
                let live = battle
                    .actors()
                    .get(actor.index())
                    .context("missing cooking actor")?;
                ensure!(
                    live.side == Side::Party,
                    "cooking actor is not a party member"
                );
                let index = usize::from(character - 1);
                let member = &party.members[index];
                let loadout = super::super::party::loadout(&self.menus, member, index)?;
                let max_hp = loadout.attributes.max_hp;
                let max_tp = loadout.attributes.max_tp;
                let [base_hp, base_tp] = member.maximum_vitals();
                ensure!(
                    max_hp > 0
                        && i32::from(member.hp) <= max_hp
                        && member.tp <= max_tp
                        && i32::from(base_hp) <= max_hp
                        && base_tp <= max_tp,
                    "invalid cooking reload vitals"
                );
                let conditions = super::super::conditions::prepare_reload(
                    &live.conditions,
                    member.ailments,
                    &member.queued_buffs,
                    &loadout.gear,
                    loadout.attributes.recovery.boost,
                )?;
                Ok(Reload {
                    actor,
                    member: index,
                    max_hp,
                    max_tp,
                    conditions,
                    overlimit: resonance_battle::OverLimit::new(u16::from(member.overlimit) * 10)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        for reload in reloads {
            let member = &party.members[reload.member];
            battle.refresh_result_member_conditions(
                reload.actor,
                i32::from(member.hp),
                reload.max_hp,
                member.tp,
                reload.max_tp,
                reload.overlimit,
                member.ailments.petrified,
                reload.conditions,
            )?;
        }
        self.party = party;
        self.gameplay_random = random;
        let results = self.results.as_mut().unwrap();
        results.notices.push(ResultNotice::Cooking {
            character,
            recipe,
            success: meal.success,
        });
        results.cook_prompt = None;
        Ok(())
    }
}
