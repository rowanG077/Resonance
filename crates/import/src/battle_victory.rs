//! Victory motion packages and group performance descriptors.
use crate::{
    animation::ModelBindings,
    read::{u16 as half, u32 as word},
    rel::Rel,
    resource::PartyResource,
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle_victory::{Condition, Group, ORDINARY_GROUPS, Ordinary, Performances},
    field_preload::{File, Role},
};
use std::{collections::BTreeMap, fs, path::Path};

pub fn publish(extracted: &Path, output: &Path) -> Result<String> {
    publish_source(extracted, output, "battle")
}

/// Every source affecting sparse binding or selection descriptors, for the
/// ordinary full publication DAG and the selected development publisher.
pub fn inputs(extracted: &Path) -> Result<BTreeMap<String, String>> {
    let files = extracted.join("files");
    let module_path = crate::field_resources::resolve_path(&files, "US_r_Top2Btl.rel")?;
    let module = Rel::read(&files.join(&module_path))?;
    let executable = fs::read(extracted.join("sys/main.dol"))?;
    let catalogue = crate::resource::read(&executable)?;
    let mut paths = vec![module_path];
    for at in [0x1d0, 0x1e4] {
        paths.push(crate::all_assets::roles::declared_path(
            &files,
            &module.text((4, at))?,
        )?);
    }
    for character in 1..=9 {
        paths.push(crate::field_resources::resolve_path(
            &files,
            catalogue.party(PartyResource::Body, character, 0)?,
        )?);
    }
    let mut result = paths
        .into_iter()
        .map(|path| {
            Ok((
                format!("files/{path}"),
                crate::media::hash_file(&files.join(path))?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    result.insert("sys/main.dol".into(), crate::digest(&executable));
    Ok(result)
}

pub(crate) fn publish_source(extracted: &Path, output: &Path, prefix: &str) -> Result<String> {
    let paths = extracted.join("files");
    let module = Rel::read(&paths.join(crate::field_resources::resolve_path(
        &paths,
        "US_r_Top2Btl.rel",
    )?))?;
    let archive_path = crate::all_assets::roles::declared_path(&paths, &module.text((4, 0x1e4))?)?;
    let groups_path = crate::all_assets::roles::declared_path(&paths, &module.text((4, 0x1d0))?)?;
    let archive = fs::read(paths.join(archive_path))?;
    let group_archive = fs::read(paths.join(groups_path))?;
    let catalogue = crate::resource::read(&fs::read(extracted.join("sys/main.dol"))?)?;
    let mut ordinary = Vec::new();
    let mut files = BTreeMap::new();
    for character in 1..=9 {
        let path = crate::field_resources::resolve_path(
            &paths,
            catalogue.party(PartyResource::Body, character, 0)?,
        )?;
        let original = fs::read(paths.join(path))?;
        let body_sha256 = crate::digest(&original);
        let body = crate::compression::payload(original)?;
        let primary = crate::field::sections(&body)?
            .into_iter()
            .next()
            .flatten()
            .context("missing victory body")?;
        let (_, resource) = crate::geometry::model_resource(&body[primary])?;
        let model =
            crate::model::Model::parse(&resource[crate::geometry::skeleton_range(resource)?])?;
        let bindings = ModelBindings::from_model(&model);
        for selector in 0..5 {
            let index = usize::from((character - 1) * 5 + selector);
            let bytes = archive
                .get(index * 0x38000..(index + 1) * 0x38000)
                .context("missing victory package")?;
            ensure!(word(bytes, 0)? == 2, "invalid victory package directory");
            let start = word(bytes, 4)? as usize;
            let motion = word(bytes, 8)? as usize;
            ensure!(
                start >= 12 && start < motion && motion < bytes.len(),
                "invalid victory package members"
            );
            let motion = crate::animation::read_member(&bytes[motion..])?
                .motion(&bindings)?
                .encode()?;
            let path = format!("clips/{}.motion", crate::digest(&motion));
            crate::write_atomic(&output.join(&path), &motion)?;
            files.insert(
                path.clone(),
                File {
                    sha256: crate::digest(&motion),
                    bytes: motion.len() as u64,
                    roles: [Role::Data].into(),
                },
            );
            ordinary.push(Ordinary {
                character,
                selector,
                source_sha256: crate::digest(bytes),
                body_sha256: body_sha256.clone(),
                motion: path,
            });
        }
    }
    let groups = ORDINARY_GROUPS
        .into_iter()
        .map(|id| {
            let index = usize::from(id);
            group_archive
                .get(index * 0x800..(index + 1) * 0x800)
                .context("missing group victory package")?;
            group(id, module.at((5, 0x5ea8 + (index - 1) * 8))?)
        })
        .collect::<Result<_>>()?;
    let record: Performances = Performances {
        module_sha256: crate::digest(&module.bytes),
        archive_sha256: crate::digest(&archive),
        group_archive_sha256: crate::digest(&group_archive),
        ordinary,
        groups,
        files,
    };
    let path = format!("{prefix}/victory.json");
    crate::write_atomic(&output.join(&path), &serde_json::to_vec(&record)?)?;
    Ok(path)
}

fn group(id: u8, row: &[u8]) -> Result<Group> {
    let row = row.get(..8).context("truncated group victory descriptor")?;
    let count = usize::from(row[3]);
    let group = Group {
        id,
        required_leader: selection_leader(id),
        condition: condition(id),
        character: row[4],
        voice: crate::battle_voice::sound(half(row, 0)?),
        participants: row
            .get(4..4 + count)
            .context("invalid victory group participant count")?
            .to_vec(),
    };
    group.validate()?;
    Ok(group)
}

// Translate packaged dialogue identities into native selection descriptors once.
fn selection_leader(id: u8) -> Option<u8> {
    Some(match id {
        1 | 19 | 21 | 32 => 5,
        5 | 14 | 41 => 3,
        6 | 10 | 11 | 15 | 16 | 17 | 22 | 27 | 34 | 35 | 36 => 1,
        7 | 38 | 39 => 9,
        8 | 9 => 6,
        12 | 13 | 40 => 7,
        20 | 25 => 2,
        23 => 4,
        33 => 8,
        _ => return None,
    })
}

fn condition(id: u8) -> Condition {
    use Condition::*;
    match id {
        1 => SheenaAffinity,
        2 => RaineFallen,
        3 => SheenaFallen,
        4 => RaineAndSheenaFallen,
        5 => GenisAffinity,
        6 | 20 => EnemyScanned,
        7 => LloydOutmatched,
        8 => ZelosAffinity,
        9 | 10 | 11 | 14 | 22 | 28 | 29 | 32 | 34 | 35 | 36 | 38 => Flawless,
        12 => PreseaDistantBeforeRecovery,
        13 => PreseaCloseAfterRecovery,
        16 => GenisAndKratosDistantFlawless,
        17 => GenisAndZelosDistantFlawless,
        18 => ChildhoodFriendsInjured,
        21 => ColetteAffinity,
        24 => LloydPoisoned,
        25 => ColetteHighParticipationFlawless,
        26 => ColetteFallen,
        27 => GenisJealous,
        30 => PreseaSheenaRainePoisoned,
        31 => GenisInjured,
        33 => RegalRecovered,
        37 => KratosFallen,
        40 => GenisFallenBeforePreseaRecovery,
        41 => PreseaDistantAfterRecoveryFlawless,
        42 => LloydManualPartyAutomatic,
        _ => Always,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_members_are_validated_at_import() -> Result<()> {
        let mut parsed = group(38, &[0x12, 0x34, 0, 3, 2, 3, 9, 0])?;
        assert_eq!(
            (parsed.character, parsed.voice),
            (2, Some(resonance_content::battle_voice::Sound::Cue(5161)))
        );
        assert_eq!(parsed.participants, [2, 3, 9]);
        assert_eq!(parsed.required_leader, Some(9));
        assert_eq!(parsed.condition, Condition::Flawless);
        parsed.required_leader = Some(1);
        assert!(parsed.validate().is_err());
        parsed.required_leader = Some(9);
        parsed.character = 1;
        assert!(parsed.validate().is_err());
        for row in [
            [0, 1, 0, 0, 2, 3, 0, 0],
            [0, 1, 0, 5, 2, 3, 0, 0],
            [0, 1, 0, 2, 2, 0, 0, 0],
            [0, 1, 0, 2, 2, 2, 0, 0],
        ] {
            assert!(group(1, &row).is_err());
        }
        assert!(group(1, &[0; 7]).is_err());
        Ok(())
    }

    #[test]
    #[ignore = "requires original discs; binds all nine characters' victory sparse curves"]
    fn victory_packages_produce_usable_clips() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let mut previous = None;
        for disc in [1, 2] {
            let extracted = local.join(format!("disc{disc}"));
            let output = tempfile::tempdir()?;
            let path = publish(&extracted, output.path())?;
            let bytes = fs::read(output.path().join(path))?;
            let value: Performances = serde_json::from_slice(&bytes)?;
            assert_eq!(value.ordinary.len(), 45);
            assert_eq!(
                value
                    .groups
                    .iter()
                    .map(|group| group.id)
                    .collect::<Vec<_>>(),
                ORDINARY_GROUPS
            );
            for performance in &value.ordinary {
                let motion = fs::read(output.path().join(&performance.motion))?;
                let declared = &value.files[&performance.motion];
                assert_eq!(declared.sha256, crate::digest(&motion));
                assert_eq!(declared.bytes, motion.len() as u64);
                assert!(
                    resonance_content::animation::Motion::decode(&motion)?.duration_frames > 0.
                );
            }
            // Disc packaging can differ while the playable content agrees.
            let semantic = serde_json::to_vec(&(value.ordinary, value.groups, value.files))?;
            if let Some(previous) = previous {
                assert_eq!(semantic, previous);
            }
            previous = Some(semantic);
        }
        Ok(())
    }
}
