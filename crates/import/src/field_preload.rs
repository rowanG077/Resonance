//! Conservative preparation manifests for cooked fields. Never executes a VM,
//! prunes a story branch, or opens a graphics/audio device.
use crate::{media::hash_file, write_atomic};
use anyhow::{Context, Result, ensure};
pub use resonance_content::field_preload::Inputs;
use resonance_content::{
    MovieAsset,
    effect::FieldEffects,
    field::FieldAssets,
    field_audio::FieldAudio,
    field_preload::{File, Manifest, Role, SHARED_PATH, Shared, VERSION},
    font::{BitmapFont, DialogueArt},
    validate_asset_path,
};
use serde::de::DeserializeOwned;
use std::{collections::BTreeMap, fs, io::ErrorKind, path::Path};

#[cfg(test)]
mod tests;

pub(crate) fn cook_field(
    root: &Path,
    inputs: Inputs,
    field: &FieldAssets,
    sha256: &str,
    paths: &std::collections::BTreeSet<String>,
    shared: &Shared,
) -> Result<Manifest> {
    inputs.validate()?;
    let mut inventory = Inventory::with_shared(root, &shared.files);
    inventory.paths(paths.iter().map(String::as_str))?;
    inventory.add(&inputs.field, Some(sha256), Role::Field)?;
    publish(root, build_field(inventory, inputs, field)?)
}

fn publish(root: &Path, manifest: Manifest) -> Result<Manifest> {
    let path = manifest.inputs.manifest_path()?;
    write_atomic(&root.join(&path), &serde_json::to_vec_pretty(&manifest)?)?;
    println!(
        "Prepared {path}: {} local files, {} missing inputs",
        manifest.files.len(),
        manifest.missing_inputs.len()
    );
    Ok(manifest)
}

fn build_field(
    mut inventory: Inventory<'_>,
    inputs: Inputs,
    field: &FieldAssets,
) -> Result<Manifest> {
    let root = inventory.root;
    field.validate()?;
    inventory.paths(field.references())?;
    let mut manifest = Manifest {
        version: VERSION,
        map_id: field.map_id,
        inputs,
        missing_inputs: Default::default(),
        files: Default::default(),
    };
    let effects: FieldEffects = inventory.json(&field.effects, None, Role::Data)?;
    effects.validate()?;
    for sprite in effects
        .sprites
        .values()
        .chain([&effects.refraction.sprite, &effects.air_refraction])
    {
        inventory.add(&sprite.texture, None, Role::Texture)?;
    }
    inventory.add(&effects.emote_texture, None, Role::Texture)?;
    inventory.add(&effects.status_texture, None, Role::Texture)?;
    for recipe in field.particles.values() {
        inventory.add(&recipe.texture, None, Role::Texture)?;
    }
    for path in field.overlays.values() {
        let art: resonance_content::effect::OverlayArt = inventory.json(path, None, Role::Data)?;
        art.validate()?;
        for image in art.textures.iter().flat_map(|texture| &texture.images) {
            inventory.add(&image.path, None, Role::Texture)?;
        }
    }
    for path in &manifest.inputs.audio {
        if !input_exists(root, path)? {
            manifest.missing_inputs.insert(path.clone());
            continue;
        }
        inventory.audio(path)?;
    }
    for path in &manifest.inputs.movies {
        if !input_exists(root, path)? {
            manifest.missing_inputs.insert(path.clone());
            continue;
        }
        let movie: MovieAsset = inventory.json(path, None, Role::MovieManifest)?;
        movie.validate()?;
        inventory.add(&movie.path, Some(&movie.sha256), Role::Movie)?;
    }

    let scripts: Vec<_> = inventory
        .files
        .iter()
        .filter(|(path, file)| file.roles.contains(&Role::Script) && path.ends_with(".ssb"))
        .map(|(path, _)| path.clone())
        .collect();
    for path in scripts {
        use symphonia_script::NativeCall::ConfigureSession;
        let bytes = fs::read(root.join(path))?;
        let calls = crate::field_resources::literal_arguments(&bytes, ConfigureSession, 2)?;
        if calls
            .iter()
            .any(|args| args[0].is_none_or(|setting| setting == 18))
        {
            let credits: resonance_content::credits::Manifest =
                inventory.json(resonance_content::credits::PATH, None, Role::Data)?;
            credits.validate()?;
            let font: BitmapFont = inventory.json("fonts/dialogue.json", None, Role::Data)?;
            for op in &credits.program.operations {
                if let resonance_content::credits::Operation::Text { text } = op {
                    for c in text.chars() {
                        ensure!(font.glyphs.contains_key(&c), "uncooked credits glyph {c:?}");
                    }
                }
            }
            for picture in &credits.pictures {
                inventory.add(&picture.path, None, Role::Texture)?;
            }
            inventory.add(
                &credits.music.asset.path,
                Some(&credits.music.asset.sha256),
                Role::Voice,
            )?;
            break;
        }
    }
    manifest.files = inventory.files;
    manifest.validate()?;
    Ok(manifest)
}

