//! Resolve native action resources for this encounter.
use crate::battle::{self, ActionResources, EffectResource};
use anyhow::{Context, Result};
use resonance_battle::{ModelDefinition, Sound};
use resonance_content::{battle_effect, prepared::Files};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

pub(crate) struct Resources<'a, S> {
    pub files: &'a Files,
    pub catalogue: &'a resonance_content::arte::Catalogue,
    pub projectiles: resonance_content::battle_projectile::Table,
    pub tints: Option<&'a battle_effect::Tints>,
    pub voices: &'a battle::voice::Resolver<'a>,
    pub common: EffectResource,
    pub techniques: EffectResource,
    pub enemy_effects: BTreeMap<u8, EffectResource>,
    pub prepared: ActionResources,
    pub feedback: &'a mut battle::feedback::Feedback,
    pub fire_ball: Option<Arc<resonance_battle::PreparedVolley>>,
    pub sound: S,
}

impl<S: FnMut(battle::voice::Sound) -> Result<Option<Sound>>> Resources<'_, S> {
    pub(super) fn prepare(&mut self) -> Result<()> {
        self.prepared.effect(self.common.clone())?;
        if !self.techniques.members.is_empty() {
            self.prepared.effect(self.techniques.clone())?;
        }
        Ok(())
    }

    pub(super) fn party(
        &mut self,
        character: u8,
        model: Option<&ModelDefinition>,
        bindings: &mut battle::ActionDefinitions,
    ) -> Result<PartyActions> {
        let source = battle::model::ModelSource::Party(character);
        let normals = battle::normal::prepare_resources(
            self.voices,
            &battle::normal::Resources {
                character,
                model,
                common: Some(&self.common),
                tints: self.tints,
            },
            &mut self.prepared,
            &mut self.sound,
        )?;
        let normals: Vec<_> = normals
            .into_iter()
            .map(|action| bindings.insert(action))
            .collect();
        let normals = normals
            .try_into()
            .map_err(|_| anyhow::anyhow!("invalid normal attack count"))?;
        let guard =
            battle::martial::special_guard(character).context("missing party Special Guard")?;
        let capacity: BTreeSet<_> = self
            .catalogue
            .learned_by(character)?
            .iter()
            .copied()
            .map(u16::from)
            .chain([guard.catalogue])
            .collect();
        let mut techniques = BTreeMap::new();
        for catalogue in capacity {
            let Some(technique) = battle::martial::technique(character, catalogue) else {
                continue;
            };
            let descriptor = self.catalogue.definition(usize::from(catalogue))?;
            anyhow::ensure!(
                descriptor.capabilities.family != Some(resonance_battle::ArteFamily::Finisher),
                "battle command blocking for technique {catalogue} is not prepared"
            );
            let flash = (!technique.is_special_guard())
                .then(|| {
                    battle::contact_feedback::admission(self.tints, descriptor.admission_flash)
                })
                .flatten();
            let action = if let Some(voice) = technique.casting_voice() {
                let cost = u16::from(descriptor.tp_cost);
                let (cast, feedback) = self.casting(source, catalogue, voice)?;
                let key = bindings.insert(battle::ActionDefinition {
                    normal: None,
                    tp_cost: cost,
                    execution: battle::ActionExecution::Casting(cast),
                });
                self.feedback.casting.insert(key, feedback);
                key
            } else {
                bindings.insert(technique.prepare(self, model)?)
            };
            if let Some(flash) = flash {
                self.feedback.admission_flashes.insert(action, flash);
            }
            techniques.insert(catalogue, action);
        }
        Ok(PartyActions {
            normals,
            techniques,
        })
    }

    pub(crate) fn casting(
        &mut self,
        source: battle::model::ModelSource,
        technique: u16,
        voice_technique: u16,
    ) -> Result<(
        Arc<resonance_battle::CastingDefinition>,
        battle::feedback::CastingFeedback,
    )> {
        let volley = if let Some(volley) = &self.fire_ball {
            Arc::clone(volley)
        } else {
            let volley = Arc::new(battle::fire_ball::prepare(
                self.files,
                &self.projectiles,
                &self.techniques,
                &mut self.prepared,
                (self.sound)(battle::voice::Sound::Cue(81))?,
            )?);
            self.fire_ball = Some(Arc::clone(&volley));
            volley
        };
        let rule = self.catalogue.definition(usize::from(technique))?;
        let profile = self.voices.profile(source)?;
        let mut voice = |phase| -> Result<_> {
            if matches!(source, battle::model::ModelSource::Enemy(_)) {
                return Ok(None);
            }
            self.voices
                .technique(source, voice_technique, phase, &mut self.sound)
        };
        let chant_voice =
            voice(battle::voice::Phase::Chant)?.or(voice(battle::voice::Phase::Fallback)?);
        let release_voice = voice(battle::voice::Phase::Release)?;
        let [chant, release] = rule.casting.effects.members();
        let charged = 6;
        let stored = 50;
        let appearance = |member| resonance_battle::EffectAppearance {
            resource: self.common.resource,
            member,
        };
        let feedback = battle::feedback::CastingFeedback {
            chant_voice,
            release_voice,
            start_sound: (self.sound)(Sound::Cue(109))?,
            release_sound: (self.sound)(Sound::Cue(123))?,
            chant_effect: appearance(chant),
            charged_effect: appearance(charged),
            stored_effect: appearance(stored),
            release_effect: appearance(release),
            tint: self
                .tints
                .and_then(|tints| tints.effect(usize::from(rule.element)))
                .unwrap_or_default(),
        };
        self.effect(false, &[chant, release, charged, stored])?;
        Ok((
            Arc::new(resonance_battle::CastingDefinition {
                duration: (i64::from(profile.cast_ticks) + i64::from(rule.cast_time_adjustment))
                    .max(0) as u32,

                recovery: u32::try_from(rule.recovery_ticks)
                    .context("negative casting recovery")?,
                release: volley,
                threat: Some(resonance_battle::CastingThreat {
                    catalogue: technique,
                    offensive: rule.capabilities.offensive,
                    element: rule.element,
                }),
            }),
            feedback,
        ))
    }

    pub(crate) fn effect(&mut self, technique: bool, members: &[u16]) -> Result<()> {
        let mut bank = if technique {
            self.techniques.clone()
        } else {
            self.common.clone()
        };
        bank.members = members.to_vec();
        self.prepared.effect(bank)
    }
}

/// Definition keys are produced alongside the actor's action metadata.
pub(super) struct PartyActions {
    pub normals: [battle::ActionKey; 7],
    pub techniques: BTreeMap<u16, battle::ActionKey>,
}
