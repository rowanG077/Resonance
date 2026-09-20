//! A costume selects a complete body/motion pair, including model-local anchors.
use super::*;
use resonance_content::{
    battle::visual::LinkedWeaponMotions,
    menu_data::{Costume, Title},
};
use std::collections::BTreeSet;

const GENIS: u8 = 3;
const WEAPON_MOTION_OFFSET: u16 = 60;

pub(super) fn required(character: u8, titles: &[Title]) -> BTreeSet<Costume> {
    std::iter::once(Costume::Standard)
        .chain(titles.iter().filter_map(|title| title.costume))
        .chain(matches!(character, 2 | 7).then_some(Costume::Story))
        .collect()
}

pub(super) fn head_bone(rel: &[u8], rig: &Rig) -> Result<u16> {
    let table = word(rel, 16)? as usize;
    let rodata = (word(rel, table + 4 * 8)? & !3) as usize;
    let name = rel
        .get(rodata + 0x228..)
        .context("missing head bone name")?;
    let end = name
        .iter()
        .position(|&b| b == 0)
        .context("unterminated head bone name")?;
    let name = std::str::from_utf8(&name[..end])?;
    rig.skeleton
        .bones
        .iter()
        .position(|bone| bone.name.eq_ignore_ascii_case(name))
        .context("missing authored stun head bone")?
        .try_into()
        .context("head bone exceeds range")
}

pub(super) fn weapon_link(archive: &MapArchive) -> WeaponMotionLink {
    WeaponMotionLink {
        source_sha256: archive.source_sha256.clone(),
        owner: GENIS,
        offset: WEAPON_MOTION_OFFSET,
        fallback: WEAPON_MOTION_OFFSET,
        time_scale: 0.5,
    }
}

pub(super) fn weapon_clips(archive: &MapArchive) -> Vec<SourceClip<'_>> {
    archive
        .sections
        .iter()
        .enumerate()
        .skip(usize::from(WEAPON_MOTION_OFFSET) + 2)
        .filter_map(|(member, range)| {
            range.as_ref().map(|range| SourceClip {
                slot: (member - 2) as u16,
                bytes: &archive.bytes[range.clone()],
                resource: None,
            })
        })
        .collect()
}

pub(super) fn linked_weapon_motions(
    rel: &[u8],
    weapon_bytes: &[u8],
    item: u16,
    owner: &MapArchive,
) -> Result<LinkedWeaponMotions> {
    let archive = weapon_archive(rel, weapon_bytes, item)?;
    let clips = weapon_clips(owner);
    ensure!(
        clips.iter().any(|clip| clip.slot == WEAPON_MOTION_OFFSET),
        "missing linked weapon fallback clip"
    );
    let rigs = weapon_packages(&archive)?
        .into_iter()
        .map(|(slot, package)| {
            let ranges = sections(package)?;
            ensure!(
                (5..=7).contains(&ranges.len()),
                "unsupported weapon layer package"
            );
            let model = &package[ranges[1].clone().context("missing linked weapon model")?];
            rig(model, &clips, RigKind::Weapon)
                .with_context(|| format!("costume weapon {item} slot {slot}"))
                .map(|rig| (slot, rig))
        })
        .collect::<Result<_>>()?;
    Ok(LinkedWeaponMotions {
        rigs,
        link: weapon_link(owner),
    })
}

pub(in crate::battle) fn validate_martial(
    catalogue: &crate::arte::Catalogue,
    actions: &resonance_content::battle::actions::BattleActions,
    visuals: &VisualAssets,
) -> Result<()> {
    use resonance_content::battle::actions::TechniqueProgram;
    for (&character, costumes) in &visuals.party {
        ensure!(
            costumes
                .concrete()
                .all(|(_, model)| model.visual.authored_motions.is_some()),
            "uncooked party {character} motion table"
        );
        let owned = catalogue.learned_by(character)?;
        for technique in actions
            .techniques
            .iter()
            .filter(|technique| owned.iter().any(|&id| u16::from(id) == technique.technique))
        {
            let TechniqueProgram::Martial { .. } = &technique.program else {
                continue;
            };
            for phase in technique.martial_phases(character)? {
                for (costume, model) in costumes.concrete() {
                    model.validate_action(&phase.action).with_context(|| {
                        format!(
                            "party {character} costume {costume:?} arte {} phase {}",
                            technique.technique, phase.variant,
                        )
                    })?;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "costume_tests.rs"]
mod tests;
