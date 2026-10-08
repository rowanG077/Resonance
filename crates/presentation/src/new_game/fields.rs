//! Prepared field packages are immutable and shared by transitions and reloads.
use super::super::loading::Cache;
use super::*;
use resonance_content::prepared::Files;

pub(crate) fn available_fields(root: &Path) -> Result<std::collections::BTreeSet<u32>> {
    let mut fields = std::collections::BTreeSet::new();
    for entry in std::fs::read_dir(root.join("fields"))? {
        let name = entry?.file_name();
        if let Some(map) = name
            .to_str()
            .and_then(|name| name.strip_prefix("map-"))
            .and_then(|name| name.strip_suffix(".preload.json"))
            .and_then(|id| id.parse().ok())
        {
            fields.insert(map);
        }
    }
    if root
        .join(resonance_content::overworld::PACKAGE_PATH)
        .is_file()
    {
        fields.insert(3000);
    }
    Ok(fields)
}

#[derive(Clone)]
pub(crate) struct FieldPackage {
    pub assets: FieldAssets,
    pub script: Arc<[u8]>,
    messages: Arc<[u8]>,
    pub audio: Arc<super::super::field_audio::Assets>,
    pub files: Arc<Files>,
    authored: Option<resonance_game::authored::FieldEvent>,
    services: Arc<resonance_game::authored::FieldServices>,
    attachments: resonance_game::field::attachments::Attachments,
    movies: BTreeSet<u32>,
}
impl FieldPackage {
    pub fn load(root: &Path, files: Arc<Files>, map: u32, cache: &mut Cache) -> Result<Self> {
        let manifest = files
            .manifests
            .get(&map)
            .context("field inventory is missing")?;
        let assets: FieldAssets = files.json(&manifest.inputs.field)?;
        assets.validate()?;
        ensure!(
            assets.map_id == map,
            "field inventory has the wrong map binding"
        );
        for (path, hash) in &assets.files {
            ensure!(
                manifest.files.get(path).is_some_and(|f| f.sha256 == *hash),
                "field dependency differs from its preparation inventory: {path}"
            );
        }
        ensure!(
            manifest.inputs.audio.len() == 1,
            "field needs one combined audio bank"
        );
        let audio = cache
            .audio
            .load(root, manifest.inputs.audio.first().unwrap(), &files)?;
        let (authored, services) = Self::prepare_scripts(map, &files, cache)?;
        let attachments =
            resonance_game::field::attachments::prepare(&assets, |path| files.read(path))?;
        let movies = manifest
            .inputs
            .movies
            .iter()
            .map(|path| {
                let id = path
                    .strip_prefix("movies/")
                    .and_then(|name| name.strip_suffix(".json"))
                    .and_then(|id| id.parse().ok())
                    .with_context(|| format!("invalid field movie binding {path}"))?;
                files.json::<MovieAsset>(path)?.validate()?;
                Ok(id)
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            script: files.read(&assets.script.path)?,
            messages: files.read(&assets.messages)?,
            assets,
            audio,
            files,
            authored,
            services,
            attachments,
            movies,
        })
    }

    pub fn prepare(
        root: &Path,
        map: u32,
        cache: &mut Cache,
        cancelled: impl Fn() -> bool,
    ) -> Result<Self> {
        let files = Arc::new(
            Files::load(root, &[&manifest_path(map)], &mut cache.bytes, cancelled)
                .with_context(|| format!("prepare field {map}"))?,
        );
        Self::load(root, files, map, cache)
    }

    /// Revisit shared cooked bytes while refreshing only editable source inputs.
    pub fn refresh_scripts(&self, cache: &mut Cache) -> Result<Self> {
        let mut package = self.clone();
        (package.authored, package.services) =
            Self::prepare_scripts(self.assets.map_id, &self.files, cache)?;
        Ok(package)
    }

    fn prepare_scripts(
        map: u32,
        files: &Files,
        cache: &mut Cache,
    ) -> Result<(
        Option<resonance_game::authored::FieldEvent>,
        Arc<resonance_game::authored::FieldServices>,
    )> {
        let sources = files.script_sources()?;
        let services = Arc::new(resonance_game::authored::FieldServices::prepare(
            &mut cache.service_scripts,
            &sources,
            &mut ScriptResources { files, font: None },
        )?);
        let authored = cache
            .scripts
            .as_mut()
            .map(|scripts| {
                scripts.prepare(map, &sources, &mut ScriptResources { files, font: None })
            })
            .transpose()?
            .flatten();
        Ok((authored, services))
    }

    pub fn queue_entry(&self, field: &mut FieldSession, kind: resonance_game::field::EntryKind) {
        field.queue_authored_entry(
            self.authored
                .as_ref()
                .and_then(|event| event.for_entry(kind)),
        );
    }

