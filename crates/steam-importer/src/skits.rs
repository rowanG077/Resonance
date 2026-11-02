//! Native skit declarations and scenarios directly into the shared catalogue.
use crate::{output, pe, script, script_input, source::Source, text::Bank};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    library::{FIELDS, Mappings},
    overworld::PartyCharacter,
    skit::{SkitCondition, SkitDefinition, SkitLocation, SkitResourcePaths, SkitTrigger, document},
};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
use symphonia_script::{NativeCall, scenario};

// Definitions and filename arrays have separate extents. Never let an unused
// definition read into its neighbor's physical filename directory.
const TABLES: [(u16, u32, usize, u32, usize); 5] = [
    (1, 0x8af098, 120, 0x8b3cc0, 120),
    (250, 0x8afbd8, 35, 0x8b3ea0, 40),
    (350, 0x8aff20, 20, 0x8b3cbc, 1),
    (600, 0x8b0100, 260, 0x8b3f40, 237),
    (450, 0x8b1960, 94, 0x8b4300, 94),
];

pub(crate) fn cook(source: &Source, library: &Path) -> Result<()> {
    crate::audio_streams::silence_skits(source, library)?;
    let image = pe::Image::parse(&source.executable)?;
    let texts = source.require("TOS_TEXT.TXB")?;
    let texts = Bank::parse(&texts)?;
    let title_table = pe::word(image.slice(0x8af05c + source.language.slot() * 4, 4)?, 0)?;
    let labels: BTreeMap<u16, String> =
        serde_json::from_str(include_str!("../../importer/src/skit-titles.json"))?;
    let conditions: BTreeMap<u16, SkitCondition> =
        serde_json::from_str(include_str!("skit-conditions.json"))?;
    let preview_order = preview_order(&image)?;
    let denied = skip_disabled(&image)?;
    let mut definitions = Vec::new();
    let mut mappings = BTreeMap::new();
    let mut resources = BTreeMap::new();
    let mut owners = BTreeMap::<String, Vec<u8>>::new();
    let mut media = BTreeSet::new();
    let mut selectors = BTreeSet::new();
    for (first, rows, count, filenames, slots) in TABLES {
        for index in 0..count {
            let row = image.slice(rows + index as u32 * 24, 24)?;
            let id = pe::half(row, 0)?;
            ensure!(
                id == first + index as u16 && pe::half(row, 2)? == 0 && row[17..20] == [0; 3],
                "invalid skit declaration {id}"
            );
            let range = [pe::word(row, 4)? as i32, pe::word(row, 8)? as i32];
            let enabled = range != [-1; 2];
            let mask = pe::half(row, 12)?;
            ensure!(
                mask & !0x3fe == 0 && matches!(row[16], 0 | 255),
                "invalid skit selector {id}"
            );
            let title = texts.menu(
                pe::word(image.slice(title_table + pe::word(row, 20)? * 4, 4)?, 0)?,
                encoding_rs::WINDOWS_1252,
            )?;
            if enabled && matches!(first, 1 | 250 | 600) {
                let condition = if row[16] == 0 {
                    SkitCondition::None
                } else {
                    selectors.insert(id);
                    conditions
                        .get(&id)
                        .with_context(|| format!("missing skit condition {id}"))?
                        .clone()
                };
                condition.validate()?;
                let location = match pe::half(row, 14)? as i16 {
                    -9999 => SkitLocation::Anywhere,
                    -1 => SkitLocation::Overworld,
                    -2 => SkitLocation::Sylvarant,
                    -3 => SkitLocation::TetheAlla,
                    -4 => SkitLocation::Field,
                    id if id >= 0 => SkitLocation::Map((id as u16).into()),
                    at => anyhow::bail!("invalid skit location {at}"),
                };
                let party = (1..=9u8)
                    .filter(|member| mask & (1 << member) != 0)
                    .map(|member| PartyCharacter::try_from(member).map_err(Into::into))
                    .collect::<Result<_>>()?;
                definitions.push(SkitDefinition {
                    id,
                    title: title.clone(),
                    trigger: if first == 600 {
                        SkitTrigger::Exploration
                    } else {
                        SkitTrigger::Entry
                    },
                    story: (range != [-999999999; 2]).then_some(range),
                    party,
                    location,
                    condition,
                });
            }
            if index >= slots {
                ensure!(!enabled, "enabled skit {id} has no filename");
                continue;
            }
            let filename =
                image.text(pe::word(image.slice(filenames + index as u32 * 4, 4)?, 0)?)?;
            ensure!(
                filename.to_ascii_lowercase().ends_with(".skp"),
                "invalid skit filename {filename}"
            );
            let Some((bytes, mut encoding)) = source.read_encoded(filename)? else {
                ensure!(
                    !enabled && !preview_order.contains(&id),
                    "required skit {id} is missing: {filename}"
                );
                continue;
            };
            // The English bank mixes retained Japanese punctuation and Western
            // accents; the untranslated debugger keeps its original code page.
            if encoding == script::TextEncoding::ShiftJis
                && !filename.eq_ignore_ascii_case("FC_C004.skp")
            {
                encoding = script::TextEncoding::ShiftJisWithWesternFallback;
            }
            let mut program = member(&bytes)?.to_vec();
            if id == 615 {
                // FC_W015 contains an unfinished outer loop whose impossible
                // false exit targets FFFF. Its preceding assignment makes the
                // first condition true; the body has no edge back to this loop.
                // Preserve every PC and the executed body, and admit only the
                // observed code (the digest excludes localized message text).
                let canonical = script::normalize(&program, encoding)?;
                let header = scenario::parse_header(&canonical)?;
                ensure!(
                    Sha256::digest(&canonical[..header.auxiliary_offset()])
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect::<String>()
                        == "059d0566ce1f6bafc31383806d7e43c7736e34a6df5a1dbbe6b79876d79dbd77",
                    "changed skit 615 control flow"
                );
                let at = header.code_base() + 0x111 * 2 + 2;
                program[at..at + 2].copy_from_slice(&0x124u16.to_le_bytes());
            }
            let decoded = script::decode(&program, encoding)
                .with_context(|| format!("skit {id}: {filename}"))?;
            let bytes = script_input::normalize(&decoded.bytes)?;
            for args in scenario::calls::arguments(&bytes, NativeCall::PlayVoice, 1)?.into_values()
            {
                if let Some(id) = args[0].filter(|id| *id != -1) {
                    media.insert(id as u32);
                } else {
                    ensure!(args[0].is_some(), "dynamic skit voice {id}");
                }
            }
            let mut name = labels
                .get(&id)
                .cloned()
                .unwrap_or_else(|| format!("skit-{id:04}"));
            if labels.values().filter(|label| **label == name).count() > 1 {
                name = format!("{name}-{id:04}");
            }
            let root = library.join("skits").join(&name);
            if let Some(previous) = owners.get(&name) {
                ensure!(previous == &bytes, "different skit programs share {name}");
            } else {
                output::write(&root.join("scenario.ssb"), &bytes)?;
                if !decoded.messages.is_empty() {
                    output::json(&root.join("messages.json"), &decoded.messages)?;
                }
                owners.insert(name.clone(), bytes);
            }
            mappings.insert(u32::from(id), name);
            if denied.contains(&id) || !title.is_empty() {
                resources.insert(
                    id,
                    SkitResourcePaths {
                        script: String::new(),
                        messages: None,
                        allow_skip: !denied.contains(&id),
                        title: Some(title),
                    },
                );
            }
        }
    }
    ensure!(
        conditions.keys().copied().collect::<BTreeSet<_>>() == selectors,
        "unbound skit conditions"
    );
    definitions.sort_by_key(|skit| skit.id);
    let fields: Mappings = serde_json::from_slice(&fs::read(library.join(FIELDS))?)?;
    resonance_content::skit::fields::name_fields(&mut definitions, &fields)?;
    let catalogue = document::Definition {
        skits: definitions,
        preview_order,
        resources,
        media: document::Media {
            ids: media,
            speed: 1.,
        },
    };
    output::json(&library.join(document::MAPPINGS), &Mappings::new(mappings)?)?;
    output::json(&library.join(document::PATH), &catalogue)
}

