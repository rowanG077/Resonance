//! Validated playback data, current editable programs and decoded sample sharing.
use crate::{
    data::{Command, Resources, Score},
    music_voice::Tables,
    sample::Sample,
};
use anyhow::{Result, ensure};
use std::{
    collections::BTreeMap,
    path::{Component, Path},
};

pub mod editable;

#[cfg(test)]
pub(crate) mod tests;

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

/// Reuses decoding when programs share the same captured WAV buffer.
#[derive(Default)]
pub struct SampleCache(
    BTreeMap<(usize, u8, u32, u32), (std::sync::Weak<[u8]>, std::sync::Weak<Sample>)>,
);

impl SampleCache {
    pub fn prune(&mut self) {
        self.0
            .retain(|_, (bytes, sample)| bytes.strong_count() != 0 && sample.strong_count() != 0);
    }
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
