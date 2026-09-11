//! Mono envelope PCM and typed controls; group gain and effects stay live.
use super::{Control, Cue, Studio};
use crate::mix::Tables;
use crate::package::{read_bounded, relative_path};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::Cursor, path::Path, sync::Arc};

pub const VERSION: u32 = 3;

#[cfg(test)]
mod tests;

#[derive(Serialize, Deserialize)]
pub struct Sample {
    pub path: String,
    pub sha256: String,
}

#[derive(Serialize, Deserialize)]
pub struct Asset {
    pub frames: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sample: Option<Sample>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub program: Option<Sample>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub controls: Vec<Control>,
}

#[derive(Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub sample_rate: u32,
    pub reverbs: [[f32; 5]; 2],
    pub tables: Tables,
    pub cues: BTreeMap<String, Asset>,
}

pub struct Loaded {
    pub sample_rate: u32,
    pub reverbs: [[f32; 5]; 2],
    pub cues: BTreeMap<String, Arc<Cue>>,
}

impl Manifest {
    pub fn load(root: &Path, path: &str, expected_sha256: &str) -> Result<Loaded> {
        relative_path(path)?;
        let bytes = read_bounded(&root.join(path), 1024 * 1024)?;
        ensure!(
            format!("{:x}", Sha256::digest(&bytes)) == expected_sha256,
            "cue manifest digest differs from metadata"
        );
        let manifest: Self = serde_json::from_slice(&bytes)?;
        ensure!(
            (2..=VERSION).contains(&manifest.version) && manifest.sample_rate == 32028,
            "unsupported cooked cue package; recook title sounds"
        );
        ensure!(
            !manifest.cues.is_empty() && manifest.cues.len() <= 1024,
            "invalid cue count"
        );
        Studio::new(manifest.reverbs)?;
        manifest.tables.validate()?;
        let tables = Arc::new(manifest.tables);
        let mut total = 0u64;
        let mut cues = BTreeMap::new();
        for (name, asset) in manifest.cues {
            ensure!(
                !name.is_empty() && name.len() <= 128 && (1..=320_000).contains(&asset.frames),
                "invalid cue name or duration"
            );
            total += u64::from(asset.frames);
            ensure!(total <= 4_000_000, "cue bank exceeds frame budget");
            ensure!(
                asset.sample.is_some() != asset.program.is_some(),
                "cue needs one source"
            );
            if let Some(program) = asset.program {
                ensure!(asset.controls.is_empty(), "program cues own their controls");
                relative_path(&program.path)?;
                let bytes = read_bounded(&root.join(&program.path), 4 * 1024 * 1024)?;
                ensure!(
                    format!("{:x}", Sha256::digest(&bytes)) == program.sha256,
                    "cue program digest differs from manifest"
                );
                let package = crate::package::Package::load(root, &program.path)?;
                ensure!(
                    package.reverbs == manifest.reverbs,
                    "cue uses a different effects studio"
                );
                cues.insert(
                    name,
                    Arc::new(Cue::program(Arc::new(package), asset.frames as usize)?),
                );
                continue;
            }
            let sample = asset.sample.unwrap();
            relative_path(&sample.path)?;
            let bytes = read_bounded(
                &root.join(sample.path),
                asset.frames as usize * 2 + 1024 * 1024,
            )?;
            ensure!(
                format!("{:x}", Sha256::digest(&bytes)) == sample.sha256,
                "cue sample digest differs from manifest"
            );
            let mut wave = hound::WavReader::new(Cursor::new(bytes))?;
            let spec = wave.spec();
            ensure!(
                spec.channels == 1
                    && spec.sample_rate == manifest.sample_rate
                    && spec.bits_per_sample == 16
                    && spec.sample_format == hound::SampleFormat::Int
                    && wave.duration() == asset.frames,
                "cue sample format differs from manifest"
            );
            let pcm = wave
                .samples::<i16>()
                .collect::<std::result::Result<Vec<_>, _>>()?;
            ensure!(pcm.len() == asset.frames as usize, "truncated cue sample");
            cues.insert(
                name,
                Arc::new(Cue::controlled(pcm, asset.controls, tables.clone())?),
            );
        }
        Ok(Loaded {
            sample_rate: manifest.sample_rate,
            reverbs: manifest.reverbs,
            cues,
        })
    }
}
