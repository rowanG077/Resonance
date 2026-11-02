//! Native meals and ingredient lists directly into named, editable recipes.
use crate::{pe, source::Source, text::Bank};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    cooking::{self, Cook, Document, Group, Icon, Ingredient, Preferences, Recipe, Selection},
    library::Mappings,
    menu_data::MealEffect,
    overworld::PartyCharacter,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
use symphonia_constants::ItemId;

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

fn ingredients(bytes: &[u8]) -> Result<Vec<u16>> {
    let count = usize::from(pe::half(bytes, 0)?);
    let end = 2usize
        .checked_add(count * 2)
        .context("ingredient count overflow")?;
    Ok(bytes
        .get(2..end)
        .context("recipe ingredient count exceeds fixed slots")?
        .chunks_exact(2)
        .map(|row| u16::from_le_bytes([row[0], row[1]]))
        .collect())
}

fn item_name(id: u16) -> Result<String> {
    Ok(ItemId::try_from(id)?
        .resource_name()
        .with_context(|| format!("item {id} has no maintained name"))?
        .to_owned())
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

fn named_ingredient(id: u16, from: u8, through: u8, groups: &[String]) -> Result<Ingredient> {
    let (item, any) = match id {
        1..30000 => (Some(item_name(id)?), None),
        30000..30032 => (None, Some(groups[usize::from(id - 30000)].clone())),
        _ => anyhow::bail!("unsupported cooking ingredient {id}"),
    };
    if from == 0 && through == 8 && any.is_none() {
        Ok(Ingredient::Item(item.unwrap()))
    } else {
        Ok(Ingredient::Selection(Selection {
            item,
            any,
            from_mastery: from,
            through_mastery: through,
        }))
    }
}

#[derive(Clone)]
struct Grade {
    effects: Vec<MealEffect>,
    recovery: u8,
    extras: Vec<u16>,
}

fn grade(bytes: &[u8]) -> Result<Grade> {
    let mask = pe::word(bytes, 0)?;
    ensure!(
        mask & !0xfff == 0,
        "unsupported cooking effect bits {mask:#x}"
    );
    Ok(Grade {
        effects: MealEffect::ALL
            .into_iter()
            .enumerate()
            .filter_map(|(bit, effect)| (mask & (1 << bit) != 0).then_some(effect))
            .collect(),
        recovery: (pe::half(bytes, 4)? as i16).try_into()?,
        extras: ingredients(&bytes[6..])?,
    })
}

// Preserve the order of three mastery lists while merging repeated ingredients.
fn merge_extras(grades: &[Grade; 3]) -> Vec<(u16, u8)> {
    let lists = std::array::from_fn(|i| {
        grades[i]
            .extras
            .iter()
            .copied()
            .filter(|&id| id != 0)
            .collect()
    });
    resonance_importer::merge_cooking_ingredients(&lists)
}

fn convert(image: &pe::Image<'_>, bank: &Bank<'_>, slot: u32) -> Result<Document> {
    let mappings: Mappings =
        serde_json::from_str(include_str!("../../importer/src/recipe-names.json"))?;
    let group_names: Vec<String> = serde_json::from_str(include_str!(
        "../../importer/src/ingredient-group-names.json"
    ))?;
    ensure!(
        mappings.iter().count() == RECIPE_COUNT && group_names.len() == 32,
        "cooking names do not cover source tables"
    );
    let directory = |at| pe::word(image.slice(at + slot * 4, 4)?, 0);
    let caption = |at: u32, index: u32| {
        bank.menu(
            pe::word(image.slice(at + index * 4, 4)?, 0)?,
            encoding_rs::WINDOWS_1252,
        )
    };
    let recipe_text = directory(0x8c7698)?;
    let mut recipes = BTreeMap::new();
    let mut used_groups = BTreeSet::new();
    for (id, row) in image
        .slice(RECIPES, RECIPE_COUNT * 700)?
        .chunks_exact(700)
        .enumerate()
    {
        let required = ingredients(&row[8..16])?
            .into_iter()
            .map(|value| {
                if (30000..30032).contains(&value) {
                    used_groups.insert(usize::from(value - 30000));
                }
                named_ingredient(value, 0, 8, &group_names)
            })
            .collect::<Result<Vec<_>>>()?;
        let mut cooks = BTreeMap::new();
        let mut recipe_effects = None;
        for (member, source) in row[16..].chunks_exact(76).enumerate() {
            let grades: [Grade; 3] = source[4..]
                .chunks_exact(24)
                .map(grade)
                .collect::<Result<Vec<_>>>()?
                .try_into()
                .map_err(|_| anyhow::anyhow!("recipe has wrong mastery grade count"))?;
            let effects = &grades[0].effects;
            ensure!(
                grades.iter().all(|grade| &grade.effects == effects),
                "recipe has differing effects between mastery grades"
            );
            if let Some(previous) = &recipe_effects {
                ensure!(
                    previous == effects,
                    "recipe has differing effects between cooks"
                );
            } else {
                recipe_effects = Some(effects.clone());
            }
            let mut extras = Vec::new();
            for (value, mask) in merge_extras(&grades) {
                if (30000..30032).contains(&value) {
                    used_groups.insert(usize::from(value - 30000));
                }
                let mut grade = 0;
                while grade < 3 {
                    if mask & (1 << grade) == 0 {
                        grade += 1;
                        continue;
                    }
                    let start = grade;
                    while grade < 3 && mask & (1 << grade) != 0 {
                        grade += 1;
                    }
                    extras.push(named_ingredient(
                        value,
                        start * 3,
                        grade * 3 - 1,
                        &group_names,
                    )?);
                }
            }
            cooks.insert(
                PartyCharacter::try_from(member as u8 + 1)?,
                Cook {
                    stars: pe::half(source, 0)?.try_into()?,
                    recovery: grades.map(|grade| grade.recovery),
                    extras,
                },
            );
        }
        let recipe = Recipe {
            text: serde_json::json!({
                "name": caption(recipe_text, pe::word(row, 0)?)?,
                "description": caption(recipe_text, pe::word(row, 4)?)?,
            }),
            effects: recipe_effects.context("recipe has no cooks")?,
            required,
            cooks,
        };
        ensure!(
            recipes
                .insert(mappings.target(id as u32)?.to_owned(), recipe)
                .is_none(),
            "duplicate recipe name"
        );
    }

    let source_group_lists = lists(image, GROUP_LISTS, 32)?;
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
    let mut groups = BTreeMap::new();
    for id in used_groups {
        let name = caption(source_group_names, id as u32)?;
        groups.insert(
            group_names[id].clone(),
            Group {
                name: serde_json::to_value(name)?,
                icon: Icon::from_index(source_icons[id])?,
                items: source_group_lists[id]
                    .iter()
                    .map(|&item| item_name(item))
                    .collect::<Result<Vec<_>>>()?,
            },
        );
    }
    let likes = lists(image, LIKES, 9)?;
    let dislikes = lists(image, DISLIKES, 9)?;
    let mut preferences = BTreeMap::new();
    for (id, (likes, dislikes)) in likes.into_iter().zip(dislikes).enumerate() {
        if likes.is_empty() && dislikes.is_empty() {
            continue;
        }
        preferences.insert(
            PartyCharacter::try_from(id as u8 + 1)?,
            Preferences {
                likes: likes
                    .into_iter()
                    .map(item_name)
                    .collect::<Result<Vec<_>>>()?,
                dislikes: dislikes
                    .into_iter()
                    .map(item_name)
                    .collect::<Result<Vec<_>>>()?,
            },
        );
    }
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
    let ex_names: Mappings =
        serde_json::from_str(include_str!("../../importer/src/ex-skill-names.json"))?;
    let bonus_id = pe::half(image.slice(0x48e1ae, 2)?, 0)?;
    Ok(Document {
        mappings,
        recipes,
        groups,
        preferences,
        genis_bonus_ex_skill: ex_names.target(u32::from(bonus_id))?.to_owned(),
        labels: serde_json::to_value(labels)?,
        effect_names,
    })
}
