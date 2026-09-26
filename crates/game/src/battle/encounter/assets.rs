//! Owned, verified encounter inputs. Loading never changes the retained field.
use super::{Enemies, enemies, party_positions};
use crate::battle::{audio, model, stage};
use anyhow::{Context, Result};
use resonance_content::{
    battle_audio::Audio,
    battle_effect::{self, SourceBank},
    battle_stage::Stage,
    battle_ui::{self, Art},
    menu_data::MenuData,
    prepared::{Cache, Files},
};
use resonance_events::{battle::Setup, party::Party};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub struct Assets {
    pub inputs: Inputs,
    pub audio: Audio,
}

impl std::ops::Deref for Assets {
    type Target = Inputs;
    fn deref(&self) -> &Inputs {
        &self.inputs
    }
}

/// Verified CPU/art inputs can also enumerate required audio during cooking.
/// A live encounter additionally requires Assets and real prepared audio bindings.
pub struct Inputs {
    pub setup: Setup,
    pub files: Files,
    pub stage: Stage,
    pub ui: Art,
    pub game_over: Option<crate::game_over::Assets>,
    pub enemies: Enemies,
    pub model_sources: Vec<model::ModelSource>,
    pub party_positions: Vec<[f32; 3]>,
    pub common_effects: SourceBank,
    pub technique_effects: SourceBank,
    pub enemy_effects: BTreeMap<u8, SourceBank>,
}

impl Assets {
    pub fn load(
        root: &Path,
        retained: &Files,
        menus: &MenuData,
        party: &Party,
        setup: Setup,
        cache: &mut Cache,
        cancelled: impl Fn() -> bool,
    ) -> Result<Self> {
        let mut inputs = Inputs::load(root, retained, menus, party, setup, cache, &cancelled)?;
        let (files, audio) = audio::prepare(root, inputs.files, cache, &cancelled)?;
        inputs.files = files;
        Ok(Self { inputs, audio })
    }
}

impl Inputs {
    /// Descriptors and source scripts must already belong to the field snapshot.
    /// Their inventories extend this candidate with complete model/audio/art bytes;
    /// the caller still prepares scripts, playback and GPU resources before entry.
    pub fn load(
        root: &Path,
        retained: &Files,
        menus: &MenuData,
        party: &Party,
        setup: Setup,
        cache: &mut Cache,
        cancelled: impl Fn() -> bool,
    ) -> Result<Self> {
        menus.validate()?;
        let enemies = enemies(retained, &menus.monsters, setup.encounter)?;
        let party_positions = party_positions(retained, menus, party)?;
        let mut model_sources = Vec::new();
        let mut selected = BTreeSet::new();
        let mut add = |key, source| {
            if selected.insert(key) {
                model_sources.push(source);
            }
        };
        for &character in party.formation.iter().take(4) {
            add(
                (0, u16::from(character)),
                model::ModelSource::Party(character),
            );
            let member = &party.members[usize::from(character - 1)];
            for (item, shield) in [(member.equipment[0], false), (member.equipment[5], true)] {
                if item != 0 && (!shield || (356..=366).contains(&item)) {
                    add((1, item), model::ModelSource::Weapon(item));
                }
            }
        }
        for enemy in &enemies.resources {
            add(
                (2, u16::from(enemy.id)),
                model::ModelSource::Enemy(enemy.id),
            );
        }
        let files = model::load_files(root, retained.clone(), &model_sources, cache, &cancelled)?;
        let (files, stage) = stage::prepare(root, files, setup.arena, cache, &cancelled)?;
        let mut files = crate::battle::victory::load_files(root, files, cache, &cancelled)?;
        let game_over = if setup.defeat == resonance_events::battle::DefeatPolicy::GameOver {
            let (candidate, art) = crate::game_over::Assets::load(root, files, cache, &cancelled)?;
            files = candidate;
            Some(art)
        } else {
            None
        };
        let ui: Art = files.json(battle_ui::PATH)?;
        ui.validate()?;
        for path in ui.files() {
            files
                .diagnostics()
                .attempt("battle UI image", files.read(path))?;
        }
        let common_effects: SourceBank = files.json(battle_effect::COMMON_PATH)?;
        let technique_effects: SourceBank = files.json(battle_effect::TECHNIQUES_PATH)?;
        let mut enemy_effects = BTreeMap::new();
        for enemy in &enemies.resources {
            let path = resonance_content::battle_model::enemy_effects_path(enemy.id);
            if files.bytes.contains_key(&path) {
                enemy_effects.insert(enemy.id, files.json::<SourceBank>(&path)?);
            }
        }
        for bank in [&common_effects, &technique_effects]
            .into_iter()
            .chain(enemy_effects.values())
        {
            let art = bank
                .art
                .as_ref()
                .context("battle effect art is not published")?;
            files = files.with_dependencies(root, art.files.clone(), cache, &cancelled)?;
        }
        Ok(Self {
            setup,
            files,
            stage,
            ui,
            game_over,
            enemies,
            model_sources,
            party_positions,
            common_effects,
            technique_effects,
            enemy_effects,
        })
    }
}
