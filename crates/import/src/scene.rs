use anyhow::{Result, ensure};
use resonance_content::CameraKey;
pub(crate) mod binding;
pub(crate) mod decoded;
pub(crate) mod glb;
mod projection;
pub(crate) mod source;
pub(crate) mod title;

pub(crate) fn camera(track: crate::all_assets::CameraTrack) -> Result<Vec<CameraKey>> {
    ensure!(
        track.transforms.len() >= 2 && track.transforms.len() == track.targets.len(),
        "scene camera needs matching position and target tracks"
    );
    track
        .transforms
        .into_iter()
        .zip(track.targets)
        .map(|(position, target)| {
            ensure!(
                position.time == target.time,
                "camera tracks have different times"
            );
            Ok(CameraKey {
                time: position.time,
                position: position.position,
                target: target.position,
            })
        })
        .collect()
}
