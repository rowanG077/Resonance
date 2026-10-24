use super::*;
use resonance_content::{
    battle_effect::{self, document as effects},
    battle_stage::document as arenas,
};

mod enemy;
mod equipment;
mod event;
mod party;
pub(crate) use party::party_path;
mod scene;
mod shared;
#[cfg(test)]
mod shared_tests;

impl Publisher<'_> {
    pub fn battle_effects(
        &mut self,
        banks: [battle_effect::SourceBank; 2],
        root: &str,
    ) -> Result<Vec<String>> {
        library::validate_path(root)?;
        ["common", "techniques"]
            .into_iter()
            .zip(banks)
            .map(|(name, bank)| self.battle_effect_bank(&format!("{root}/effects/{name}"), bank))
            .collect()
    }

    pub fn battle_tints(&mut self, tints: battle_effect::Tints, root: &str) -> Result<String> {
        library::validate_path(root)?;
        let path = format!("{root}/effects/tints.json");
        self.json(&path, &tints)?;
        Ok(path)
    }

    /// The cooker supplies the original catalogue IDs, including unused arenas.
    /// Discovery order and equal package bytes never assign or merge identities.
    pub fn battle_arenas(
        &mut self,
        stages: impl IntoIterator<Item = (u16, crate::battle_stage::Prepared)>,
    ) -> Result<Vec<String>> {
        self.battle_arenas_under(stages, "battle")
    }

    pub(crate) fn battle_arenas_under(
        &mut self,
        stages: impl IntoIterator<Item = (u16, crate::battle_stage::Prepared)>,
        root: &str,
    ) -> Result<Vec<String>> {
        library::validate_path(root)?;
        let mut mappings = BTreeMap::new();
        let mut paths = Vec::new();
        for (id, prepared) in stages {
            ensure!(
                !mappings.contains_key(&u32::from(id)),
                "duplicate arena ID {id}"
            );
            let name = format!("arena-{id:03}");
            let owner = format!("{root}/arenas/{name}");
            let mut stage = prepared.stage;
            stage.validate()?;
            let mut layers = BTreeMap::new();
            for (&slot, layer) in &mut stage.layers {
                let root = format!("{owner}/layers/layer-{slot:02}");
                self.part(&root, 0, &mut layer.scene)?;
                let path = format!("{root}/model.json");
                self.json(
                    &path,
                    &arenas::Layer {
                        version: arenas::VERSION,
                        model: layer.clone(),
                    },
                )?;
                layers.insert(slot, path);
            }
            // Native texture/palette indices are independently editable owners,
            // including entries whose original decoded pixels happen to match.
            for (index, texture) in stage.textures.iter_mut().enumerate() {
                for (palette, image) in texture.images.iter_mut().enumerate() {
                    // The stage and its effect bank both address archive section
                    // 10 as native bank 9; these are the same declared images.
                    image.path = self.image(&image.path, &format!("{owner}/effects/textures/bank-09/texture-{index:03}-palette-{palette:02}"))?;
                }
            }
            stage.effects = prepared
                .effects
                .map(|bank| self.battle_effect_bank(&format!("{owner}/effects"), bank))
                .transpose()?;
            let path = format!("{owner}/arena.json");
            self.json(&path, &arenas::Definition::from_stage(stage, layers))?;
            paths.push(path);
            mappings.insert(u32::from(id), name);
        }
        let path = format!("{root}/arenas/mappings.json");
        self.json(&path, &Mappings::new(mappings)?)?;
        Ok(paths)
    }

    pub(super) fn battle_effect_bank(
        &mut self,
        owner: &str,
        bank: battle_effect::SourceBank,
    ) -> Result<String> {
        self.battle_effect_bank_with_models(owner, bank, BTreeMap::new())
    }

    pub(super) fn battle_effect_bank_with_models(
        &mut self,
        owner: &str,
        bank: battle_effect::SourceBank,
        mut shared: BTreeMap<u8, String>,
    ) -> Result<String> {
        let programs = format!("{owner}/programs.json");
        let actors = format!("{owner}/particles.json");
        self.json(&programs, &bank.programs)?;
        self.json(&actors, &bank.actors)?;
        let art = bank.art.map(|mut art| -> Result<_> {
            for (bank, textures) in &mut art.textures {
                for (index, texture) in textures.iter_mut().enumerate() {
                    for (palette, image) in texture.images.iter_mut().enumerate() {
                        image.path = self.image(&image.path, &format!("{owner}/textures/bank-{bank:02}/texture-{index:03}-palette-{palette:02}"))?;
                    }
                }
            }
            let mut models = BTreeMap::new();
            for (slot, model) in art.models {
                let root = format!("{owner}/models/model-{slot:03}");
                let path = if let Some(path) = shared.remove(&slot) {
                    path
                } else {
                    self.battle_model_part(&root, model)?
                };
                models.insert(slot, path);
            }
            let path = format!("{owner}/art.json");
            self.json(&path, &effects::Artwork { version: effects::VERSION, textures: art.textures, models })?;
            Ok(path)
        }).transpose()?;
        ensure!(
            shared.is_empty(),
            "shared model has no effect slot in {owner}"
        );
        let path = format!("{owner}/effects.json");
        self.json(
            &path,
            &effects::Definition {
                version: effects::VERSION,
                programs,
                actors,
                art,
            },
        )?;
        Ok(path)
    }
}

#[cfg(test)]
pub(super) mod tests;

#[cfg(test)]
mod party_tests;

#[cfg(test)]
mod equipment_tests;

#[cfg(test)]
mod enemy_tests;

#[cfg(test)]
mod choreography_tests;
