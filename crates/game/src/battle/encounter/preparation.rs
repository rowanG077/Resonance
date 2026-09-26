//! Prepare one complete generation without changing the suspended session.
use super::{
    ActorActions, Inputs, Prepared, RenderModel, render,
    resources::{ActorResources, Resources},
};
use crate::battle::{self, EffectResource, command, entry, model};
use anyhow::{Context, Result, bail, ensure};
use resonance_battle::{Control, ControlMotions, MotionBinding, ParticleDefinition, SoundBinding};
use resonance_content::{
    animation::Motion, arte, battle_effect, battle_model, battle_profile, menu_data::MenuData,
    model_preview::PreviewPart, prepared::Files,
};
use resonance_events::party::Party;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use symphonia_script_tools::PreparationCache;

/// Source state observed when the field asks to enter battle. The independent
/// battle seed is captured once; preparation may be retried without new draws.
pub struct PrepareOptions {
    pub random_seed: u32,
    pub map: u16,
    pub world_music: i32,
    pub story: i32,
    pub overlimit_boost: bool,
}

struct Handles {
    resource: u32,
    action: u16,
}
impl Handles {
    fn resource(&mut self) -> u32 {
        let id = self.resource;
        self.resource += 1;
        id
    }
    fn actions<const N: usize>(&mut self) -> [u16; N] {
        std::array::from_fn(|_| {
            let id = self.action;
            self.action += 1;
            id
        })
    }
}

struct ActorPlan {
    profile: battle_profile::Profile,
    normals: Option<[u16; 7]>,
    techniques: BTreeMap<u16, u16>,
    enemy_actions: Vec<u16>,
    fidget_ticks: u16,
    strategy: [u8; 3],
    initialize: Option<u16>,
    decision: Option<u16>,
}

