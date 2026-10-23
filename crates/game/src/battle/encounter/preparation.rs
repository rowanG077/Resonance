//! Prepare one complete generation without changing the suspended session.
use super::{Inputs, Prepared, RenderModel, assets::EquipmentKind, render, resources::Resources};
use crate::battle::lifecycle::victory_selection::VictoryGroupId;
use crate::battle::model::MotionRole;
use crate::battle::party::Character;
use crate::battle::{self, EffectResource, command, entry, model};
use anyhow::{Context, Result, ensure};
use resonance_battle::TechniqueId;
use resonance_battle::WeaponSlot;
use resonance_battle::{Difficulty, ResourceId, Sound, SpellChargeDefinition, WeaponDefinition};
use resonance_content::{
    battle_model, battle_profile, menu_data::MenuData, model_preview::PreviewPart,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

const HAMMER_MODEL: u8 = 1;
const HAMMER_EFFECT: u16 = 14;
const COLETTE_WING_CENTER_RISE: f32 = 10.;

/// Source state observed when the field asks to enter battle. The independent
/// battle seed is captured once; preparation may be retried without new draws.
pub struct PrepareOptions {
    pub event_flags: BTreeSet<u16>,
    pub hunting_symbol: Option<u32>,
    /// Saved event flag 0x3FA; story acquisition remains field-owned.
    pub devils_arms_unlocked: bool,
    pub random_seed: u64,
    pub map: u16,
    pub world_music: i32,
    pub story: i32,
    pub colette_state: i32,
    pub victory_story_flags: [bool; 2],
    pub overlimit_boost: bool,
}

struct Handles {
    resource: ResourceId,
}
impl Handles {
    fn new(first_resource: ResourceId) -> Self {
        Self {
            resource: first_resource,
        }
    }

    fn resource(&mut self) -> ResourceId {
        let id = self.resource;
        self.resource.0 += 1;
        id
    }
}

struct EquipmentAttachments {
    models: Vec<RenderModel>,
    render_trails: Vec<super::RenderTrail>,
    weapons: Vec<Arc<resonance_battle::WeaponDefinition>>,
}

impl Inputs {
    fn prepare_weapon(
        &self,
        character: Character,
        profile: &battle_profile::Profile,
        source: &battle_model::Party,
        weapon: &battle_model::Weapon,
        shield: bool,
        handles: &mut Handles,
    ) -> Result<EquipmentAttachments> {
        // Sheena's four card attachments share two prepared model parts.
        let slots: Vec<_> = if character == Character::Sheena && !shield {
            (0..4).map(|slot| (slot, slot / 2)).collect()
        } else {
            weapon.parts.keys().map(|&part| (part, part)).collect()
        };
        let mut prepared = EquipmentAttachments {
            models: Vec::new(),
            render_trails: Vec::new(),
            weapons: Vec::new(),
        };
        for (slot, part_index) in slots {
            let slot = slot
                .checked_add(u8::from(shield))
                .context("invalid weapon slot")?;
            let bone = *source
                .body
                .attachments
                .get(&slot)
                .context("missing weapon attachment")?;
            let part = weapon
                .parts
                .get(&part_index)
                .context("missing equipped weapon part")?;
            let resources = render_weapon_layers(
                part,
                *profile
                    .weapon_styles
                    .get(usize::from(slot))
                    .context("missing weapon style")?,
                handles,
                &mut prepared.models,
            );
            let playback = (character == Character::Genis).then(battle::weapon::owner_linked);
            let definition = battle::weapon::prepare(
                &self.files,
                part,
                slot,
                bone,
                &resources,
                playback,
                weapon.uv_animation.get(&part_index),
            )?;
            let resource = definition.primary_resource();
            prepared.weapons.push(definition);
            if let Some(&material) = weapon.trails.get(&part_index)
                && let Some(render) = render::weapon_trail(
                    self.files.diagnostics(),
                    &part.rig.skeleton,
                    resource,
                    material,
                    true,
                    self.common_effects.as_deref(),
                    None,
                )?
            {
                prepared.render_trails.push(render);
            }
        }
        if shield {
            ensure!(
                prepared.weapons.len() == 1 && prepared.weapons[0].slot == Some(WeaponSlot(1)),
                "prepared shield has no attachment"
            );
        }
        Ok(prepared)
    }
}

pub(super) struct ActorRow<'a> {
    actor: resonance_battle::Actor,
    profile: &'a battle_profile::Profile,
    actions: Option<super::resources::PartyActions>,
    strategy: resonance_battle::TargetPolicy,
    quick_item: bool,
    extended_overlimit: bool,
    pub(super) source: model::ModelSource,
    pub(super) model: Option<Arc<resonance_battle::ModelDefinition>>,
    setup: resonance_battle::ActorSetup,
}

impl ActorRow<'_> {
    fn prepare_control(&mut self, inputs: &Inputs, difficulty: Difficulty) -> Result<()> {
        self.setup.overlimit_gain = self.profile.overlimit_gain;
        self.setup.extended_overlimit = self.extended_overlimit;
        self.setup.wait_for_defeat_animation = self.profile.traits.wait_for_defeat_animation;
        self.setup.stationary_defeat =
            self.profile.death_motion == 0 && !self.profile.traits.retain_defeated_body;
        let party = &inputs.party;
        let table = &inputs.party_profiles;
        let catalogue = &inputs.catalogue;
        let ActorRow {
            profile,
            actions,
            source,
            model: artwork,
            setup,
            ..
        } = self;
        setup.decision = Some(resonance_battle::DecisionDefinition {
            idle_ticks: u32::from(profile.idle_ticks),
            idle_variation: u32::from(profile.idle_variation),
        });
        let model = artwork.as_deref();
        let enemy = matches!(*source, model::ModelSource::Enemy(_));
        if let Some(model) = model {
            let motion =
                battle::model::common_motion(Some(model), battle::model::MotionRole::Revival);
            ensure!(enemy || motion.is_some(), "missing party revival motion");
            if let Some(motion) = motion {
                let pose = resonance_battle::AttackPose {
                    motion,
                    start: 0.,
                    end: None,
                    repeat: false,
                    rate: 0.5,
                    blend: 4,
                };
                let duration = model.motions[&motion.clip].duration_frames / pose.rate;
                // Retain the final pose through completion, then initialize idle next visit.
                let ticks = duration.floor() as u32 + u32::from(pose.blend) + 2;
                setup.revival = Some((pose, ticks));
            }
        }
        setup.contact_recovery = !enemy || profile.traits.enemy_contact_recovery;
        setup.recovery_return = Some(resonance_battle::RecoveryReturnDefinition {
            speed: if enemy {
                profile.walk_speed
            } else {
                profile.run_speed
            },
            run_speed: profile.run_speed,
            turn_ticks: profile.turn_ticks,
            disabled: enemy && difficulty != Difficulty::Normal,
        });
        if let model::ModelSource::Party(character) = *source {
            let actions = actions.as_ref().context("missing party actions")?;
            let techniques = &actions.techniques;
            setup.arte_chain_limit = matches!(
                character,
                battle::party::Character::Zelos | battle::party::Character::Kratos
            )
            .then_some(3);
            setup.techniques = battle::control::techniques(catalogue, character, techniques)?;
            let member = &party.members[character.index()];
            setup.disabled_techniques = member
                .disabled_techniques
                .iter()
                .filter_map(|catalogue| techniques.get(catalogue).copied())
                .collect();
            setup.companion = Some(battle::companion::prepare(table, character, member));
            let mut definition = battle::control::party(
                profile,
                table
                    .normal_attacks
                    .get(character.index())
                    .context("missing normal attack table")?,
                actions.normals,
                battle::control::motions(model),
            )?;
            definition.shortcuts = member
                .shortcuts
                .map(|id| (id != 0).then_some(TechniqueId(id)));
            let guard = battle::martial::special_guard(character);
            setup.special_guard = Some(techniques[&guard.catalogue]);
            setup.control = Some(Arc::new(definition));
        }
        Ok(())
    }
    fn prepare_feedback(
        &mut self,
        voices: &battle::voice::Resolver<'_>,
        common_id: ResourceId,
        unison_ready: Option<Sound>,
        feedback: &mut battle::feedback::Feedback,
        mut sound: impl FnMut(Sound) -> Result<Option<Sound>>,
    ) -> Result<()> {
        let ActorRow {
            setup,
            profile,
            source,
            model: artwork,
            ..
        } = self;
        let mut controls = if let model::ModelSource::Party(character) = *source {
            battle::feedback::ActorFeedback {
                acquisition_motion: model::common_motion(
                    artwork.as_deref(),
                    model::MotionRole::Acquisition,
                ),
                overlimit: None,
                overlimit_voice: voices.absolute(
                    *source,
                    voices
                        .party_table()
                        .overlimit_voices
                        .get(character.index())
                        .copied()
                        .flatten(),
                    &mut sound,
                )?,
                technique_command: voices.select(*source, |v| v.technique_command, &mut sound)?,
                taunt: voices.select(*source, |v| v.taunt, &mut sound)?,
                charge: voices.absolute(*source, Some(Sound::Stream(753)), &mut sound)?,
                charge_failed: voices.absolute(*source, Some(Sound::Cue(1258)), &mut sound)?,
                backstep: sound(Sound::Cue(137))?,
                knockdown: sound(Sound::Cue(72))?,
            }
        } else {
            Default::default()
        };
        ensure!(
            profile.overlimit_scale.is_finite() && profile.overlimit_scale >= 0.,
            "invalid Over Limit effect scale"
        );
        if let Some(model) = artwork {
            let bone = match source {
                model::ModelSource::Party(_) => model
                    .skeleton
                    .bones
                    .iter()
                    .position(|bone| bone.name == "Bone_sebone01")
                    .context("party model is missing its Over Limit body anchor")?,
                _ => usize::from(profile.overlimit_bone),
            };
            ensure!(
                bone < model.skeleton.bones.len(),
                "Over Limit effect bone is missing"
            );
            controls.overlimit = Some(battle::feedback::OverLimitFeedback {
                appearance: resonance_battle::EffectAppearance {
                    resource: common_id,
                    member: 45,
                },
                bone: bone
                    .try_into()
                    .context("Over Limit effect bone exceeds u8")?,
                scale: profile.overlimit_scale,
            });
        }
        feedback.actors.push(controls);
        let expressions = !profile.texture_channels.is_empty();
        let rescue = battle::feedback::RescueFeedback {
            motion: (!profile.traits.body_motion_disabled)
                .then(|| model::common_motion(artwork.as_deref(), MotionRole::Hurt))
                .flatten(),
            expression: expressions.then_some(profile.rescue_expression),
            appearance: resonance_battle::EffectAppearance {
                resource: common_id,
                member: 19,
            },
            sound: unison_ready,
        };
        feedback.breakfalls.push(if setup.contact_recovery {
            Some(battle::feedback::BreakfallFeedback {
                effect: resonance_battle::EffectAppearance {
                    resource: common_id,
                    member: 15,
                },
                voice: voices.select(*source, |v| v.contact_recovery, &mut sound)?,
                sound: sound(Sound::Cue(137))?,
            })
        } else {
            None
        });
        feedback.rescues.push(rescue);
        Ok(())
    }
}

