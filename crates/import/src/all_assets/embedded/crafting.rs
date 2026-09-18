//! Equipment crafting recipes and the recipes each vendor offers.
use super::text::{TextPool, TextRef, TextSource};
use crate::{
    dol,
    read::{u16 as half, u32 as word},
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

const FAMILY: &str = "crafting";
const RECIPES: u32 = 0x80220cd0;
const RECIPE_COUNT: usize = 151;
const RECIPE_BYTES: usize = 20;
const VENDORS: u32 = 0x8022189c;
const VENDOR_COUNT: usize = 22;
const VENDOR_BYTES: usize = 48;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Ingredient {
    pub(crate) item: i16,
    pub(crate) count: u8,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Recipe {
    /// Crafting consumes one base item and produces one result item.
    pub(crate) base_item: i16,
    pub(crate) result_item: i16,
    pub(crate) ingredients: Vec<Ingredient>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Vendor {
    pub(crate) name: Option<TextRef>,
    /// Zero is a valid recipe index, including inside the active prefix.
    pub(crate) recipes: Vec<i16>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Catalogue {
    texts: Vec<String>,
    pub(crate) recipes: Vec<Recipe>,
    pub(crate) vendors: Vec<Vendor>,
}

fn recipe(bytes: &[u8]) -> Result<Recipe> {
    let row = bytes
        .get(..RECIPE_BYTES)
        .context("truncated crafting recipe")?;
    Ok(Recipe {
        base_item: half(row, 0)? as i16,
        result_item: half(row, 2)? as i16,
        ingredients: row[4..]
            .chunks_exact(4)
            .map(|slot| Ingredient {
                item: i16::from_be_bytes([slot[0], slot[1]]),
                count: slot[2],
            })
            .take_while(|ingredient| ingredient.item != 0)
            .collect(),
    })
}

fn vendor_recipes(row: &[u8]) -> Result<Vec<i16>> {
    let count = (half(row, 4)? as i16).max(0) as usize;
    Ok(row
        .get(6..46)
        .and_then(|slots| slots.get(..count * 2))
        .context("crafting vendor count exceeds its recipe slots")?
        .chunks_exact(2)
        .map(|value| i16::from_be_bytes([value[0], value[1]]))
        .collect())
}

fn parse(executable: &[u8]) -> Result<(Catalogue, Vec<TextSource>)> {
    let recipes = dol::slice(executable, RECIPES, RECIPE_COUNT * RECIPE_BYTES)?
        .chunks_exact(RECIPE_BYTES)
        .map(recipe)
        .collect::<Result<_>>()?;
    let mut texts = TextPool::default();
    let vendors = dol::slice(executable, VENDORS, VENDOR_COUNT * VENDOR_BYTES)?
        .chunks_exact(VENDOR_BYTES)
        .map(|row| {
            Ok(Vendor {
                name: texts.reference(executable, word(row, 0)?)?,
                recipes: vendor_recipes(row)?,
            })
        })
        .collect::<Result<_>>()?;
    Ok((
        Catalogue {
            texts: texts.values,
            recipes,
            vendors,
        },
        texts.sources,
    ))
}

pub(super) fn cook(file: &Path, executable: &[u8], output: &Path) -> Result<Vec<String>> {
    let (catalogue, _) = parse(executable)?;
    crate::embedded::write(file, output, FAMILY, &catalogue)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn ingredients_stop_at_zero_but_vendor_recipe_zero_is_valid() -> Result<()> {
        let mut bytes = [0; RECIPE_BYTES];
        bytes[4..8].copy_from_slice(&[0, 81, 2, 255]);
        bytes[12..16].copy_from_slice(&[0, 99, 3, 255]);
        assert_eq!(
            recipe(&bytes)?.ingredients,
            [Ingredient { item: 81, count: 2 }]
        );
        assert!(recipe(&bytes[..RECIPE_BYTES - 1]).is_err());
        let mut vendor = [0; VENDOR_BYTES];
        for (count, length) in [(-1i16, 0), (0, 0), (1, 1), (20, 20)] {
            vendor[4..6].copy_from_slice(&count.to_be_bytes());
            assert_eq!(vendor_recipes(&vendor)?, vec![0; length]);
        }
        vendor[4..6].copy_from_slice(&21i16.to_be_bytes());
        assert!(vendor_recipes(&vendor).is_err());
        assert!(vendor_recipes(&vendor[..4]).is_err());
        Ok(())
    }

    #[test]
    #[ignore = "requires both extracted executables; no cooking or devices"]
    fn original_crafting_preserves_all_consumed_recipes() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        for disc in [1, 2] {
            let executable = fs::read(local.join(format!("extracted/disc{disc}/sys/main.dol")))?;
            let (catalogue, _) = parse(&executable)?;
            assert_eq!(
                (catalogue.recipes.len(), catalogue.vendors.len()),
                (151, 22)
            );
            // Full semantic snapshot: every consumed recipe, vendor list and text.
            let value = serde_json::to_value(catalogue)?;
            assert_eq!(
                crate::digest(&serde_json::to_vec(&value)?),
                "69b92bf68c9712d953afef0c642fce1169d77d7f64acb16da37ef4e56d5ee55c",
                "disc {disc}"
            );
        }
        Ok(())
    }
}
