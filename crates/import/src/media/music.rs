use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};

const SONG_COUNT: u16 = 112;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "change", rename_all = "snake_case")]
pub(crate) enum ReverbChange {
    /// Continue using the shared studio's current parameters.
    Keep,
    Set {
        parameters: [[f32; 5]; 2],
    },
}

/// Two reverb buses; the title selects preset 1. Parameter order is
/// coloration, mix, time, damping, and pre-delay.
pub(super) fn title_reverbs(executable: &[u8]) -> Result<[[f32; 5]; 2]> {
    song_reverbs(executable, 1)
}

pub(crate) fn song_reverbs(executable: &[u8], song: u16) -> Result<[[f32; 5]; 2]> {
    match song_reverb_change(executable, song)? {
        ReverbChange::Set { parameters } => Ok(parameters),
        ReverbChange::Keep => bail!("song {song} retains the current reverb parameters"),
    }
}

pub(crate) fn song_reverb_change(executable: &[u8], song: u16) -> Result<ReverbChange> {
    ensure!(song < SONG_COUNT, "song {song} is outside the reverb table");
    let preset = crate::dol::slice(executable, 0x8021_08b0 + u32::from(song), 1)?[0];
    ensure!(preset <= 2, "unsupported song reverb preset {preset}");
    if preset == 0 {
        return Ok(ReverbChange::Keep);
    }
    let addresses = [
        if preset == 1 {
            [
                0x8035_c47c,
                0x8035_c480,
                0x8035_c470,
                0x8035_c478,
                0x8035_c474,
            ]
        } else {
            [
                0x8035_c480,
                0x8035_c480,
                0x8035_c484,
                0x8035_c478,
                0x8035_c488,
            ]
        },
        [
            0x8035_c490,
            0x8035_c4ac,
            0x8035_c490,
            0x8035_c47c,
            0x8035_c4a8,
        ],
    ];
    let mut result = [[0.; 5]; 2];
    for (bus, addresses) in result.iter_mut().zip(addresses) {
        for (parameter, address) in bus.iter_mut().zip(addresses) {
            *parameter = f32::from_be_bytes(crate::dol::slice(executable, address, 4)?.try_into()?);
        }
        for (value, (min, max)) in
            bus.iter()
                .zip([(0., 1.), (0., 1.), (0.01, 10.), (0., 1.), (0., 0.1)])
        {
            ensure!(
                value.is_finite() && (min..=max).contains(value),
                "invalid original reverb parameter"
            );
        }
    }
    Ok(ReverbChange::Set { parameters: result })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeSet, fs, path::Path};

    #[test]
    #[ignore = "requires both extracted discs; checks original tables without audio playback"]
    fn original_song_reverb_changes_preserve_retained_and_selected_presets() -> Result<()> {
        let auxiliary = [1., 0.5, 1., 0.8, 0.01];
        let field = [[0.8, 0.7, 3.6, 0.6, 0.08], auxiliary];
        let battle = [[0.7, 0.7, 2.5, 0.6, 0.05], auxiliary];
        for disc in [1, 2] {
            let root = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../../local/extracted/disc{disc}"));
            let executable = fs::read(root.join("sys/main.dol"))?;
            let table =
                crate::dol::slice(&executable, 0x801f_982c, (usize::from(SONG_COUNT) + 1) * 8)?;
            let ids: BTreeSet<_> = table
                .chunks_exact(8)
                .take_while(|row| row[..2] != [255, 255])
                .map(|row| u16::from_be_bytes([row[0], row[1]]))
                .collect();
            assert_eq!(ids, (0..SONG_COUNT).collect());
            assert_eq!(&table[usize::from(SONG_COUNT) * 8..][..2], &[255, 255]);
            let mut retained = Vec::new();
            let mut battle_ids = Vec::new();
            for id in ids {
                let path = super::super::music_path(&executable, id)?;
                assert!(root.join("files").join(path).is_file());
                match song_reverb_change(&executable, id)? {
                    ReverbChange::Keep => {
                        retained.push(id);
                        assert!(song_reverbs(&executable, id).is_err());
                    }
                    ReverbChange::Set { parameters } if parameters == battle => {
                        battle_ids.push(id);
                    }
                    ReverbChange::Set { parameters } => assert_eq!(parameters, field),
                }
            }
            assert_eq!(retained, [0, 111]);
            assert_eq!(battle_ids, (85..=96).chain([104, 105]).collect::<Vec<_>>());
            assert_eq!(title_reverbs(&executable)?, field);
            for id in [SONG_COUNT, u16::MAX] {
                assert!(song_reverb_change(&executable, id).is_err());
            }
        }
        Ok(())
    }
}
