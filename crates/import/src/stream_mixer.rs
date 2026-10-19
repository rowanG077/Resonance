//! Stream settings map through attenuation and pan curves before the stereo mixer.
use crate::dol;
use anyhow::{Result, ensure};

const SETTING_ADDRESS: u32 = 0x801f_9bc4;
const VOLUME_ADDRESS: u32 = 0x802b_4680;
const PAN_ADDRESS: u32 = 0x802b_4a68;
const SETTING_COUNT: usize = 128;
const VOLUME_COUNT: usize = 1000;
const PAN_COUNT: usize = 31;

#[derive(Debug, PartialEq)]
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
}
