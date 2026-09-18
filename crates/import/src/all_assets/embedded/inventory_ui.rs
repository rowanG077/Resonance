//! Complete equipment and inventory/book text tables, including aliases and storage.
use super::text::{FixedText, TextPool, TextRef, TextSource};
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
    pub(crate) preview_loading: FixedText,
    pub(crate) inventory_formats: InventoryFormats,
    pub(crate) fallback_icons: [i8; 6],
    pub(crate) fallback_icon_storage: [u8; 2],
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
    pub(crate) storage: Option<TextRef>,
}

super::text::record! {
    pub(crate) struct EquipmentFormats(r: TextRef) {
        pub(crate) stat_arrow: TextRef => r[0],
        pub(crate) quantity: TextRef => r[1],
        pub(crate) position: TextRef => r[2],
        pub(crate) empty_position: TextRef => r[3],
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct InventoryFormats {
    pub empty_count: FixedText,
    pub selected_count: FixedText,
    pub total_count: FixedText,
    pub quantity: FixedText,
    pub party_position: FixedText,
    pub empty_collection: FixedText,
    pub collection_percent: FixedText,
}

#[cfg(test)]
impl InventoryFormats {
    fn slots(&self) -> [&FixedText; 7] {
        [
            &self.empty_count,
            &self.selected_count,
            &self.total_count,
            &self.quantity,
            &self.party_position,
            &self.empty_collection,
            &self.collection_percent,
        ]
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

fn parse(executable: &[u8]) -> Result<(Catalogue, Vec<TextSource>)> {
    let mut texts = TextPool::default();
    let mut equipment = texts.table(executable, EQUIPMENT, 24)?;
    let storage = equipment.pop().unwrap();
    let e = equipment
        .into_iter()
        .map(|r| r.context("null required inventory UI text"))
        .collect::<Result<Vec<_>>>()?;
    let i = texts.required_table(executable, INVENTORY, 74)?;
    // Numeric formats are separately aligned strings, not a pointer array.
    let f = [0x8035cee4, 0x8035cee8, 0x8035cef0, 0x8035cef8]
        .into_iter()
        .map(|address| texts.required(executable, address))
        .collect::<Result<Vec<_>>>()?;
    Ok((
        Catalogue {
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
                storage,
            },
            inventory: Inventory::from_refs(&i),
            formats: EquipmentFormats::from_refs(&f),
            detail_attributes: texts.array(executable, DETAILS)?,
            inventory_categories: texts.array(executable, DETAILS + 9 * 4)?,
            item_categories: texts.table(executable, CATEGORIES, 48)?,
            preview_loading: texts.fixed(executable, 0x801aa9f8, 32)?,
            inventory_formats: {
                let mut slots = INVENTORY_FORMATS
                    .into_iter()
                    .map(|(address, size)| texts.fixed(executable, address, size));
                InventoryFormats {
                    empty_count: slots.next().unwrap()?,
                    selected_count: slots.next().unwrap()?,
                    total_count: slots.next().unwrap()?,
                    quantity: slots.next().unwrap()?,
                    party_position: slots.next().unwrap()?,
                    empty_collection: slots.next().unwrap()?,
                    collection_percent: slots.next().unwrap()?,
                }
            },
            fallback_icons: dol::slice(executable, 0x8035d310, 6)?
                .try_into()
                .map(|v: [u8; 6]| v.map(|v| v as i8))?,
            fallback_icon_storage: dol::slice(executable, 0x8035d316, 2)?.try_into()?,
            texts: texts.values,
        },
        texts.sources,
    ))
}

pub(crate) fn read(executable: &[u8]) -> Result<Catalogue> {
    Ok(parse(executable)?.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn slot_refs(s: &Slots) -> [TextRef; 6] {
        [
            s.weapon,
            s.body,
            s.head,
            s.arm,
            s.accessories[0],
            s.accessories[1],
        ]
    }

    fn comparison_refs(s: &Comparison) -> [TextRef; 8] {
        [
            s.slash,
            s.thrust,
            s.defense,
            s.accuracy,
            s.evasion,
            s.intelligence,
            s.luck,
            s.attack,
        ]
    }

    fn ordered_refs(catalogue: &Catalogue) -> (Vec<Option<TextRef>>, Vec<TextRef>) {
        let e = &catalogue.equipment;
        let mut equipment = vec![e.title];
        equipment.extend(slot_refs(&e.slots));
        equipment.extend([e.optimal, e.remove, e.change_order, e.optimal_selection]);
        equipment.extend(e.attack_preference);
        equipment.extend(comparison_refs(&e.comparison));
        equipment.extend(e.ordering);
        let equipment = equipment.into_iter().map(Some).chain([e.storage]).collect();
        let i = &catalogue.inventory;
        let mut inventory = vec![i.title, i.discard, i.transformed, i.discarded];
        inventory.extend(comparison_refs(&i.comparison));
        inventory.extend(slot_refs(&i.slots));
        let a = &i.attributes;
        inventory.extend([
            a.strength,
            a.slash,
            a.thrust,
            a.defense,
            a.luck,
            a.accuracy,
            a.evasion,
            a.intelligence,
        ]);
        let a = &i.actions;
        inventory.extend([
            a.confirm_discard,
            a.yes,
            a.no,
            a.select_item,
            a.remaining_format,
            a.select_target,
            a.use_hint,
            a.equip_target,
            a.transform_full,
            a.transform_empty,
            a.holy_aura,
            a.dark_aura,
        ]);
        inventory.extend(i.worlds);
        inventory.extend([i.books.training_manual, i.books.figurines, i.books.list]);
        let m = &i.monster;
        inventory.push(m.title);
        inventory.extend(m.categories);
        inventory.extend(m.difficulties);
        inventory.extend([
            m.attack,
            m.experience,
            m.gald,
            m.defense,
            m.drops,
            m.steal,
            m.location,
            m.attack_element,
            m.weak,
            m.strong,
            m.hide_notes,
            m.display_notes,
            m.battle_rank,
            i.books.collectors_book,
        ]);
        (equipment, inventory)
    }

    #[test]
    #[ignore = "requires both extracted discs; no media conversion or playback"]
    fn original_inventory_ui_preserves_all_pointers_aliases_and_control_text() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let mut first = None;
        for disc in [1, 2] {
            let mut executable = fs::read(local.join(format!("disc{disc}/sys/main.dol")))?;
            let (catalogue, sources) = parse(&executable)?;
            let encoded = serde_json::to_vec(&catalogue)?;
            let restored: Catalogue = serde_json::from_slice(&encoded)?;
            assert_eq!(restored, catalogue);
            if let Some(expected) = &first {
                assert_eq!(&encoded, expected);
            } else {
                first = Some(encoded);
            }
            let pointer_bytes = |references: Vec<Option<TextRef>>| {
                references
                    .into_iter()
                    .flat_map(|reference| {
                        reference
                            .map_or(0, |id| sources[id.0].address)
                            .to_be_bytes()
                    })
                    .collect::<Vec<_>>()
            };
            let (equipment, inventory) = ordered_refs(&restored);
            assert_eq!(
                pointer_bytes(equipment),
                dol::slice(&executable, EQUIPMENT, 24 * 4)?
            );
            assert_eq!(
                pointer_bytes(inventory.into_iter().map(Some).collect()),
                dol::slice(&executable, INVENTORY, 74 * 4)?
            );
            assert_eq!(
                pointer_bytes(
                    restored
                        .detail_attributes
                        .into_iter()
                        .chain(restored.inventory_categories)
                        .collect()
                ),
                dol::slice(&executable, DETAILS, 18 * 4)?
            );
            assert_eq!(
                pointer_bytes(restored.item_categories.clone()),
                dol::slice(&executable, CATEGORIES, 48 * 4)?
            );
            for (source, text) in sources.iter().zip(&restored.texts) {
                let (bytes, _, invalid) = encoding_rs::SHIFT_JIS.encode(text);
                assert!(!invalid, "cannot reconstruct original text");
                let terminated = [bytes.as_ref(), &[0]].concat();
                assert_eq!(terminated.len() as u32, source.source_size);
                assert_eq!(
                    terminated,
                    dol::slice(&executable, source.address, source.source_size as usize)?
                );
            }
            for (slot, (address, size)) in restored
                .inventory_formats
                .slots()
                .into_iter()
                .zip(INVENTORY_FORMATS)
                .chain([(&restored.preview_loading, (0x801aa9f8, 32))])
            {
                let (bytes, _, invalid) = encoding_rs::SHIFT_JIS.encode(restored.text(slot.text));
                assert!(!invalid);
                assert_eq!(
                    [bytes.as_ref(), &[0], &slot.storage].concat(),
                    dol::slice(&executable, address, size)?
                );
            }
            assert_eq!(
                restored
                    .fallback_icons
                    .map(|v| v as u8)
                    .into_iter()
                    .chain(restored.fallback_icon_storage)
                    .collect::<Vec<_>>(),
                dol::slice(&executable, 0x8035d310, 8)?
            );
            let i = &restored.inventory;
            assert_eq!(i.comparison.slash, i.attributes.slash);
            assert_eq!(i.comparison.defense, i.monster.defense);
            assert_eq!(i.attributes.strength, i.monster.strong);
            assert!(restored.equipment.storage.is_none());
            assert_eq!(restored.text(i.books.list), "List");
            assert_eq!(restored.text(i.monster.hide_notes), "Hide Notes");
            assert_eq!(restored.text(i.monster.display_notes), "Display Notes");
            assert_eq!(restored.text(restored.formats.quantity), ":%2u");
            assert_eq!(restored.text(restored.formats.position), "%d/%d");
            assert_eq!(restored.text(restored.formats.empty_position), "-/%d");
            assert_eq!(restored.text(restored.formats.stat_arrow), "→");

            // Preserve a non-null storage cell and zero-valued control operands.
            let remaining = sources[i.actions.remaining_format.0].address;
            let use_hint = sources[i.actions.use_hint.0].address;
            for (address, replacement) in [
                (
                    EQUIPMENT + 23 * 4,
                    sources[i.title.0].address.to_be_bytes().to_vec(),
                ),
                (remaining, b"Remaining:\x0c\0%d\0".to_vec()),
                (use_hint, b"\x0b\0 : Use\0".to_vec()),
                (DETAILS, 0u32.to_be_bytes().to_vec()),
                (
                    CATEGORIES,
                    sources[i.title.0].address.to_be_bytes().to_vec(),
                ),
            ] {
                let source = dol::slice(&executable, address, replacement.len())?;
                let offset = source.as_ptr() as usize - executable.as_ptr() as usize;
                executable[offset..offset + replacement.len()].copy_from_slice(&replacement);
            }
            let changed = read(&executable)?;
            assert_eq!(changed.detail_attributes[0], None);
            assert_eq!(changed.item_categories[0], Some(changed.inventory.title));
            assert_eq!(changed.equipment.storage, Some(changed.inventory.title));
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
