//! Shared script-selected banks bound directly to native scenery joints.
use crate::{dol, field_bank_bindings, field_core, shared_animations, world_gpl, world_motion};
use anyhow::{Context, Result, ensure};
use encoding_rs::SHIFT_JIS;
use resonance_content::{SceneClip, ScenePart};
use std::{collections::BTreeMap, path::Path};

/// Append each selected bank using native name matching when names overlap,
/// with original joint IDs as the fallback for unrelated or unnamed rigs.
pub(crate) fn append_shared_scenery_clips(
    extracted: &Path,
    library: &Path,
    field: u32,
    archive: &[u8],
    parts: &mut [ScenePart],
    banks: &BTreeMap<u32, String>,
) -> Result<()> {
    let mut sources = Vec::new();
    for (&bank, name) in banks {
        let motion = shared_animations::mapped_motion_path(library, bank)?;
        ensure!(
            library.join(&motion).is_file(),
            "field scenery needs {motion}"
        );
        let native = shared_animations::read_bank(extracted, Some(field), bank)?;
        let tracks = shared_animations::track_names(extracted, library, &native, name)
            .with_context(|| format!("shared scenery bank {bank} ({name})"))?;
        let descriptors = field_bank_bindings::source_tracks(&native)?;
        sources.push((bank, motion, tracks, descriptors));
    }
    for part in parts {
        let native = field_core::optional_section(archive, usize::from(part.resource))?
            .with_context(|| format!("field scenery model {} is missing", part.resource))?;
        let targets = world_gpl::named_animation_bindings(native)
            .with_context(|| format!("scenery model {} bindings", part.resource))?;
        for (bank, motion, tracks, descriptors) in &sources {
            let selections = field_bank_bindings::selected_tracks(&targets, descriptors);
            let joints = targets
                .iter()
                .zip(selections)
                .filter_map(|((_, target, _), selected)| {
                    let source = selected.and_then(|index| tracks.get(&descriptors[index].id));
                    if source.map(String::as_str)
                        == tracks
                            .values()
                            .any(|name| name == target)
                            .then_some(target.as_str())
                    {
                        None
                    } else {
                        Some((target.clone(), source.cloned()))
                    }
                })
                .collect();
            part.clips.push(SceneClip {
                motion: motion.clone(),
                joints,
                resource_slot: 12,
                animation_resource: Some(*bank),
                duration_seconds: 0.,
                pose_chains: Vec::new(),
            });
        }
    }
    Ok(())
}

/// The field archive itself supplies these banks. Their member position is the
/// script resource ID, while native joint IDs connect them to scenery parts.
pub(crate) fn append_local_scenery_clips(
    archive: &[u8],
    root: &str,
    parts: &mut [ScenePart],
) -> Result<()> {
    let section_count = dol::u32(archive, 0)? as usize;
    let mut motions = Vec::new();
    for section in 16..section_count {
        let Some(motion) = field_core::optional_section(archive, section)? else {
            continue;
        };
        if motion.len() < 36 || dol::u32(motion, 0)? != 0x007b_7960 {
            continue;
        }
        motions.push((
            section - 16,
            world_motion::indexed_joints(motion)?,
            local_source_names(motion)?,
            world_motion::indexed_track_names(motion)?,
        ));
    }
    for part in parts {
        let native = field_core::optional_section(archive, usize::from(part.resource))?
            .with_context(|| format!("field scenery model {} is missing", part.resource))?;
        let targets = world_gpl::named_animation_bindings(native)?;
        for (index, joints, names, labels) in &motions {
            // The native animation chooses name binding for this model if any
            // track name occurs in its rig. Otherwise its numeric IDs apply.
            let by_name = names.as_ref().is_some_and(|names| {
                names
                    .iter()
                    .any(|(_, source)| targets.iter().any(|(_, _, target)| target == source))
            });
            let map = targets
                .iter()
                .filter_map(|(id, target, raw)| {
                    let source = if by_name {
                        names.as_ref()?.iter().find(|(_, name)| name == raw)?.0
                    } else {
                        joints.binary_search(id).ok()?;
                        *id
                    };
                    Some((target.clone(), Some(labels.get(&source)?.clone())))
                })
                .collect();
            part.clips.push(SceneClip {
                motion: format!("{root}/animations/local-motion-{index}.glb"),
                joints: map,
                resource_slot: 12,
                animation_resource: Some(0xffee_0000 + *index as u32),
                duration_seconds: 0.,
                pose_chains: Vec::new(),
            });
        }
    }
    Ok(())
}

fn local_source_names(motion: &[u8]) -> Result<Option<Vec<(u16, String)>>> {
    if dol::u32(motion, 16)? == 0 || dol::u32(motion, 20)? == 0 {
        return Ok(None);
    }
    let count = usize::from(dol::u16(motion, 12)?);
    let mut cursor = dol::u32(motion, 20)? as usize;
    let mut names = Vec::with_capacity(count);
    for index in 0..count {
        let tail = motion
            .get(cursor..)
            .context("field motion names exceed source")?;
        let end = tail
            .iter()
            .position(|&byte| byte == 0)
            .context("unterminated field motion joint name")?;
        let (name, invalid) = SHIFT_JIS.decode_without_bom_handling(&tail[..end]);
        ensure!(!invalid, "invalid field motion joint name");
        names.push((dol::u16(motion, 36 + index * 16 + 10)?, name.into_owned()));
        cursor += end + 1;
    }
    Ok(Some(names))
}
