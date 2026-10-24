//! Verify a selected arena's complete dependency closure before activation.
use anyhow::{Result, ensure};
use resonance_content::{
    battle_stage::{self, Stage},
    prepared::{Cache, Files},
};
use std::collections::BTreeMap;

/// Prepare a named arena independently. Its capture operation also accepts a
/// caller-owned snapshot for the full encounter preparation boundary.
pub fn prepare_named(
    library: &resonance_content::library::Library,
    arena: u16,
    cache: &mut Cache,
    diagnostics: resonance_content::diagnostics::Diagnostics,
    cancelled: impl Fn() -> bool,
) -> Result<(Files, Stage)> {
    let mut snapshot = library.snapshot();
    let stage = battle_stage::document::capture(&mut snapshot, arena, &diagnostics, &cancelled)?;
    if let Some(reason) = &stage.unsupported_reason {
        anyhow::bail!("battle stage {arena} presentation is not prepared: {reason}");
    }
    let files = Files::from_snapshot(snapshot, cache, cancelled, diagnostics)?;
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
                        let motion = resonance_content::animation::Motion::decode(
                            &files.read(&clip.motion)?,
                        )?;
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
