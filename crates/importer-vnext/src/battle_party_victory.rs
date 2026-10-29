//! Named victory performances from the native five-clip character packages.
use crate::{
    battle_party_mappings,
    battle_party_native::{float, half, signed, word},
    character_models, compression, dol,
    rel::Rel,
    world_motion,
};
use anyhow::{Context, Result, ensure};
use resonance_content::battle_victory::{
    Event,
    document::{Definition, Performance, Timeline, selector_name},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
use symphonia_constants::PartyCharacterId;

fn timeline(bytes: &[u8], duration: f32) -> Result<Vec<(u16, Event)>> {
    let mut events = Vec::new();
    let mut cursor = 0;
    let mut completed = None;
    loop {
        let at = signed(bytes, cursor)?;
        if at == -2 {
            ensure!(!events.is_empty(), "empty victory timeline");
            return Ok(events);
        }
        let row = bytes
            .get(cursor..cursor + 12)
            .context("truncated victory timeline")?;
        if at == -3 {
            ensure!(
                row[2] == 0 && row[4..8] == [0, 0, 0x48, 255] && float(row, 8)? == 0.5,
                "unsupported victory completion posture"
            );
            let at = completed
                .take()
                .context("victory completion requires finite pose")?;
            ensure!(
                events.last().is_none_or(|(previous, _)| *previous <= at),
                "unordered victory completion"
            );
            events.push((at, Event::Idle { blend: row[3] }));
            cursor += 12;
            continue;
        }
        let at = u16::try_from(at).context("unsupported victory timeline control")?;
        ensure!(
            events.last().is_none_or(|(previous, _)| *previous <= at),
            "unordered victory events"
        );
        let event = if row[2] == 255 {
            ensure!(
                row[7..].iter().all(|&v| v == 0),
                "unsupported victory expression"
            );
            Event::Expression(row[3..7].try_into()?)
        } else {
            ensure!(
                row[2] == 23 && row[7] == 255 && row[6] & !0x40 == 8,
                "unsupported victory pose"
            );
            let rate = float(row, 8)?;
            ensure!(
                rate.is_finite() && (0.01..=4.).contains(&rate),
                "invalid victory rate"
            );
            let start = f32::from(row[4]) * rate;
            let end = (row[5] != 0).then_some(f32::from(row[5]) * rate);
            ensure!(
                end.is_none_or(|end| end > start),
                "invalid victory interval"
            );
            completed = if row[6] & 0x40 == 0 {
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
                repeat: row[6] & 0x40 != 0,
            }
        };
        events.push((at, event));
        cursor += 12;
    }
}

struct Row {
    costume: u8,
    name: String,
    selector: u8,
    motion: Vec<u8>,
    timeline: Timeline,
    existing: Option<String>,
}

pub fn cook_battle_victories(root: &Path, library: &Path) -> Result<()> {
    cook_battle_victory_group(
        root,
        library,
        &PartyCharacterId::ALL.map(PartyCharacterId::raw),
    )
}

pub fn cook_battle_victory_group(root: &Path, library: &Path, characters: &[u8]) -> Result<()> {
    let files = root.join("disc1/files");
    let module = Rel::read(fs::read(files.join("US_r_Top2Btl.rel"))?)?;
    let archive_name = module.text(4, 0x1e4)?;
    let archive_path = battle_party_mappings::native_path(
        &files,
        archive_name.strip_prefix("./").unwrap_or(&archive_name),
    )?
    .context("missing native victory archive")?;
    let archive = fs::read(archive_path)?;
    ensure!(archive.len() >= 45 * 0x38000, "truncated victory archive");
    let exe = fs::read(root.join("disc1/sys/main.dol"))?;
    let mut motions = BTreeMap::<Vec<u8>, String>::new();
    let mut enemy_motions = BTreeMap::<u64, Vec<String>>::new();
    for enemy in fs::read_dir(library.join("meshes/monsters"))? {
        let directory = enemy?.path().join("animations");
        if !directory.is_dir() {
            continue;
        }
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            if entry.path().extension().is_some_and(|v| v == "glb") {
                enemy_motions
                    .entry(entry.metadata()?.len())
                    .or_default()
                    .push(
                        entry
                            .path()
                            .strip_prefix(library)?
                            .to_string_lossy()
                            .into_owned(),
                    );
            }
        }
    }
    let selected = characters.iter().copied().collect::<BTreeSet<_>>();
    ensure!(
        !selected.is_empty() && selected.len() == characters.len(),
        "empty or duplicate victory characters"
    );
    for character in selected {
        let id = PartyCharacterId::try_from(character)?;
        let name = id.resource_name().context("unnamed victory character")?;
        let mut rows = Vec::new();
        for costume in 0..5u8 {
            let source = battle_party_mappings::source_name(&exe, 0x801f_ace4, character, costume)?;
            if battle_party_mappings::native_path(&files, &source)?.is_none() {
                continue;
            }
            let costume_name = battle_party_mappings::costume_name(character, costume)?;
            let body_name = character_models::party_body_name(character, costume)?;
            let body_base = if body_name == "default" {
                format!("meshes/characters/{name}")
            } else {
                format!("meshes/characters/{name}/{body_name}")
            };
            let glb = fs::read(library.join(format!("{body_base}/meshes/body.glb")))
                .with_context(|| format!("victory {name}:{costume} requires native body media"))?;
            let bindings = character_models::party_joint_bindings(root, character, costume)?;
            let party: resonance_content::battle_model::document::PartyDefinition =
                serde_json::from_slice(&fs::read(
                    library.join(format!("battle/party/{name}/{costume_name}/party.json")),
                )?)?;
            let bank = crate::battle_party_model::motion_bank(root, character, costume)?;
            let mut native_motions = BTreeMap::new();
            for clip in party.animations.iter().flat_map(|group| &group.clips) {
                let duel = (character == 1 && clip.resource_slot == 100)
                    .then(|| crate::battle_party_model::duel_motion(root))
                    .transpose()?;
                let bytes = if let Some(bytes) = &duel {
                    Some(bytes.as_slice())
                } else {
                    crate::field_core::optional_section(&bank, usize::from(clip.resource_slot) + 2)?
                };
                if let Some(bytes) = bytes {
                    native_motions.insert(
                        crate::battle_party_model::expanded_motion(bytes)?,
                        clip.motion.clone(),
                    );
                }
            }
            for selector in 0..5u8 {
                let index = usize::from((character - 1) * 5 + selector);
                let package = &archive[index * 0x38000..(index + 1) * 0x38000];
                ensure!(word(package, 0)? == 2, "invalid victory package");
                let at = word(package, 4)? as usize;
                let motion = word(package, 8)? as usize;
                ensure!(
                    at >= 12 && at < motion && motion < package.len(),
                    "invalid victory members"
                );
                let bytes = if dol::u32(package, motion)? == 0x007b7960 {
                    package[motion..].to_vec()
                } else {
                    compression::decode(&package[motion..])?
                };
                let duration =
                    (0..usize::from(half(&bytes, 12)?)).try_fold(0f32, |duration, index| {
                        Ok::<_, anyhow::Error>(duration.max(float(&bytes, 36 + index * 16)?))
                    })?;
                let events = timeline(&package[at..motion], duration)
                    .with_context(|| format!("victory {name}:{costume}, selector {selector}"))?;
                // The body owner already established the receiving rig's native
                // motion bindings, including compatible costume sharing.
                let mut existing = None;
                for (native, path) in &native_motions {
                    if world_motion::same_source(native, &bytes)? {
                        existing = Some(path.clone());
                        break;
                    }
                }
                let encoded = if let Some(path) = &existing {
                    fs::read(library.join(path))?
                } else {
                    world_motion::encode_bound(&bytes, &glb, &bindings)
                        .with_context(|| format!("victory {name}:{costume}, selector {selector}"))?
                };
                if existing.is_none() {
                    if let Some(paths) = enemy_motions.get(&(encoded.len() as u64)) {
                        for path in paths {
                            if fs::read(library.join(path))? == encoded {
                                existing = Some(path.clone());
                                break;
                            }
                        }
                    }
                }
                rows.push(Row {
                    costume,
                    name: costume_name.clone(),
                    selector,
                    motion: encoded,
                    timeline: Timeline {
                        events: Some(events),
                        unsupported_reason: None,
                    },
                    existing,
                });
            }
        }
        let mut definition: Definition = Definition::default();
        for row in &rows {
            let selector = selector_name(row.selector)?;
            let path = if let Some(path) = &row.existing {
                motions
                    .entry(row.motion.clone())
                    .or_insert_with(|| path.clone());
                path.clone()
            } else if let Some(path) = motions.get(&row.motion) {
                path.clone()
            } else {
                let shared = rows
                    .iter()
                    .filter(|other| other.motion == row.motion)
                    .map(|other| other.costume)
                    .collect::<BTreeSet<_>>()
                    .len()
                    > 1;
                let owner = if shared {
                    format!("battle/party/{name}")
                } else {
                    format!("battle/party/{name}/{}", row.name)
                };
                let stem = format!("{owner}/victory/{selector}");
                let mut path = format!("{stem}.glb");
                let mut variant = 2;
                while motions.values().any(|previous| previous == &path) {
                    path = format!("{stem}-variant-{variant}.glb");
                    variant += 1;
                }
                crate::output::write(&library.join(&path), &row.motion)?;
                motions.insert(row.motion.clone(), path.clone());
                path
            };
            let timeline = if let Some((name, _)) = definition
                .timelines
                .iter()
                .find(|(_, value)| **value == row.timeline)
            {
                name.clone()
            } else {
                let stem = format!("celebration-{}", row.selector + 1);
                let mut key = stem.clone();
                let mut variant = 2;
                while definition.timelines.contains_key(&key) {
                    key = format!("{stem}-variant-{variant}");
                    variant += 1;
                }
                definition
                    .timelines
                    .insert(key.clone(), row.timeline.clone());
                key
            };
            let performance = Performance {
                motion: path,
                timeline,
            };
            if let Some(previous) = definition
                .costumes
                .entry(row.name.clone())
                .or_default()
                .insert(selector.into(), performance.clone())
            {
                ensure!(
                    previous == performance,
                    "conflicting victory costume {name}:{}",
                    row.name
                );
            }
        }
        ensure!(
            !definition.costumes.is_empty() && definition.costumes.values().all(|v| v.len() == 5),
            "incomplete victory owner {name}"
        );
        crate::output::json(
            &library.join(format!("battle/party/{name}/victory.json")),
            &definition,
        )?;
    }
    Ok(())
}
