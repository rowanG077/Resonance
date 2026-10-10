//! Versioned cooked JSON and ordinary PCM WAVs, shared by the importer/player.
use crate::{
    data::{Command, Resources, Score},
    music_voice::Tables,
    sample::Sample,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Cursor, Read},
    path::{Component, Path},
};

pub const VERSION: u32 = 4;
const MAX_PACKAGE_BYTES: usize = 16 * 1024 * 1024;

#[cfg(test)]
pub(crate) mod tests;

#[derive(Serialize, Deserialize)]
pub struct SampleAsset {
    pub path: String,
    pub sha256: String,
    pub key: u8,
    pub rate: u16,
    pub first_frames: u32,
    pub loop_start: u32,
    pub loop_length: u32,
}

#[derive(Serialize, Deserialize)]
pub struct Package {
    pub version: u32,
    pub programs: BTreeMap<u16, Vec<Command>>,
    pub samples: BTreeMap<u16, SampleAsset>,
    pub score: Score,
    pub tables: Tables,
    pub reverbs: [[f32; 5]; 2],
}

pub struct Loaded {
    resources: Resources,
    score: Score,
    tables: Tables,
    reverbs: [[f32; 5]; 2],
}

impl Loaded {
    /// Validate immutable playback data once, before it reaches a mixer.
    pub fn new(
        resources: Resources,
        score: Score,
        tables: Tables,
        reverbs: [[f32; 5]; 2],
    ) -> Result<Self> {
        validate_playback(&resources, &score, &tables)?;
        for reverb in reverbs {
            crate::reverb::validate_parameters(reverb)?;
        }
        Ok(Self {
            resources,
            score,
            tables,
            reverbs,
        })
    }

    pub fn resources(&self) -> &Resources {
        &self.resources
    }
    pub fn score(&self) -> &Score {
        &self.score
    }
    pub fn tables(&self) -> &Tables {
        &self.tables
    }
    pub fn reverbs(&self) -> [[f32; 5]; 2] {
        self.reverbs
    }
}

pub(crate) fn validate_playback(
    resources: &Resources,
    score: &Score,
    tables: &Tables,
) -> Result<()> {
    tables.validate()?;
    resources.validate()?;
    score.validate(resources)?;
    ensure!(
        tables.mix.spatial.is_some()
            || !resources
                .programs
                .values()
                .flatten()
                .any(|command| matches!(
                    command,
                    Command::VolumeCurve {
                        interaural_delay: true,
                        ..
                    }
                )),
        "instrument requires uncooked spatial audio tables"
    );
    Ok(())
}

/// Shares decoded samples while packages or live voices own them.
#[derive(Default)]
pub struct SampleCache(BTreeMap<String, std::sync::Weak<Sample>>);

impl SampleCache {
    pub fn prune(&mut self) {
        self.0.retain(|_, sample| sample.strong_count() != 0);
    }
}

pub(crate) fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)
        .with_context(|| format!("opening {}", path.display()))?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= limit, "music resource exceeds read budget");
    Ok(bytes)
}

pub(crate) fn relative_path(path: &str) -> Result<()> {
    ensure!(
        !path.is_empty()
            && !path.contains(['\\', ':', '#'])
            && Path::new(path)
                .components()
                .all(|part| matches!(part, Component::Normal(_))),
        "invalid music asset path"
    );
    Ok(())
}

impl Package {
    /// Load only cooked data. This path has no bank/executable or device access.
    pub fn load(root: &Path, path: &str) -> Result<Loaded> {
        Self::load_with(
            path,
            &mut |path, limit| read_bounded(&root.join(path), limit),
            &mut SampleCache::default(),
        )
    }

    pub fn load_verified(
        root: &Path,
        path: &str,
        expected_sha256: &str,
        cache: &mut SampleCache,
    ) -> Result<Loaded> {
        Self::load_with(
            path,
            &mut |requested, limit| {
                let bytes = read_bounded(&root.join(requested), limit)?;
                if requested == path {
                    ensure!(
                        format!("{:x}", Sha256::digest(&bytes)) == expected_sha256,
                        "music package digest differs from metadata"
                    );
                }
                Ok(bytes)
            },
            cache,
        )
    }

