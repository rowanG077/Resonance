//! Shared HUD artwork coordinates, colors and numeric layouts.
use super::embedded::{self, Layout};
use crate::rel::Rel;
use anyhow::{Context, Result};
use resonance_content::battle::ui::{
    DamageLayout, GaugeStyle, NumberStyle, PartyPanel, ResultsLayout, TargetLayout,
};
use serde::{Deserialize, Serialize};
use std::path::Path;

const FAMILY: &str = "battle-ui-tables";

#[derive(Clone, Copy, Serialize)]
pub(super) struct UiLayout {
    /// Section 4 roots for plain and two-texture punctuation, then party icons.
    pub punctuation: [usize; 2],
    pub party_icons: usize,
    pub party_colors: usize,
    pub gauge_bonus: usize,
    pub combo: usize,
    pub damage: usize,
    pub notices: usize,
    pub result_positions: usize,
    pub result_colors: usize,
    pub texture_bindings: usize,
    pub scan: usize,
    pub unison_palettes: usize,
    pub strategy: usize,
    pub roster: usize,
    pub leader: usize,
    pub marker: usize,
    /// Height/raise, width, trail step and shrink, stored as separate floats.
    pub marker_scale: [usize; 4],
    pub marker_follow_speed: usize,
}

type Color = [u8; 4];

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct UiTables {
    /// ASCII '!' through '/'; zero coordinates remain authored coordinates.
    pub(super) plain_punctuation: [[i16; 2]; 15],
    pub(super) overlay_punctuation: [[i16; 2]; 15],
    /// Actor kind minus one selects [x, y, width, height].
    pub(super) party_icons: [[i16; 4]; 9],
    pub party: PartyStyle,
    pub gauge_bonus: [[Color; 2]; 3],
    pub combo: ComboStyle,
    pub damage: DamageLayout,
    pub notices: NoticeStyle,
    pub results: ResultStyle,
    /// Five active selectors and all three remaining bytes of the allocation.
    pub texture_bindings: [u8; 8],
    pub scan: ScanStyle,
    pub unison_palettes: [u8; 4],
    pub target: TargetStyle,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct ScanStyle {
    pub colors: [[Color; 2]; 2],
    /// The consumer copies the first five bytes; retain the whole allocation.
    pub spacing: [u8; 8],
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct TargetStyle {
    pub strategy_colors: [Color; 3],
    pub strategy_shadow: Color,
    pub entry_colors: [Color; 4],
    pub entry_text: Color,
    pub roster_colors: [Color; 4],
    pub colors: [Color; 5],
    pub leader: [f32; 4],
    /// The marker renderer copies three words; the middle word is its RGBA color.
    pub marker_words: [u32; 3],
    pub marker_scale: [f32; 4],
    pub marker_follow_speed: f32,
}
impl TargetStyle {
    pub fn layout(&self, cancel_orders: String) -> TargetLayout {
        let [height, width, step, shrink] = self.marker_scale;
        TargetLayout {
            entry_colors: self.entry_colors,
            entry_text: self.entry_text,
            cancel_orders,
            strategy_colors: [self.strategy_colors[0], self.strategy_colors[1]],
            strategy_shadow: self.strategy_shadow,
            marker_half_size: [width, height],
            marker_raise: height,
            marker_follow_speed: self.marker_follow_speed,
            marker_trail_step: step,
            marker_trail_shrink: shrink,
            marker_color: self.marker_words[1].to_be_bytes(),
            roster_colors: self.roster_colors,
            leader: self.leader,
            colors: self.colors,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct PartyStyle {
    pub numbers: [[Color; 2]; 2],
    pub bars: [[Color; 4]; 2],
    pub lost_value: Color,
    pub shadows: [Color; 2],
}
impl PartyStyle {
    pub fn panel(&self) -> PartyPanel {
        let gauge = |index, number_offset, bar_offset| GaugeStyle {
            number_offset,
            bar_offset,
            bar_size: [56, 8],
            colors: self.bars[index],
            number_colors: self.numbers[index],
        };
        PartyPanel {
            origin: [12, 388],
            spacing: 112,
            portrait_offset: [0, 8],
            portrait_size: [64, 64],
            portrait_inset: 1,
            hp: gauge(0, [50, 16], [48, 34]),
            tp: gauge(1, [50, 44], [48, 62]),
            number: NumberStyle {
                glyph_size: [16, 24],
                advance: 13,
                skew: 10,
                colors: self.numbers[0],
            },
            bar_shadow: self.shadows[0],
            lost_value_color: self.lost_value,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct ComboStyle {
    pub colors: [Color; 8],
    pub panels: [[Color; 4]; 2],
    pub anchors: [i16; 2],
    pub initial_shown: [u8; 2],
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct NoticeStyle {
    pub colors: [Color; 2],
    pub panels: [[Color; 4]; 2],
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct ResultStyle {
    pub positions: [[i16; 2]; 6],
    /// Six numeric gradients and the shared item/notice heading gradient.
    pub colors: [Color; 14],
    pub shadows: [Color; 3],
    pub strip: Color,
    pub items: [Color; 5],
}
impl ResultStyle {
    pub fn layout(&self) -> ResultsLayout {
        ResultsLayout {
            positions: self.positions,
            colors: std::array::from_fn(|row| [self.colors[row * 2], self.colors[row * 2 + 1]]),
            numbers: NumberStyle {
                glyph_size: [20, 28],
                advance: 18,
                skew: 14,
                colors: [self.colors[0], self.colors[1]],
            },
            strip_color: self.strip,
        }
    }
}

fn colors<const N: usize>(data: &[u8]) -> Result<[Color; N]> {
    let bytes = data.get(..N * 4).context("truncated battle color table")?;
    Ok(std::array::from_fn(|row| {
        bytes[row * 4..row * 4 + 4].try_into().unwrap()
    }))
}

fn table<const ROWS: usize, const COLUMNS: usize>(data: &[u8]) -> Result<[[i16; COLUMNS]; ROWS]> {
    let bytes = data
        .get(..ROWS * COLUMNS * 2)
        .context("truncated battle UI table")?;
    let mut values = [[0; COLUMNS]; ROWS];
    for (value, bytes) in values.iter_mut().flatten().zip(bytes.chunks_exact(2)) {
        *value = i16::from_be_bytes([bytes[0], bytes[1]]);
    }
    Ok(values)
}

pub(super) fn read(rel: &Rel, layout: &Layout) -> Result<UiTables> {
    let ui = layout.ui;
    let at = |offset| rel.at((4, offset));
    let party = ui.party_colors;
    let combo = ui.combo;
    let result = ui.result_colors;
    let float = |offset| crate::read::f32(at(offset)?, 0);
    Ok(UiTables {
        plain_punctuation: table(rel.at((4, ui.punctuation[0]))?)?,
        overlay_punctuation: table(rel.at((4, ui.punctuation[1]))?)?,
        party_icons: table(rel.at((4, ui.party_icons))?)?,
        party: PartyStyle {
            numbers: [colors(at(party)?)?, colors(at(party + 8)?)?],
            bars: [colors(at(party + 16)?)?, colors(at(party + 32)?)?],
            lost_value: colors::<1>(at(party + 48)?)?[0],
            shadows: colors(at(party + 52)?)?,
        },
        gauge_bonus: [
            colors(at(ui.gauge_bonus)?)?,
            colors(at(ui.gauge_bonus + 8)?)?,
            colors(at(ui.gauge_bonus + 16)?)?,
        ],
        combo: ComboStyle {
            colors: colors(at(combo)?)?,
            panels: [colors(at(combo + 32)?)?, colors(at(combo + 48)?)?],
            anchors: table::<1, 2>(at(combo + 64)?)?[0],
            initial_shown: at(combo + 68)?
                .get(..2)
                .context("truncated combo flags")?
                .try_into()?,
        },
        damage: DamageLayout {
            palettes: [
                colors(at(ui.damage)?)?,
                colors(at(ui.damage + 8)?)?,
                colors(at(ui.damage + 16)?)?,
            ],
            sizes: table::<3, 2>(at(ui.damage + 24)?)?.map(|row| row.map(|value| value as u16)),
        },
        notices: NoticeStyle {
            colors: colors(at(ui.notices)?)?,
            panels: [colors(at(ui.notices + 8)?)?, colors(at(ui.notices + 24)?)?],
        },
        results: ResultStyle {
            positions: table(at(ui.result_positions)?)?,
            colors: colors(at(result)?)?,
            shadows: colors(at(result + 56)?)?,
            strip: colors::<1>(at(result + 68)?)?[0],
            items: colors(at(result + 72)?)?,
        },
        texture_bindings: at(ui.texture_bindings)?
            .get(..8)
            .context("texture bindings")?
            .try_into()?,
        scan: ScanStyle {
            colors: [colors(at(ui.scan)?)?, colors(at(ui.scan + 8)?)?],
            spacing: at(ui.scan + 16)?
                .get(..8)
                .context("scan spacing")?
                .try_into()?,
        },
        unison_palettes: at(ui.unison_palettes)?
            .get(..4)
            .context("Unison palettes")?
            .try_into()?,
        target: TargetStyle {
            strategy_colors: colors(at(ui.strategy)?)?,
            strategy_shadow: colors::<1>(at(ui.strategy + 12)?)?[0],
            entry_colors: colors(at(ui.strategy + 16)?)?,
            entry_text: colors::<1>(at(ui.strategy + 32)?)?[0],
            roster_colors: colors(at(ui.roster)?)?,
            colors: colors(at(ui.leader)?)?,
            leader: [
                float(ui.leader + 20)?,
                float(ui.leader + 24)?,
                float(ui.leader + 28)?,
                float(ui.leader + 32)?,
            ],
            marker_words: [
                crate::read::u32(at(ui.marker)?, 0)?,
                crate::read::u32(at(ui.marker)?, 4)?,
                crate::read::u32(at(ui.marker)?, 8)?,
            ],
            marker_scale: [
                float(ui.marker_scale[0])?,
                float(ui.marker_scale[1])?,
                float(ui.marker_scale[2])?,
                float(ui.marker_scale[3])?,
            ],
            marker_follow_speed: float(ui.marker_follow_speed)?,
        },
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
        serde_json::json!({"section":4, "layout":layout.ui,
            "punctuation":{"first_character":b'!',"count":15,"stride":4},
            "party_icons":{"count":9,"stride":8}}),
    )
    .map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeMap, fs};

    fn color_bytes<'a>(colors: impl IntoIterator<Item = &'a Color>) -> Vec<u8> {
        colors.into_iter().flatten().copied().collect()
    }

    #[test]
    fn signed_coordinates_and_zero_rows_survive_and_incomplete_tables_fail() -> Result<()> {
        let mut bytes = vec![0; 72];
        bytes[..4].copy_from_slice(&[0x80, 0, 0x7f, 0xff]);
        bytes[68..].copy_from_slice(&[0xff, 0xff, 0, 1]);
        let rectangles = table::<9, 4>(&bytes)?;
        assert_eq!(rectangles[0], [i16::MIN, i16::MAX, 0, 0]);
        assert_eq!(rectangles[8], [0, 0, -1, 1]);
        assert_eq!(table::<15, 2>(&bytes)?[1], [0, 0]);
        let restored: Vec<_> = rectangles
            .into_iter()
            .flatten()
            .flat_map(i16::to_be_bytes)
            .collect();
        assert_eq!(restored, bytes);
        for length in [0, 1, 71] {
            assert!(table::<9, 4>(&bytes[..length]).is_err());
        }
        assert!(table::<15, 2>(&bytes[..59]).is_err());
        Ok(())
    }

    #[test]
    #[ignore = "requires both original extracted discs; only publishes small JSON tables"]
    fn original_ui_tables_reconstruct_and_deduplicate_all_modules() -> Result<()> {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join(FAMILY));
        let mut publications = BTreeMap::new();
        let mut common = None;
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
                    let (_, layout) = Layout::identify(&file).context("missing UI layout")?;
                    let rel = Rel::read(&file)?;
                    let tables = read(&rel, &layout)?;
                    let ui = layout.ui;
                    for (offset, values) in [
                        (ui.punctuation[0], tables.plain_punctuation.as_flattened()),
                        (ui.punctuation[1], tables.overlay_punctuation.as_flattened()),
                        (ui.party_icons, tables.party_icons.as_flattened()),
                    ] {
                        assert!(rel.local_targets().contains(&(4, offset)), "{module}");
                        let restored: Vec<_> =
                            values.iter().copied().flat_map(i16::to_be_bytes).collect();
                        assert_eq!(restored, rel.at((4, offset))?[..values.len() * 2]);
                    }
                    for offset in ui.punctuation {
                        assert!(rel.local_targets().contains(&(4, offset + 60)), "{module}");
                    }
                    let party = &tables.party;
                    let party_bytes = color_bytes(
                        party
                            .numbers
                            .iter()
                            .flatten()
                            .chain(party.bars.iter().flatten())
                            .chain([&party.lost_value])
                            .chain(&party.shadows),
                    );
                    let mut combo_bytes = color_bytes(
                        tables
                            .combo
                            .colors
                            .iter()
                            .chain(tables.combo.panels.iter().flatten()),
                    );
                    combo_bytes.extend(tables.combo.anchors.into_iter().flat_map(i16::to_be_bytes));
                    combo_bytes.extend(tables.combo.initial_shown);
                    let mut damage_bytes = color_bytes(tables.damage.palettes.iter().flatten());
                    damage_bytes.extend(
                        tables
                            .damage
                            .sizes
                            .into_iter()
                            .flatten()
                            .flat_map(u16::to_be_bytes),
                    );
                    let result = &tables.results;
                    let target = &tables.target;
                    let mut scan_bytes = color_bytes(tables.scan.colors.iter().flatten());
                    scan_bytes.extend(tables.scan.spacing);
                    let strategy_bytes = color_bytes(
                        target
                            .strategy_colors
                            .iter()
                            .chain([&target.strategy_shadow])
                            .chain(&target.entry_colors)
                            .chain([&target.entry_text]),
                    );
                    let mut leader_bytes = color_bytes(&target.colors);
                    leader_bytes.extend(target.leader.into_iter().flat_map(f32::to_be_bytes));
                    for (offset, restored, size) in [
                        (ui.texture_bindings, tables.texture_bindings.to_vec(), 8),
                        (ui.scan, scan_bytes, 24),
                        (ui.unison_palettes, tables.unison_palettes.to_vec(), 4),
                        (ui.strategy, strategy_bytes, 36),
                        (ui.roster, color_bytes(&target.roster_colors), 16),
                        (ui.leader, leader_bytes, 36),
                        (
                            ui.marker,
                            target
                                .marker_words
                                .into_iter()
                                .flat_map(u32::to_be_bytes)
                                .collect(),
                            12,
                        ),
                        (
                            ui.marker_scale[0],
                            target.marker_scale[0].to_be_bytes().to_vec(),
                            4,
                        ),
                        (
                            ui.marker_scale[1],
                            target.marker_scale[1].to_be_bytes().to_vec(),
                            4,
                        ),
                        (
                            ui.marker_scale[2],
                            target.marker_scale[2].to_be_bytes().to_vec(),
                            4,
                        ),
                        (
                            ui.marker_scale[3],
                            target.marker_scale[3].to_be_bytes().to_vec(),
                            4,
                        ),
                        (
                            ui.marker_follow_speed,
                            target.marker_follow_speed.to_be_bytes().to_vec(),
                            4,
                        ),
                        (ui.party_colors, party_bytes, 60),
                        (
                            ui.gauge_bonus,
                            color_bytes(tables.gauge_bonus.iter().flatten()),
                            24,
                        ),
                        (ui.combo, combo_bytes, 70),
                        (ui.damage, damage_bytes, 36),
                        (
                            ui.notices,
                            color_bytes(
                                tables
                                    .notices
                                    .colors
                                    .iter()
                                    .chain(tables.notices.panels.iter().flatten()),
                            ),
                            40,
                        ),
                        (
                            ui.result_positions,
                            result
                                .positions
                                .into_iter()
                                .flatten()
                                .flat_map(i16::to_be_bytes)
                                .collect(),
                            24,
                        ),
                        (
                            ui.result_colors,
                            color_bytes(
                                result
                                    .colors
                                    .iter()
                                    .chain(&result.shadows)
                                    .chain([&result.strip])
                                    .chain(&result.items),
                            ),
                            92,
                        ),
                    ] {
                        assert!(
                            rel.local_targets().contains(&(4, offset)),
                            "{module}/{offset:x}"
                        );
                        assert_eq!(restored.len(), size);
                        assert_eq!(
                            restored,
                            rel.at((4, offset))?[..size],
                            "{module}/{offset:x}"
                        );
                    }
                    assert_eq!(target.marker_scale, [32.0, 24.0, -35.0, 1.5], "{module}");
                    assert_eq!(target.marker_follow_speed, 50.0, "{module}");
                    assert_eq!(tables.texture_bindings[..5], [10, 0, 11, 12, 1], "{module}");
                    assert_eq!(tables.unison_palettes, [25, 25, 24, 24], "{module}");
                    let shared = (tables.plain_punctuation, tables.party_icons);
                    assert_eq!(shared, *common.get_or_insert(shared));
                    assert_eq!(tables.plain_punctuation[4], [208, 48]);
                    assert_eq!(tables.overlay_punctuation[4], [0, 0]);
                    assert!(matches!(
                        &tables.overlay_punctuation[7..9],
                        [[0, 0], [0, 0]] | [[320, 136], [336, 136]]
                    ));
                    let paths = cook_all(&file, &destination)?.context("unrecognized module")?;
                    assert_eq!(
                        serde_json::to_value(crate::embedded::read::<UiTables>(
                            &destination,
                            FAMILY,
                            module
                        )?)?,
                        serde_json::to_value(&tables)?
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
