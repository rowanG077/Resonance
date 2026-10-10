//! Program-only menu cue manifests; synthesis and group gain remain live.
use crate::package::{read_bounded, relative_path};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path, sync::Arc};

pub const VERSION: u32 = 5;

#[cfg(test)]
mod tests;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Program {
    pub path: String,
    pub sha256: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    pub program: Program,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest<Entry = Asset> {
    pub version: u32,
    pub sample_rate: u32,
    pub reverbs: [[f32; 5]; 2],
    pub cues: BTreeMap<String, Entry>,
}

pub struct Loaded {
    pub sample_rate: u32,
    pub reverbs: [[f32; 5]; 2],
    pub cues: BTreeMap<String, Arc<crate::package::Loaded>>,
}

impl Manifest {
    pub fn load(
        root: &Path,
        path: &str,
        expected_sha256: &str,
        mut on_error: impl FnMut(&str, anyhow::Error) -> Result<()>,
    ) -> Result<Loaded> {
        relative_path(path)?;
        let bytes = read_bounded(&root.join(path), 1024 * 1024)?;
        ensure!(
            format!("{:x}", Sha256::digest(&bytes)) == expected_sha256,
            "cue manifest digest differs from metadata"
        );
        let manifest: Manifest<serde_json::Value> = serde_json::from_slice(&bytes)?;
        ensure!(
            manifest.version == VERSION && manifest.sample_rate == crate::SOURCE_RATE,
            "unsupported cooked cue package; recook title sounds"
        );
        ensure!(
            !manifest.cues.is_empty() && manifest.cues.len() <= 1024,
            "invalid cue count"
        );
        for parameters in manifest.reverbs {
            crate::reverb::validate_parameters(parameters)?;
        }
        let mut cues = BTreeMap::new();
        let mut samples = crate::package::SampleCache::default();
        for (name, asset) in manifest.cues {
            let result = (|| -> Result<crate::package::Loaded> {
                ensure!(!name.is_empty() && name.len() <= 128, "invalid cue name");
                let asset: Asset = serde_json::from_value(asset)?;
                let package = crate::package::Package::load_verified(
                    root,
                    &asset.program.path,
                    &asset.program.sha256,
                    &mut samples,
                )?;
                ensure!(
                    package.reverbs() == manifest.reverbs,
                    "cue uses a different effects studio"
                );
                Ok(package)
            })();
            match result {
                Ok(cue) => {
                    cues.insert(name, Arc::new(cue));
                }
                Err(error) => on_error(&name, error)?,
            }
        }
        Ok(Loaded {
            sample_rate: manifest.sample_rate,
            reverbs: manifest.reverbs,
            cues,
        })
    }
}
