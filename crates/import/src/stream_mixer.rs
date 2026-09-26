//! Stream settings map through attenuation and pan curves before the stereo mixer.
use crate::{dol, embedded};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

const SETTING_ADDRESS: u32 = 0x801f_9bc4;
const VOLUME_ADDRESS: u32 = 0x802b_4680;
const PAN_ADDRESS: u32 = 0x802b_4a68;
const FAMILY: &str = "stream-mixer";
const SETTING_COUNT: usize = 128;
const VOLUME_COUNT: usize = 1000;
const PAN_COUNT: usize = 31;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Tables {
    /// Settings 0..127 select attenuation; multiply by ten for the volume curve.
    pub setting_attenuation: Vec<u8>,
    pub setting_storage: [u8; 4],
    /// The native stream setter clamps attenuation to 0..999 before lookup.
    pub volume_by_attenuation: Vec<u8>,
    /// Pan -15..15 selects a mixer control in 0..127.
    pub pan_controls: Vec<u32>,
    pub pan_storage: u32,
}

impl Tables {
    pub fn read(executable: &[u8]) -> Result<Self> {
        let settings = dol::slice(executable, SETTING_ADDRESS, SETTING_COUNT + 4)?;
        let pan = dol::slice(executable, PAN_ADDRESS, (PAN_COUNT + 1) * 4)?;
        let tables = Self {
            setting_attenuation: settings[..SETTING_COUNT].to_vec(),
            setting_storage: settings[SETTING_COUNT..].try_into()?,
            volume_by_attenuation: dol::slice(executable, VOLUME_ADDRESS, VOLUME_COUNT)?.to_vec(),
            pan_controls: pan[..PAN_COUNT * 4]
                .chunks_exact(4)
                .map(|word| u32::from_be_bytes(word.try_into().unwrap()))
                .collect(),
            pan_storage: u32::from_be_bytes(pan[PAN_COUNT * 4..].try_into()?),
        };
        tables.validate()?;
        Ok(tables)
    }

    fn validate(&self) -> Result<()> {
        ensure!(
            self.setting_attenuation.len() == SETTING_COUNT
                && self.volume_by_attenuation.len() == VOLUME_COUNT
                && self.pan_controls.len() == PAN_COUNT,
            "invalid stream mixer table lengths"
        );
        ensure!(
            self.volume_by_attenuation.iter().all(|&value| value <= 127)
                && self.pan_controls.iter().all(|&value| value <= 127),
            "stream mixer lookup exceeds control range"
        );
        Ok(())
    }

    pub fn gains(&self, mix: &resonance_audio::mix::Tables) -> Result<Vec<f32>> {
        self.validate()?;
        mix.validate()?;
        Ok(self
            .setting_attenuation
            .iter()
            .map(|&value| {
                let index = (usize::from(value) * 10).min(VOLUME_COUNT - 1);
                mix.volume[usize::from(self.volume_by_attenuation[index])]
            })
            .collect())
    }

    pub fn pan(&self, mix: &resonance_audio::mix::Tables) -> Result<Vec<[f32; 2]>> {
        self.validate()?;
        mix.validate()?;
        Ok(self
            .pan_controls
            .iter()
            .map(|&pan| {
                mix.gains(127 << 16, 16383, pan as u8, [0; 2])[0]
                    .map(|gain| f32::from(gain) / 32768.)
            })
            .collect())
    }

