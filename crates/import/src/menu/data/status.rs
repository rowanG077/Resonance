use super::*;
use crate::item::Definition;
use resonance_content::menu_data::{
    ConditionText, Element, EquipmentEffect, EquipmentProperties, StatusData, StatusText,
};

pub(super) fn properties(id: usize, row: &Definition) -> Result<EquipmentProperties> {
    use resonance_content::menu_data::{
        EquipmentAilment as Ailment, ExStat, ExStatBonus, TpDiscount,
    };
    ensure!(row.attack_element <= 8, "invalid weapon element");
    let mut value = EquipmentProperties {
        attack_element: row
            .attack_element
            .checked_sub(1)
            .map(|i| Element::ALL[usize::from(i)]),
        neutral_resistance: row.resistance_modifiers[0],
        resistance: Element::ALL
            .into_iter()
            .zip(&row.resistance_modifiers[1..9])
            .filter_map(|(element, &v)| (v != 0).then_some((element, v)))
            .collect(),
        critical_chance_bonus: row.critical_chance_bonus,
        technique_drift: row.technique_drift,
        species_bonus: (97..=108)
            .contains(&row.primary_effect)
            .then(|| u16::from(row.primary_effect - 96)),
        physical_damage_boost: id == 415,
        physical_damage_reduction: id == 416,
        magic_damage_boost: id == 417,
        kills_damage: [158, 174, 189, 204, 218, 227, 241, 257, 273].contains(&id),
        rescue: match id {
            414 => Some(resonance_content::menu_data::EquipmentRescue::Chance),
            457 => Some(resonance_content::menu_data::EquipmentRescue::Consumable),
            _ => None,
        },
        caption_ids: std::iter::once(row.primary_effect)
            .chain(row.effects.iter().map(|slot| slot.effect))
            .filter(|&id| id != 0)
            .collect(),
        ..Default::default()
    };
    if let Some((stat, percent)) = match id {
        454 => Some((ExStat::MaxHp, 30)),
        455 => Some((ExStat::MaxTp, 30)),
        419 => Some((ExStat::Defense, 10)),
        399 => Some((ExStat::Defense, 15)),
        398 => Some((ExStat::Defense, 5)),
        418 => Some((ExStat::Strength, 10)),
        _ => None,
    } {
        value.stat_bonuses.push(ExStatBonus { stat, percent });
    }
    // Decode source slots here; runtime properties have no slot-dependent meaning.
    for (effect, active) in std::iter::once((row.primary_effect, false))
        .chain(row.effects.iter().map(|slot| (slot.effect, true)))
    {
        match effect {
            0 => {}
            81 => value.gald_one_and_a_half = true,
            82 => value.gald_double = true,
            83 => value.quick_escape = true,
            1..=5 | 7 | 10 | 12..=14 | 39 | 47..=53 | 60..=61 | 86..=87 if !active => {}
            1 => {
                value.immunities.insert(Ailment::Poison);
            }
            2 => {
                value.immunities.insert(Ailment::Stun);
            }
            3 => {
                value.immunities.insert(Ailment::Paralysis);
            }
            4 => {
                value.immunities.insert(Ailment::Weak);
            }
            5 => {
                value.immunities.insert(Ailment::Petrify);
            }
            7 => {
                value.immunities.insert(Ailment::Curse);
            }
            10 => {
                value.immunities.insert(Ailment::Heavy);
            }
            12 => {
                value.immunities.insert(Ailment::PhysicalAilments);
            }
            13 => value.ailment_resistance = true,
            14 => value.short_stun = true,
            39 => {
                value.ailments.insert(Ailment::Heavy);
            }
            47 => value.faster_casting = true,
            48 => value.tp_discount = value.tp_discount.max(TpDiscount::Third),
            49 => value.tp_discount = TpDiscount::Half,
            50 => value.hp_regeneration += 1,
            51 => value.tp_regeneration += 1,
            52 => value.hp_regeneration += 3,
            53 => value.tp_regeneration += 3,
            60 => value.kill_hp_recovery = true,
            61 => value.kill_tp_recovery = true,
            73 if active => value.movement_bonus += 1,
            74 if active => value.movement_bonus -= 1,
            86 => {
                value.ailments.insert(Ailment::Curse);
                value.defense_halved = true;
                value.experience_percent = value.experience_percent.max(50);
            }
            87 => {
                value
                    .ailments
                    .extend([Ailment::Poison, Ailment::Curse, Ailment::PhysicalAilments]);
                value.experience_percent = 100;
            }
            33 | 38 | 64 | 65 | 68 | 73 | 74 | 84 | 85 | 97..=108 => {}
            _ => value.unsupported_modifier = true,
        }
    }
    Ok(value)
}

