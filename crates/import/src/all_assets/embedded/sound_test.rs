//! Sound Test's authored selection order, song titles and display resources.
use crate::{dol, read::u32 as word};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

const MUSIC: u32 = 0x8021ea90;
const MUSIC_COUNT: usize = 103;
const CUES: u32 = 0x8021eaf8;
const CUE_COUNT: usize = 340;
const SELECTABLE_CUES: usize = 325;
const TEXT: u32 = 0x8019a9a0;
const TITLES: u32 = TEXT + 0xd58;
const TITLE_COUNT: usize = 111;
const COLORS: u32 = TEXT + 0xf14;
const SPECTRUM: u32 = 0x8021cd8c;
const SAMPLE_ORDER: u32 = 0x8019ae60;
const CONSTANTS: u32 = 0x8035cba0;
const FAMILY: &str = "sound-test";

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Catalogue {
    /// Displayed positions are one-based; values address the music directory.
    music_order: Vec<u8>,
    music_storage: u8,
    /// Retain all 340 authored cells, including the 15 after the selectable range.
    sound_cues: Vec<u16>,
    selectable_sound_cues: usize,
    voice: VoiceSelection,
    /// Indexed by music ID, independently of the displayed selection order.
    song_titles: Vec<Option<String>>,
    options: Vec<Option<String>>,
    playback_modes: Vec<Option<String>>,
    spectrum_colors: Vec<[u8; 4]>,
    spectrum: Spectrum,
    labels: BTreeMap<Label, Option<String>>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Spectrum {
    sample_order: Vec<u8>,
    /// Two consecutive calls perform 512 butterfly steps each.
    transform_steps: Vec<Butterfly>,
    steps_per_pass: usize,
    /// Absolute PCM sample amplitude is quantized against these signed thresholds.
    level_thresholds: Vec<i16>,
    level_threshold_storage: [i16; 2],
    sample_weights: Vec<f64>,
    /// [sine, cosine] coefficient pairs; the source's views overlap by 64 entries.
    twiddles: Vec<[f64; 2]>,
    energy_thresholds: Vec<i32>,
    magnitude_scale: f64,
    saturation_energy: f64,
    /// Adjacent authored scalar with no established use in the two spectrum callbacks.
    constant_storage: f64,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Butterfly {
    indices: [u8; 2],
    twiddle: u8,
}

fn spectrum(executable: &[u8]) -> Result<Spectrum> {
    let doubles = |address, count: usize| -> Result<Vec<f64>> {
        dol::slice(executable, address, count * 8)?
            .chunks_exact(8)
            .map(|row| {
                let value = f64::from_be_bytes(row.try_into()?);
                ensure!(value.is_finite(), "non-finite spectrum coefficient");
                Ok(value)
            })
            .collect()
    };
    let steps = dol::slice(executable, SPECTRUM, 3 * 1024)?;
    let levels = dol::slice(executable, 0x8021d98c, 130 * 2)?;
    let twiddles = doubles(0x8021e290, 192)?;
    let constants = doubles(CONSTANTS, 3)?;
    Ok(Spectrum {
        sample_order: dol::slice(executable, SAMPLE_ORDER, 256)?.to_vec(),
        transform_steps: (0..1024)
            .map(|index| {
                let twiddle = steps[2 * 1024 + index];
                ensure!(
                    twiddle < 128,
                    "spectrum twiddle index exceeds coefficient table"
                );
                Ok(Butterfly {
                    indices: [steps[index], steps[1024 + index]],
                    twiddle,
                })
            })
            .collect::<Result<_>>()?,
        steps_per_pass: 512,
        level_thresholds: levels[..256]
            .chunks_exact(2)
            .map(|row| i16::from_be_bytes([row[0], row[1]]))
            .collect(),
        level_threshold_storage: [
            i16::from_be_bytes(levels[256..258].try_into()?),
            i16::from_be_bytes(levels[258..260].try_into()?),
        ],
        sample_weights: doubles(0x8021da90, 256)?,
        twiddles: (0..128)
            .map(|index| [twiddles[index], twiddles[index + 64]])
            .collect(),
        energy_thresholds: dol::slice(executable, 0x8021e890, 128 * 4)?
            .chunks_exact(4)
            .map(|row| i32::from_be_bytes(row.try_into().unwrap()))
            .collect(),
        magnitude_scale: constants[1],
        saturation_energy: constants[2],
        constant_storage: constants[0],
    })
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct VoiceSelection {
    resource_key: u32,
    first: u16,
    last: u16,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Label {
    Heading,
    NumberPrefix,
    NumberFormat,
    VolumeFormat,
    Playing,
    Stopped,
    Paused,
    EmptyStatus,
    ElapsedStatus,
    Stereo,
    Mono,
}

fn read(executable: &[u8]) -> Result<Catalogue> {
    let strings = |address, count| -> Result<Vec<Option<String>>> {
        dol::slice(executable, address, count * 4)?
            .chunks_exact(4)
            .map(|row| dol::optional_text(executable, word(row, 0)?))
            .collect()
    };
    let music = dol::slice(executable, MUSIC, MUSIC_COUNT + 1)?;
    Ok(Catalogue {
        music_order: music[..MUSIC_COUNT].to_vec(),
        music_storage: music[MUSIC_COUNT],
        sound_cues: dol::slice(executable, CUES, CUE_COUNT * 2)?
            .chunks_exact(2)
            .map(|row| u16::from_be_bytes([row[0], row[1]]))
            .collect(),
        selectable_sound_cues: SELECTABLE_CUES,
        // Selection wraps through 1..0x781 before the loader combines the key and ID.
        voice: VoiceSelection {
            resource_key: 0x1e0000,
            first: 1,
            last: 0x781,
        },
        song_titles: strings(TITLES, TITLE_COUNT)?,
        options: strings(TEXT + 0x5f4, 11)?,
        playback_modes: strings(TEXT + 0x5cc, 3)?,
        spectrum_colors: dol::slice(executable, COLORS, 16 * 4)?
            .chunks_exact(4)
            .map(|row| row.try_into().unwrap())
            .collect(),
        spectrum: spectrum(executable)?,
        labels: [
            (
                Label::Heading,
                word(dol::slice(executable, TEXT + 0x74, 4)?, 0)?,
            ),
            (Label::NumberPrefix, 0x8035cb7c),
            (Label::NumberFormat, 0x8035cb80),
            (Label::VolumeFormat, 0x8035cb84),
            (Label::Playing, 0x8035cb88),
            (Label::Stopped, 0x8035cb90),
            (Label::Paused, 0x8035cb98),
            (Label::EmptyStatus, TEXT + 0xf54),
            (Label::ElapsedStatus, TEXT + 0xf60),
            (Label::Stereo, 0x8035cad4),
            (Label::Mono, 0x8035cadc),
        ]
        .into_iter()
        .map(|(label, address)| Ok((label, dol::optional_text(executable, address)?)))
        .collect::<Result<_>>()?,
    })
}

pub(super) fn cook(file: &Path, executable: &[u8], output: &Path) -> Result<Vec<String>> {
    crate::embedded::write(
        file,
        output,
        FAMILY,
        &read(executable)?,
        serde_json::json!({
            "music":{"address":MUSIC,"count":MUSIC_COUNT,"storage_bytes":1},
            "sound_cues":{"address":CUES,"count":CUE_COUNT,"stride":2,"selectable":SELECTABLE_CUES},
            "song_titles":{"address":TITLES,"count":TITLE_COUNT,"stride":4},
            "spectrum_colors":{"address":COLORS,"count":16,"stride":4},
            "options":{"address":TEXT+0x5f4,"count":11,"stride":4},
            "playback_modes":{"address":TEXT+0x5cc,"count":3,"stride":4},
            "sample_order":{"address":SAMPLE_ORDER,"count":256,"stride":1},
            "spectrum":{"address":SPECTRUM,"bytes":MUSIC-SPECTRUM},
            "spectrum_constants":{"address":CONSTANTS,"count":3,"stride":8},
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn spectrum_bytes(spectrum: &Spectrum) -> Vec<u8> {
        let mut bytes = Vec::new();
        for column in 0..3 {
            bytes.extend(spectrum.transform_steps.iter().map(|step| {
                if column == 2 {
                    step.twiddle
                } else {
                    step.indices[column]
                }
            }));
        }
        bytes.extend(
            spectrum
                .level_thresholds
                .iter()
                .copied()
                .chain(spectrum.level_threshold_storage)
                .flat_map(i16::to_be_bytes),
        );
        bytes.extend(
            spectrum
                .sample_weights
                .iter()
                .flat_map(|value| value.to_be_bytes()),
        );
        for index in 0..64 {
            assert_eq!(
                spectrum.twiddles[index + 64][0].to_bits(),
                spectrum.twiddles[index][1].to_bits()
            );
        }
        bytes.extend(
            spectrum
                .twiddles
                .iter()
                .map(|pair| pair[0])
                .chain(spectrum.twiddles[64..].iter().map(|pair| pair[1]))
                .flat_map(f64::to_be_bytes),
        );
        bytes.extend(
            spectrum
                .energy_thresholds
                .iter()
                .flat_map(|value| value.to_be_bytes()),
        );
        bytes
    }

    #[test]
    #[ignore = "requires both extracted discs; no media conversion or playback"]
    fn original_sound_test_catalogues_preserve_all_authored_selections() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let mut first = None;
        for disc in [1, 2] {
            let mut executable = fs::read(local.join(format!("disc{disc}/sys/main.dol")))?;
            let catalogue = read(&executable)?;
            let encoded = serde_json::to_vec(&catalogue)?;
            let restored: Catalogue = serde_json::from_slice(&encoded)?;
            assert_eq!(restored, catalogue);
            assert_eq!(
                spectrum_bytes(&restored.spectrum),
                dol::slice(&executable, SPECTRUM, (MUSIC - SPECTRUM) as usize)?
            );
            assert_eq!(
                restored.spectrum.sample_order,
                dol::slice(&executable, SAMPLE_ORDER, 256)?
            );
            let constants: Vec<_> = [
                restored.spectrum.constant_storage,
                restored.spectrum.magnitude_scale,
                restored.spectrum.saturation_energy,
            ]
            .into_iter()
            .flat_map(f64::to_be_bytes)
            .collect();
            assert_eq!(constants, dol::slice(&executable, CONSTANTS, 24)?);
            if let Some(expected) = &first {
                assert_eq!(&encoded, expected);
            } else {
                first = Some(encoded);
            }
            let mut music = catalogue.music_order.clone();
            music.push(catalogue.music_storage);
            assert_eq!(music, dol::slice(&executable, MUSIC, MUSIC_COUNT + 1)?);
            let cues: Vec<_> = catalogue
                .sound_cues
                .iter()
                .flat_map(|id| id.to_be_bytes())
                .collect();
            assert_eq!(cues, dol::slice(&executable, CUES, CUE_COUNT * 2)?);
            let colors: Vec<_> = catalogue
                .spectrum_colors
                .iter()
                .flatten()
                .copied()
                .collect();
            assert_eq!(colors, dol::slice(&executable, COLORS, 16 * 4)?);
            assert_eq!(catalogue.song_titles.len(), TITLE_COUNT);
            assert_eq!(catalogue.options.len(), 11);
            assert_eq!(catalogue.playback_modes.len(), 3);
            let directory = crate::music_directory::Directory::read(&executable)?;
            for &id in &catalogue.music_order {
                directory.path(u16::from(id))?;
                assert!(catalogue.song_titles[usize::from(id)].is_some());
            }
            assert_eq!(catalogue.selectable_sound_cues, SELECTABLE_CUES);
            assert_eq!(catalogue.sound_cues.len() - SELECTABLE_CUES, 15);

            // Nonzero retained cells and null strings must survive, not become errors.
            for (address, replacement) in [
                (MUSIC + MUSIC_COUNT as u32, &[0xa5][..]),
                (CUES + (CUE_COUNT as u32 - 1) * 2, &[0x12, 0x34][..]),
                (TITLES, &[0, 0, 0, 0][..]),
                (0x8021da8c, &[0xff, 0xfe, 0x12, 0x34][..]),
                (CONSTANTS, &[0x40, 0x04, 0, 0, 0, 0, 0, 0][..]),
            ] {
                let original = dol::slice(&executable, address, replacement.len())?;
                let offset = original.as_ptr() as usize - executable.as_ptr() as usize;
                executable[offset..offset + replacement.len()].copy_from_slice(replacement);
            }
            let changed = read(&executable)?;
            assert_eq!(changed.music_storage, 0xa5);
            assert_eq!(changed.sound_cues.last(), Some(&0x1234));
            assert_eq!(changed.song_titles[0], None);
            assert_eq!(changed.spectrum.level_threshold_storage, [-2, 0x1234]);
            assert_eq!(changed.spectrum.constant_storage, 2.5);
        }
        Ok(())
    }
}
