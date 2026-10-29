//! Native title rows, stable names and award rules to one editable document.
use crate::{dol, text};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    library::Mappings,
    overworld::PartyCharacter,
    titles::{Awards, Document, Entry, Statistic},
};
use std::{collections::BTreeMap, fs, path::Path};
use symphonia_constants::{ItemId, KnownId};

pub fn cook_titles(extracted: &Path, library: &Path) -> Result<()> {
    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    let document = convert(&exe)?;
    let path = library.join(resonance_content::titles::PATH);
    crate::output::json(&path, &document)?;
    Ok(())
}

fn convert(exe: &[u8]) -> Result<Document> {
    let mappings: BTreeMap<PartyCharacter, Mappings> =
        serde_json::from_str(include_str!("title-names.json"))?;
    let costumes: BTreeMap<u8, BTreeMap<u8, CostumeName>> =
        serde_json::from_str(include_str!("costume-names.json"))?;
    let awards: resonance_content::menu_data::TitleAwardRules =
        serde_json::from_str(include_str!("title-awards.json"))?;
    let starts = dol::slice(exe, 0x80210920, 9 * 2)?;
    let rows = dol::slice(exe, 0x80210934, 159 * 16)?;
    let ranges = (0..9)
        .map(|index| {
            let start = usize::from(u16::from_be_bytes(
                starts[index * 2..index * 2 + 2].try_into()?,
            ));
            let end = if index == 8 {
                159
            } else {
                usize::from(u16::from_be_bytes(
                    starts[index * 2 + 2..index * 2 + 4].try_into()?,
                ))
            };
            ensure!(
                start < end && end <= 159,
                "invalid title range for character {index}"
            );
            Ok(start..end)
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        ranges[0].start == 0 && ranges.windows(2).all(|pair| pair[0].end == pair[1].start),
        "title ranges do not cover native catalogue"
    );

    let bindings = dol::slice(exe, 0x80199d84, 3 * 9 * 2)?;
    let variants = dol::slice(exe, 0x8035c938, 3)?;
    let mut selected_costumes = BTreeMap::new();
    for (row, &variant) in bindings.chunks_exact(18).zip(variants) {
        ensure!(
            [1, 2, 4].contains(&variant),
            "unknown title costume variant {variant}"
        );
        for (member, bytes) in row.chunks_exact(2).enumerate() {
            let id = i16::from_be_bytes(bytes.try_into()?);
            if id == -1 {
                continue;
            }
            ensure!(
                (1..32).contains(&id) && (id as usize) <= ranges[member].len(),
                "invalid costume title {id} for character {}",
                member + 1
            );
            let name = &costumes
                .get(&(member as u8 + 1))
                .and_then(|names| names.get(&variant))
                .context("missing maintained costume name")?
                .name;
            ensure!(
                selected_costumes
                    .insert((member, id as usize), name.clone())
                    .is_none(),
                "duplicate costume title {id} for character {}",
                member + 1
            );
        }
    }

    let mut titles = BTreeMap::new();
    for (index, range) in ranges.iter().enumerate() {
        let member = PartyCharacter::try_from(index as u8 + 1)?;
        let names = mappings
            .get(&member)
            .context("missing title names for character")?;
        ensure!(
            names.iter().count() == range.len(),
            "title names do not cover character {index}"
        );
        for (offset, source) in rows[range.start * 16..range.end * 16]
            .chunks_exact(16)
            .enumerate()
        {
            let id = offset + 1;
            let name = names.target(id as u32)?;
            let growth = Statistic::ALL
                .into_iter()
                .zip(&source[8..15])
                .filter(|(_, value)| **value != 0)
                .map(|(stat, &value)| {
                    let scale = if matches!(stat, Statistic::Hp | Statistic::Tp) {
                        1.
                    } else {
                        10.
                    };
                    (stat, f64::from(value) / scale)
                })
                .collect();
            let entry = Entry {
                growth,
                costume: selected_costumes.get(&(index, id)).cloned(),
                text: serde_json::json!({
                    "name": text::string(exe, dol::u32(source, 0)?)?,
                    "description": text::string(exe, dol::u32(source, 4)?)?,
                }),
            };
            if let Some(previous) = titles.insert(name.to_owned(), entry.clone()) {
                ensure!(
                    previous == entry,
                    "shared title {name} has differing definitions"
                );
            }
        }
    }
    let item_names = Mappings::new(
        ItemId::ALL
            .iter()
            .filter_map(|&id| id.resource_name().map(|name| (id.value(), name.to_owned())))
            .collect(),
    )?;
    // The award recipe is authored knowledge; names and item IDs resolve once here.
    let awards = Awards::from_parts(&awards, &mappings, &item_names)?;
    Ok(Document {
        mappings,
        titles,
        awards,
    })
}

#[derive(serde::Deserialize)]
struct CostumeName {
    name: String,
}
