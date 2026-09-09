//! Offline preparation inventory. This describes work to prepare, not live events
//! to execute or a promise that a renderer has finished uploading its assets.
use crate::validate_asset_path;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inputs {
    /// Cooked-root-relative FieldAssets manifest.
    pub field: String,
    /// Every audio bank available to this field, across all story branches.
    pub audio: BTreeSet<String>,
    /// Movie metadata, not movie payload paths. Payloads remain streamable.
    pub movies: BTreeSet<String>,
}

impl Inputs {
    pub fn validate(&self) -> Result<()> {
        for path in std::iter::once(&self.field)
            .chain(&self.audio)
            .chain(&self.movies)
        {
            validate_asset_path(path)?;
        }
        ensure!(self.field.ends_with(".json"), "field input must be JSON");
        Ok(())
    }

    pub fn manifest_path(&self) -> Result<String> {
        self.validate()?;
        Ok(format!(
            "{}.preload.json",
            self.field.strip_suffix(".json").unwrap()
        ))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub version: u32,
    pub map_id: u32,
    pub inputs: Inputs,
    /// A missing separately cooked input keeps this inventory incomplete.
    /// This does not certify that every original native has been implemented.
    pub missing_inputs: BTreeSet<String>,
    pub files: BTreeMap<String, File>,
    /// Unique on-disk bytes; deliberately not an estimate of decoded RAM/VRAM.
    pub total_file_bytes: u64,
    pub scenes: Vec<Scene>,
    pub features: BTreeSet<Feature>,
    pub scripts: Vec<Script>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct File {
    pub sha256: String,
    pub bytes: u64,
    pub roles: BTreeSet<Role>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Field,
    Data,
    Script,
    Mesh,
    Texture,
    AudioManifest,
    AudioPackage,
    InstrumentSample,
    Voice,
    MovieManifest,
    Movie,
}

/// The renderer reads full material/sampler/skeleton recipes from the referenced
/// FieldAssets part. Prepare all clips and materials, including hidden ones.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scene {
    /// None refers to FieldAssets.parts; Some refers to the matching actor.
    pub actor_resource: Option<u32>,
    pub part: usize,
    pub mesh: String,
    pub scene_index: usize,
    pub animation_indices: Vec<usize>,
    pub material_indices: Vec<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Feature {
    FieldGeometry,
    Actors,
    Animation,
    SecondaryMotion,
    TextureAnimation,
    ToonLighting,
    Outlines,
    ContactShadows,
    Dialogue,
    Choices,
    Subtitles,
    Billboards,
    Emotes,
    Audio,
    Movies,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Script {
    pub path: String,
    /// Default entry plus every active registry entry; branch conditions are
    /// never used to prune the decoder's control-flow traversal.
    pub entry_pcs: BTreeSet<u32>,
    pub instruction_count: usize,
    pub native_calls: Vec<NativeCall>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeCall {
    pub opcode: u8,
    /// Research label only, not a verified ABI or support classification.
    pub name: Option<String>,
    pub pcs: Vec<u32>,
}

impl Manifest {
    pub fn is_complete(&self) -> bool {
        self.missing_inputs.is_empty()
    }

    pub fn validate(&self) -> Result<()> {
        self.inputs.validate()?;
        ensure!(self.version == VERSION, "unsupported field preload version");
        ensure!(
            self.files.contains_key(&self.inputs.field),
            "missing field input"
        );
        let manifest_path = self.inputs.manifest_path()?;
        let mut total = 0u64;
        for (path, file) in &self.files {
            validate_asset_path(path)?;
            ensure!(path != &manifest_path, "preload manifest includes itself");
            ensure!(
                file.sha256.len() == 64
                    && file.sha256.bytes().all(|b| b.is_ascii_hexdigit())
                    && !file.roles.is_empty(),
                "invalid preload file {path}"
            );
            total = total
                .checked_add(file.bytes)
                .ok_or_else(|| anyhow::anyhow!("preload size overflow"))?;
        }
        ensure!(
            total == self.total_file_bytes,
            "preload byte count differs from inventory"
        );
        for path in self.inputs.audio.iter().chain(&self.inputs.movies) {
            ensure!(
                self.files.contains_key(path) != self.missing_inputs.contains(path),
                "input must be either present or missing: {path}"
            );
        }
        ensure!(
            self.missing_inputs
                .iter()
                .all(|p| self.inputs.audio.contains(p) || self.inputs.movies.contains(p)),
            "unknown missing preload input"
        );
        for scene in &self.scenes {
            ensure!(
                self.files
                    .get(&scene.mesh)
                    .is_some_and(|f| f.roles.contains(&Role::Mesh)),
                "scene mesh missing from preload"
            );
        }
        for script in &self.scripts {
            ensure!(
                self.files
                    .get(&script.path)
                    .is_some_and(|f| f.roles.contains(&Role::Script))
                    && !script.entry_pcs.is_empty(),
                "script missing from preload"
            );
        }
        Ok(())
    }
}
