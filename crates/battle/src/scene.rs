//! Two stored-spell slots and their end-of-update transition (3EB74/3E42C).
//! The casting script owns motion, costs and release cues. This module owns the
//! shared pause, slot lifetime and dispatch boundary.
use crate::{ActionId, ActorId, Battle, Cue, SpellSlot, script::step_sequence};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub struct SceneFrame {
    pub slot: u8,
    pub actor: ActorId,
    pub spell: u16,
    /// None after activation; zero still holds until the script activates it.
    pub remaining: Option<u16>,
}

pub(crate) struct Scene {
    pub actor: ActorId,
    parent: ActionId,
    spell: u16,
    phase: Phase,
    models: BTreeMap<(u32, u8), crate::PreparedEffectModel>,
}

enum Phase {
    Transition(u16),
    Active(ActionId),
}

impl Battle {
    pub(crate) fn transition_owner(&self) -> Option<ActorId> {
        self.scenes
            .iter()
            .flatten()
            .find_map(|scene| matches!(scene.phase, Phase::Transition(_)).then_some(scene.actor))
    }

    pub(crate) fn scene_available(&self) -> bool {
        self.transition_owner().is_none() && self.scenes.iter().any(Option::is_none)
    }

    pub(crate) fn begin_scene(
        &mut self,
        parent: ActionId,
        actor: ActorId,
        spell: u16,
        duration: u16,
    ) -> Result<()> {
        ensure!(
            duration > 0 && duration <= i16::MAX as u16,
            "invalid scene transition duration"
        );
        ensure!(
            self.transition_owner().is_none(),
            "scene transition is already active"
        );
        ensure!(
            !self.spell_active(actor, SpellSlot::Primary),
            "primary spell slot is busy"
        );
        let definition = self
            .prepared
            .actions
            .iter()
            .find(|action| action.id == spell)
            .context("unprepared scene spell")?;
        let mut models = BTreeMap::new();
        for resource in &definition.resources {
            if let crate::ResourceBinding::Effect(bank) = resource {
                for (&slot, model) in &self.prepared.effects[bank].models {
                    models.insert((*bank, slot), model.clone());
                }
            }
        }
        let slot = self
            .scenes
            .iter_mut()
            .find(|slot| slot.is_none())
            .context("stored scene slots are busy")?;
        *slot = Some(Scene {
            actor,
            parent,
            spell,
            phase: Phase::Transition(duration),
            models,
        });
        if let Some(camera) = &mut self.camera {
            camera.phase = crate::camera::Phase::Entry;
        }
        Ok(())
    }

    pub(crate) fn scene_remaining(&self, parent: ActionId) -> Result<u16> {
        self.scenes
            .iter()
            .flatten()
            .find_map(|scene| match scene.phase {
                Phase::Transition(remaining) if scene.parent == parent => Some(remaining),
                _ => None,
            })
            .context("action does not own a scene transition")
    }

    pub(crate) fn activate_scene(
        &mut self,
        parent: ActionId,
        target: ActorId,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        let index = self
            .scenes
            .iter()
            .position(|scene| {
                scene.as_ref().is_some_and(|scene| {
                    scene.parent == parent && matches!(scene.phase, Phase::Transition(0))
                })
            })
            .context("scene transition is not ready")?;
        let scene = self.scenes[index].as_ref().unwrap();
        let resident = self
            .release(
                scene.spell,
                scene.actor,
                target,
                SpellSlot::Primary,
                parent,
                cues,
            )?
            .context("primary spell slot is busy")?;
        self.scenes[index].as_mut().unwrap().phase = Phase::Active(resident);
        if let Some(camera) = &mut self.camera {
            camera.phase = crate::camera::Phase::Returning;
        }
        Ok(())
    }

    pub(crate) fn clean_scenes(&mut self) {
        let mut expired_scenes = Vec::new();
        for slot in &mut self.scenes {
            let expired = slot.as_ref().is_some_and(|scene| {
                let owner = match scene.phase {
                    Phase::Transition(_) => scene.parent,
                    Phase::Active(resident) => resident,
                };
                !self.sequences.contains_key(&owner)
            });
            if expired && let Some(scene) = slot.take() {
                // 37DD8 restores every actor after active stored-scene cleanup.
                if matches!(scene.phase, Phase::Active(_)) {
                    self.actors_visible = true;
                } else if let Some(camera) = &mut self.camera {
                    camera.phase = crate::camera::Phase::Returning;
                }
                expired_scenes.push(scene.parent);
            }
        }
        self.particles.retain(|_, particle| {
            particle
                .scene
                .is_none_or(|id| !expired_scenes.contains(&id))
        });
        self.sequences.retain(|_, sequence| {
            sequence
                .effect
                .as_ref()
                .and_then(|effect| effect.scene)
                .is_none_or(|id| !expired_scenes.contains(&id))
        });
    }

    pub(crate) fn scene_owner(&self, action: ActionId) -> Option<ActionId> {
        self.scenes.iter().flatten().find_map(|scene| {
            (scene.parent == action || matches!(scene.phase, Phase::Active(id) if id == action))
                .then_some(scene.parent)
        })
    }

