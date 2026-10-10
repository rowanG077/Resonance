//! Prepare one complete generation without changing the suspended session.
use super::{Inputs, Prepared, RenderModel, assets::EquipmentKind, render, resources::Resources};
use crate::battle::model::MotionRole;
use crate::battle::{self, EffectResource, command, entry, model};
use anyhow::{Context, Result, ensure};
use resonance_battle::{Sound, SpellChargeDefinition};
use resonance_content::{
    battle_model, battle_profile, menu_data::MenuData, model_preview::PreviewPart,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

const HAMMER_MODEL: u8 = 1;
const HAMMER_EFFECT: u16 = 14;
const BEAST_MODEL: u8 = 6;

/// Source state observed when the field asks to enter battle. The independent
/// battle seed is captured once; preparation may be retried without new draws.
pub struct PrepareOptions {
    /// Saved event flag 0x3FA; story acquisition remains field-owned.
    pub devils_arms_unlocked: bool,
    pub random_seed: u64,
    pub map: u16,
    pub world_music: i32,
    pub story: i32,
    pub colette_state: i32,
    /// Exact saved story flag 3 used by Colette's successor/base learning branches.
    pub story3: bool,
    pub victory_story_flags: [bool; 2],
    pub overlimit_boost: bool,
}

struct Handles {
    resource: u32,
}
impl Handles {
    fn new(first_resource: u32) -> Self {
        Self {
            resource: first_resource,
        }
    }

    fn resource(&mut self) -> u32 {
        let id = self.resource;
        self.resource += 1;
        id
    }
}

struct EquipmentAttachments {
    models: Vec<RenderModel>,
    render_trails: Vec<super::RenderTrail>,
    weapons: Vec<Arc<resonance_battle::WeaponDefinition>>,
}

impl Inputs {
    fn prepare_equipment(
        &self,
        character: u8,
        profile: &battle_profile::Profile,
        source: &battle_model::Party,
        item: u16,
        shield: bool,
        handles: &mut Handles,
    ) -> Result<EquipmentAttachments> {
        let weapon = self
            .weapons
            .get(&item)
            .context("missing selected weapon descriptor")?;
        // Sheena's four card attachments share two prepared model parts.
        let slots: Vec<_> = if character == battle::party::Character::Sheena as u8 && !shield {
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
            let playback = (character == battle::party::Character::Genis as u8)
                .then(battle::weapon::owner_linked);
            let definition =
                battle::weapon::prepare(&self.files, part, slot, bone, &resources, playback)?;
            let resource = definition.primary_resource();
            prepared.weapons.push(definition);
            if let Some(&material) = weapon.trails.get(&part_index)
                && let Some(render) = render::weapon_trail(
                    self.files.diagnostics(),
                    &part.rig.skeleton,
                    resource,
                    material,
                    self.common_effects.as_deref(),
                    None,
                )?
            {
                prepared.render_trails.push(render);
            }
        }
        if shield {
            ensure!(
                prepared.weapons.len() == 1 && prepared.weapons[0].slot == 1,
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
    fn prepare_control(&mut self, inputs: &Inputs, level_difference: i8) -> Result<()> {
        self.setup.overlimit_gain = self.profile.overlimit_gain;
        self.setup.extended_overlimit = self.extended_overlimit;
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
        setup.contact_recovery = !enemy || profile.traits.enemy_contact_recovery;
        setup.recovery_return = Some(resonance_battle::RecoveryReturnDefinition {
            speed: if enemy {
                profile.walk_speed
            } else {
                profile.run_speed
            },
            turn_ticks: profile.turn_ticks,
            disabled: enemy && party.settings.preferences.battle_rank >= 1,
        });
        if let model::ModelSource::Party(character) = *source {
            let actions = actions.as_ref().context("missing party actions")?;
            let techniques = &actions.techniques;
            setup.arte_chain_limit = matches!(
                battle::party::Character::try_from(character)?,
                battle::party::Character::Zelos | battle::party::Character::Kratos
            )
            .then_some(3);
            setup.techniques = battle::control::techniques(catalogue, character, techniques)?;
            let member = &party.members[usize::from(character - 1)];
            setup.disabled_techniques = member
                .disabled_techniques
                .iter()
                .filter_map(|catalogue| techniques.get(catalogue).copied())
                .collect();
            setup.companion = Some(battle::companion::prepare(
                table,
                character,
                member,
                level_difference,
            )?);
            let mut definition = battle::control::party(
                profile,
                character,
                actions.normals,
                battle::control::motions(model),
            )?;
            definition.shortcuts = member.shortcuts;
            if let Some(guard) = battle::martial::special_guard(character) {
                let catalogue = guard.catalogue;
                let action = techniques[&catalogue];
                setup.special_guard = Some(action);
            }
            setup.control = Some(Arc::new(definition));
        }
        Ok(())
    }
    fn prepare_feedback(
        &mut self,
        voices: &battle::voice::Resolver<'_>,
        common_id: u32,
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
        let controls = if let model::ModelSource::Party(character) = *source {
            battle::feedback::ActorFeedback {
                overlimit_voice: voices.absolute(
                    *source,
                    voices
                        .party_table()
                        .overlimit_voices
                        .get(usize::from(character - 1))
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
    pub character: u8,
    pub actor: resonance_battle::Actor,
    pub model: Option<Arc<resonance_battle::ModelDefinition>>,
    equipment: BTreeMap<u16, Vec<Arc<resonance_battle::WeaponDefinition>>>,
    performances: Vec<battle::victory::Performance>,
    models: Vec<RenderModel>,
    render_trails: Vec<super::RenderTrail>,
    pub next_resource: u32,
}

/// One selected enemy instance with its body and carried artwork.
pub struct PreparedEnemyMember {
    pub actor: resonance_battle::Actor,
    pub model: Option<Arc<resonance_battle::ModelDefinition>>,
    pub next_resource: u32,
    models: Vec<RenderModel>,
    render_trails: Vec<super::RenderTrail>,
}

impl Inputs {
    pub fn prepare_enemy_member(
        &self,
        spawn: usize,
        resource: u32,
        first_resource: u32,
        random: &mut resonance_battle::Random,
    ) -> Result<PreparedEnemyMember> {
        let spawn = self
            .enemies
            .spawns
            .get(spawn)
            .context("missing enemy spawn")?;
        if let Some(reason) = &spawn.unsupported_reason {
            anyhow::bail!("{reason}");
        }
        let monster = &self.enemies.resources[spawn.resource].monster;
        let definition = &self.enemy_definitions[&monster.id];
        let stats = battle::enemy::statistics(
            &monster.statistics[spawn.variant],
            self.party.settings.preferences.battle_rank,
        )?;
        let actor = battle::enemy::actor(
            monster,
            &definition.profile,
            definition.guard_recovery_bonus,
            stats,
            &self.recoil,
        )?;
        let mut model = self
            .enemy_models
            .get(&monster.id)
            .map(|(source, motions)| {
                let body = (|| {
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
                                motions,
                                actor.hp,
                                actor.equipment.max_hp,
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
        if let Some(model) = &mut model {
            let source = &self.enemy_models[&monster.id].0;
            models.push(RenderModel {
                resource,
                skeleton: source.body.skeleton.clone(),
                parts: monster
                    .preview
                    .parts
                    .iter()
                    .filter(|part| part.attached_to.is_none())
                    .cloned()
                    .collect(),
                capacity: 1,
                lit: true,
                effect: false,
                texture_channels: definition.profile.texture_channels.clone(),
                suppressed_nodes: render::suppressed_body_nodes(
                    &source.body.skeleton,
                    definition.profile.traits.show_body_attachments,
                ),
                weapon_style: None,
            });
            for (&slot, part) in &source.attachments {
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
                    let weapon =
                        battle::weapon::prepare(&self.files, part, slot, bone, &resources, None)?;
                    Ok(weapon)
                })();
                let Some(weapon) = self
                    .files
                    .diagnostics()
                    .attempt("enemy attachment", prepared)?
                else {
                    continue;
                };
                let resource = weapon.primary_resource();
                Arc::get_mut(model)
                    .context("enemy body shared before attachment")?
                    .weapons
                    .push(weapon);
                models.extend(render);
                if let Some(&material) = source.trails.get(&slot)
                    && let Some(render) = render::weapon_trail(
                        self.files.diagnostics(),
                        &part.rig.skeleton,
                        resource,
                        material,
                        self.common_effects.as_deref(),
                        self.enemy_effects.get(&monster.id).map(Arc::as_ref),
                    )?
                {
                    render_trails.push(render);
                }
            }
        }
        Ok(PreparedEnemyMember {
            actor,
            model,
            next_resource: handles.resource,
            models,
            render_trails,
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
        first_resource: u32,
        victory: Option<&resonance_content::battle_victory::Performances>,
    ) -> Result<PreparedPartyMember> {
        let input = self.party_actors.get(slot).context("missing party input")?;
        let profile = self
            .party_profiles
            .records
            .get(usize::from(input.character - 1))
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
        if let (Some(source), Some(setup)) = (&input.source, setup.model) {
            let resource = setup.resource;
            let prepared = (|| {
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
                    capacity: 1,
                    lit: true,
                    effect: false,
                    texture_channels: render::party_texture_channels(
                        &profile.texture_channels,
                        source.parts.first().context("missing party body")?,
                    )?,
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
            if let Some((prepared, render)) = self
                .files
                .diagnostics()
                .attempt("party body artwork", prepared)?
            {
                body = Some(prepared);
                models.push(render);
            }
        }
        let current = &self.party.members[usize::from(character - 1)];
        let performances =
            if let (Some(victory), Some(source), Some(model)) = (victory, &input.source, &body) {
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
        if let (Some(source), Some(body)) = (&input.source, &mut body) {
            let body = Arc::get_mut(body).context("party body shared before attachment")?;
            for (&item, &kind) in &input.equipment {
                let shield = matches!(kind, EquipmentKind::Shield);
                let component =
                    self.prepare_equipment(character, profile, source, item, shield, &mut handles);
                let Some(mut component) = self.files.diagnostics().attempt(
                    "battle equipment artwork",
                    component.with_context(|| {
                        format!("equipment item {item} for character {character}")
                    }),
                )?
                else {
                    continue;
                };
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

        mut sound: impl FnMut(battle::voice::Sound) -> Result<Option<Sound>>,
    ) -> Result<Prepared> {
        let party = &self.party;
        let style = battle::results::ResultStyle {
            play_music: self.enemies.settings.play_music,
            celebrate: self.enemies.settings.celebrate,
        };
        // Reject unimplemented E7 damage/reward bits before preparing any
        // candidate resources, including caller-made Party values from tests.
        let initial_unison_full = party.initial_unison_full()?;
        ensure!(
            party.settings.preferences.battle_rank <= 2,
            "invalid battle difficulty"
        );
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
        let mut handles = Handles { resource: 1 };
        let common_id = handles.resource();
        let techniques_id = handles.resource();
        for cue in [1, 2, 3, 4, 6, 7, 38] {
            let _ = sound(battle::voice::Sound::Cue(cue))?;
        }
        let mut models = Vec::new();
        let mut render_trails = Vec::new();
        let mut rows = Vec::new();
        let mut bindings = battle::ActionDefinitions::default();
        let mut feedback = battle::feedback::Feedback::default();
        let mut common_members = BTreeSet::from([9, 14, 15, 17, 19, 42, 44, 48, 50]);
        let mut performances = Vec::new();
        let mut prepared_equipment = BTreeMap::new();
        let mut postures = Vec::new();
        for (slot, input) in self.party_actors.iter().enumerate() {
            let character = input.character;
            let profile = table
                .records
                .get(usize::from(character - 1))
                .context("missing party actor profile")?;
            let member = &party.members[usize::from(character - 1)];
            let loadout = battle::party::loadout(menus, member, usize::from(character - 1))?;
            let motions = &input.motions;
            let initial = if input.source.is_some() {
                self.files.diagnostics().attempt(
                    "party entry pose",
                    entry::initial_playback(
                        &mut cosmetic_random,
                        &table.entry,
                        profile,
                        motions,
                        i32::from(member.hp),
                        loadout.attributes.max_hp,
                        member.ailments.petrified,
                    )
                    .with_context(|| format!("initial playback for character {character}")),
                )?
            } else {
                None
            };
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
            handles.resource = prepared.next_resource;
            models.append(&mut prepared.models);
            render_trails.append(&mut prepared.render_trails);
            let strategy = resonance_battle::TargetPolicy::try_from(if member.strategy[0] == 0 {
                table.default_strategy[usize::from(character - 1)][0]
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
                    automatic: character != battle::party::Character::Genis as u8,
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
                    ..Default::default()
                },
            });
        }
        let mut technique_models = BTreeMap::new();
        // Learning and equipment changes can enable these effects during combat.
        for (character, slot) in [
            (battle::party::Character::Colette, HAMMER_MODEL),
            (battle::party::Character::Presea, BEAST_MODEL),
        ] {
            if party
                .formation
                .iter()
                .take(4)
                .any(|&id| id == character as u8)
                && let Some(part) = self
                    .technique_effects
                    .as_ref()
                    .and_then(|bank| bank.art.as_ref())
                    .and_then(|art| art.models.get(&slot))
            {
                let resource = handles.resource();
                if let Some((model, render)) = render::effect_model(&self.files, part, resource)? {
                    technique_models.insert(slot, model);
                    models.push(render);
                }
            }
        }
        let hammer_effect = technique_models.contains_key(&HAMMER_MODEL).then_some(
            resonance_battle::EffectAppearance {
                resource: techniques_id,
                member: HAMMER_EFFECT,
            },
        );
        feedback.hammer = hammer_effect;
        let mut enemy_banks = BTreeMap::new();
        for enemy in self
            .enemies
            .resources
            .iter()
            .map(|resource| resource.monster.id)
        {
            let bank = self.enemy_effects.get(&enemy);
            let resource = handles.resource();
            let mut prepared = BTreeMap::new();
            for (&slot, part) in bank
                .into_iter()
                .filter_map(|bank| bank.art.as_ref())
                .flat_map(|art| &art.models)
            {
                let model_resource = handles.resource();
                if let Some((model, render)) =
                    render::effect_model(&self.files, part, model_resource)?
                {
                    prepared.insert(slot, model);
                    models.push(render);
                }
            }
            enemy_banks.insert(
                enemy,
                EffectResource {
                    bank: bank.cloned(),
                    resource,
                    members: vec![],
                    models: prepared,
                },
            );
        }
        let mut enemy_resources = BTreeMap::new();
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
                    existing.capacity += model.capacity;
                } else {
                    models.push(model);
                }
            }
            render_trails.extend(prepared.render_trails);
            let actor = prepared.actor;
            let model = prepared.model;
            enemy_entry_rows.push(source.entry_row);
            result_enemies.push(battle::results::PreparedEnemy {
                actor: resonance_battle::ActorId::from_index(rows.len())?,
                level: actor.equipment.stats.level,
                grade: monster.grade,
                reward: battle::rewards::EnemyReward::from_monster(monster, spawn.variant)?,
            });
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
                setup: Default::default(),
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
            catalogue,
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
            prepared: Default::default(),
            feedback: &mut feedback,
            fire_ball: None,
            sound: &mut sound,
        };
        resources.prepare()?;
        let mut enemy_actions = BTreeMap::new();
        for row in &mut rows {
            match row.source {
                model::ModelSource::Party(character) => {
                    let actions =
                        resources.party(character, row.model.as_deref(), &mut bindings)?;
                    let member = &party.members[usize::from(character - 1)];
                    for technique in member
                        .techniques
                        .iter()
                        .filter(|id| !actions.techniques.contains_key(id))
                    {
                        self.files.diagnostics().report("battle technique preparation",
                            anyhow::anyhow!("learned technique {technique} for character {character} has no executable action"))?;
                    }
                    if !actions.techniques.values().any(|&key| {
                        matches!(bindings[key].execution, battle::ActionExecution::Casting(_))
                    }) {
                        row.setup.spell_charge = None;
                    }
                    row.actions = Some(actions);
                }
                model::ModelSource::Enemy(enemy) => {
                    let source = &self.enemy_definitions[&enemy];
                    let actions = match enemy_actions.entry(enemy) {
                        std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                        std::collections::btree_map::Entry::Vacant(entry) => entry.insert(
                            resources.enemy(enemy, row.model.as_deref(), source, &mut bindings)?,
                        ),
                    };
                    let mut decision = battle::ai::definition(
                        source,
                        actions,
                        party.settings.preferences.battle_rank,
                    )?;
                    decision.walk_motion =
                        model::common_motion(row.model.as_deref(), MotionRole::Walk);
                    row.setup.enemy_decision = Some(decision);
                }
                _ => unreachable!("encounter rows contain party and enemy actors"),
            }
        }
        let action_resources = resources.prepared;
        let effect_banks = action_resources.prepare_effects(
            &mut |id| (sound)(battle::voice::Sound::Cue(id)),
            self.files.diagnostics(),
        )?;
        let ids = (0..rows.len())
            .map(resonance_battle::ActorId::from_index)
            .collect::<Result<Vec<_>>>()?;
        let characters: Vec<_> = rows
            .iter()
            .filter_map(|row| match row.source {
                model::ModelSource::Party(character) => Some(character),
                _ => None,
            })
            .collect();
        let party_roster: Vec<_> = ids
            .iter()
            .copied()
            .zip(characters.iter().copied())
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
        if self.enemies.settings.entry_voice {
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
            row.prepare_control(self, level_difference)?;
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
            adaptive: party.settings.preferences.battle_auto_zoom,
        };
        let enabled = command::enabled_rows(self.enemies.settings.escape_restricted, options.story)
            & !party.battle_rules.disabled_commands;
        let hover_bobbing = rows
            .iter()
            .zip(&ids)
            .filter_map(|(row, &id)| row.profile.traits.hover_bobbing.then_some(id))
            .collect();
        if let Some(tints) = &tints {
            feedback.recovery_tint = Some(tints.actors.recovery[..3].try_into().unwrap());
        }
        let (items, item_feedback) = battle::items::prepare(
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
        let effects = action_resources
            .effects
            .values()
            .filter_map(|effect| effect.bank.as_ref().map(|source| (effect, source)))
            .map(|(effect, source)| {
                render::effects(
                    effect.resource,
                    source,
                    &effect.members,
                    tints.as_ref(),
                    self.files.diagnostics(),
                )
            })
            .collect::<Result<_>>()?;
        let model_definitions = rows.iter().map(|row| row.model.clone()).collect();
        let participants = rows.into_iter().map(|row| (row.actor, row.setup)).collect();
        let mut core =
            resonance_battle::PreparedBattle::new(participants, bindings, random.state())?
                .with_entry_choices(entry_choices)?
                .with_arena_boundary()
                .with_hover_bobbing(hover_bobbing)?
                .with_unison_gauge(party.unison_gauge, enabled & 0x02 != 0, initial_unison_full)?
                .with_camera(camera)?
                .with_entry()
                .with_grade_rank(party.settings.preferences.battle_rank)?
                .with_items(items)?;
        let lifecycle = battle::lifecycle::Lifecycle::new(Some(command::Setup::new(
            ids[..self.party_actors.len()].to_vec(),
            enabled,
            core.initial_actors(),
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
            options.story3,
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
            for &character in &characters {
                let source = model::ModelSource::Party(character);
                let Some(profile) = &voices.profile(source)?.voices else {
                    continue;
                };
                let mut choices = Vec::new();
                for &line in &profile.victory {
                    if let Some(voice) = voices.absolute(source, Some(line), &mut sound)? {
                        choices.push(voice);
                    }
                }
                if !choices.is_empty() {
                    victory_voices.insert(character, choices);
                }
            }
        }
        let mut groups = Vec::new();
        for group in victory.groups.iter().filter(|_| style.celebrate) {
            let Some(actor) = characters.iter().position(|&id| id == group.character) else {
                continue;
            };
            let participants = (|| {
                group.validate()?;
                ensure!(
                    !groups
                        .iter()
                        .any(|row: &battle::results::PreparedGroup| row.id == group.id),
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
            if !participants.iter().all(|id| characters.contains(id)) {
                continue;
            }
            if let Some(voice) = voices.absolute(sources[actor], group.voice, &mut sound)? {
                groups.push(battle::results::PreparedGroup {
                    id: group.id,
                    leader: group.character,
                    participants: participants.to_vec(),
                    required_leader: group.required_leader,
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
            style,
            story: options
                .story
                .try_into()
                .context("negative battle story progress")?,
            colette_state: options
                .colette_state
                .try_into()
                .context("negative Colette state")?,
            victory_story_flags: options.victory_story_flags,
            devils_arms_unlocked: options.devils_arms_unlocked,
            groups,
            performances,
            postures,
            victory_voices,
        };
        let mut core = core.finish()?;
        core.set_diagnostics(self.files.diagnostics().clone());
        let model_player = resonance_battle::Models::new(
            core.actors(),
            model_definitions,
            equipment,
            self.files.diagnostics().clone(),
        )?;
        Ok(Prepared {
            core,
            model_player,
            lifecycle,
            models,
            effects,
            effect_banks,
            feedback,
            poison_effect: resonance_battle::EffectAppearance {
                resource: common_id,
                member: 44,
            },
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
                .find(|technique| technique.catalogue == selected.technique)
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
) -> Vec<u32> {
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
                capacity: 1,
                lit: true,
                effect: false,
                texture_channels: vec![],
                suppressed_nodes: BTreeSet::new(),
                weapon_style: Some(style),
            });
            resource
        })
        .collect()
}

#[cfg(test)]
mod tests;
