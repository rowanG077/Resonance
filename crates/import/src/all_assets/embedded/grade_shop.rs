//! New Game Plus purchases, selection constraints and Grade accounting.
use crate::{dol, read::u32 as word};
use anyhow::Result;
use resonance_content::grade::{Benefit, Purchase, Shop};
use std::{collections::BTreeMap, path::Path};

const OPTIONS: u32 = 0x8019d130;
const LABELS: u32 = 0x8019bdc4;
const FAMILY: &str = "grade-shop";

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

pub(crate) fn read(executable: &[u8]) -> Result<Shop> {
    let mut labels: BTreeMap<_, _> = [
        "buy_cancel",
        "total_cost",
        "finish",
        "confirmation",
        "yes",
        "no",
    ]
    .into_iter()
    .zip(dol::slice(executable, LABELS, 24)?.chunks_exact(4))
    .map(|(label, pointer)| Ok((label.to_owned(), dol::text(executable, word(pointer, 0)?)?)))
    .collect::<Result<_>>()?;
    for (label, address) in [
        ("heading", 0x8019d2c8),
        ("grade", 0x8035ce18),
        ("balance_format", 0x8035ce2c),
        ("price_format", 0x8035ce30),
    ] {
        labels.insert(label.to_owned(), dol::text(executable, address)?);
    }
    let shop = Shop {
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
        labels,
    };
    shop.validate()?;
    Ok(shop)
}

pub(super) fn cook(file: &Path, executable: &[u8], output: &Path) -> Result<Vec<String>> {
    crate::embedded::write(file, output, FAMILY, &read(executable)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires extracted discs; metadata only"]
    fn grade_shop_reads_both_discs() -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let first = read(&std::fs::read(root.join("disc1/sys/main.dol"))?)?;
        let second = read(&std::fs::read(root.join("disc2/sys/main.dol"))?)?;
        assert_eq!(first, second);
        assert!(
            first
                .cost(&[Benefit::IncreasedHp, Benefit::MinimumHp].into())
                .is_err()
        );
        Ok(())
    }
}