fn member(bytes: &[u8]) -> Result<&[u8]> {
    ensure!(
        pe::word(bytes, 0)? == 2 && pe::word(bytes, 4)? == 64,
        "unsupported skit package directory"
    );
    let end = pe::word(bytes, 8)? as usize;
    ensure!(
        end >= 64 && end.is_multiple_of(64),
        "invalid skit member boundary"
    );
    let tail = bytes.get(end..).context("skit member exceeds package")?;
    ensure!(
        tail.len() == 64
            && tail[..8] == [1, 0, 0, 0, 0, 0, 0, 0]
            && tail[8..].iter().all(|byte| *byte == 0xfe),
        "invalid skit resource directory"
    );
    bytes.get(64..end).context("truncated skit program")
}

fn preview_order(image: &pe::Image<'_>) -> Result<Vec<u16>> {
    let mut order = Vec::new();
    let mut seen = BTreeSet::new();
    for row in image.slice(0x8b4478, 860 * 2)?.as_chunks::<2>().0 {
        let id = pe::half(row, 0)?;
        if id == u16::MAX {
            return Ok(order);
        }
        ensure!(
            (1..=860).contains(&id) && seen.insert(id),
            "invalid skit replay ID {id}"
        );
        order.push(id);
    }
    anyhow::bail!("unterminated skit preview order")
}

fn skip_disabled(image: &pe::Image<'_>) -> Result<BTreeSet<u16>> {
    let bytes = image.slice(0x55a02b, 84)?;
    let mut ids = BTreeSet::new();
    for at in (0..72).step_by(8) {
        ensure!(
            bytes[at..at + 2] == [0x81, 0xff],
            "changed skit skip comparison"
        );
        let branch = if at == 64 {
            [0x75, 4]
        } else {
            [0x74, (64 - at) as u8]
        };
        ensure!(bytes[at + 6..at + 8] == branch, "changed skit skip branch");
        ids.insert(u16::try_from(pe::word(bytes, at + 2)?)?);
    }
    Ok(ids)
}
