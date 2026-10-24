//! Field movement, interaction and animation completion semantics. Runtime
//! assembly supplies the same actor overrides and current clip durations.
use super::record_json;
use crate::{ScenePart, field::document::Definition, library::Snapshot, preparation::ASSET_LIMIT};
use anyhow::{Context, Result, ensure};
use sha2::Sha256;
use std::collections::BTreeMap;

pub(super) fn capture(
    snapshot: &mut Snapshot,
    hash: &mut Sha256,
    id: u32,
    definition: &Definition,
    cancelled: &impl Fn() -> bool,
) -> Result<()> {
    let field = definition.assemble(id, |path| {
        ensure!(!cancelled(), "field identity preparation cancelled");
        snapshot.read(path, ASSET_LIMIT)
    })?;
    record_json(
        hash,
        &format!("field/{id}/collision"),
        &(field.ground, field.regions, field.doors),
    )?;
    parts(hash, &format!("field/{id}/scenery"), &field.parts)?;
    for part in &field.parts {
        attachment(
            snapshot,
            hash,
            &format!("field/{id}/scenery/{}/attachment", part.resource),
            part,
            cancelled,
        )?;
    }
    let actors: BTreeMap<_, _> = field
        .actors
        .iter()
        .map(|actor| (actor.resource, actor))
        .collect();
    for (resource, actor) in actors {
        let name = format!("field/{id}/actor/{resource}");
        record_json(hash, &format!("{name}/collision"), &actor.collision)?;
        parts(hash, &name, &actor.parts)?;
        attachment(
            snapshot,
            hash,
            &format!("{name}/attachment"),
            actor.parts.first().context("field actor has no body")?,
            cancelled,
        )?;
    }
    Ok(())
}

fn attachment(
    snapshot: &mut Snapshot,
    hash: &mut Sha256,
    name: &str,
    part: &ScenePart,
    cancelled: &impl Fn() -> bool,
) -> Result<()> {
    let inputs = crate::field::attachments::read(part, |path| {
        ensure!(
            !cancelled(),
            "field attachment identity preparation cancelled"
        );
        snapshot.read(path, ASSET_LIMIT)
    })?;
    record_json(hash, name, &inputs.is_some())?;
    if let Some(inputs) = inputs {
        record_json(
            hash,
            &format!("{name}/rig"),
            &(&*inputs.skeleton, inputs.mesh_bounds),
        )?;
        for ((resource, slot), motion) in inputs.clips {
            record_json(
                hash,
                &format!("{name}/motion/{resource:?}/{slot}"),
                &*motion,
            )?;
        }
    }
    Ok(())
}

fn parts(hash: &mut Sha256, name: &str, parts: &[ScenePart]) -> Result<()> {
    for (index, part) in parts.iter().enumerate() {
        let clips: BTreeMap<_, _> = part
            .clips
            .iter()
            .map(|clip| {
                (
                    (clip.animation_resource, clip.resource_slot),
                    clip.duration_seconds,
                )
            })
            .collect();
        // Script waits depend on selected slots and actual completion time.
        // Non-primary drawing layers contribute timing only. Gameplay attachment
        // curves are recorded separately through the runtime's own input reader.
        record_json(
            hash,
            &format!("{name}/part/{index}"),
            &(part.resource, clips.into_iter().collect::<Vec<_>>()),
        )?;
    }
    Ok(())
}