pub(crate) fn cook_shared(
    root: &Path,
    paths: &std::collections::BTreeSet<String>,
) -> Result<Shared> {
    let mut inventory = Inventory::new(root);
    inventory.paths(paths.iter().map(String::as_str))?;
    close_shared(&mut inventory)?;
    let shared = Shared {
        version: VERSION,
        files: inventory.files,
    };
    shared.validate()?;
    write_atomic(
        &root.join(SHARED_PATH),
        &serde_json::to_vec_pretty(&shared)?,
    )?;
    Ok(shared)
}

fn close_shared(inventory: &mut Inventory<'_>) -> Result<()> {
    let identity: resonance_content::save_identity::Identity =
        inventory.json(resonance_content::save_identity::PATH, None, Role::Data)?;
    identity.validate()?;
    // Close the shared UI/effect descriptors as well as the field's file list.
    // Keep every emote recipe and glyph, not just the current dialogue/route.
    let ui: DialogueArt = inventory.json("ui/dialogue.json", None, Role::Data)?;
    ui.validate()?;
    let font: BitmapFont = inventory.json(&ui.font, None, Role::Data)?;
    font.validate()?;
    inventory.add(&font.texture, None, Role::Texture)?;
    for texture in &ui.textures {
        inventory.add(&texture.path, None, Role::Texture)?;
    }
    let menu: resonance_content::menu::MenuArt =
        inventory.json("ui/menu.json", None, Role::Data)?;
    let data: resonance_content::menu_data::MenuData =
        inventory.json("game/menu-data.json", None, Role::Data)?;
    menu.validate(data.items.len())?;
    for texture in menu.textures.into_values() {
        inventory.add(&texture.path, None, Role::Texture)?;
    }
    data.validate()?;
    use resonance_content::menu_data::{
        CUSTOMIZE_PATH, CustomizeData, FIGURINES_PATH, MANUAL_PATH, RENAME_PATH, RenameData,
        SYNOPSIS_PATH, SynopsisData, TrainingManual,
    };
    let manual: TrainingManual = inventory.json(MANUAL_PATH, None, Role::Data)?;
    let figurines: resonance_content::figurine::FigurineBook =
        inventory.json(FIGURINES_PATH, None, Role::Data)?;
    let synopsis: SynopsisData = inventory.json(SYNOPSIS_PATH, None, Role::Data)?;
    let customize: CustomizeData = inventory.json(CUSTOMIZE_PATH, None, Role::Data)?;
    let rename: RenameData = inventory.json(RENAME_PATH, None, Role::Data)?;
    manual.validate()?;
    figurines.validate()?;
    synopsis.validate()?;
    customize.validate()?;
    rename.validate()?;
    for text in data
        .texts()
        .chain(manual.texts())
        .chain(figurines.texts())
        .chain(synopsis.texts())
        .chain(customize.texts())
        .chain(rename.texts())
    {
        ensure!(
            text.len() <= 4096 && text.chars().all(|c| !c.is_control() || c == '\n'),
            "invalid menu page text {text:?}"
        );
        for c in text.chars().filter(|c| *c != '\n') {
            ensure!(font.glyphs.contains_key(&c), "uncooked menu glyph {c:?}");
        }
    }
    close_skits(inventory, &font)?;
    close_battle_audio(inventory)
}

fn close_skits(inventory: &mut Inventory<'_>, font: &BitmapFont) -> Result<()> {
    if inventory.files.contains_key("game/skits.json") {
        let skits: resonance_content::skit::SkitCatalog =
            inventory.json("game/skits.json", None, Role::Data)?;
        skits.validate()?;
        for resource in skits.resources.values() {
            inventory.add(&resource.script, None, Role::Script)?;
            inventory.add(&resource.messages, None, Role::Data)?;
            let messages: Vec<symphonia_script::message::Message> =
                serde_json::from_slice(&fs::read(inventory.root.join(&resource.messages))?)?;
            crate::font::validate_messages(font, &messages)?;
        }
        for image in skits
            .portraits
            .values()
            .flat_map(|portrait| &portrait.images)
        {
            inventory.add(&image.texture, None, Role::Texture)?;
        }
        for voice in skits.media.values().filter_map(|m| m.voice.as_ref()) {
            inventory.add(&voice.asset.path, Some(&voice.asset.sha256), Role::Voice)?;
        }
    }
    Ok(())
}

