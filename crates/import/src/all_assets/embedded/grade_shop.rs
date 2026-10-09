//! New Game Plus purchases, selection constraints and Grade accounting.
use crate::{dol, read::u32 as word};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

const OPTIONS: u32 = 0x8019d130;
const LABELS: u32 = 0x8019bdc4;
const FAMILY: &str = "grade-shop";

super::ordered! {
    /// The ordinal is the bit in both the pending and previously applied selections.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[repr(u8)]
    #[serde(rename_all = "snake_case")]
    enum Benefit {
        ExSkills,
        ExGems,
        Affection,
        IncreasedTension,
        PlayTime,
        MemoryCircles,
        ThirtyItems,
        Gald,
        Recipes,
        CookingAbility,
        Titles,
        Figurines,
        MonsterList,
        CollectorsBook,
        WorldMap,
        MiniGames,
        BattleInfo,
        Tech,
        TechUsage,
        IncreasedHp,
        MinimumHp,
        ComboExperience,
        HalfExperience,
        DoubleExperience,
        TenfoldExperience,
        IncreasedGrade,
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Catalogue {
    options: Vec<Purchase>,
    /// Opening the shop starts with no selected purchases, even on later cycles.
    initial_selection: Vec<Benefit>,
    account: Account,
    labels: BTreeMap<Label, String>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Purchase {
    benefit: Benefit,
    /// Whole Grade; affordability uses the truncated whole-Grade balance.
    price: u32,
    name: String,
    description: String,
    excludes: Vec<Benefit>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Account {
    /// Purchase confirmation debits this many save-data units per Grade.
    units_per_grade: u32,
    /// Completing another cycle refunds every applied option at its current price.
    refund_previous_purchases: bool,
    /// Applied after each individual refund, measured in save-data units.
    refund_balance_limit: u32,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Label {
    BuyCancel,
    TotalCost,
    Finish,
    Confirmation,
    Yes,
    No,
    Heading,
    Grade,
    BalanceFormat,
    PriceFormat,
}

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
        .filter(|other| *other != benefit)
        .collect()
}

fn read(executable: &[u8]) -> Result<Catalogue> {
    let mut labels: BTreeMap<_, _> = [
        Label::BuyCancel,
        Label::TotalCost,
        Label::Finish,
        Label::Confirmation,
        Label::Yes,
        Label::No,
    ]
    .into_iter()
    .zip(dol::slice(executable, LABELS, 24)?.chunks_exact(4))
    .map(|(label, pointer)| Ok((label, dol::text(executable, word(pointer, 0)?)?)))
    .collect::<Result<_>>()?;
    for (label, address) in [
        (Label::Heading, 0x8019d2c8),
        (Label::Grade, 0x8035ce18),
        (Label::BalanceFormat, 0x8035ce2c),
        (Label::PriceFormat, 0x8035ce30),
    ] {
        labels.insert(label, dol::text(executable, address)?);
    }
    Ok(Catalogue {
        options: Benefit::ALL
            .into_iter()
            .zip(dol::slice(executable, OPTIONS, Benefit::ALL.len() * 12)?.chunks_exact(12))
            .map(|(benefit, row)| {
                Ok(Purchase {
                    benefit,
                    price: word(row, 0)?,
                    name: dol::text(executable, word(row, 4)?)?,
                    description: dol::text(executable, word(row, 8)?)?,
                    excludes: exclusions(benefit),
                })
            })
            .collect::<Result<_>>()?,
        initial_selection: Vec::new(),
        account: Account {
            units_per_grade: 100,
            refund_previous_purchases: true,
            refund_balance_limit: 99_999_999,
        },
        labels,
    })
}

pub(super) fn cook(file: &Path, executable: &[u8], output: &Path) -> Result<Vec<String>> {
    crate::embedded::write(file, output, FAMILY, &read(executable)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    #[ignore = "requires both extracted discs; no media conversion or playback"]
    fn original_grade_shop_preserves_all_purchases_constraints_and_accounting() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let mut first = None;
        for disc in [1, 2] {
            let mut executable = fs::read(local.join(format!("disc{disc}/sys/main.dol")))?;
            let catalogue = read(&executable)?;
            let encoded = serde_json::to_vec(&catalogue)?;
            let restored: Catalogue = serde_json::from_slice(&encoded)?;
            assert_eq!(restored, catalogue);
            if let Some(expected) = &first {
                assert_eq!(&encoded, expected);
            } else {
                first = Some(encoded);
            }
            assert_eq!(catalogue.account.units_per_grade, 100);
            assert_eq!(catalogue.account.refund_balance_limit, 99_999_999);
            assert_eq!(catalogue.labels[&Label::Heading], "GRADE SHOP");
            let mut table = Vec::new();
            for (index, option) in restored.options.iter().enumerate() {
                assert_eq!(option.benefit as usize, index);
                let row = dol::slice(&executable, OPTIONS + index as u32 * 12, 12)?;
                table.extend(option.price.to_be_bytes());
                for (offset, text) in [(4, &option.name), (8, &option.description)] {
                    let pointer = word(row, offset)?;
                    table.extend(pointer.to_be_bytes());
                    let mut terminated = text.as_bytes().to_vec();
                    terminated.push(0);
                    assert_eq!(
                        terminated,
                        dol::slice(&executable, pointer, terminated.len())?
                    );
                }
                let mask = option
                    .excludes
                    .iter()
                    .fold(0, |mask, benefit| mask | 1u32 << *benefit as u8);
                assert_eq!(
                    mask,
                    match index {
                        19 => 0x100000,
                        20 => 0x80000,
                        21 => 0x1c00000,
                        22 => 0x1a00000,
                        23 => 0x1600000,
                        24 => 0xe00000,
                        _ => 0,
                    }
                );
            }
            assert_eq!(table, dol::slice(&executable, OPTIONS, 26 * 12)?);
            // Prices come from the item data.
            let source = dol::slice(&executable, OPTIONS, 4)?;
            let offset = source.as_ptr() as usize - executable.as_ptr() as usize;
            executable[offset..offset + 4].copy_from_slice(&777u32.to_be_bytes());
            let changed = read(&executable)?;
            assert_eq!(changed.options[0].price, 777);
        }
        Ok(())
    }
}
