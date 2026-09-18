//! Complete world locations, shop declarations and exploration requirement tables.
use super::text::{TextPool, TextRef, TextSource};
use crate::{
    dol,
    read::{f32 as float, u16 as half, u32 as word},
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
#[cfg(test)]
use std::path::Path;

#[cfg(test)]
const FAMILY: &str = "world-map";
const LOCATIONS: [(u32, usize); 2] = [(0x8026ae80, 100), (0x8026b650, 83)];
const WORLD_TABLES: u32 = 0x8035a1c8;
const SHOPS: u32 = 0x80230980;
const SHOP_COUNT: usize = 52;
const SHOP_BINDINGS: u32 = 0x80227f00;
const REWARDS: u32 = 0x8026bccc;
const REQUIREMENTS: u32 = 0x8026bd24;
const LISTS: [(u32, usize); 18] = [
    (0x8035a170, 4),
    (0x8035a174, 8),
    (0x8035a17c, 4),
    (0x8035a180, 8),
    (0x8035a188, 8),
    (0x8035a190, 4),
    (0x8035a194, 4),
    (0x8035a198, 4),
    (0x8035a19c, 4),
    (0x8035a1a0, 4),
    (0x8035a1a4, 8),
    (0x8035a1ac, 4),
    (0x8035a1b0, 4),
    (0x8035a1b4, 4),
    (0x8035a1b8, 4),
    (0x8035a1bc, 4),
    (0x8035a1c0, 4),
    (0x8035a1c4, 4),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum World {
    Sylvarant,
    Tethealla,
}
impl World {
    pub(crate) fn index(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Interaction {
    Disabled,
    Active,
    Blocked,
    Unknown(u8),
}
impl Interaction {
    fn read(value: u8) -> Self {
        match value {
            0 => Self::Disabled,
            1 => Self::Active,
            2 => Self::Blocked,
            _ => Self::Unknown(value),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Marker {
    None,
    Model { id: u8 },
    FieldPoint,
    Unmodeled,
    Unknown { value: u8 },
}
impl Marker {
    fn read(value: u8) -> Self {
        match value {
            0 => Self::None,
            id @ 1..=17 => Self::Model { id },
            0xfe => Self::FieldPoint,
            0xff => Self::Unmodeled,
            _ => Self::Unknown { value },
        }
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Location {
    pub position: [i32; 2],
    /// Zero uses terrain height; 0.01 forces zero height; other values are explicit.
    pub height: f32,
    pub radius: u16,
    pub listed: bool,
    pub interaction: Interaction,
    pub marker: Marker,
    pub text: Option<TextRef>,
}
impl Location {
    pub(crate) fn is_terminator(&self) -> bool {
        self.position.contains(&-1)
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Shop {
    pub name: Option<TextRef>,
    pub items: Vec<u16>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct StockChange {
    pub before: Vec<u8>,
    pub after: Vec<u8>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct StoryShops {
    pub luin: StockChange,
    pub hima: StockChange,
    pub flanoir: StockChange,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct ItemReward {
    pub location: u16,
    pub item: u16,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct PartyRequirement {
    pub location: u16,
    pub required_character: u16,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Catalogue {
    texts: Vec<String>,
    pub locations: [Vec<Location>; 2],
    pub world_tables: [Option<World>; 2],
    pub shops: Vec<Shop>,
    /// Every declared list, including those without a current location binding.
    pub shop_lists: Vec<Vec<u8>>,
    pub shop_bindings: [[Option<Vec<u8>>; 11]; 2],
    pub story_shops: StoryShops,
    /// Complete pair tables; consumers stop at the first zero location.
    pub item_rewards: Vec<ItemReward>,
    pub party_requirements: Vec<PartyRequirement>,
}
impl Catalogue {
    pub(crate) fn text(&self, reference: TextRef) -> &str {
        &self.texts[reference.0]
    }
    pub(crate) fn required_text(&self, reference: Option<TextRef>) -> Result<&str> {
        Ok(self.text(reference.context("null required world-map text")?))
    }
    pub(crate) fn world(&self, index: usize) -> Result<&[Location]> {
        Ok(&self.locations[self.world_tables[index]
            .context("null world location table")?
            .index()])
    }
}

fn stock(row: &[u8]) -> Result<Vec<u16>> {
    let count = (half(row, 4)? as i16).max(0) as usize;
    row.get(6..6 + count * 2)
        .context("shop stock count exceeds fixed slots")?
        .chunks_exact(2)
        .map(|item| half(item, 0))
        .collect()
}

fn shop_list(executable: &[u8], address: u32) -> Result<Option<Vec<u8>>> {
    if address == 0 {
        return Ok(None);
    }
    let count = usize::from(dol::slice(executable, address, 1)?[0]);
    let size = LISTS
        .iter()
        .find(|&&(start, _)| start == address)
        .map_or(count + 1, |&(_, size)| size);
    Ok(Some(
        dol::slice(executable, address, size)?
            .get(1..count + 1)
            .context("shop-list count exceeds fixed declaration")?
            .to_vec(),
    ))
}

fn parse(executable: &[u8]) -> Result<(Catalogue, Vec<TextSource>)> {
    let mut text = TextPool::default();
    let locations = LOCATIONS
        .into_iter()
        .map(|(address, count)| {
            dol::slice(executable, address, count * 20)?
                .chunks_exact(20)
                .map(|row| {
                    Ok(Location {
                        position: [word(row, 0)? as i32, word(row, 4)? as i32],
                        height: float(row, 8)?,
                        radius: half(row, 12)?,
                        listed: row[14] & 0x80 != 0,
                        interaction: Interaction::read(row[14] & 0x7f),
                        marker: Marker::read(row[15]),
                        text: text.reference(executable, word(row, 16)?)?,
                    })
                })
                .collect::<Result<Vec<_>>>()
        })
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .unwrap();
    let world_tables = dol::slice(executable, WORLD_TABLES, 8)?
        .chunks_exact(4)
        .map(|row| {
            Ok(match word(row, 0)? {
                0 => None,
                0x8026ae80 => Some(World::Sylvarant),
                0x8026b650 => Some(World::Tethealla),
                pointer => {
                    anyhow::bail!("world pointer {pointer:#x} outside declared location tables")
                }
            })
        })
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .unwrap();
    let shops = dol::slice(executable, SHOPS, SHOP_COUNT * 48)?
        .chunks_exact(48)
        .map(|row| {
            Ok(Shop {
                name: text.reference(executable, word(row, 0)?)?,
                items: stock(row)?,
            })
        })
        .collect::<Result<_>>()?;
    let shop_lists = LISTS
        .into_iter()
        .map(|(address, _)| Ok(shop_list(executable, address)?.unwrap()))
        .collect::<Result<_>>()?;
    let shop_bindings = dol::slice(executable, SHOP_BINDINGS, 88)?
        .chunks_exact(44)
        .map(|world| {
            Ok(world
                .chunks_exact(4)
                .map(|row| shop_list(executable, word(row, 0)?))
                .collect::<Result<Vec<_>>>()?
                .try_into()
                .unwrap())
        })
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .unwrap();
    let change = |before, after| -> Result<StockChange> {
        Ok(StockChange {
            before: shop_list(executable, before)?.context("null before-stock list")?,
            after: shop_list(executable, after)?.context("null after-stock list")?,
        })
    };
    let story_shops = StoryShops {
        luin: change(0x8035a190, 0x8035a194)?,
        hima: change(0x8035a198, 0x8035a19c)?,
        flanoir: change(0x8035a1b8, 0x8035a1bc)?,
    };
    let item_rewards = dol::slice(executable, REWARDS, 22 * 4)?
        .chunks_exact(4)
        .map(|row| {
            Ok(ItemReward {
                location: half(row, 0)?,
                item: half(row, 2)?,
            })
        })
        .collect::<Result<_>>()?;
    let party_requirements = dol::slice(executable, REQUIREMENTS, 43 * 4)?
        .chunks_exact(4)
        .map(|row| {
            Ok(PartyRequirement {
                location: half(row, 0)?,
                required_character: half(row, 2)?,
            })
        })
        .collect::<Result<_>>()?;
    Ok((
        Catalogue {
            texts: text.values,
            locations,
            world_tables,
            shops,
            shop_lists,
            shop_bindings,
            story_shops,
            item_rewards,
            party_requirements,
        },
        text.sources,
    ))
}

pub(crate) fn read(executable: &[u8]) -> Result<Catalogue> {
    Ok(parse(executable)?.0)
}

#[cfg(test)]
pub(crate) fn cook(file: &Path, executable: &[u8], output: &Path) -> Result<Vec<String>> {
    let (catalogue, _) = parse(executable)?;
    crate::embedded::write(file, output, FAMILY, &catalogue)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn patch(executable: &mut [u8], address: u32, bytes: &[u8]) -> Result<()> {
        let offset = dol::slice(executable, address, bytes.len())?.as_ptr() as usize
            - executable.as_ptr() as usize;
        executable[offset..offset + bytes.len()].copy_from_slice(bytes);
        Ok(())
    }

    #[test]
    fn stock_uses_its_signed_count_and_rejects_overflow() -> Result<()> {
        let mut row = [0xff; 48];
        assert!(stock(&row)?.is_empty());
        row[4..6].copy_from_slice(&2u16.to_be_bytes());
        row[6..10].copy_from_slice(&[0, 1, 0, 5]);
        assert_eq!(stock(&row)?, [1, 5]);
        row[4..6].copy_from_slice(&22u16.to_be_bytes());
        assert!(stock(&row).is_err());
        Ok(())
    }

    #[test]
    #[ignore = "requires both original executables; no codecs or devices"]
    fn original_world_map_preserves_all_shops_bindings_and_variants() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = tempfile::tempdir()?;
        let mut first = None;
        for disc in [1, 2] {
            let file = local.join(format!("disc{disc}/sys/main.dol"));
            let mut executable = fs::read(&file)?;
            let (catalogue, _) = parse(&executable)?;
            assert_eq!(catalogue.locations.each_ref().map(Vec::len), [100, 83]);
            assert_eq!(catalogue.shops.len(), 52);
            assert_eq!(catalogue.shop_lists.len(), 18);
            assert_eq!(catalogue.item_rewards.len(), 22);
            assert_eq!(catalogue.party_requirements.len(), 43);
            // Complete semantic stock snapshot from the validated original catalogue.
            let stock = serde_json::json!({
                "shops": catalogue.shops,
                "shop_lists": catalogue.shop_lists,
                "shop_bindings": catalogue.shop_bindings,
                "story_shops": catalogue.story_shops,
            });
            assert_eq!(
                crate::digest(&serde_json::to_vec(&stock)?),
                "5d02674ec5bee6813375880d36067c63ded37722c096646003dae819f1934cfc"
            );
            let paths = cook(&file, &executable, output.path())?;
            assert_eq!(
                crate::embedded::read::<Catalogue>(output.path(), FAMILY, "main.dol")?,
                catalogue
            );
            if let Some(expected) = &first {
                assert_eq!(&paths[0], expected);
            } else {
                first = Some(paths[0].clone());
            }
            // Neither inactive item slots nor alignment bytes are consumed.
            patch(
                &mut executable,
                SHOPS + 25 * 48 + 6 + 20 * 2,
                &u16::MAX.to_be_bytes(),
            )?;
            patch(&mut executable, 0x8035a193, &[0xff])?;
            assert_eq!(read(&executable)?, catalogue);
            // Unknown location flags and metadata beyond a terminator remain recoverable.
            patch(
                &mut executable,
                LOCATIONS[0].0 + 99 * 20 + 14,
                &[0xff, 0xfd],
            )?;
            patch(&mut executable, REWARDS + 21 * 4, &[0, 0, 0x12, 0x34])?;
            let changed = read(&executable)?;
            assert_eq!(
                changed.locations[0][99].interaction,
                Interaction::Unknown(127)
            );
            assert_eq!(
                changed.locations[0][99].marker,
                Marker::Unknown { value: 253 }
            );
            assert_eq!(changed.item_rewards[21].item, 0x1234);
            patch(&mut executable, 0x8035a190, &[255])?;
            assert!(read(&executable).is_err());
        }
        Ok(())
    }
}
