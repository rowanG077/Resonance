use super::*;
use resonance_content::{
    battle_enemy::document as enemies,
    battle_model::{
        self,
        document::{self as models, enemy as artwork},
    },
    model_preview::ModelPreview,
};

impl Publisher<'_> {
    /// Publish every original enemy, including dormant encounter reserves. The
    /// menu preview and battle body share one declared monster model owner.
    pub fn battle_enemies(
        &mut self,
        enemies: impl IntoIterator<Item = (u8, crate::battle_model::PreparedEnemy)>,
    ) -> Result<Vec<String>> {
        let mut mappings = BTreeMap::new();
        let mut paths = Vec::new();
        for (id, prepared) in enemies {
            let crate::battle_model::PreparedEnemy {
                preview,
                artwork: source,
                rules,
                effects: bank,
            } = prepared;
            let name = crate::naming::catalogue_model("monsters", u32::from(id))?;
            let owner = format!("battle/enemies/{name}");
            let shared = format!("meshes/monsters/{name}");
            let model = self.catalogue_preview("monsters", u32::from(id), preview.clone())?;
            paths.push(model.clone());
            let rig = format!("{owner}/rig.json");
            self.json(&rig, &source.body)?;
            let mut effect_models = BTreeMap::new();
            let secondary_body = source
                .secondary_body
                .map(|secondary| -> Result<_> {
                    // Both records address this native extra-model slot. Share its
                    // geometry with the preview, but keep the effect motion bank.
                    if let Some(effect) = bank
                        .art
                        .as_ref()
                        .and_then(|art| art.models.get(&secondary.model_slot))
                    {
                        ensure!(
                            effect.layers.len() == secondary.model.layers.len(),
                            "secondary effect layer count differs for enemy {id}"
                        );
                        let mut effect = effect.clone();
                        let effect_owner =
                            format!("{owner}/effects/models/model-{:03}", secondary.model_slot);
                        for (layer, body) in effect.layers.iter_mut().zip(&secondary.model.layers) {
                            self.enemy_shared_layer(
                                &shared,
                                &preview,
                                body.scene.resource,
                                &mut layer.scene,
                            )?;
                            self.clips(
                                &format!("{effect_owner}/animations"),
                                0,
                                &mut layer.scene.clips,
                            )?;
                        }
                        let effect_path = format!("{effect_owner}/model.json");
                        self.json(
                            &effect_path,
                            &models::Model {
                                version: models::VERSION,
                                model: effect,
                            },
                        )?;
                        effect_models.insert(secondary.model_slot, effect_path);
                    }
                    Ok(artwork::Secondary {
                        model_slot: secondary.model_slot,
                        motion_offset: secondary.motion_offset,
                        model: self.enemy_shared_component(
                            &format!("{owner}/secondary-body"),
                            &shared,
                            &preview,
                            secondary.model,
                            true,
                        )?,
                    })
                })
                .transpose()?;
            let mut attachments = BTreeMap::new();
            for (slot, model) in source.attachments {
                attachments.insert(
                    slot,
                    self.enemy_shared_component(
                        &format!("{owner}/attachments/slot-{slot}"),
                        &shared,
                        &preview,
                        model,
                        false,
                    )?,
                );
            }
            let mut weapon_variants = BTreeMap::new();
            for (slot, variant) in source.weapon_variants {
                weapon_variants.insert(
                    slot,
                    artwork::Variant {
                        model: self.battle_model_part(
                            &format!("{owner}/weapon-variants/variant-{slot}"),
                            variant.model,
                        )?,
                        trail: variant.trail,
                        animation: variant.animation,
                    },
                );
            }
            let art = format!("{owner}/model.json");
            self.json(
                &art,
                &artwork::Definition {
                    version: models::VERSION,
                    rig,
                    model,
                    secondary_body,
                    body_variant: source.body_variant,
                    attached_models: source.attached_models,
                    attachments,
                    uv_animation: source.uv_animation,
                    weapon_variants,
                    trails: source.trails,
                },
            )?;
            let rules_path = format!("{owner}/rules.json");
            self.json(&rules_path, &enemies::Rules::from(rules))?;
            let effects = self.battle_effect_bank_with_models(
                &format!("{owner}/effects"),
                bank,
                effect_models,
            )?;
            self.json(
                &format!("{owner}/enemy.json"),
                &enemies::Entry {
                    version: enemies::VERSION,
                    rules: rules_path,
                    artwork: art,
                    effects: Some(effects),
                },
            )?;
            paths.push(format!("{owner}/enemy.json"));
            ensure!(
                mappings.insert(u32::from(id), name).is_none(),
                "duplicate enemy ID {id}"
            );
        }
        self.json(enemies::MAPPINGS, &Mappings::new(mappings)?)?;
        paths.push(enemies::MAPPINGS.into());
        Ok(paths)
    }

    fn enemy_shared_component(
        &mut self,
        owner: &str,
        shared: &str,
        preview: &ModelPreview,
        mut model: battle_model::ModelPart,
        shared_motions: bool,
    ) -> Result<String> {
        for layer in &mut model.layers {
            let index =
                self.enemy_shared_layer(shared, preview, layer.scene.resource, &mut layer.scene)?;
            self.clips(
                &format!("{}/animations", if shared_motions { shared } else { owner }),
                index,
                &mut layer.scene.clips,
            )?;
        }
        let path = format!("{owner}/model.json");
        self.json(
            &path,
            &models::Model {
                version: models::VERSION,
                model,
            },
        )?;
        Ok(path)
    }

    fn enemy_shared_layer(
        &mut self,
        owner: &str,
        preview: &ModelPreview,
        resource: u16,
        scene: &mut ScenePart,
    ) -> Result<usize> {
        // The importer explicitly copied this preview component, identified
        // by its resource slot. Hash equality never chooses an owner.
        let mut matching = preview
            .parts
            .iter()
            .enumerate()
            .filter(|(_, part)| part.scene.resource == resource);
        let (index, part) = matching
            .next()
            .context("enemy component has no preview owner")?;
        ensure!(matching.next().is_none(), "ambiguous enemy component owner");
        ensure!(
            part.scene.mesh == scene.mesh && part.scene.textures == scene.textures,
            "enemy component differs from its declared preview owner: {owner}"
        );
        self.body(owner, index, scene)?;
        Ok(index)
    }
}