    pub fn restore(
        &self,
        checkpoint: &FieldCheckpoint,
        data: Arc<resonance_content::session::SessionData>,
        skits: Arc<resonance_content::skit::SkitCatalog>,
        available_fields: BTreeSet<u32>,
    ) -> Result<FieldSession> {
        let mut entry = checkpoint
            .clone()
            .entry(&self.assets, data, available_fields)?;
        entry.skits = Some(skits);
        let mut field = self.enter(entry)?;
        initialize_checkpoint(&mut field, checkpoint)?;
        self.queue_entry(&mut field, resonance_game::field::EntryKind::Restore);
        Ok(field)
    }

    pub fn transition(&self, previous: &FieldSession) -> Result<FieldSession> {
        let request = previous
            .events
            .world
            .field_transition
            .as_ref()
            .context("field transition is missing")?;
        ensure!(
            self.assets.map_id == request.map,
            "prepared field differs from the requested destination"
        );
        let resources = previous.events.resources();
        let mut field = self.enter(FieldEntry {
            allow_incomplete_scripts: previous.allow_incomplete_scripts,
            play_time: previous.play_time,
            persistent: previous.events.persistent_state()?,
            data: resources.session_data.clone(),
            skits: resources.skits.clone(),
            available_fields: resources.fields.clone(),
            position: request.position,
            heading: request.heading,
            camera: request.camera.clone(),
            ..Default::default()
        })?;
        field.continue_ambient(previous);
        self.queue_entry(&mut field, resonance_game::field::EntryKind::Arrival);
        Ok(field)
    }

    pub fn enter(&self, mut entry: FieldEntry) -> Result<FieldSession> {
        let menu: resonance_content::menu_data::MenuData =
            self.files.json("game/menu-data.json")?;
        menu.validate()?;
        let effects: resonance_content::effect::FieldEffects =
            self.files.json(&self.assets.effects)?;
        effects.validate()?;
        entry.effect_palette = resonance_events::effect::Palette(effects.palette);
        entry.menu_data = Some(Arc::new(menu));
        entry.text = Arc::new(self.files.json("game/text.json")?);
        entry.services = Some(self.services.clone());
        entry.attachments = self.attachments.clone();
        entry.available_movies = self.movies.clone();
        let mut field = FieldSession::enter(
            &self.script,
            serde_json::from_slice(&self.messages)?,
            &self.assets,
            entry,
        )?;
        field.events.world.voice_durations = self.audio.voice_durations();
        field.prepare_skits(&self.files)?;
        Ok(field)
    }
}

struct ScriptResources<'a> {
    files: &'a Files,
    font: Option<resonance_content::font::BitmapFont>,
}
impl resonance_game::authored::Resources for ScriptResources<'_> {
    fn asset(&mut self, reference: &resonance_game::authored::AssetReference) -> Result<()> {
        resonance_content::validate_asset_path(&reference.path)?;
        self.files.read(&reference.path)?;
        Ok(())
    }
    fn message(&mut self, text: &str) -> Result<()> {
        if self.font.is_none() {
            let dialogue: resonance_content::font::DialogueArt =
                self.files.json("ui/dialogue.json")?;
            dialogue.validate()?;
            let font: resonance_content::font::BitmapFont = self.files.json(&dialogue.font)?;
            font.validate()?;
            self.files.read(&font.texture)?;
            self.font = Some(font);
        }
        let font = self.font.as_ref().unwrap();
        for character in text.chars().filter(|character| *character != '\n') {
            ensure!(
                font.glyphs.contains_key(&character),
                "authored message requires an uncooked glyph {character:?}"
            );
        }
        Ok(())
    }
    fn substitution(&mut self, ty: symphonia_script::authored::Type) -> Result<()> {
        use symphonia_script::authored::{TextReferenceKind, Type};
        if ty == Type::I32 {
            return self.message("0123456789-");
        }
        let text: resonance_content::session::GameText = self.files.json("game/text.json")?;
        match ty {
            Type::TextReference {
                kind: TextReferenceKind::Character,
                ..
            } => {
                for name in resonance_events::ResourceLibrary::character_names()
                    .values()
                    .chain(text.characters.values())
                {
                    self.message(name)?;
                }
                // Saved names accept printable ASCII. Validate the whole rename
                // alphabet, since an active menu may change a name later.
                self.message(&(' '..='~').collect::<String>())?;
            }
            Type::TextReference {
                kind: TextReferenceKind::Item,
                ..
            } => {
                for name in text.items.values() {
                    self.message(name)?;
                }
            }
            _ => anyhow::bail!("unsupported message substitution type {ty:?}"),
        }
        Ok(())
    }
}
