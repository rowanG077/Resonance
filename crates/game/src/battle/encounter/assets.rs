//! Owned, verified encounter inputs. Loading never changes the retained field.
use super::{Enemies, PrepareOptions, Prepared, enemies, placement::party_positions};
use crate::battle::{audio, effect_program, model, stage};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    arte,
    battle_audio::{Audio, Selection},
    battle_effect::{self, SourceBank},
    battle_enemy, battle_model, battle_profile,
    battle_stage::Stage,
    battle_ui::Art,
    menu_data::MenuData,
    prepared::{Cache, Files},
    session::SessionData,
};
use resonance_events::{battle::Setup, party::Party};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Arc,
};

pub struct Assets {
    pub inputs: Inputs,
    pub audio: Option<Audio>,
}

impl std::ops::Deref for Assets {
    type Target = Inputs;
    fn deref(&self) -> &Inputs {
        &self.inputs
    }
}

#[derive(Clone, Copy)]
pub(super) enum EquipmentKind {
    Weapon,
    Shield,
}

/// Verified CPU/art inputs can also enumerate required audio during cooking.
/// A live encounter resolves available audio before binding simulation requests.
pub struct Inputs {
    pub setup: Setup,
    pub(super) events: Option<crate::battle::event::Plan>,
    event_audio: Selection,
    pub files: Files,
    pub stage: Stage,
    pub ui: Art,
    pub catalogue: Arc<arte::Catalogue>,
    pub unison: Arc<resonance_content::battle_unison::Catalogue>,
    pub(super) recoil: crate::battle::recoil::Parameters,
    pub(crate) victory: resonance_content::battle_victory::Performances,
    pub game_over: Option<crate::game_over::Assets>,
    pub enemies: Enemies,
    pub(super) party: Party,
    pub(super) party_profiles: battle_profile::Table,
    pub(super) party_actors: Vec<PartyInput>,
    pub(super) weapons: BTreeMap<u16, battle_model::Weapon>,
    pub(super) enemy_definitions: BTreeMap<u8, battle_enemy::Definition>,
    pub(super) enemy_models: BTreeMap<
        u8,
        (
            battle_model::Enemy,
            BTreeMap<u16, resonance_content::animation::Motion>,
        ),
    >,
    pub common_effects: Option<Arc<SourceBank>>,
    pub technique_effects: Option<Arc<SourceBank>>,
    pub stage_effects: Option<Arc<SourceBank>>,
    pub enemy_effects: BTreeMap<u8, Arc<SourceBank>>,
    pub spell_scenes: BTreeMap<u16, Arc<SourceBank>>,
    pub skill_scenes: BTreeMap<u16, Arc<SourceBank>>,
}

pub(super) struct PartyInput {
    pub character: u8,
    pub position: [f32; 3],
    pub(super) source: Option<battle_model::Party>,
    pub(super) equipment: BTreeMap<u16, EquipmentKind>,
    pub(super) motions: BTreeMap<u16, resonance_content::animation::Motion>,
}

impl Assets {
    #[expect(
        clippy::too_many_arguments,
        reason = "Preparation borrows independently owned content and runtime resources."
    )]
    pub fn load(
        root: &Path,
        retained: &Files,
        menus: &MenuData,
        session: &SessionData,
        party: &Party,
        setup: Setup,
        options: PrepareOptions,
        cache: &mut Cache,
        cancelled: impl Fn() -> bool,
    ) -> Result<(Self, Prepared)> {
        let mut inputs = Inputs::load(
            root, retained, menus, session, party, setup, cache, &cancelled,
        )?;
        let catalogue = audio::read(&inputs.files)?;
        let mut selected = inputs.event_audio.clone();
        let prepared = inputs.prepare(menus, options, |sound| {
            audio::collect(
                catalogue.as_ref(),
                &mut selected,
                inputs.files.diagnostics(),
                sound,
            )
        })?;
        selected.music.insert(prepared.music);
        selected
            .music
            .insert(crate::battle::lifecycle::DEFEAT_MUSIC);
        if prepared.results.style.play_music {
            selected
                .music
                .insert(crate::battle::lifecycle::VICTORY_MUSIC);
        }
        let (files, audio) =
            audio::prepare(root, inputs.files, catalogue, &selected, cache, &cancelled)?;
        inputs.files = files;
        Ok((Self { inputs, audio }, prepared))
    }
}