fn close_battle_audio(inventory: &mut Inventory<'_>) -> Result<()> {
    // Battle packages are prepared only when an encounter is requested, but its
    // descriptor belongs to this verified snapshot. It is produced after the
    // shared field descriptors, before final inventory publication.
    if inventory
        .files
        .contains_key(resonance_content::battle_formation::PATH)
    {
        let path = resonance_content::battle_audio::PATH;
        if input_exists(inventory.root, path)? {
            let audio: resonance_content::battle_audio::Audio =
                inventory.json(path, None, Role::Data)?;
            audio.validate()?;
        }
    }

    Ok(())
}

fn input_exists(root: &Path, path: &str) -> Result<bool> {
    match fs::metadata(root.join(path)) {
        Ok(metadata) => {
            ensure!(metadata.is_file(), "preload input is not a file: {path}");
            Ok(true)
        }
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).with_context(|| format!("inspect preload input {path}")),
    }
}

pub(crate) struct Inventory<'a> {
    root: &'a Path,
    files: BTreeMap<String, File>,
    shared: Option<&'a BTreeMap<String, File>>,
}

impl<'a> Inventory<'a> {
    pub(crate) fn new(root: &'a Path) -> Self {
        Self {
            root,
            files: BTreeMap::new(),
            shared: None,
        }
    }

    fn with_shared(root: &'a Path, shared: &'a BTreeMap<String, File>) -> Self {
        Self {
            root,
            files: BTreeMap::new(),
            shared: Some(shared),
        }
    }

    fn paths<'p>(&mut self, paths: impl IntoIterator<Item = &'p str>) -> Result<()> {
        for path in paths {
            let role = match Path::new(path).extension().and_then(|s| s.to_str()) {
                Some("ssb" | "sym") => Role::Script,
                Some("glb") => Role::Mesh,
                Some("ktx2" | "png") => Role::Texture,
                _ => Role::Data,
            };
            self.add(path, None, role)?;
        }
        Ok(())
    }

    fn file(&self, path: &str) -> &File {
        self.files
            .get(path)
            .or_else(|| self.shared.and_then(|files| files.get(path)))
            .unwrap()
    }

    pub(crate) fn json<T: DeserializeOwned>(
        &mut self,
        path: &str,
        expected: Option<&str>,
        role: Role,
    ) -> Result<T> {
        self.add(path, expected, role)?;
        let bytes = fs::read(self.root.join(path)).with_context(|| format!("read {path}"))?;
        ensure!(
            crate::digest(&bytes) == self.file(path).sha256,
            "preload metadata changed during analysis: {path}"
        );
        serde_json::from_slice(&bytes).with_context(|| format!("decode {path}"))
    }

    pub(crate) fn add(&mut self, path: &str, expected: Option<&str>, role: Role) -> Result<()> {
        validate_asset_path(path)?;
        if let Some(file) = self.shared.and_then(|files| files.get(path)) {
            ensure!(
                expected.is_none_or(|expected| file.sha256 == expected),
                "preload asset hash differs: {path}; recook its source package"
            );
            return Ok(());
        }
        if !self.files.contains_key(path) {
            let absolute = self.root.join(path);
            let metadata =
                fs::metadata(&absolute).with_context(|| format!("missing preload asset {path}"))?;
            ensure!(metadata.is_file(), "preload asset is not a file: {path}");
            self.files.insert(
                path.into(),
                File {
                    sha256: hash_file(&absolute)?,
                    bytes: metadata.len(),
                    roles: Default::default(),
                },
            );
        }
        let file = self.files.get_mut(path).unwrap();
        if let Some(expected) = expected {
            ensure!(
                file.sha256 == expected,
                "preload asset hash differs: {path}; recook its source package"
            );
        }
        file.roles.insert(role);
        Ok(())
    }

    fn audio(&mut self, path: &str) -> Result<()> {
        let audio: FieldAudio = self.json(path, None, Role::AudioManifest)?;
        self.audio_assets(&audio)
    }

    pub(crate) fn audio_assets(&mut self, audio: &FieldAudio) -> Result<()> {
        audio.validate()?;
        for asset in audio.music.values().chain(audio.sounds.values()) {
            let package: resonance_audio::package::Package =
                self.json(&asset.path, Some(&asset.sha256), Role::AudioPackage)?;
            ensure!(
                package.version == resonance_audio::package::VERSION,
                "unsupported preload audio package"
            );
            for sample in package.samples.values() {
                self.add(&sample.path, Some(&sample.sha256), Role::InstrumentSample)?;
            }
        }
        for voice in audio.voices.values() {
            self.add(&voice.asset.path, Some(&voice.asset.sha256), Role::Voice)?;
        }
        Ok(())
    }
}