impl Inputs {
    pub fn prepare(
        &self,
        menus: &MenuData,
        party: &Party,
        options: PrepareOptions,
        cache: &mut PreparationCache,
        mut sound: impl FnMut(battle::voice::Sound) -> Result<SoundBinding>,
    ) -> Result<Prepared> {
        menus.validate()?;
        ensure!(
            party.settings.preferences.battle_rank == 0,
            "non-Normal battle statistics are not prepared"
        );
        ensure!(
            self.party_positions.len() == party.formation.len().min(4),
            "prepared formation changed"
        );
        ensure!(
            !self.enemies.spawns.is_empty(),
            "battle has no enemy actors"
        );
        let table: battle_profile::Table = self.files.json(battle_profile::PARTY_PATH)?;
        let previous_formation = entry::previous_formation(
            self.files.diagnostics(),
            self.enemies.flags,
            party.battles.previous_formation,
        )?;
        let catalogue: arte::Catalogue = self.files.json(arte::PATH)?;
        let tints: battle_effect::Tints = self.files.json(battle_effect::TINTS_PATH)?;
        let victory: resonance_content::battle_victory::Performances =
            self.files.json(resonance_content::battle_victory::PATH)?;
        let mut random = entry::Random::from_state(options.random_seed);
        // The admitted field native rejects every nonzero route override, so
        // request byte 0x0d is zero. 5C38 constructs the frozen-screen pieces
        // before 40C8 initializes any actor, sharing this same random stream.
        let entry_transition = battle::entry_transition::EntryTransition::new(
            &table.entry.screen_break,
            table.entry.fade_color,
            &mut random,
            sound(battle::voice::Sound::Cue(130))?,
        )?;
        let mut handles = Handles {
            resource: 1,
            action: 1,
        };
        let common_id = handles.resource();
        let techniques_id = handles.resource();
        let stun = Arc::new(ParticleDefinition {
            resource: common_id,
            member: 19,
            model: None,
            data: self.common_effects.particle(19)?,
        });
        let stun_sound = sound(battle::voice::Sound::Cue(117))?;
        // 9E38 command-strip events use the common battle menu score bank.
        // Bind every Stage 3 cue before activation; the live owner never
        // performs a late audio lookup.
        for cue in 1..=4 {
            let _ = sound(battle::voice::Sound::Cue(cue))?;
        }
        let mut models = Vec::new();
        let mut render_trails = Vec::new();
        let mut trails = Vec::new();
        let mut actors = Vec::new();
        let mut actor_resources = Vec::new();
        let mut plans = Vec::new();
        let mut bindings = Vec::new();
        let mut admission_flashes = BTreeMap::new();
        let mut setups = Vec::new();
        let mut common_members = BTreeSet::from([14, 17, 20]);
        let mut elements = BTreeSet::new();
        let mut contact_elements = BTreeSet::from([None]);
        let mut fire_ball = None;
        for (slot, &character) in party.formation.iter().take(4).enumerate() {
            let profile = battle::profile::party_template(&self.files, character)?;
            let source: battle_model::Party =
                self.files.json(&battle_model::party_path(character))?;
            let member = &party.members[usize::from(character - 1)];
            let stats = member.stats_for(menus, usize::from(character - 1));
            let motions = motions(
                &self.files,
                source.parts.first().context("missing party body")?,
            )?;
            let initial = entry::initial_playback(
                &mut random,
                &table.entry,
                &profile,
                &motions,
                i32::from(member.hp),
                i32::from(stats.hp),
            )?;
            let resource = handles.resource();
            models.push(RenderModel {
                resource,
                skeleton: source.body.skeleton.clone(),
                capacity: 1,
                lit: true,
                effect: false,
                texture_channels: profile.texture_channels.clone(),
                suppressed_nodes: render::suppressed_body_nodes(
                    &source.body.skeleton,
                    profile.flags,
                ),
                weapon_flags: None,
                parts: source
                    .parts
                    .into_iter()
                    .map(|scene| PreviewPart {
                        scene,
                        animation: None,
                        attached_to: None,
                        additive: false,
                        uv_offsets: vec![],
                    })
                    .collect(),
            });
            setups.push(battle::party::Setup {
                position: self.party_positions[slot],
                heading: 0.,
                model: model::ModelSetup {
                    resource,
                    initial: initial.playback,
                    suppress_root_translation: [false; 3],
                    stun: Some(model::StunResources {
                        particle: stun.clone(),
                        sound: stun_sound,
                    }),
                },
            });
            let normals = handles.actions();
            bindings.extend(match character {
                1 => battle::normal::lloyd_bindings(&self.files, normals)?,
                2 => battle::normal::colette_bindings(&self.files, normals)?,
                3 => battle::normal::genis_bindings(&self.files, normals)?,
                _ => bail!("normal attacks for character {character} are not prepared"),
            });
            let mut techniques = BTreeMap::new();
            for &technique in &member.techniques {
                let [id] = handles.actions();
                match (character, technique) {
                    (1, 1) | (2, 35) => {
                        bindings.push(battle::martial::binding(&self.files, technique, id)?);
                        let flags = catalogue.definition(usize::from(technique))?.flags;
                        if let Some(color) =
                            battle::contact_feedback::admission(&tints, flags, true)
                        {
                            admission_flashes.insert(id, color);
                        }
                    }
                    (3, 66) => {
                        let [resident] = handles.actions();
                        bindings.extend(battle::fire_ball::bindings(&self.files, id, resident)?);
                        fire_ball = Some(resident);
                        let definition = catalogue.definition(usize::from(technique))?;
                        let pulse = if definition.flags & 0x00400000 != 0 {
                            3
                        } else if definition.flags & 0x00800000 != 0 {
                            4
                        } else {
                            5
                        };
                        common_members.extend([pulse, if pulse == 4 { 8 } else { 7 }]);
                    }
                    _ => bail!(
                        "learned technique {technique} for character {character} is not prepared"
                    ),
                }
                let element = catalogue.definition(usize::from(technique))?.element;
                elements.insert(element);
                if (1..=8).contains(&element) {
                    contact_elements.insert(Some(
                        resonance_battle::Element::ALL[usize::from(element - 1)],
                    ));
                }
                techniques.insert(technique, id);
            }
            let [decision] = handles.actions();
            bindings.push(if party.settings.battle_controls[slot] == 2 {
                battle::companion::binding(character, decision)?
            } else {
                battle::companion::reevaluation_binding(character, decision)?
            });
            plans.push(ActorPlan {
                profile,
                normals: Some(normals),
                techniques,
                enemy_actions: vec![],
                fidget_ticks: initial.fidget_ticks,
                strategy: std::array::from_fn(|column| {
                    if member.strategy[column] == 0 {
                        table.default_strategy[usize::from(character - 1)][column]
                    } else {
                        member.strategy[column]
                    }
                }),
                initialize: None,
                decision: Some(decision),
            });
        }
        let mut technique_models = BTreeMap::new();
        let mut performances = Vec::new();
        let mut postures = Vec::new();
        for mut member in battle::party::prepare(&self.files, menus, party, setups)? {
            let mut weapon_resources = BTreeMap::new();
            let mut actor_trails = Vec::new();
            for weapon in &member.weapons {
                let source = model::weapon(&self.files, weapon.item)?;
                let part = source
                    .parts
                    .get(&weapon.part)
                    .context("missing equipped weapon part")?;
                let resource = handles.resource();
                weapon_resources.insert(weapon.slot, resource);
                models.push(RenderModel {
                    resource,
                    skeleton: part.rig.skeleton.clone(),
                    parts: sampled_parts(part),
                    capacity: 1,
                    lit: true,
                    effect: false,
                    texture_channels: vec![],
                    suppressed_nodes: BTreeSet::new(),
                    weapon_flags: Some(
                        *plans[actors.len()]
                            .profile
                            .weapon_draw_flags
                            .get(usize::from(weapon.slot))
                            .context("missing equipped weapon draw flags")?,
                    ),
                });
                if let Some(&material) = source.trails.get(&weapon.part) {
                    let resource = handles.resource();
                    if let Some(definition) =
                        battle::trail::weapon(part, weapon.slot, resource, member.character)?
                    {
                        actor_trails.push(definition);
                        render_trails.push(render::trail(
                            resource,
                            material,
                            &self.common_effects,
                            None,
                        )?);
                    }
                }
                if member.character == 2 && weapon.slot == 0 {
                    let resource = handles.resource();
                    technique_models.insert(0, model::effect(&self.files, part, resource)?);
                    models.push(RenderModel {
                        resource,
                        skeleton: part.rig.skeleton.clone(),
                        parts: sampled_parts(part),
                        capacity: 416,
                        lit: true,
                        effect: true,
                        texture_channels: vec![],
                        suppressed_nodes: BTreeSet::new(),
                        weapon_flags: None,
                    });
                }
            }
            let contacts = member.attach_weapons(&self.files, &weapon_resources)?;
            let performance_ids = handles.actions();
            bindings.extend(battle::victory::bindings(
                member.character,
                performance_ids,
            )?);
            let (model, selected) = battle::victory::prepare(
                &self.files,
                member.character,
                &member.model,
                performance_ids,
            )?;
            member.model = model;
            performances.extend(selected);
            let posture_ids = handles.actions();
            bindings.extend(battle::victory::posture_bindings(
                member.character,
                posture_ids,
            )?);
            postures.push(battle::victory::prepare_posture(
                &self.files,
                member.character,
                &member.model,
                posture_ids,
            )?);
            elements.insert(
                member
                    .actor
                    .elements
                    .base
                    .map_or(0, |element| element as u8 + 1),
            );
            contact_elements.insert(member.actor.elements.base);
            actors.push(member.actor);
            trails.push(actor_trails);
            actor_resources.push(ActorResources {
                source: model::ModelSource::Party(member.character),
                model: member.model,
                contacts,
                death: [None; 2],
            });
        }
        let mut enemy_banks = BTreeMap::new();
        let mut enemy_bank_sources = BTreeMap::new();
        for (&enemy, bank) in &self.enemy_effects {
            let resource = handles.resource();
            let mut prepared = BTreeMap::new();
            for (&slot, part) in &bank
                .art
                .as_ref()
                .context("missing enemy effect artwork")?
                .models
            {
                let model_resource = handles.resource();
                prepared.insert(slot, model::effect(&self.files, part, model_resource)?);
                models.push(RenderModel {
                    resource: model_resource,
                    skeleton: part.rig.skeleton.clone(),
                    parts: sampled_parts(part),
                    capacity: 416,
                    lit: true,
                    effect: true,
                    texture_channels: vec![],
                    suppressed_nodes: BTreeSet::new(),
                    weapon_flags: None,
                });
            }
            enemy_banks.insert(
                enemy,
                EffectResource {
                    source: battle_model::enemy_effects_path(enemy),
                    resource,
                    members: vec![],
                    scene: None,
                    models: prepared,
                },
            );
            enemy_bank_sources.insert(resource, bank);
        }
        let mut enemy_resources = BTreeMap::new();
        let mut enemy_actions: BTreeMap<u8, Vec<u16>> = BTreeMap::new();
        let mut enemy_tasks = BTreeMap::new();
        let mut enemy_rewards = Vec::new();
        let mut enemy_levels = Vec::new();
        let mut enemy_grades = Vec::new();
        for spawn in &self.enemies.spawns {
            let monster = &self.enemies.resources[spawn.resource];
            let source: battle_model::Enemy =
                self.files.json(&battle_model::enemy_path(monster.id))?;
            ensure!(
                spawn.appearance == 0 && spawn.attachments == [0; 2],
                "encounter appearance override is not prepared"
            );
            let position = spawn
                .position
                .context("automatic enemy placement is not prepared")?;
            let stats = &monster.statistics[spawn.variant];
            let hp = if stats.initial_hp == 0 {
                stats.hp
            } else {
                stats.initial_hp
            };
            let motions = motions(&self.files, &monster.preview.parts[0].scene)?;
            let initial = entry::initial_playback(
                &mut random,
                &table.entry,
                &source.profile,
                &motions,
                hp.try_into()?,
                stats.hp.try_into()?,
            )?;
            let resource = if let Some(&resource) = enemy_resources.get(&monster.id) {
                models
                    .iter_mut()
                    .find(|model| model.resource == resource)
                    .unwrap()
                    .capacity += 1;
                resource
            } else {
                let resource = handles.resource();
                enemy_resources.insert(monster.id, resource);
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
                    texture_channels: source.profile.texture_channels.clone(),
                    suppressed_nodes: render::suppressed_body_nodes(
                        &source.body.skeleton,
                        source.profile.flags,
                    ),
                    weapon_flags: None,
                });
                resource
            };
            let (mut actor, mut model) = model::enemy(
                &self.files,
                monster,
                spawn.variant,
                model::ModelSetup {
                    resource,
                    initial: initial.playback,
                    suppress_root_translation: [false; 3],
                    stun: Some(model::StunResources {
                        particle: stun.clone(),
                        sound: stun_sound,
                    }),
                },
            )?;
            actor.position = [
                f32::from(position[0]),
                source.profile.ground_offset.finite()?,
                f32::from(position[1]),
            ];
            let mut contacts = groups(&source.body);
            let mut actor_trails = Vec::new();
            for (&slot, part) in &source.attachments {
                let resource = handles.resource();
                models.push(RenderModel {
                    resource,
                    skeleton: part.rig.skeleton.clone(),
                    parts: sampled_parts(part),
                    capacity: 1,
                    lit: true,
                    effect: false,
                    texture_channels: vec![],
                    suppressed_nodes: BTreeSet::new(),
                    weapon_flags: Some(
                        *source
                            .profile
                            .weapon_draw_flags
                            .get(usize::from(slot))
                            .context("missing enemy weapon draw flags")?,
                    ),
                });
                let attached = battle::weapon::attach(
                    &self.files,
                    Arc::make_mut(&mut model),
                    part,
                    slot,
                    *source
                        .body
                        .attachments
                        .get(&slot)
                        .context("missing enemy attachment bone")?,
                    resource,
                    resonance_battle::WeaponPlayback::Rigid,
                )?;
                if contacts.iter().all(Vec::is_empty) {
                    contacts = attached;
                } else {
                    contacts.extend(attached);
                }
                // The enemy publication retains its source member-zero material.
                if let Some(&material) = source.trails.get(&slot) {
                    let resource = handles.resource();
                    if let Some(definition) = battle::trail::weapon(part, slot, resource, 0)? {
                        actor_trails.push(definition);
                        render_trails.push(render::trail(
                            resource,
                            material,
                            &self.common_effects,
                            self.enemy_effects.get(&monster.id),
                        )?);
                    }
                }
            }
            let action_ids = if let Some(ids) = enemy_actions.get(&monster.id) {
                ids.clone()
            } else {
                let actions = match monster.id {
                    36 => battle::enemy::zombie_bindings(&self.files, handles.actions())?,
                    49 => battle::enemy::ghost_bindings(&self.files, handles.actions())?,
                    id => bail!("enemy {id} behavior is not prepared"),
                };
                let ids: Vec<_> = actions.iter().map(|action| action.id).collect();
                bindings.extend(actions);
                enemy_actions.insert(monster.id, ids.clone());
                ids
            };
            let [initialize, decision] = if let Some(&tasks) = enemy_tasks.get(&monster.id) {
                tasks
            } else {
                let tasks = handles.actions();
                bindings.extend(battle::ai::bindings(
                    u16::from(monster.id),
                    tasks[0],
                    tasks[1],
                )?);
                enemy_tasks.insert(monster.id, tasks);
                tasks
            };
            enemy_rewards.push(battle::rewards::EnemyReward::from_monster(
                monster,
                spawn.variant,
            )?);
            enemy_levels.push(stats.level);
            enemy_grades.push(monster.grade);
            elements.insert(actor.elements.base.map_or(0, |element| element as u8 + 1));
            contact_elements.insert(actor.elements.base);
            actors.push(actor);
            trails.push(actor_trails);
            plans.push(ActorPlan {
                profile: source.profile,
                normals: None,
                techniques: BTreeMap::new(),
                enemy_actions: action_ids,
                fidget_ticks: initial.fidget_ticks,
                strategy: [source.target_strategy, 0, 0],
                initialize: Some(initialize),
                decision: Some(decision),
            });
            actor_resources.push(ActorResources {
                source: model::ModelSource::Enemy(monster.id),
                model,
                contacts,
                death: [None; 2],
            });
        }
        let [fall, initial] = handles.actions();
        bindings.extend(battle::death::bindings(fall, initial));
        let [notice_action] = handles.actions();
        bindings.push(battle::ActionBinding {
            id: notice_action,
            phase: resonance_battle::ActionPhase::Resident,
            module: "battle::result_notice".into(),
            entry: "run".into(),
            duration: 0,
            tp_cost: 0,
        });
        let entry_voice = previous_formation.map(|previous| {
            let [action] = handles.actions();
            bindings.push(battle::ActionBinding {
                id: action,
                phase: resonance_battle::ActionPhase::Decision,
                module: "battle::entry_voice".into(),
                entry: "initialize".into(),
                duration: 0,
                tp_cost: 0,
            });
            (action, previous)
        });
        let mut deaths = Vec::new();
        for (plan, actor) in plans.iter().zip(&mut actor_resources) {
            let (binding, motion) =
                battle::death::prepare(&plan.profile, &actor.model, fall, initial);
            deaths.push(Some(binding));
            actor.death = motion;
        }
        let (contact_feedback, contact_members) =
            battle::contact_feedback::prepare(&tints, common_id, &contact_elements)?;
        common_members.extend(contact_members);
        let mut resources = Resources {
            files: &self.files,
            actors: &actor_resources,
            common: EffectResource {
                source: battle_effect::COMMON_PATH.into(),
                resource: common_id,
                members: common_members.into_iter().collect(),
                scene: None,
                models: BTreeMap::new(),
            },
            techniques: EffectResource {
                source: battle_effect::TECHNIQUES_PATH.into(),
                resource: techniques_id,
                members: vec![],
                scene: None,
                models: technique_models,
            },
            enemy_effects: enemy_banks,
            selected: BTreeMap::new(),
            fire_ball,
            performances: &performances,
            sound: &mut sound,
        };
        let prepared = battle::prepare(
            cache,
            &self.files,
            &bindings,
            actors.clone(),
            random.state(),
            &mut resources,
            actor_resources
                .iter()
                .map(|actor| Some(actor.model.clone()))
                .collect(),
        )?;
        let mut core = Arc::try_unwrap(prepared)
            .map_err(|_| anyhow::anyhow!("battle candidate was shared before activation"))?;
        let ids: Vec<_> = core.actor_ids().collect();
        let enemy_definitions = actor_resources
            .iter()
            .enumerate()
            .filter_map(|(index, actor)| {
                if let model::ModelSource::Enemy(id) = actor.source {
                    Some((index, id))
                } else {
                    None
                }
            })
            .map(|(index, id)| {
                battle::ai::definition(
                    &self.files,
                    u16::from(id),
                    ids[index],
                    &plans[index].enemy_actions,
                    party.settings.preferences.battle_rank,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        let level_difference = ((actors
            .iter()
            .take(self.party_positions.len())
            .map(|a| i32::from(a.stats.level))
            .sum::<i32>()
            / self.party_positions.len() as i32)
            - (enemy_levels
                .iter()
                .map(|&level| i32::from(level))
                .sum::<i32>()
                / enemy_levels.len() as i32))
            .clamp(-8, 8) as i8;
        if let Some((action, previous)) = entry_voice {
            core = core.with_entry_voice(resonance_battle::EntryVoiceDefinition {
                action,
                repeated_formation: previous == self.setup.encounter,
                major_enemy: plans
                    .iter()
                    .skip(self.party_positions.len())
                    .any(|plan| plan.profile.camera_category >= 3),
                enemy_count: enemy_levels.len().try_into()?,
                level_difference,
            })?;
        }
        core = core
            .with_enemy_decisions(enemy_definitions)?
            .with_entry_timers(
                plans
                    .iter()
                    .map(|plan| resonance_battle::EntryTimers {
                        idle_variation: u16::from(plan.profile.idle_variation),
                        fidget_ticks: plan.fidget_ticks,
                    })
                    .collect(),
            )?
            .with_entry_choices(
                plans
                    .iter()
                    .enumerate()
                    .map(|(index, plan)| resonance_battle::EntryChoice {
                        actor: ids[index],
                        strategy: plan.strategy[0],
                        action: plan.initialize,
                        idle_ticks: u16::from(plan.profile.idle_ticks),
                        idle_variation: u16::from(plan.profile.idle_variation),
                    })
                    .collect(),
            )?;
        let targets: Vec<_> = core.initial_targets().iter().map(|id| id.index()).collect();
        let mut controls = Vec::new();
        let mut decisions = Vec::new();
        let mut companions = Vec::new();
        let mut recovery_returns = Vec::new();
        let mut actor_actions = Vec::new();
        for (index, plan) in plans.iter().enumerate() {
            let actor = &actors[index];
            if let Some(action) = plan.decision {
                decisions.push(resonance_battle::DecisionDefinition {
                    actor: ids[index],
                    target: ids[targets[index]],
                    action,
                    idle_motion: Some(MotionBinding {
                        model: actor_resources[index].model.resource,
                        clip: 0,
                    }),
                    idle_ticks: u16::from(plan.profile.idle_ticks),
                    idle_variation: u16::from(plan.profile.idle_variation),
                    fidget_ticks: plan.fidget_ticks,
                });
            }
            if let model::ModelSource::Party(character) = actor_resources[index].source
                && matches!(character, 2 | 3)
            {
                companions.push(battle::companion::prepare(
                    &self.files,
                    character,
                    ids[index],
                    &party.members[usize::from(character - 1)],
                    &plan.techniques,
                    level_difference,
                )?);
            }
            if matches!(actor.control, Control::Auto | Control::Enemy) {
                let model = &actor_resources[index].model;
                let enemy = actor.control == Control::Enemy;
                // Prepared opening enemy rows exclude the source running flag.
                // Their recovery walk and the party recovery run remain distinct.
                let clip = if enemy || !model.motions.contains_key(&19) {
                    1
                } else {
                    19
                };
                recovery_returns.push(resonance_battle::RecoveryReturnDefinition {
                    actor: ids[index],
                    motion: Some(MotionBinding {
                        model: model.resource,
                        clip,
                    }),
                    stop: (!enemy).then_some(MotionBinding {
                        model: model.resource,
                        clip: 18,
                    }),
                    speed: if enemy {
                        plan.profile.walk_speed
                    } else {
                        plan.profile.run_speed
                    }
                    .finite()?,
                    run_speed: plan.profile.run_speed.finite()?,
                    motion_rate: 0.5,
                    turn_ticks: plan.profile.turn_ticks,
                    disabled: enemy && party.settings.preferences.battle_rank >= 1,
                });
            }
            if matches!(
                actor.control,
                Control::Manual | Control::SemiAuto | Control::Auto
            ) {
                let character = party.formation[index];
                let model = &actor_resources[index].model;
                let motion = |clip| -> Result<_> {
                    ensure!(
                        model.motions.contains_key(&clip),
                        "missing player control motion {clip}"
                    );
                    Ok(MotionBinding {
                        model: model.resource,
                        clip,
                    })
                };
                let combo = if matches!(character, 2 | 3 | 4 | 7 | 8) {
                    2
                } else {
                    3
                };
                let mut definition = battle::control::party(
                    &self.files,
                    character,
                    ids[index],
                    ids[targets[index]],
                    plan.normals.context("missing controlled normal actions")?,
                    ControlMotions {
                        idle: motion(0)?,
                        walk: motion(1)?,
                        run: motion(if model.motions.contains_key(&19) {
                            19
                        } else {
                            1
                        })?,
                        stop: motion(18)?,
                        landing: motion(16)?,
                    },
                    combo,
                )?;
                definition.shortcuts = battle::control::shortcuts(
                    &self.files,
                    character,
                    &party.members[usize::from(character - 1)],
                    &plan.techniques,
                )?;
                controls.push(definition);
            }
            actor_actions.push(ActorActions {
                actor: ids[index],
                target: ids[targets[index]],
                normals: plan.normals,
                techniques: plan.techniques.clone(),
                enemy_actions: plan.enemy_actions.clone(),
                fidget_ticks: plan.fidget_ticks,
                strategy: plan.strategy,
            });
        }
        let (camera, entry_camera) = entry::camera(
            &self.files,
            ids[0],
            ids[targets[0]],
            self.stage.camera_pitch_offset,
            party.settings.preferences.battle_auto_zoom,
        )?;
        core = core
            .with_arena_boundary()
            .with_hover_bobbing(
                plans
                    .iter()
                    .zip(&ids)
                    .filter_map(|(plan, &id)| (plan.profile.flags & 2 != 0).then_some(id))
                    .collect(),
                &self.ui.sine[..360],
            )?
            .with_landing_effect(resonance_battle::EffectAppearance {
                resource: common_id,
                member: 17,
            })?
            .with_blinking(
                plans
                    .iter()
                    .map(|plan| {
                        plan.profile.flags & 0x8000 != 0
                            && !plan.profile.texture_channels.is_empty()
                    })
                    .collect(),
            )?
            .with_idle_expressions(
                plans
                    .iter()
                    .map(|plan| {
                        (!plan.profile.texture_channels.is_empty())
                            .then_some(plan.profile.idle_expression)
                    })
                    .collect(),
            )?
            .with_deaths(deaths)?
            .with_trails(trails)?
            .with_controls(controls)?
            .with_companions(companions)?
            .with_decisions(decisions)?
            .with_recovery_returns(recovery_returns)?
            .with_contact_feedback(contact_feedback)?
            .with_admission_flashes(admission_flashes)?
            .with_entry_camera(camera, entry_camera)?
            .with_result_camera(victory.camera)?
            .with_grade_rank(party.settings.preferences.battle_rank)?
            .with_ambient_color(self.stage.actor_color[..3].try_into().unwrap())
            .with_stage_colors(self.stage.color, self.stage.model_colors())
            .with_voices_enabled(party.settings.preferences.battle_voiceover);
        let mut effects = vec![render::effects(
            common_id,
            &self.common_effects,
            resources
                .selected
                .get(&common_id)
                .unwrap_or(&BTreeSet::new()),
            &[19],
            &tints,
            &elements,
        )?];
        if let Some(members) = resources.selected.get(&techniques_id) {
            effects.push(render::effects(
                techniques_id,
                &self.technique_effects,
                members,
                &[],
                &tints,
                &elements,
            )?);
        }
        for (resource, source) in enemy_bank_sources {
            if let Some(members) = resources.selected.get(&resource) {
                effects.push(render::effects(
                    resource,
                    source,
                    members,
                    &[],
                    &tints,
                    &elements,
                )?);
            }
        }
        // 1E12C sets command-block10C2.0x10 only for arte flags0x20.
        // Every prepared party arte must preserve the clear-bit opening domain.
        for plan in plans.iter().take(self.party_positions.len()) {
            for &technique in plan.techniques.keys() {
                ensure!(
                    catalogue.definition(usize::from(technique))?.flags & 0x20 == 0,
                    "battle command blocking for technique {technique} is not prepared"
                );
            }
        }
        // 5878 starts atFF. The current session initializes1F46/1E3C to zero;
        // their field setter(native_e7) is unsupported and cannot silently
        // mutate accepted Stage3 state. Formation and story remain real inputs.
        let enabled = command::enabled_rows(self.enemies.escape_restricted, options.story);
        let lifecycle =
            battle::lifecycle::PreparedLifecycle::prepare(cache, &self.files.script_sources()?)?
                .with_command_setup(command::Setup {
                    actors: ids[..self.party_positions.len()].to_vec(),
                    enabled,
                });
        let characters: Vec<_> = party.formation.iter().take(4).copied().collect();
        let sources: Vec<_> = actor_resources.iter().map(|actor| actor.source).collect();
        core = core.with_death_feedback(battle::death::feedback(
            &self.files,
            &sources,
            party,
            common_id,
            options.overlimit_boost,
            &mut sound,
        )?)?;
        core = core.with_contact_audio(battle::contact_audio::prepare(
            &self.files,
            &sources,
            &mut sound,
        )?)?;
        let mut ordinary_voices = BTreeMap::new();
        for line in 28..=37 {
            for (&character, voice) in characters.iter().zip(battle::voice::relative(
                &self.files,
                &sources,
                line,
                &mut sound,
            )?) {
                if let Some(voice) = voice {
                    ordinary_voices.insert((character, line), voice);
                }
            }
        }
        let mut group_voices = BTreeMap::new();
        for group in &victory.groups {
            let actor = sources.iter().position(
                |source| matches!(source, model::ModelSource::Party(id) if *id == group.character),
            );
            if let Some(actor) = actor {
                let voice = battle::voice::absolute(
                    &self.files,
                    &sources,
                    group.voice_command,
                    &mut sound,
                )?;
                if let Some(voice) = voice[actor] {
                    group_voices.insert(group.id, voice);
                }
            }
        }
        let results = battle::results::Setup {
            enemies: enemy_rewards,
            enemy_levels,
            enemy_grades,
            actors: ids
                .iter()
                .copied()
                .zip(characters.iter().copied())
                .collect(),
            formation: self.setup.encounter,
            formation_flags: self.enemies.flags,
            story: options
                .story
                .try_into()
                .context("negative battle story progress")?,
            intrinsic_conditions: plans
                .iter()
                .take(characters.len())
                .map(|plan| {
                    u64::from(
                        plan.profile.condition_flags[0] | plan.profile.intrinsic_conditions[0],
                    ) << 32
                        | u64::from(
                            plan.profile.condition_flags[1] | plan.profile.intrinsic_conditions[1],
                        )
                })
                .collect(),
            groups: victory.groups,
            performances,
            postures,
            notice_action,
            ordinary_voices,
            group_voices,
        };
        Ok(Prepared {
            core: Arc::new(core),
            entry_transition,
            lifecycle,
            models,
            effects,
            trails: render_trails,
            characters,
            actors: actor_actions,
            music: entry::music(&self.setup, u32::from(options.map), options.world_music),
            results,
        })
    }
}

fn motions(files: &Files, scene: &resonance_content::ScenePart) -> Result<BTreeMap<u16, Motion>> {
    scene
        .clips
        .iter()
        .map(|clip| {
            Ok((
                clip.resource_slot,
                Motion::decode(&files.read(&clip.motion)?)?,
            ))
        })
        .collect()
}

fn groups(rig: &battle_model::Rig) -> Vec<Vec<u16>> {
    let count = rig
        .attack_groups
        .keys()
        .next_back()
        .map_or(0, |&group| usize::from(group) + 1);
    (0..count)
        .map(|group| {
            rig.attack_groups
                .get(&(group as u8))
                .cloned()
                .unwrap_or_default()
        })
        .collect()
}

fn sampled_parts(part: &battle_model::ModelPart) -> Vec<PreviewPart> {
    part.layers
        .iter()
        .cloned()
        .map(|mut layer| {
            // The simulation's world pose already includes this attachment.
            layer.attached_to = None;
            layer.animation = None;
            layer
        })
        .collect()
}
