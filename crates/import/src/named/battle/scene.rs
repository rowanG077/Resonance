use super::*;
use resonance_content::battle_scene::{self, document as scenes};

impl Publisher<'_> {
    pub fn battle_scene_catalogue(
        &mut self,
        prepared: crate::battle_scene::PreparedCatalogue,
        root: &str,
    ) -> Result<crate::battle_scene::Publication> {
        library::validate_path(root)?;
        let mut paths = self.battle_scenes_under(
            Some(scenes::Kind::Technique),
            prepared.techniques,
            &format!("{root}/scenes"),
        )?;
        paths.extend(self.battle_scenes_under(
            Some(scenes::Kind::Skill),
            prepared.skills,
            &format!("{root}/skills"),
        )?);
        paths.extend(self.battle_scenes_under(
            None,
            prepared.unreferenced_skills,
            &format!("{root}/skills/packages"),
        )?);
        Ok(crate::battle_scene::Publication {
            paths,
            failures: prepared.failures,
        })
    }

    /// Catalogue IDs and unreferenced physical slots have separate lookup scopes.
    pub fn battle_scenes(
        &mut self,
        kind: scenes::Kind,
        prepared: impl IntoIterator<Item = (u16, crate::battle_scene::Prepared)>,
    ) -> Result<Vec<String>> {
        self.battle_scenes_under(
            Some(kind),
            prepared,
            kind.mapping().strip_suffix("/mappings.json").unwrap(),
        )
    }

    fn battle_scenes_under(
        &mut self,
        kind: Option<scenes::Kind>,
        prepared: impl IntoIterator<Item = (u16, crate::battle_scene::Prepared)>,
        category: &str,
    ) -> Result<Vec<String>> {
        let mut mappings = BTreeMap::new();
        let mut paths = Vec::new();
        for (id, prepared) in prepared {
            ensure!(
                !mappings.contains_key(&u32::from(id)),
                "duplicate battle scene ID {id}"
            );
            let name = match kind {
                Some(kind) => crate::naming::battle_scene(kind, id)?,
                None => format!("package-{id:03}"),
            };
            let owner = format!("{category}/{name}");
            let mut scene = prepared.scene;
            scene.validate()?;
            let bank = prepared.effects;
            let art = bank
                .art
                .as_ref()
                .context("stored scene has no effect artwork")?;
            // Native scene model slots and bank 6 are direct views of this same
            // package. Keep one editable declaration instead of mirrored lists.
            ensure!(
                serde_json::to_value(&scene.models)? == serde_json::to_value(&art.models)?,
                "scene/effect model bindings differ for {id}"
            );
            let textures = art
                .textures
                .get(&6)
                .context("stored scene has no texture bank 6")?
                .iter()
                .map(|texture| battle_scene::Texture {
                    images: texture.images.clone(),
                    sampler: texture.sampler.clone(),
                })
                .collect::<Vec<_>>();
            ensure!(
                serde_json::to_value(&scene.textures)? == serde_json::to_value(&textures)?,
                "scene/effect texture bindings differ for {id}"
            );
            let effects = self.battle_effect_bank(&owner, bank)?;
            if let Some(cutin) = &mut scene.cutin {
                for (palette, image) in cutin.images.iter_mut().enumerate() {
                    image.path =
                        self.image(&image.path, &format!("{owner}/cutin/palette-{palette}"))?;
                }
            }
            let weapon = scene
                .weapon
                .take()
                .map(|weapon| self.equipment_model(&format!("{owner}/weapon"), weapon))
                .transpose()?;
            let cutin = scene.cutin.take();
            let rules = format!("{owner}/rules.json");
            self.json(&rules, &scenes::Rules::from(scene))?;
            let path = format!("{owner}/scene.json");
            self.json(
                &path,
                &scenes::Definition {
                    version: scenes::VERSION,
                    rules,
                    effects,
                    cutin,
                    weapon,
                },
            )?;
            paths.push(path);
            mappings.insert(u32::from(id), name);
        }
        let path = format!("{category}/mappings.json");
        self.json(&path, &Mappings::new(mappings)?)?;
        paths.push(path);
        Ok(paths)
    }
}
