//! Editable skit catalogue, scenarios and audio clocks.
use super::*;
use crate::{
    field_audio::{Voice, document::SPEECH, speech},
    library::{Mappings, Snapshot, validate_path},
};
use anyhow::Context;
use std::{collections::BTreeSet, sync::Arc};

pub const PATH: &str = "skits/catalogue.json";
pub const MAPPINGS: &str = "skits/mappings.json";
pub mod portraits;
#[cfg(test)]
mod tests;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub skits: Vec<SkitDefinition>,
    pub preview_order: Vec<u16>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub resources: BTreeMap<u16, SkitResourcePaths>,
    #[serde(default, skip_serializing_if = "Media::is_empty")]
    pub media: Media,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Media {
    pub ids: BTreeSet<u32>,
    /// Playback multiplier for both WAVs and silence clocks in the speech mapping.
    #[serde(default = "normal_speed", skip_serializing_if = "is_normal_speed")]
    pub speed: f64,
}

fn normal_speed() -> f64 {
    1.0
}

fn is_normal_speed(speed: &f64) -> bool {
    *speed == 1.0
}

impl Default for Media {
    fn default() -> Self {
        Self {
            ids: BTreeSet::new(),
            speed: normal_speed(),
        }
    }
}

impl Media {
    fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    fn playback_rate(&self, source_rate: u32) -> Result<u32> {
        let rate = (f64::from(source_rate) * self.speed).floor();
        ensure!(
            rate.is_finite() && (8000.0..=96000.0).contains(&rate),
            "skit media speed produces an invalid playback rate"
        );
        Ok(rate as u32)
    }

    fn resolve(
        &self,
        read: &mut impl FnMut(&str, usize) -> Result<Option<Arc<[u8]>>>,
    ) -> Result<BTreeMap<u32, SkitMedia>> {
        ensure!(
            self.speed.is_finite() && self.speed > 0.0,
            "invalid skit media speed"
        );
        if self.ids.is_empty() {
            return Ok(BTreeMap::new());
        }
        let speech: speech::Mappings =
            serde_json::from_slice(&required(read, SPEECH, 16 * 1024 * 1024)?)?;
        self.ids
            .iter()
            .map(|&id| {
                let source = speech
                    .get(&id)
                    .with_context(|| format!("missing skit speech {id} in {SPEECH}"))?
                    .resolve()?;
                let media = match source {
                    speech::Source::Silence { silence, .. } => SkitMedia {
                        voice: None,
                        frames: silence.frames,
                        sample_rate: self.playback_rate(silence.sample_rate)?,
                    },
                    speech::Source::Wave(path) => {
                        let bytes = required(read, &path, 129 * 1024 * 1024)
                            .with_context(|| format!("skit speech {id}: {path}"))?;
                        let mut voice = Voice::from_wave(&path, None, &bytes)
                            .with_context(|| format!("decode skit speech {id}: {path}"))?;
                        voice.sample_rate = self.playback_rate(voice.source_sample_rate)?;
                        SkitMedia {
                            frames: voice.frames,
                            sample_rate: voice.sample_rate,
                            voice: Some(voice),
                        }
                    }
                };
                Ok((id, media))
            })
            .collect()
    }
}

/// Prepare using one captured reader. The same operation serves snapshot
/// capture and later resident consumers, without reopening anything on disk.
pub fn read(mut read: impl FnMut(&str, usize) -> Result<Option<Arc<[u8]>>>) -> Result<SkitCatalog> {
    let mut definition: Definition =
        serde_json::from_slice(&required(&mut read, PATH, 64 * 1024 * 1024)?)?;
    let fields: Mappings = serde_json::from_slice(&required(
        &mut read,
        crate::library::FIELDS,
        16 * 1024 * 1024,
    )?)?;
    super::fields::prepare_fields(&mut definition.skits, &fields)?;

    let mappings: Mappings =
        serde_json::from_slice(&required(&mut read, MAPPINGS, 16 * 1024 * 1024)?)?;
    let mut options = std::mem::take(&mut definition.resources);
    for (id, _) in mappings.iter() {
        let root = mappings.resolve(MAPPINGS, id, None)?;
        let mut resource = options
            .remove(&u16::try_from(id)?)
            .unwrap_or(SkitResourcePaths {
                script: String::new(),
                messages: None,
                allow_skip: true,
                title: None,
            });
        ensure!(
            resource.script.is_empty() && resource.messages.is_none(),
            "skit paths are determined by skits/mappings.json"
        );
        resource.script = format!("{root}/scenario.ssb");
        validate_path(&resource.script)?;
        required(&mut read, &resource.script, 64 * 1024 * 1024)
            .with_context(|| format!("skit {id} refers to {}", resource.script))?;
        let messages = format!("{root}/messages.json");
        validate_path(&messages)?;
        resource.messages = read(&messages, 64 * 1024 * 1024)?.map(|_| messages);
        definition.resources.insert(u16::try_from(id)?, resource);
    }
    ensure!(
        options.is_empty(),
        "skit options refer to unmapped resources"
    );
    let portrait_document: portraits::Portraits =
        serde_json::from_slice(&required(&mut read, portraits::PATH, 16 * 1024 * 1024)?)?;
    let (portraits, portrait_recipes) = portrait_document.resolve(&mut read)?;
    let result = SkitCatalog {
        skits: definition.skits,
        preview_order: definition.preview_order,
        resources: definition.resources,
        media: definition.media.resolve(&mut read)?,
        portraits,
        portrait_recipes,
    };
    result.validate()?;
    Ok(result)
}

fn required(
    read: &mut impl FnMut(&str, usize) -> Result<Option<Arc<[u8]>>>,
    path: &str,
    limit: usize,
) -> Result<Arc<[u8]>> {
    read(path, limit)?.with_context(|| format!("missing skit resource {path}"))
}

pub fn capture(snapshot: &mut Snapshot, cancelled: &impl Fn() -> bool) -> Result<SkitCatalog> {
    read(|path, limit| {
        ensure!(!cancelled(), "skit preparation cancelled");
        snapshot.read_optional(path, limit)
    })
}
