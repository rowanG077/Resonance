use super::*;
use resonance_content::menu_data::{MapLocation, MapShop, MapShopVariant, WorldMapData};

pub(super) fn cook(
    executable: &[u8],
    text: &impl Fn(&[u8], usize) -> Result<String>,
) -> Result<WorldMapData> {
    let word = |row: &[u8], at| u32::from_be_bytes(row[at..at + 4].try_into().unwrap());
    let half = |row: &[u8], at| u16::from_be_bytes(row[at..at + 2].try_into().unwrap());
    let shop_list = |address| -> Result<Vec<u8>> {
        if address == 0 {
            return Ok(Vec::new());
        }
        let count = usize::from(dol::slice(executable, address, 1)?[0]);
        ensure!(count <= 8, "invalid map shop list");
        Ok(dol::slice(executable, address + 1, count)?.to_vec())
    };
    let mut locations = std::collections::BTreeMap::new();
    for (world, address) in [0x8026ae80, 0x8026b650].into_iter().enumerate() {
        let mut terminated = false;
        for local in 1..128u16 {
            let row = dol::slice(executable, address + u32::from(local) * 20, 20)?;
            let position = [word(row, 0) as i32, word(row, 4) as i32];
            if position == [-1, -1] {
                terminated = true;
                break;
            }
            let id = (world as u16) * 256 + local;
            let mut location = MapLocation {
                name: text(row, 16)?,
                point: position.map(|v| (v / 200) as i16),
                listed: row[14] & 0x80 != 0,
                visit_alias: matches!(id, 43 | 44).then_some(7),
                shops: if local < 11 {
                    let pointer = dol::slice(
                        executable,
                        0x80227f00 + world as u32 * 44 + u32::from(local) * 4,
                        4,
                    )?;
                    shop_list(word(pointer, 0))?
                } else {
                    Vec::new()
                },
                shop_variants: Vec::new(),
            };
            let variant = match id {
                7 => Some((0x2e, 500_000, 0x8035a190, 0x8035a194)),
                8 => Some((3, 301, 0x8035a198, 0x8035a19c)),
                262 => Some((0, 0x014fc8f0, 0x8035a1b8, 0x8035a1bc)),
                _ => None,
            };
            if let Some((global, at_least, before, after)) = variant {
                location.shops = shop_list(before)?;
                location.shop_variants.push(MapShopVariant {
                    global: global + 16,
                    at_least,
                    shops: shop_list(after)?,
                });
            }
            locations.insert(id, location);
        }
        ensure!(terminated, "unterminated world location table");
    }
    let field_locations = dol::slice(executable, 0x801e4060, 0x3348)?
        .chunks_exact(24)
        .enumerate()
        .filter_map(|(field, row)| {
            let id = half(row, 6);
            (!matches!(id, 0 | 0x100 | 0x200)).then_some((field as u32, id))
        })
        .collect();
    let shops = dol::slice(executable, 0x80230980, 52 * 48)?
        .chunks_exact(48)
        .map(|row| {
            let count = usize::from(half(row, 4));
            ensure!(count <= 21, "invalid map shop inventory");
            Ok(MapShop {
                name: text(row, 0)?,
                items: row[6..6 + count * 2]
                    .chunks_exact(2)
                    .map(|v| half(v, 0))
                    .collect(),
            })
        })
        .collect::<Result<_>>()?;
    Ok(WorldMapData {
        names: [
            text(dol::slice(executable, 0x8019d650 + 152, 4)?, 0)?,
            text(dol::slice(executable, 0x8019d650 + 156, 4)?, 0)?,
        ],
        locations,
        field_locations,
        shops,
    })
}
