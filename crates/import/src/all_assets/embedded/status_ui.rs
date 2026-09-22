//! Status labels, name formats and equipment descriptions with display suppression rules.
use super::text::{TextPool, TextRef};
use crate::{dol, read::u32 as word};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
#[cfg(test)]
use std::path::Path;

const EFFECTS: u32 = 0x80199bd0;
const CONDITIONS: u32 = 0x801995a4;
const OVERRIDES: u32 = 0x8035c93c;
const LABELS: u32 = 0x80199398;
const FULL_NAMES: u32 = 0x80211fb8;

super::text::record! {
    pub(crate) struct Labels(r: Option<TextRef>) {
        pub(crate) title: Option<TextRef> => r[0],
        pub(crate) next: Option<TextRef> => r[1],
        pub(crate) strength: Option<TextRef> => r[2],
        pub(crate) defense: Option<TextRef> => r[3],
        pub(crate) slash: Option<TextRef> => r[4],
        pub(crate) accuracy: Option<TextRef> => r[5],
        pub(crate) attack: Option<TextRef> => r[6],
        pub(crate) thrust: Option<TextRef> => r[7],
        pub(crate) evasion: Option<TextRef> => r[8],
        pub(crate) intelligence: Option<TextRef> => r[9],
        pub(crate) luck: Option<TextRef> => r[10],
        pub(crate) weapon: Option<TextRef> => r[11],
        pub(crate) body: Option<TextRef> => r[12],
        pub(crate) head: Option<TextRef> => r[13],
        pub(crate) arm: Option<TextRef> => r[14],
        pub(crate) accessory_1: Option<TextRef> => r[15],
        pub(crate) accessory_2: Option<TextRef> => r[16],
        pub(crate) element_attack: Option<TextRef> => r[17],
        pub(crate) element_defense: Option<TextRef> => r[18],
        pub(crate) weak: Option<TextRef> => r[19],
        pub(crate) absorb: Option<TextRef> => r[20],
        pub(crate) invalid: Option<TextRef> => r[21],
        pub(crate) reduce: Option<TextRef> => r[22],
        pub(crate) growth: Option<TextRef> => r[23],
        pub(crate) growth_hp: Option<TextRef> => r[24],
        pub(crate) growth_tp: Option<TextRef> => r[25],
        pub(crate) growth_strength: Option<TextRef> => r[26],
        pub(crate) growth_defense: Option<TextRef> => r[27],
        pub(crate) growth_intelligence: Option<TextRef> => r[28],
        pub(crate) growth_evasion: Option<TextRef> => r[29],
        pub(crate) growth_accuracy: Option<TextRef> => r[30],
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Catalogue {
    texts: Vec<String>,
    pub(crate) labels: Labels,
    /// Character slots 0 through 8, preserving aliases and nullable source pointers.
    pub(crate) full_name_formats: [Option<TextRef>; 9],
    /// Regal uses this format until story flag 0x1b reveals his surname.
    pub(crate) hidden_surname_format: TextRef,
    pub(crate) level: TextRef,
    pub(crate) experience: TextRef,
    pub(crate) resistance_fallback: TextRef,
    /// Indexed by the raw item effect byte, including unused and empty entries.
    pub(crate) equipment_effects: Vec<Option<TextRef>>,
    /// Indexed by the character's status bit, including aliases and empty labels.
    pub(crate) conditions: [Option<TextRef>; 32],
    pub(crate) overrides: [EffectOverride; 4],
    pub(crate) ailment_protection: Protection,
    pub(crate) technical_type: TextRef,
    pub(crate) strike_type: TextRef,
}

impl Catalogue {
    pub(crate) fn text(&self, reference: TextRef) -> &str {
        &self.texts[reference.0]
    }

    pub(crate) fn required_text(&self, reference: Option<TextRef>) -> Result<&str> {
        Ok(self.text(reference.context("null required status text")?))
    }

    pub(crate) fn effect(&self, id: u8) -> Result<&str> {
        let reference = self
            .equipment_effects
            .get(usize::from(id))
            .context("equipment effect outside description table")?
            .context("null equipment effect description")?;
        Ok(self.text(reference))
    }
}

/// Hide the weaker description when both effects are equipped.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct EffectOverride {
    pub(crate) effect: u8,
    pub(crate) suppresses: u8,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Protection {
    pub(crate) effect: u8,
    pub(crate) suppresses: Vec<u8>,
}

fn protection(executable: &[u8]) -> Result<Protection> {
    let immediate = |address, opcode| -> Result<u16> {
        let instruction = word(dol::slice(executable, address, 4)?, 0)?;
        ensure!(
            instruction & 0xffff_0000 == opcode,
            "unexpected ailment suppression comparison"
        );
        Ok(instruction as u16)
    };
    // Equality, a wrapping byte range, then two more equalities all clear the label.
    for (address, instruction) in [
        (0x800a4418, 0x40820058),
        (0x800a4434, 0x41820024),
        (0x800a443c, 0x5400063e),
        (0x800a4444, 0x40810014),
        (0x800a444c, 0x4182000c),
        (0x800a4454, 0x40820008),
        (0x800a4458, 0x98a8002b),
    ] {
        ensure!(
            word(dol::slice(executable, address, 4)?, 0)? == instruction,
            "unexpected ailment suppression branch at {address:#x}"
        );
    }
    let effect = immediate(0x800a4414, 0x28000000)?.try_into()?;
    let equal = [
        immediate(0x800a4430, 0x28090000)?,
        immediate(0x800a4448, 0x28090000)?,
        immediate(0x800a4450, 0x28090000)?,
    ];
    let adjustment = immediate(0x800a4438, 0x38090000)? as u8;
    let limit = immediate(0x800a4440, 0x28000000)?;
    Ok(Protection {
        effect,
        suppresses: (0..=u8::MAX)
            .filter(|&id| {
                equal.contains(&u16::from(id)) || u16::from(id.wrapping_add(adjustment)) <= limit
            })
            .collect(),
    })
}

pub(crate) fn read(executable: &[u8]) -> Result<Catalogue> {
    let mut texts = TextPool::default();
    let equipment_effects = texts.table(executable, EFFECTS, 109)?;
    let conditions = texts.array(executable, CONDITIONS)?;
    let pairs = dol::slice(executable, OVERRIDES, 8)?;
    let overrides: [EffectOverride; 4] = std::array::from_fn(|i| EffectOverride {
        effect: pairs[i * 2],
        suppresses: pairs[i * 2 + 1],
    });
    let ailment_protection = protection(executable)?;
    ensure!(
        overrides
            .iter()
            .flat_map(|p| [p.effect, p.suppresses])
            .chain([ailment_protection.effect])
            .chain(ailment_protection.suppresses.iter().copied())
            .all(|id| usize::from(id) < equipment_effects.len()),
        "status suppression outside effect table"
    );
    let technical_type = texts
        .reference(executable, 0x8035d46c)?
        .context("null technical label")?;
    let strike_type = texts
        .reference(executable, 0x8035d474)?
        .context("null strike label")?;
    let labels: [Option<TextRef>; 31] = texts.array(executable, LABELS)?;
    let full_name_formats = texts.array(executable, FULL_NAMES)?;
    let hidden_surname_format = texts.required(executable, 0x8035c948)?;
    let level = texts.required(executable, 0x8035c94c)?;
    let experience = texts.required(executable, 0x8035c950)?;
    let resistance_fallback = texts.required(executable, 0x8035c930)?;
    Ok(Catalogue {
        labels: Labels::from_refs(&labels),
        full_name_formats,
        hidden_surname_format,
        level,
        experience,
        resistance_fallback,
        equipment_effects,
        conditions,
        overrides,
        ailment_protection,
        technical_type,
        strike_type,
        texts: texts.values,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    #[ignore = "requires both extracted discs; no media conversion or playback"]
    fn original_status_ui_preserves_labels_conditions_and_suppression() -> Result<()> {
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
            let labels = &restored.labels;
            assert_eq!(restored.required_text(labels.title)?, "Status");
            assert_eq!(labels.strength, labels.growth_strength);
            assert_eq!(labels.defense, labels.growth_defense);
            assert_eq!(labels.intelligence, labels.growth_intelligence);
            assert_eq!(labels.evasion, labels.growth_evasion);
            assert_eq!(labels.accuracy, labels.growth_accuracy);
            assert_eq!(restored.full_name_formats[2], restored.full_name_formats[3]);
            assert_eq!(
                restored.required_text(restored.full_name_formats[7])?,
                "%s Bryant"
            );
            for (reference, expected) in [
                (restored.hidden_surname_format, "%s"),
                (restored.level, "Lv"),
                (restored.experience, "EXP"),
                (restored.resistance_fallback, ""),
            ] {
                assert_eq!(restored.text(reference), expected);
            }
            let pairs: Vec<_> = restored
                .overrides
                .iter()
                .flat_map(|p| [p.effect, p.suppresses])
                .collect();
            assert_eq!(pairs, dol::slice(&executable, OVERRIDES, 8)?);
            assert_eq!(pairs, [82, 81, 49, 48, 2, 14, 80, 79]);
            assert_eq!(restored.ailment_protection.effect, 12);
            assert_eq!(
                restored.ailment_protection.suppresses,
                [1, 3, 4, 5, 6, 7, 9, 10]
            );
            assert_eq!(restored.equipment_effects.len(), 109);
            assert_eq!(restored.effect(108)?, "Added damage to angels");
            assert_eq!(restored.effect(6)?, "");
            assert_eq!(restored.equipment_effects[0], restored.equipment_effects[6]);
            assert_eq!(restored.equipment_effects[0], restored.conditions[10]);
            assert_eq!(restored.conditions[0], restored.conditions[4]);
            assert!(restored.effect(109).is_err());

            // Empty labels and absent labels have different display semantics.
            for (address, replacement) in [
                (LABELS + 8, 0u32.to_be_bytes().to_vec()),
                (FULL_NAMES, 0u32.to_be_bytes().to_vec()),
                (0x8035c930, b"?\0".to_vec()),
                (EFFECTS + 108 * 4, 0u32.to_be_bytes().to_vec()),
                (CONDITIONS + 31 * 4, 0x8035d46cu32.to_be_bytes().to_vec()),
                (0x800a4414, 0x2800000du32.to_be_bytes().to_vec()),
                (0x800a4430, 0x28090002u32.to_be_bytes().to_vec()),
                (0x800a4438, 0x3809fffcu32.to_be_bytes().to_vec()),
            ] {
                let slice = dol::slice(&executable, address, replacement.len())?;
                let offset = slice.as_ptr() as usize - executable.as_ptr() as usize;
                executable[offset..offset + replacement.len()].copy_from_slice(&replacement);
            }
            let changed = read(&executable)?;
            assert!(changed.labels.strength.is_none());
            assert!(changed.required_text(changed.labels.strength).is_err());
            assert!(changed.labels.growth_strength.is_some());
            assert!(changed.full_name_formats[0].is_none());
            assert_eq!(changed.text(changed.resistance_fallback), "?");
            assert!(changed.equipment_effects[108].is_none() && changed.effect(108).is_err());
            assert_eq!(changed.conditions[31], Some(changed.technical_type));
            assert_eq!(changed.ailment_protection.effect, 13);
            assert_eq!(
                changed.ailment_protection.suppresses,
                [2, 4, 5, 6, 7, 8, 9, 10]
            );
            let at = dol::slice(&executable, OVERRIDES, 1)?.as_ptr() as usize
                - executable.as_ptr() as usize;
            executable[at] = 109;
            assert!(
                read(&executable).is_err(),
                "suppression must name an existing effect"
            );
        }
        Ok(())
    }
}
