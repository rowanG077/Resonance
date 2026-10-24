//! Authoritative program JSON and ordinary WAV samples. Header facts and cache
//! digests are prepared from current bytes, never maintained by a mod author.
use super::*;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{io::Cursor, sync::Arc};

pub const VERSION: u32 = 1;
pub const MAX_SAMPLE_BYTES: usize = 64 * 1024 * 1024 + 1024 * 1024;
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Program {
    pub version: u32,
    #[serde(deserialize_with = "id_table")]
    pub programs: BTreeMap<u16, Vec<Command>>,
    #[serde(deserialize_with = "id_table")]
    pub samples: BTreeMap<u16, Sample>,
    pub score: Score,
    pub tables: Tables,
    /// Standalone arrangements may own their studio. Shared songs and cues
    /// inherit the selected field/battle environment instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reverbs: Option<[[f32; 5]; 2]>,
}

// JSON object keys remain strings when serde buffers an internally tagged
// effect. Read the keys explicitly so the same program works both on its own
// and inside kind=synthesized, retaining duplicate and canonical-ID checks.
fn id_table<'de, D, V>(deserializer: D) -> std::result::Result<BTreeMap<u16, V>, D::Error>
where
    D: serde::Deserializer<'de>,
    V: Deserialize<'de>,
{
    use serde::de::{Error, MapAccess, Visitor};
    struct IdTable<V>(std::marker::PhantomData<V>);
    impl<'de, V: Deserialize<'de>> Visitor<'de> for IdTable<V> {
        type Value = BTreeMap<u16, V>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("an object with canonical u16 IDs")
        }

        fn visit_map<A: MapAccess<'de>>(
            self,
            mut map: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            let mut result = BTreeMap::new();
            while let Some((key, value)) = map.next_entry::<String, V>()? {
                let id = key.parse::<u16>().map_err(A::Error::custom)?;
                if key != id.to_string() || result.insert(id, value).is_some() {
                    return Err(A::Error::custom(format!(
                        "duplicate or noncanonical audio ID {key}"
                    )));
                }
            }
            Ok(result)
        }
    }
    deserializer.deserialize_map(IdTable(std::marker::PhantomData))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sample {
    pub path: String,
    /// MIDI root note; changing it changes the instrument's tuning.
    pub key: u8,
    pub loop_start: u32,
    /// Native repeated-loop PCM follows the first-pass PCM in the WAV. Keeping
    /// these frames separately preserves the original decoder's loop history.
    pub loop_length: u32,
}

impl Program {
    /// Capture all instrument dependencies, derive format facts, and use those
    /// same bytes for decoding. This accepts edits without any stored hash gate.
    pub fn prepare_editable(
        self,
        read: &mut impl FnMut(&str, usize) -> Result<Arc<[u8]>>,
        cache: &mut SampleCache,
    ) -> Result<Loaded> {
        self.prepare_in(None, read, cache)
    }

    pub fn prepare_in(
        self,
        environment: Option<[[f32; 5]; 2]>,
        read: &mut impl FnMut(&str, usize) -> Result<Arc<[u8]>>,
        cache: &mut SampleCache,
    ) -> Result<Loaded> {
        ensure!(
            self.version == VERSION,
            "unsupported editable audio program version"
        );
        ensure!(
            self.reverbs.is_none() || environment.is_none(),
            "shared audio programs inherit their playback environment; remove inline reverbs"
        );
        let reverbs = self
            .reverbs
            .or(environment)
            .context("audio program requires a playback environment")?;
        cache.prune();
        let mut captured = BTreeMap::new();
        let mut samples = BTreeMap::new();
        let mut frames = 0u64;
        for (id, sample) in self.samples {
            relative_path(&sample.path)?;
            let bytes = match captured.entry(sample.path.clone()) {
                std::collections::btree_map::Entry::Occupied(entry) => Arc::clone(entry.get()),
                std::collections::btree_map::Entry::Vacant(entry) => {
                    let bytes = read(&sample.path, MAX_SAMPLE_BYTES)
                        .with_context(|| format!("instrument sample {}", sample.path))?;
                    ensure!(
                        bytes.len() <= MAX_SAMPLE_BYTES,
                        "instrument sample exceeds read budget"
                    );
                    entry.insert(bytes).clone()
                }
            };
            let mut wave = hound::WavReader::new(Cursor::new(&*bytes))
                .with_context(|| format!("decode instrument WAV {}", sample.path))?;
            let spec = wave.spec();
            frames = frames
                .checked_add(u64::from(wave.duration()))
                .context("music sample count overflow")?;
            ensure!(
                frames <= 32_000_000,
                "music bank exceeds decoded frame budget"
            );
            ensure!(
                spec.channels == 1
                    && spec.bits_per_sample == 16
                    && spec.sample_format == hound::SampleFormat::Int,
                "instrument {} requires mono PCM16 WAV",
                sample.path
            );
            let first_frames = wave
                .duration()
                .checked_sub(sample.loop_length)
                .with_context(|| format!("loop waveform exceeds instrument {}", sample.path))?;
            let rate = spec
                .sample_rate
                .try_into()
                .context("unsupported instrument sample rate")?;
            // Cache identity comes from captured bytes and editable tuning/loop
            // settings. It is never a digest maintained by the author.
            let key = serde_json::to_string(&(
                format!("{:x}", Sha256::digest(&bytes)),
                sample.key,
                sample.loop_start,
                sample.loop_length,
            ))?;
            let decoded = if let Some(sample) = cache.0.get(&key).and_then(std::sync::Weak::upgrade)
            {
                sample
            } else {
                let mut pcm = wave
                    .samples::<i16>()
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                ensure!(
                    pcm.len() == wave.duration() as usize,
                    "truncated instrument WAV"
                );
                let loop_pcm = pcm.split_off(first_frames as usize);
                let decoded = Arc::new(super::Sample {
                    key: sample.key,
                    rate,
                    loop_start: sample.loop_start,
                    loop_length: sample.loop_length,
                    pcm,
                    loop_pcm,
                });
                cache.0.insert(key, Arc::downgrade(&decoded));
                decoded
            };
            samples.insert(id, decoded);
        }
        Loaded::new(
            Resources {
                programs: self.programs,
                samples,
            },
            self.score,
            self.tables,
            reverbs,
        )
    }
}
