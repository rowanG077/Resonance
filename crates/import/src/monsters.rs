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
use std::{collections::BTreeMap, fs, path::Path};
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

pub(crate) fn prepare(
    extracted: &Path,
    output: &Path,
    executable: &[u8],
    catalogue: &Catalogue,
    ui: &inventory_ui::Catalogue,
) -> Result<MonsterBook> {
    let sources = crate::source_assets::Sources::read_with(extracted, executable)?;
    let files = extracted.join("files");
    let usual = fs::read(files.join(sources.usual))?;
    let archive = files.join(sources.enemy);
    ensure!(
        catalogue.records.len() == MONSTER_COUNT,
        "incomplete monster catalogue"
    );
    let mut records = Vec::new();
    let mut behaviors = crate::model_behavior::Bindings::new()?;
    for (index, row) in catalogue.records.iter().enumerate() {
        let id = u8::try_from(index)?;
        let bytes = crate::source_assets::enemy_package(&archive, &usual, u16::from(id))?;
        let record = source::read(&bytes)?;
        let metadata = record.metadata;
        let element = record.attack_element;
        ensure!(element <= 8, "invalid monster element");
        let elements = |test: fn(i8) -> bool| {
            Element::ALL
                .into_iter()
                .enumerate()
                .filter_map(|(i, element)| test(record.affinities[i]).then_some(element))
                .collect()
        };
        let item = |id| (id != 0).then_some(id);
        let mut preview = preview::menu(
            preview::from_package(&bytes, metadata, id, &source::clips(&bytes)?, output)?,
            metadata,
        )?;
        preview.behavior = behaviors.bind(crate::model_behavior::Subject::Monster(id));
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
            steal: item(record.steal),
            attack_element: element.checked_sub(1).map(|i| Element::ALL[usize::from(i)]),
            weaknesses: elements(|v| v == 1),
            resistances: elements(|v| v > 1),
            preview,
        };
        monster.validate(528)?;
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
