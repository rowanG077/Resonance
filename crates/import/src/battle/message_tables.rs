//! Verbatim battle notices and result text, including native formatting markers.
use super::embedded::{self, Layout};
use crate::rel::Rel;
use anyhow::Result;
use resonance_content::battle::ui::{NoticeKind, ResultMessage, StealText};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

pub(super) const FAMILY: &str = "battle-messages";

#[derive(Clone, Copy, Serialize)]
pub(super) struct MessageLayout {
    /// Fourteen relocated pairs in section 4; strings may live in another section.
    pub notices: usize,
    pub result_section: usize,
    pub results: [usize; 12],
    /// Format, verb, item-plus-Gald suffix, Gald alone, and both Rover notice lines.
    pub steal: [usize; 6],
    pub steal_prefix: bool,
    pub result_messages: [usize; 11],
    /// Escape, defeat, cancel orders, hit count and damage labels.
    pub hud: [usize; 5],
    /// Overlimit, defense, accuracy, evasion and status cancellation.
    pub extra_notices: [[usize; 2]; 5],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ResultText {
    Experience,
    Bonus,
    MaxCombo,
    Gald,
    Time,
    PositiveGrade,
    NegativeGrade,
    ItemsFound,
    CookHeading,
    ExSkillEffect,
    Info,
    CookButton,
}

impl ResultText {
    const ALL: [Self; 12] = [
        Self::Experience,
        Self::Bonus,
        Self::MaxCombo,
        Self::Gald,
        Self::Time,
        Self::PositiveGrade,
        Self::NegativeGrade,
        Self::ItemsFound,
        Self::CookHeading,
        Self::ExSkillEffect,
        Self::Info,
        Self::CookButton,
    ];
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Messages {
    /// Native notice kind minus one selects the two displayed lines.
    pub notices: BTreeMap<NoticeKind, [String; 2]>,
    pub hud: HudText,
    pub results: BTreeMap<ResultText, String>,
    pub steal_format: String,
    pub steal: StealText,
    pub result_messages: BTreeMap<ResultMessage, String>,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct HudText {
    pub escape_banner: String,
    pub defeat_banner: String,
    pub cancel_orders: String,
    pub combo_hits: String,
    pub combo_damage: String,
}

pub(super) fn read(rel: &Rel, layout: &Layout) -> Result<Messages> {
    let layout = layout.messages;
    let text = |offset| rel.text((layout.result_section, offset));
    let mut notices = BTreeMap::new();
    for (row, kind) in NoticeKind::ALL.into_iter().take(14).enumerate() {
        let line = |column: usize| rel.text(rel.pointer(4, layout.notices + row * 8 + column * 4)?);
        notices.insert(kind, [line(0)?, line(1)?]);
    }
    for (kind, [first, second]) in NoticeKind::ALL
        .into_iter()
        .skip(14)
        .zip(layout.extra_notices)
    {
        notices.insert(kind, [text(first)?, text(second)?]);
    }
    let results = ResultText::ALL
        .into_iter()
        .zip(layout.results)
        .map(|(kind, offset)| Ok((kind, rel.text((layout.result_section, offset))?)))
        .collect::<Result<_>>()?;
    let [format, verb, with_gald, gald_only, first, second] = layout.steal;
    let verb = text(verb)?;
    let (prefix, suffix) = if layout.steal_prefix {
        (verb, String::new())
    } else {
        (String::new(), verb)
    };
    Ok(Messages {
        notices,
        hud: HudText {
            escape_banner: text(layout.hud[0])?,
            defeat_banner: text(layout.hud[1])?,
            cancel_orders: text(layout.hud[2])?,
            combo_hits: text(layout.hud[3])?,
            combo_damage: text(layout.hud[4])?,
        },
        results,
        steal_format: text(format)?,
        steal: StealText {
            prefix,
            suffix,
            with_gald: text(with_gald)?,
            gald_only: text(gald_only)?,
            rover_notice: [text(first)?, text(second)?],
        },
        result_messages: ResultMessage::ALL
            .into_iter()
            .zip(layout.result_messages)
            .map(|(kind, offset)| Ok((kind, text(offset)?)))
            .collect::<Result<_>>()?,
    })
}

pub(crate) fn cook_all(file: &Path, output: &Path) -> Result<Option<Vec<String>>> {
    let Some((_, layout)) = Layout::identify(file) else {
        return Ok(None);
    };
    embedded::write(
        file,
        output,
        FAMILY,
        &read(&Rel::read(file)?, &layout)?,
        serde_json::json!({"layout": layout.messages,
            "notices":{"section":4,"first_kind":1,"count":14,"stride":8},
            "result_order":ResultText::ALL}),
    )
    .map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Context;
    use std::{collections::BTreeSet, fs};

    #[test]
    fn messages_preserve_formatting_and_shift_jis_and_reject_invalid_references() -> Result<()> {
        let mut bytes = vec![0; 144];
        bytes.extend_from_slice(b"GAIN %+02d\0\x97\xbf\x97\x9d\0");
        let mut rel = Rel {
            sections: vec![(0, 0), (0, 0), (0, 0), (0, 0), (4, 112), (144, 16)],
            pointers: (0..28).map(|i| ((4, i * 4), (5, 0))).collect(),
            local_targets: BTreeSet::new(),
            bytes,
        };
        let layout = Layout {
            messages: MessageLayout {
                notices: 0,
                result_section: 5,
                results: [11; 12],
                steal: [0; 6],
                steal_prefix: true,
                result_messages: [0; 11],
                hud: [0; 5],
                extra_notices: [[0; 2]; 5],
            },
            ..Layout::RETAIL
        };
        let messages = read(&rel, &layout)?;
        assert_eq!(
            messages.notices[&NoticeKind::MagicEffect],
            ["GAIN %+02d", "GAIN %+02d"]
        );
        assert_eq!(messages.results[&ResultText::CookButton], "料理");
        rel.pointers.remove(&(4, 27 * 4));
        assert!(read(&rel, &layout).is_err());
        rel.pointers.insert((4, 27 * 4), (5, 16));
        assert!(read(&rel, &layout).is_err());
        rel.pointers.insert((4, 27 * 4), (5, 0));
        rel.bytes[155] = 0x81;
        rel.bytes[156] = 0;
        assert!(read(&rel, &layout).is_err());
        rel.bytes[155..].fill(b'x');
        assert!(read(&rel, &layout).is_err());
        Ok(())
    }

    /// Restrict the source evidence to address loads in executable section 1.
    fn code_references(rel: &Rel) -> Result<BTreeSet<(usize, usize)>> {
        use crate::read::u32 as word;
        let bytes = &rel.bytes;
        let import = word(bytes, 40)? as usize;
        let mut targets = BTreeSet::new();
        for entry in (import..import + word(bytes, 44)? as usize).step_by(8) {
            if word(bytes, entry)? != word(bytes, 0)? {
                continue;
            }
            let mut cursor = word(bytes, entry + 4)? as usize;
            let mut section = 0;
            loop {
                let record = bytes.get(cursor..cursor + 8).context("REL relocation")?;
                match record[2] {
                    203 => break,
                    202 => section = record[3],
                    4 | 6 if section == 1 => {
                        targets.insert((record[3] as usize, word(record, 4)? as usize));
                    }
                    _ => {}
                }
                cursor += 8;
            }
        }
        Ok(targets)
    }

    #[test]
    #[ignore = "requires both original extracted discs; only publishes small JSON tables"]
    fn original_messages_preserve_native_strings_in_every_module() -> Result<()> {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join(FAMILY));
        let mut publications = BTreeMap::new();
        let result = (|| -> Result<()> {
            for disc in [1, 2] {
                let destination = output.join(format!("disc{disc}"));
                for module in [
                    "US_r_Top2Btl.rel",
                    "r_Top2Btl.rel",
                    "US_Top2Btl.rel",
                    "US_m_Top2Btl.rel",
                    "Top2Btl.rel",
                    "m_Top2Btl.rel",
                    "Top2BtlD.rel",
                ] {
                    let file = extracted.join(format!("disc{disc}/files/{module}"));
                    let (_, layout) = Layout::identify(&file).context("missing message layout")?;
                    let rel = Rel::read(&file)?;
                    let messages = read(&rel, &layout)?;
                    let english = module.starts_with("US_");
                    let mut notices = [
                        ["CRITICAL", "DAMAGE"],
                        ["GUARD", "BREAK"],
                        ["STATUS", "EFFECT"],
                        ["GOT", "ITEM"],
                        ["HEAL", "HP"],
                        ["HEAL", "TP"],
                        ["LEVEL", "UP"],
                        ["ITEM", "EFFECT"],
                        ["EX SKILL", "EFFECT"],
                        ["WEPON", "BREAK"],
                        ["STATUS", "UP"],
                        ["STATUS", "DOWN"],
                        ["NEW", "EX SKILL"],
                        ["MAGIC", "EFFECT"],
                    ];
                    if !english {
                        for (index, pair) in [
                            (3, ["ITEM", "GET!"]),
                            (6, ["LEVEL", "UP!"]),
                            (8, ["EXSKILL", "EFFECT"]),
                            (10, ["STATUS", "UP!"]),
                            (11, ["STATUS", "DONW!"]),
                            (12, ["NEW", "EXSKILL"]),
                        ] {
                            notices[index] = pair;
                        }
                    }
                    for (kind, expected) in NoticeKind::ALL.into_iter().zip(notices) {
                        assert_eq!(
                            messages.notices[&kind], expected,
                            "{disc}/{module}/{kind:?}"
                        );
                    }
                    let extra = if english {
                        [
                            ["OVER", "LIMIT"],
                            ["DEFENSE", "DOWN"],
                            ["ACC", "DOWN"],
                            ["AVOID", "DOWN"],
                            ["STATUS", "CANCEL"],
                        ]
                    } else {
                        [
                            ["OVER", "LIMITS"],
                            ["DEFFENCE", "DOWN"],
                            ["HIT", "DOWN"],
                            ["ABOID", "DOWN"],
                            ["STATUS", "CANCEL"],
                        ]
                    };
                    for (kind, expected) in NoticeKind::ALL.into_iter().skip(14).zip(extra) {
                        assert_eq!(
                            messages.notices[&kind], expected,
                            "{disc}/{module}/{kind:?}"
                        );
                    }
                    assert_eq!(
                        messages.hud,
                        HudText {
                            escape_banner: if english {
                                "Escaped"
                            } else {
                                "逃げきった"
                            }
                            .into(),
                            defeat_banner: if english { "Defeated" } else { "全滅した" }.into(),
                            cancel_orders: if english {
                                "Cancel orders"
                            } else {
                                "号令解除"
                            }
                            .into(),
                            combo_hits: "HITS".into(),
                            combo_damage: "DAMAGE".into(),
                        },
                        "{disc}/{module}"
                    );
                    let results = [
                        "EXP   %7d",
                        "BONUS +%6d",
                        "MAX  %4d HIT",
                        "GALD  %8d",
                        "TIME  %d%d'%d%d\"%d%d",
                        "GRADE   +%2d.%d%d",
                        "GRADE   -%2d.%d%d",
                        if english {
                            "ITEM(S) FOUND"
                        } else {
                            "GET ITEM LIST"
                        },
                        "COOK",
                        if english {
                            "EX SKILL EFFECT"
                        } else {
                            "EXSKILL"
                        },
                        "INFO",
                        if english { "Cook" } else { "料理" },
                    ];
                    for (kind, expected) in ResultText::ALL.into_iter().zip(results) {
                        assert_eq!(
                            messages.results[&kind], expected,
                            "{disc}/{module}/{kind:?}"
                        );
                    }
                    assert_eq!(messages.steal_format, "%s", "{disc}/{module}");
                    let [prefix, suffix, with_gald, gald_only, first, second] = if english {
                        ["Stole ", "", " and Gald", "Gald", "STEAL", "ANYTHING"]
                    } else {
                        ["", "を盗んだ", "とガルド", "ガルド", "ANYTING", "STEEL"]
                    };
                    assert_eq!(
                        messages.steal,
                        StealText {
                            prefix: prefix.into(),
                            suffix: suffix.into(),
                            with_gald: with_gald.into(),
                            gald_only: gald_only.into(),
                            rover_notice: [first.into(), second.into()],
                        },
                        "{disc}/{module}"
                    );
                    let formats = if english {
                        [
                            "%s's max HP increased",
                            "%s max HP increased",
                            "%s's max TP increased",
                            "%s max TP increased",
                            "%s gained extra EXP",
                            "Acquired additional Gald",
                            "%s acquired \"%s\"",
                            "Earned the title, \"%s\"",
                            "Successfully prepared %s",
                            "Failed at making %s",
                            "%s discovered a Compound EX Skill",
                        ]
                    } else {
                        [
                            "%sのＨＰ最大値が上昇",
                            "%sのＨＰ最大値が上昇",
                            "%sのＴＰ最大値が上昇",
                            "%sのＴＰ最大値が上昇",
                            "%sの取得経験値が増加",
                            "取得ガルドが増加",
                            "%sが「%s」を修得",
                            "称号「%s」を取得",
                            "%sの料理に成功しました",
                            "%sの料理に失敗・・・",
                            "%sは複合ＥＸスキルを発見",
                        ]
                    };
                    assert_eq!(messages.result_messages.len(), ResultMessage::ALL.len());
                    for (kind, expected) in ResultMessage::ALL.into_iter().zip(formats) {
                        assert_eq!(
                            messages.result_messages[&kind], expected,
                            "{disc}/{module}/{kind:?}"
                        );
                    }
                    let references = code_references(&rel)?;
                    assert!(
                        references.contains(&(4, layout.messages.notices)),
                        "{module}"
                    );
                    for offset in layout
                        .messages
                        .results
                        .into_iter()
                        .chain(layout.messages.steal)
                        .chain(layout.messages.result_messages)
                        .chain(layout.messages.hud)
                        .chain(layout.messages.extra_notices.into_iter().flatten())
                    {
                        assert!(
                            references.contains(&(layout.messages.result_section, offset)),
                            "{module}/{offset:x}"
                        );
                    }
                    let paths = cook_all(&file, &destination)?.context("unrecognized module")?;
                    assert_eq!(
                        crate::embedded::read::<Messages>(&destination, FAMILY, module)?,
                        messages
                    );
                    *publications.entry(paths[0].clone()).or_insert(0) += 1;
                }
            }
            let mut copies: Vec<_> = publications.into_values().collect();
            copies.sort_unstable();
            assert_eq!(copies, [6, 8]);
            Ok(())
        })();
        if output.exists() {
            fs::remove_dir_all(output)?;
        }
        result
    }
}
