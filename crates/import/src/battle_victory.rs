//! Victory motion packages and group performance descriptors.
use crate::{
    animation::ModelBindings,
    read::{u16 as half, u32 as word},
    rel::Rel,
};
use anyhow::{Context, Result, ensure};
use resonance_content::battle_victory::{
    Condition, Event, GROUP_COUNT, Group, Ordinary, Performances,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

/// Every source affecting sparse binding or selection descriptors, for the
/// ordinary full publication DAG and the selected development publisher.
pub fn inputs(extracted: &Path) -> Result<BTreeMap<String, String>> {
    let files = extracted.join("files");
    let module_path = crate::field_resources::resolve_path(&files, "US_r_Top2Btl.rel")?;
    let module = Rel::read(&files.join(&module_path))?;
    let executable = fs::read(extracted.join("sys/main.dol"))?;
    let catalogue = crate::resource::read(&executable)?;
    let mut paths = BTreeSet::from([module_path]);
    paths.insert(crate::all_assets::roles::declared_path(
        &files,
        &module.text((4, 0x1e4))?,
    )?);
    for name in catalogue.party_bodies.iter().flatten().flatten() {
        if let Some(path) = crate::field_resources::find_path(&files, name)? {
            paths.insert(path);
        }
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

pub fn prepare(extracted: &Path, output: &Path) -> Result<Performances> {
    let paths = extracted.join("files");
    let module = Rel::read(&paths.join(crate::field_resources::resolve_path(
        &paths,
        "US_r_Top2Btl.rel",
    )?))?;
    let archive_path = crate::all_assets::roles::declared_path(&paths, &module.text((4, 0x1e4))?)?;
    let archive = fs::read(paths.join(archive_path))?;
    let catalogue = crate::resource::read(&fs::read(extracted.join("sys/main.dol"))?)?;
    let mut ordinary = Vec::new();
    for character in 1..=9 {
        for (costume, name) in catalogue.party_bodies[usize::from(character - 1)]
            .iter()
            .enumerate()
        {
            let Some(name) = name else { continue };
            let Some(path) = crate::field_resources::find_path(&paths, name)? else {
                continue;
            };
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
                let clip = crate::animation::read_member(&bytes[motion..])?.motion(&bindings)?;
                let (timeline, unsupported_reason) =
                    match timeline(&bytes[start..motion], clip.duration_frames) {
                        Ok(events) => (Some(events), None),
                        Err(error) => (None, Some(format!("{error:#}"))),
                    };
                let motion = clip.encode()?;
                let path = format!("clips/{}.motion.json", crate::digest(&motion));
                crate::write_atomic(&output.join(&path), &motion)?;
                ordinary.push(Ordinary {
                    character,
                    costume: Some(costume.try_into()?),
                    selector,
                    source_sha256: crate::digest(bytes),
                    body_sha256: body_sha256.clone(),
                    motion: path,
                    timeline,
                    unsupported_reason,
                });
            }
        }
    }
    #[derive(serde::Deserialize)]
    struct SelectionRecipes {
        provenance: String,
        groups: BTreeMap<u8, resonance_content::battle_victory::SelectionRule>,
    }
    let selection: SelectionRecipes =
        serde_json::from_str(include_str!("battle_victory/selection.json"))?;
    ensure!(
        !selection.provenance.is_empty(),
        "missing victory selection provenance"
    );
    ensure!(
        selection.groups.keys().copied().eq(1..=GROUP_COUNT),
        "victory selection recipes must cover the source groups"
    );
    for (id, rule) in &selection.groups {
        rule.validate()?;
        ensure!(
            selection
                .groups
                .range(..id)
                .all(|(_, earlier)| !rule.conflicts_with(earlier)),
            "ambiguous victory selection priority"
        );
    }
    let groups = (1..=GROUP_COUNT)
        .map(|id| {
            let index = usize::from(id);
            group(
                id,
                module.at((5, 0x5ea8 + (index - 1) * 8))?,
                selection.groups[&id].clone(),
            )
        })
        .collect::<Result<_>>()?;
    let record: Performances = Performances {
        module_sha256: crate::digest(&module.bytes),
        archive_sha256: crate::digest(&archive),
        ordinary,
        groups,
    };
    Ok(record)
}

/// These short streams select the packaged clip or change facial atlas layers.
fn timeline(bytes: &[u8], duration: f32) -> Result<Vec<(u16, Event)>> {
    const ROW_BYTES: usize = 12;
    const MOTION: u8 = 8;
    const REPEAT: u8 = 0x40;
    const VICTORY_CLIP: u8 = 23;
    let mut events = Vec::new();
    let mut cursor = 0;
    let mut completed_at = None;
    loop {
        let at = half(bytes, cursor)? as i16;
        if at == -2 {
            ensure!(!events.is_empty(), "empty victory timeline");
            return Ok(events);
        }
        let row = bytes
            .get(cursor..cursor + ROW_BYTES)
            .context("truncated victory timeline")?;
        if at == -3 {
            ensure!(
                row[2] == 0
                    && row[4..8] == [0, 0, MOTION | REPEAT, u8::MAX]
                    && crate::read::f32(row, 8)? == 0.5,
                "unsupported victory completion posture"
            );
            let at = completed_at
                .take()
                .context("victory completion needs a finite pose")?;
            ensure!(
                events.last().is_none_or(|(previous, _)| *previous <= at),
                "unordered victory completion"
            );
            events.push((at, Event::Idle { blend: row[3] }));
            cursor += ROW_BYTES;
            continue;
        }
        let at = u16::try_from(at).context("victory timeline control is not prepared")?;
        ensure!(
            events.last().is_none_or(|(previous, _)| *previous <= at),
            "unordered victory timeline"
        );
        let event = if row[2] == u8::MAX {
            ensure!(
                row[7..].iter().all(|&byte| byte == 0),
                "unsupported victory expression fields"
            );
            Event::Expression(row[3..7].try_into().unwrap())
        } else {
            ensure!(
                row[2] == VICTORY_CLIP && row[7] == u8::MAX,
                "unsupported victory clip binding"
            );
            ensure!(
                row[6] & !REPEAT == MOTION,
                "unsupported victory motion flags {:#x}",
                row[6]
            );
            let rate = crate::read::f32(row, 8)?;
            ensure!(
                rate.is_finite() && (0.01..=4.).contains(&rate),
                "invalid victory motion rate"
            );
            let start = f32::from(row[4]) * rate;
            let end = (row[5] != 0).then_some(f32::from(row[5]) * rate);
            ensure!(
                end.is_none_or(|end| end > start),
                "invalid victory motion interval"
            );
            completed_at = if row[6] & REPEAT == 0 {
                let completion = f32::from(at)
                    + f32::from(row[3])
                    + ((end.unwrap_or(duration) - start) / rate).floor()
                    + 1.;
                ensure!(
                    completion.is_finite() && (0. ..=f32::from(u16::MAX)).contains(&completion),
                    "victory completion exceeds timeline"
                );
                Some(completion as u16)
            } else {
                None
            };
            Event::Pose {
                start,
                end,
                rate,
                blend: row[3],
                repeat: row[6] & REPEAT != 0,
            }
        };
        events.push((at, event));
        cursor += ROW_BYTES;
    }
}

fn group(
    id: u8,
    row: &[u8],
    selection: resonance_content::battle_victory::SelectionRule,
) -> Result<Group> {
    let row = row.get(..8).context("truncated group victory descriptor")?;
    let count = usize::from(row[3]);
    let group = Group {
        id,
        required_leader: selection_leader(id),
        condition: condition(id),
        selection,
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
    fn victory_timeline_keeps_expression_and_delayed_loop() -> Result<()> {
        let mut rows = vec![
            0, 0, 23, 15, 0, 0, 8, 255, 0x3f, 0, 0, 0, 0, 0, 255, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            111, 23, 0, 96, 0, 0x48, 255, 0x3f, 0, 0, 0, 255, 254,
        ];
        let events = timeline(&rows, 88.)?;
        assert_eq!(events[1], (0, Event::Expression([4, 0, 0, 0])));
        assert_eq!(
            events[2],
            (
                111,
                Event::Pose {
                    start: 48.,
                    end: None,
                    rate: 0.5,
                    blend: 0,
                    repeat: true,
                }
            )
        );
        // Timeline offsets are update counts, scaled by the selected playback rate.
        for rate in [0.5_f32, 1.] {
            rows[24..26].copy_from_slice(&231_u16.to_be_bytes());
            rows[28] = 216;
            rows[29] = 240;
            rows[32..36].copy_from_slice(&rate.to_be_bytes());
            assert_eq!(
                timeline(&rows, 336.)?[2],
                (
                    231,
                    Event::Pose {
                        start: 216. * rate,
                        end: Some(240. * rate),
                        rate,
                        blend: 0,
                        repeat: true,
                    }
                )
            );
        }
        rows[24..26].copy_from_slice(&(-4_i16).to_be_bytes());
        assert!(
            timeline(&rows, 88.)
                .unwrap_err()
                .to_string()
                .contains("control")
        );
        Ok(())
    }

    #[test]
    fn completed_colette_celebrations_resume_idle_after_the_blended_clip() -> Result<()> {
        let mut rows = vec![
            0, 0, 23, 12, 0, 0, 8, 255, 0x3f, 0x80, 0, 0, 0, 0, 255, 4, 0, 0, 0, 0, 0, 0, 0, 0,
            255, 253, 0, 4, 0, 0, 0x48, 255, 0x3f, 0, 0, 0, 255, 254,
        ];
        for blend in [4, 0] {
            rows[27] = blend;
            let events = timeline(&rows, 252.)?;
            assert_eq!(events[1], (0, Event::Expression([4, 0, 0, 0])));
            assert_eq!(events[2], (265, Event::Idle { blend }));
        }
        rows[6] |= 0x40;
        assert!(
            timeline(&rows, 252.).is_err(),
            "a loop has no completion deadline"
        );
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
            let value = prepare(&extracted, output.path())?;
            let catalogue = crate::resource::read(&fs::read(extracted.join("sys/main.dol"))?)?;
            let mut bodies = 0;
            for costumes in &catalogue.party_bodies {
                for name in costumes.iter().flatten() {
                    bodies += usize::from(
                        crate::field_resources::find_path(&extracted.join("files"), name)?
                            .is_some(),
                    );
                }
            }
            assert_eq!(value.ordinary.len(), bodies * 5);
            assert_eq!(
                value
                    .groups
                    .iter()
                    .map(|group| group.id)
                    .collect::<Vec<_>>(),
                (1..=GROUP_COUNT).collect::<Vec<_>>()
            );
            for performance in &value.ordinary {
                assert!(
                    performance.timeline.is_some(),
                    "undecoded victory stream: {performance:?}"
                );
                assert!(performance.unsupported_reason.is_none());
                let motion = fs::read(output.path().join(&performance.motion))?;
                assert!(performance.costume.is_some());
                assert!(
                    resonance_content::animation::Motion::decode(&motion)?.duration_frames > 0.
                );
            }
            // Disc packaging can differ while the playable content agrees.
            let semantic = serde_json::to_vec(&(value.ordinary, value.groups))?;
            if let Some(previous) = previous {
                assert_eq!(semantic, previous);
            }
            previous = Some(semantic);
        }
        Ok(())
    }
}