    pub fn load_with(
        path: &str,
        read: &mut impl FnMut(&str, usize) -> Result<Vec<u8>>,
        cache: &mut SampleCache,
    ) -> Result<Loaded> {
        relative_path(path)?;
        let bytes = read(path, MAX_PACKAGE_BYTES)?;
        ensure!(
            bytes.len() <= MAX_PACKAGE_BYTES,
            "music package exceeds read budget"
        );
        let package: Self = serde_json::from_slice(&bytes)?;
        package.prepare(
            &mut |asset, limit| {
                let bytes = read(&asset.path, limit)?;
                ensure!(
                    bytes.len() <= limit && format!("{:x}", Sha256::digest(&bytes)) == asset.sha256,
                    "instrument sample digest or size differs from manifest"
                );
                Ok(bytes.into())
            },
            cache,
        )
    }

    /// Decode once from an already parsed package and verified immutable samples.
    /// The reader validates each sample's digest and budget, including cache hits.
    pub fn prepare(
        self,
        read: &mut impl FnMut(&SampleAsset, usize) -> Result<std::sync::Arc<[u8]>>,
        cache: &mut SampleCache,
    ) -> Result<Loaded> {
        cache.prune();
        let package = self;
        ensure!(
            package.version == VERSION,
            "unsupported cooked music version; recook audio packages"
        );
        let mut samples = BTreeMap::new();
        let mut total = 0usize;
        for (id, asset) in package.samples {
            relative_path(&asset.path)?;
            let count = u64::from(asset.first_frames) + u64::from(asset.loop_length);
            ensure!(
                count <= 32_000_000,
                "instrument sample exceeds frame budget"
            );
            total = total
                .checked_add(count as usize)
                .context("music sample count overflow")?;
            ensure!(
                total <= 32_000_000,
                "music bank exceeds decoded frame budget"
            );
            // The same WAV can have different tuning/loop metadata.
            let key = serde_json::to_string(&(
                &asset.sha256,
                asset.key,
                asset.rate,
                asset.first_frames,
                asset.loop_start,
                asset.loop_length,
            ))?;
            let bytes = read(&asset, count as usize * 2 + 1024 * 1024)?;
            if let Some(sample) = cache.0.get(&key).and_then(std::sync::Weak::upgrade) {
                samples.insert(id, sample);
                continue;
            }
            let mut wave = hound::WavReader::new(Cursor::new(bytes))?;
            let spec = wave.spec();
            ensure!(
                spec.channels == 1
                    && spec.sample_format == hound::SampleFormat::Int
                    && spec.bits_per_sample == 16
                    && spec.sample_rate == u32::from(asset.rate)
                    && u64::from(wave.duration()) == count,
                "instrument WAV differs from manifest"
            );
            let mut pcm: Vec<_> = wave
                .samples::<i16>()
                .collect::<std::result::Result<_, _>>()?;
            ensure!(pcm.len() == count as usize, "truncated instrument WAV");
            let loop_pcm = pcm.split_off(asset.first_frames as usize);
            let sample = std::sync::Arc::new(Sample {
                key: asset.key,
                rate: asset.rate,
                loop_start: asset.loop_start,
                loop_length: asset.loop_length,
                pcm,
                loop_pcm,
            });
            cache.0.insert(key, std::sync::Arc::downgrade(&sample));
            samples.insert(id, sample);
        }
        let resources = Resources {
            programs: package.programs,
            samples,
        };
        Loaded::new(resources, package.score, package.tables, package.reverbs)
    }
}

pub(crate) mod array {
    use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};
    pub fn serialize<T: Serialize, S: Serializer, const N: usize>(
        value: &[T; N],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        value.as_slice().serialize(serializer)
    }
    pub fn deserialize<'de, T: Deserialize<'de>, D: Deserializer<'de>, const N: usize>(
        deserializer: D,
    ) -> Result<[T; N], D::Error> {
        Vec::<T>::deserialize(deserializer)?
            .try_into()
            .map_err(|v: Vec<T>| {
                D::Error::custom(format!("expected {N} table entries, got {}", v.len()))
            })
    }
}

pub(crate) mod coefficients {
    use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};
    pub fn serialize<S: Serializer>(
        value: &[[[i16; 4]; 128]; 4],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        value
            .iter()
            .flatten()
            .flatten()
            .copied()
            .collect::<Vec<_>>()
            .serialize(serializer)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<[[[i16; 4]; 128]; 4], D::Error> {
        let values = Vec::<i16>::deserialize(deserializer)?;
        if values.len() != 2048 {
            return Err(D::Error::custom("expected 2048 interpolation coefficients"));
        }
        let mut tables = [[[0; 4]; 128]; 4];
        for (target, value) in tables.iter_mut().flatten().flatten().zip(values) {
            *target = value;
        }
        Ok(tables)
    }
}
