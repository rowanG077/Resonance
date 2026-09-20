//! Equipment crafting recipes and vendor selections, including inactive slots.
use super::text::{TextPool, TextRef, TextSource};
use crate::{dol, read::u32 as word};
use anyhow::Result;
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
    /// A zero item ends the consumed prefix; later slots remain authored data.
    pub(crate) item: i16,
    pub(crate) count: u8,
    pub(crate) storage: u8,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Recipe {
    /// Crafting consumes one base item and produces one result item.
    pub(crate) base_item: i16,
    pub(crate) result_item: i16,
    pub(crate) ingredients: [Ingredient; 4],
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Vendor {
    pub(crate) name: Option<TextRef>,
    /// Signed native count; physical decoding does not activate or resolve slots.
    pub(crate) recipe_count: i16,
    /// Zero is a valid recipe index, including inside the active prefix.
    pub(crate) recipes: [i16; 20],
    /// Final halfword outside the declared recipe array.
    pub(crate) storage: u16,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Catalogue {
    texts: Vec<String>,
    pub(crate) recipes: Vec<Recipe>,
    pub(crate) vendors: Vec<Vendor>,
}

fn parse(executable: &[u8]) -> Result<(Catalogue, Vec<TextSource>)> {
    let half = |row: &[u8], at| i16::from_be_bytes([row[at], row[at + 1]]);
    let recipes = dol::slice(executable, RECIPES, RECIPE_COUNT * RECIPE_BYTES)?
        .chunks_exact(RECIPE_BYTES)
        .map(|row| Recipe {
            base_item: half(row, 0),
            result_item: half(row, 2),
            ingredients: std::array::from_fn(|i| Ingredient {
                item: half(row, 4 + i * 4),
                count: row[6 + i * 4],
                storage: row[7 + i * 4],
            }),
        })
        .collect();
    let mut texts = TextPool::default();
    let vendors = dol::slice(executable, VENDORS, VENDOR_COUNT * VENDOR_BYTES)?
        .chunks_exact(VENDOR_BYTES)
        .map(|row| {
            Ok(Vendor {
                name: texts.reference(executable, word(row, 0)?)?,
                recipe_count: half(row, 4),
                recipes: std::array::from_fn(|i| half(row, 6 + i * 2)),
                storage: half(row, 46) as u16,
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
    let (catalogue, texts) = parse(executable)?;
    crate::embedded::write(
        file,
        output,
        FAMILY,
        &catalogue,
        serde_json::json!({
            "recipes":{"address":RECIPES,"count":RECIPE_COUNT,"stride":RECIPE_BYTES,
                "source_size":RECIPE_COUNT*RECIPE_BYTES},
            "vendors":{"address":VENDORS,"count":VENDOR_COUNT,"stride":VENDOR_BYTES,
                "source_size":VENDOR_COUNT*VENDOR_BYTES},
            "texts":texts,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeSet, fs};

    fn reconstruct(c: &Catalogue, sources: &[TextSource]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for recipe in &c.recipes {
            bytes.extend(recipe.base_item.to_be_bytes());
            bytes.extend(recipe.result_item.to_be_bytes());
            for ingredient in &recipe.ingredients {
                bytes.extend(ingredient.item.to_be_bytes());
                bytes.extend([ingredient.count, ingredient.storage]);
            }
        }
        for vendor in &c.vendors {
            bytes.extend(
                vendor
                    .name
                    .map_or(0, |id| sources[id.0].address)
                    .to_be_bytes(),
            );
            bytes.extend(vendor.recipe_count.to_be_bytes());
            bytes.extend(vendor.recipes.into_iter().flat_map(i16::to_be_bytes));
            bytes.extend(vendor.storage.to_be_bytes());
        }
        bytes
    }

    fn check_source(executable: &[u8], c: &Catalogue, sources: &[TextSource]) -> Result<()> {
        let bytes = reconstruct(c, sources);
        assert_eq!(
            bytes.len(),
            RECIPE_COUNT * RECIPE_BYTES + VENDOR_COUNT * VENDOR_BYTES
        );
        assert_eq!(bytes, dol::slice(executable, RECIPES, bytes.len())?);
        for (source, text) in sources.iter().zip(&c.texts) {
            let (encoded, _, invalid) = encoding_rs::SHIFT_JIS.encode(text);
            assert!(!invalid);
            let bytes = [encoded.as_ref(), &[0]].concat();
            assert_eq!(bytes.len() as u32, source.source_size);
            assert_eq!(bytes, dol::slice(executable, source.address, bytes.len())?);
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires both original discs; crafting JSON only, no media conversion or playback"]
    fn original_crafting_catalogue_preserves_recipes_vendor_slots_and_storage() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join(FAMILY));
        let result = (|| -> Result<()> {
            let mut payloads = BTreeSet::new();
            for disc in [1, 2] {
                let file = local.join(format!("disc{disc}/sys/main.dol"));
                let mut executable = fs::read(&file)?;
                let destination = output.join(format!("disc{disc}"));
                let (catalogue, sources) = parse(&executable)?;
                let paths = cook(&file, &executable, &destination)?;
                let restored: Catalogue = crate::embedded::read(&destination, FAMILY, "main.dol")?;
                assert_eq!(restored, catalogue);
                check_source(&executable, &restored, &sources)?;
                payloads.insert(paths[0].clone());
                let provenance: serde_json::Value =
                    serde_json::from_slice(&fs::read(destination.join(&paths[1]))?)?;
                assert_eq!(provenance["source_sha256"], crate::digest(&executable));
                assert_eq!(provenance["recipes"]["source_size"], 3020);
                assert_eq!(provenance["vendors"]["source_size"], 1056);
                assert_eq!((restored.recipes.len(), restored.vendors.len()), (151, 22));
                assert_eq!(
                    (
                        restored.recipes[0].base_item,
                        restored.recipes[0].result_item
                    ),
                    (136, 139)
                );
                assert_eq!(
                    (
                        restored.recipes[0].ingredients[0].item,
                        restored.recipes[0].ingredients[0].count
                    ),
                    (81, 1)
                );
                assert_eq!(restored.vendors[0].recipes[0], 0);
                assert_eq!(restored.vendors[5].recipe_count, 20);

                // Counts never filter physical slots; preserve nulls, pointer aliases,
                // high-bit signed operands, inactive ingredients and the empty last row.
                let alias = word(
                    dol::slice(&executable, VENDORS + 2 * VENDOR_BYTES as u32, 4)?,
                    0,
                )?;
                for (address, replacement) in [
                    (
                        RECIPES,
                        [(-1i16).to_be_bytes(), i16::MIN.to_be_bytes()].concat(),
                    ),
                    (RECIPES + 4, vec![0, 0, 0, 0xa5, 0xff, 0xfe, 0xff, 0x80]),
                    (
                        RECIPES + 150 * RECIPE_BYTES as u32,
                        32767i16.to_be_bytes().to_vec(),
                    ),
                    (VENDORS, 0u32.to_be_bytes().to_vec()),
                    (
                        VENDORS + 4,
                        [0i16.to_be_bytes(), (-1i16).to_be_bytes()].concat(),
                    ),
                    (
                        VENDORS + 44,
                        [i16::MIN.to_be_bytes(), 0xbeefu16.to_be_bytes()].concat(),
                    ),
                    (VENDORS + VENDOR_BYTES as u32, alias.to_be_bytes().to_vec()),
                    (
                        VENDORS + VENDOR_BYTES as u32 + 4,
                        (-2i16).to_be_bytes().to_vec(),
                    ),
                    (
                        VENDORS + 21 * VENDOR_BYTES as u32 + 4,
                        i16::MAX.to_be_bytes().to_vec(),
                    ),
                ] {
                    let at = dol::slice(&executable, address, replacement.len())?.as_ptr() as usize
                        - executable.as_ptr() as usize;
                    executable[at..at + replacement.len()].copy_from_slice(&replacement);
                }
                let (changed, sources) = parse(&executable)?;
                let roundtrip: Catalogue = serde_json::from_slice(&serde_json::to_vec(&changed)?)?;
                assert_eq!(roundtrip, changed);
                check_source(&executable, &roundtrip, &sources)?;
                assert_eq!(
                    (changed.recipes[0].base_item, changed.recipes[0].result_item),
                    (-1, i16::MIN)
                );
                assert_eq!(
                    changed.recipes[0].ingredients[1],
                    Ingredient {
                        item: -2,
                        count: 255,
                        storage: 128
                    }
                );
                assert_eq!(changed.recipes[150].base_item, 32767);
                assert_eq!(changed.vendors[0].name, None);
                assert_eq!(changed.vendors[1].name, changed.vendors[2].name);
                assert_eq!(
                    (
                        changed.vendors[0].recipe_count,
                        changed.vendors[0].recipes[19],
                        changed.vendors[0].storage
                    ),
                    (0, i16::MIN, 0xbeef)
                );
                assert_eq!(changed.vendors[1].recipe_count, -2);
                assert_eq!(changed.vendors[21].recipe_count, i16::MAX);

                let end = dol::slice(&executable, VENDORS, VENDOR_COUNT * VENDOR_BYTES)?.as_ptr()
                    as usize
                    - executable.as_ptr() as usize
                    + VENDOR_COUNT * VENDOR_BYTES;
                assert!(parse(&executable[..end - 1]).is_err());
                let at = dol::slice(&executable, VENDORS, 4)?.as_ptr() as usize
                    - executable.as_ptr() as usize;
                executable[at..at + 4].copy_from_slice(&u32::MAX.to_be_bytes());
                assert!(parse(&executable).is_err());
            }
            assert_eq!(
                payloads.len(),
                1,
                "identical disc tables should share one publication"
            );
            Ok(())
        })();
        fs::remove_dir_all(output)?;
        result
    }
}
