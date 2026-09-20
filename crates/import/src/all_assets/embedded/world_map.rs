//! Complete world locations, shop declarations and exploration requirement tables.
use super::text::{TextPool, TextRef, TextSource};
use crate::{
    dol,
    read::{f32 as float, u16 as half, u32 as word},
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

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
    #[cfg(test)]
    fn source(self) -> u8 {
        match self {
            Self::Disabled => 0,
            Self::Active => 1,
            Self::Blocked => 2,
            Self::Unknown(value) => value,
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
    #[cfg(test)]
    fn source(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Model { id } => id,
            Self::FieldPoint => 0xfe,
            Self::Unmodeled => 0xff,
            Self::Unknown { value } => value,
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
    pub active_count: u16,
    /// All 21 authored slots, including inactive stock and empty entries.
    pub slots: [Option<u16>; 21],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct ListRef(usize);

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct ShopList {
    pub active_count: u8,
    /// The entire fixed declaration after its count, including unconsumed bytes.
    pub slots: Vec<u8>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct StockChange {
    pub before: ListRef,
    pub after: ListRef,
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
    pub shop_lists: Vec<ShopList>,
    pub shop_bindings: [[Option<ListRef>; 11]; 2],
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
    pub(crate) fn shops(&self, reference: Option<ListRef>) -> Result<Vec<u8>> {
        let Some(reference) = reference else {
            return Ok(Vec::new());
        };
        let list = &self.shop_lists[reference.0];
        Ok(list
            .slots
            .get(..usize::from(list.active_count))
            .context("shop-list count exceeds fixed declaration")?
            .to_vec())
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
    values: Vec<ShopList>,
    sources: Vec<ListSource>,
}
impl Lists {
    fn insert(&mut self, executable: &[u8], address: u32, size: usize) -> Result<ListRef> {
        let bytes = dol::slice(executable, address, size)?;
        let id = ListRef(self.values.len());
        self.values.push(ShopList {
            active_count: bytes[0],
            slots: bytes[1..].to_vec(),
        });
        self.sources.push(ListSource {
            address,
            source_size: size,
        });
        self.ids.insert(address, id);
        Ok(id)
    }
    fn reference(&mut self, executable: &[u8], address: u32) -> Result<Option<ListRef>> {
        if address == 0 {
            return Ok(None);
        }
        if let Some(&id) = self.ids.get(&address) {
            return Ok(Some(id));
        }
        let used = usize::from(dol::slice(executable, address, 1)?[0]) + 1;
        Ok(Some(self.insert(
            executable,
            address,
            used.next_multiple_of(4),
        )?))
    }
}

fn parse(executable: &[u8]) -> Result<(Catalogue, Vec<TextSource>, Vec<ListSource>)> {
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
                active_count: half(row, 4)?,
                slots: row[6..]
                    .chunks_exact(2)
                    .map(|row| {
                        Ok(match half(row, 0)? {
                            0 => None,
                            item => Some(item),
                        })
                    })
                    .collect::<Result<Vec<_>>>()?
                    .try_into()
                    .unwrap(),
            })
        })
        .collect::<Result<_>>()?;
    let mut lists = Lists::default();
    for (address, size) in LISTS {
        lists.insert(executable, address, size)?;
    }
    let shop_bindings = dol::slice(executable, SHOP_BINDINGS, 88)?
        .chunks_exact(44)
        .map(|world| {
            Ok(world
                .chunks_exact(4)
                .map(|row| lists.reference(executable, word(row, 0)?))
                .collect::<Result<Vec<_>>>()?
                .try_into()
                .unwrap())
        })
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .unwrap();
    let mut change = |before, after| -> Result<StockChange> {
        Ok(StockChange {
            before: lists
                .reference(executable, before)?
                .context("null before-stock list")?,
            after: lists
                .reference(executable, after)?
                .context("null after-stock list")?,
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
            shop_lists: lists.values,
            shop_bindings,
            story_shops,
            item_rewards,
            party_requirements,
        },
        text.sources,
        lists.sources,
    ))
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
            "locations": LOCATIONS.map(|(address,count)| serde_json::json!({"address":address,"count":count,"stride":20})),
            "world_tables":{"address":WORLD_TABLES,"count":2,"stride":4},
            "shops":{"address":SHOPS,"count":SHOP_COUNT,"stride":48},
            "shop_bindings":{"address":SHOP_BINDINGS,"count":22,"stride":4},
            "story_lists":{"luin":[0x8035a190u32,0x8035a194u32],"hima":[0x8035a198u32,0x8035a19cu32],"flanoir":[0x8035a1b8u32,0x8035a1bcu32]},
            "item_rewards":{"address":REWARDS,"count":22,"stride":4},
            "party_requirements":{"address":REQUIREMENTS,"count":43,"stride":4},"texts":texts,"lists":lists,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::ensure;
    use std::fs;

    fn reconstruct(
        c: &Catalogue,
        texts: &[TextSource],
        lists: &[ListSource],
    ) -> Result<Vec<(u32, Vec<u8>)>> {
        let pointer = |reference: Option<TextRef>| {
            reference.map_or(0, |id| texts[id.0].address).to_be_bytes()
        };
        let list_pointer = |reference: Option<ListRef>| {
            reference.map_or(0, |id| lists[id.0].address).to_be_bytes()
        };
        let mut spans = Vec::new();
        for (world, rows) in c.locations.iter().enumerate() {
            let mut bytes = Vec::new();
            for row in rows {
                bytes.extend(row.position.into_iter().flat_map(i32::to_be_bytes));
                bytes.extend(row.height.to_be_bytes());
                bytes.extend(row.radius.to_be_bytes());
                bytes.push((u8::from(row.listed) * 0x80) | row.interaction.source());
                bytes.push(row.marker.source());
                bytes.extend(pointer(row.text));
            }
            spans.push((LOCATIONS[world].0, bytes));
        }
        spans.push((
            WORLD_TABLES,
            c.world_tables
                .into_iter()
                .flat_map(|world| {
                    world
                        .map_or(0, |world| LOCATIONS[world.index()].0)
                        .to_be_bytes()
                })
                .collect(),
        ));
        let mut shops = Vec::new();
        for shop in &c.shops {
            shops.extend(pointer(shop.name));
            shops.extend(shop.active_count.to_be_bytes());
            shops.extend(
                shop.slots
                    .into_iter()
                    .flat_map(|item| item.unwrap_or(0).to_be_bytes()),
            );
        }
        spans.push((SHOPS, shops));
        spans.push((
            SHOP_BINDINGS,
            c.shop_bindings
                .into_iter()
                .flatten()
                .flat_map(list_pointer)
                .collect(),
        ));
        spans.push((
            REWARDS,
            c.item_rewards
                .iter()
                .flat_map(|row| [row.location, row.item])
                .flat_map(u16::to_be_bytes)
                .collect(),
        ));
        spans.push((
            REQUIREMENTS,
            c.party_requirements
                .iter()
                .flat_map(|row| [row.location, row.required_character])
                .flat_map(u16::to_be_bytes)
                .collect(),
        ));
        for (id, source) in lists.iter().enumerate() {
            let row = &c.shop_lists[id];
            let mut bytes = vec![row.active_count];
            bytes.extend(&row.slots);
            assert_eq!(bytes.len(), source.source_size);
            spans.push((source.address, bytes));
        }
        for (id, source) in texts.iter().enumerate() {
            let (encoded, _, invalid) = encoding_rs::SHIFT_JIS.encode(c.text(TextRef(id)));
            ensure!(
                !invalid,
                "world-map text cannot reconstruct source encoding"
            );
            let bytes = [encoded.as_ref(), &[0]].concat();
            assert_eq!(bytes.len() as u32, source.source_size);
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
    fn original_world_map_reconstructs_all_declarations_and_publishes_shared_data() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join("world-map"));
        fs::create_dir(&output)?;
        let result = (|| -> Result<()> {
            let mut first = None;
            for disc in [1, 2] {
                let file = local.join(format!("disc{disc}/sys/main.dol"));
                let mut executable = fs::read(&file)?;
                let (c, texts, lists) = parse(&executable)?;
                let restored: Catalogue = serde_json::from_slice(&serde_json::to_vec(&c)?)?;
                assert_eq!(c, restored);
                for (address, bytes) in reconstruct(&restored, &texts, &lists)? {
                    assert_eq!(
                        bytes,
                        dol::slice(&executable, address, bytes.len())?,
                        "span {address:#x}"
                    );
                }
                assert_eq!(c.locations.each_ref().map(Vec::len), [100, 83]);
                assert_eq!(c.shops.len(), 52);
                assert_eq!(c.shop_lists.len(), 18);
                assert_eq!(c.item_rewards.len(), 22);
                assert_eq!(c.party_requirements.len(), 43);
                assert!(c.shop_bindings[0][0].is_none() && c.shop_bindings[1][0].is_none());
                assert_eq!(c.story_shops.luin.before, c.shop_bindings[0][7].unwrap());
                assert_eq!(c.story_shops.hima.before, c.shop_bindings[0][8].unwrap());
                assert_eq!(c.story_shops.flanoir.before, c.shop_bindings[1][6].unwrap());
                for (id, count, item) in [(25, 20, 125), (35, 13, 37), (41, 13, 129)] {
                    assert_eq!(usize::from(c.shops[id].active_count), count);
                    assert_eq!(c.shops[id].slots[count], Some(item));
                }
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
                assert_eq!(source["shop_bindings"]["count"], 22);

                // Consumers stop at sentinel fields, not at the declared allocation end.
                let mut early = executable.clone();
                patch(&mut early, LOCATIONS[0].0 + 40, &(-1i32).to_be_bytes())?;
                patch(&mut early, REWARDS, &[0, 0, 0xff, 0xff])?;
                patch(&mut early, REQUIREMENTS, &[0, 0, 0xff, 0xff])?;
                patch(
                    &mut early,
                    SHOPS + 25 * 48 + 6 + 20 * 2,
                    &u16::MAX.to_be_bytes(),
                )?;
                let (early, _, _) = parse(&early)?;
                assert_eq!(early.locations[0].len(), 100);
                assert_eq!(early.item_rewards[0].item, u16::MAX);
                assert_eq!(early.party_requirements[0].required_character, u16::MAX);
                let phases = crate::field_catalogue::read(&executable)?;
                let ui = super::super::inventory_ui::read(&executable)?;
                let projected = crate::menu::world_map(&early, &phases, &ui)?;
                assert_eq!(projected.locations.len(), 82);
                assert_eq!(projected.shops[25].items.len(), 20);
                assert_eq!(projected.shops[25].items.last(), Some(&121));

                let name = texts[c.locations[0][1].text.context("first named location")?.0].address;
                for (address, bytes) in [
                    (LOCATIONS[0].0 + 2 * 20 + 16, name.to_be_bytes().to_vec()),
                    (
                        LOCATIONS[0].0 + 99 * 20 + 8,
                        123.5f32.to_be_bytes().to_vec(),
                    ),
                    (LOCATIONS[0].0 + 99 * 20 + 14, vec![0xff, 0xfd]),
                    (LOCATIONS[0].0 + 99 * 20 + 16, name.to_be_bytes().to_vec()),
                    (LOCATIONS[1].0 + 82 * 20 + 16, vec![0; 4]),
                    (WORLD_TABLES, LOCATIONS[1].0.to_be_bytes().to_vec()),
                    (WORLD_TABLES + 4, vec![0; 4]),
                    (SHOPS, vec![0; 4]),
                    (SHOPS + 4, u16::MAX.to_be_bytes().to_vec()),
                    (SHOPS + 6, u16::MAX.to_be_bytes().to_vec()),
                    (SHOP_BINDINGS + 8, 0x8035a190u32.to_be_bytes().to_vec()),
                    (SHOP_BINDINGS + 12, vec![0; 4]),
                    (0x8035a190, vec![255, 0x53, 0x54, 0x55]),
                    (REWARDS + 21 * 4, vec![0, 0, 0x12, 0x34]),
                    (REQUIREMENTS + 42 * 4, vec![0, 0, 0x56, 0x78]),
                    (name, b"\x0b\0X\0".to_vec()),
                ] {
                    patch(&mut executable, address, &bytes)?;
                }
                let (changed, texts, lists) = parse(&executable)?;
                let restored: Catalogue = serde_json::from_slice(&serde_json::to_vec(&changed)?)?;
                assert_eq!(changed, restored);
                assert_eq!(
                    changed.locations[0][99].interaction,
                    Interaction::Unknown(127)
                );
                assert_eq!(
                    changed.locations[0][99].marker,
                    Marker::Unknown { value: 253 }
                );
                assert_eq!(changed.locations[0][1].text, changed.locations[0][99].text);
                assert_eq!(
                    changed.required_text(changed.locations[0][1].text)?,
                    "\x0b\0X"
                );
                assert_eq!(changed.world_tables, [Some(World::Tethealla), None]);
                assert!(changed.shops[0].name.is_none());
                assert_eq!(changed.shops[0].active_count, u16::MAX);
                assert_eq!(changed.shop_bindings[0][2], changed.shop_bindings[0][7]);
                assert!(changed.shop_bindings[0][3].is_none());
                assert!(
                    changed
                        .shops(Some(changed.story_shops.luin.before))
                        .is_err()
                );
                for (address, bytes) in reconstruct(&restored, &texts, &lists)? {
                    assert_eq!(
                        bytes,
                        dol::slice(&executable, address, bytes.len())?,
                        "changed span {address:#x}"
                    );
                }
                patch(&mut executable, SHOPS, &u32::MAX.to_be_bytes())?;
                assert!(read(&executable).is_err());
            }
            Ok(())
        })();
        fs::remove_dir_all(output)?;
        result
    }
}
