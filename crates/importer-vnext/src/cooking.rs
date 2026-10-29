//! Native meals and ingredient lists directly into named, editable recipes.
use crate::{dol, text};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    cooking::{self, Cook, Document, Group, Icon, Ingredient, Preferences, Recipe, Selection},
    library::Mappings,
    menu_data::MealEffect,
    overworld::PartyCharacter,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
use symphonia_constants::ItemId;

const RECIPES: u32 = 0x80215b0c;
const RECIPE_COUNT: usize = 24;
const GROUP_LISTS: u32 = 0x8021597c;
const GROUP_NAMES: u32 = 0x802159fc;
const EFFECTS: u32 = 0x80215a7c;
const LIKES: u32 = 0x80215ac4;
const DISLIKES: u32 = 0x80215ae8;
const LABELS: u32 = 0x80199e3c;

pub fn cook_cooking(extracted: &Path, library: &Path) -> Result<()> {
    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    let document = convert(&exe)?;
    let path = library.join(cooking::PATH);
    crate::output::json(&path, &document)?;
    Ok(())
}

fn ingredients(bytes: &[u8]) -> Result<Vec<u16>> {
    let count = usize::from(dol::u16(bytes, 0)?);
    let end = 2usize
        .checked_add(count * 2)
        .context("ingredient count overflow")?;
    Ok(bytes
        .get(2..end)
        .context("recipe ingredient count exceeds fixed slots")?
        .chunks_exact(2)
        .map(|row| u16::from_be_bytes([row[0], row[1]]))
        .collect())
}

fn item_name(id: u16) -> Result<String> {
    Ok(ItemId::try_from(id)?
        .resource_name()
        .with_context(|| format!("item {id} has no maintained name"))?
        .to_owned())
}

fn item_list(exe: &[u8], address: u32) -> Result<Vec<u16>> {
    ensure!(address != 0, "null cooking item list");
    let count = (dol::u16(dol::slice(exe, address, 2)?, 0)? as i16).max(0) as usize;
    Ok(dol::slice(exe, address + 2, count * 2)?
        .chunks_exact(2)
        .map(|row| u16::from_be_bytes([row[0], row[1]]))
        .collect())
}

fn lists(exe: &[u8], address: u32, count: usize) -> Result<Vec<Vec<u16>>> {
    dol::slice(exe, address, count * 4)?
        .chunks_exact(4)
        .map(|row| item_list(exe, dol::u32(row, 0)?))
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
    let mask = dol::u32(bytes, 0)?;
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
        recovery: (dol::u16(bytes, 4)? as i16).try_into()?,
        extras: ingredients(&bytes[6..])?,
    })
}

// Preserve the order of three mastery lists while merging repeated ingredients.
fn merge_extras(grades: &[Grade; 3]) -> Vec<(u16, u8)> {
    fn visit(
        lists: &[Vec<u16>; 3],
        at: [usize; 3],
        cache: &mut BTreeMap<[usize; 3], Vec<(u16, u8)>>,
    ) -> Vec<(u16, u8)> {
        if let Some(found) = cache.get(&at) {
            return found.clone();
        }
        let mut best: Option<Vec<(u16, u8)>> = None;
        for i in 0..3 {
            let Some(&value) = lists[i].get(at[i]) else {
                continue;
            };
            let mut next = at;
            let mut mask = 0;
            for j in 0..3 {
                if lists[j].get(at[j]) == Some(&value) {
                    next[j] += 1;
                    mask |= 1 << j;
                }
            }
            let mut result = vec![(value, mask)];
            result.extend(visit(lists, next, cache));
            if best
                .as_ref()
                .is_none_or(|current| result.len() < current.len())
            {
                best = Some(result);
            }
        }
        let result = best.unwrap_or_default();
        cache.insert(at, result.clone());
        result
    }
    let lists = std::array::from_fn(|i| {
        grades[i]
            .extras
            .iter()
            .copied()
            .filter(|&id| id != 0)
            .collect()
    });
    visit(&lists, [0; 3], &mut BTreeMap::new())
}

fn convert(exe: &[u8]) -> Result<Document> {
    let mappings: Mappings = serde_json::from_str(include_str!("recipe-names.json"))?;
    let group_names: Vec<String> =
        serde_json::from_str(include_str!("ingredient-group-names.json"))?;
    ensure!(
        mappings.iter().count() == RECIPE_COUNT && group_names.len() == 32,
        "cooking names do not cover source tables"
    );
    let mut recipes = BTreeMap::new();
    let mut used_groups = BTreeSet::new();
    for (id, row) in dol::slice(exe, RECIPES, RECIPE_COUNT * 700)?
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
                    stars: dol::u16(source, 0)?.try_into()?,
                    recovery: grades.map(|grade| grade.recovery),
                    extras,
                },
            );
        }
        let recipe = Recipe {
            text: serde_json::json!({
                "name": text::string(exe, dol::u32(row, 0)?)?,
                "description": text::string(exe, dol::u32(row, 4)?)?,
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

    let source_group_lists = lists(exe, GROUP_LISTS, 32)?;
    let source_group_names = dol::slice(exe, GROUP_NAMES, 32 * 4)?;
    let source_icons = dol::slice(exe, 0x80199e68, 32)?;
    let mut groups = BTreeMap::new();
    for id in used_groups {
        let name = text::string(exe, dol::u32(source_group_names, id * 4)?)?;
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
    let likes = lists(exe, LIKES, 9)?;
    let dislikes = lists(exe, DISLIKES, 9)?;
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
        labels.insert(name, text::pointer(exe, LABELS + (id as u32 + 1) * 4)?);
    }
    labels.insert("locked", text::string(exe, 0x80199e88)?);
    let effect_names = MealEffect::ALL
        .into_iter()
        .enumerate()
        .map(|(id, effect)| {
            Ok((
                effect,
                serde_json::to_value(text::pointer(exe, EFFECTS + id as u32 * 4)?)?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let ex_names: Mappings = serde_json::from_str(include_str!("ex-skill-names.json"))?;
    let bonus_id = dol::u16(dol::slice(exe, 0x8018d1f4, 2)?, 0)?;
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
