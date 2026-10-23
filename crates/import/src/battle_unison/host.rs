//! Shared team-attack placement and presentation from initialized asset data.
use crate::{read::Field, rel::Rel};
use anyhow::Result;
use resonance_content::battle_unison::{Host, Prelude};

pub(super) fn read(module: &Rel) -> Result<Host> {
    let data = module.at((4, 0))?;
    let zero = f32::read(data, 0xf14)?;
    let host = Host {
        source_sha256: crate::digest(&module.bytes),
        placement: <[[f32; 3]; 4]>::read(data, 0xec8)?,
        minimum_distance: f32::read(data, 0xf74)?,
        short_weapon_penalty: f32::read(data, 0xf70)?,
        restore_height: f32::read(data, 0xf2c)?,
        windup_rate: f32::read(data, 0xf44)?,
        windup_color: <[u8; 3]>::read(data, 0xe60)?,
        prelude: Prelude {
            first_x: f32::read(data, 0xf3c)?,
            spacing: f32::read(data, 0xf40)?,
            camera_eye: [zero, f32::read(data, 0xf30)?, f32::read(data, 0xf34)?],
            camera_target: [zero, f32::read(data, 0xf38)?, zero],
            color: <[u8; 3]>::read(data, 0xef8)?,
            voices: <[u16; 9]>::read(data, 0xefc)?.map(crate::battle_voice::sound),
        },
    };
    host.validate()?;
    Ok(host)
}
