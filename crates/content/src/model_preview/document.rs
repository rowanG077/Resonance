//! Named catalogue models. Menu records contain a direct path to model.json;
//! geometry, textures and motions are prepared only when the viewer needs them.
use super::ModelPreview;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

pub const VERSION: u32 = 1;
pub const TOON_RAMP: &str = "effects/field/textures/toon-ramp.png";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Definition {
    pub version: u32,
    #[serde(flatten)]
    pub model: ModelPreview,
}

pub fn read(path: &str, mut read: impl FnMut(&str) -> Result<Arc<[u8]>>) -> Result<ModelPreview> {
    crate::library::validate_path(path)?;
    let definition: Definition = serde_json::from_slice(&read(path)?)
        .with_context(|| format!("decode model document {path}"))?;
    ensure!(
        definition.version == VERSION,
        "unsupported model document version in {path}"
    );
    let mut model = definition.model;
    model.document = Some(path.into());
    Ok(model)
}

/// Decode the small named records into the existing typed runtime catalogue.
/// Return origins separately because they are preparation metadata, not JSON.
pub fn expand_catalogue(
    mut value: Value,
    mut read_file: impl FnMut(&str) -> Result<Arc<[u8]>>,
) -> Result<(Value, Vec<String>)> {
    let records = value
        .get_mut("records")
        .and_then(Value::as_array_mut)
        .context("model catalogue needs records")?;
    let mut paths = Vec::with_capacity(records.len());
    for record in records {
        let path = record
            .get("preview")
            .and_then(Value::as_str)
            .context("named catalogue preview must be a model.json path")?
            .to_owned();
        record["preview"] = serde_json::to_value(read(&path, &mut read_file)?)?;
        paths.push(path);
    }
    Ok((value, paths))
}

/// Capture referenced descriptors without loading every catalogue mesh/texture.
pub fn capture_catalogue(
    value: &Value,
    snapshot: &mut crate::library::Snapshot,
    cancelled: &impl Fn() -> bool,
) -> Result<()> {
    let records = value
        .get("records")
        .and_then(Value::as_array)
        .context("model catalogue needs records")?;
    for record in records {
        ensure!(!cancelled(), "catalogue preparation cancelled");
        let path = record
            .get("preview")
            .and_then(Value::as_str)
            .context("named catalogue preview must be a model.json path")?;
        snapshot
            .read(path, crate::preparation::JSON_LIMIT)
            .with_context(|| format!("catalogue preview refers to {path}"))?;
    }
    Ok(())
}

/// Current motion data supplies duration; bone/cloth relationships remain explicit.
pub fn prepare(
    model: &mut ModelPreview,
    mut read: impl FnMut(&str) -> Result<Arc<[u8]>>,
) -> Result<()> {
    for part in &mut model.parts {
        read(&part.scene.mesh)?;
        for texture in &part.scene.textures {
            read(texture)?;
        }
        for clip in &mut part.scene.clips {
            let motion = crate::animation::Motion::decode(&read(&clip.motion)?)
                .with_context(|| format!("decode preview motion {}", clip.motion))?;
            motion.validate_bones(part.scene.bone_names.len())?;
            clip.duration_seconds = motion.duration_frames / crate::animation::FRAME_HZ;
        }
    }
    model.validate()
}
