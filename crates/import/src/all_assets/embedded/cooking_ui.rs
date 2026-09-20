//! Complete recipe records and the menu's authored ingredient/result bindings.
use super::text::{TextPool, TextRef, TextSource};
use crate::{
    dol,
    read::{u16 as half, u32 as word},
};
use anyhow::{Context, Result, ensure};
use resonance_content::menu_data::MealEffect;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

const FAMILY: &str = "cooking-ui";
const RECIPES: u32 = 0x80215b0c;
const RECIPE_COUNT: usize = 24;
const RECIPE_STRIDE: usize = 700;
const RECIPE_SIZE: usize = 0x41b4;
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
impl Label {
    const ALL: [Self; 11] = [
        Self::Title,
        Self::Cook,
        Self::Required,
        Self::Additional,
        Self::Success,
        Self::Failure,
        Self::NoEffect,
        Self::Missing,
        Self::Full,
        Self::Unknown,
        Self::ResultJoin,
    ];
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Ingredient {
    None,
    Item(u16),
    Group(u8),
    Unknown(u16),
}
impl Ingredient {
    fn read(value: u16) -> Self {
        match value {
            0 => Self::None,
            1..30000 => Self::Item(value),
            30000..30032 => Self::Group((value - 30000) as u8),
            _ => Self::Unknown(value),
        }
    }
    #[cfg(test)]
    fn source(self) -> u16 {
        match self {
            Self::None => 0,
            Self::Item(v) | Self::Unknown(v) => v,
            Self::Group(v) => u16::from(v) + 30000,
        }
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Ingredients {
    pub active: Vec<Ingredient>,
    pub storage: Vec<u16>,
}
impl Ingredients {
    fn read(bytes: &[u8]) -> Result<Self> {
        let count = usize::from(half(bytes, 0)?);
        ensure!(
            count < bytes.len() / 2,
            "recipe ingredient count exceeds its fixed slots"
        );
        let slots = bytes[2..]
            .chunks_exact(2)
            .map(|v| half(v, 0))
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            active: slots[..count]
                .iter()
                .copied()
                .map(Ingredient::read)
                .collect(),
            storage: slots[count..].to_vec(),
        })
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Grade {
    pub effects: Vec<MealEffect>,
    pub unknown_effect_bits: u32,
    pub recovery: i16,
    pub extras: Ingredients,
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
    pub required: Ingredients,
    pub cooks: [Cook; 9],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct ListRef(usize);

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct ItemList {
    pub items: Vec<u16>,
    pub storage: Vec<u8>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Group {
    pub name: Option<TextRef>,
    pub category: u8,
    pub items: Option<ListRef>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Preference {
    pub likes: Option<ListRef>,
    pub dislikes: Option<ListRef>,
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
    pub lists: Vec<ItemList>,
    pub recipes: Vec<Recipe>,
    pub recipe_storage: Vec<u8>,
    pub groups: Vec<Group>,
    pub preferences: [Preference; 9],
    pub effects: [Option<TextRef>; 12],
    pub labels: BTreeMap<Label, Option<TextRef>>,
    pub locked: TextRef,
    pub locked_storage: Vec<u8>,
    pub formats: Formats,
    pub format_storage: [Vec<u8>; 5],
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
    pub(crate) fn items(&self, reference: Option<ListRef>) -> Result<&[u16]> {
        Ok(&self.lists[reference.context("null required cooking item list")?.0].items)
    }
}

#[derive(Serialize)]
struct ListSource {
    address: u32,
    source_size: usize,
}

#[derive(Default)]
struct Lists {
    ids: BTreeMap<u32, ListRef>,
    values: Vec<ItemList>,
    sources: Vec<ListSource>,
}
impl Lists {
    fn reference(&mut self, executable: &[u8], address: u32) -> Result<Option<ListRef>> {
        if address == 0 {
            return Ok(None);
        }
        if let Some(id) = self.ids.get(&address) {
            return Ok(Some(*id));
        }
        let count = usize::from(half(dol::slice(executable, address, 2)?, 0)?);
        let used = 2 + count * 2;
        let size = used.next_multiple_of(4);
        let bytes = dol::slice(executable, address, size)?;
        let id = ListRef(self.values.len());
        self.values.push(ItemList {
            items: bytes[2..used]
                .chunks_exact(2)
                .map(|v| half(v, 0))
                .collect::<Result<_>>()?,
            storage: bytes[used..].to_vec(),
        });
        self.sources.push(ListSource {
            address,
            source_size: size,
        });
        self.ids.insert(address, id);
        Ok(Some(id))
    }
    fn table(
        &mut self,
        executable: &[u8],
        address: u32,
        count: usize,
    ) -> Result<Vec<Option<ListRef>>> {
        dol::slice(executable, address, count * 4)?
            .chunks_exact(4)
            .map(|row| self.reference(executable, word(row, 0)?))
            .collect()
    }
}

fn parse(executable: &[u8]) -> Result<(Catalogue, Vec<TextSource>, Vec<ListSource>)> {
    let mut text = TextPool::default();
    let mut lists = Lists::default();
    let bytes = dol::slice(executable, RECIPES, RECIPE_SIZE)?;
    let used = RECIPE_COUNT * RECIPE_STRIDE;
    let recipes = bytes[..used]
        .chunks_exact(RECIPE_STRIDE)
        .map(|row| {
            Ok(Recipe {
                name: text.reference(executable, word(row, 0)?)?,
                description: text.reference(executable, word(row, 4)?)?,
                required: Ingredients::read(&row[8..16])?,
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
                                        extras: Ingredients::read(&grade[6..])?,
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
    let group_lists = lists.table(executable, GROUP_LISTS, 32)?;
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
    let preferences = lists
        .table(executable, LIKES, 9)?
        .into_iter()
        .zip(lists.table(executable, DISLIKES, 9)?)
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
    let format_storage = format_refs
        .iter()
        .zip(FORMATS)
        .map(|(id, address)| {
            let size = text.sources[id.0].source_size as usize;
            ensure!(size <= 8, "cooking format exceeds fixed extent");
            Ok(dol::slice(executable, address + size as u32, 8 - size)?.to_vec())
        })
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .unwrap();
    let [item_count, recipe_counter, result_heading, recovery, effect] = format_refs;
    let catalogue = Catalogue {
        recipes,
        recipe_storage: bytes[used..].to_vec(),
        groups,
        preferences,
        effects: text.array(executable, EFFECTS)?,
        labels,
        locked,
        locked_storage: dol::slice(executable, LOCKED + locked_size as u32, 16 - locked_size)?
            .to_vec(),
        formats: Formats {
            item_count,
            recipe_counter,
            result_heading,
            recovery,
            effect,
        },
        format_storage,
        bonus_skill: half(dol::slice(executable, BONUS_SKILL, 2)?, 0)?,
        lists: lists.values,
        texts: text.values,
    };
    Ok((catalogue, text.sources, lists.sources))
}

pub(crate) fn read(executable: &[u8]) -> Result<Catalogue> {
    Ok(parse(executable)?.0)
}

pub(super) fn cook(file: &Path, executable: &[u8], output: &Path) -> Result<Vec<String>> {
    let (catalogue, texts, lists) = parse(executable)?;
    crate::embedded::write(
        file,
        output,
        FAMILY,
        &catalogue,
        serde_json::json!({
            "recipes":{"address":RECIPES,"count":RECIPE_COUNT,"stride":RECIPE_STRIDE,"source_size":RECIPE_SIZE},
            "groups":{"lists_address":GROUP_LISTS,"names_address":GROUP_NAMES,"categories_address":CATEGORIES,"count":32},
            "preferences":{"likes_address":LIKES,"dislikes_address":DISLIKES,"count":9},
            "labels":{"address":LABELS,"count":11}, "locked":{"address":LOCKED,"source_size":16},
            "effects":{"address":EFFECTS,"count":12}, "formats":{"addresses":FORMATS,"stride":8},
            "bonus_skill":{"address":BONUS_SKILL,"source_size":2}, "texts":texts,"lists":lists,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn reconstruct(
        c: &Catalogue,
        text: &[TextSource],
        lists: &[ListSource],
    ) -> Result<Vec<(u32, Vec<u8>)>> {
        let pointer =
            |reference: Option<TextRef>| reference.map_or(0, |id| text[id.0].address).to_be_bytes();
        let list_pointer = |reference: Option<ListRef>| {
            reference.map_or(0, |id| lists[id.0].address).to_be_bytes()
        };
        let ingredients = |values: &Ingredients| {
            std::iter::once(values.active.len() as u16)
                .chain(values.active.iter().map(|&v| v.source()))
                .chain(values.storage.iter().copied())
                .flat_map(u16::to_be_bytes)
                .collect::<Vec<_>>()
        };
        let mut recipes = Vec::new();
        for recipe in &c.recipes {
            recipes.extend(pointer(recipe.name));
            recipes.extend(pointer(recipe.description));
            recipes.extend(ingredients(&recipe.required));
            for cook in &recipe.cooks {
                recipes.extend(cook.base_stars.to_be_bytes());
                recipes.extend(cook.storage.to_be_bytes());
                for grade in &cook.grades {
                    let mask =
                        grade
                            .effects
                            .iter()
                            .fold(grade.unknown_effect_bits, |mask, effect| {
                                mask | 1
                                    << MealEffect::ALL.iter().position(|v| v == effect).unwrap()
                            });
                    recipes.extend(mask.to_be_bytes());
                    recipes.extend(grade.recovery.to_be_bytes());
                    recipes.extend(ingredients(&grade.extras));
                }
            }
        }
        recipes.extend(&c.recipe_storage);
        let mut spans = vec![
            (RECIPES, recipes),
            (
                GROUP_LISTS,
                c.groups
                    .iter()
                    .flat_map(|g| list_pointer(g.items))
                    .collect(),
            ),
            (
                GROUP_NAMES,
                c.groups.iter().flat_map(|g| pointer(g.name)).collect(),
            ),
            (CATEGORIES, c.groups.iter().map(|g| g.category).collect()),
            (
                LIKES,
                c.preferences
                    .iter()
                    .flat_map(|p| list_pointer(p.likes))
                    .collect(),
            ),
            (
                DISLIKES,
                c.preferences
                    .iter()
                    .flat_map(|p| list_pointer(p.dislikes))
                    .collect(),
            ),
            (
                LABELS,
                Label::ALL
                    .into_iter()
                    .flat_map(|label| pointer(c.labels[&label]))
                    .collect(),
            ),
            (EFFECTS, c.effects.into_iter().flat_map(pointer).collect()),
            (BONUS_SKILL, c.bonus_skill.to_be_bytes().to_vec()),
        ];
        for (index, source) in lists.iter().enumerate() {
            let list = &c.lists[index];
            let mut bytes: Vec<_> = std::iter::once(list.items.len() as u16)
                .chain(list.items.iter().copied())
                .flat_map(u16::to_be_bytes)
                .collect();
            bytes.extend(&list.storage);
            assert_eq!(bytes.len(), source.source_size);
            spans.push((source.address, bytes));
        }
        for (index, source) in text.iter().enumerate() {
            let (encoded, _, invalid) = encoding_rs::SHIFT_JIS.encode(c.text(TextRef(index)));
            ensure!(!invalid, "cooking text cannot reconstruct source encoding");
            let mut bytes = [encoded.as_ref(), &[0]].concat();
            assert_eq!(bytes.len() as u32, source.source_size);
            if source.address == LOCKED {
                bytes.extend(&c.locked_storage);
            }
            if let Some(index) = FORMATS
                .iter()
                .position(|&address| address == source.address)
            {
                bytes.extend(&c.format_storage[index]);
            }
            spans.push((source.address, bytes));
        }
        Ok(spans)
    }

    fn patch(executable: &mut [u8], address: u32, bytes: &[u8]) -> Result<()> {
        let offset = dol::slice(executable, address, bytes.len())?.as_ptr() as usize
            - executable.as_ptr() as usize;
        executable[offset..offset + bytes.len()].copy_from_slice(bytes);
        Ok(())
    }

    #[test]
    #[ignore = "requires both original executables; no codecs or devices"]
    fn original_cooking_ui_reconstructs_complete_tables_and_publishes_shared_data() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join("cooking-ui"));
        fs::create_dir(&output)?;
        let result = (|| -> Result<()> {
            let mut first = None;
            for disc in [1, 2] {
                let file = local.join(format!("disc{disc}/sys/main.dol"));
                let mut executable = fs::read(&file)?;
                let (c, text, lists) = parse(&executable)?;
                let restored: Catalogue = serde_json::from_slice(&serde_json::to_vec(&c)?)?;
                assert_eq!(c, restored);
                for (address, bytes) in reconstruct(&restored, &text, &lists)? {
                    assert_eq!(
                        bytes,
                        dol::slice(&executable, address, bytes.len())?,
                        "span {address:#x}"
                    );
                }
                assert_eq!(c.recipes.len(), 24);
                assert_eq!(c.groups.len(), 32);
                assert_eq!(c.groups[2].name, c.groups[3].name);
                assert_eq!(c.label(Label::Title)?, "Cooking");
                assert_eq!(c.text(c.locked), "????????");
                assert_eq!(c.text(c.formats.item_count), ":%2d");
                assert_eq!(c.recipe_storage.len(), 20);
                assert!(
                    c.recipes
                        .iter()
                        .flat_map(|r| &r.cooks)
                        .flat_map(|c| &c.grades)
                        .all(|g| g.unknown_effect_bits == 0)
                );
                let paths = cook(&file, &executable, &output)?;
                assert_eq!(
                    crate::embedded::read::<Catalogue>(&output, FAMILY, "main.dol")?,
                    c
                );
                if let Some(expected) = &first {
                    assert_eq!(&paths[0], expected);
                } else {
                    first = Some(paths[0].clone());
                }
                let source: serde_json::Value =
                    serde_json::from_slice(&fs::read(output.join(&paths[1]))?)?;
                assert_eq!(source["source_sha256"], crate::digest(&executable));
                assert_eq!(source["recipes"]["source_size"], RECIPE_SIZE);

                // Fields excluded from the runtime projection must survive publication.
                let padded_list = lists
                    .iter()
                    .enumerate()
                    .find(|(id, _)| !c.lists[*id].storage.is_empty())
                    .context("list storage")?;
                let tail = padded_list.1.address + padded_list.1.source_size as u32 - 1;
                let first_like = lists[c.preferences[0].likes.unwrap().0].address;
                for (address, bytes) in [
                    (RECIPES, vec![0; 4]),
                    (RECIPES + 8, 1u16.to_be_bytes().to_vec()),
                    (RECIPES + 12, 0xabcd_u16.to_be_bytes().to_vec()),
                    (RECIPES + 18, 0x1234_u16.to_be_bytes().to_vec()),
                    (RECIPES + 20, 0x8000_0001u32.to_be_bytes().to_vec()),
                    (RECIPES + 24, (-10i16).to_be_bytes().to_vec()),
                    (RECIPES + 26, 1u16.to_be_bytes().to_vec()),
                    (RECIPES + 28, 30032u16.to_be_bytes().to_vec()),
                    (RECIPES + 30, 0x9876u16.to_be_bytes().to_vec()),
                    (RECIPES + (RECIPE_COUNT * RECIPE_STRIDE) as u32, vec![0x5a]),
                    (GROUP_LISTS, vec![0; 4]),
                    (LIKES + 4, first_like.to_be_bytes().to_vec()),
                    (LOCKED + 15, vec![0x7e]),
                    (tail, vec![0x9a]),
                    (FORMATS[0], b"\x0b\0X\0".to_vec()),
                    (FORMATS[0] + 7, vec![0xac]),
                ] {
                    patch(&mut executable, address, &bytes)?;
                }
                let (changed, changed_text, changed_lists) = parse(&executable)?;
                assert!(changed.recipes[0].name.is_none());
                assert!(changed.groups[0].items.is_none());
                assert_eq!(changed.preferences[0].likes, changed.preferences[1].likes);
                let grade = &changed.recipes[0].cooks[0].grades[0];
                assert_eq!(grade.recovery, -10);
                assert_eq!(grade.unknown_effect_bits, 0x8000_0000);
                assert_eq!(grade.extras.active, [Ingredient::Unknown(30032)]);
                assert_eq!(changed.text(changed.formats.item_count), "\x0b\0X");
                for (address, bytes) in reconstruct(&changed, &changed_text, &changed_lists)? {
                    assert_eq!(
                        bytes,
                        dol::slice(&executable, address, bytes.len())?,
                        "changed span {address:#x}"
                    );
                }
                patch(&mut executable, RECIPES + 8, &4u16.to_be_bytes())?;
                assert!(read(&executable).is_err());
            }
            Ok(())
        })();
        fs::remove_dir_all(output)?;
        result
    }
}
