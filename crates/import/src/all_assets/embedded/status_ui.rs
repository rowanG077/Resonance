//! Status labels, name formats and equipment descriptions with display suppression rules.
use super::text::{TextPool, TextRef, TextSource};
use crate::{dol, read::u32 as word};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::Path;

const FAMILY: &str = "status-ui";
const EFFECTS: u32 = 0x80199bd0;
const CONDITIONS: u32 = 0x801995a4;
const OVERRIDES: u32 = 0x8035c93c;
const LABELS: u32 = 0x80199398;
const FULL_NAMES: u32 = 0x80211fb8;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Labels {
    pub(crate) title: Option<TextRef>,
    pub(crate) next: Option<TextRef>,
    pub(crate) strength: Option<TextRef>,
    pub(crate) defense: Option<TextRef>,
    pub(crate) slash: Option<TextRef>,
    pub(crate) accuracy: Option<TextRef>,
    pub(crate) attack: Option<TextRef>,
    pub(crate) thrust: Option<TextRef>,
    pub(crate) evasion: Option<TextRef>,
    pub(crate) intelligence: Option<TextRef>,
    pub(crate) luck: Option<TextRef>,
    pub(crate) weapon: Option<TextRef>,
    pub(crate) body: Option<TextRef>,
    pub(crate) head: Option<TextRef>,
    pub(crate) arm: Option<TextRef>,
    pub(crate) accessory_1: Option<TextRef>,
    pub(crate) accessory_2: Option<TextRef>,
    pub(crate) element_attack: Option<TextRef>,
    pub(crate) element_defense: Option<TextRef>,
    pub(crate) weak: Option<TextRef>,
    pub(crate) absorb: Option<TextRef>,
    pub(crate) invalid: Option<TextRef>,
    pub(crate) reduce: Option<TextRef>,
    pub(crate) growth: Option<TextRef>,
    pub(crate) growth_hp: Option<TextRef>,
    pub(crate) growth_tp: Option<TextRef>,
    pub(crate) growth_strength: Option<TextRef>,
    pub(crate) growth_defense: Option<TextRef>,
    pub(crate) growth_intelligence: Option<TextRef>,
    pub(crate) growth_evasion: Option<TextRef>,
    pub(crate) growth_accuracy: Option<TextRef>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Catalogue {
    texts: Vec<String>,
    pub(crate) labels: Labels,
    /// Character slots 0 through 8, preserving aliases and nullable source pointers.
    pub(crate) full_name_formats: [Option<TextRef>; 9],
    /// The final word of the source symbol is outside the nine character formats.
    pub(crate) full_name_storage: u32,
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

fn parse(executable: &[u8]) -> Result<(Catalogue, Vec<TextSource>)> {
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
    let full_name_storage = word(dol::slice(executable, FULL_NAMES + 36, 4)?, 0)?;
    let hidden_surname_format = texts.required(executable, 0x8035c948)?;
    let level = texts.required(executable, 0x8035c94c)?;
    let experience = texts.required(executable, 0x8035c950)?;
    let resistance_fallback = texts.required(executable, 0x8035c930)?;
    Ok((
        Catalogue {
            labels: Labels {
                title: labels[0],
                next: labels[1],
                strength: labels[2],
                defense: labels[3],
                slash: labels[4],
                accuracy: labels[5],
                attack: labels[6],
                thrust: labels[7],
                evasion: labels[8],
                intelligence: labels[9],
                luck: labels[10],
                weapon: labels[11],
                body: labels[12],
                head: labels[13],
                arm: labels[14],
                accessory_1: labels[15],
                accessory_2: labels[16],
                element_attack: labels[17],
                element_defense: labels[18],
                weak: labels[19],
                absorb: labels[20],
                invalid: labels[21],
                reduce: labels[22],
                growth: labels[23],
                growth_hp: labels[24],
                growth_tp: labels[25],
                growth_strength: labels[26],
                growth_defense: labels[27],
                growth_intelligence: labels[28],
                growth_evasion: labels[29],
                growth_accuracy: labels[30],
            },
            full_name_formats,
            full_name_storage,
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
        },
        texts.sources,
    ))
}

pub(crate) fn read(executable: &[u8]) -> Result<Catalogue> {
    Ok(parse(executable)?.0)
}

pub(super) fn cook(file: &Path, executable: &[u8], output: &Path) -> Result<Vec<String>> {
    let (catalogue, sources) = parse(executable)?;
    crate::embedded::write(
        file,
        output,
        FAMILY,
        &catalogue,
        serde_json::json!({
            "labels":{"address":LABELS,"count":31,"stride":4,"source_size":124},
            "full_names":{"address":FULL_NAMES,"count":9,"stride":4,"source_size":40,
                "uninterpreted_storage":{"offset":36,"source_size":4}},
            "direct":{
                "hidden_surname_format":sources[catalogue.hidden_surname_format.0],
                "level":sources[catalogue.level.0],
                "experience":sources[catalogue.experience.0],
                "resistance_fallback":sources[catalogue.resistance_fallback.0],
            },
            "equipment_effects":{"address":EFFECTS,"count":109,"stride":4,"source_size":436},
            "conditions":{"address":CONDITIONS,"count":32,"stride":4,"source_size":128},
            "overrides":{"address":OVERRIDES,"count":4,"stride":2,"source_size":8},
            "ailment_suppression":{"address":0x800a4414u32,"source_size":72},
            "texts":sources,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    #[ignore = "requires both extracted discs; no media conversion or playback"]
    fn original_status_ui_preserves_effect_storage_conditions_and_suppression() -> Result<()> {
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
            let labels = &restored.labels;
            let label_references = [
                labels.title,
                labels.next,
                labels.strength,
                labels.defense,
                labels.slash,
                labels.accuracy,
                labels.attack,
                labels.thrust,
                labels.evasion,
                labels.intelligence,
                labels.luck,
                labels.weapon,
                labels.body,
                labels.head,
                labels.arm,
                labels.accessory_1,
                labels.accessory_2,
                labels.element_attack,
                labels.element_defense,
                labels.weak,
                labels.absorb,
                labels.invalid,
                labels.reduce,
                labels.growth,
                labels.growth_hp,
                labels.growth_tp,
                labels.growth_strength,
                labels.growth_defense,
                labels.growth_intelligence,
                labels.growth_evasion,
                labels.growth_accuracy,
            ];
            for (address, references) in [
                (LABELS, label_references.as_slice()),
                (FULL_NAMES, restored.full_name_formats.as_slice()),
                (EFFECTS, restored.equipment_effects.as_slice()),
                (CONDITIONS, restored.conditions.as_slice()),
            ] {
                let bytes: Vec<_> = references
                    .iter()
                    .flat_map(|reference| {
                        reference
                            .map_or(0, |id| sources[id.0].address)
                            .to_be_bytes()
                    })
                    .collect();
                assert_eq!(
                    bytes,
                    dol::slice(&executable, address, references.len() * 4)?
                );
            }
            assert_eq!(
                restored.full_name_storage.to_be_bytes(),
                dol::slice(&executable, FULL_NAMES + 36, 4)?
            );
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
            for (reference, address, expected) in [
                (restored.hidden_surname_format, 0x8035c948, "%s"),
                (restored.level, 0x8035c94c, "Lv"),
                (restored.experience, 0x8035c950, "EXP"),
                (restored.resistance_fallback, 0x8035c930, ""),
            ] {
                assert_eq!(restored.text(reference), expected);
                assert_eq!(sources[reference.0].address, address);
            }
            for (source, text) in sources.iter().zip(&restored.texts) {
                let (bytes, _, invalid) = encoding_rs::SHIFT_JIS.encode(text);
                assert!(!invalid);
                let terminated = [bytes.as_ref(), &[0]].concat();
                assert_eq!(terminated.len() as u32, source.source_size);
                assert_eq!(
                    terminated,
                    dol::slice(&executable, source.address, source.source_size as usize)?
                );
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

            // Empty text and null storage remain distinct; text identity is its pointer.
            for (address, replacement) in [
                (LABELS + 8, 0u32.to_be_bytes().to_vec()),
                (FULL_NAMES, 0u32.to_be_bytes().to_vec()),
                (FULL_NAMES + 36, 0xdead_beefu32.to_be_bytes().to_vec()),
                (0x8035c930, b"?\0".to_vec()),
                (EFFECTS + 108 * 4, 0u32.to_be_bytes().to_vec()),
                (
                    CONDITIONS + 31 * 4,
                    sources[restored.technical_type.0]
                        .address
                        .to_be_bytes()
                        .to_vec(),
                ),
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
            assert_eq!(changed.full_name_storage, 0xdead_beef);
            assert_eq!(changed.text(changed.resistance_fallback), "?");
            assert!(changed.equipment_effects[108].is_none() && changed.effect(108).is_err());
            assert_eq!(changed.conditions[31], Some(changed.technical_type));
            assert_eq!(changed.ailment_protection.effect, 13);
            assert_eq!(
                changed.ailment_protection.suppresses,
                [2, 4, 5, 6, 7, 8, 9, 10]
            );
        }
        Ok(())
    }
}
