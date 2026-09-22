//! The single-buffer GLB layout emitted by the scene cooker.
use anyhow::Result;
#[cfg(test)]
use anyhow::{Context, ensure};
use resonance_content::animation::Motion;
use serde_json::Value;
use std::sync::Arc;
#[cfg(test)]
use std::{fs, path::Path};

#[derive(Clone)]
pub(crate) struct MotionAsset {
    pub path: String,
    pub bytes: Arc<[u8]>,
}

#[derive(Clone)]
pub(crate) struct Glb {
    pub json: Value,
    pub binary: Arc<Vec<u8>>,
    pub motions: Vec<MotionAsset>,
}

impl Glb {
    pub(crate) fn animate(&mut self, motion: Motion) -> Result<resonance_content::SceneClip> {
        let bytes = motion.encode()?;
        let path = format!("clips/{}.motion", crate::digest(&bytes));
        let clip = resonance_content::SceneClip {
            motion: path.clone(),
            resource_slot: 0,
            duration_seconds: motion.duration_frames / resonance_content::animation::FRAME_HZ,
            animation_resource: None,
            secondary_pose_nodes: motion
                .tracks
                .iter()
                .filter(|track| track.times.len() > 2)
                .map(|track| track.bone)
                .collect(),
        };
        self.motions.push(MotionAsset {
            path,
            bytes: bytes.into(),
        });
        Ok(clip)
    }

    #[cfg(test)]
    pub(crate) fn read(path: &Path) -> Result<Self> {
        Self::parse(&fs::read(path)?).with_context(|| format!("reading {}", path.display()))
    }

    #[cfg(test)]
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
            binary: Arc::new(bytes[binary + 8..].to_vec()),
            motions: Vec::new(),
        })
    }
}
