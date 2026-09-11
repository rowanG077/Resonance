use super::*;
use resonance_content::menu_data::{Element, EquipmentEffect, EquipmentProperties, StatusData};

pub(super) fn properties(row: &[u8]) -> Result<EquipmentProperties> {
    ensure!(row[0x13] <= 8, "invalid weapon element");
    Ok(EquipmentProperties {
        attack_element: row[0x13]
            .checked_sub(1)
            .map(|i| Element::ALL[usize::from(i)]),
        resistance: Element::ALL
            .into_iter()
            .zip(&row[0x1c..0x24])
            .filter_map(|(element, &v)| (v != 0).then_some((element, v as i8)))
            .collect(),
        effects: [row[0x14], row[0x26], row[0x28], row[0x2a], row[0x2c]]
            .into_iter()
            .filter(|&id| id != 0)
            .collect(),
    })
}

pub(super) fn cook(
    executable: &[u8],
    items: &[Item],
    text: &impl Fn(&[u8], usize) -> Result<String>,
) -> Result<StatusData> {
    const LABELS: u32 = 0x80199398;
    let pairs = dol::slice(executable, 0x8035c93c, 8)?;
    let mut ids: std::collections::BTreeSet<_> = items
        .iter()
        .flat_map(|item| item.properties.effects.iter().copied())
        .collect();
    ids.extend(pairs);
    // The all-ailment protection supersedes these individual protections.
    let ailment_protections = [1, 3, 4, 5, 6, 7, 9, 10];
    ids.extend(ailment_protections);
    let equipment_effects = ids
        .into_iter()
        .map(|id| {
            Ok((
                id,
                EquipmentEffect {
                    description: text(
                        dol::slice(executable, LABELS + 0x838 + u32::from(id) * 4, 4)?,
                        0,
                    )?,
                    suppresses: pairs
                        .chunks_exact(2)
                        .filter_map(|p| (p[0] == id).then_some(p[1]))
                        .chain(ailment_protections.into_iter().filter(|_| id == 12))
                        .collect(),
                },
            ))
        })
        .collect::<Result<_>>()?;
    Ok(StatusData {
        conditions: dol::slice(executable, LABELS + 0x20c, 32 * 4)?
            .chunks_exact(4)
            .map(|r| text(r, 0))
            .collect::<Result<Vec<_>>>()?
            .try_into()
            .unwrap(),
        equipment_effects,
        technical_type: text(&0x8035d46cu32.to_be_bytes(), 0)?,
        strike_type: text(&0x8035d474u32.to_be_bytes(), 0)?,
    })
}