/// One complete actor and its equipped components, prepared from the encounter snapshot.
pub struct PreparedPartyMember {
    pub loadout: battle::party::Loadout,
    pub character: Character,
    pub actor: resonance_battle::Actor,
    pub model: Option<Arc<resonance_battle::ModelDefinition>>,
    equipment: BTreeMap<u16, Vec<Arc<resonance_battle::WeaponDefinition>>>,
    performances: Vec<battle::victory::Performance>,
    models: Vec<RenderModel>,
    render_trails: Vec<super::RenderTrail>,
    pub next_resource: ResourceId,
}

/// One selected enemy instance with its body and carried artwork.
pub struct PreparedEnemyMember {
    pub actor: resonance_battle::Actor,
    pub model: Option<Arc<resonance_battle::ModelDefinition>>,
    pub next_resource: ResourceId,
    models: Vec<RenderModel>,
    render_trails: Vec<super::RenderTrail>,
    weapon_groups: Vec<Vec<Arc<WeaponDefinition>>>,
}

impl Inputs {
    pub fn prepare_enemy_member(
        &self,
        spawn_index: usize,
        resource: ResourceId,
        first_resource: ResourceId,
        random: &mut resonance_battle::Random,
    ) -> Result<PreparedEnemyMember> {
        let difficulty = Difficulty::try_from(self.party.settings.preferences.battle_rank)?;
        let spawn = self
            .enemies
            .spawns
            .get(spawn_index)
            .context("missing enemy spawn")?;
        if let Some(reason) = &spawn.unsupported_reason {
            anyhow::bail!("{reason}");
        }
        let monster = &self.enemies.resources[spawn.resource].monster;
        let definition = &self.enemy_definitions[&monster.id];
        let stats = battle::enemy::statistics(
            monster
                .statistics
                .get(spawn.variant)
                .context("enemy statistics variant is not authored")?,
            difficulty,
        );
        let mut actor = battle::enemy::actor(
            monster,
            &definition.profile,
            definition.guard_recovery_bonus,
            stats,
            &self.recoil,
        )?;
        if monster.id == 191 {
            actor.body.scale = battle::enemy_stage::sword_scale(spawn.variant, actor.body.scale)?;
        }
        let mut model = self
            .enemy_models
            .get(&monster.id)
            .map(|(source, motions)| {
                let body = (|| {
                    ensure!(
                        definition.profile.traits.secondary_body_entry
                            == source.secondary_body.is_some(),
                        "secondary body metadata is not prepared"
                    );
                    let scene = &monster
                        .preview
                        .parts
                        .first()
                        .context("missing enemy body scene")?
                        .scene;
                    model::body(
                        &self.files,
                        &source.body,
                        scene,
                        &definition.profile,
                        motions.clone(),
                        &actor,
                        model::ModelSetup {
                            resource,
                            initial: entry::initial_playback(
                                random,
                                &self.party_profiles.entry,
                                &definition.profile,
                                actor.side,
                                motions,
                                (actor.hp, actor.equipment.max_hp),
                                false,
                            )?,
                            suppress_root_translation: [false; 3],
                        },
                    )
                })();
                self.files.diagnostics().attempt("enemy body artwork", body)
            })
            .transpose()?
            .flatten();
        let mut handles = Handles::new(first_resource);
        let mut models = Vec::new();
        let mut render_trails = Vec::new();
        let mut weapon_groups = Vec::new();
        if let Some(model) = &mut model {
            let source = &self.enemy_models[&monster.id].0;
            let mut body_parts = render::enemy_body_parts(
                &monster.preview.parts,
                source.body_variant,
                spawn.texture_variant,
            )?;
            if let Some(secondary) = &source.secondary_body {
                body_parts.retain(|part| {
                    !secondary
                        .model
                        .layers
                        .iter()
                        .any(|layer| layer.scene.mesh == part.scene.mesh)
                });
            }
            models.push(RenderModel {
                resource,
                skeleton: source.body.skeleton.clone(),
                parts: body_parts,
                instances: render::RenderInstances::Fixed(1),
                lit: true,
                texture_channels: definition.profile.texture_channels.clone(),
                target_anchor: Some(render::TargetAnchor::prepare(
                    &definition.profile.target_anchor,
                    &source.body.skeleton,
                )),
                suppressed_nodes: render::suppressed_body_nodes(
                    &source.body.skeleton,
                    definition.profile.traits.show_body_attachments,
                ),
                weapon_style: None,
            });
            if let Some(secondary) = &source.secondary_body {
                let primary = secondary
                    .model
                    .layers
                    .first()
                    .context("secondary body has no layers")?;
                let attachment = primary
                    .attached_to
                    .as_ref()
                    .map(|name| {
                        source
                            .body
                            .skeleton
                            .bones
                            .iter()
                            .position(|bone| &bone.name == name)
                            .map(|bone| bone as u16)
                            .context("secondary body attachment is absent")
                    })
                    .transpose()?;
                let idle = if definition.profile.initial_motion_override != 0 {
                    definition.profile.initial_motion_override
                } else {
                    definition.profile.initial_motion
                };
                let resting = model.initial.clip == u16::from(idle);
                let clip = if resting {
                    if definition.profile.initial_motion_override != 0 {
                        u16::from(definition.profile.initial_motion_override)
                    } else {
                        secondary.motion_offset
                    }
                } else {
                    model
                        .initial
                        .clip
                        .checked_add(secondary.motion_offset)
                        .context("secondary body entry clip overflow")?
                };
                let duration = primary
                    .scene
                    .clips
                    .iter()
                    .find(|motion| motion.resource_slot == clip)
                    .context("secondary body entry motion is absent")?
                    .duration_seconds
                    * resonance_content::animation::FRAME_HZ;
                ensure!(
                    duration.is_finite() && duration > 0.,
                    "invalid secondary body entry duration"
                );
                let initial = resonance_battle::Playback {
                    clip,
                    frame: if resting && model.initial.frame == 0. {
                        f32::from(random.next_u16()) % duration
                    } else {
                        model.initial.frame
                    },
                    ..model.initial
                };
                let mut secondary_models = Vec::new();
                let resources = render_weapon_layers(
                    &secondary.model,
                    Default::default(),
                    &mut handles,
                    &mut secondary_models,
                );
                for render in &mut secondary_models {
                    render.weapon_style = None;
                }
                let component = battle::weapon::secondary_body(
                    &self.files,
                    secondary,
                    &resources,
                    attachment,
                    initial,
                )?;
                Arc::get_mut(model)
                    .context("enemy body shared before secondary body")?
                    .weapons
                    .push(component);
                models.extend(secondary_models);
            }
            ensure!(
                spawn.weapon_variant == 0 || source.attachments.contains_key(&0),
                "enemy weapon variant has no equipped attachment"
            );
            let mut prepare_weapon = |slot,
                                      part: &battle_model::ModelPart,
                                      trail: Option<&battle_model::TrailMaterial>,
                                      animation: Option<&battle_model::AttachmentAnimation>|
             -> Result<Option<Arc<WeaponDefinition>>> {
                let mut render = Vec::new();
                let prepared = (|| {
                    let style = *definition
                        .profile
                        .weapon_styles
                        .get(usize::from(slot))
                        .context("missing enemy weapon style")?;
                    let resources = render_weapon_layers(part, style, &mut handles, &mut render);
                    let bone = *source
                        .body
                        .attachments
                        .get(&slot)
                        .context("missing enemy attachment bone")?;
                    let playback = part
                        .layers
                        .first()
                        .context("enemy attachment has no primary layer")?
                        .selected_clip()?
                        .map(|(_, clip)| {
                            resonance_battle::WeaponPlayback::Local(resonance_battle::Playback {
                                clip: clip.resource_slot,
                                frame: 0.,
                                rate: 0.5,
                                repeat: true,
                            })
                        });
                    let mut weapon = battle::weapon::prepare(
                        &self.files,
                        part,
                        slot,
                        bone,
                        &resources,
                        playback,
                        animation,
                    )?;
                    Arc::get_mut(&mut weapon)
                        .context("enemy attachment shared before initialization")?
                        .initially_visible = definition
                        .weapon_visibility
                        .get(&slot)
                        .is_none_or(|visibility| visibility.visible(spawn_index));
                    Ok(weapon)
                })();
                let Some(weapon) = self
                    .files
                    .diagnostics()
                    .attempt("enemy attachment", prepared)?
                else {
                    return Ok(None);
                };
                let resource = weapon.primary_resource();
                models.extend(render);
                if let Some(&material) = trail
                    && let Some(render) = render::weapon_trail(
                        self.files.diagnostics(),
                        &part.rig.skeleton,
                        resource,
                        material,
                        false,
                        self.common_effects.as_deref(),
                        self.enemy_effects.get(&monster.id).map(Arc::as_ref),
                    )?
                {
                    render_trails.push(render);
                }
                Ok(Some(weapon))
            };
            let mut prepared_weapons = BTreeMap::new();
            for (&slot, base_part) in &source.attachments {
                let (part, trail, animation) = if slot == 0 && spawn.weapon_variant != 0 {
                    let variant = source
                        .weapon_variants
                        .get(&spawn.weapon_variant)
                        .context("enemy weapon variant is not prepared")?;
                    (
                        &variant.model,
                        Some(&variant.trail),
                        variant.animation.as_ref(),
                    )
                } else {
                    (
                        base_part,
                        source.trails.get(&slot),
                        source.uv_animation.get(&slot),
                    )
                };
                if let Some(weapon) = prepare_weapon(slot, part, trail, animation)? {
                    prepared_weapons.insert((slot, slot), Arc::clone(&weapon));
                    Arc::get_mut(model)
                        .context("enemy body shared before attachment")?
                        .weapons
                        .push(weapon);
                }
            }
            if let Some(resonance_content::battle_action::EnemyBehavior::ActionWeapons {
                groups,
                ..
            }) = &definition.actions.policy.behavior
            {
                for group in groups {
                    let mut weapons = Vec::new();
                    for &(identity, slot) in group {
                        if let std::collections::btree_map::Entry::Vacant(entry) =
                            prepared_weapons.entry((identity, slot))
                        {
                            let variant = source
                                .weapon_variants
                                .get(&identity)
                                .context("enemy action weapon is not prepared")?;
                            let weapon = prepare_weapon(
                                slot,
                                &variant.model,
                                Some(&variant.trail),
                                variant.animation.as_ref(),
                            )?
                            .context("enemy action weapon artwork is missing")?;
                            entry.insert(weapon);
                        }
                        weapons.push(Arc::clone(&prepared_weapons[&(identity, slot)]));
                    }
                    weapon_groups.push(weapons);
                }
            }
        }
        Ok(PreparedEnemyMember {
            actor,
            model,
            next_resource: handles.resource,
            models,
            render_trails,
            weapon_groups,
        })
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "Preparation borrows independently owned content and runtime resources."
    )]
    pub fn prepare_party_member(
        &self,
        menus: &MenuData,
        slot: usize,
        loadout: battle::party::Loadout,
        setup: battle::party::Setup,
        devils_arms_unlocked: bool,
        first_resource: ResourceId,
        victory: Option<&resonance_content::battle_victory::Performances>,
    ) -> Result<PreparedPartyMember> {
        let input = self.party_actors.get(slot).context("missing party input")?;
        let profile = self
            .party_profiles
            .records
            .get(input.character.index())
            .context("missing party actor profile")?;
        let character = input.character;
        let actor = battle::party::prepare_actor(
            &self.recoil,
            menus,
            &self.party,
            slot,
            &loadout,
            setup.position,
            setup.heading,
            devils_arms_unlocked,
            profile,
        )?;
        let mut models = Vec::new();
        let mut body = None;
        let source = &input.source;
        if let Some(setup) = setup.model {
            let resource = setup.resource;
            let prepared: Result<_> = (|| {
                let body = model::body(
                    &self.files,
                    &source.body,
                    source.parts.first().context("missing party body scene")?,
                    profile,
                    input.motions.clone(),
                    &actor,
                    setup,
                )?;
                let render = RenderModel {
                    resource,
                    skeleton: source.body.skeleton.clone(),
                    instances: render::RenderInstances::Fixed(1),
                    lit: true,
                    texture_channels: render::party_texture_channels(
                        &profile.texture_channels,
                        source.parts.first().context("missing party body")?,
                    )?,
                    target_anchor: Some(render::TargetAnchor::prepare(
                        &profile.target_anchor,
                        &source.body.skeleton,
                    )),
                    suppressed_nodes: render::suppressed_body_nodes(
                        &source.body.skeleton,
                        profile.traits.show_body_attachments,
                    ),
                    weapon_style: None,
                    parts: source
                        .parts
                        .iter()
                        .cloned()
                        .map(|scene| PreviewPart {
                            scene,
                            animation: None,
                            attached_to: None,
                            additive: false,
                            uv_offsets: vec![],
                        })
                        .collect(),
                };
                Ok((body, render))
            })();
            let (prepared, render) =
                prepared.with_context(|| format!("party body for character {character:?}"))?;
            body = Some(prepared);
            models.push(render);
        }
        let current = &self.party.members[character.index()];
        let performances = if let (Some(victory), Some(model)) = (victory, &body) {
            let (prepared, performances) =
                battle::victory::prepare(&self.files, victory, source, character, model)?;
            body = Some(prepared);
            performances
        } else {
            Vec::new()
        };
        let mut handles = Handles::new(first_resource);
        let mut render_trails = Vec::new();
        let mut equipment = BTreeMap::new();
        if let Some(body) = &mut body {
            let body = Arc::get_mut(body).context("party body shared before attachment")?;
            for (&item, &kind) in &input.equipment {
                let shield = matches!(kind, EquipmentKind::Shield);
                let weapon = self
                    .weapons
                    .get(&item)
                    .context("missing selected weapon descriptor")?;
                let component =
                    self.prepare_weapon(character, profile, source, weapon, shield, &mut handles);
                let mut component = component.with_context(|| {
                    format!("equipment item {item} for character {character:?}")
                })?;
                models.append(&mut component.models);
                render_trails.append(&mut component.render_trails);
                if current.equipment[if shield { 5 } else { 0 }] == item {
                    body.weapons.extend(component.weapons.iter().cloned());
                }
                equipment.insert(item, component.weapons);
            }
        }
        Ok(PreparedPartyMember {
            loadout,
            character,
            actor,
            model: body,
            equipment,
            performances,
            models,
            render_trails,
            next_resource: handles.resource,
        })
    }

    pub fn prepare(
        &self,
        menus: &MenuData,
        options: PrepareOptions,
        sound: impl FnMut(battle::voice::Sound) -> Result<Option<Sound>>,
    ) -> Result<Prepared> {
        self.prepare_with(menus, options, sound, |_| Ok(()))
    }

    /// Arrange participants before activation, retaining the prepared action and art bindings.
    #[cfg(feature = "test-support")]
    pub fn prepare_fixture(
        &self,
        menus: &MenuData,
        options: PrepareOptions,
        sound: impl FnMut(battle::voice::Sound) -> Result<Option<Sound>>,
        configure: impl FnOnce(
            &mut [(resonance_battle::Actor, resonance_battle::ActorSetup)],
        ) -> Result<()>,
    ) -> Result<Prepared> {
        self.prepare_with(menus, options, sound, configure)
    }

    fn prepare_with(
        &self,
        menus: &MenuData,
        options: PrepareOptions,
        mut sound: impl FnMut(battle::voice::Sound) -> Result<Option<Sound>>,
        configure: impl FnOnce(
            &mut [(resonance_battle::Actor, resonance_battle::ActorSetup)],
        ) -> Result<()>,
    ) -> Result<Prepared> {
        let party = &self.party;
        let style = battle::results::ResultStyle {
            play_music: self.enemies.settings.play_music,
            celebrate: self.enemies.settings.celebrate,
        };
        // Reject unimplemented E7 damage/reward bits before preparing any
        // candidate resources, including caller-made Party values from tests.
        let initial_unison_full = party.initial_unison_full()?;
        let difficulty = Difficulty::try_from(party.settings.preferences.battle_rank)?;
        ensure!(
            !self.enemies.spawns.is_empty(),
            "battle has no enemy actors"
        );
        let table = &self.party_profiles;
        let catalogue = &self.catalogue;
        let tints = battle::effect_program::load_tints(&self.files)?;
        let victory = &self.victory;
        let mut random = resonance_battle::Random::new(options.random_seed);
        let mut cosmetic_random = resonance_battle::Random::new(!options.random_seed);
        let mut handles = Handles {
            resource: ResourceId(1),
        };
        let common_id = handles.resource();
        let techniques_id = handles.resource();
        for cue in [1, 2, 3, 4, 6, 7, 38] {
            let _ = sound(battle::voice::Sound::Cue(cue))?;
        }
        let mut models = Vec::new();
        let mut render_trails = Vec::new();
        let mut rows = Vec::new();
        let mut bindings = battle::ActionDefinitions::default();
        let mut feedback = battle::feedback::Feedback {
            target_changed: sound(battle::voice::Sound::Cue(battle::audio::TARGET_CHANGED))?,
            result_advancement: Some(resonance_battle::EffectAppearance {
                resource: common_id,
                member: 20,
            }),
            result_advancement_sound: sound(Sound::Cue(battle::audio::RESULT_ADVANCEMENT))?,
            acquisition: Some(resonance_battle::EffectAppearance {
                resource: common_id,
                member: 13,
            }),
            acquisition_sound: sound(Sound::Cue(77))?,
            ..Default::default()
        };
        let mut common_members =
            BTreeSet::from([9, 13, 14, 15, 17, 19, 20, 42, 43, 44, 45, 46, 48, 50]);
        let mut performances = Vec::new();
        let mut prepared_equipment = BTreeMap::new();
        let mut postures = Vec::new();
        for (slot, input) in self.party_actors.iter().enumerate() {
            let character = input.character;
            let profile = table
                .records
                .get(character.index())
                .context("missing party actor profile")?;
            let member = &party.members[character.index()];
            let loadout = battle::party::loadout(menus, member, character)?;
            let motions = &input.motions;
            let initial = self.files.diagnostics().attempt(
                "party entry pose",
                entry::initial_playback(
                    &mut cosmetic_random,
                    &table.entry,
                    profile,
                    resonance_battle::Side::Party,
                    motions,
                    (i32::from(member.hp), loadout.attributes.max_hp),
                    member.ailments.petrified,
                )
                .with_context(|| format!("initial playback for character {character:?}")),
            )?;
            let resource = handles.resource();
            let setup = battle::party::Setup {
                position: input.position,
                heading: 0.,
                model: initial.map(|initial| model::ModelSetup {
                    resource,
                    initial,
                    suppress_root_translation: [false; 3],
                }),
            };
            let mut prepared = self.prepare_party_member(
                menus,
                slot,
                loadout,
                setup,
                options.devils_arms_unlocked,
                handles.resource,
                style.celebrate.then_some(victory),
            )?;
            if let Some(eyes) =
                resonance_content::appearance::forced_face(character as u32, options.colette_state)
                && let Some(model) = &mut prepared.model
            {
                Arc::make_mut(model).forced_eye_frame = Some(eyes);
            }
            if let Some(name) = resonance_content::appearance::hidden_accessory(
                character as u32,
                &options.event_flags,
            ) && let Some(body) = prepared
                .models
                .iter_mut()
                .find(|body| body.resource == resource)
                && let Some(node) = body
                    .skeleton
                    .bones
                    .iter()
                    .position(|bone| bone.name == name)
            {
                body.suppressed_nodes.insert(node);
            }
            let costume_frame = resonance_content::appearance::costume_frame(
                character as u32,
                &options.event_flags,
            );
            if let Some(body) = prepared
                .models
                .iter_mut()
                .find(|body| body.resource == resource)
            {
                for part in &mut body.parts {
                    let Some(texture) = part.scene.appearance.as_ref().and_then(|a| a.costume)
                    else {
                        continue;
                    };
                    part.uv_offsets.resize(part.scene.materials.len(), [0.; 4]);
                    for (offset, material) in part.uv_offsets.iter_mut().zip(&part.scene.materials)
                    {
                        for (stage, binding) in [&material.color, &material.multiply]
                            .into_iter()
                            .enumerate()
                        {
                            if binding.as_ref().is_some_and(|b| b.texture == texture) {
                                offset[stage * 2 + 1] += f32::from(costume_frame) / 4.;
                            }
                        }
                    }
                }
            }
            if character == Character::Colette && options.colette_state != 0 {
                prepared.actor.body.center_offset[1] += COLETTE_WING_CENTER_RISE;
            }
            if self.events.is_some() && character == Character::Kratos {
                prepared.actor.availability = resonance_battle::ActorAvailability::Absent;
            }
            handles.resource = prepared.next_resource;
            models.append(&mut prepared.models);
            render_trails.append(&mut prepared.render_trails);
            let strategy = resonance_battle::TargetPolicy::try_from(if member.strategy[0] == 0 {
                table.default_strategy[character.index()][0]
            } else {
                member.strategy[0]
            })?;
            let member = prepared;
            performances.extend(member.performances);
            prepared_equipment.extend(
                member
                    .equipment
                    .into_iter()
                    .map(|(item, weapons)| ((slot, item), weapons)),
            );
            postures.push(battle::victory::prepare_posture(
                &self.files,
                member.character,
                member.model.as_deref(),
            )?);
            let spell_charge = member
                .loadout
                .spell_charge
                .then_some(SpellChargeDefinition {
                    automatic: character != Character::Genis,
                    enabled: true,
                });
            rows.push(ActorRow {
                actor: member.actor,
                profile,
                actions: None,
                strategy,
                quick_item: member.loadout.quick_item,
                extended_overlimit: member.loadout.extended_overlimit,
                source: model::ModelSource::Party(character),
                model: member.model,
                setup: resonance_battle::ActorSetup {
                    spell_charge,
                    aerial_spells: member.loadout.aerial_spells,
                    ..Default::default()
                },
            });
        }
        let mut unison_weapons = BTreeMap::new();
        for &native in self
            .spell_scenes
            .keys()
            .filter(|id| matches!(id, 301 | 312 | 314))
        {
            let scene: resonance_content::battle_scene::Scene = self
                .files
                .json(&resonance_content::battle_scene::path(native))?;
            let mut weapon = scene.weapon.context("missing combination weapon")?;
            if native == 301 {
                // Pow Blade equips the same hammer in both of Lloyd's hands.
                let part = weapon
                    .parts
                    .get(&0)
                    .context("missing Pow Blade hammer")?
                    .clone();
                weapon.parts.insert(1, part);
                if let Some(&trail) = weapon.trails.get(&0) {
                    weapon.trails.insert(1, trail);
                }
                if let Some(animation) = weapon.uv_animation.get(&0).cloned() {
                    weapon.uv_animation.insert(1, animation);
                }
            }
            for (slot, input) in self.party_actors.iter().enumerate() {
                if !matches!(
                    (native, input.character),
                    (301, Character::Lloyd)
                        | (312, Character::Presea)
                        | (314, Character::Zelos | Character::Kratos)
                ) {
                    continue;
                }
                let mut prepared = self.prepare_weapon(
                    input.character,
                    rows[slot].profile,
                    &input.source,
                    &weapon,
                    false,
                    &mut handles,
                )?;
                models.append(&mut prepared.models);
                render_trails.append(&mut prepared.render_trails);
                unison_weapons
                    .entry(slot)
                    .or_insert_with(BTreeMap::new)
                    .insert(native, prepared.weapons);
            }
        }
        let mut skill_weapons = BTreeMap::new();
        for &catalogue in self.skill_scenes.keys() {
            let scene: resonance_content::battle_scene::Scene = self
                .files
                .json(&resonance_content::battle_scene::skill_path(catalogue))?;
            let Some(weapon) = scene.weapon else { continue };
            for (slot, input) in self.party_actors.iter().enumerate() {
                if !super::resources::party_techniques(&self.catalogue, input.character)?
                    .contains_key(&catalogue)
                {
                    continue;
                }
                let mut prepared = self.prepare_weapon(
                    input.character,
                    rows[slot].profile,
                    &input.source,
                    &weapon,
                    false,
                    &mut handles,
                )?;
                models.append(&mut prepared.models);
                render_trails.append(&mut prepared.render_trails);
                skill_weapons.insert((input.character, catalogue), prepared.weapons);
            }
        }
        let mut technique_models = BTreeMap::new();
        // Party and enemy spells share this resident model bank.
        for (&slot, part) in self
            .technique_effects
            .iter()
            .filter_map(|bank| bank.art.as_ref())
            .flat_map(|art| &art.models)
        {
            let resource = handles.resource();
            if let Some((model, render)) = render::effect_model(&self.files, part, resource, false)?
            {
                technique_models.insert(slot, model);
                models.push(render);
            }
        }
        // Resident slot zero draws Colette's current chakram, including equipment swaps.
        let mut thrown_weapons = None;
        if let Some(slot) = rows.iter().position(|row| {
            matches!(row.source, model::ModelSource::Party(character)
                if character == battle::party::Character::Colette)
        }) && let Some(body) = &rows[slot].model
        {
            let equipped = body
                .weapons
                .iter()
                .find(|weapon| weapon.slot == Some(WeaponSlot(0)))
                .context("Colette has no prepared primary weapon")?
                .primary_resource();
            let mut variants = BTreeMap::new();
            for (&(owner, item), weapons) in &prepared_equipment {
                if owner != slot {
                    continue;
                }
                let Some(weapon) = weapons
                    .iter()
                    .find(|weapon| weapon.slot == Some(WeaponSlot(0)))
                else {
                    continue;
                };
                let part = self.weapons[&item]
                    .parts
                    .get(&0)
                    .context("Colette's primary weapon artwork is missing")?;
                if let Some((model, render)) =
                    render::effect_model(&self.files, part, handles.resource(), true)?
                {
                    variants.insert(weapon.primary_resource(), model);
                    models.push(render);
                }
            }
            technique_models.insert(
                0,
                variants
                    .get(&equipped)
                    .context("Colette's equipped chakram effect is not prepared")?
                    .clone(),
            );
            thrown_weapons = Some((slot, variants));
        }
        let hammer_effect = technique_models.contains_key(&HAMMER_MODEL).then_some(
            resonance_battle::EffectAppearance {
                resource: techniques_id,
                member: HAMMER_EFFECT,
            },
        );
        feedback.hammer = hammer_effect;
        let mut enemy_banks = BTreeMap::new();
        let mut passive_effects = Vec::new();
        let mut independent_models = Vec::new();
        for enemy in self
            .enemies
            .resources
            .iter()
            .map(|resource| resource.monster.id)
        {
            let bank = self.enemy_effects.get(&enemy);
            let resource = handles.resource();
            let mut prepared = BTreeMap::new();
            let independent = match self.enemy_definitions[&enemy].actions.policy.behavior {
                Some(resonance_content::battle_action::EnemyBehavior::IndependentModel {
                    model,
                }) => Some(model),
                _ => None,
            };
            let instances = self
                .enemies
                .spawns
                .iter()
                .filter(|spawn| self.enemies.resources[spawn.resource].monster.id == enemy)
                .count();
            for (&slot, part) in bank
                .into_iter()
                .filter_map(|bank| bank.art.as_ref())
                .flat_map(|art| &art.models)
            {
                let model_resource = handles.resource();
                if let Some((model, mut render)) =
                    render::effect_model(&self.files, part, model_resource, false)?
                {
                    if independent == Some(slot) {
                        render.instances = render::RenderInstances::Fixed(instances);
                    }
                    prepared.insert(slot, model);
                    models.push(render);
                }
            }
            if let Some(slot) = independent {
                let model = prepared.get(&slot).with_context(|| {
                    format!("enemy {enemy} independent model {slot} is not prepared")
                })?;
                independent_models.extend(std::iter::repeat_n(model.clone(), instances));
            }
            let mut members: BTreeSet<_> = self.enemy_definitions[&enemy]
                .profile
                .passive_effects
                .iter()
                .map(|effect| effect.program)
                .collect();
            members.extend(
                self.enemy_definitions[&enemy]
                    .profile
                    .idle_effect
                    .map(|effect| effect.program),
            );
            members.extend(
                self.enemy_definitions[&enemy]
                    .profile
                    .movement_effect
                    .map(|effect| effect.program),
            );
            if let Some(resonance_content::battle_action::EnemyBehavior::ActionWeapons {
                effect,
                ..
            }) = &self.enemy_definitions[&enemy].actions.policy.behavior
            {
                members.insert(*effect);
            }
            if let Some((model, _)) = self.enemy_models.get(&enemy) {
                members.extend(model.uv_animation.iter().filter_map(|(&slot, animation)| {
                    (animation.effect_interval != 0).then_some(u16::from(slot))
                }));
                if model.weapon_variants.values().any(|variant| {
                    variant
                        .animation
                        .as_ref()
                        .is_some_and(|animation| animation.effect_interval != 0)
                }) {
                    members.insert(0);
                }
            }
            if let Some(resonance_content::battle_enemy::Opening::Entrance(entrance)) = self
                .enemy_definitions[&enemy]
                .presentation
                .as_ref()
                .and_then(|presentation| presentation.opening.as_ref())
            {
                members.extend(entrance.cues.iter().filter_map(|(_, cue)| match cue {
                    resonance_content::battle_enemy::EntranceCue::Effect(member) => Some(*member),
                    _ => None,
                }));
            }
            enemy_banks.insert(
                enemy,
                EffectResource {
                    bank: bank.cloned(),
                    resource,
                    members: members.into_iter().collect(),
                    models: prepared,
                },
            );
        }
        let mut prepare_scene = |bank: &Arc<resonance_content::battle_effect::SourceBank>| {
            render::scene_bank(
                &self.files,
                bank,
                handles.resource(),
                || handles.resource(),
                &mut models,
            )
        };
        let spell_scenes = self
            .spell_scenes
            .iter()
            .map(|(&id, bank)| Ok((id, prepare_scene(bank)?)))
            .collect::<Result<BTreeMap<_, _>>>()?;
        let skill_scenes = self
            .skill_scenes
            .iter()
            .map(|(&id, bank)| Ok((id, prepare_scene(bank)?)))
            .collect::<Result<BTreeMap<_, _>>>()?;
        let mut enemy_resources = BTreeMap::new();
        let mut enemy_weapon_groups = BTreeMap::new();
        let mut result_enemies = Vec::new();
        let mut enemy_entry_rows = Vec::new();
        for (spawn_index, spawn) in self.enemies.spawns.iter().enumerate() {
            let monster = &self.enemies.resources[spawn.resource].monster;
            let source = &self.enemy_definitions[&monster.id];
            let resource = *enemy_resources
                .entry(monster.id)
                .or_insert_with(|| handles.resource());
            let prepared = self.prepare_enemy_member(
                spawn_index,
                resource,
                handles.resource,
                &mut cosmetic_random,
            )?;
            handles.resource = prepared.next_resource;
            for model in prepared.models {
                if let Some(existing) = models
                    .iter_mut()
                    .find(|existing| existing.resource == model.resource)
                {
                    match (&mut existing.instances, model.instances) {
                        (
                            render::RenderInstances::Fixed(total),
                            render::RenderInstances::Fixed(count),
                        ) => *total += count,
                        _ => anyhow::bail!("duplicate particle render resource"),
                    }
                } else {
                    models.push(model);
                }
            }
            render_trails.extend(prepared.render_trails);
            enemy_weapon_groups.insert(rows.len(), prepared.weapon_groups);
            let actor = prepared.actor;
            let model = prepared.model;
            enemy_entry_rows.push(source.entry_row);
            result_enemies.push(battle::results::PreparedEnemy {
                actor: resonance_battle::ActorId::from_index(rows.len())?,
                level: actor.equipment.stats.level,
                grade: monster.grade,
                steal: monster.steal,
                steal_chance: monster.steal_chance,
                reward: battle::rewards::EnemyReward::from_monster(monster, spawn.variant)?,
            });
            for (effect, idle_only) in source
                .profile
                .passive_effects
                .iter()
                .map(|effect| (effect, false))
                .chain(
                    source
                        .profile
                        .idle_effect
                        .iter()
                        .map(|effect| (effect, true)),
                )
            {
                passive_effects.push(resonance_battle::PassiveEffect {
                    actor: resonance_battle::ActorId::from_index(rows.len())?,
                    appearance: resonance_battle::EffectAppearance {
                        resource: enemy_banks[&monster.id].resource,
                        member: effect.program,
                    },
                    interval: effect.interval,
                    idle_only,
                    weapon: None,
                });
            }
            for weapon in model.iter().flat_map(|model| &model.weapons) {
                if let (Some(interval), Some(slot)) = (weapon.effect_interval, weapon.slot) {
                    passive_effects.push(resonance_battle::PassiveEffect {
                        actor: resonance_battle::ActorId::from_index(rows.len())?,
                        appearance: resonance_battle::EffectAppearance {
                            resource: enemy_banks[&monster.id].resource,
                            member: u16::from(slot.0),
                        },
                        interval,
                        idle_only: false,
                        weapon: Some(weapon.primary_resource()),
                    });
                }
            }
            rows.push(ActorRow {
                actor,
                quick_item: false,
                extended_overlimit: false,
                profile: &source.profile,
                actions: None,
                strategy: source
                    .target_strategy
                    .context("unsupported enemy target strategy")?,
                source: model::ModelSource::Enemy(monster.id),
                model,
                setup: resonance_battle::ActorSetup {
                    target_excluded: source.profile.hosted_body.is_some(),
                    resists_cyclone: source.profile.camera_category >= 2,
                    movement_effect: source.profile.movement_effect.map(|effect| {
                        (
                            resonance_battle::EffectAppearance {
                                resource: enemy_banks[&monster.id].resource,
                                member: effect.program,
                            },
                            effect.interval,
                        )
                    }),
                    ..Default::default()
                },
            });
        }
        let enemy_positions =
            super::placement::enemy_positions(&self.enemies.spawns, &enemy_entry_rows, || {
                random.next_u16()
            })?;
        for (row, [x, z]) in rows[self.party_actors.len()..]
            .iter_mut()
            .zip(enemy_positions)
        {
            row.actor.position[0] = x;
            row.actor.position[2] = z;
            if let model::ModelSource::Enemy(enemy) = row.source
                && let Some(position) = battle::defense_system::initial_position(enemy)
            {
                row.actor.position = position;
            }
        }
        for row in &mut rows {
            if let Some(model) = &mut row.model {
                Arc::make_mut(model).tint[..3].copy_from_slice(&self.stage.actor_color[..3]);
            }
        }
        feedback.contact_art = tints.as_ref().map(|tints| {
            let (feedback, members) = battle::contact_feedback::prepare(tints, common_id);
            common_members.extend(members);
            feedback
        });
        let voices = battle::voice::Resolver::new(
            &self.files,
            table,
            self.enemy_definitions
                .iter()
                .map(|(&id, enemy)| (id, &enemy.profile)),
        );
        let mut resources = Resources {
            files: &self.files,
            recoil: &self.recoil,
            catalogue,
            unison: &self.unison,
            party_null_motions: self
                .party_actors
                .iter()
                .map(|actor| (actor.character, &actor.source.null_motions))
                .collect(),
            projectiles: self
                .files
                .json(resonance_content::battle_projectile::PATH)?,
            tints: tints.as_ref(),
            voices: &voices,
            common: EffectResource {
                bank: self.common_effects.clone(),
                resource: common_id,
                members: common_members.into_iter().collect(),
                models: BTreeMap::new(),
            },
            techniques: EffectResource {
                bank: self.technique_effects.clone(),
                resource: techniques_id,
                members: hammer_effect.iter().map(|effect| effect.member).collect(),
                models: technique_models,
            },
            enemy_effects: enemy_banks,
            spell_scenes,
            skill_scenes,
            skill_weapons,
            prepared: Default::default(),
            feedback: &mut feedback,
            spells: BTreeMap::new(),
            sound: &mut sound,
        };
        resources.prepare()?;
        for resource in resources
            .enemy_effects
            .values()
            .filter(|resource| !resource.members.is_empty())
        {
            resources.prepared.effect(resource.clone())?;
        }
        for (row_index, row) in rows.iter_mut().enumerate() {
            match row.source {
                model::ModelSource::Party(character) => {
                    let null_motions = &self.party_actors[row_index].source.null_motions;
                    let actions = resources.party(
                        character,
                        row.model.as_deref(),
                        null_motions,
                        &mut bindings,
                    )?;
                    let member = &party.members[character.index()];
                    for technique in member
                        .techniques
                        .iter()
                        .filter(|id| !actions.techniques.contains_key(id))
                    {
                        self.files.diagnostics().report("battle technique preparation",
                            anyhow::anyhow!("learned technique {technique} for character {character:?} has no executable action"))?;
                    }
                    if !actions.techniques.values().any(|&key| {
                        matches!(bindings[key].execution, battle::ActionExecution::Casting(_))
                    }) {
                        row.setup.spell_charge = None;
                    }
                    if character == battle::party::Character::Colette {
                        use battle::colette_spells::{HOLY_JUDGMENT, HOLY_SONG, JUDGMENT};
                        const TRAINED_SPELL_USES: u16 = 50;
                        const HOLY_JUDGMENT_ONE_IN: u16 = 16;
                        row.setup.cast_replacement = Some(resonance_battle::CastReplacement {
                            input: *actions
                                .techniques
                                .get(&HOLY_SONG)
                                .context("Holy Song is not prepared")?,
                            output: *actions
                                .techniques
                                .get(&HOLY_JUDGMENT)
                                .context("Holy Judgment is not prepared")?,
                            required_uses: &[
                                (TechniqueId(HOLY_SONG), TRAINED_SPELL_USES),
                                (TechniqueId(JUDGMENT), TRAINED_SPELL_USES),
                            ],
                            trigger: resonance_battle::CastReplacementTrigger::Chance {
                                one_in: HOLY_JUDGMENT_ONE_IN,
                            },
                        });
                    } else if character == battle::party::Character::Genis {
                        use battle::elemental_spells::{INDIGNATION, INDIGNATION_JUDGMENT};
                        const TRAINED_SPELL_USES: u16 = 50;
                        row.setup.cast_replacement = Some(resonance_battle::CastReplacement {
                            input: *actions
                                .techniques
                                .get(&INDIGNATION)
                                .context("Indignation is not prepared")?,
                            output: *actions
                                .techniques
                                .get(&INDIGNATION_JUDGMENT)
                                .context("Indignation Judgment is not prepared")?,
                            required_uses: &[(TechniqueId(INDIGNATION), TRAINED_SPELL_USES)],
                            trigger: resonance_battle::CastReplacementTrigger::Overlimit,
                        });
                    }
                    row.actions = Some(actions);
                }
                model::ModelSource::Enemy(enemy) => {
                    let source = &self.enemy_definitions[&enemy];
                    let weapon_groups = enemy_weapon_groups.remove(&row_index).unwrap_or_default();
                    let actions = resources.enemy(
                        enemy,
                        row.model.as_deref(),
                        source,
                        &weapon_groups,
                        &mut bindings,
                    )?;
                    let mut decision = battle::ai::definition(
                        enemy,
                        source,
                        &actions,
                        difficulty,
                        row.model.as_deref(),
                    )?;
                    decision.walk_motion =
                        model::common_motion(row.model.as_deref(), MotionRole::Walk);
                    if let Some(resonance_content::battle_action::EnemyBehavior::ActionWeapons {
                        actions,
                        idle_group,
                        effect,
                        anchors,
                        drop_on_hurt,
                        ..
                    }) = &source.actions.policy.behavior
                    {
                        let model = row.model.as_ref().context("missing enemy weapon model")?;
                        let body = &self.enemy_models[&enemy].0.body;
                        let anchor = |slot| -> Result<_> {
                            Ok(resonance_battle::MeleeAnchor {
                                resource: model.resource,
                                bone: *body
                                    .attachments
                                    .get(&slot)
                                    .context("missing enemy weapon effect attachment")?,
                            })
                        };
                        decision.behavior = Some(resonance_battle::EnemyBehavior::ActionWeapons {
                            groups: weapon_groups,
                            actions: actions.clone(),
                            idle_group: *idle_group,
                            effect: resonance_battle::EffectAppearance {
                                resource: resources.enemy_effects[&enemy].resource,
                                member: *effect,
                            },
                            anchors: anchors
                                .iter()
                                .map(|&slot| anchor(slot))
                                .collect::<Result<_>>()?,
                            drop_on_hurt: drop_on_hurt
                                .map(|drop| Ok::<_, anyhow::Error>((drop, anchor(drop.slot)?)))
                                .transpose()?,
                        });
                    }
                    if enemy == 191 {
                        battle::enemy_stage::apply_sword(
                            self.enemies.spawns[row_index - self.party_actors.len()].variant,
                            row.model.as_deref(),
                            &mut decision,
                        )?;
                    }
                    decision
                        .disabled_choices
                        .extend(battle::ai::disabled_by_flags(
                            &source.actions.rows,
                            &options.event_flags,
                        ));
                    row.setup.enemy_decision = Some(decision);
                }
                _ => unreachable!("encounter rows contain party and enemy actors"),
            }
        }
        let ids = (0..rows.len())
            .map(resonance_battle::ActorId::from_index)
            .collect::<Result<Vec<_>>>()?;
        let wings = self
            .party_actors
            .iter()
            .enumerate()
            .find(|(_, input)| input.character == Character::Colette && options.colette_state != 0)
            .filter(|(slot, _)| rows[*slot].model.is_some())
            .map(|(slot, input)| -> Result<_> {
                let (wings, render) = render::wings(
                    &self.files,
                    &input.source,
                    ids[slot],
                    handles.resource(),
                    resources
                        .common
                        .bank
                        .as_deref()
                        .context("wing sparkle effects are missing")?,
                )?;
                models.push(render);
                Ok(wings)
            })
            .transpose()?;
        let defense_members: Vec<_> = rows
            .iter()
            .zip(&ids)
            .filter_map(|(row, &actor)| match row.source {
                model::ModelSource::Enemy(enemy @ 211..=214) => Some((actor, enemy)),
                _ => None,
            })
            .collect();
        let defense_system = if defense_members.is_empty() {
            None
        } else {
            let core = defense_members
                .iter()
                .find(|(_, enemy)| *enemy == 211)
                .context("Defense System formation is missing its core")?
                .0;
            Some(battle::defense_system::definition(
                &defense_members,
                rows[core.index()]
                    .model
                    .as_deref()
                    .context("Defense System core body is not prepared")?,
            )?)
        };
        let mut hosted_participants = Vec::new();
        if defense_system.is_none()
            && let Some(host) = rows.get(self.party_actors.len())
            && host.profile.traits.hosts_participants
        {
            for (slot, row) in rows[self.party_actors.len()..].iter().enumerate().skip(1) {
                let Some(release) = row.profile.hosted_body else {
                    continue;
                };
                let model = host
                    .model
                    .as_deref()
                    .context("participant host body is not prepared")?;
                let marker = format!("pa{:02}_", slot - 1);
                let bone = model
                    .skeleton
                    .bones
                    .iter()
                    .position(|bone| bone.name.to_ascii_lowercase().starts_with(&marker))
                    .with_context(|| format!("participant host is missing {marker} attachment"))?;
                hosted_participants.push(resonance_battle::HostedParticipant {
                    actor: ids[self.party_actors.len() + slot],
                    host: ids[self.party_actors.len()],
                    anchor: resonance_battle::MeleeAnchor {
                        resource: model.resource,
                        bone: bone.try_into()?,
                    },
                    release,
                    released_hurt: [
                        model::common_motion(row.model.as_deref(), MotionRole::Hurt),
                        model::motion(row.model.as_deref(), 4).or_else(|| {
                            model::common_motion(row.model.as_deref(), MotionRole::Hurt)
                        }),
                    ],
                });
            }
        }
        let empty_weapons = BTreeMap::new();
        let unison_members: Vec<_> = rows
            .iter()
            .enumerate()
            .filter_map(|(slot, row)| {
                let model::ModelSource::Party(character) = row.source else {
                    return None;
                };
                Some(battle::unison::HostMember {
                    actor: ids[slot],
                    character,
                    model: row.model.as_deref()?,
                    null_motions: &self.party_actors[slot].source.null_motions,
                    profile: row.profile,
                    techniques: &row.actions.as_ref()?.techniques,
                    temporary_weapons: unison_weapons.get(&slot).unwrap_or(&empty_weapons),
                    short_weapon: row.actor.equipment.dagger_reach,
                })
            })
            .collect();
        let active_compounds: BTreeSet<_> = self
            .party_actors
            .iter()
            .flat_map(|input| {
                let index = input.character.index();
                menus.ex_skills.characters[index]
                    .equipped_compounds(&party.members[index].ex_skills)
                    .map(|(_, recipe)| recipe.skill)
            })
            .collect();
        let immune_targets = rows
            .iter()
            .zip(&ids)
            .filter_map(|(row, &id)| {
                (row.actor.side == resonance_battle::Side::Enemy
                    && row.profile.traits.immune_to_unison)
                    .then_some(id)
            })
            .collect();
        use resonance_content::menu_data::ex_effect;
        let unison = if unison_members.len() < 2 || unison_members.len() != self.party_actors.len()
        {
            None
        } else {
            Some(battle::unison::prepare_host(
                &self.unison,
                &unison_members,
                immune_targets,
                active_compounds.contains(&ex_effect::RETAIN_UNISON_GAUGE),
                active_compounds.contains(&ex_effect::UNISON_DAMAGE),
                &mut resources,
                &mut bindings,
            )?)
        };
        let mut action_resources = resources.prepared;
        let enemy_effects = resources.enemy_effects;
        let ambient_effect = if let Some(interval) = self.stage.recurring_interval {
            let resource = handles.resource();
            action_resources.effect(EffectResource {
                bank: self.stage_effects.clone(),
                resource,
                members: vec![1],
                models: BTreeMap::new(),
            })?;
            Some((
                resonance_battle::EffectAppearance {
                    resource,
                    member: 1,
                },
                interval,
            ))
        } else {
            None
        };
        let mut effect_banks = action_resources.prepare_effects(
            &mut |id| (sound)(battle::voice::Sound::Cue(id)),
            self.files.diagnostics(),
        )?;
        if let Some((slot, variants)) = thrown_weapons {
            effect_banks
                .iter_mut()
                .find(|bank| bank.resource() == techniques_id)
                .context("Colette's resident effect bank is not prepared")?
                .bind_weapon_models(0, ids[slot], WeaponSlot(0), variants)?;
        }
        let party_roster: Vec<_> = ids
            .iter()
            .copied()
            .zip(rows.iter().filter_map(|row| match row.source {
                model::ModelSource::Party(character) => Some(character),
                _ => None,
            }))
            .collect();
        let characters: Vec<_> = party_roster
            .iter()
            .map(|&(_, character)| character)
            .collect();
        let equipment = prepared_equipment
            .into_iter()
            .map(|((slot, item), component)| ((ids[slot], item), component))
            .collect();
        let sources: Vec<_> = rows.iter().map(|row| row.source).collect();
        let level_difference = ((rows
            .iter()
            .take(self.party_actors.len())
            .map(|row| i32::from(row.actor.equipment.stats.level))
            .sum::<i32>()
            / self.party_actors.len() as i32)
            - (result_enemies
                .iter()
                .map(|enemy| i32::from(enemy.level))
                .sum::<i32>()
                / result_enemies.len() as i32))
            .clamp(-8, 8) as i8;
        let mut enemy_presentations = Vec::new();
        for (index, source) in sources.iter().copied().enumerate() {
            let model::ModelSource::Enemy(enemy) = source else {
                continue;
            };
            if let Some(mut presentation) = self.enemy_definitions[&enemy].presentation.clone() {
                use resonance_content::battle_enemy::{EntranceCue, Opening};
                let speaker_source = match &presentation.opening {
                    Some(Opening::Entrance(entrance)) => {
                        entrance.staging.as_ref().and_then(|staging| {
                            staging
                                .character
                                .try_into()
                                .ok()
                                .map(model::ModelSource::Party)
                        })
                    }
                    _ => None,
                };
                let speaker = speaker_source
                    .and_then(|speaker| sources.iter().position(|source| *source == speaker))
                    .map(|index| ids[index]);
                if speaker_source.is_some() && speaker.is_none() {
                    presentation.opening = match &presentation.opening {
                        Some(Opening::Entrance(entrance)) => entrance
                            .staging
                            .as_ref()
                            .and_then(|staging| staging.fallback.clone())
                            .map(Opening::Voice),
                        _ => None,
                    };
                }
                let speaker_model =
                    speaker.and_then(|speaker| rows[speaker.index()].model.as_ref());
                match &mut presentation.opening {
                    Some(Opening::Voice(opening)) => {
                        opening.sound = voices.absolute(source, opening.sound, &mut sound)?;
                    }
                    Some(Opening::Entrance(entrance)) => {
                        for (_, cue) in &mut entrance.cues {
                            match cue {
                                EntranceCue::Voice(line) => {
                                    *line = voices
                                        .absolute(source, Some(*line), &mut sound)?
                                        .context("entrance voice is unavailable")?
                                }
                                EntranceCue::PartyVoice(line) => {
                                    *line = voices
                                        .absolute(
                                            speaker_source
                                                .context("entrance speaker is missing")?,
                                            Some(*line),
                                            &mut sound,
                                        )?
                                        .context("entrance party voice is unavailable")?
                                }
                                EntranceCue::Sound(line) => {
                                    *line =
                                        sound(*line)?.context("entrance sound is unavailable")?
                                }
                                EntranceCue::Pose { clip, .. } => ensure!(
                                    rows[index]
                                        .model
                                        .as_ref()
                                        .is_some_and(|model| model.motions.contains_key(clip)),
                                    "entrance motion {clip} is unavailable"
                                ),
                                EntranceCue::PartyPose { clip, .. } => ensure!(
                                    speaker_model
                                        .is_some_and(|model| model.motions.contains_key(clip)),
                                    "entrance party motion {clip} is unavailable"
                                ),
                                _ => {}
                            }
                        }
                    }
                    None => {}
                }
                enemy_presentations.push(resonance_battle::EnemyPresentation {
                    actor: ids[index],
                    definition: presentation,
                    model: rows[index]
                        .model
                        .as_ref()
                        .context("enemy presentation model is missing")?
                        .resource,
                    effects: enemy_effects[&enemy].resource,
                    speaker,
                    speaker_model: speaker_model.map(|model| model.resource),
                });
            }
        }
        if self.enemies.settings.entry_voice && self.events.is_none() {
            let mut choices = Vec::new();
            let major = rows
                .iter()
                .skip(self.party_actors.len())
                .any(|row| row.profile.camera_category >= 3);
            for (index, row) in rows[..self.party_actors.len()].iter().enumerate() {
                if !row.actor.available() || row.actor.hp == 0 {
                    continue;
                }
                let Some(profile) = &row.profile.voices else {
                    continue;
                };
                for &line in battle::voice::entry_lines(
                    profile,
                    party.battles.previous_formation == Some(self.setup.formation()?),
                    major,
                    self.party_actors.len(),
                    result_enemies.len(),
                    i32::from(level_difference),
                ) {
                    if let Some(voice) = voices.absolute(row.source, line, &mut sound)? {
                        choices.push((ids[index], voice));
                    }
                }
            }
            if !choices.is_empty() {
                let index = usize::from(cosmetic_random.next_u16()) % choices.len();
                feedback.entry_voice = Some(choices.swap_remove(index));
            }
        }

        let entry_choices = rows
            .iter()
            .enumerate()
            .map(|(index, row)| resonance_battle::EntryChoice {
                actor: ids[index],
                strategy: row.strategy,
            })
            .collect();
        let unison_ready = sound(Sound::Cue(77))?;
        feedback.unison_ready = unison_ready;
        feedback.overlimit_sound = unison_ready;
        feedback.takeoff = Some(resonance_battle::EffectAppearance {
            resource: common_id,
            member: 17,
        });
        feedback.skill_ready = Some(resonance_battle::EffectAppearance {
            resource: common_id,
            member: 50,
        });
        feedback.counter = Some(resonance_battle::EffectAppearance {
            resource: common_id,
            member: 48,
        });
        for row in &mut rows {
            row.prepare_control(self, difficulty)?;
            row.prepare_feedback(&voices, common_id, unison_ready, &mut feedback, &mut sound)?;
        }
        prepare_assists(party, &mut rows[..self.party_actors.len()], &ids);
        let camera_leader = entry::camera_leader(
            rows.iter()
                .take(self.party_actors.len())
                .map(|row| row.actor.control),
        );
        let camera = resonance_battle::CameraDefinition {
            leader: ids[camera_leader],
            stage_pitch: self.stage.camera_pitch_offset,
            auto_zoom: party.settings.preferences.battle_auto_zoom,
            yaw_limits: defense_system
                .as_ref()
                .map(|_| battle::defense_system::CAMERA_YAW_LIMITS),
        };
        let items_forbidden = self
            .enemy_definitions
            .values()
            .any(|enemy| enemy.actions.policy.items_forbidden);
        let mut enabled = command::enabled_rows(
            self.enemies.settings.escape_restricted || party.battle_rules.coliseum,
            options.story,
        ) & !party.battle_rules.disabled_commands;
        if items_forbidden || party.battle_rules.coliseum {
            enabled &= !command::Command::Items.mask();
        }
        let hover_bobbing = rows
            .iter()
            .zip(&ids)
            .filter_map(|(row, &id)| row.profile.traits.hover_bobbing.then_some(id))
            .collect();
        if let Some(tints) = &tints {
            feedback.recovery_tint = Some(tints.actors.recovery[..3].try_into().unwrap());
        }
        let (mut items, item_feedback) = battle::items::prepare(
            &voices,
            tints.as_ref(),
            rows.iter().map(|row| {
                (
                    row.source,
                    row.model.as_deref(),
                    row.profile,
                    row.quick_item,
                )
            }),
            common_id,
            &mut sound,
        )?;
        if items_forbidden {
            items.actors.fill(None);
        }
        feedback.items = Some(item_feedback);
        if rows
            .iter()
            .any(|row| row.actor.equipment.recovery.common.self_cure)
        {
            let notice = menus.ex_skill_text().and_then(|text| {
                text.skills
                    .get(&77)
                    .context("missing Self Cure notice")
                    .map(|skill| skill.name.clone())
            });
            if let Some(notice) = self
                .files
                .diagnostics()
                .attempt("Self Cure presentation", notice)?
            {
                feedback.self_cure_notice = Some(notice);
            }
        }
        let stun_particle = self
            .common_effects
            .as_ref()
            .map(|source| {
                self.files.diagnostics().attempt(
                    "stun bird artwork",
                    source.particle(19).map(|data| {
                        Arc::new(resonance_battle::ParticleDefinition {
                            resource: common_id,
                            member: 19,
                            model: None,
                            data: data.clone(),
                        })
                    }),
                )
            })
            .transpose()?
            .flatten();
        let effects = action_resources
            .effects
            .values()
            .filter_map(|effect| effect.bank.as_ref().map(|source| (effect, source)))
            .map(|(effect, source)| {
                let mut render = render::effects(
                    effect.resource,
                    source,
                    &effect.members,
                    // Elemental attack, casting and seal feedback use resident banks.
                    // Other banks retain the palettes selected by their programs.
                    tints.as_ref().filter(|_| {
                        effect.resource == common_id || effect.resource == techniques_id
                    }),
                    self.files.diagnostics(),
                )?;
                if effect.resource == common_id && stun_particle.is_some() {
                    render.members.insert(19);
                }
                Ok(render)
            })
            .collect::<Result<_>>()?;
        let head_anchors = rows
            .iter()
            .map(|row| {
                let mut anchor = row.profile.head_anchor;
                if let Some(model) = &row.model {
                    if matches!(row.source, model::ModelSource::Party(_)) {
                        anchor.bone = model
                            .skeleton
                            .bones
                            .iter()
                            .position(|bone| bone.name == "Bone_atama")
                            .and_then(|index| u8::try_from(index).ok())
                            .unwrap_or(0);
                    } else if usize::from(anchor.bone) >= model.skeleton.bones.len() {
                        anchor.bone = 0;
                    }
                }
                anchor
            })
            .collect();
        let model_definitions = rows.iter().map(|row| row.model.clone()).collect();
        let participants = rows.into_iter().map(|row| (row.actor, row.setup)).collect();
        let mut core =
            resonance_battle::PreparedBattle::new(participants, bindings, random.state())?
                .with_formation_count(self.setup.formations()?.len())?
                .with_enemy_presentations(enemy_presentations)?
                .with_hosted_participants(hosted_participants)?
                .with_entry_choices(entry_choices)?
                .with_arena_boundary()
                .with_hover_bobbing(hover_bobbing)?
                .with_unison_gauge(party.unison_gauge, enabled & 0x02 != 0, initial_unison_full)?
                .with_grade_rank(difficulty)
                .with_items(items)?;
        if let Some(defense_system) = defense_system {
            core = core.with_defense_system(defense_system)?;
        }
        #[cfg(feature = "test-support")]
        {
            core = core.configure_fixture(configure)?;
        }
        #[cfg(not(feature = "test-support"))]
        let _ = configure;
        core = core.with_camera(camera)?;
        if let Some(unison) = unison {
            core = core.with_unison(unison)?;
        }
        if self.events.is_none() {
            core = core.with_entry();
        }
        let mut lifecycle = battle::lifecycle::Lifecycle::new(Some(command::Setup::new(
            ids[..self.party_actors.len()].to_vec(),
            enabled,
            core.initial_actors(),
            party.battle_rules.coliseum,
        )?));
        core = core.with_escape(battle::escape::prepare(
            &voices,
            ids.iter().copied().zip(sources.iter().copied()),
            party,
            menus,
            enabled & 0x20 != 0,
            level_difference,
            &mut sound,
        )?);
        let learning_members = battle::learning::read_inputs(
            catalogue.clone(),
            party,
            core.initial_actors(),
            &party_roster,
            options.colette_state != 0,
        )?;
        core = core.with_technique_learning_members(learning_members)?;
        core = core.with_overlimit_boost(options.overlimit_boost);
        feedback.death = Some(battle::death::feedback(
            &voices, &sources, party, common_id, &mut sound,
        )?);
        feedback.contact_audio = Some(battle::contact_audio::prepare(
            &voices, &sources, &mut sound,
        )?);
        feedback.rescue_names = table.lethal_rescue_names.clone();
        feedback.landing = Some(resonance_battle::EffectAppearance {
            resource: common_id,
            member: 17,
        });
        let mut victory_voices = BTreeMap::new();
        if style.celebrate {
            for &(_, character) in &party_roster {
                let source = model::ModelSource::Party(character);
                let Some(profile) = &voices.profile(source)?.voices else {
                    continue;
                };
                let choices = profile
                    .victory
                    .iter()
                    .map(|pair| {
                        Ok([
                            voices.absolute(source, Some(pair[0]), &mut sound)?,
                            voices.absolute(source, Some(pair[1]), &mut sound)?,
                        ])
                    })
                    .collect::<Result<Vec<_>>>()?;
                if choices.iter().flatten().any(Option::is_some) {
                    victory_voices.insert(character, choices);
                }
            }
        }
        let mut groups = Vec::new();
        for group in victory.groups.iter().filter(|_| style.celebrate) {
            let Some(actor) = characters
                .iter()
                .position(|&id| id as u8 == group.character)
            else {
                continue;
            };
            let participants = (|| {
                group.validate()?;
                ensure!(
                    !groups
                        .iter()
                        .any(|row: &battle::results::PreparedGroup| row.id
                            == VictoryGroupId(group.id)),
                    "duplicate victory group identity"
                );
                Ok(&group.participants)
            })();
            let Some(participants) = self
                .files
                .diagnostics()
                .attempt("victory group", participants)?
            else {
                continue;
            };
            if !participants
                .iter()
                .all(|id| characters.iter().any(|&character| character as u8 == *id))
            {
                continue;
            }
            if let Some(voice) = voices.absolute(sources[actor], group.voice, &mut sound)? {
                groups.push(battle::results::PreparedGroup {
                    id: VictoryGroupId(group.id),
                    leader: Character::try_from(group.character)?,
                    participants: participants
                        .iter()
                        .copied()
                        .map(Character::try_from)
                        .collect::<Result<_>>()?,
                    required_leader: group.required_leader.map(Character::try_from).transpose()?,
                    condition: group.condition,
                    voice,
                });
            }
        }
        let results = battle::results::Setup {
            enemies: result_enemies,
            level_difference,
            actors: party_roster,
            formation: self.setup.formation()?,
            formation_count: self.setup.formations()?.len(),
            hunting_symbol: options.hunting_symbol,
            style,
            story: options
                .story
                .try_into()
                .context("negative battle story progress")?,
            colette_state: u32::try_from(options.colette_state)
                .context("negative Colette state")?
                .into(),
            victory_story_flags: options.victory_story_flags,
            devils_arms_unlocked: options.devils_arms_unlocked,
            groups,
            performances,
            postures,
            victory_voices,
        };
        let mut core = core.finish()?;
        core.set_diagnostics(self.files.diagnostics().clone());
        let mut model_player = resonance_battle::Models::new(
            core.actors(),
            model_definitions,
            equipment,
            self.files.diagnostics().clone(),
        )?;
        for model in independent_models {
            model_player.add_independent(model)?;
        }
        core.set_models(model_player);
        if let Some(plan) = &self.events {
            let event_bosses = self
                .enemies
                .spawns
                .iter()
                .enumerate()
                .filter_map(|(index, spawn)| {
                    let enemy = self.enemies.resources[spawn.resource].monster.id;
                    self.enemy_definitions[&enemy]
                        .actions
                        .policy
                        .event_hp_hold
                        .then_some(ids[self.party_actors.len() + index])
                })
                .collect::<Vec<_>>();
            ensure!(
                event_bosses.len() == 1,
                "battle event requires one held boss"
            );
            lifecycle =
                lifecycle.with_events(plan.clone(), party.clone(), event_bosses[0], &core)?;
        }
        Ok(Prepared {
            core,
            lifecycle,
            models,
            wings,
            effects,
            effect_banks,
            feedback,
            poison_effect: resonance_battle::EffectAppearance {
                resource: common_id,
                member: 44,
            },
            holy_song_effect: resonance_battle::EffectAppearance {
                resource: common_id,
                member: 43,
            },
            stun_particle,
            head_anchors,
            ambient_effect,
            passive_effects,
            trails: render_trails,
            characters,
            music: entry::music(&self.setup, u32::from(options.map), options.world_music),
            results,
        })
    }
}

