//! Equipment and inventory/book text tables.
use super::text::{TextPool, TextRef};
use crate::dol;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
#[cfg(test)]
use std::path::Path;

const EQUIPMENT: u32 = 0x8019d370;
const INVENTORY: u32 = 0x8019d650;
const DETAILS: u32 = 0x801ab22c;
const CATEGORIES: u32 = 0x801ab2e0;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Catalogue {
    texts: Vec<String>,
    pub(crate) equipment: Equipment,
    pub(crate) inventory: Inventory,
    pub(crate) formats: EquipmentFormats,
    /// Item comparison slots precede the nine inventory heading selectors.
    pub(crate) detail_attributes: [Option<TextRef>; 9],
    pub(crate) inventory_categories: [Option<TextRef>; 9],
    pub(crate) item_categories: Vec<Option<TextRef>>,
    pub(crate) preview_loading: TextRef,
    pub(crate) inventory_formats: InventoryFormats,
    pub(crate) fallback_icons: [i8; 6],
}

impl Catalogue {
    pub(crate) fn text(&self, reference: TextRef) -> &str {
        &self.texts[reference.0]
    }

    pub(crate) fn required_text(&self, reference: Option<TextRef>) -> Result<&str> {
        Ok(self.text(reference.context("null required inventory text")?))
    }
}

super::text::record! {
    pub(crate) struct Slots(r: TextRef) {
        pub(crate) weapon: TextRef => r[0],
        pub(crate) body: TextRef => r[1],
        pub(crate) head: TextRef => r[2],
        pub(crate) arm: TextRef => r[3],
        pub(crate) accessories: [TextRef; 2] => [r[4], r[5]],
    }
}

