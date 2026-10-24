//! Editable skit catalogue, scenarios and audio clocks.
use super::*;
use crate::{
    field_audio::Voice,
    library::{Mappings, Snapshot, validate_path},
};
use anyhow::Context;
use std::sync::Arc;

pub const PATH: &str = "skits/catalogue.json";
pub const MAPPINGS: &str = "skits/mappings.json";
pub const SPEECH_MAPPINGS: &str = "audio/speech/skits/mappings.json";
pub const VERSION: u32 = 1;
pub type Definition = SkitCatalog<Media>;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Media {
    /// Silent native tracks carry timing, not an unused file or recorded hash.
    Silence { frames: u32, sample_rate: u32 },
    Speech {
        /// Omit to follow the current WAV header; retain intentional caller clocks.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sample_rate: Option<u32>,
    },
}

/// Prepare using one captured reader. The same operation serves snapshot
/// capture and later resident consumers, without reopening anything on disk.
pub fn read(mut read: impl FnMut(&str, usize) -> Result<Arc<[u8]>>) -> Result<SkitCatalog> {
    let mut definition: Definition = serde_json::from_slice(&read(PATH, 64 * 1024 * 1024)?)?;
    ensure!(
        definition.version == VERSION && definition.resources.is_empty(),
        "skit resources must use skits/mappings.json in layout version {VERSION}"
    );
    let mappings: Mappings = serde_json::from_slice(&read(MAPPINGS, 16 * 1024 * 1024)?)?;
    let speech: Mappings = serde_json::from_slice(&read(SPEECH_MAPPINGS, 16 * 1024 * 1024)?)?;
    for (id, _) in mappings.iter() {
        let path = mappings.resolve(MAPPINGS, id, Some("skit.json"))?;
        let resource: SkitResourcePaths = serde_json::from_slice(&read(&path, 1024 * 1024)?)
            .with_context(|| format!("decode skit {id}: {path}"))?;
        for path in [&resource.script, &resource.messages] {
            validate_path(path)?;
            read(path, 64 * 1024 * 1024).with_context(|| format!("skit {id} refers to {path}"))?;
        }
        definition.resources.insert(u16::try_from(id)?, resource);
    }
    for portrait in definition.portraits.values() {
        for image in &portrait.images {
            validate_path(&image.texture)?;
            read(&image.texture, 64 * 1024 * 1024)
                .with_context(|| format!("skit portrait refers to {}", image.texture))?;
        }
    }
    let mut result = definition.map_media(|id, media| match media {
        Media::Silence {
            frames,
            sample_rate,
        } => Ok(SkitMedia {
            voice: None,
            frames,
            sample_rate,
        }),
        Media::Speech { sample_rate } => {
            let path = speech.resolve(SPEECH_MAPPINGS, id, None)?;
            validate_path(&path)?;
            let bytes = read(&path, 129 * 1024 * 1024)
                .with_context(|| format!("skit speech {id}: {path}"))?;
            let voice = Voice::from_wave(&path, sample_rate, &bytes)
                .with_context(|| format!("decode skit speech {id}: {path}"))?;
            Ok(SkitMedia {
                frames: voice.frames,
                sample_rate: voice.sample_rate,
                voice: Some(voice),
            })
        }
    })?;
    result.version = super::VERSION;
    result.validate()?;
    Ok(result)
}

pub fn capture(snapshot: &mut Snapshot, cancelled: &impl Fn() -> bool) -> Result<SkitCatalog> {
    read(|path, limit| {
        ensure!(!cancelled(), "skit preparation cancelled");
        snapshot.read(path, limit)
    })
}
