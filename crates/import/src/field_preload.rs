//! Conservative preparation manifests for cooked fields. Never executes a VM,
//! prunes a story branch, or opens a graphics/audio device.
use crate::{media::hash_file, write_atomic};
use anyhow::{Context, Result, ensure};
pub use resonance_content::field_preload::Inputs;
use resonance_content::{
    MovieAsset, ScenePart,
    effect::FieldEffects,
    field::FieldAssets,
    field_audio::FieldAudio,
    field_preload::{Feature, File, Manifest, NativeCall, Role, Scene, Script, VERSION},
    font::{BitmapFont, DialogueArt},
    validate_asset_path,
};
use serde::de::DeserializeOwned;
use std::{collections::BTreeMap, fs, io::ErrorKind, path::Path};
use symphonia_script::{Program, scenario, semantics::NativeRegistry};

#[cfg(test)]
mod tests;

/// Writes <field>.preload.json beside the field metadata. Separately cooked
/// inputs may not exist yet; they remain explicitly listed as missing. Missing
/// payloads, changed hashes, and malformed existing metadata are errors.
pub fn cook(root: &Path, inputs: Inputs) -> Result<Manifest> {
    let manifest = build(root, inputs)?;
    let path = manifest.inputs.manifest_path()?;
    write_atomic(&root.join(&path), &serde_json::to_vec_pretty(&manifest)?)?;
    println!(
        "Prepared {path}: {} files, {} bytes, {} scenes, {} missing inputs",
        manifest.files.len(),
        manifest.total_file_bytes,
        manifest.scenes.len(),
        manifest.missing_inputs.len()
    );
    Ok(manifest)
}

