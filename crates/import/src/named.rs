//! Publish prepared values and converted payloads under stable, meaningful owners.
//! Converter scratch paths never become identities of editable files.
use anyhow::{Context, Result, ensure};
use resonance_content::{
    SceneClip, ScenePart,
    field::{
        self, FieldAssets,
        document::{self, Actor, Definition, Model},
    },
    library::{self, Descriptor, Mappings},
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

mod audio;
mod battle;
pub(crate) use battle::party_path;
mod geometry;
mod images;
pub use geometry::PhysicalScene;
mod movies;
mod resources;
mod screens;
mod shared;
mod startup;
mod visuals;
mod world;
pub use audio::AudioSource;
pub(crate) use audio::source_name;
pub use visuals::FieldArt;

/// Destination identities guard against accidental sharing. Repeated publication
/// of a deliberately shared model must agree; unrelated owners get separate files.
pub struct Publisher<'a> {
    source: &'a Path,
    output: &'a Path,
    published: BTreeMap<String, String>,
    spelling: BTreeMap<String, String>,
    fields: BTreeMap<u32, String>,
    audio_source: Option<AudioSource>,
    mappings: BTreeMap<String, BTreeMap<u32, String>>,
    rendered_music: BTreeSet<u16>,
    published_effects: BTreeSet<(u8, u16)>,
    audio_outputs: BTreeMap<String, BTreeSet<String>>,
}

impl<'a> Publisher<'a> {
    pub fn new(source: &'a Path, output: &'a Path) -> Result<Self> {
        fs::create_dir_all(output)?;
        ensure!(source.is_dir(), "converted payload directory is missing");
        Ok(Self {
            source,
            output,
            published: BTreeMap::new(),
            spelling: BTreeMap::new(),
            fields: BTreeMap::new(),
            audio_source: None,
            mappings: BTreeMap::new(),
            rendered_music: BTreeSet::new(),
            published_effects: BTreeSet::new(),
            audio_outputs: BTreeMap::new(),
        })
    }

    pub fn with_audio_source(mut self, source: AudioSource) -> Self {
        self.audio_source = Some(source);
        self
    }

    fn audio_source(&self) -> Result<&AudioSource> {
        self.audio_source
            .as_ref()
            .context("audio publication needs original disc inputs")
    }

    pub fn field(
        &mut self,
        name: &str,
        field: FieldAssets,
        art: FieldArt,
        movies: BTreeSet<u32>,
    ) -> Result<()> {
        let audio = self.field_audio_definition(field.map_id)?;
        self.field_with_audio(name, field, art, movies, audio)
    }

