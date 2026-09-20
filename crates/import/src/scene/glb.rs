//! The single-buffer GLB layout emitted by the scene cooker.
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::{fs, path::Path};

pub(crate) struct Glb {
    pub json: Value,
    pub binary: Vec<u8>,
}

impl Glb {
    pub(crate) fn read(path: &Path) -> Result<Self> {
        Self::parse(&fs::read(path)?).with_context(|| format!("reading {}", path.display()))
    }

    pub(crate) fn parse(bytes: &[u8]) -> Result<Self> {
        let word = |at| -> Result<usize> {
            Ok(
                u32::from_le_bytes(bytes.get(at..at + 4).context("truncated GLB")?.try_into()?)
                    as usize,
            )
        };
        ensure!(
            word(0)? == 0x46546c67 && word(4)? == 2 && word(8)? == bytes.len(),
            "invalid cooked GLB header"
        );
        ensure!(word(16)? == 0x4e4f534a, "missing cooked GLB JSON chunk");
        let binary = 20 + word(12)?;
        ensure!(
            word(binary + 4)? == 0x004e4942 && binary + 8 + word(binary)? == bytes.len(),
            "invalid cooked GLB binary chunk"
        );
        let json: Value = serde_json::from_slice(&bytes[20..binary])?;
        ensure!(
            json["buffers"]
                .as_array()
                .is_some_and(|buffers| buffers.len() == 1)
                && json["buffers"][0]["uri"].is_null()
                && json["buffers"][0]["byteLength"]
                    .as_u64()
                    .context("expected a cooked GLB integer")?
                    <= (bytes.len() - binary - 8) as u64,
            "expected one embedded cooked GLB buffer"
        );
        Ok(Self {
            json,
            binary: bytes[binary + 8..].to_vec(),
        })
    }
}

#[cfg(test)]
pub(crate) type AnimationSample = (serde_json::Value, Vec<f32>, Vec<f32>);

#[cfg(test)]
pub(crate) fn animation_samples(
    glb: &crate::scene::glb::Glb,
    index: usize,
) -> Result<Vec<AnimationSample>> {
    use serde_json::json;
    let animation = &glb.json["animations"][index];
    let accessor = |index: &serde_json::Value| -> Result<_> {
        let record = &glb.json["accessors"][index.as_u64().context("accessor index")? as usize];
        let view = &glb.json["bufferViews"]
            [record["bufferView"].as_u64().context("buffer view")? as usize];
        let start = view["byteOffset"].as_u64().unwrap_or(0) as usize
            + record["byteOffset"].as_u64().unwrap_or(0) as usize;
        ensure!(record["componentType"] == 5126, "expected float animation");
        let width = match record["type"].as_str() {
            Some("SCALAR") => 4,
            Some("VEC3") => 12,
            Some("VEC4") => 16,
            _ => anyhow::bail!("unexpected animation accessor"),
        };
        let stride = view["byteStride"].as_u64().unwrap_or(width as u64) as usize;
        let mut bytes = Vec::new();
        for row in 0..record["count"].as_u64().context("accessor count")? as usize {
            bytes.extend_from_slice(
                glb.binary
                    .get(start + row * stride..start + row * stride + width)
                    .context("animation bounds")?,
            );
        }
        Ok(bytes
            .chunks_exact(4)
            .map(|bytes| f32::from_le_bytes(bytes.try_into().unwrap()))
            .collect::<Vec<_>>())
    };
    animation["channels"]
        .as_array()
        .context("animation channels")?
        .iter()
        .map(|channel| {
            let sampler =
                &animation["samplers"][channel["sampler"].as_u64().context("sampler")? as usize];
            Ok((
                json!({"target":channel["target"],"interpolation":sampler["interpolation"]}),
                accessor(&sampler["input"])?,
                accessor(&sampler["output"])?,
            ))
        })
        .collect::<Result<Vec<_>>>()
}
