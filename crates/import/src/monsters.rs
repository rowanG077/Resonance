//! Prepare the Monster Book from shared enemy records and visual resources.
use crate::all_assets::{
    inventory_ui,
    monster_catalogue::{Catalogue, TextKind},
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
        unseen_count_group: row.unseen_count_group,
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
        steal_chance: record.steal_chance,
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

pub(crate) struct Prepared {
    pub book: MonsterBook,
    pub enemies: BTreeMap<u8, crate::battle_model::PreparedEnemy>,
}

pub(crate) fn prepare(
    extracted: &Path,
    output: &Path,
    sources: &crate::source_assets::Sources,
    usual: &[u8],
    catalogue: &Catalogue,
    ui: &inventory_ui::Catalogue,
    shared_effects: &crate::battle_effect::SharedPalettes,
) -> Result<Prepared> {
    let voices = crate::battle_voice::read(usual)?;
    let mut shared = shared_effects.clone();
    let files = extracted.join("files");
    let guard_bonus = crate::battle_recoil::guard_defaults(&crate::rel::Rel::read(
        &files.join(&sources.module),
    )?)?[0];
    let archive = files.join(&sources.enemy);
    ensure!(
        catalogue.records.len() == MONSTER_COUNT,
        "incomplete monster catalogue"
    );
    let mut records = Vec::new();
    let mut enemies = BTreeMap::new();
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
        let enemy = crate::battle_model::prepare_enemy(
            &bytes,
            usual,
            &voices,
            guard_bonus,
            id,
            &monster.preview,
            &mut shared,
            output,
        )?;
        enemies.insert(id, enemy);
        println!("Prepared monster {id}: {}", monster.name);
        records.push(monster);
    }
    behaviors.finish(crate::model_behavior::Catalogue::Monsters)?;
    Ok(Prepared {
        book: book(catalogue, ui, records)?,
        enemies,
    })
}

/// Event actors use the same authored body layers and clips as the catalogue.
pub(crate) fn event_model(
    bytes: &[u8],
    id: u8,
    output: &Path,
) -> Result<resonance_content::battle_event::Model> {
    let record = source::read(bytes)?;
    let preview = preview::menu(
        preview::from_package(bytes, record.metadata, id, &source::clips(bytes)?, output)?,
        record.metadata,
    )?;
    let members = crate::model_preview::PointerMembers::new(bytes, 0x18..0x1e8)?;
    let rig = crate::battle_model::rig(
        members.model(0x18)?.context("missing event enemy body")?,
        crate::battle_model::RigKind::Body,
    )?;
    Ok(resonance_content::battle_event::Model {
        skeleton: rig.skeleton,
        parts: preview
            .parts
            .into_iter()
            .filter(|part| part.attached_to.is_none())
            .map(|part| part.scene)
            .collect(),
    })
}

#[cfg(test)]
mod tests;
