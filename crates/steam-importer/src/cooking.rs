//! Native meals and ingredient lists directly into named, editable recipes.
use crate::{pe, source::Source, text::Bank};
use anyhow::{Result, ensure};
use resonance_content::{
    cooking::{self, Document},
    menu_data::MealEffect,
};
use std::{collections::BTreeMap, path::Path};

const RECIPES: u32 = 0x8c76b0;
const RECIPE_COUNT: usize = 24;
const GROUP_LISTS: u32 = 0x8c6c30;
const GROUP_NAMES: u32 = 0x8c7060;
const EFFECTS: u32 = 0x8c7200;
const LIKES: u32 = 0x8c6d0c;
const DISLIKES: u32 = 0x8c6d30;
const LABELS: u32 = 0x8cb850;

pub(crate) fn cook(source: &Source, library: &Path) -> Result<()> {
    let image = pe::Image::parse(&source.executable)?;
    let bytes = source.require("TOS_TEXT.TXB")?;
    let bank = Bank::parse(&bytes)?;
    let document = convert(&image, &bank, source.language.slot())?;
    crate::output::json(&library.join(cooking::PATH), &document)
}

fn item_list(image: &pe::Image<'_>, address: u32) -> Result<Vec<u16>> {
    // These three authored empty preferences live in the PE's zero-filled tail.
    if [0x1c30d5c, 0x1c30dd0, 0x1c30dd4].contains(&address) {
        ensure!(
            image.is_zero_initialized(address, 2)?,
            "empty preference is not zero-initialized"
        );
        return Ok(Vec::new());
    }
    ensure!(address != 0, "null cooking item list");
    let count = (pe::half(image.slice(address, 2)?, 0)? as i16).max(0) as usize;
    Ok(image
        .slice(address + 2, count * 2)?
        .chunks_exact(2)
        .map(|row| u16::from_le_bytes([row[0], row[1]]))
        .collect())
}

fn lists(image: &pe::Image<'_>, address: u32, count: usize) -> Result<Vec<Vec<u16>>> {
    image
        .slice(address, count * 4)?
        .chunks_exact(4)
        .map(|row| item_list(image, pe::word(row, 0)?))
        .collect()
}

fn convert(image: &pe::Image<'_>, bank: &Bank<'_>, slot: u32) -> Result<Document> {
    let directory = |at| pe::word(image.slice(at + slot * 4, 4)?, 0);
    let caption = |at: u32, index: u32| {
        bank.menu(
            pe::word(image.slice(at + index * 4, 4)?, 0)?,
            encoding_rs::WINDOWS_1252,
        )
    };
    let recipe_text = directory(0x8c7698)?;
    let source_group_names = directory(GROUP_NAMES)?;
    let mut source_icons = Vec::new();
    for i in 0..7 {
        let instruction = image.slice(0x4f60ab + i * 8, 8)?;
        ensure!(
            instruction[..4] == [0xc7, 0x44, 0x24, 0x64 + i as u8 * 4],
            "unknown cooking icons"
        );
        source_icons.extend_from_slice(&instruction[4..]);
    }
    source_icons.extend_from_slice(image.slice(0x4f60ea, 4)?);
    let mut fields = vec![(0, 4), (4, 4)];
    fields.extend((8..16).step_by(2).map(|at| (at, 2)));
    for member in 0..9 {
        let at = 16 + member * 76;
        fields.push((at, 2));
        for grade in 0..3 {
            let at = at + 4 + grade * 24;
            fields.push((at, 4));
            fields.extend((at + 4..at + 24).step_by(2).map(|at| (at, 2)));
        }
    }
    let rows =
        crate::battle_normal::canonical(image.slice(RECIPES, RECIPE_COUNT * 700)?, 700, &fields)?;
    let mut document = resonance_importer::cooking_document(
        resonance_importer::CookingSource {
            recipes: &rows,
            group_items: &lists(image, GROUP_LISTS, 32)?,
            group_icons: &source_icons,
            likes: &lists(image, LIKES, 9)?,
            dislikes: &lists(image, DISLIKES, 9)?,
            bonus_skill: pe::half(image.slice(0x48e1ae, 2)?, 0)?,
        },
        |index| caption(recipe_text, index),
        |id| caption(source_group_names, id as u32),
    )?;
    let mut labels = BTreeMap::new();
    for (id, name) in [
        "cook",
        "required",
        "additional",
        "success",
        "failure",
        "no_effect",
        "missing",
        "full",
        "unknown",
        "result_join",
    ]
    .into_iter()
    .enumerate()
    {
        labels.insert(
            name,
            caption(directory(LABELS)?, [1, 2, 3, 5, 6, 7, 9, 10, 11, 12][id])?,
        );
    }
    labels.insert("locked", caption(directory(LABELS)?, 4)?);
    let effect_names = MealEffect::ALL
        .into_iter()
        .enumerate()
        .map(|(id, effect)| {
            Ok((
                effect,
                serde_json::to_value(caption(directory(EFFECTS)?, id as u32)?)?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    document.labels = serde_json::to_value(labels)?;
    document.effect_names = effect_names;
    Ok(document)
}
