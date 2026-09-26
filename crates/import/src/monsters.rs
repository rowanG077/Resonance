//! Prepare the Monster Book from shared enemy records and visual resources.
use crate::{
    all_assets::{
        inventory_ui,
        monster_catalogue::{Catalogue, TextKind},
    },
    write_atomic,
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    menu_data::Element,
    monster::{MONSTER_COUNT, MONSTER_VERSION, Monster, MonsterBook},
};
use std::{collections::BTreeMap, path::Path};
pub(crate) mod preview;
mod source;

fn book(
    catalogue: &Catalogue,
    ui: &inventory_ui::Catalogue,
    records: Vec<Monster>,
) -> Result<MonsterBook> {
    let monster = &ui.inventory.monster;
    let mut labels: BTreeMap<String, String> = [
        ("number", TextKind::Number),
        ("hp", TextKind::Hp),
        ("tp", TextKind::Tp),
        ("unknown_item", TextKind::UnknownItem),
    ]
    .into_iter()
    .map(|(key, kind)| (key.into(), catalogue.direct_text(kind).into()))
    .collect();
    for (key, reference) in [
        ("title", monster.title),
        ("normal", monster.difficulties[0]),
        ("hard", monster.difficulties[1]),
        ("mania", monster.difficulties[2]),
        ("attack", monster.attack),
        ("experience", monster.experience),
        ("gald", monster.gald),
        ("defense", monster.defense),
        ("drops", monster.drops),
        ("steal", monster.steal),
        ("location", monster.location),
        ("attack_element", monster.attack_element),
        ("weak", monster.weak),
        ("strong", monster.strong),
        ("battle_rank", monster.battle_rank),
    ] {
        labels.insert(key.into(), ui.text(reference).into());
    }
    labels.insert(
        "unknown_stat".into(),
        catalogue
            .required_text(catalogue.stat_labels.unknown)?
            .into(),
    );
    Ok(MonsterBook { labels, records })
}

fn metadata(
    id: u8,
    record: source::Record<'_>,
    preview: resonance_content::model_preview::ModelPreview,
    catalogue: &Catalogue,
    ui: &inventory_ui::Catalogue,
) -> Result<Monster> {
    ensure!(record.attack_element <= 8, "invalid monster element");
    let row = &catalogue.records[usize::from(id)];
    let elements = |test: fn(i8) -> bool| {
        Element::ALL
            .into_iter()
            .enumerate()
            .filter_map(|(i, element)| test(record.affinities[i + 1]).then_some(element))
            .collect()
    };
    let item = |id| (id != 0).then_some(id);
    let monster = Monster {
        version: MONSTER_VERSION,
        id,
        name: catalogue.required_text(row.name)?.into(),
        location: catalogue.location(row.location)?.into(),
        category: ui
            .text(
                *ui.inventory
                    .monster
                    .categories
                    .get(usize::from(record.family))
                    .context("invalid monster family")?,
            )
            .into(),
        statistics: record.statistics,
        drops: record.drops.map(item),
        drop_chances: record.drop_chances,
        grade: record.grade,
        steal: item(record.steal),
        attack_element: record
            .attack_element
            .checked_sub(1)
            .map(|i| Element::ALL[usize::from(i)]),
        affinities: record.affinities,
        weaknesses: elements(|v| v == 1),
        resistances: elements(|v| v > 1),
        preview,
    };
    monster.validate(528)?;
    Ok(monster)
}

/// Refresh semantic enemy records from original source while retaining already
/// converted visual previews. Used for targeted development publications; no
/// model conversion or runtime compatibility path is involved.
pub fn publish_metadata(extracted: &Path, prepared: &Path, output: &Path) -> Result<Vec<String>> {
    let executable = std::fs::read(extracted.join("sys/main.dol"))?;
    let sources = crate::source_assets::Sources::read_with(extracted, &executable)?;
    let usual = std::fs::read(extracted.join("files").join(&sources.usual))?;
    let archive = extracted.join("files").join(&sources.enemy);
    let catalogue = crate::all_assets::monster_catalogue::read(&executable)?;
    let ui = inventory_ui::read(&executable)?;
    ensure!(
        catalogue.records.len() == MONSTER_COUNT,
        "incomplete monster catalogue"
    );
    let mut records = Vec::new();
    let mut paths = Vec::new();
    for id in 0..MONSTER_COUNT as u8 {
        let path = format!("monsters/{id:03}.json");
        let previous: serde_json::Value =
            serde_json::from_slice(&std::fs::read(prepared.join(&path))?)?;
        let preview = serde_json::from_value(previous["preview"].clone())?;
        let bytes = crate::source_assets::enemy_package(&archive, &usual, u16::from(id))?;
        let monster = metadata(id, source::read(&bytes)?, preview, &catalogue, &ui)?;
        write_atomic(&output.join(&path), &serde_json::to_vec_pretty(&monster)?)?;
        records.push(monster);
        paths.push(path);
    }
    let book = book(&catalogue, &ui, records)?;
    book.validate(528)?;
    let mut menu: serde_json::Value =
        serde_json::from_slice(&std::fs::read(prepared.join("game/menu-data.json"))?)?;
    menu["monsters"] = serde_json::to_value(book)?;
    write_atomic(
        &output.join("game/menu-data.json"),
        &serde_json::to_vec_pretty(&menu)?,
    )?;
    paths.push("game/menu-data.json".into());
    Ok(paths)
}

pub(crate) fn prepare(
    extracted: &Path,
    output: &Path,
    sources: &crate::source_assets::Sources,
    usual: &[u8],
    catalogue: &Catalogue,
    ui: &inventory_ui::Catalogue,
) -> Result<MonsterBook> {
    let files = extracted.join("files");
    let archive = files.join(&sources.enemy);
    ensure!(
        catalogue.records.len() == MONSTER_COUNT,
        "incomplete monster catalogue"
    );
    let mut records = Vec::new();
    let mut behaviors = crate::model_behavior::Bindings::new()?;
    for index in 0..catalogue.records.len() {
        let id = u8::try_from(index)?;
        let bytes = crate::source_assets::enemy_package(&archive, usual, u16::from(id))?;
        let record = source::read(&bytes)?;
        let metadata = record.metadata;
        let mut preview = preview::menu(
            preview::from_package(&bytes, metadata, id, &source::clips(&bytes)?, output)?,
            metadata,
        )?;
        preview.behavior = behaviors.bind(crate::model_behavior::Subject::Monster(id));
        let monster = self::metadata(id, record, preview, catalogue, ui)?;
        crate::battle_model::publish_enemy(&bytes, id, &monster.preview, output, output)?;
        write_atomic(
            &output.join(format!("monsters/{id:03}.json")),
            &serde_json::to_vec_pretty(&monster)?,
        )?;
        println!("Prepared monster {id}: {}", monster.name);
        records.push(monster);
    }
    behaviors.finish(crate::model_behavior::Catalogue::Monsters)?;
    book(catalogue, ui, records)
}

#[cfg(test)]
mod tests;
