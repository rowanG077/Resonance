//! Native Defense System controller values in the existing named definition.
use crate::{
    audio_effects, output, pe,
    source::Source,
    text::{Bank, Menu},
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle_defense_system::{self as defense, Definition, FixedMember},
    library::Mappings,
};
use std::{fs, path::Path};

pub(crate) fn cook(source: &Source, library: &Path) -> Result<()> {
    let texts = source.require("TOS_TEXT.TXB")?;
    let menu = Menu {
        image: pe::Image::parse(&source.executable)?,
        bank: Bank::parse(&texts)?,
        slot: source.language.slot(),
    };
    let image = &menu.image;
    let word = |address| pe::word(image.slice(address, 4)?, 0);
    let byte = |address| -> Result<u8> { Ok(image.slice(address, 1)?[0]) };
    let scalar = |address| -> Result<f32> { Ok(f32::from_bits(word(address)?)) };
    let names: Mappings = serde_json::from_slice(&fs::read(
        library.join(resonance_content::battle_enemy::document::MAPPINGS),
    )?)?;
    let name = |id| -> Result<String> { Ok(names.target(id)?.to_owned()) };
    ensure!(scalar(0x882ea4)? == 180., "changed Defense System facing");
    let fixed = |id, address| -> Result<_> {
        Ok(FixedMember {
            enemy: name(id)?,
            position: [scalar(address)?, 0., 0.],
            heading: -90.,
        })
    };
    let messages = menu.directory(0x8ed0d0)?;
    let definition = Definition {
        core: fixed(word(0x401ca7)?, 0x8e5064)?,
        repair: fixed(word(0x401d1d)?, 0x8830a0)?,
        orbit: name(212)?,
        orbit_count: byte(0x401ee7)?,
        arm: name(word(0x401be4)?)?,
        arm_joints: ["PA00_Arm_R", "PA01_Arm_R", "PA02_Arm_L", "PA03_Arm_L"].map(str::to_owned),
        deployment: defense::Deployment {
            clip: u16::from(byte(0x401f58)?),
            ticks: word(0x401fdb)?.try_into()?,
            hp_percent: byte(0x401f34)?,
            rate: scalar(0x883e58)?,
            blend: byte(0x401f56)?,
        },
        repair_cycle: defense::RepairCycle {
            self_ticks: word(0x401d81)?.try_into()?,
            repeat_ticks: u16::from(byte(0x401dfd)?),
            orbit_ticks: word(0x401e4c)?.try_into()?,
            self_percent: byte(0x401dcd)?,
            orbit_percent: byte(0x401e9c)?,
            defense_divisor: 4,
            defense_multiplier: 3,
        },
        repair_sound: audio_effects::id(word(0x401d90)?.try_into()?)?,
        repair_sound_priority: byte(0x401d8e)?,
        messages: defense::Messages {
            automatic: menu.caption(messages, 0)?,
            orbit: menu.caption(messages, 1)?,
            deployment: menu.caption(messages, 2)?,
            duration: u16::from(byte(0x401dab)?),
        },
        camera_yaw_limits: [102., 256.],
    };
    definition
        .clone()
        .prepare(&names)
        .context("Defense System definition")?;
    output::json(&library.join(defense::PATH), &definition)
}
