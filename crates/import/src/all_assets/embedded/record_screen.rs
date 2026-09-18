//! Record screen labels and their native statistic/format bindings.
use crate::{dol, read::u32 as word};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

const LABELS: u32 = 0x8019cbb0;
const HEADING: u32 = 0x8035ce20;
const COUNT_ANNOTATION: u32 = 0x8035ce28;
const FORMATS: [(Format, u32); 6] = [
    (Format::PlayTime, 0x8019d268),
    (Format::Gald, 0x8019d284),
    (Format::CommonCount, 0x8019d290),
    (Format::BattleCount, 0x8019d29c),
    (Format::Combo, 0x8019d2a8),
    (Format::Amount, 0x8019d2bc),
];
const DIGIT_GROUPS: [(DigitGroup, u32); 4] = [
    (DigitGroup::First, 0x8035d8f0),
    (DigitGroup::Middle, 0x8035d8e0),
    (DigitGroup::Last, 0x8035d8e8),
    (DigitGroup::Only, 0x8035d8f4),
];

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Catalogue {
    heading: String,
    sections: Vec<Section>,
    /// Preserve spacing and inline text-control bytes in the authored formats.
    formats: BTreeMap<Format, String>,
    digit_group_formats: BTreeMap<DigitGroup, String>,
    /// Drawn alongside saves, clears, encounters and escapes; empty in English.
    count_annotation: String,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Section {
    kind: SectionKind,
    label: String,
    statistics: Vec<Display>,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SectionKind {
    CommonData,
    BattleData,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Statistic {
    MaxPlayTime,
    MaxGald,
    TotalGaldUsed,
    Saves,
    GamesCleared,
    Encounters,
    Escapes,
    MaxCombo,
    MaxDamage,
    MaxGrade,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Format {
    PlayTime,
    Gald,
    CommonCount,
    BattleCount,
    Combo,
    Amount,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DigitGroup {
    First,
    Middle,
    Last,
    Only,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Display {
    statistic: Statistic,
    label: String,
    format: Format,
    argument: Argument,
    count_annotation: bool,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Argument {
    HoursMinutesSeconds { ticks_per_second: u32 },
    GroupedInteger { digits_per_group: u8 },
    Integer { divisor: u32 },
}

fn read(executable: &[u8]) -> Result<Catalogue> {
    use Statistic::*;
    let labels = dol::slice(executable, LABELS, 12 * 4)?;
    let label = |index| dol::text(executable, word(labels, index * 4)?);
    let sections = [
        (
            SectionKind::CommonData,
            [MaxPlayTime, MaxGald, TotalGaldUsed, Saves, GamesCleared],
        ),
        (
            SectionKind::BattleData,
            [Encounters, Escapes, MaxCombo, MaxDamage, MaxGrade],
        ),
    ]
    .into_iter()
    .enumerate()
    .map(|(section, (kind, statistics))| {
        Ok(Section {
            kind,
            label: label(section * 6)?,
            statistics: statistics
                .into_iter()
                .enumerate()
                .map(|(row, statistic)| {
                    // 800AF3CC supplies these arguments; 800E9BB0 groups Gald.
                    let format = match statistic {
                        MaxPlayTime => Format::PlayTime,
                        MaxGald | TotalGaldUsed => Format::Gald,
                        Saves | GamesCleared => Format::CommonCount,
                        Encounters | Escapes => Format::BattleCount,
                        MaxCombo => Format::Combo,
                        MaxDamage | MaxGrade => Format::Amount,
                    };
                    let argument = match statistic {
                        MaxPlayTime => Argument::HoursMinutesSeconds {
                            ticks_per_second: 60,
                        },
                        MaxGald | TotalGaldUsed => Argument::GroupedInteger {
                            digits_per_group: 3,
                        },
                        MaxGrade => Argument::Integer { divisor: 100 },
                        _ => Argument::Integer { divisor: 1 },
                    };
                    Ok(Display {
                        statistic,
                        label: label(section * 6 + row + 1)?,
                        format,
                        argument,
                        count_annotation: matches!(
                            statistic,
                            Saves | GamesCleared | Encounters | Escapes
                        ),
                    })
                })
                .collect::<Result<_>>()?,
        })
    })
    .collect::<Result<_>>()?;
    Ok(Catalogue {
        heading: dol::text(executable, HEADING)?,
        sections,
        formats: FORMATS
            .into_iter()
            .map(|(format, address)| Ok((format, dol::text(executable, address)?)))
            .collect::<Result<_>>()?,
        digit_group_formats: DIGIT_GROUPS
            .into_iter()
            .map(|(group, address)| Ok((group, dol::text(executable, address)?)))
            .collect::<Result<_>>()?,
        count_annotation: dol::text(executable, COUNT_ANNOTATION)?,
    })
}

pub(super) fn cook(file: &Path, executable: &[u8], output: &Path) -> Result<Vec<String>> {
    crate::embedded::write(file, output, "record-screen", &read(executable)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    #[ignore = "requires both extracted executables; no cooking or devices"]
    fn original_record_catalogues_preserve_all_labels_and_formats() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let mut first = None;
        for disc in [1, 2] {
            let mut executable = fs::read(local.join(format!("disc{disc}/sys/main.dol")))?;
            let catalogue = read(&executable)?;
            let restored: Catalogue = serde_json::from_slice(&serde_json::to_vec(&catalogue)?)?;
            assert_eq!(restored, catalogue);
            let check = |address, text: &str| -> Result<()> {
                let mut bytes = text.as_bytes().to_vec();
                bytes.push(0);
                assert_eq!(dol::slice(&executable, address, bytes.len())?, bytes);
                Ok(())
            };
            let labels = dol::slice(&executable, LABELS, 12 * 4)?;
            let displayed = restored.sections.iter().flat_map(|section| {
                std::iter::once(&section.label)
                    .chain(section.statistics.iter().map(|statistic| &statistic.label))
            });
            assert_eq!(displayed.clone().count(), 12);
            for (row, text) in displayed.enumerate() {
                check(word(labels, row * 4)?, text)?;
            }
            for (format, address) in FORMATS {
                check(address, &restored.formats[&format])?;
            }
            for (group, address) in DIGIT_GROUPS {
                check(address, &restored.digit_group_formats[&group])?;
            }
            check(HEADING, &restored.heading)?;
            check(COUNT_ANNOTATION, &restored.count_annotation)?;
            assert_eq!(restored.heading, "RECORD");
            let grade = restored.sections[1].statistics.last().unwrap();
            assert_eq!(grade.statistic, Statistic::MaxGrade);
            assert_eq!(grade.label, "Max Grade");
            assert_eq!(grade.argument, Argument::Integer { divisor: 100 });
            if let Some(previous) = &first {
                assert_eq!(&restored, previous);
            } else {
                first = Some(restored);
            }

            // Labels must follow the source pointers, not their original text/location.
            let replacement = word(labels, 2 * 4)?;
            let offset = labels.as_ptr() as usize - executable.as_ptr() as usize + 4;
            executable[offset..offset + 4].copy_from_slice(&replacement.to_be_bytes());
            let changed = read(&executable)?;
            assert_eq!(changed.sections[0].statistics[0].label, "Max Gald");
            assert_eq!(
                changed.sections[0].statistics[0].statistic,
                Statistic::MaxPlayTime
            );
        }
        Ok(())
    }
}
