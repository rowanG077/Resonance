//! EX choices and compound recipes directly into the named asset document.
use crate::{pe, source::Source, text::Bank};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    ex_skills::{self, Character, Combination, Document, Gems, Skill},
    library::Mappings,
    menu_data::{ExActivation, ExCaption, ExStat, ExStatBonus, ExTendency},
    overworld::PartyCharacter,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
use symphonia_constants::ItemId;

pub(crate) fn cook(source: &Source, library: &Path) -> Result<()> {
    let image = pe::Image::parse(&source.executable)?;
    let bytes = source.require("TOS_TEXT.TXB")?;
    let bank = Bank::parse(&bytes)?;
    let document = convert(&image, &bank, source.language.slot())?;
    crate::output::json(&library.join(ex_skills::PATH), &document)
}

fn caption(
    image: &pe::Image<'_>,
    bank: &Bank<'_>,
    slot: u32,
    directory: u32,
    index: u32,
) -> Result<String> {
    let directory = pe::word(image.slice(directory + slot * 4, 4)?, 0)?;
    let id = pe::word(image.slice(directory + index * 4, 4)?, 0)?;
    if id == 0 {
        return Ok(String::new());
    }
    bank.menu_with_buttons(id, encoding_rs::WINDOWS_1252, |id| {
        Ok(match id {
            8 => 6,
            23 => 23,
            _ => anyhow::bail!("unknown EX action button {id}"),
        })
    })
}

fn stat(id: u8) -> Result<ExStat> {
    Ok(match id {
        1 => ExStat::Strength,
        2 => ExStat::Defense,
        3 => ExStat::Accuracy,
        4 => ExStat::Evasion,
        5 => ExStat::MaxHp,
        6 => ExStat::MaxTp,
        7 => ExStat::Luck,
        8 => ExStat::Intelligence,
        _ => anyhow::bail!("unsupported EX stat selector {id}"),
    })
}

fn item(id: u16) -> Result<String> {
    Ok(ItemId::try_from(id)?
        .resource_name()
        .context("EX gem has no maintained item name")?
        .to_owned())
}