#[test]
fn equipment_properties_decode_native_behavior_and_independent_captions() {
    use resonance_content::menu_data::{EquipmentAilment, TpDiscount};
    let mut bytes = [0; 60];
    bytes[0x12] = 17;
    bytes[0x13] = 8;
    bytes[0x14] = 99;
    bytes[0x1b] = 5;
    bytes[0x1c] = (-2i8) as u8;
    bytes[0x23] = 3;
    bytes[0x26] = 102;
    bytes[0x28] = 3;
    bytes[0x2c] = 1;
    bytes[0x31] = (-2i8) as u8;
    let mut definition = Definition::decode(&[], &bytes).unwrap();
    let value = properties(454, &definition).unwrap();
    assert_eq!(value.attack_element, Some(Element::Darkness));
    assert_eq!(value.neutral_resistance, 5);
    assert_eq!(value.critical_chance_bonus, 17);
    assert_eq!(value.technique_drift, -2);
    assert_eq!(
        value.resistance,
        [(Element::Water, -2), (Element::Darkness, 3)].into()
    );
    assert_eq!(value.species_bonus, Some(3));
    assert_eq!(
        value.immunities,
        [EquipmentAilment::Paralysis, EquipmentAilment::Poison].into()
    );
    assert_eq!(value.caption_ids, [99, 102, 3, 1]);
    assert_eq!(value.stat_bonuses[0].percent, 30);
    for (id, expected) in [
        (
            414,
            Some(resonance_content::menu_data::EquipmentRescue::Chance),
        ),
        (
            457,
            Some(resonance_content::menu_data::EquipmentRescue::Consumable),
        ),
        (454, None),
    ] {
        assert_eq!(properties(id, &definition).unwrap().rescue, expected);
    }
    definition.primary_effect = 49;
    let value = properties(0, &definition).unwrap();
    assert_eq!(value.species_bonus, None);
    assert_eq!(value.tp_discount, TpDiscount::None);
    definition.effects[0].effect = 49;
    assert_eq!(
        properties(0, &definition).unwrap().tp_discount,
        TpDiscount::Half
    );
    definition.attack_element = 9;
    assert!(properties(0, &definition).is_err());
}

pub(super) fn cook(
    ui: &crate::all_assets::status_ui::Catalogue,
    items: &[Item],
) -> Result<(StatusData, StatusText)> {
    let mut ids: std::collections::BTreeSet<_> = items
        .iter()
        .flat_map(|item| item.properties.caption_ids.iter().copied())
        .collect();
    ids.extend(
        ui.overrides
            .iter()
            .flat_map(|pair| [pair.effect, pair.suppresses]),
    );
    ids.extend(&ui.ailment_protection.suppresses);
    let captions = ids
        .iter()
        .map(|&id| Ok((id, ui.effect(id)?.to_owned())))
        .collect::<Result<_>>()?;
    let equipment_effects = ids
        .into_iter()
        .map(|id| {
            Ok((
                id,
                EquipmentEffect {
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
    let condition = |index: usize| -> Result<String> {
        Ok(ui
            .text(ui.conditions[index].context("null status condition label")?)
            .to_owned())
    };
    Ok((
        StatusData { equipment_effects },
        StatusText {
            conditions: Some(ConditionText {
                poison: condition(5)?,
                severe_poison: condition(6)?,
                paralysis: condition(7)?,
                petrified: condition(8)?,
                curse: condition(9)?,
                attack_up: condition(12)?,
                attack_down: condition(13)?,
                defense_up: condition(14)?,
                defense_down: condition(15)?,
                accuracy_up: condition(16)?,
                accuracy_down: condition(17)?,
                magic_attack_up: condition(18)?,
                magic_attack_down: condition(19)?,
                magic_defense_up: condition(20)?,
                knockout: condition(31)?,
            }),
            equipment_effects: captions,
            technical_type: ui.text(ui.technical_type).to_owned(),
            strike_type: ui.text(ui.strike_type).to_owned(),
        },
    ))
}