    pub(crate) fn field_with_audio(
        &mut self,
        name: &str,
        mut field: FieldAssets,
        art: FieldArt,
        movies: BTreeSet<u32>,
        audio: resonance_content::field_audio::document::Definition,
    ) -> Result<()> {
        library::validate_path(name)?;
        ensure!(
            !name.contains('/'),
            "field name must be one directory: {name}"
        );
        field.validate()?;
        ensure!(
            !self.fields.contains_key(&field.map_id),
            "field {} was published twice",
            field.map_id
        );
        let root = format!("fields/{name}");
        let definition = Definition {
            version: document::VERSION,
            script: format!("{root}/scripts/scenario.ssb"),
            messages: format!("{root}/messages.json"),
            scene: format!("{root}/scene.json"),
            collision: format!("{root}/collision.json"),
            actors: format!("{root}/actors.json"),
            audio: format!("{root}/audio.json"),
            movies,
        };
        self.copy(&field.script, &definition.script)?;
        self.copy(&field.messages, &definition.messages)?;
        self.json(&definition.audio, &audio)?;
        let id = field.map_id;
        field.script.clone_from(&definition.script);
        field.messages.clone_from(&definition.messages);
        for (index, part) in field.parts.iter_mut().enumerate() {
            self.part(&root, index, part)?;
        }
        self.field_art(&root, &mut field, art)?;
        if let Some(path) = &mut field.panorama {
            *path = self.image(path, &format!("{root}/textures/panorama"))?;
        }
        for (index, emitter) in field.environment.particles.iter_mut().enumerate() {
            emitter.sprite.texture = self.image(
                &emitter.sprite.texture,
                &format!("{root}/textures/ambient-{index}"),
            )?;
        }
        if let Some(haze) = &mut field.environment.heat_haze {
            haze.sprite.texture =
                self.image(&haze.sprite.texture, &format!("{root}/textures/heat-haze"))?;
        }
        let (scene, collision, actors) = document::split(field);
        let mut bindings = Vec::with_capacity(actors.len());
        for mut actor in actors {
            let shared = party_owner(actor.resource);
            let owner = shared
                .clone()
                .unwrap_or_else(|| format!("{root}/models/model-{:08x}", actor.resource));
            let mut animations = BTreeMap::new();
            for (index, part) in actor.parts.iter_mut().enumerate() {
                // Shared character geometry and its own motions are one editable
                // resource; scenario-specific animation bindings remain field-owned.
                let (base, mut extra): (Vec<_>, Vec<_>) = part.clips.drain(..).partition(|clip| {
                    shared.is_none()
                        || clip.animation_resource.is_none_or(|id| {
                            (field::FIELD_SERVICE_MOTION_RESOURCE_BASE
                                ..field::FIELD_SERVICE_MOTION_RESOURCE_BASE + 256)
                                .contains(&id)
                        })
                });
                part.clips = base;
                if !extra.is_empty() {
                    let motions = format!("{root}/animations/model-{:08x}", actor.resource);
                    self.clips(&motions, index, &mut extra)?;
                    animations.insert(u16::try_from(index)?, extra);
                }
            }
            let model = Model {
                version: document::VERSION,
                parts: actor.parts,
                collision: actor.collision,
            };
            let model_path = self.model(&owner, model)?;
            bindings.push(Actor {
                resource: actor.resource,
                model: model_path,
                hidden_nodes: actor.hidden_nodes,
                animations,
            });
        }
        self.json(&definition.scene, &scene)?;
        self.json(&definition.collision, &collision)?;
        self.json(&definition.actors, &bindings)?;
        self.json(&format!("{root}/field.json"), &definition)?;
        self.fields.insert(id, name.into());
        Ok(())
    }

    /// Publish the completed mapping once, after all selected field publications
    /// succeed. The complete cooker will use this boundary for every domain.
    pub fn finish_fields(&mut self) -> Result<()> {
        let mappings = Mappings::new(self.fields.clone())?;
        self.json(library::FIELDS, &mappings)?;
        self.finish_audio()
    }

    pub fn finish_audio(&mut self) -> Result<()> {
        for (path, entries) in self.mappings.clone() {
            self.json(&path, &Mappings::new(entries)?)?;
        }
        Ok(())
    }

    pub(crate) fn audio_source_outputs(&self) -> &BTreeMap<String, BTreeSet<String>> {
        &self.audio_outputs
    }

    pub fn descriptor(&mut self, game: &str, revision: u32) -> Result<()> {
        let descriptor = Descriptor {
            version: library::VERSION,
            game: game.into(),
            revision,
        };
        descriptor.validate()?;
        self.json(library::DESCRIPTOR, &descriptor)
    }

    pub(crate) fn model(&mut self, owner: &str, mut model: Model) -> Result<String> {
        model.validate()?;
        for (index, part) in model.parts.iter_mut().enumerate() {
            self.part(owner, index, part)?;
        }
        let path = format!("{owner}/model.json");
        self.json(&path, &model)?;
        Ok(path)
    }

    fn part(&mut self, owner: &str, index: usize, part: &mut ScenePart) -> Result<()> {
        self.body(owner, index, part)?;
        self.clips(&format!("{owner}/animations"), index, &mut part.clips)
    }

    /// Meshes and textures may have an intentional shared body owner while
    /// purpose-specific animation banks retain their own native slot scopes.
    fn body(&mut self, owner: &str, index: usize, part: &mut ScenePart) -> Result<()> {
        let mesh = format!("{owner}/meshes/part-{index}.glb");
        self.copy(&part.mesh, &mesh)?;
        part.mesh = mesh;
        for (texture, path) in part.textures.iter_mut().enumerate() {
            *path = self.image(
                path,
                &format!("{owner}/textures/part-{index}-texture-{texture}"),
            )?;
        }
        Ok(())
    }

