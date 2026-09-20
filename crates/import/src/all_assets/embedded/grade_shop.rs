//! New Game Plus purchases, selection constraints and Grade accounting.
use crate::{dol, read::u32 as word};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

const OPTIONS: u32 = 0x8019d130;
const LABELS: u32 = 0x8019bdc4;
const FAMILY: &str = "grade-shop";

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

impl Benefit {
    const ALL: [Self; 26] = [
        Self::ExSkills,
        Self::ExGems,
        Self::Affection,
        Self::IncreasedTension,
        Self::PlayTime,
        Self::MemoryCircles,
        Self::ThirtyItems,
        Self::Gald,
        Self::Recipes,
        Self::CookingAbility,
        Self::Titles,
        Self::Figurines,
        Self::MonsterList,
        Self::CollectorsBook,
        Self::WorldMap,
        Self::MiniGames,
        Self::BattleInfo,
        Self::Tech,
        Self::TechUsage,
        Self::IncreasedHp,
        Self::MinimumHp,
        Self::ComboExperience,
        Self::HalfExperience,
        Self::DoubleExperience,
        Self::TenfoldExperience,
        Self::IncreasedGrade,
    ];
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

fn immediate(executable: &[u8], address: u32, opcode: u32) -> Result<u16> {
    let instruction = word(dol::slice(executable, address, 4)?, 0)?;
    ensure!(
        instruction >> 16 == opcode,
        "unexpected Grade Shop instruction at {address:#x}"
    );
    Ok(instruction as u16)
}

fn exclusions(executable: &[u8]) -> Result<[Vec<Benefit>; 26]> {
    let mut exclusions = std::array::from_fn(|_| Vec::new());
    // The six restricted cases in the selection callback test these exact masks.
    for (index, address) in (19..=24).zip([
        0x800afaf0, 0x800afafc, 0x800afb08, 0x800afb14, 0x800afb20, 0x800afb2c,
    ]) {
        let instruction = word(dol::slice(executable, address, 4)?, 0)?;
        let mask = if instruction >> 16 == 0x7503 {
            (instruction & 0xffff) << 16 // andis.
        } else {
            ensure!(
                instruction & 0xfffff801 == 0x55030001,
                "unexpected Grade exclusion mask"
            );
            let first = (instruction >> 6) & 31;
            let last = (instruction >> 1) & 31;
            ensure!(first <= last, "wrapped Grade exclusion mask");
            (u32::MAX >> first) & (u32::MAX << (31 - last)) // rlwinm. without rotation
        };
        ensure!(
            mask >> Benefit::ALL.len() == 0 && mask & (1 << index) == 0,
            "invalid Grade exclusions"
        );
        exclusions[index] = Benefit::ALL
            .into_iter()
            .filter(|benefit| mask & (1 << *benefit as u8) != 0)
            .collect();
    }
    Ok(exclusions)
}

fn read(executable: &[u8]) -> Result<Catalogue> {
    let exclusions = exclusions(executable)?;
    let units_per_grade = u32::from(immediate(executable, 0x800afe04, 0x1ca3)?);
    ensure!(units_per_grade > 0, "zero Grade currency scale");
    // All 26 refund branches use the same scale and saturation constant.
    let limit_high = immediate(executable, 0x80043c2c, 0x3c60)?;
    let limit_low = immediate(executable, 0x80043c3c, 0x3803)?;
    let refund_balance_limit =
        (u32::from(limit_high) << 16).wrapping_add_signed(i32::from(limit_low as i16));
    for index in 0..Benefit::ALL.len() as u32 {
        let offset = index * 0x48;
        ensure!(
            u32::from(immediate(executable, 0x80043c40 + offset, 0x1c64)?) == units_per_grade
                && immediate(executable, 0x80043c2c + offset, 0x3c60)? == limit_high
                && immediate(executable, 0x80043c3c + offset, 0x3803)? == limit_low,
            "inconsistent Grade refund accounting"
        );
    }
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
                    excludes: exclusions[benefit as usize].clone(),
                })
            })
            .collect::<Result<_>>()?,
        initial_selection: Vec::new(),
        account: Account {
            units_per_grade,
            refund_previous_purchases: true,
            refund_balance_limit,
        },
        labels,
    })
}

pub(super) fn cook(file: &Path, executable: &[u8], output: &Path) -> Result<Vec<String>> {
    crate::embedded::write(
        file,
        output,
        FAMILY,
        &read(executable)?,
        serde_json::json!({
            "options":{"address":OPTIONS,"count":Benefit::ALL.len(),"stride":12},
            "labels":{"address":LABELS,"count":6,"stride":4},
            "direct_labels":[0x8019d2c8u32,0x8035ce18u32,0x8035ce2cu32,0x8035ce30u32],
            "constraints":{"consumer":0x800af920u32,"first_mask":0x800afaf0u32,"mask_stride":12,"count":6},
            "refunds":{"consumer":0x80043a44u32,"command":10,"first_branch":0x80043c1cu32,"stride":0x48,"count":26},
            "save_fields":{"grade":{"offset":0x1f28,"bytes":4},"applied_selection":{"offset":0x1f54,"bytes":4},"pending_selection":{"offset":0x1f60,"bytes":4}}
        }),
    )
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
            // A changed native price and mask must reach semantic data, not fixed recipes.
            for (address, value) in [(OPTIONS, 777u32), (0x800afb14, 0x75030020)] {
                let source = dol::slice(&executable, address, 4)?;
                let offset = source.as_ptr() as usize - executable.as_ptr() as usize;
                executable[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
            }
            let changed = read(&executable)?;
            assert_eq!(changed.options[0].price, 777);
            assert_eq!(changed.options[22].excludes, [Benefit::ComboExperience]);
        }
        Ok(())
    }
}
