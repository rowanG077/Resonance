//! Independent body and motion selections for each character's costume.
use crate::{field::MapArchive, field_resources::resolve_path, resource::PartyResource};
use anyhow::{Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub(super) fn motion_table(
    archive: &MapArchive,
) -> Result<resonance_content::battle::visual::AuthoredMotionTable> {
    let motions = archive
        .sections
        .get(2..)
        .context("missing party motion table")?;
    Ok(resonance_content::battle::visual::AuthoredMotionTable {
        count: motions.len().try_into()?,
        nulls: motions
            .iter()
            .enumerate()
            .filter_map(|(slot, resource)| resource.is_none().then_some(slot as u16))
            .collect(),
    })
}

pub(in crate::battle) fn archive(
    executable: &[u8],
    files: &Path,
    character: u8,
    costume: u8,
) -> Result<MapArchive> {
    MapArchive::open(&path(
        executable,
        files,
        character,
        costume,
        PartyResource::BattleMotion,
    )?)
}

pub(super) fn body(executable: &[u8], files: &Path, character: u8, costume: u8) -> Result<Vec<u8>> {
    Ok(fs::read(path(
        executable,
        files,
        character,
        costume,
        PartyResource::Body,
    )?)?)
}

fn path(
    executable: &[u8],
    files: &Path,
    character: u8,
    costume: u8,
    kind: PartyResource,
) -> Result<PathBuf> {
    let resources = crate::resource::read(executable)?;
    let name = resources.party(kind, character, costume)?;
    let actual = resolve_path(files, name)
        .with_context(|| format!("party character {character} costume {costume}: {name}"))?;
    Ok(files.join(actual))
}

/// Keep declarations separate from existence checks; one original body is absent.
#[cfg(test)]
pub(super) fn names(executable: &[u8], character: u8, costume: u8) -> Result<(String, String)> {
    let resources = crate::resource::read(executable)?;
    Ok((
        resources
            .party(PartyResource::Body, character, costume)?
            .into(),
        resources
            .party(PartyResource::BattleMotion, character, costume)?
            .into(),
    ))
}