    pub(crate) fn play_effect_model(
        &mut self,
        scene: ActionId,
        binding: crate::EffectMotionBinding,
    ) -> Result<()> {
        self.scene_model(scene, binding.bank, binding.model)?
            .play(binding.clip)
    }

    fn scene_model(
        &mut self,
        scene: ActionId,
        bank: u32,
        model: u8,
    ) -> Result<&mut crate::PreparedEffectModel> {
        self.scenes
            .iter_mut()
            .flatten()
            .find(|slot| slot.parent == scene)
            .context("stale scene handle")?
            .models
            .get_mut(&(bank, model))
            .context("unprepared scene model")
    }

    pub(crate) fn set_particle_model(
        &mut self,
        handle: i32,
        action: ActionId,
        model: u8,
    ) -> Result<()> {
        let particle = self
            .owned_particle(handle, action)
            .map_err(anyhow::Error::msg)?;
        ensure!(particle.model.is_some(), "particle has no model binding");
        let scene = particle.scene.context("model particle needs a scene")?;
        let bank = particle.frame.resource;
        self.scene_model(scene, bank, model)?;
        self.particles
            .get_mut(&crate::ParticleId(handle))
            .unwrap()
            .model = Some(model);
        Ok(())
    }

    pub(crate) fn advance_effect_models(&mut self, cues: &mut Vec<Cue>) -> Result<()> {
        let advance = self.transition_owner().is_none();
        let ids: Vec<_> = self
            .particles
            .iter()
            .filter(|(_, particle)| particle.initialized)
            .map(|(&id, _)| id)
            .collect();
        for id in ids {
            let particle = self.particles.get_mut(&id).unwrap();
            let result = (|| {
                if let Some(model) = &mut particle.own_model {
                    particle.frame.model = Some(model.step(&particle.frame, advance)?);
                    return Ok(());
                }
                let Some(model) = particle.model else {
                    return Ok(());
                };
                let owner = particle.scene.context("model particle needs a scene")?;
                let scene = self
                    .scenes
                    .iter_mut()
                    .flatten()
                    .find(|scene| scene.parent == owner)
                    .context("stale scene handle")?;
                let model = scene
                    .models
                    .get_mut(&(particle.frame.resource, model))
                    .context("unprepared scene model")?;
                particle.frame.model = Some(model.step(&particle.frame, advance)?);
                Ok(())
            })();
            if let Err(error) = result {
                self.diagnostics.report("battle particle model", error)?;
                self.diagnostic = true;
                self.particles.remove(&id);
                cues.push(Cue::ParticleExpired { particle: id });
            }
        }
        Ok(())
    }

    pub(crate) fn hide_actors(&mut self, resident: ActionId) -> Result<()> {
        ensure!(
            self.scenes
                .iter()
                .flatten()
                .any(|scene| matches!(scene.phase, Phase::Active(id) if id == resident)),
            "hide_actors needs an active stored scene"
        );
        self.actors_visible = false;
        Ok(())
    }

    pub(crate) fn advance_scene(&mut self, cues: &mut Vec<Cue>) -> Result<()> {
        let Some(parent) =
            self.scenes.iter().flatten().find_map(|scene| {
                matches!(scene.phase, Phase::Transition(_)).then_some(scene.parent)
            })
        else {
            return Ok(());
        };
        let mut sequence = self
            .sequences
            .remove(&parent)
            .context("missing scene transition owner")?;
        // 3E42C runs after contacts/residents, including the entry update. The
        // ordinary actor clock is held; only this script's continuation runs.
        if let Err(error) = step_sequence(self, parent, &mut sequence, cues) {
            self.diagnostics.report("battle scene action", error)?;
            self.diagnostic = true;
            self.discard_action(parent, sequence.actor, sequence.definition.phase, cues);
            self.clean_scenes();
            return Ok(());
        }
        if sequence.finished {
            self.actors[sequence.actor.index()].activity = crate::Activity::Idle;
            self.actors[sequence.actor.index()].reaction.armor.reset();
            cues.push(Cue::Completed { action: parent });
        } else {
            self.sequences.insert(parent, sequence);
        }
        // 1C40 decrements after the stored callback. Activation at zero leaves
        // the new resident pending until the next update's resident dispatch.
        for scene in self.scenes.iter_mut().flatten() {
            if let Phase::Transition(remaining) = &mut scene.phase {
                *remaining = remaining.saturating_sub(1);
            }
        }
        self.clean_scenes();
        Ok(())
    }

    pub(crate) fn scene_frames(&self) -> Vec<SceneFrame> {
        self.scenes
            .iter()
            .enumerate()
            .filter_map(|(slot, scene)| {
                let scene = scene.as_ref()?;
                Some(SceneFrame {
                    slot: slot as u8,
                    actor: scene.actor,
                    spell: scene.spell,
                    remaining: match scene.phase {
                        Phase::Transition(remaining) => Some(remaining),
                        Phase::Active(_) => None,
                    },
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