fn convert(image: &pe::Image<'_>, bank: &Bank<'_>, slot: u32) -> Result<Document> {
    let mappings: Mappings =
        serde_json::from_str(include_str!("../../importer/src/ex-skill-names.json"))?;
    let definitions = image.slice(0x8e1208, 166 * 20)?;
    ensure!(
        pe::half(definitions, 16)? == 0,
        "empty EX slot contributes technique drift"
    );
    // Raine's Personal skill reduces field healing at save points to one TP.
    let personal = image.slice(0x48e1ab, 7)?;
    ensure!(
        personal[..3] == [0xc7, 0x45, 0xec],
        "Steam personal EX initializer changed"
    );
    let save_point = pe::half(personal, 5)?;
    let mut skills = BTreeMap::new();
    for (id, name) in mappings.iter() {
        let row = definitions
            .get(id as usize * 20..id as usize * 20 + 20)
            .context("EX skill outside native definitions")?;
        ensure!(
            id > 0 && pe::word(row, 0)? == id,
            "unsupported EX definition {id}"
        );
        let bonuses = row[12..16]
            .chunks_exact(2)
            .filter(|b| b != &[0, 0])
            .map(|b| {
                ensure!(b[1] > 0, "zero EX stat bonus for {id}");
                Ok(ExStatBonus {
                    stat: stat(b[0])?,
                    percent: b[1],
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let tendency = match pe::half(row, 16)? as i16 {
            -1 => Some(ExTendency::Technical),
            0 => None,
            1 => Some(ExTendency::Strike),
            other => anyhow::bail!("unsupported EX tendency {other} in skill {id}"),
        };
        let activation = match row[19] {
            0 => ExActivation::Constant,
            1 => ExActivation::Chance,
            2 => ExActivation::BattleEnd,
            3 => ExActivation::Other,
            other => anyhow::bail!("unsupported EX activation {other} in skill {id}"),
        };
        let skill = Skill {
            text: serde_json::to_value(ExCaption {
                name: caption(image, bank, slot, 0x8cb8a4, pe::word(row, 4)?)?,
                description: resonance_importer::decode_menu_text(
                    &caption(image, bank, slot, 0x8cb8a4, pe::word(row, 8)?)?,
                    9,
                )?,
            })?,
            stat_bonuses: bonuses,
            save_point_tp_cost: (id == u32::from(save_point)).then_some(1),
            tendency,
            activation,
            combinations: Vec::new(),
        };
        ensure!(
            skills.insert(name.to_owned(), skill).is_none(),
            "duplicate EX name {name}"
        );
    }

    let choices = image.slice(0x8e0a70, 9 * 16)?;
    let recipes = image.slice(0x8e0b00, 9 * 196)?;
    let mut characters = BTreeMap::new();
    let mut referenced = BTreeSet::new();
    for (index, (choice, recipe)) in choices
        .chunks_exact(16)
        .zip(recipes.chunks_exact(196))
        .enumerate()
    {
        let character = PartyCharacter::try_from(index as u8 + 1)?;
        ensure!(
            pe::word(recipe, 0)? == 24,
            "invalid compound EX skill count"
        );
        let allowed: BTreeSet<_> = choice.iter().copied().collect();
        ensure!(allowed.len() == 16, "duplicate EX choice for {character:?}");
        ensure!(
            allowed.iter().any(|&id| u16::from(id) == save_point) == (index == 3),
            "save-point TP skill does not match its caster"
        );
        let mut levels = BTreeMap::new();
        for (level, row) in choice.chunks_exact(4).enumerate() {
            let names = row
                .iter()
                .map(|&id| {
                    referenced.insert(u32::from(id));
                    let name = mappings.target(u32::from(id))?;
                    ensure!(
                        skills[name].tendency.is_some(),
                        "EX choice {id} has no tendency"
                    );
                    Ok(name.to_owned())
                })
                .collect::<Result<Vec<_>>>()?;
            levels.insert(level as u8 + 1, names.try_into().unwrap());
        }
        let mut compounds = BTreeMap::new();
        let mut distinct = BTreeSet::new();
        for (local_id, row) in recipe[4..].chunks_exact(8).enumerate() {
            let id = u32::from(pe::half(row, 0)?);
            ensure!(distinct.insert(id), "duplicate compound EX skill {id}");
            referenced.insert(id);
            let name = mappings.target(id)?;
            let count = usize::from(pe::half(row, 2)?);
            ensure!((2..=4).contains(&count), "invalid compound EX requirements");
            let selected = &row[4..4 + count];
            ensure!(
                selected.iter().all(|id| allowed.contains(id))
                    && selected.iter().collect::<BTreeSet<_>>().len() == count,
                "invalid compound EX requirements for {character:?}"
            );
            let requires = selected
                .iter()
                .map(|&id| Ok(mappings.target(u32::from(id))?.to_owned()))
                .collect::<Result<Vec<_>>>()?;
            let skill = skills
                .get_mut(name)
                .context("missing named compound EX skill")?;
            ensure!(
                skill.tendency.is_none(),
                "compound EX skill {id} has a tendency"
            );
            if let Some(existing) = skill
                .combinations
                .iter_mut()
                .find(|c| c.requires == requires)
            {
                existing.characters.push(character);
            } else {
                skill.combinations.push(Combination {
                    characters: vec![character],
                    requires,
                });
            }
            compounds.insert(local_id as u32, name.to_owned());
        }
        characters.insert(
            character,
            Character {
                levels,
                compounds: Mappings::new(compounds)?,
            },
        );
    }
    ensure!(
        referenced == mappings.iter().map(|(id, _)| id).collect(),
        "EX names do not cover source skills"
    );

    let source_labels = [
        17, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 11, 12, 13, 14, 22, 23, 24, 25,
    ]
    .into_iter()
    .map(|id| caption(image, bank, slot, 0x8cb88c, id))
    .collect::<Result<Vec<_>>>()?;
    let mut labels = BTreeMap::new();
    for (index, name) in [
        "title",
        "hp",
        "tp",
        "slash",
        "thrust",
        "defense",
        "accuracy",
        "evasion",
        "intelligence",
        "luck",
        "attack",
        "",
        "",
        "",
        "",
        "set_gem",
        "replace_gem",
        "yes",
        "no",
    ]
    .into_iter()
    .enumerate()
    {
        if !name.is_empty() {
            labels.insert(name, source_labels[index].clone());
        }
    }
    for (name, address) in [("gem_max", 19), ("gem_level", 20), ("gem_empty", 21)] {
        labels.insert(name, caption(image, bank, slot, 0x8cb88c, address)?);
    }
    Ok(Document {
        mappings,
        skills,
        characters,
        gem_items: Gems {
            level1: item(40)?,
            level2: item(41)?,
            level3: item(42)?,
            level4: item(43)?,
            max: item(496)?,
        },
        labels: serde_json::to_value(labels)?,
        activation_labels: serde_json::json!({
            "constant": source_labels[11], "chance": source_labels[12],
            "battle_end": source_labels[13], "other": source_labels[14],
        }),
    })
}
