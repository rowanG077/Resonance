//! Complete recipe records and the menu's authored ingredient/result bindings.
use super::text::{TextPool, TextRef, TextSource};
use crate::{
    dol,
    read::{u16 as half, u32 as word},
};
use anyhow::{Context, Result, ensure};
use resonance_content::menu_data::MealEffect;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[cfg(test)]
use std::path::Path;

const RECIPES: u32 = 0x80215b0c;
const RECIPE_COUNT: usize = 24;
const RECIPE_STRIDE: usize = 700;
const GROUP_LISTS: u32 = 0x8021597c;
const GROUP_NAMES: u32 = 0x802159fc;
const EFFECTS: u32 = 0x80215a7c;
const LIKES: u32 = 0x80215ac4;
const DISLIKES: u32 = 0x80215ae8;
const LABELS: u32 = 0x80199e3c;
const CATEGORIES: u32 = 0x80199e68;
const LOCKED: u32 = 0x80199e88;
const FORMATS: [u32; 5] = [0x8035c970, 0x8035c978, 0x8035c980, 0x8035c988, 0x8035c990];
const BONUS_SKILL: u32 = 0x8018d1f4;

super::ordered! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub(crate) enum Label {
        Title,
        Cook,
        Required,
        Additional,
        Success,
        Failure,
        NoEffect,
        Missing,
        Full,
        Unknown,
        ResultJoin,
    }
}
impl Label {
    pub(crate) const RUNTIME: [(Self, &'static str); 10] = [
        (Self::Cook, "cook"),
        (Self::Required, "required"),
        (Self::Additional, "additional"),
        (Self::Success, "success"),
        (Self::Failure, "failure"),
        (Self::NoEffect, "no_effect"),
        (Self::Missing, "missing"),
        (Self::Full, "full"),
        (Self::Unknown, "unknown"),
        (Self::ResultJoin, "result_join"),
    ];
}

/// Keep the authored IDs, including empty slots and unknown ingredient encodings.
fn ingredients(bytes: &[u8]) -> Result<Vec<u16>> {
    let count = usize::from(half(bytes, 0)?);
    Ok(bytes
        .get(2..2 + count * 2)
        .context("recipe ingredient count exceeds its fixed slots")?
        .chunks_exact(2)
        .map(|value| u16::from_be_bytes([value[0], value[1]]))
        .collect())
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Grade {
    pub effects: Vec<MealEffect>,
    pub unknown_effect_bits: u32,
    pub recovery: i16,
    pub extras: Vec<u16>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Cook {
    pub base_stars: u16,
    pub storage: u16,
    pub grades: [Grade; 3],
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Recipe {
    pub name: Option<TextRef>,
    pub description: Option<TextRef>,
    pub required: Vec<u16>,
    pub cooks: [Cook; 9],
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Group {
    pub name: Option<TextRef>,
    pub category: u8,
    pub items: Option<Vec<u16>>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Preference {
    pub likes: Option<Vec<u16>>,
    pub dislikes: Option<Vec<u16>>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Formats {
    pub item_count: TextRef,
    pub recipe_counter: TextRef,
    pub result_heading: TextRef,
    pub recovery: TextRef,
    pub effect: TextRef,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Catalogue {
    texts: Vec<String>,
    pub recipes: Vec<Recipe>,
    pub groups: Vec<Group>,
    pub preferences: [Preference; 9],
    pub effects: [Option<TextRef>; 12],
    pub labels: BTreeMap<Label, Option<TextRef>>,
    pub locked: TextRef,
    pub formats: Formats,
    pub bonus_skill: u16,
}
impl Catalogue {
    pub(crate) fn text(&self, reference: TextRef) -> &str {
        &self.texts[reference.0]
    }
    pub(crate) fn required_text(&self, reference: Option<TextRef>) -> Result<&str> {
        Ok(self.text(reference.context("null required cooking text")?))
    }
    pub(crate) fn label(&self, label: Label) -> Result<&str> {
        self.required_text(self.labels[&label])
    }
}

fn item_list(executable: &[u8], address: u32) -> Result<Option<Vec<u16>>> {
    if address == 0 {
        return Ok(None);
    }
    let count = (half(dol::slice(executable, address, 2)?, 0)? as i16).max(0) as usize;
    Ok(Some(
        dol::slice(executable, address + 2, count * 2)?
            .chunks_exact(2)
            .map(|value| u16::from_be_bytes([value[0], value[1]]))
            .collect(),
    ))
}

fn parse(executable: &[u8]) -> Result<(Catalogue, Vec<TextSource>)> {
    let mut text = TextPool::default();
    let recipes = dol::slice(executable, RECIPES, RECIPE_COUNT * RECIPE_STRIDE)?
        .chunks_exact(RECIPE_STRIDE)
        .map(|row| {
            Ok(Recipe {
                name: text.reference(executable, word(row, 0)?)?,
                description: text.reference(executable, word(row, 4)?)?,
                required: ingredients(&row[8..16])?,
                cooks: row[16..]
                    .chunks_exact(76)
                    .map(|cook| {
                        Ok(Cook {
                            base_stars: half(cook, 0)?,
                            storage: half(cook, 2)?,
                            grades: cook[4..]
                                .chunks_exact(24)
                                .map(|grade| {
                                    let mask = word(grade, 0)?;
                                    Ok(Grade {
                                        effects: MealEffect::ALL
                                            .into_iter()
                                            .enumerate()
                                            .filter_map(|(bit, effect)| {
                                                (mask & (1 << bit) != 0).then_some(effect)
                                            })
                                            .collect(),
                                        unknown_effect_bits: mask & !0xfff,
                                        recovery: half(grade, 4)? as i16,
                                        extras: ingredients(&grade[6..])?,
                                    })
                                })
                                .collect::<Result<Vec<_>>>()?
                                .try_into()
                                .unwrap(),
                        })
                    })
                    .collect::<Result<Vec<_>>>()?
                    .try_into()
                    .unwrap(),
            })
        })
        .collect::<Result<_>>()?;
    let group_names = text.table(executable, GROUP_NAMES, 32)?;
    let lists = |address, count: usize| -> Result<Vec<_>> {
        dol::slice(executable, address, count * 4)?
            .chunks_exact(4)
            .map(|row| item_list(executable, word(row, 0)?))
            .collect()
    };
    let group_lists = lists(GROUP_LISTS, 32)?;
    let categories = dol::slice(executable, CATEGORIES, 32)?;
    let groups = group_names
        .into_iter()
        .zip(group_lists)
        .zip(categories)
        .map(|((name, items), &category)| Group {
            name,
            items,
            category,
        })
        .collect();
    let preferences = lists(LIKES, 9)?
        .into_iter()
        .zip(lists(DISLIKES, 9)?)
        .map(|(likes, dislikes)| Preference { likes, dislikes })
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    let labels = Label::ALL
        .into_iter()
        .zip(text.table(executable, LABELS, 11)?)
        .collect();
    let locked = text.required(executable, LOCKED)?;
    let locked_size = text.sources[locked.0].source_size as usize;
    ensure!(
        locked_size <= 16,
        "locked cooking label exceeds fixed extent"
    );
    let format_refs: [TextRef; 5] = FORMATS
        .into_iter()
        .map(|at| text.required(executable, at))
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .unwrap();
    ensure!(
        format_refs
            .iter()
            .all(|id| text.sources[id.0].source_size <= 8),
        "cooking format exceeds fixed extent"
    );
    let [item_count, recipe_counter, result_heading, recovery, effect] = format_refs;
    let catalogue = Catalogue {
        recipes,
        groups,
        preferences,
        effects: text.array(executable, EFFECTS)?,
        labels,
        locked,
        formats: Formats {
            item_count,
            recipe_counter,
            result_heading,
            recovery,
            effect,
        },
        bonus_skill: half(dol::slice(executable, BONUS_SKILL, 2)?, 0)?,
        texts: text.values,
    };
    Ok((catalogue, text.sources))
}

pub(crate) fn read(executable: &[u8]) -> Result<Catalogue> {
    Ok(parse(executable)?.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ingredient_counts_select_the_consumed_prefix() -> Result<()> {
        assert_eq!(
            ingredients(&[0, 2, 0, 81, 0x75, 0x31, 0xff, 0xff])?,
            [81, 30001]
        );
        assert!(ingredients(&[0, 0, 0xff, 0xff])?.is_empty());
        assert!(ingredients(&[0, 2, 0, 81]).is_err());
        assert!(ingredients(&[]).is_err());
        assert_eq!(
            ingredients(&[0, 3, 0, 0, 0x75, 0x50, 0xff, 0xff])?,
            [0, 30032, 65535]
        );
        Ok(())
    }

    #[test]
    #[ignore = "requires both extracted executables; no codecs or devices"]
    fn original_cooking_preserves_all_meals_and_character_grades() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let mut first = None;
        for disc in [1, 2] {
            let executable = std::fs::read(local.join(format!("disc{disc}/sys/main.dol")))?;
            let (catalogue, _) = parse(&executable)?;
            assert_eq!((catalogue.recipes.len(), catalogue.groups.len()), (24, 32));
            assert_eq!(catalogue.label(Label::Title)?, "Cooking");
            assert!(
                catalogue
                    .recipes
                    .iter()
                    .flat_map(|r| &r.cooks)
                    .flat_map(|c| &c.grades)
                    .all(|g| g.unknown_effect_bits == 0)
            );
            if let Some(expected) = &first {
                assert_eq!(&catalogue, expected);
            } else {
                first = Some(catalogue);
            }
        }
        Ok(())
    }
}