    pub fn cook(extracted: &Path, output: &Path) -> Result<Vec<String>> {
        let file = extracted.join("sys/main.dol");
        embedded::write(&file, output, FAMILY, &Self::read(&fs::read(&file)?)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_mixer_retains_storage_and_rejects_invalid_lookup_bounds() -> Result<()> {
        let spans = [
            (SETTING_ADDRESS, 132),
            (VOLUME_ADDRESS, 1000),
            (PAN_ADDRESS, 128),
        ];
        let mut executable = vec![0; 0x100 + 1260];
        let mut offset = 0x100;
        for (index, (address, bytes)) in spans.into_iter().enumerate() {
            for (at, value) in [
                (index * 4, offset as u32),
                (0x48 + index * 4, address),
                (0x90 + index * 4, bytes),
            ] {
                executable[at..at + 4].copy_from_slice(&value.to_be_bytes());
            }
            offset += bytes as usize;
        }
        executable[0x100] = 99;
        executable[0x180..0x184].copy_from_slice(&[1, 2, 3, 4]);
        executable[offset - 4..].copy_from_slice(&0xdead_beef_u32.to_be_bytes());
        let tables = Tables::read(&executable)?;
        assert_eq!(tables.setting_storage, [1, 2, 3, 4]);
        assert_eq!(tables.pan_storage, 0xdead_beef);
        assert!(Tables::read(&executable[..offset - 1]).is_err());
        for (index, (_, bytes)) in spans.into_iter().enumerate() {
            let mut bad = executable.clone();
            let at = 0x90 + index * 4;
            bad[at..at + 4].copy_from_slice(&(bytes - 1).to_be_bytes());
            assert!(Tables::read(&bad).is_err());
        }
        for (at, value) in [(0x184, 128), (0x184 + 1000 + 3, 128)] {
            let mut bad = executable.clone();
            bad[at] = value;
            assert!(Tables::read(&bad).is_err());
        }
        let mix = resonance_audio::mix::Tables {
            volume: std::array::from_fn(|i| i as f32 / 128.),
            alternate_volume: [0.; 129],
            pan: [0.; 4],
            volume_16_scale: 1.,
            controller_14_scale: 1.,
            pan_16_scale: 1.,
            spatial: None,
        };
        executable[0x184 + 990] = 1;
        executable[0x184 + 999] = 2;
        for value in [99, 100, 255] {
            executable[0x100] = value;
            let tables = Tables::read(&executable)?;
            assert_eq!(tables.setting_attenuation[0], value);
            assert_eq!(
                tables.gains(&mix)?[0],
                mix.volume[if value == 99 { 1 } else { 2 }]
            );
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires both original extracted discs; no synthesis or playback"]
    fn original_stream_mixer_tables_reconstruct_every_physical_value() -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join("stream-mixer"));
        let result = (|| -> Result<()> {
            for disc in 1..=2 {
                let extracted = root.join(format!("disc{disc}"));
                let executable = fs::read(extracted.join("sys/main.dol"))?;
                let tables = Tables::read(&executable)?;
                let mut settings = tables.setting_attenuation.clone();
                settings.extend(tables.setting_storage);
                let pan: Vec<_> = tables
                    .pan_controls
                    .iter()
                    .copied()
                    .chain([tables.pan_storage])
                    .flat_map(u32::to_be_bytes)
                    .collect();
                for (address, values) in [
                    (SETTING_ADDRESS, &settings),
                    (VOLUME_ADDRESS, &tables.volume_by_attenuation),
                    (PAN_ADDRESS, &pan),
                ] {
                    ensure!(
                        values == dol::slice(&executable, address, values.len())?,
                        "disc{disc}: stream table {address:#x} changed"
                    );
                }
                let paths = Tables::cook(&extracted, &output)?;
                ensure!(
                    embedded::read::<Tables>(&output, FAMILY, "main.dol")? == tables,
                    "stream mixer publication changed"
                );
                let source: serde_json::Value =
                    serde_json::from_slice(&fs::read(output.join(&paths[1]))?)?;
                ensure!(
                    source["source_sha256"] == crate::digest(&executable),
                    "stream mixer source digest changed"
                );
                eprintln!("disc{disc}: 1159 stream lookups and eight storage bytes reconstructed");
            }
            Ok(())
        })();
        if output.exists() {
            fs::remove_dir_all(output)?;
        }
        result
    }
}
