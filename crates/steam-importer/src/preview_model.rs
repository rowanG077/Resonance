//! Catalogue bodies and selected idle clips use the shared model writer.
use crate::{actor_model, animation::Animation, legacy_motion, model, output, source::Source};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    SceneClip, ScenePart,
    animation::Skeleton,
    model_preview::{ModelPreview, PreviewPart},
};
use std::{fs, path::Path};

pub(crate) fn part(
    source: &Source,
    library: &Path,
    descriptor: &str,
    owner: &str,
    label: &str,
    motions: &[(u16, String)],
) -> Result<(ScenePart, model::Model)> {
    part_on_rig(source, library, descriptor, owner, label, motions, None)
}

pub(crate) fn part_on_rig(
    source: &Source,
    library: &Path,
    descriptor: &str,
    owner: &str,
    label: &str,
    motions: &[(u16, String)],
    rig: Option<&Skeleton>,
) -> Result<(ScenePart, model::Model)> {
    let (mut scene, native) = model::cook_on_rig(source, library, descriptor, owner, label, rig)?;
    let bytes = fs::read(library.join(&scene.mesh))?;
    let skeleton = Skeleton::from_glb(&bytes)?;
    for (slot, name) in motions {
        let source_bytes = source.require(name)?;
        let motion = if name.to_ascii_lowercase().ends_with(".toanmb") {
            Animation::read(&source_bytes)?
                .bind(&native.rig, &skeleton)?
                .encode(&skeleton)?
        } else {
            legacy_motion::convert(
                &source_bytes,
                &bytes,
                &native
                    .rig
                    .nodes
                    .iter()
                    .map(|n| n.name.clone())
                    .collect::<Vec<_>>(),
            )?
        };
        let path = format!("{owner}/animations/{label}-{slot}.glb");
        output::write(&library.join(&path), &motion)?;
        scene.clips.push(SceneClip {
            motion: path,
            resource_slot: *slot,
            joints: Default::default(),
            duration_seconds: 0.,
            animation_resource: None,
            pose_chains: Vec::new(),
        });
    }
    Ok((scene, native))
}

pub(crate) fn cook(
    source: &Source,
    library: &Path,
    bodies: [&str; 2],
    owner: &str,
    motions: &[(u16, String)],
) -> Result<(ModelPreview, model::Model)> {
    let (body, native) = part(source, library, bodies[0], owner, "body", motions)?;
    let mut outline = if bodies[1].is_empty() {
        let mut part = body.clone();
        part.mesh = format!("{owner}/meshes/outline.glb");
        output::write(&library.join(&part.mesh), &native.outline_glb()?)?;
        part
    } else {
        part(source, library, bodies[1], owner, "outline", motions)?.0
    };
    actor_model::style_outline(&mut outline);
    let hidden_geometry = native
        .rig
        .nodes
        .iter()
        .zip(native.rig.joint_names())
        .filter_map(|(node, name)| {
            (!node.visible || name.to_ascii_lowercase().starts_with("kk")).then_some(name)
        })
        .collect();
    let parts = [body, outline]
        .into_iter()
        .map(|mut scene| {
            scene.autoplay = !scene.clips.is_empty();
            PreviewPart {
                animation: scene.clips.first().map(|clip| clip.resource_slot),
                scene,
                attached_to: None,
                additive: false,
                uv_offsets: Vec::new(),
            }
        })
        .collect();
    let preview = ModelPreview {
        document: None,
        scale: 1.,
        elevation: 0.,
        parts,
        hidden_geometry,
        behavior: None,
    };
    preview.validate_definition()?;
    Ok((preview, native))
}

/// Enemy package resources use absolute little-endian offsets to filenames.
pub(crate) fn reference(bytes: &[u8], at: usize) -> Result<Option<String>> {
    let offset = crate::pe::word(bytes, at)? as usize;
    if offset == 0 {
        return Ok(None);
    }
    let name = bytes
        .get(offset..)
        .context("enemy resource exceeds package")?;
    let end = name
        .iter()
        .take(260)
        .position(|&b| b == 0)
        .context("unterminated enemy filename")?;
    let name = std::str::from_utf8(&name[..end])?;
    crate::source::validate_name(name)?;
    ensure!(!name.is_empty(), "empty enemy filename");
    Ok(Some(name.to_owned()))
}

pub(crate) fn member(bytes: &[u8], at: usize) -> Result<Option<&[u8]>> {
    let start = crate::pe::word(bytes, at)? as usize;
    if start == 0 {
        return Ok(None);
    }
    ensure!(
        start >= 0x1f8 && start < bytes.len(),
        "enemy member exceeds package"
    );
    let mut end = bytes.len();
    for at in (0x18..0x1f8).step_by(4) {
        let offset = crate::pe::word(bytes, at)? as usize;
        if offset > start && offset < end {
            end = offset;
        }
    }
    for at in [4, 6] {
        let offset = usize::from(crate::pe::half(bytes, at)?);
        if offset > start && offset < end {
            end = offset;
        }
    }
    Ok(Some(&bytes[start..end]))
}
