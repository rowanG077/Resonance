//! Verify a selected arena's complete dependency closure before activation.
use anyhow::{Result, ensure};
use resonance_content::{
    battle_stage::{self, Stage},
    prepared::Files,
};
use std::collections::BTreeMap;

/// Prepare a named arena independently. Its capture operation also accepts a
/// caller-owned snapshot for the full encounter preparation boundary.
pub fn prepare_named(
    library: &resonance_content::library::Library,
    arena: u16,

    diagnostics: resonance_content::diagnostics::Diagnostics,
    cancelled: impl Fn() -> bool,
) -> Result<(Files, Stage)> {
    let mut snapshot = library.snapshot();
    let stage = battle_stage::document::capture(&mut snapshot, arena, &diagnostics, &cancelled)?;
    if let Some(reason) = &stage.unsupported_reason {
        anyhow::bail!("battle stage {arena} presentation is not prepared: {reason}");
    }
    let files = Files::from_snapshot(snapshot, cancelled, diagnostics)?;
    Model::load(&files, &stage)?;
    Ok((files, stage))
}

/// Scene-owned animation uses the same indexed, affine-capable poses as actors.
pub struct Model {
    pub skeleton: resonance_content::animation::Skeleton,
    motion: Option<resonance_content::animation::Motion>,
    rate: f32,
}
impl Model {
    pub fn load(files: &Files, stage: &Stage) -> Result<BTreeMap<u8, Self>> {
        stage
            .layers
            .iter()
            .map(|(&slot, layer)| {
                let skeleton = resonance_content::animation::Skeleton::from_glb(
                    &files.read(&layer.scene.mesh)?,
                )?;
                ensure!(
                    skeleton
                        .bones
                        .iter()
                        .map(|b| &b.name)
                        .eq(layer.scene.bone_names.iter()),
                    "stage bone indices differ from its mesh"
                );
                let motion = layer
                    .selected_clip()?
                    .map(|(_, clip)| {
                        let motion =
                            clip.decode_for_skeleton(&files.read(&clip.motion)?, &skeleton)?;
                        motion.validate(&skeleton)?;
                        Ok::<_, anyhow::Error>(motion)
                    })
                    .transpose()?;
                Ok((
                    slot,
                    Self {
                        skeleton,
                        motion,
                        rate: stage.animation_rates.get(&slot).copied().unwrap_or(0.),
                    },
                ))
            })
            .collect()
    }
    pub fn pose(&self, tick: u64) -> Result<Vec<resonance_content::animation::Matrix>> {
        let Some(motion) = &self.motion else {
            return Ok(self.skeleton.bind_pose()?.global);
        };
        let time = if motion.duration_frames > 0. {
            (tick as f32 * self.rate).rem_euclid(motion.duration_frames)
        } else {
            0.
        };
        let matrices = resonance_content::animation::pose::sample(&self.skeleton, motion, time)?
            .iter()
            .map(|bone| bone.matrix())
            .collect::<Vec<_>>();
        Ok(self
            .skeleton
            .pose_with_matrices(
                self.skeleton.bones.iter().map(|bone| bone.bind).collect(),
                &matrices,
            )?
            .global)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Context;

    #[test]
    #[ignore = "requires RESONANCE_TEST_ASSETS; samples existing arena GLBs on the CPU"]
    fn installed_arenas_sample_every_named_layer_and_attachment() -> Result<()> {
        let root =
            std::env::var_os("RESONANCE_TEST_ASSETS").context("set RESONANCE_TEST_ASSETS")?;
        let library = resonance_content::library::Library::open(std::path::Path::new(&root), &[])?;
        let mappings = library
            .snapshot()
            .mappings(battle_stage::document::MAPPINGS)?
            .clone();
        let diagnostics = resonance_content::diagnostics::Diagnostics::new(true);
        let mut count = 0;
        let mut joints = 0;
        for (id, _) in mappings.iter() {
            let mut snapshot = library.snapshot();
            let stage =
                battle_stage::document::capture(&mut snapshot, id as u16, &diagnostics, &|| false)?;
            let files = Files::from_snapshot(snapshot, || false, diagnostics.clone())?;
            let models = Model::load(&files, &stage)?;
            for tick in [0, 1, 120] {
                let poses = models
                    .iter()
                    .map(|(&slot, model)| Ok((slot, model.pose(tick)?)))
                    .collect::<Result<BTreeMap<_, _>>>()?;
                for (&slot, pose) in &poses {
                    ensure!(
                        pose.iter().flatten().flatten().all(|v| v.is_finite()),
                        "nonfinite arena pose"
                    );
                    assert_eq!(pose.len(), stage.layers[&slot].scene.bone_names.len());
                }
                for (&slot, &joint) in &stage.scenery_bones {
                    assert!(poses.contains_key(&slot));
                    assert!(poses[&0].get(usize::from(joint)).is_some());
                }
            }
            count += models.len();
            joints += stage.scenery_bones.len();
        }
        assert_eq!((mappings.iter().count(), count, joints), (86, 415, 305));
        eprintln!(
            "Sampled all {count} arena layers and {joints} attachments at three playback times."
        );
        Ok(())
    }
}
