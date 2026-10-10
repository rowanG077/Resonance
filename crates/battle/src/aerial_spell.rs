//! Release an eligible basic spell while chaining an aerial attack.
use crate::{ActionId, ActorId, Battle, Control, Cue};
use anyhow::Result;

impl Battle {
    pub(crate) fn can_use_aerial_spell(&self, actor: ActorId, action: crate::ActionKey) -> bool {
        let index = actor.index();
        self.prepared.actor_setup[index].aerial_spells
            && matches!(
                self.actors[index].control,
                Control::Manual | Control::SemiAuto
            )
            && self.actors[index].airborne()
            && self
                .prepared
                .technique(actor, action)
                .is_some_and(|command| {
                    command.capabilities.spell
                        && command.capabilities.family == Some(crate::ArteFamily::Basic)
                })
            && self.prepared.casting(action).is_some()
    }

    /// Admit first, then release and debit exactly once. The current aerial action continues.
    pub(crate) fn try_aerial_spell_selection(
        &mut self,
        actor: ActorId,
        action: crate::ActionKey,
        parent: Option<ActionId>,
        cues: &mut Vec<Cue>,
    ) -> Result<bool> {
        let index = actor.index();
        if !self.can_use_aerial_spell(actor, action)
            || self.spell_active(actor, crate::SpellSlot::Secondary)
            || !self.technique_available(actor, action)
            || !self.actors[index].conditions.arte_queue_allowed()
            || !self.action_family_available(actor, action)
            || !self.chain_count_available(actor, self.runtime[index].combo.normal_links)
        {
            return Ok(false);
        }
        let quote = self.action_quote(actor, action);
        if u32::from(self.actors[index].tp) < quote {
            return Ok(false);
        }
        let (cost, lucky) = crate::tp::commit_spell_cost(&self.actors[index], quote, || {
            self.random.next_u16() % 100
        });
        self.actors[index].tp -= cost as u16;
        self.record_immediate_spell_use(actor, action)?;
        if lucky {
            let owner = &self.actors[index];
            cues.push(Cue::ExSkillLabel {
                actor,
                position: std::array::from_fn(|axis| {
                    owner.position[axis] + owner.body.center_offset[axis] * 2.
                }),
            });
        }
        self.actors[index].attack_power = 100;
        self.dispatch_spell(actor, action, crate::SpellSlot::Secondary, parent, cues)
    }
}