/// Resolve saved assist targets against the admitted party and prepared actions.
fn prepare_assists(
    party: &resonance_events::party::Party,
    rows: &mut [ActorRow<'_>],
    ids: &[resonance_battle::ActorId],
) {
    for (slot, &character) in party.formation.iter().take(rows.len()).enumerate() {
        let member = &party.members[usize::from(character - 1)];
        for (assist, selected) in member.assist_shortcuts.into_iter().enumerate() {
            let Some(selected) = selected else {
                continue;
            };
            // Off-formation assignments stay saved until their target returns.
            let Some(target) = party
                .formation
                .iter()
                .take(rows.len())
                .position(|&id| usize::from(id - 1) == selected.character)
            else {
                continue;
            };
            let Some(action) = rows[target]
                .setup
                .techniques
                .iter()
                .find(|technique| technique.catalogue == TechniqueId(selected.technique))
                .map(|technique| technique.action)
            else {
                continue;
            };
            rows[slot].setup.assist_shortcuts[assist] = Some((ids[target], action));
        }
    }
}

fn render_weapon_layers(
    part: &battle_model::ModelPart,
    style: battle_profile::WeaponStyle,
    handles: &mut Handles,
    models: &mut Vec<RenderModel>,
) -> Vec<ResourceId> {
    part.layers
        .iter()
        .map(|layer| {
            let resource = handles.resource();
            let skeleton = part
                .layer_skeletons
                .get(&layer.scene.resource)
                .unwrap_or(&part.rig.skeleton);
            let mut layer = layer.clone();
            layer.attached_to = None;
            layer.animation = None;
            models.push(RenderModel {
                resource,
                skeleton: skeleton.clone(),
                parts: vec![layer],
                instances: render::RenderInstances::Fixed(1),
                lit: true,
                texture_channels: vec![],
                suppressed_nodes: BTreeSet::new(),
                weapon_style: Some(style),
                target_anchor: None,
            });
            resource
        })
        .collect()
}

#[cfg(test)]
mod tests;
