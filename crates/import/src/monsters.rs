//! Prepare the Monster Book from shared enemy records and visual resources.
use crate::{cooked::Source, write_atomic};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    menu_data::Element,
    monster::{MONSTER_COUNT, MONSTER_VERSION, Monster, MonsterStats},
};
use serde::Deserialize;
use std::{fs, path::Path};
pub(crate) mod preview;

pub(crate) fn book(output: &Path, disc: u8) -> Result<resonance_content::monster::MonsterBook> {
    Ok(resonance_content::monster::MonsterBook {
        labels: crate::menu::monster_catalogue(output, disc)?.labels,
        records: (0..MONSTER_COUNT)
            .map(|id| {
                serde_json::from_slice(&fs::read(output.join(format!("monsters/{id:03}.json")))?)
                    .context("monster assets need preparation; run cook-monsters")
            })
            .collect::<Result<_>>()?,
    })
}

#[derive(Deserialize)]
struct Settings {
    combat: Combat,
    rewards: Rewards,
}
#[derive(Deserialize)]
struct Combat {
    attack_element: u8,
    element_affinities: [u8; 9],
    family: usize,
    variant_count: usize,
}
#[derive(Deserialize)]
struct Rewards {
    experience: u32,
    gald: u32,
    drops: [u16; 2],
    steal: u16,
}
#[derive(Deserialize)]
struct Statistics {
    hp: u32,
    tp: u16,
    attack: u16,
    defense: u16,
}

fn record<T: serde::de::DeserializeOwned>(source: &Source<'_>, id: u8, name: &str) -> Result<T> {
    let (_, bytes) = source.resolve(&format!("battle/all/enemy-{id}/{name}.json"))?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub fn cook(extracted: &Path, output: &Path, selected: &[u8]) -> Result<()> {
    ensure!(
        selected.iter().all(|&id| usize::from(id) < MONSTER_COUNT),
        "unknown monster requested"
    );
    let disc = crate::disc_number(extracted)?;
    let sources = crate::battle::all::Sources::read(extracted)?;
    let source = Source::open(output, disc, &sources.enemy)?;
    let catalogue = crate::menu::monster_catalogue(output, disc)?;
    for (index, row) in catalogue.records.into_iter().enumerate() {
        let id = u8::try_from(index)?;
        ensure!(row.slot == index, "unordered cooked monster catalogue");
        if !selected.is_empty() && !selected.contains(&id) {
            continue;
        }
        let settings: Settings = record(&source, id, "header-4")?;
        let stats: Statistics = record(&source, id, "header-6")?;
        let mut statistics = vec![MonsterStats {
            hp: stats.hp,
            tp: stats.tp,
            attack: stats.attack,
            defense: stats.defense,
            experience: settings.rewards.experience,
            gald: settings.rewards.gald,
        }];
        if settings.combat.variant_count != 0 {
            #[derive(Deserialize)]
            struct Variants {
                variants: Vec<MonsterStats>,
            }
            let variants: Variants = record(&source, id, "variants")?;
            ensure!(
                variants.variants.len() == settings.combat.variant_count,
                "enemy variant count differs from settings"
            );
            statistics.extend(variants.variants);
        }
        let element = settings.combat.attack_element;
        ensure!(element <= 8, "invalid monster element");
        let elements = |test: fn(i8) -> bool| {
            Element::ALL
                .into_iter()
                .enumerate()
                .filter_map(|(i, element)| {
                    test(settings.combat.element_affinities[i] as i8).then_some(element)
                })
                .collect()
        };
        let item = |id| (id != 0).then_some(id);
        let monster = Monster {
            version: MONSTER_VERSION,
            id,
            name: row.name.context("missing monster name")?,
            location: row.location,
            category: catalogue
                .categories
                .get(settings.combat.family)
                .context("invalid monster family")?
                .clone(),
            statistics,
            drops: settings.rewards.drops.map(item),
            steal: item(settings.rewards.steal),
            attack_element: element.checked_sub(1).map(|i| Element::ALL[usize::from(i)]),
            weaknesses: elements(|v| v == 1),
            resistances: elements(|v| v > 1),
            preview: preview::bind(&source, id)?,
        };
        monster.validate(528)?;
        write_atomic(
            &output.join(format!("monsters/{id:03}.json")),
            &serde_json::to_vec_pretty(&monster)?,
        )?;
        println!("Prepared monster {id}: {}", monster.name);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
