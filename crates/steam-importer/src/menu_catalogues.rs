//! Native Grade Shop and crafting rules in the shared named documents.
use crate::{
    output, pe,
    source::Source,
    text::{Bank, Menu},
};
use anyhow::{Result, ensure};
use resonance_content::{
    grade,
    menu_data::{self, crafting},
};
use std::{collections::BTreeMap, path::Path};

pub(crate) fn cook(source: &Source, output: &Path) -> Result<()> {
    let bytes = source.require("TOS_TEXT.TXB")?;
    let source = Menu {
        image: pe::Image::parse(&source.executable)?,
        bank: Bank::parse(&bytes)?,
        slot: source.language.slot(),
    };
    output::json(
        &output.join(menu_data::GRADE_SHOP_PATH),
        &grade_shop(&source)?,
    )?;
    let items: resonance_content::items::Document =
        serde_json::from_slice(&std::fs::read(output.join(resonance_content::items::PATH))?)?;
    let data = crafting(&source)?;
    data.validate(items.items.len())?;
    let document = crafting::document::Document::from_data(&data, &items.mappings)?;
    document
        .prepare(&items.mappings)?
        .validate(items.items.len())?;
    output::json(&output.join(menu_data::CRAFTING_PATH), &document)
}

fn grade_shop(source: &Menu<'_>) -> Result<grade::Shop> {
    let text = source.directory(0x8bfdb4)?;
    let labels = source.directory(0x8bfdcc)?;
    let shop = grade::Shop {
        options: grade::Benefit::ALL
            .into_iter()
            .zip(source.image.slice(0x77bc20, 26 * 12)?.chunks_exact(12))
            .map(|(benefit, row)| {
                Ok(grade::Purchase {
                    benefit,
                    price: pe::word(row, 0)?,
                    name: source.caption(text, pe::word(row, 4)?)?,
                    description: source.caption(text, pe::word(row, 8)?)?,
                    excludes: resonance_importer::grade_exclusions(benefit),
                })
            })
            .collect::<Result<_>>()?,
        labels: [
            "heading",
            "buy_cancel",
            "finish",
            "grade",
            "total_cost",
            "confirmation",
            "yes",
            "no",
        ]
        .into_iter()
        .enumerate()
        .map(|(index, name)| Ok((name.into(), source.caption(labels, index as u32)?)))
        .collect::<Result<_>>()?,
    };
    shop.validate()?;
    Ok(shop)
}

fn crafting(source: &Menu<'_>) -> Result<crafting::Data> {
    let text = source.directory(0x8c6048)?;
    let names = source.directory(0x8c5af4)?;
    let heading = source.caption(text, 7)?;
    let recipes = source
        .image
        .slice(0x8c4f28, 151 * 20)?
        .chunks_exact(20)
        .map(|row| {
            let mut ingredients = BTreeMap::new();
            let base = pe::half(row, 0)?;
            if base != 0 {
                ingredients.insert(base, 1);
            }
            for ingredient in row[4..].chunks_exact(4) {
                let item = pe::half(ingredient, 0)?;
                if item == 0 {
                    break;
                }
                *ingredients.entry(item).or_default() += u16::from(ingredient[2]);
            }
            Ok(crafting::Recipe {
                result: pe::half(row, 2)?,
                ingredients,
            })
        })
        .collect::<Result<_>>()?;
    let vendors = source
        .image
        .slice(0x8c5c28, 22 * 48)?
        .chunks_exact(48)
        .map(|row| {
            let count = (pe::half(row, 4)? as i16).max(0) as usize;
            ensure!(count <= 20, "Steam crafting vendor exceeds recipe slots");
            let name = source.caption(names, pe::word(row, 0)?)?;
            Ok(crafting::Vendor {
                // Unused vendors have blank localized names, as in the common importer.
                name: if name.is_empty() {
                    heading.clone()
                } else {
                    name
                },
                recipes: row[6..6 + count * 2]
                    .chunks_exact(2)
                    .map(|id| pe::half(id, 0))
                    .collect::<Result<_>>()?,
            })
        })
        .collect::<Result<_>>()?;
    Ok(crafting::Data {
        recipes,
        vendors,
        labels: crafting::Labels {
            heading,
            confirmation: source.caption(text, 0)?,
            yes: source.caption(text, 1)?,
            no: source.caption(text, 2)?,
            missing_materials: source.caption(text, 3)?,
            inventory_full: source.caption(text, 4)?,
        },
    })
}
