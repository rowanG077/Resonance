//! Native shop stock and field destinations in the shared map-menu document.
use crate::{
    output, pe,
    source::Source,
    text::{Bank, Menu},
};
use anyhow::{Result, ensure};
use resonance_content::{
    library::{FIELDS, Mappings},
    menu_data::{
        document::unique_name,
        world_map::document::{Document, NamedShop},
    },
};
use std::{collections::BTreeMap, fs, path::Path};

pub(crate) fn cook(source: &Source, library: &Path) -> Result<()> {
    let bytes = source.require("TOS_TEXT.TXB")?;
    let source = Menu {
        image: pe::Image::parse(&source.executable)?,
        bank: Bank::parse(&bytes)?,
        slot: source.language.slot(),
    };
    let fields: Mappings = serde_json::from_slice(&fs::read(library.join(FIELDS))?)?;
    let items: resonance_content::items::Document =
        serde_json::from_slice(&fs::read(library.join(resonance_content::items::PATH))?)?;
    let shop_names = source.directory(0x8b9484)?;
    let mut shops = BTreeMap::new();
    let mut names = BTreeMap::new();
    for (id, row) in source
        .image
        .slice(0x8b8950, 52 * 48)?
        .chunks_exact(48)
        .enumerate()
    {
        let caption = source.caption(shop_names, pe::word(row, 0)?)?;
        let count = (pe::half(row, 4)? as i16).max(0) as usize;
        ensure!(count <= 21, "shop stock exceeds its fixed slots");
        let stock = row[6..6 + count * 2]
            .chunks_exact(2)
            .map(|slot| {
                let item = pe::half(slot, 0)?;
                Ok(items.mappings.target(u32::from(item))?.to_owned())
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
        ensure!(id < 554, "field {id} has no native phase row");
        let location = pe::half(source.image.slice(0x8cb908 + id * 24 + 6, 2)?, 0)?;
        if !matches!(location, 0 | 0x100 | 0x200) {
            field_locations.insert(name.to_owned(), location);
        }
    }
    let inventory = source.directory(0x8b943c)?;
    let label = |index: u32| source.caption(inventory, 1 + index);
    let document = Document {
        landmarks: "world/landmarks.json".into(),
        names: serde_json::json!({"sylvarant":label(0)?,"tethe_alla":label(1)?}),
        field_locations,
        mappings,
        shops,
        // Native saved discovery already records the selected shop directly.
        shop_visit_groups: Vec::new(),
    };
    document.prepare(&items.mappings, &fields)?;
    output::json(&library.join("game/menu/world-map.json"), &document)
}
