use crate::{ActionId, ActorId, Battle, Cue, SpellSlot};
use anyhow::{Context, Result};

#[derive(Debug, Clone)]
pub struct SpellChargeDefinition {
    pub automatic: bool,
    pub enabled: bool,
}

impl Battle {
    pub(crate) fn spell_target(&self, actor: ActorId, release: &crate::PreparedSpell) -> ActorId {
        let runtime = &self.runtime[actor.index()];
        if release.uses_support_target() {
            runtime.support_target
        } else {
            runtime.target
        }
    }

    pub(crate) fn store_cast_requested(
        &mut self,
        actor: ActorId,
        action: crate::ActionKey,
    ) -> Result<bool> {
        let index = actor.index();
        if self.actors[index].stored_spell.is_some() {
            return Ok(false);
        }
        let Some(binding) = self.prepared.actor_setup[index]
            .spell_charge
            .as_ref()
            .filter(|binding| binding.enabled)
        else {
            return Ok(false);
        };
        let cast = self
            .prepared
            .casting(action)
            .context("stored cast is not prepared")?;
        Ok(match self.actors[index].control {
            crate::Control::Manual | crate::Control::SemiAuto => self
                .cast_inputs
                .get(usize::from(self.actors[index].control_slot))
                .is_some_and(|input| input.attack),
            crate::Control::Auto => {
                cast.threat.is_some_and(|threat| threat.offensive)
                    && binding.automatic
                    && self.random.next_u16() & 1 != 0
            }
            crate::Control::Enemy => false,
        })
    }

    pub(crate) fn store_committed_cast(
        &mut self,
        actor: ActorId,
        action: crate::ActionKey,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        let index = actor.index();
        cues.push(Cue::Casting {
            actor,
            action,
            phase: crate::CastPhase::Stored,
        });
        self.actors[index].casting_state.previous_spell = Some(action);
        self.actors[index].stored_spell = Some(action);
        let owner = &self.actors[index];
        cues.push(Cue::ExSkillLabel {
            actor,
            position: owner.condition_label_origin(),
        });
        self.stop_voice(actor, cues);
        if self.has_pending_item(actor)
            || self.cast_command_changed(actor, action)
            || self.special_guard_threat_pending(actor)
        {
            self.clear_cast_memory(actor);
        }
        Ok(())
    }

    pub(crate) fn discharge_stored_spell(
        &mut self,
        parent: ActionId,
        actor: ActorId,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        let index = actor.index();
        let Some(action) = self.actors[index].stored_spell else {
            return Ok(());
        };
        if self.phase() != crate::BattlePhase::Combat
            || self.spell_active(actor, SpellSlot::Primary)
        {
            return Ok(());
        }
        if !self.dispatch_spell(actor, action, SpellSlot::Primary, Some(parent), cues)? {
            return Ok(());
        }
        self.actors[index].casting_state.power_percent = 100;
        self.actors[index].stored_spell = None;
        Ok(())
    }

    /// Starts a resident in the requested independent slot. Failed admission has no side effects.
    pub(crate) fn dispatch_spell(
        &mut self,
        actor: ActorId,
        action: crate::ActionKey,
        slot: SpellSlot,
        parent: Option<ActionId>,
        cues: &mut Vec<Cue>,
    ) -> Result<bool> {
        let casting = self
            .prepared
            .casting(action)
            .context("spell release requires a casting action")?;
        let release = casting.release.clone();
        let phase = if release.transition_ticks > 0 {
            crate::CastPhase::SceneOpening
        } else {
            crate::CastPhase::Released
        };
        let release_pose = casting.release_pose;
        let target = self.spell_target(actor, &release);
        let started = if let Some(summon) = &release.summon {
            slot == SpellSlot::Primary
                && self.begin_summon_entry(summon.entry, release, actor, target, parent, cues)?
        } else {
            let resident = self.release_volley(release, actor, target, slot, parent, cues)?;
            if let Some(resident) = resident {
                self.volleys.get_mut(&resident).unwrap().scene_action = (phase
                    == crate::CastPhase::SceneOpening
                    && !(parent.is_some()
                        && matches!(release_pose, crate::CastReleasePose::Scene { .. })))
                .then_some(action);
            }
            if let (
                Some(resident),
                Some(parent),
                crate::CastReleasePose::Scene {
                    opening,
                    release,
                    release_loop_start,
                    at,
                },
            ) = (resident, parent, release_pose)
            {
                self.volleys.get_mut(&resident).unwrap().cast_pose =
                    Some(crate::release::CastPoseTransition {
                        parent,
                        action,
                        opening: opening.map(|pose| pose.motion),
                        release,
                        release_loop_start,
                        at,
                    });
            }
            resident.is_some()
        };
        if !started {
            return Ok(false);
        }
        cues.push(Cue::Casting {
            actor,
            action,
            phase,
        });
        Ok(true)
    }

    pub(crate) fn try_spell_revenge(
        &mut self,
        actor: ActorId,
        inputs: &[crate::ControlInput],
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        let index = actor.index();
        let reaction = &self.actors[index].reaction;
        if !self.actors[index].equipment.spell_revenge
            || reaction.spell_revenge_used
            || reaction.recoil.kind == crate::RecoilKind::Normal
            || self.spell_active(actor, SpellSlot::Secondary)
        {
            return Ok(());
        }
        let slot = self.actors[index].control_slot;
        let Some(input) = inputs.iter().find(|input| {
            self.actors[input.actor.index()].control_slot == slot && input.technique.pressed
        }) else {
            return Ok(());
        };
        let direction = crate::NormalAttack::from_input(input.stick, false);
        let Some(action) = self.runtime[index]
            .control
            .as_ref()
            .and_then(|control| self.shortcut_action(actor, control.shortcuts[direction as usize]))
        else {
            return Ok(());
        };
        let Some(command) = self.prepared.technique(actor, action) else {
            return Ok(());
        };
        if !command.capabilities.spell
            || command.capabilities.family != Some(crate::ArteFamily::Basic)
        {
            return Ok(());
        }
        if self.dispatch_spell(actor, action, SpellSlot::Secondary, None, cues)? {
            self.actors[index].casting_state.power_percent = 100;
            self.actors[index].reaction.spell_revenge_used = true;
        }
        Ok(())
    }
}