    fn clips(&mut self, owner: &str, part: usize, clips: &mut [SceneClip]) -> Result<()> {
        for clip in clips {
            let bank = clip
                .animation_resource
                .map_or_else(|| "local".into(), |id| format!("resource-{id:08x}"));
            let path = format!(
                "{owner}/part-{part}/{bank}/slot-{}.motion.json",
                clip.resource_slot
            );
            let bytes = fs::read(self.source.join(&clip.motion))
                .with_context(|| format!("read publication source {}", clip.motion))?;
            let motion = resonance_content::animation::Motion::decode(&bytes)
                .with_context(|| format!("decode publication source {}", clip.motion))?;
            self.write(&path, &motion.encode()?)?;
            clip.motion = path;
            clip.duration_seconds = 0.;
        }
        Ok(())
    }

    fn copy(&mut self, source: &str, destination: &str) -> Result<()> {
        library::validate_path(source)?;
        let bytes = fs::read(self.source.join(source))
            .with_context(|| format!("read publication source {source}"))?;
        self.write(destination, &bytes)
    }

    fn json(&mut self, path: &str, value: &impl Serialize) -> Result<()> {
        let mut bytes = serde_json::to_vec_pretty(value)?;
        bytes.push(b'\n');
        self.write(path, &bytes)
    }

    fn write(&mut self, path: &str, bytes: &[u8]) -> Result<()> {
        self.check_spelling(path)?;
        let hash = crate::digest(bytes);
        if self.already_published(path, &hash)? {
            return Ok(());
        }
        // Write independent files. Hard links would couple otherwise separate
        // owners when a modder edits a texture or geometry file in place.
        crate::write_atomic(&self.output.join(path), bytes)?;
        self.remember(path, hash)?;
        Ok(())
    }

    fn remember(&mut self, path: &str, hash: String) -> Result<()> {
        crate::publication::export(&self.output.join(path))?;
        self.published.insert(path.into(), hash);
        Ok(())
    }

    fn check_spelling(&mut self, path: &str) -> Result<()> {
        library::validate_path(path)?;
        let mut prefix = String::new();
        for component in path.split('/') {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(component);
            let folded = prefix.to_lowercase();
            if let Some(previous) = self.spelling.get(&folded) {
                ensure!(
                    previous == &prefix,
                    "case-conflicting publication paths: {previous} and {prefix}"
                );
            }
            self.spelling.insert(folded, prefix.clone());
        }
        Ok(())
    }

    fn already_published(&self, path: &str, hash: &str) -> Result<bool> {
        if let Some(previous) = self.published.get(path) {
            ensure!(
                previous == hash,
                "conflicting named publication {path}; distinct resources require distinct owners"
            );
            return Ok(true);
        }
        let existing = self.output.join(path);
        if existing.try_exists()? {
            ensure!(
                crate::media::hash_file(&existing)? == hash,
                "refusing to replace an existing named resource: {path}; cook into a fresh directory"
            );
            crate::publication::export(&existing)?;
            return Ok(true);
        }
        Ok(false)
    }
}

fn party_owner(resource: u32) -> Option<String> {
    let (character, costume) = if (1..=9).contains(&resource) {
        (resource, 0)
    } else if resource & 0xffff_0000 == 0x2300_0000 && (1..=9).contains(&(resource & 0xff)) {
        (resource & 0xff, (resource >> 8) & 0xff)
    } else {
        return None;
    };
    let name = [
        "lloyd", "colette", "genis", "raine", "sheena", "zelos", "presea", "regal", "kratos",
    ][character as usize - 1];
    Some(if costume == 0 {
        format!("meshes/characters/{name}")
    } else {
        format!("meshes/characters/{name}/costume-{costume}")
    })
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) use battle::tests::equivalent as equivalent_assets;
