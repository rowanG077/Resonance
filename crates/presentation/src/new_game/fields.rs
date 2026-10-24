//! Prepared field packages are immutable and shared by transitions and reloads.
use super::super::loading::Cache;
use super::*;
use resonance_content::prepared::Files;
use std::collections::BTreeSet;
#[cfg(test)]
mod tests;

pub(crate) fn available_fields(root: &Path) -> Result<std::collections::BTreeSet<u32>> {
    let mut snapshot = resonance_content::library::Library::open(root, &[])?.snapshot();
    capture_fields(&mut snapshot)
}

fn capture_fields(snapshot: &mut resonance_content::library::Snapshot) -> Result<BTreeSet<u32>> {
    let mut fields: BTreeSet<_> = snapshot
        .mappings(resonance_content::library::FIELDS)?
        .iter()
        .map(|(id, _)| id)
        .collect();
    if resonance_content::overworld::document::present(snapshot)? {
        fields.insert(3000);
    }
    Ok(fields)
}

#[derive(Clone)]
pub(crate) struct FieldPackage {
    pub assets: FieldAssets,
    pub fields: BTreeSet<u32>,
    pub data: Arc<resonance_content::session::SessionData>,
    pub script: Arc<[u8]>,
    messages: Arc<[u8]>,
    text: Arc<resonance_content::session::GameText>,
    pub skits: Arc<resonance_content::skit::SkitCatalog>,
    effects: resonance_content::effect::FieldEffects,
    pub audio: Arc<super::super::field_audio::Assets>,
    pub files: Arc<Files>,
    authored: Option<resonance_game::authored::FieldEvent>,
    services: Arc<resonance_game::authored::FieldServices>,
    attachments: resonance_game::field::attachments::Attachments,
    movies: BTreeSet<u32>,
}

struct PreparedScripts {
    authored: Option<resonance_game::authored::FieldEvent>,
    services: Arc<resonance_game::authored::FieldServices>,
    project: Option<resonance_game::authored::FieldProject>,
}
impl FieldPackage {
    pub fn prepare(
        root: &Path,
        map: u32,
        cache: &mut Cache,
        cancelled: impl Fn() -> bool,
    ) -> Result<Self> {
        Self::prepare_with_diagnostics(
            root,
            map,
            cache,
            cancelled,
            resonance_content::diagnostics::Diagnostics::new(true),
        )
    }

    pub fn prepare_with_diagnostics(
        root: &Path,
        map: u32,
        cache: &mut Cache,
        cancelled: impl Fn() -> bool,
        diagnostics: resonance_content::diagnostics::Diagnostics,
    ) -> Result<Self> {
        let library = resonance_content::library::Library::open(root, &[])?;
        Self::capture(&library, map, cache, cancelled, diagnostics)
            .with_context(|| format!("prepare field {map}"))
    }

    /// Follow the effective named documents once. Compilation may discover
    /// additional declared resources; all are captured before installing Files.
    pub fn capture(
        library: &resonance_content::library::Library,
        map: u32,
        cache: &mut Cache,
        cancelled: impl Fn() -> bool,
        diagnostics: resonance_content::diagnostics::Diagnostics,
    ) -> Result<Self> {
        use resonance_content::{field::document, preparation, skit};
        let mut snapshot = library.snapshot();
        let (definition, assets) = document::capture_scene(&mut snapshot, map, &cancelled)?;
        let sources = preparation::gameplay(&mut snapshot, &diagnostics, &cancelled)?;
        preparation::ui(&mut snapshot, &diagnostics, &cancelled)?;
        let skits = Arc::new(skit::document::capture(&mut snapshot, &cancelled)?);
        resonance_game::screen_resources::capture(
            &mut snapshot,
            std::iter::once(assets.script.as_str())
                .chain(skits.resources.values().map(|skit| skit.script.as_str())),
            &diagnostics,
            &cancelled,
        )?;
        let mut audio = cache.audio.capture_named(
            &definition.audio,
            &mut snapshot,
            diagnostics.clone(),
            &cancelled,
        )?;
        cache
            .audio
            .capture_skits(&mut audio, &skits, &mut snapshot, &cancelled)?;
        for &id in &definition.movies {
            ensure!(!cancelled(), "field preparation cancelled");
            // Keep payloads streamable. Actual playback takes its own captured
            // handle and probes the current container before decoding begins.
            diagnostics.attempt(
                "field movie",
                resonance_content::movie::capture(&mut snapshot, id),
            )?;
        }
        let PreparedScripts {
            authored,
            services,
            project,
        } = Self::prepare_from_sources(map, &sources, cache, |path| {
            ensure!(!cancelled(), "field script preparation cancelled");
            snapshot.read(path, preparation::ASSET_LIMIT)
        })?;
        let mut identity =
            resonance_content::save_identity::Identity::capture(&mut snapshot, &cancelled)?;
        if let Some(project) = project {
            identity = project.bind_identity(&identity)?;
        }
        let fields = capture_fields(&mut snapshot)?;
        let files = Arc::new(
            Files::from_snapshot(snapshot, &mut cache.bytes, &cancelled, diagnostics)?
                .with_identity(identity)?,
        );
        let data = Arc::new(resonance_content::session::SessionData::load(&files)?);
        let effects: resonance_content::effect::FieldEffects = files.json(&assets.effects)?;
        effects.validate()?;
        let attachments =
            resonance_game::field::attachments::prepare(&assets, |path| files.read(path))?;
        ensure!(!cancelled(), "field preparation cancelled");
        Ok(Self {
            fields,
            text: Arc::new(files.json("game/text.json")?),
            script: files.read(&assets.script)?,
            messages: files.read(&assets.messages)?,
            assets,
            data,
            skits,
            effects,
            audio,
            files,
            authored,
            services,
            attachments,
            movies: definition.movies,
        })
    }

