//! Offline preparation inventory. This describes work to prepare, not live events
//! to execute or a promise that a renderer has finished uploading its assets.
use crate::validate_asset_path;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const VERSION: u32 = 2;
pub const SHARED_PATH: &str = "shared.preload.json";

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
pub struct Manifest<Entry = File> {
    pub version: u32,
    pub map_id: u32,
    pub inputs: Inputs,
    /// Separately cooked media inputs that are not yet available.
    pub missing_inputs: BTreeSet<String>,
    pub files: BTreeMap<String, Entry>,
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

/// Dependencies used by every field, published once.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shared<Entry = File> {
    pub version: u32,
    pub files: BTreeMap<String, Entry>,
}

impl Shared {
    pub fn validate(&self) -> Result<()> {
        self.validate_structure()?;
        validate_files(&self.files)
    }
}

impl<Entry> Shared<Entry> {
    /// Check the container before admitting only the dependencies a caller needs.
    pub fn validate_structure(&self) -> Result<()> {
        ensure!(
            self.version == VERSION,
            "unsupported shared preload version"
        );
        ensure!(
            !self.files.contains_key(SHARED_PATH),
            "shared inventory includes itself"
        );
        Ok(())
    }
}

fn validate_files(files: &BTreeMap<String, File>) -> Result<()> {
    for (path, file) in files {
        file.validate(path)?;
    }
    Ok(())
}

impl File {
    pub fn validate(&self, path: &str) -> Result<()> {
        validate_asset_path(path)?;
        ensure!(
            self.sha256.len() == 64
                && self.sha256.bytes().all(|b| b.is_ascii_hexdigit())
                && !self.roles.is_empty(),
            "invalid preload file {path}"
        );
        Ok(())
    }
}

impl Manifest {
    pub fn validate(&self) -> Result<()> {
        self.validate_structure()?;
        validate_files(&self.files)
    }
}

impl<Entry> Manifest<Entry> {
    pub fn is_complete(&self) -> bool {
        self.missing_inputs.is_empty()
    }

    pub(crate) fn validate_structure(&self) -> Result<()> {
        self.inputs.validate()?;
        ensure!(self.version == VERSION, "unsupported field preload version");
        ensure!(
            self.files.contains_key(&self.inputs.field),
            "missing field input"
        );
        ensure!(
            !self.files.contains_key(&self.inputs.manifest_path()?),
            "preload manifest includes itself"
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
        Ok(())
    }
}
