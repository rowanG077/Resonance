//! Native shop and phase tables directly into the editable world-map menu.
use crate::{dol, text};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    library::{FIELDS, Mappings},
    menu_data::{
        document::unique_name,
        world_map::document::{Document, NamedShop},
    },
};
use serde::Deserialize;
use std::{collections::BTreeMap, fs, path::Path};
use symphonia_constants::ItemId;

const SHOPS: u32 = 0x80230980;
const PHASES: u32 = 0x801e4060;
const INVENTORY: u32 = 0x8019d650;

#[derive(Deserialize)]
struct ShopVisits {
    provenance: String,
    groups: Vec<Vec<u8>>,
}

fn convert(exe: &[u8], fields: &Mappings) -> Result<Document> {
    let items: BTreeMap<_, _> = ItemId::ALL
        .iter()
        .filter_map(|id| id.resource_name().map(|name| (id.raw(), name)))
        .collect();
    let mut shops = BTreeMap::new();
    let mut names = BTreeMap::new();
    for (id, row) in dol::slice(exe, SHOPS, 52 * 48)?
        .chunks_exact(48)
        .enumerate()
    {
        let caption = text::string(exe, dol::u32(row, 0)?)?;
        let count = (dol::u16(row, 4)? as i16).max(0) as usize;
        ensure!(count <= 21, "shop stock exceeds its fixed slots");
        let stock = row[6..6 + count * 2]
            .chunks_exact(2)
            .map(|slot| {
                let item = dol::u16(slot, 0)?;
                Ok(items
                    .get(&item)
                    .with_context(|| format!("unnamed shop item {item}"))?
                    .to_string())
            })
            .collect::<Result<Vec<_>>>()?;
        let name = unique_name(&caption, &shops);
        shops.insert(
            name.clone(),
            NamedShop {
                name: caption.into(),
                items: stock,
            },
        );
        names.insert(id as u32, name);
    }
    let mappings = Mappings::new(names)?;
    let mut field_locations = BTreeMap::new();
    for (id, name) in fields.iter() {
        ensure!(id < 547, "field {id} has no native phase row");
        let location = dol::u16(dol::slice(exe, PHASES + id * 24 + 6, 2)?, 0)?;
        if !matches!(location, 0 | 0x100 | 0x200) {
            field_locations.insert(name.to_owned(), location);
        }
    }
    let label = |index: u32| -> Result<String> {
        text::string(
            exe,
            dol::u32(dol::slice(exe, INVENTORY + (38 + index) * 4, 4)?, 0)?,
        )
    };
    let visits: ShopVisits = serde_json::from_str(include_str!("shop-visits.json"))?;
    ensure!(
        !visits.provenance.is_empty(),
        "missing shop visit provenance"
    );
    let shop_visit_groups = visits
        .groups
        .into_iter()
        .map(|group| {
            group
                .into_iter()
                .map(|id| Ok(mappings.target(id as u32)?.to_owned()))
                .collect::<Result<Vec<_>>>()
        })
        .collect::<Result<_>>()?;
    Ok(Document {
        landmarks: "world/landmarks.json".into(),
        names: serde_json::json!({"sylvarant":label(0)?,"tethe_alla":label(1)?}),
        field_locations,
        mappings,
        shops,
        shop_visit_groups,
    })
}

pub fn cook_menu_world_map(extracted: &Path, library: &Path) -> Result<()> {
    let output = library.join("game/menu/world-map.json");
    ensure!(!output.exists(), "world-map menu output already exists");
    let fields: Mappings = serde_json::from_slice(&fs::read(library.join(FIELDS))?)?;
    let document = convert(&fs::read(extracted.join("disc1/sys/main.dol"))?, &fields)?;
    crate::output::json(&output, &document)?;
    Ok(())
}
