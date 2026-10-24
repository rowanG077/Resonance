use super::*;
use resonance_content::{
    font::BitmapFont,
    menu::MenuArt,
    skit::{SkitCatalog, document as skits},
};
#[cfg(test)]
mod tests;

impl Publisher<'_> {
    pub(crate) fn session(
        &mut self,
        data: &resonance_content::session::SessionData,
        text: &resonance_content::session::GameText,
        root: &str,
    ) -> Result<Vec<String>> {
        library::validate_path(root)?;
        let session = format!("{root}/session-data.json");
        let labels = format!("{root}/text.json");
        self.json(&session, data)?;
        self.json(&labels, text)?;
        Ok(vec![session, labels])
    }

    pub(crate) fn menu_tables(&mut self, tables: &crate::menu::Tables) -> Result<Vec<String>> {
        let mut menu = serde_json::to_value(&tables.data)?;
        self.preview_catalogue(&mut menu["presentation"]["monsters"], "monsters")?;
        self.json("game/menu-data.json", &menu)?;
        let mut figurines = serde_json::to_value(&tables.figurines)?;
        self.preview_catalogue(&mut figurines, "figurines")?;
        self.json(resonance_content::menu_data::FIGURINES_PATH, &figurines)?;
        let mut paths = vec![
            "game/menu-data.json".into(),
            resonance_content::menu_data::FIGURINES_PATH.into(),
        ];
        for (path, value) in [
            (
                resonance_content::menu_data::MANUAL_PATH,
                serde_json::to_value(&tables.manual)?,
            ),
            (
                resonance_content::menu_data::SYNOPSIS_PATH,
                serde_json::to_value(&tables.synopsis)?,
            ),
            (
                resonance_content::menu_data::CUSTOMIZE_PATH,
                serde_json::to_value(&tables.customize)?,
            ),
            (
                resonance_content::menu_data::RENAME_PATH,
                serde_json::to_value(&tables.rename)?,
            ),
        ] {
            self.json(path, &value)?;
            paths.push(path.into());
        }
        Ok(paths)
    }

    fn preview_catalogue(&mut self, catalogue: &mut serde_json::Value, domain: &str) -> Result<()> {
        use resonance_content::model_preview::ModelPreview;
        let records = catalogue
            .get_mut("records")
            .and_then(serde_json::Value::as_array_mut)
            .context("preview catalogue needs records")?;
        for record in records {
            let id = u32::try_from(
                record["id"]
                    .as_u64()
                    .context("preview catalogue needs an ID")?,
            )?;
            let model: ModelPreview = serde_json::from_value(record["preview"].take())?;
            record["preview"] = self.catalogue_preview(domain, id, model)?.into();
        }
        // This is one deliberate shared lighting resource, also owned by fields.
        let ramp = self.image(
            crate::field_lighting::SCRATCH_RAMP,
            "effects/field/textures/toon-ramp",
        )?;
        ensure!(
            ramp == resonance_content::model_preview::document::TOON_RAMP,
            "shared lighting ramp must have one image level"
        );
        Ok(())
    }

    pub(super) fn catalogue_preview(
        &mut self,
        domain: &str,
        id: u32,
        mut model: resonance_content::model_preview::ModelPreview,
    ) -> Result<String> {
        use resonance_content::model_preview::document;
        let name = crate::naming::catalogue_model(domain, id)?;
        let owner = format!("meshes/{domain}/{name}");
        model.validate()?;
        for (index, part) in model.parts.iter_mut().enumerate() {
            self.part(&owner, index, &mut part.scene)?;
        }
        let path = format!("{owner}/model.json");
        self.json(
            &path,
            &document::Definition {
                version: document::VERSION,
                model,
            },
        )?;
        Ok(path)
    }

    pub(crate) fn dialogue(
        &mut self,
        prepared: crate::font::PreparedDialogue,
    ) -> Result<BitmapFont> {
        let crate::font::PreparedDialogue {
            mut font,
            mut art,
            subtitles,
        } = prepared;
        art.validate()?;
        font.validate()?;
        font.texture = self.image(&font.texture, "fonts/dialogue/atlas")?;
        self.json(&art.font, &font)?;
        for (index, texture) in art.textures.iter_mut().enumerate() {
            texture.path = self.image(
                &texture.path,
                &format!("ui/dialogue/textures/texture-{index:02}"),
            )?;
        }
        art.cursor.path = self.image(&art.cursor.path, "ui/dialogue/textures/cursor")?;
        self.json("ui/dialogue.json", &art)?;
        subtitles.validate()?;
        self.json("ui/story-subtitles.json", &subtitles)?;
        Ok(font)
    }

    pub(crate) fn menu_art(&mut self, mut menu: MenuArt) -> Result<String> {
        menu.validate_structure()?;
        for (id, texture) in &mut menu.textures {
            texture.path =
                self.image(&texture.path, &format!("ui/menu/textures/texture-{id:04}"))?;
        }
        let path = "ui/menu.json";
        self.json(path, &menu)?;
        Ok(path.into())
    }

    pub(crate) fn skits(&mut self, mut catalogue: SkitCatalog) -> Result<Vec<String>> {
        catalogue.validate()?;
        let mut mappings = BTreeMap::new();
        for (id, mut resource) in std::mem::take(&mut catalogue.resources) {
            let name = format!("skit-{id:04}");
            let root = format!("skits/{name}");
            self.copy(&resource.script, &format!("{root}/scenario.ssb"))?;
            self.copy(&resource.messages, &format!("{root}/messages.json"))?;
            resource.script = format!("{root}/scenario.ssb");
            resource.messages = format!("{root}/messages.json");
            self.json(&format!("{root}/skit.json"), &resource)?;
            mappings.insert(u32::from(id), name);
        }
        self.json(skits::MAPPINGS, &Mappings::new(mappings)?)?;
        for (id, portrait) in &mut catalogue.portraits {
            for (index, image) in portrait.images.iter_mut().enumerate() {
                image.texture = self.image(
                    &image.texture,
                    &format!("skits/portraits/portrait-{id}/image-{index:02}"),
                )?;
            }
        }
        let mut speech = BTreeMap::new();
        let mut definition = catalogue.map_media(|id, media| {
            Ok(match media.voice {
                Some(voice) => {
                    let name = format!("line-{id:08x}.wav");
                    self.copy(&voice.asset.path, &format!("audio/speech/skits/{name}"))?;
                    speech.insert(id, name);
                    skits::Media::Speech {
                        sample_rate: (voice.sample_rate != voice.source_sample_rate)
                            .then_some(voice.sample_rate),
                    }
                }
                None => skits::Media::Silence {
                    frames: media.frames,
                    sample_rate: media.sample_rate,
                },
            })
        })?;
        definition.version = skits::VERSION;
        self.json(skits::SPEECH_MAPPINGS, &Mappings::new(speech)?)?;
        self.json(skits::PATH, &definition)?;
        Ok(vec![
            skits::PATH.into(),
            skits::MAPPINGS.into(),
            skits::SPEECH_MAPPINGS.into(),
        ])
    }
}
