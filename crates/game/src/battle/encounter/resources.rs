//! Resolve native action resources for this encounter.
use crate::battle::{self, ActionResources, EffectResource};
use anyhow::{Context, Result};
use resonance_battle::{ModelDefinition, Sound};
use resonance_content::{battle_effect, prepared::Files};
use std::{collections::BTreeMap, sync::Arc};

pub(crate) struct Resources<'a, S> {
    pub files: &'a Files,
    pub catalogue: &'a resonance_content::arte::Catalogue,
    pub projectiles: resonance_content::battle_projectile::Table,
    pub tints: Option<&'a battle_effect::Tints>,
    pub voices: &'a battle::voice::Resolver<'a>,
    pub common: EffectResource,
    pub techniques: EffectResource,
    pub enemy_effects: BTreeMap<u8, EffectResource>,
    pub spell_scenes: BTreeMap<u16, EffectResource>,
    pub skill_scenes: BTreeMap<u16, EffectResource>,
    pub skill_weapons: BTreeMap<(u8, u16), Vec<Arc<resonance_battle::WeaponDefinition>>>,
    pub prepared: ActionResources,
    pub feedback: &'a mut battle::feedback::Feedback,
    pub spells: BTreeMap<u16, Arc<resonance_battle::PreparedSpell>>,
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
        let recoil = battle::recoil::Parameters::load(self.files)?;
        let normals = battle::normal::prepare_resources(
            self.voices,
            &battle::normal::Resources {
                character,
                model,
                recoil: &recoil,
                common: Some(&self.common),
                tints: self.tints,
            },
            &mut self.prepared,
            &mut self.sound,
        )
        .with_context(|| format!("prepare normal attacks for character {character}"))?;
        let normals: Vec<_> = normals
            .into_iter()
            .map(|action| bindings.insert(action))
            .collect();
        let normals = normals
            .try_into()
            .map_err(|_| anyhow::anyhow!("invalid normal attack count"))?;
        let mut techniques = BTreeMap::new();
        for (catalogue, technique) in party_techniques(self.catalogue, character)? {
            let descriptor = self.catalogue.definition(usize::from(catalogue))?;
            let flash = (!technique.is_special_guard())
                .then(|| {
                    battle::contact_feedback::admission(self.tints, descriptor.admission_flash)
                })
                .flatten();
            let action = if let Some(voice) = technique.casting_voice() {
                let cost = u16::from(descriptor.tp_cost);
                let (mut cast, feedback) = self
                    .casting(source, catalogue, voice, None)
                    .with_context(|| {
                        format!("prepare arte {catalogue} for character {character}")
                    })?;
                if let Some(model) = model
                    && !self.voices.profile(source)?.traits.body_motion_disabled
                    && let Some(steps) = self.voices.party_table().chant_sequences.get(&character)
                {
                    Arc::make_mut(&mut cast).chant = steps
                        .iter()
                        .map(|&(at, pose)| {
                            Ok((
                                at,
                                resonance_battle::AttackPose {
                                    motion: battle::model::motion(Some(model), pose.clip)
                                        .context("casting opening pose is not prepared")?,
                                    start: pose.start,
                                    end: pose.end,
                                    rate: pose.rate,
                                    blend: pose.blend,
                                    repeat: pose.repeat,
                                },
                            ))
                        })
                        .collect::<Result<_>>()?;
                }
                if character == 2 && catalogue == battle::colette_spells::HOLY_JUDGMENT {
                    const OPENING_CLIP: u16 = 13;
                    const OPENING_HOLD_TICKS: u16 = 26;
                    let cast = Arc::make_mut(&mut cast);
                    cast.hide_primary_weapons = true;
                    cast.release_pose = resonance_battle::CastReleasePose::SustainAfter {
                        opening: battle::model::motion(model, OPENING_CLIP)
                            .context("Holy Judgment opening pose is not prepared")?,
                        hold_ticks: OPENING_HOLD_TICKS,
                    };
                }
                let key = bindings.insert(battle::ActionDefinition {
                    normal: None,
                    tp_cost: cost,
                    execution: battle::ActionExecution::Casting(cast),
                });
                self.feedback.casting.insert(key, feedback);
                key
            } else {
                bindings.insert(technique.prepare(self, model).with_context(|| {
                    format!("prepare arte {catalogue} for character {character}")
                })?)
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

    /// Resolve a spell once, independently of whether an actor chants or releases it directly.
    pub(crate) fn spell(&mut self, technique: u16) -> Result<Arc<resonance_battle::PreparedSpell>> {
        Ok(if let Some(spell) = self.spells.get(&technique) {
            Arc::clone(spell)
        } else {
            let mut spell = match technique {
                battle::first_aid::CATALOGUE | battle::first_aid::KNIGHT_CATALOGUE => {
                    battle::first_aid::prepare(&self.techniques, &mut self.prepared)?
                }
                battle::fire_ball::CATALOGUE | battle::fire_ball::KNIGHT_CATALOGUE => {
                    battle::fire_ball::prepare(
                        self.files,
                        &self.projectiles,
                        &self.techniques,
                        &mut self.prepared,
                        (self.sound)(battle::voice::Sound::Cue(81))?,
                    )?
                }
                _ if battle::support_spells::native(technique).is_some() => {
                    battle::support_spells::prepare(
                        technique,
                        &self.projectiles,
                        &self.techniques,
                        &mut self.prepared,
                    )?
                }
                _ if battle::enchant_seals::native(technique).is_some() => {
                    battle::enchant_seals::prepare(
                        technique,
                        self.catalogue,
                        &self.techniques,
                        self.tints.context("missing elemental seal colors")?,
                        &mut self.prepared,
                    )?
                }
                _ if battle::summons::scene(technique).is_some() => battle::summons::prepare(
                    self.files,
                    technique,
                    self.spell_scenes
                        .get(&battle::summons::scene(technique).unwrap())
                        .context("missing summon scene artwork")?,
                    &self.common,
                    &self.techniques,
                    &mut self.prepared,
                    &mut self.sound,
                )?,
                _ if battle::stored_spells::scene(technique).is_some() => {
                    battle::stored_spells::prepare(
                        self.files,
                        technique,
                        self.spell_scenes
                            .get(&battle::stored_spells::scene(technique).unwrap())
                            .context("missing stored spell scene artwork")?,
                        &self.common,
                        &self.techniques,
                        &mut self.prepared,
                    )?
                }
                _ if battle::resident_spells::source_spell(technique).is_some() => {
                    battle::resident_spells::prepare(
                        self.files,
                        &self.projectiles,
                        technique,
                        &self.techniques,
                        &mut self.prepared,
                    )?
                }
                _ => anyhow::bail!("spell {technique} is not prepared"),
            };
            if technique == battle::elemental_spells::FREEZE_LANCER {
                spell.sound = (self.sound)(battle::voice::Sound::Cue(95))?;
            }
            let spell = Arc::new(spell);
            self.spells.insert(technique, Arc::clone(&spell));
            spell
        })
    }

    pub(crate) fn casting(
        &mut self,
        source: battle::model::ModelSource,
        technique: u16,
        voice_technique: u16,
        cast_ticks: Option<u32>,
    ) -> Result<(
        Arc<resonance_battle::CastingDefinition>,
        battle::feedback::CastingFeedback,
    )> {
        let spell = self.spell(technique)?;
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
        let release = if spell.transition_ticks > 0 {
            const STORED_PRELUDE_EFFECT: u16 = 37;
            const HOLY_JUDGMENT_PRELUDE_EFFECT: u16 = 51;
            if matches!(source, battle::model::ModelSource::Party(2))
                && technique == battle::colette_spells::HOLY_JUDGMENT
            {
                HOLY_JUDGMENT_PRELUDE_EFFECT
            } else {
                STORED_PRELUDE_EFFECT
            }
        } else {
            release
        };
        let charged = 6;
        let stored = 50;
        let appearance = |member| resonance_battle::EffectAppearance {
            resource: self.common.resource,
            member,
        };
        let feedback = battle::feedback::CastingFeedback {
            catalogue: technique,
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
                chant: vec![],
                duration: cast_ticks.unwrap_or_else(|| {
                    (i64::from(profile.cast_ticks) + i64::from(rule.cast_time_adjustment)).max(0)
                        as u32
                }),

                recovery: u32::try_from(rule.recovery_ticks)
                    .context("negative casting recovery")?,
                release_pose: Default::default(),
                hide_primary_weapons: false,
                release: spell,
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

/// Prepare implemented capacity before entry so learning can activate it in battle.
pub(super) fn party_techniques(
    catalogue: &resonance_content::arte::Catalogue,
    character: u8,
) -> Result<BTreeMap<u16, battle::martial::Technique>> {
    let guard = battle::martial::special_guard(character).context("missing party Special Guard")?;
    Ok(catalogue
        .learned_by(character)?
        .iter()
        .copied()
        .map(u16::from)
        .chain([guard.catalogue])
        .filter_map(|id| battle::martial::technique(character, id).map(|technique| (id, technique)))
        .collect())
}

/// Definition keys are produced alongside the actor's action metadata.
pub(super) struct PartyActions {
    pub normals: [battle::ActionKey; 7],
    pub techniques: BTreeMap<u16, battle::ActionKey>,
}
