use super::*;
use resonance_content::battle_model::{self, document as models};

impl Publisher<'_> {
    pub fn battle_equipment(&mut self, weapons: battle_model::Weapons) -> Result<Vec<String>> {
        let mut mappings = BTreeMap::new();
        let mut paths = Vec::new();
        for (id, attachment) in weapons.records {
            let name = crate::naming::equipment(id)?;
            let owner = format!("meshes/equipment/{name}");
            let attachment = match attachment {
                battle_model::Attachment::Nonvisual => models::AttachmentDefinition::Nonvisual,
                battle_model::Attachment::Weapon(weapon) => {
                    models::AttachmentDefinition::Weapon(self.equipment_model(&owner, weapon)?)
                }
                battle_model::Attachment::Shield(weapon) => {
                    models::AttachmentDefinition::Shield(self.equipment_model(&owner, weapon)?)
                }
            };
            self.json(
                &format!("{owner}/equipment.json"),
                &models::Equipment {
                    version: models::VERSION,
                    attachment,
                },
            )?;
            paths.push(format!("{owner}/equipment.json"));
            mappings.insert(u32::from(id), name);
        }
        self.json(models::EQUIPMENT, &Mappings::new(mappings)?)?;
        paths.push(models::EQUIPMENT.into());
        Ok(paths)
    }

    pub(super) fn equipment_model(
        &mut self,
        owner: &str,
        weapon: battle_model::Weapon,
    ) -> Result<models::WeaponDefinition> {
        let mut parts = BTreeMap::new();
        for (slot, part) in weapon.parts {
            parts.insert(
                slot,
                self.battle_model_part(&format!("{owner}/parts/part-{slot}"), part)?,
            );
        }
        Ok(models::WeaponDefinition {
            parts,
            trails: weapon.trails,
            uv_animation: weapon.uv_animation,
        })
    }

    pub(super) fn battle_model_part(
        &mut self,
        owner: &str,
        mut model: battle_model::ModelPart,
    ) -> Result<String> {
        for (index, layer) in model.layers.iter_mut().enumerate() {
            self.body(owner, index, &mut layer.scene)?;
            let motion_owner = if model.layer_skeletons.contains_key(&layer.scene.resource) {
                index
            } else {
                0
            };
            self.clips(
                &format!("{owner}/animations"),
                motion_owner,
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
}