super::text::record! {
    pub(crate) struct Comparison(r: TextRef) {
        pub(crate) slash: TextRef => r[0],
        pub(crate) thrust: TextRef => r[1],
        pub(crate) defense: TextRef => r[2],
        pub(crate) accuracy: TextRef => r[3],
        pub(crate) evasion: TextRef => r[4],
        pub(crate) intelligence: TextRef => r[5],
        pub(crate) luck: TextRef => r[6],
        /// Non-Lloyd rows replace slash with attack and omit thrust.
        pub(crate) attack: TextRef => r[7],
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Equipment {
    pub(crate) title: TextRef,
    pub(crate) slots: Slots,
    pub(crate) optimal: TextRef,
    pub(crate) remove: TextRef,
    pub(crate) change_order: TextRef,
    pub(crate) optimal_selection: TextRef,
    /// Native choice order: slash, thrust.
    pub(crate) attack_preference: [TextRef; 2],
    pub(crate) comparison: Comparison,
    /// Native sort selector order: alphabetical, parameter.
    pub(crate) ordering: [TextRef; 2],
}

super::text::record! {
    pub(crate) struct EquipmentFormats(r: TextRef) {
        pub(crate) stat_arrow: TextRef => r[0],
        pub(crate) quantity: TextRef => r[1],
        pub(crate) position: TextRef => r[2],
        pub(crate) empty_position: TextRef => r[3],
    }
}

super::text::record! {
    pub(crate) struct InventoryFormats(r: TextRef) {
        pub empty_count: TextRef => r[0],
        pub selected_count: TextRef => r[1],
        pub total_count: TextRef => r[2],
        pub quantity: TextRef => r[3],
        pub party_position: TextRef => r[4],
        pub empty_collection: TextRef => r[5],
        pub collection_percent: TextRef => r[6],
    }
}

const INVENTORY_FORMATS: [(u32, usize); 7] = [
    (0x8035d318, 8),
    (0x8035d320, 8),
    (0x8035d328, 8),
    (0x8035d330, 8),
    (0x8035d338, 4),
    (0x8035d33c, 4),
    (0x8035d340, 8),
];

super::text::record! {
    pub(crate) struct Attributes(r: TextRef) {
        pub(crate) strength: TextRef => r[0],
        pub(crate) slash: TextRef => r[1],
        pub(crate) thrust: TextRef => r[2],
        pub(crate) defense: TextRef => r[3],
        pub(crate) luck: TextRef => r[4],
        pub(crate) accuracy: TextRef => r[5],
        pub(crate) evasion: TextRef => r[6],
        pub(crate) intelligence: TextRef => r[7],
    }
}

super::text::record! {
    pub(crate) struct InventoryActions(r: TextRef) {
        pub(crate) confirm_discard: TextRef => r[0],
        pub(crate) yes: TextRef => r[1],
        pub(crate) no: TextRef => r[2],
        pub(crate) select_item: TextRef => r[3],
        pub(crate) remaining_format: TextRef => r[4],
        pub(crate) select_target: TextRef => r[5],
        pub(crate) use_hint: TextRef => r[6],
        pub(crate) equip_target: TextRef => r[7],
        pub(crate) transform_full: TextRef => r[8],
        pub(crate) transform_empty: TextRef => r[9],
        pub(crate) holy_aura: TextRef => r[10],
        pub(crate) dark_aura: TextRef => r[11],
    }
}

super::text::record! {
    pub(crate) struct Books(r: TextRef) {
        pub(crate) training_manual: TextRef => r[0],
        pub(crate) figurines: TextRef => r[1],
        pub(crate) list: TextRef => r[2],
        pub(crate) collectors_book: TextRef => r[33],
    }
}

super::text::record! {
    pub(crate) struct MonsterLabels(r: TextRef) {
        pub(crate) title: TextRef => r[0],
        /// Indexed by the authored monster family, including unknown at zero.
        pub(crate) categories: [TextRef; 13] => r[1..14].try_into().unwrap(),
        /// Native difficulty order: normal, hard, mania.
        pub(crate) difficulties: [TextRef; 3] => r[14..17].try_into().unwrap(),
        pub(crate) attack: TextRef => r[17],
        pub(crate) experience: TextRef => r[18],
        pub(crate) gald: TextRef => r[19],
        pub(crate) defense: TextRef => r[20],
        pub(crate) drops: TextRef => r[21],
        pub(crate) steal: TextRef => r[22],
        pub(crate) location: TextRef => r[23],
        pub(crate) attack_element: TextRef => r[24],
        pub(crate) weak: TextRef => r[25],
        pub(crate) strong: TextRef => r[26],
        pub(crate) hide_notes: TextRef => r[27],
        pub(crate) display_notes: TextRef => r[28],
        pub(crate) battle_rank: TextRef => r[29],
    }
}

super::text::record! {
    pub(crate) struct Inventory(r: TextRef) {
        pub(crate) title: TextRef => r[0],
        pub(crate) discard: TextRef => r[1],
        pub(crate) transformed: TextRef => r[2],
        pub(crate) discarded: TextRef => r[3],
        pub(crate) comparison: Comparison => Comparison::from_refs(&r[4..12]),
        pub(crate) slots: Slots => Slots::from_refs(&r[12..18]),
        pub(crate) attributes: Attributes => Attributes::from_refs(&r[18..26]),
        pub(crate) actions: InventoryActions => InventoryActions::from_refs(&r[26..38]),
        /// Native world order: Sylvarant, Tethe'alla.
        pub(crate) worlds: [TextRef; 2] => [r[38], r[39]],
        pub(crate) books: Books => Books::from_refs(&r[40..]),
        pub(crate) monster: MonsterLabels => MonsterLabels::from_refs(&r[43..73]),
    }
}

pub(crate) fn read(executable: &[u8]) -> Result<Catalogue> {
    let mut texts = TextPool::default();
    let e = texts.required_table(executable, EQUIPMENT, 23)?;
    let i = texts.required_table(executable, INVENTORY, 74)?;
    // Numeric formats are separately aligned strings, not a pointer array.
    let f = [
        (0x8035cee4, 4),
        (0x8035cee8, 8),
        (0x8035cef0, 8),
        (0x8035cef8, 8),
    ]
    .into_iter()
    .map(|(address, size)| texts.fixed(executable, address, size))
    .collect::<Result<Vec<_>>>()?;
    let inventory_formats = INVENTORY_FORMATS
        .into_iter()
        .map(|(address, size)| texts.fixed(executable, address, size))
        .collect::<Result<Vec<_>>>()?;
    Ok(Catalogue {
        equipment: Equipment {
            title: e[0],
            slots: Slots::from_refs(&e[1..7]),
            optimal: e[7],
            remove: e[8],
            change_order: e[9],
            optimal_selection: e[10],
            attack_preference: [e[11], e[12]],
            comparison: Comparison::from_refs(&e[13..21]),
            ordering: [e[21], e[22]],
        },
        inventory: Inventory::from_refs(&i),
        formats: EquipmentFormats::from_refs(&f),
        detail_attributes: texts.array(executable, DETAILS)?,
        inventory_categories: texts.array(executable, DETAILS + 9 * 4)?,
        item_categories: texts.table(executable, CATEGORIES, 48)?,
        preview_loading: texts.fixed(executable, 0x801aa9f8, 32)?,
        inventory_formats: InventoryFormats::from_refs(&inventory_formats),
        fallback_icons: dol::slice(executable, 0x8035d310, 6)?
            .try_into()
            .map(|v: [u8; 6]| v.map(|v| v as i8))?,
        texts: texts.values,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::read::u32 as word;
    use std::fs;

    #[test]
    #[ignore = "requires both extracted discs; no media conversion or playback"]
    fn original_inventory_ui_preserves_labels_and_control_text() -> Result<()> {
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
            let i = &restored.inventory;
            assert_eq!(i.comparison.slash, i.attributes.slash);
            assert_eq!(i.comparison.defense, i.monster.defense);
            assert_eq!(i.attributes.strength, i.monster.strong);
            assert_eq!(restored.text(i.books.list), "List");
            assert_eq!(restored.text(i.monster.hide_notes), "Hide Notes");
            assert_eq!(restored.text(i.monster.display_notes), "Display Notes");
            assert_eq!(restored.text(restored.formats.quantity), ":%2u");
            assert_eq!(restored.text(restored.formats.position), "%d/%d");
            assert_eq!(restored.text(restored.formats.empty_position), "-/%d");
            assert_eq!(restored.text(restored.formats.stat_arrow), "→");

            let pointer = |index: u32| word(dol::slice(&executable, INVENTORY + index * 4, 4)?, 0);
            let remaining = pointer(30)?;
            let use_hint = pointer(32)?;
            let title = pointer(0)?;
            for (address, replacement) in [
                (remaining, b"Remaining:\x0c\0%d\0".to_vec()),
                (use_hint, b"\x0b\0 : Use\0".to_vec()),
                (DETAILS, 0u32.to_be_bytes().to_vec()),
                (CATEGORIES, title.to_be_bytes().to_vec()),
            ] {
                let source = dol::slice(&executable, address, replacement.len())?;
                let offset = source.as_ptr() as usize - executable.as_ptr() as usize;
                executable[offset..offset + replacement.len()].copy_from_slice(&replacement);
            }
            let changed = read(&executable)?;
            assert_eq!(changed.detail_attributes[0], None);
            assert_eq!(changed.item_categories[0], Some(changed.inventory.title));
            assert_eq!(
                changed.text(changed.inventory.actions.remaining_format),
                "Remaining:\x0c\0%d"
            );
            assert_eq!(
                changed.text(changed.inventory.actions.use_hint),
                "\x0b\0 : Use"
            );
        }
        Ok(())
    }
}