impl Inputs {
    /// Descriptors must already belong to the field snapshot.
    /// Their inventories extend this candidate with complete model/audio/art bytes;
    /// the caller still prepares actions, playback and GPU resources before entry.
    #[expect(
        clippy::too_many_arguments,
        reason = "Preparation borrows independently owned content and runtime resources."
    )]
    pub fn load(
        root: &Path,
        retained: &Files,
        menus: &MenuData,
        session: &SessionData,
        party: &Party,
        setup: Setup,
        cache: &mut Cache,
        cancelled: impl Fn() -> bool,
    ) -> Result<Self> {
        let event_scenes = if setup.route == [0; 5] {
            Vec::new()
        } else {
            ensure!(
                setup.formation()? == 4
                    && setup.arena == 26
                    && setup.route == [2, 3, 600, 0, 3]
                    && setup.music == Some(91)
                    && setup.defeat == resonance_events::battle::DefeatPolicy::GameOver,
                "unsupported battle route overrides"
            );
            ensure!(
                party
                    .formation
                    .iter()
                    .take(4)
                    .copied()
                    .collect::<BTreeSet<_>>()
                    == BTreeSet::from([1, 2, 3, 9])
                    && party.formation.get(3) == Some(&9),
                "Vidarr event requires its initialized party"
            );
            [2, 3]
                .into_iter()
                .map(|id| {
                    retained
                        .json::<resonance_content::battle_event::Scene>(
                            &resonance_content::battle_event::path(id),
                        )
                        .map(Arc::new)
                })
                .collect::<Result<Vec<_>>>()?
        };
        ensure!(
            !party.battle_rules.coliseum
                && party.battle_rules.attack_adjustment == 0
                && party.battle_rules.defense_adjustment == 0
                && party.battle_rules.intelligence_adjustment == 0,
            "battle rules require unsupported combat adjustments"
        );
        let enemies = enemies(
            retained,
            menus.monsters()?,
            menus.items.len(),
            setup.formation()?,
        )?;
        let party_profiles: battle_profile::Table = retained.json(battle_profile::PARTY_PATH)?;
        let party_positions = party_positions(menus, party)?;
        ensure!(
            session.items.len() == menus.items.len(),
            "item definitions differ from the menu catalogue"
        );
        // Decode artwork independently of item eligibility and combat attributes.
        let records = retained
            .diagnostics()
            .attempt(
                "battle equipment artwork",
                (|| {
                    let bank: serde_json::Value = retained.json(battle_model::WEAPONS_PATH)?;
                    bank.get("records")
                        .and_then(serde_json::Value::as_object)
                        .cloned()
                        .context("missing equipment attachment catalogue")
                })(),
            )?
            .unwrap_or_default();
        let equipped: BTreeSet<_> = party
            .formation
            .iter()
            .take(party_positions.len())
            .flat_map(|&character| {
                let member = &party.members[usize::from(character - 1)];
                [member.equipment[0], member.equipment[5]]
            })
            .filter(|&item| item != 0)
            .collect();
        let mut candidates: BTreeSet<_> = party
            .items
            .iter()
            .filter_map(|(&item, &count)| (item != 0 && count > 0).then_some(item))
            .collect();
        let selected_characters = party
            .formation
            .iter()
            .take(party_positions.len())
            .fold(0, |mask, &character| mask | (1 << (character - 1)));
        for member in &party.members {
            for item in [member.equipment[0], member.equipment[5]] {
                if item != 0 {
                    candidates.insert(item);
                }
            }
        }
        let mut available_equipment = BTreeMap::new();
        let mut weapons = BTreeMap::new();
        for item in candidates {
            let selected = (|| -> Result<_> {
                let definition = session
                    .items
                    .get(usize::from(item))
                    .with_context(|| format!("missing equipment item definition {item}"))?;
                if !equipped.contains(&item)
                    && definition.allowed_characters & selected_characters == 0
                {
                    return Ok(None);
                }
                let Some(record) = records.get(&item.to_string()) else {
                    ensure!(
                        !matches!(definition.equipment_kind, Some(0 | 3)),
                        "missing equipment attachment {item}"
                    );
                    return Ok(None);
                };
                let attachment: battle_model::Attachment = serde_json::from_value(record.clone())?;
                let (kind, mut weapon) = match attachment {
                    battle_model::Attachment::Weapon(weapon) => (EquipmentKind::Weapon, weapon),
                    battle_model::Attachment::Shield(weapon) => (EquipmentKind::Shield, weapon),
                    battle_model::Attachment::Nonvisual => return Ok(None),
                };
                // Genis follows body clips; other carried models only run their selected loop.
                if selected_characters & definition.allowed_characters & (1 << 2) == 0 {
                    let mut unused = BTreeSet::new();
                    let mut selected = BTreeSet::new();
                    for layer in weapon.parts.values().flat_map(|part| &part.layers) {
                        unused.extend(layer.scene.clips.iter().map(|clip| clip.motion.clone()));
                        if let Some((_, clip)) = layer.selected_clip()? {
                            selected.insert(clip.motion.clone());
                        }
                    }
                    weapon
                        .files
                        .retain(|path, _| !unused.contains(path) || selected.contains(path));
                }
                Ok(Some((kind, definition.allowed_characters, weapon)))
            })()
            .with_context(|| format!("equipment item {item}"));
            let selected = retained
                .diagnostics()
                .attempt("battle equipment artwork", selected)?
                .flatten();
            if let Some((kind, allowed, weapon)) = selected {
                available_equipment.insert(item, (kind, allowed));
                weapons.insert(item, weapon);
            }
        }
        let mut party_actors = Vec::new();
        for (&character, position) in party.formation.iter().zip(party_positions) {
            let member_index = usize::from(character - 1);
            let member = &party.members[member_index];
            let mut items = BTreeMap::new();
            for (&item, &(kind, allowed_characters)) in &available_equipment {
                // Any held or equipped item can move to another eligible active member.
                let equipped = member.equipment[0] == item || member.equipment[5] == item;
                if equipped || allowed_characters & (1 << member_index) != 0 {
                    items.insert(item, kind);
                }
            }
            party_actors.push(PartyInput {
                character,
                position,
                equipment: items,
                source: retained.diagnostics().attempt(
                    "party body descriptor",
                    retained.json(&battle_model::party_path(character)),
                )?,
                motions: BTreeMap::new(),
            });
        }
        let mut inventory = BTreeMap::new();
        for scene in &event_scenes {
            model::merge_dependencies(&mut inventory, scene.files.clone())?;
        }
        model::merge_dependencies(
            &mut inventory,
            party_actors
                .iter()
                .filter_map(|actor| actor.source.as_ref())
                .flat_map(|source| source.files.clone()),
        )?;
        let mut enemy_definitions = BTreeMap::<u8, battle_enemy::Definition>::new();
        let mut enemy_models = BTreeMap::new();
        for resource in &enemies.resources {
            let id = resource.monster.id;
            enemy_definitions.insert(id, retained.json(&battle_enemy::path(id))?);
            let artwork = (|| {
                let source: battle_model::Enemy = retained.json(&battle_model::enemy_path(id))?;
                let mut candidate = inventory.clone();
                model::merge_dependencies(&mut candidate, source.files.clone())?;
                Ok((source, candidate))
            })();
            if let Some((source, candidate)) = retained
                .diagnostics()
                .attempt("enemy body descriptor", artwork)?
            {
                inventory = candidate;
                enemy_models.insert(id, (source, BTreeMap::new()));
            }
        }
        ensure!(
            enemy_definitions
                .values()
                .all(|enemy| !enemy.actions.policy.event_hp_hold || !event_scenes.is_empty()),
            "enemy requires a prepared battle event route"
        );
        let mut files =
            retained
                .clone()
                .with_dependencies(root, inventory.clone(), cache, &cancelled)?;
        let weapon_ids: Vec<_> = weapons.keys().copied().collect();
        for item in weapon_ids {
            let Some(weapon) = weapons.get(&item) else {
                continue;
            };
            let mut candidate_inventory = inventory.clone();
            let merged = weapon
                .files
                .iter()
                .try_for_each(|(path, file)| file.validate(path))
                .and_then(|()| {
                    model::merge_dependencies(&mut candidate_inventory, weapon.files.clone())
                })
                .with_context(|| format!("equipment item {item}"));
            let available = files
                .diagnostics()
                .attempt("battle equipment artwork", merged)?
                .is_some();
            if !available {
                weapons.remove(&item);
                for actor in &mut party_actors {
                    actor.equipment.remove(&item);
                }
                continue;
            }
            // Files reports missing payloads; component preparation omits their artwork.
            // Cancellation still aborts the encounter candidate.
            files = files.with_dependencies(root, weapon.files.clone(), cache, &cancelled)?;
            inventory = candidate_inventory;
        }
        let (files, stage) = stage::prepare(root, files, setup.arena, cache, &cancelled)?;
        let (mut files, victory) = crate::battle::victory::load(
            root,
            files,
            if enemies.settings.celebrate {
                &party.formation[..party_actors.len()]
            } else {
                &[]
            },
            cache,
            &cancelled,
        )?;
        let game_over = if setup.defeat == resonance_events::battle::DefeatPolicy::GameOver {
            let (candidate, art) = crate::game_over::Assets::load(root, files, cache, &cancelled)?;
            files = candidate;
            Some(art)
        } else {
            None
        };
        let ui = Art::load(&files)?;
        let catalogue = Arc::new(files.json(arte::PATH)?);
        let unison: resonance_content::battle_unison::Catalogue =
            files.json(resonance_content::battle_unison::PATH)?;
        unison.validate()?;
        let unison = Arc::new(unison);
        let recoil = crate::battle::recoil::Parameters::load(&files)?;
        let common_effects = effect_program::load(&files, battle_effect::COMMON_PATH)?;
        let technique_effects = effect_program::load(&files, battle_effect::TECHNIQUES_PATH)?;
        let mut stage_effects = if stage.recurring_interval.is_some() {
            let path = stage
                .effects
                .as_deref()
                .context("recurring arena has no effect bank")?;
            effect_program::load(&files, path)?
        } else {
            None
        };
        let mut enemy_effects = BTreeMap::new();
        for resource in &enemies.resources {
            let enemy = &resource.monster;
            let path = resonance_content::battle_model::enemy_effects_path(enemy.id);
            if files.contains_key(&path)
                && let Some(bank) = effect_program::load(&files, &path)?
            {
                enemy_effects.insert(enemy.id, bank);
            }
        }
        let mut scene_ids = BTreeSet::new();
        let mut skill_ids = BTreeSet::new();
        let mut party_artes = BTreeSet::new();
        for actor in &party_actors {
            for catalogue in
                super::resources::party_techniques(&catalogue, actor.character)?.into_keys()
            {
                if crate::battle::skill_scene::contains(catalogue) {
                    skill_ids.insert(catalogue);
                }
                scene_ids.extend(
                    crate::battle::stored_spells::scene(catalogue)
                        .or_else(|| crate::battle::summons::scene(catalogue)),
                );
                if let Some(placement) = unison.placements.get(usize::from(catalogue)) {
                    party_artes.insert(placement.native_id);
                }
            }
        }
        for enemy in enemy_definitions.values() {
            for native in crate::battle::enemy::technique_ids(enemy) {
                if let Some(catalogue) = crate::battle::enemy::catalogue_id(&unison, native) {
                    scene_ids.extend(
                        crate::battle::stored_spells::scene(catalogue)
                            .or_else(|| crate::battle::summons::scene(catalogue)),
                    );
                    if crate::battle::skill_scene::contains(catalogue) {
                        skill_ids.insert(catalogue);
                    }
                }
            }
        }
        scene_ids.extend(unison.combinations.iter().filter_map(|combination| {
            combination
                .recipes
                .iter()
                .any(|recipe| {
                    recipe
                        .iter()
                        .all(|ingredient| party_artes.contains(ingredient))
                })
                .then_some(combination.native_id)
        }));
        let mut spell_scenes = BTreeMap::new();
        for technique in scene_ids {
            files = model::load_files(
                root,
                files,
                &[model::ModelSource::Scene(technique)],
                cache,
                &cancelled,
            )?;
            let scene: resonance_content::battle_scene::Scene =
                files.json(&resonance_content::battle_scene::path(technique))?;
            if let Some(bank) = effect_program::load(&files, &scene.effects)? {
                spell_scenes.insert(technique, bank);
            }
        }
        let (loaded, mut skill_scenes) =
            crate::battle::skill_scene::load(root, files, skill_ids, cache, &cancelled)?;
        files = loaded;
        for bank in stage_effects
            .iter_mut()
            .chain(enemy_effects.values_mut())
            .chain(spell_scenes.values_mut())
            .chain(skill_scenes.values_mut())
        {
            if let (Some(art), Some(common)) = (
                Arc::make_mut(bank).art.as_mut(),
                common_effects.as_ref().and_then(|bank| bank.art.as_ref()),
            ) {
                for (&slot, textures) in &common.textures {
                    art.textures.entry(slot).or_insert_with(|| textures.clone());
                }
                art.files.extend(common.files.clone());
            }
        }
        for bank in [
            common_effects.as_ref(),
            technique_effects.as_ref(),
            stage_effects.as_ref(),
        ]
        .into_iter()
        .flatten()
        .chain(enemy_effects.values())
        .chain(spell_scenes.values())
        .chain(skill_scenes.values())
        {
            if let Some(art) = &bank.art {
                files = files.with_dependencies(root, art.files.clone(), cache, &cancelled)?;
            }
        }
        for input in &mut party_actors {
            if let Some(source) = &input.source {
                input.motions = files
                    .diagnostics()
                    .attempt(
                        "party body motions",
                        (|| {
                            model::motions(
                                &files,
                                source.parts.first().context("missing party body scene")?,
                                &source.body.skeleton,
                            )
                        })(),
                    )?
                    .unwrap_or_default();
            }
        }
        for resource in &enemies.resources {
            let enemy = &resource.monster;
            let Some((source, motions)) = enemy_models.get_mut(&enemy.id) else {
                continue;
            };
            *motions = files
                .diagnostics()
                .attempt(
                    "enemy body motions",
                    (|| {
                        model::motions(
                            &files,
                            &enemy
                                .preview
                                .parts
                                .first()
                                .context("missing enemy body scene")?
                                .scene,
                            &source.body.skeleton,
                        )
                    })(),
                )?
                .unwrap_or_default();
        }
        let mut event_audio = Selection::default();
        for scene in &event_scenes {
            event_audio.sounds.extend(&scene.audio.sounds);
            event_audio.streams.extend(&scene.audio.streams);
            event_audio.voices.extend(&scene.audio.voices);
            event_audio.music.extend(&scene.audio.music);
        }
        let events = if event_scenes.is_empty() {
            None
        } else {
            Some(crate::battle::event::Plan::vidarr(
                super::event_assets::load(&files, event_scenes[0].clone())?,
                super::event_assets::load(&files, event_scenes[1].clone())?,
            ))
        };
        Ok(Self {
            setup,
            events,
            event_audio,
            files,
            stage,
            ui,
            catalogue,
            unison,
            recoil,
            victory,
            game_over,
            enemies,
            party: party.clone(),
            party_profiles,
            party_actors,
            weapons,
            enemy_definitions,
            enemy_models,
            common_effects,
            technique_effects,
            stage_effects,
            enemy_effects,
            spell_scenes,
            skill_scenes,
        })
    }
}