    fn prepare_from_sources(
        map: u32,
        sources: &std::collections::BTreeMap<String, String>,
        cache: &mut Cache,
        read: impl FnMut(&str) -> Result<Arc<[u8]>>,
    ) -> Result<PreparedScripts> {
        let mut resources = ScriptResources { read, font: None };
        let services = Arc::new(resonance_game::authored::FieldServices::prepare(
            &mut cache.service_scripts,
            sources,
            &mut resources,
        )?);
        let project = cache
            .scripts
            .as_ref()
            .map(|scripts| scripts.capture())
            .transpose()?;
        let authored = cache
            .scripts
            .as_mut()
            .zip(project.as_ref())
            .map(|(scripts, project)| {
                scripts.prepare_captured(map, project, sources, &mut resources)
            })
            .transpose()?
            .flatten();
        Ok(PreparedScripts {
            authored,
            services,
            project,
        })
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
        available_fields: BTreeSet<u32>,
    ) -> Result<(FieldSession, Option<FieldCheckpoint>)> {
        let entry = checkpoint
            .clone()
            .entry(&self.assets, self.data.clone(), available_fields)?;
        let kind = entry.kind;
        let mut field = self.enter(entry)?;
        let restored = if kind == resonance_game::field::EntryKind::Restore {
            initialize_checkpoint(&mut field, checkpoint, &self.data)?;
            Some(field.checkpoint()?)
        } else {
            None
        };
        // Record the loaded state before an entry script takes control. New
        // Game Plus starts a new story and has no restored exploration frame.
        self.queue_entry(&mut field, kind);
        Ok((field, restored))
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
        entry.effect_palette = resonance_events::effect::Palette(self.effects.palette.clone());
        entry.rising_light_destination = Some(self.effects.rising_light_destination);
        entry.text = self.text.clone();
        entry.skits = Some(self.skits.clone());
        entry.data = Some(self.data.clone());
        entry.menu_data = self.data.rules.clone();
        entry.services = Some(self.services.clone());
        entry.attachments = self.attachments.clone();
        entry.available_movies = self.movies.clone();
        entry.menu_files = self.files.clone();
        let mut field = FieldSession::enter(
            &self.script,
            serde_json::from_slice(&self.messages)?,
            &self.assets,
            entry,
        )?;
        field.set_diagnostics(self.files.diagnostics().clone());
        field.events.world.voice_durations = self.audio.voice_durations();
        field.prepare_skits(&self.files)?;
        Ok(field)
    }
}

struct ScriptResources<R> {
    read: R,
    font: Option<resonance_content::font::BitmapFont>,
}
impl<R: FnMut(&str) -> Result<Arc<[u8]>>> ScriptResources<R> {
    fn json<T: serde::de::DeserializeOwned>(&mut self, path: &str) -> Result<T> {
        serde_json::from_slice(&(self.read)(path)?).with_context(|| format!("decode {path}"))
    }
}
impl<R: FnMut(&str) -> Result<Arc<[u8]>>> resonance_game::authored::Resources
    for ScriptResources<R>
{
    fn asset(&mut self, reference: &resonance_game::authored::AssetReference) -> Result<()> {
        resonance_content::validate_asset_path(&reference.path)?;
        (self.read)(&reference.path)?;
        Ok(())
    }
    fn message(&mut self, text: &str) -> Result<()> {
        if self.font.is_none() {
            let dialogue: resonance_content::font::DialogueArt = self.json("ui/dialogue.json")?;
            dialogue.validate()?;
            let font: resonance_content::font::BitmapFont = self.json(&dialogue.font)?;
            font.validate()?;
            (self.read)(&font.texture)?;
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
        let text: resonance_content::session::GameText = self.json("game/text.json")?;
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
