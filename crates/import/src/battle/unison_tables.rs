//! Party cues, staging positions and presentation data for Unison.
use super::embedded::{self, Layout};
use crate::{read::f32 as float, rel::Rel};
use anyhow::{Context, Result};
use resonance_content::battle::unison::CombinedPrelude;
use serde::{Deserialize, Serialize};
use std::path::Path;

const FAMILY: &str = "battle-unison-tables";

#[derive(Clone, Copy, Serialize)]
pub(super) struct UnisonLayout {
    /// Section 4 ranges retain the storage after the nine logical entries.
    pub opener_contact_delays: [usize; 2],
    pub placement: usize,
    pub combined_voices: [usize; 2],
    pub overlimit_voices: [usize; 2],
    pub presentation: UnisonPresentationLayout,
}

#[derive(Clone, Copy, Serialize)]
pub(super) struct UnisonPresentationLayout {
    pub windup_rate: usize,
    pub windup_color: usize,
    pub combined_color: usize,
    /// X/Y of the hidden position; Y/Z of the camera eye.
    pub hidden_position: [usize; 2],
    pub camera_eye: [usize; 2],
    pub camera_target_y: usize,
    pub first_x: usize,
    pub spacing: usize,
    pub short_weapon_penalty: usize,
    pub minimum_distance: usize,
    pub zero: usize,
    pub title: (usize, usize),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct UnisonTables {
    pub(super) opener_contact_delays: [u8; 9],
    pub(super) placement: [[f32; 3]; 4],
    pub(super) overlimit_voices: [u16; 9],
    pub(super) windup_rate: f32,
    pub(super) windup_color: [u8; 4],
    pub(super) combined_prelude: CombinedPrelude,
    pub(super) short_weapon_penalty: f32,
    pub(super) minimum_distance: f32,
    /// Remaining bytes after delays, combined voices and overlimit voices.
    trailing_storage: [Vec<u8>; 3],
}

fn table(rel: &Rel, [start, end]: [usize; 2], length: usize) -> Result<(&[u8], Vec<u8>)> {
    let bytes = end.checked_sub(start).context("reversed Unison table")?;
    let (records, tail) = rel
        .at((4, start))?
        .get(..bytes)
        .context("truncated Unison table")?
        .split_at_checked(length)
        .context("incomplete Unison records")?;
    Ok((records, tail.to_vec()))
}

pub(super) fn read(rel: &Rel, layout: &Layout) -> Result<UnisonTables> {
    let layout = layout.unison;
    let (delays, delay_tail) = table(rel, layout.opener_contact_delays, 9)?;
    let (combined, combined_tail) = table(rel, layout.combined_voices, 18)?;
    let (overlimit, overlimit_tail) = table(rel, layout.overlimit_voices, 18)?;
    let voices = |bytes: &[u8]| {
        std::array::from_fn(|i| u16::from_be_bytes([bytes[i * 2], bytes[i * 2 + 1]]))
    };
    let bytes = rel
        .at((4, layout.placement))?
        .get(..48)
        .context("truncated Unison placement")?;
    let mut placement = [[0.; 3]; 4];
    for (index, value) in placement.iter_mut().flatten().enumerate() {
        *value = float(bytes, index * 4)?;
    }
    let presentation = layout.presentation;
    let scalar = |offset| float(rel.at((4, offset))?, 0);
    let color = |offset| -> Result<[u8; 4]> {
        Ok(rel
            .at((4, offset))?
            .get(..4)
            .context("truncated Unison color")?
            .try_into()?)
    };
    let zero = scalar(presentation.zero)?;
    Ok(UnisonTables {
        opener_contact_delays: delays.try_into()?,
        placement,
        overlimit_voices: voices(overlimit),
        windup_rate: scalar(presentation.windup_rate)?,
        windup_color: color(presentation.windup_color)?,
        combined_prelude: CombinedPrelude {
            hidden_position: [
                scalar(presentation.hidden_position[0])?,
                scalar(presentation.hidden_position[1])?,
                zero,
            ],
            first_x: scalar(presentation.first_x)?,
            spacing: scalar(presentation.spacing)?,
            camera_eye: [
                zero,
                scalar(presentation.camera_eye[0])?,
                scalar(presentation.camera_eye[1])?,
            ],
            camera_target: [zero, scalar(presentation.camera_target_y)?, zero],
            color: color(presentation.combined_color)?,
            voices: voices(combined),
            title: rel.text(presentation.title)?,
        },
        short_weapon_penalty: scalar(presentation.short_weapon_penalty)?,
        minimum_distance: scalar(presentation.minimum_distance)?,
        trailing_storage: [delay_tail, combined_tail, overlimit_tail],
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
        serde_json::json!({"section":4, "layout":layout.unison,
            "party_count":9, "placement":{"count":4,"stride":12}, "voice_stride":2,
            "presentation":{"scalar_count":11,"color_count":2,"title_encoding":"shift_jis"}}),
    )
    .map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeMap, fs};

    #[test]
    fn unison_tables_keep_storage_and_reject_invalid_ranges_and_positions() -> Result<()> {
        let mut layout = Layout::RETAIL;
        layout.unison = UnisonLayout {
            opener_contact_delays: [0, 12],
            placement: 12,
            combined_voices: [60, 80],
            overlimit_voices: [80, 100],
            presentation: UnisonPresentationLayout {
                windup_rate: 60,
                windup_color: 0,
                combined_color: 4,
                hidden_position: [12, 16],
                camera_eye: [20, 24],
                camera_target_y: 28,
                first_x: 32,
                spacing: 36,
                short_weapon_penalty: 40,
                minimum_distance: 44,
                zero: 48,
                title: (5, 0),
            },
        };
        let mut rel = Rel {
            bytes: (0..101).collect(),
            sections: vec![(1, 100); 6],
            pointers: Default::default(),
            local_targets: Default::default(),
        };
        rel.bytes.extend_from_slice(b"title\0");
        rel.sections[5] = (101, 6);
        for (index, value) in (-6..6).enumerate() {
            rel.bytes[13 + index * 4..17 + index * 4]
                .copy_from_slice(&(value as f32).to_be_bytes());
        }
        let tables = read(&rel, &layout)?;
        assert_eq!(tables.opener_contact_delays, [1, 2, 3, 4, 5, 6, 7, 8, 9]);
        assert_eq!(tables.placement[3], [3., 4., 5.]);
        assert_eq!(tables.combined_prelude.voices[0], 0x3d3e);
        assert_eq!(tables.overlimit_voices[8], 0x6162);
        assert_eq!(
            tables.trailing_storage,
            [vec![10, 11, 12], vec![79, 80], vec![99, 100]]
        );
        let restored = serde_json::from_slice::<UnisonTables>(&serde_json::to_vec(&tables)?)?;
        assert_eq!(
            serde_json::to_value(restored)?,
            serde_json::to_value(&tables)?
        );
        for (range, length) in [
            ([0, 8], 9),
            ([60, 77], 18),
            ([80, 97], 18),
            ([12, 0], 9),
            ([80, 101], 18),
        ] {
            assert!(table(&rel, range, length).is_err());
        }
        rel.sections[4].1 -= 1;
        assert!(read(&rel, &layout).is_err());
        rel.sections[4].1 += 1;
        rel.sections[5].1 -= 1;
        assert!(read(&rel, &layout).is_err());
        rel.sections[5].1 += 1;
        let saved = rel.bytes[61..65].to_vec();
        rel.bytes[61..65].copy_from_slice(&f32::INFINITY.to_be_bytes());
        assert!(read(&rel, &layout).is_err());
        rel.bytes[61..65].copy_from_slice(&saved);
        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            rel.bytes[13..17].copy_from_slice(&invalid.to_be_bytes());
            assert!(read(&rel, &layout).is_err());
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires both original extracted discs; only publishes small JSON tables"]
    fn original_unison_tables_reconstruct_and_deduplicate_every_module() -> Result<()> {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join(FAMILY));
        let mut publications = BTreeMap::new();
        let result = (|| -> Result<()> {
            for disc in [1, 2] {
                let destination = output.join(format!("disc{disc}"));
                let mut count = 0;
                for file in fs::read_dir(extracted.join(format!("disc{disc}/files")))? {
                    let file = file?.path();
                    let Some((module, layout)) = Layout::identify(&file) else {
                        continue;
                    };
                    let rel = Rel::read(&file)?;
                    let tables = read(&rel, &layout)?;
                    let unison = layout.unison;
                    let voices = |values: [u16; 9]| {
                        values
                            .into_iter()
                            .flat_map(u16::to_be_bytes)
                            .collect::<Vec<_>>()
                    };
                    for (index, (range, mut bytes)) in [
                        (
                            unison.opener_contact_delays,
                            tables.opener_contact_delays.to_vec(),
                        ),
                        (
                            unison.combined_voices,
                            voices(tables.combined_prelude.voices),
                        ),
                        (unison.overlimit_voices, voices(tables.overlimit_voices)),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        for offset in range {
                            assert!(rel.local_targets().contains(&(4, offset)), "{module}");
                        }
                        bytes.extend_from_slice(&tables.trailing_storage[index]);
                        assert_eq!(bytes, rel.at((4, range[0]))?[..range[1] - range[0]]);
                    }
                    let bytes: Vec<_> = tables
                        .placement
                        .into_iter()
                        .flatten()
                        .flat_map(f32::to_be_bytes)
                        .collect();
                    assert_eq!(bytes, rel.at((4, unison.placement))?[..48]);
                    for offset in [unison.placement, unison.placement + 48] {
                        assert!(rel.local_targets().contains(&(4, offset)), "{module}");
                    }
                    assert_eq!(tables.opener_contact_delays, [0, 0, 15, 15, 0, 0, 0, 0, 0]);
                    assert_eq!(
                        tables.placement,
                        [[0., 0., -1.], [-1., 0., 0.], [1., 0., 0.], [0., 0., 1.]]
                    );
                    assert_eq!(
                        tables.combined_prelude.voices,
                        [34710, 34708, 34738, 34756, 33337, 33459, 34736, 0, 34712]
                    );
                    assert_eq!(
                        tables.overlimit_voices,
                        [34715, 233, 350, 454, 573, 687, 785, 902, 1017]
                    );
                    let presentation = unison.presentation;
                    let prelude = &tables.combined_prelude;
                    let scalars = [
                        (presentation.windup_rate, tables.windup_rate),
                        (presentation.hidden_position[0], prelude.hidden_position[0]),
                        (presentation.hidden_position[1], prelude.hidden_position[1]),
                        (presentation.camera_eye[0], prelude.camera_eye[1]),
                        (presentation.camera_eye[1], prelude.camera_eye[2]),
                        (presentation.camera_target_y, prelude.camera_target[1]),
                        (presentation.first_x, prelude.first_x),
                        (presentation.spacing, prelude.spacing),
                        (
                            presentation.short_weapon_penalty,
                            tables.short_weapon_penalty,
                        ),
                        (presentation.minimum_distance, tables.minimum_distance),
                        (presentation.zero, prelude.hidden_position[2]),
                    ];
                    assert_eq!(
                        scalars.map(|(_, value)| value),
                        [0.5, 3000., 0.1, 200., 1500., 75., -75., 150., 40., 25., 0.]
                    );
                    for (offset, value) in scalars {
                        assert!(rel.local_targets().contains(&(4, offset)), "{module}");
                        assert_eq!(value.to_be_bytes(), rel.at((4, offset))?[..4]);
                    }
                    for (offset, value, expected) in [
                        (
                            presentation.windup_color,
                            tables.windup_color,
                            [16, 16, 16, 255],
                        ),
                        (
                            presentation.combined_color,
                            prelude.color,
                            [64, 64, 64, 255],
                        ),
                    ] {
                        assert!(rel.local_targets().contains(&(4, offset)), "{module}");
                        assert_eq!(value, rel.at((4, offset))?[..4]);
                        assert_eq!(value, expected);
                    }
                    assert!(
                        rel.local_targets().contains(&presentation.title),
                        "{module}"
                    );
                    assert_eq!(
                        prelude.title,
                        if module.starts_with("US_") {
                            "-Compound Special Attack-"
                        } else {
                            "－複合特技－"
                        }
                    );
                    let (text, _, invalid) = encoding_rs::SHIFT_JIS.encode(&prelude.title);
                    assert!(!invalid);
                    let source_text = rel.at(presentation.title)?;
                    assert_eq!(text.as_ref(), &source_text[..text.len()]);
                    assert_eq!(source_text[text.len()], 0);
                    assert!(
                        tables
                            .trailing_storage
                            .iter()
                            .flatten()
                            .all(|&byte| byte == 0)
                    );
                    let paths = cook_all(&file, &destination)?
                        .context("missing Unison table publication")?;
                    let restored =
                        crate::embedded::read::<UnisonTables>(&destination, FAMILY, module)?;
                    assert_eq!(
                        serde_json::to_value(restored)?,
                        serde_json::to_value(&tables)?
                    );
                    let source: serde_json::Value =
                        serde_json::from_slice(&fs::read(destination.join(&paths[1]))?)?;
                    assert_eq!(source["module"], module);
                    assert_eq!(source["source_sha256"], crate::digest(&rel.bytes));
                    assert_eq!(source["data"], paths[0]);
                    assert_eq!(source["layout"], serde_json::to_value(unison)?);
                    *publications.entry(paths[0].clone()).or_insert(0) += 1;
                    count += 1;
                }
                assert_eq!(count, 7);
            }
            let mut copies: Vec<_> = publications.into_values().collect();
            copies.sort_unstable();
            assert_eq!(copies, [2, 6, 6]);
            Ok(())
        })();
        if output.exists() {
            fs::remove_dir_all(output)?;
        }
        result
    }
}
