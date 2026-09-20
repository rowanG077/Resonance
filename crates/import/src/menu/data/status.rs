use super::*;
use crate::item::Definition;
use resonance_content::menu_data::{Element, EquipmentEffect, EquipmentProperties, StatusData};

pub(super) fn properties(row: &Definition) -> Result<EquipmentProperties> {
    ensure!(row.attack_element <= 8, "invalid weapon element");
    Ok(EquipmentProperties {
        family_bonus: resonance_content::battle::Family::from_weapon_trait(row.primary_effect),
        attack_element: row
            .attack_element
            .checked_sub(1)
            .map(|i| Element::ALL[usize::from(i)]),
        resistance: Element::ALL
            .into_iter()
            .zip(&row.resistance_modifiers[1..9])
            .filter_map(|(element, &v)| (v != 0).then_some((element, v)))
            .collect(),
        effects: std::iter::once(row.primary_effect)
            .chain(row.effects.iter().map(|slot| slot.effect))
            .filter(|&id| id != 0)
            .collect(),
    })
}

#[test]
fn equipment_properties_keep_element_bounds_and_authored_effect_order() {
    use resonance_content::battle::Family;
    let mut row = [0; 60];
    row[0x13] = 8;
    row[0x14] = 99;
    row[0x1b] = 5;
    row[0x1c] = (-2i8) as u8;
    row[0x23] = 3;
    row[0x24] = 6;
    row[0x26] = 102;
    row[0x28] = 3;
    row[0x2c] = 1;
    let mut definition = Definition::decode(&[], &row).unwrap();
    let value = properties(&definition).unwrap();
    assert_eq!(value.family_bonus, Some(Family(3)));
    assert_eq!(value.attack_element, Some(Element::Darkness));
    assert_eq!(
        value.resistance,
        [(Element::Water, -2), (Element::Darkness, 3)].into()
    );
    assert_eq!(value.effects, [99, 102, 3, 1]);
    definition.primary_effect = 0;
    let value = properties(&definition).unwrap();
    assert_eq!(value.family_bonus, None);
    assert_eq!(value.effects, [102, 3, 1]);
    definition.attack_element = 9;
    assert!(properties(&definition).is_err());
}

pub(super) fn cook(
    ui: &crate::all_assets::status_ui::Catalogue,
    items: &[Item],
) -> Result<StatusData> {
    let mut ids: std::collections::BTreeSet<_> = items
        .iter()
        .flat_map(|item| item.properties.effects.iter().copied())
        .collect();
    ids.extend(
        ui.overrides
            .iter()
            .flat_map(|pair| [pair.effect, pair.suppresses]),
    );
    ids.extend(&ui.ailment_protection.suppresses);
    let equipment_effects = ids
        .into_iter()
        .map(|id| {
            Ok((
                id,
                EquipmentEffect {
                    description: ui.effect(id)?.to_owned(),
                    suppresses: ui
                        .overrides
                        .iter()
                        .filter_map(|p| (p.effect == id).then_some(p.suppresses))
                        .chain(
                            ui.ailment_protection
                                .suppresses
                                .iter()
                                .copied()
                                .filter(|_| id == ui.ailment_protection.effect),
                        )
                        .collect(),
                },
            ))
        })
        .collect::<Result<_>>()?;
    Ok(StatusData {
        conditions: ui
            .conditions
            .iter()
            .map(|reference| {
                Ok(ui
                    .text(reference.context("null status condition label")?)
                    .to_owned())
            })
            .collect::<Result<Vec<_>>>()?
            .try_into()
            .unwrap(),
        equipment_effects,
        technical_type: ui.text(ui.technical_type).to_owned(),
        strike_type: ui.text(ui.strike_type).to_owned(),
    })
}