/// Pure filesystem inspection plus static script analysis; no output writes.
pub fn build(root: &Path, inputs: Inputs) -> Result<Manifest> {
    inputs.validate()?;
    let mut inventory = Inventory::new(root);
    let field: FieldAssets = inventory.json(&inputs.field, None, Role::Field)?;
    field.validate()?;
    for (path, hash) in &field.files {
        inventory.add(
            path,
            Some(hash),
            match Path::new(path).extension().and_then(|s| s.to_str()) {
                Some("ssb") => Role::Script,
                Some("glb") => Role::Mesh,
                Some("ktx2" | "png") => Role::Texture,
                _ => Role::Data,
            },
        )?;
    }
    let mut manifest = Manifest {
        version: VERSION,
        map_id: field.map_id,
        inputs,
        missing_inputs: Default::default(),
        files: Default::default(),
        total_file_bytes: 0,
        scenes: Vec::new(),
        features: [
            Feature::FieldGeometry,
            Feature::ContactShadows,
            Feature::ToonLighting,
        ]
        .into(),
        scripts: Vec::new(),
    };
    for (index, part) in field.parts.iter().enumerate() {
        add_scene(&mut manifest, None, index, part);
    }
    for actor in &field.actors {
        manifest.features.insert(Feature::Actors);
        for (index, part) in actor.parts.iter().enumerate() {
            add_scene(&mut manifest, Some(actor.resource), index, part);
        }
    }

    // Close the shared UI/effect descriptors as well as the field's file list.
    // Keep every emote recipe and glyph, not just the current dialogue/route.
    let ui: DialogueArt = inventory.json("ui/dialogue.json", None, Role::Data)?;
    ui.validate()?;
    let font: BitmapFont = inventory.json(&ui.font, None, Role::Data)?;
    font.validate()?;
    inventory.add(&font.texture, None, Role::Texture)?;
    for texture in ui.textures.iter().chain([&ui.cursor]) {
        inventory.add(&texture.path, None, Role::Texture)?;
    }
    let menu: resonance_content::menu::MenuArt =
        inventory.json("ui/menu.json", None, Role::Data)?;
    menu.validate()?;
    for texture in menu.textures {
        inventory.add(&texture.path, None, Role::Texture)?;
    }
    let data: resonance_content::menu_data::MenuData =
        inventory.json("game/menu-data.json", None, Role::Data)?;
    data.validate()?;
    for c in data.texts().flat_map(str::chars).filter(|c| *c != '\n') {
        ensure!(font.glyphs.contains_key(&c), "uncooked menu glyph {c:?}");
    }
    if inventory.files.contains_key("game/skits.json") {
        let skits: resonance_content::skit::SkitCatalog =
            inventory.json("game/skits.json", None, Role::Data)?;
        skits.validate()?;
        for resource in skits.resources.values() {
            inventory.add(&resource.script, None, Role::Script)?;
            inventory.add(&resource.messages, None, Role::Data)?;
            let messages: Vec<symphonia_script::message::Message> =
                serde_json::from_slice(&fs::read(root.join(&resource.messages))?)?;
            crate::font::validate_messages(&font, &messages)?;
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
    manifest
        .features
        .extend([Feature::Dialogue, Feature::Choices]);
    if inventory.files.contains_key("ui/story-subtitles.json") {
        manifest.features.insert(Feature::Subtitles);
    }
    let effects: FieldEffects = inventory.json(&field.effects, None, Role::Data)?;
    effects.validate()?;
    for sprite in effects.sprites.values().chain([&effects.refraction.sprite]) {
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
    manifest
        .features
        .extend([Feature::Billboards, Feature::Emotes]);

    for path in &manifest.inputs.audio {
        manifest.features.insert(Feature::Audio);
        if !input_exists(root, path)? {
            manifest.missing_inputs.insert(path.clone());
            continue;
        }
        inventory.audio(path)?;
    }
    for path in &manifest.inputs.movies {
        manifest.features.insert(Feature::Movies);
        if !input_exists(root, path)? {
            manifest.missing_inputs.insert(path.clone());
            continue;
        }
        let movie: MovieAsset = inventory.json(path, None, Role::MovieManifest)?;
        movie.validate()?;
        inventory.add(&movie.path, Some(&movie.sha256), Role::Movie)?;
    }

    // The decoder walks both branch successors, subroutine calls and every
    // active registry root. No native execution or guessed return values.
    let registry = NativeRegistry::gqseaf();
    for (path, file) in &inventory.files {
        if file.roles.contains(&Role::Script) {
            let bytes = fs::read(root.join(path))?;
            ensure!(
                crate::digest(&bytes) == file.sha256,
                "preload script changed during analysis: {path}"
            );
            manifest
                .scripts
                .push(analyze_script(path, &bytes, &registry)?);
        }
    }
    manifest.files = inventory.files;
    manifest.total_file_bytes = manifest.files.values().try_fold(0u64, |sum, file| {
        sum.checked_add(file.bytes)
            .context("preload byte count overflow")
    })?;
    manifest.validate()?;
    Ok(manifest)
}

fn add_scene(manifest: &mut Manifest, actor: Option<u32>, index: usize, part: &ScenePart) {
    manifest.scenes.push(Scene {
        actor_resource: actor,
        part: index,
        mesh: part.mesh.clone(),
        scene_index: 0,
        animation_indices: (0..part.clips.len()).collect(),
        material_indices: (0..part.materials.len()).collect(),
    });
    if !part.clips.is_empty() {
        manifest.features.insert(Feature::Animation);
    }
    if !part.secondary_motion.is_empty() {
        manifest.features.insert(Feature::SecondaryMotion);
    }
    if !part.texture_animations.is_empty() || part.appearance.is_some() {
        manifest.features.insert(Feature::TextureAnimation);
    }
    if part.outline_color.is_some() {
        manifest.features.insert(Feature::Outlines);
    }
}

fn analyze_script(path: &str, bytes: &[u8], registry: &NativeRegistry) -> Result<Script> {
    // Reject undecoded code instead of writing an apparently complete report.
    Program::decode(bytes).with_context(|| format!("validate preload script {path}"))?;
    let analysis = scenario::analyze(bytes)?;
    let mut calls = BTreeMap::<u8, Vec<u32>>::new();
    for instruction in analysis
        .instructions
        .values()
        .filter(|i| i.mnemonic == "proc")
    {
        calls
            .entry(instruction.operands[0] as u8)
            .or_default()
            .push(instruction.pc);
    }
    Ok(Script {
        path: path.into(),
        entry_pcs: analysis.roots,
        instruction_count: analysis.instructions.len(),
        native_calls: calls
            .into_iter()
            .map(|(opcode, pcs)| NativeCall {
                opcode,
                name: registry.get(opcode).map(|n| n.name.clone()),
                pcs,
            })
            .collect(),
    })
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
}

impl<'a> Inventory<'a> {
    pub(crate) fn new(root: &'a Path) -> Self {
        Self {
            root,
            files: BTreeMap::new(),
        }
    }

    pub(crate) fn into_files(self) -> BTreeMap<String, File> {
        self.files
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
            crate::digest(&bytes) == self.files[path].sha256,
            "preload metadata changed during analysis: {path}"
        );
        serde_json::from_slice(&bytes).with_context(|| format!("decode {path}"))
    }

    pub(crate) fn add(&mut self, path: &str, expected: Option<&str>, role: Role) -> Result<()> {
        validate_asset_path(path)?;
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
