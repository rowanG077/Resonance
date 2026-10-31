//! Native Grade Shop and crafting tables directly into separate menu documents.
use crate::{dol, text};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    grade::{Benefit, Purchase, Shop},
    library::Mappings,
    menu_data::{
        crafting::{
            Labels,
            document::{Document as Crafting, NamedRecipe, NamedVendor},
        },
        document::unique_name,
    },
};
use std::{collections::BTreeMap, fs, path::Path};
use symphonia_constants::ItemId;

const GRADE_OPTIONS: u32 = 0x8019d130;
const GRADE_LABELS: u32 = 0x8019bdc4;
const RECIPES: u32 = 0x80220cd0;
const VENDORS: u32 = 0x8022189c;

fn exclusions(benefit: Benefit) -> Vec<Benefit> {
    use Benefit::*;
    let group: &[Benefit] = match benefit {
        IncreasedHp | MinimumHp => &[IncreasedHp, MinimumHp],
        ComboExperience | HalfExperience | DoubleExperience | TenfoldExperience => &[
            ComboExperience,
            HalfExperience,
            DoubleExperience,
            TenfoldExperience,
        ],
        _ => &[],
    };
    group
        .iter()
        .copied()
        .filter(|&other| other != benefit)
        .collect()
}

fn grade_shop(exe: &[u8]) -> Result<Shop> {
    let mut labels: BTreeMap<_, _> = [
        "buy_cancel",
        "total_cost",
        "finish",
        "confirmation",
        "yes",
        "no",
    ]
    .into_iter()
    .zip(dol::slice(exe, GRADE_LABELS, 24)?.chunks_exact(4))
    .map(|(key, row)| Ok((key.to_owned(), text::string(exe, dol::u32(row, 0)?)?)))
    .collect::<Result<_>>()?;
    for (key, address) in [
        ("heading", 0x8019d2c8),
        ("grade", 0x8035ce18),
        ("balance_format", 0x8035ce2c),
        ("price_format", 0x8035ce30),
    ] {
        labels.insert(key.into(), text::string(exe, address)?);
    }
    let options = Benefit::ALL
        .into_iter()
        .zip(dol::slice(exe, GRADE_OPTIONS, Benefit::ALL.len() * 12)?.chunks_exact(12))
        .map(|(benefit, row)| {
            Ok(Purchase {
                benefit,
                price: dol::u32(row, 0)?,
                name: text::string(exe, dol::u32(row, 4)?)?,
                description: text::string(exe, dol::u32(row, 8)?)?,
                excludes: exclusions(benefit),
            })
        })
        .collect::<Result<_>>()?;
    let shop = Shop { options, labels };
    shop.validate()?;
    Ok(shop)
}

fn recipe(row: &[u8], items: &BTreeMap<u16, &str>) -> Result<Option<NamedRecipe>> {
    let result = u16::try_from(dol::u16(row, 2)? as i16)?;
    let mut ingredients = BTreeMap::<u16, u16>::new();
    let first = dol::u16(row, 0)? as i16;
    if first != 0 {
        ingredients.insert(u16::try_from(first)?, 1);
    }
    for slot in row[4..].chunks_exact(4) {
        let id = dol::u16(slot, 0)? as i16;
        if id == 0 {
            break;
        }
        let count = slot[2] as u16;
        ensure!(count > 0, "zero crafting ingredient count");
        let value = ingredients.entry(u16::try_from(id)?).or_default();
        *value = value
            .checked_add(count)
            .context("crafting ingredient count overflow")?;
    }
    if result == 0 && ingredients.is_empty() {
        return Ok(None);
    }
    let name = |id| {
        items
            .get(&id)
            .copied()
            .with_context(|| format!("unnamed crafting item {id}"))
    };
    Ok(Some(NamedRecipe {
        result: name(result)?.into(),
        ingredients: ingredients
            .into_iter()
            .map(|(id, count)| Ok((name(id)?.into(), count)))
            .collect::<Result<_>>()?,
    }))
}

fn crafting(exe: &[u8]) -> Result<Crafting> {
    let [
        heading,
        confirmation,
        yes,
        no,
        missing_materials,
        inventory_full,
    ] = text::table(exe, 0x8019bb08)?;
    let labels = Labels {
        heading,
        confirmation,
        yes,
        no,
        missing_materials,
        inventory_full,
    };
    let items: BTreeMap<_, _> = ItemId::ALL
        .iter()
        .filter_map(|id| id.resource_name().map(|name| (id.raw(), name)))
        .collect();
    let mut recipes = BTreeMap::new();
    let mut names = BTreeMap::new();
    for (id, row) in dol::slice(exe, RECIPES, 151 * 20)?
        .chunks_exact(20)
        .enumerate()
    {
        if let Some(recipe) = recipe(row, &items)? {
            let name = unique_name(&recipe.result, &recipes);
            recipes.insert(name.clone(), recipe);
            names.insert(id, name);
        }
    }
    let mut vendors = BTreeMap::new();
    let mut mappings = BTreeMap::new();
    for (id, row) in dol::slice(exe, VENDORS, 22 * 48)?
        .chunks_exact(48)
        .enumerate()
    {
        let pointer = dol::u32(row, 0)?;
        let name = if pointer == 0 {
            String::new()
        } else {
            text::string(exe, pointer)?
        };
        let name = if name.is_empty() {
            labels.heading.clone()
        } else {
            name
        };
        let count = (dol::u16(row, 4)? as i16).max(0) as usize;
        ensure!(
            count <= 20,
            "crafting vendor recipe count exceeds its slots"
        );
        let offered = row[6..6 + count * 2]
            .chunks_exact(2)
            .map(|slot| {
                let recipe = u16::try_from(dol::u16(slot, 0)? as i16)? as usize;
                names
                    .get(&recipe)
                    .cloned()
                    .context("vendor requests an empty recipe")
            })
            .collect::<Result<Vec<_>>>()?;
        let key = unique_name(&name, &vendors);
        vendors.insert(
            key.clone(),
            NamedVendor {
                name,
                recipes: offered,
            },
        );
        mappings.insert(id as u32, key);
    }
    Ok(Crafting {
        mappings: Mappings::new(mappings)?,
        vendors,
        recipes,
        labels,
    })
}

pub fn cook_menu_catalogues(extracted: &Path, library: &Path) -> Result<()> {
    let grade_path = library.join("game/menu/grade-shop.json");
    let crafting_path = library.join("game/menu/crafting.json");
    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    let grade = grade_shop(&exe)?;
    let crafting = crafting(&exe)?;
    for (path, data) in [
        (grade_path, resonance_asset_writer::json::encode(&grade)?),
        (
            crafting_path,
            resonance_asset_writer::json::encode(&crafting)?,
        ),
    ] {
        crate::output::write(&path, &data)?;
    }
    Ok(())
}
