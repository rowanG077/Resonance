//! Rig-space authoring helpers used only while preparing finisher curves.
use super::*;
use anyhow::{Context, Result, ensure};
use glam::{Mat4, Quat, Vec3};
use resonance_content::animation::{Skeleton, Transform};

pub(super) fn ease(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * t * (t * (t * 6. - 15.) + 10.)
}

pub(super) fn joint(skeleton: &Skeleton, name: &str) -> Result<usize> {
    skeleton
        .bone(name)
        .map(usize::from)
        .with_context(|| format!("Fatal Strike rig is missing {name}"))
}

pub(super) fn globals(skeleton: &Skeleton, local: &[Transform]) -> Result<Vec<Mat4>> {
    Ok(skeleton
        .pose(local.to_vec())?
        .global
        .iter()
        .map(Mat4::from_cols_array_2d)
        .collect())
}

pub(super) fn turn(
    skeleton: &Skeleton,
    local: &mut [Transform],
    bone: usize,
    delta: Quat,
) -> Result<()> {
    let global = globals(skeleton, local)?;
    let parent = skeleton.bones[bone]
        .parent
        .map_or(Mat4::IDENTITY, |p| global[usize::from(p)])
        * skeleton.bones[bone]
            .parent_transform
            .map_or(Mat4::IDENTITY, |m| Mat4::from_cols_array_2d(&m));
    let (_, parent, _) = parent.to_scale_rotation_translation();
    local[bone].rotation =
        (parent.inverse() * delta * parent * Quat::from_array(local[bone].rotation))
            .normalize()
            .to_array();
    Ok(())
}

/// Analytic two-link reach preserves bone lengths, bends toward the authored
/// pole, and softens full extension instead of snapping an elbow straight.
pub(super) fn reach(
    skeleton: &Skeleton,
    local: &mut [Transform],
    chain: [usize; 3],
    target: Vec3,
    pole: Vec3,
) -> Result<()> {
    let [upper, lower, end] = chain;
    let global = globals(skeleton, local)?;
    let [a, b, c] = chain.map(|i| global[i].transform_point3(Vec3::ZERO));
    let first = a.distance(b);
    let second = b.distance(c);
    ensure!(
        first > 0.01 && second > 0.01,
        "degenerate Fatal Strike limb"
    );
    let toward = target - a;
    let extension = if skeleton.bones[upper].name.starts_with("leg-") {
        0.9999
    } else {
        0.985
    };
    let distance = toward
        .length()
        .clamp((first - second).abs() + 0.01, (first + second) * extension);
    let axis = toward.normalize_or_zero();
    let bend = (pole - axis * pole.dot(axis)).normalize_or_zero();
    let along = (first * first - second * second + distance * distance) / (2. * distance);
    let elbow = a + axis * along + bend * (first * first - along * along).max(0.).sqrt();
    turn(
        skeleton,
        local,
        upper,
        Quat::from_rotation_arc((b - a).normalize(), (elbow - a).normalize()),
    )?;
    let global = globals(skeleton, local)?;
    let b = global[lower].transform_point3(Vec3::ZERO);
    let c = global[end].transform_point3(Vec3::ZERO);
    turn(
        skeleton,
        local,
        lower,
        Quat::from_rotation_arc((c - b).normalize(), (a + axis * distance - b).normalize()),
    )
}

pub(super) fn sample(model: &ModelDefinition, clip: u16, fraction: f32) -> Result<Vec<Transform>> {
    let motion = model
        .motions
        .get(&clip)
        .context("missing caster source motion")?;
    Ok(model
        .skeleton
        .sample(motion, motion.duration_frames * fraction.clamp(0., 1.))?
        .local)
}
