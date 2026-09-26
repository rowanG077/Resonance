//! Shared actor layout; party templates are copied by original 1CAA8.
use crate::{
    read::{Field, FloatOperand, unreferenced_storage},
    rel::Rel,
};
use anyhow::{Context, Result, ensure};
use resonance_content::battle_profile::{Casting, Placement, Profile, Table, VoiceSequence};
use std::path::Path;

pub(crate) const BYTES: usize = 496;

pub(crate) fn read(bytes: &[u8]) -> Result<Profile> {
    let b = bytes
        .get(..BYTES)
        .context("truncated battle actor profile")?;
    let channels = usize::from(b[0xce]);
    ensure!(channels <= 4, "too many actor texture channels");
    let weapons = usize::from(b[0x1e4]);
    ensure!(weapons <= 8, "too many actor weapon slots");
    let mut referenced = vec![0x1e4..0x1e5];
    referenced.extend((0..weapons).map(|slot| 0x124 + slot * 24..0x125 + slot * 24));
    Ok(Profile {
        walk_speed: FloatOperand::read(b, 0x20)?,
        run_speed: FloatOperand::read(b, 0x24)?,
        turn_ticks: b[0x2c],
        idle_ticks: b[0x94],
        idle_variation: b[0x95],
        initial_motion: b[0x97],
        initial_motion_override: b[0xc1],
        texture_channels: (0..channels)
            .map(|index| resonance_content::battle_profile::TextureChannel {
                texture: b[0xd3 + index],
                frames: b[0xcf + index],
            })
            .collect(),
        idle_expression: std::array::from_fn(
            |index| if index < channels { b[0xd7 + index] } else { 0 },
        ),
        weapon_draw_flags: (0..weapons).map(|slot| b[0x124 + slot * 24]).collect(),
        condition_flags: <[u32; 2]>::read(b, 0x10)?,
        condition_immunity: <[u32; 2]>::read(b, 0x18)?,
        intrinsic_conditions: <[u32; 2]>::read(b, 0x38)?,
        weight: b[0x28],
        stun_resistance: b[0x2d],
        stagger_threshold: b[0x2e],
        guard_reduction: b[0x2f],
        stagger_ticks: b[0x55],
        guard_pressure_limit: i16::read(b, 0x58)?,
        flags: u32::read(b, 0x5c)?,
        body_flags: u16::read(b, 0xb4)?,
        armor: b[0x11b],
        center_offset: <[FloatOperand; 3]>::read(b, 0x60)?,
        target_bone: b[0x51],
        target_offset: <[FloatOperand; 3]>::read(b, 0x78)?,
        model_scale: FloatOperand::read(b, 0x84)?,
        shadow_scale: FloatOperand::read(b, 0x88)?,
        shadow_color: <[u8; 4]>::read(b, 0x108)?,
        effect_scale: FloatOperand::read(b, 0x8c)?,
        camera_yaw_offset: b[0xa7],
        camera_category: b[0xe7],
        camera_minimum_radius: i16::read(b, 0xf0)?,
        voice_base: u32::read(b, 0x104)?,
        death_voice: u16::read(b, 0xf4)?,
        death_motion: b[0x9c],
        overlimit_gain: b[0xad],
        ground_offset: FloatOperand::read(b, 0x90)?,
        head_bone: b[0x9f],
        stun_offset: <[FloatOperand; 3]>::read(b, 0x6c)?,
        casting: Casting {
            base_ticks: i16::read(b, 0x56)?,
            loop_start: b[0x9d],
            animation_rate: FloatOperand::read(b, 0xa0)?,
            command_index: i16::read(b, 0xec)?,
            effect_interval: b[0x96],
            motion_flags: b[0x30],
            resume_start: b[0xa4],
            resume_blend: b[0xa5],
            resume_loop_start: b[0xa8],
            stored_recovery_clip: b[0xa6],
        },
        storage: unreferenced_storage(
            b,
            [
                referenced,
                vec![
                    0x10..0x28,
                    0x28..0x29,
                    0x2c..0x31,
                    0x38..0x40,
                    0x51..0x52,
                    0x55..0x5a,
                    0x5c..0x84,
                    0x84..0x8c,
                    0x8c..0x90,
                    0x90..0x98,
                    0xc1..0xc2,
                    0xce..0xcf + channels,
                    0xd3..0xd3 + channels,
                    0xd7..0xd7 + channels,
                    0x9c..0x9e,
                    0x9f..0xa9,
                    0xad..0xae,
                    0xb4..0xb6,
                    0xec..0xee,
                    0xe7..0xe8,
                    0xf0..0xf2,
                    0xf4..0xf6,
                    0x104..0x10c,
                    0x11b..0x11c,
                ],
            ]
            .concat(),
        ),
    })
}

pub fn publish_party(file: &Path, output: &Path, prefix: &str) -> Result<String> {
    let module = Rel::read(file)?;
    let bytes = module
        .at((5, 0x3d30))?
        .get(..11 * BYTES)
        .context("truncated party profile table")?;
    let table = Table {
        source_sha256: crate::digest(&module.bytes),
        contact_sounds: resonance_content::battle_profile::ContactSounds {
            party: (0..9)
                .map(|index| u16::read(module.at((4, 0x1d30))?, index * 2))
                .collect::<Result<Vec<_>>>()?
                .try_into()
                .unwrap(),
            elements: module
                .at((5, 0x1404))?
                .get(..9)
                .context("truncated element contact sounds")?
                .try_into()
                .unwrap(),
        },
        default_strategy: {
            let columns = module
                .at((4, 0x10dc))?
                .get(..30)
                .context("truncated strategy defaults")?;
            std::array::from_fn(|character| {
                std::array::from_fn(|kind| columns[kind * 10 + character])
            })
        },
        companion_policy: resonance_content::battle_profile::CompanionPolicy {
            tp_limits: <[u8; 9]>::read(module.at((4, 0x2860))?, 0)?,
            healing_limits: <[u8; 9]>::read(module.at((4, 0x286c))?, 0)?,
            support_level_limits: <[u8; 9]>::read(module.at((4, 0x2878))?, 0)?.map(|v| v as i8),
        },
        placement: placement(&module)?,
        entry: entry(&module)?,
        chant: module
            .at((5, 0x11e0))?
            .get(..48)
            .context("truncated casting motion rows")?
            .chunks_exact(12)
            .map(crate::battle_action::animation)
            .collect::<Result<_>>()?,
        voice_sequences: (0..10)
            .map(|character| voice_sequence(&module, character))
            .collect::<Result<_>>()?,
        death_voice_pairs: module
            .at((4, 0x177c))?
            .get(..28)
            .context("truncated ally death voice pairs")?
            .chunks_exact(2)
            .map(|pair| [pair[0], pair[1]])
            .collect(),
        records: bytes.chunks_exact(BYTES).map(read).collect::<Result<_>>()?,
    };
    let path = format!("{prefix}/party-profiles.json");
    crate::write_atomic(&output.join(&path), &serde_json::to_vec(&table)?)?;
    Ok(path)
}

fn entry(module: &Rel) -> Result<resonance_content::battle_profile::Entry> {
    let read = |offset| FloatOperand::read(module.at((4, offset))?, 0)?.finite();
    Ok(resonance_content::battle_profile::Entry {
        bootstrap_eye: [read(0x160)?, read(0x164)?, read(0x168)?],
        bootstrap_focus: [read(0x16c)?, read(0x170)?, read(0x174)?],
        initial_yaw: read(0x7e4)?,
        radius: read(0x6b4)?,
        focus_x: read(0x7dc)?,
        focus_speed_scale: read(0x7e0)?,
        replacement_motion_rate: read(0x2a04)?,
        fade_color: <[u8; 3]>::read(module.at((4, 0x148))?, 0)?,
        screen_break: screen_break(module)?,
    })
}

fn screen_break(module: &Rel) -> Result<resonance_content::battle_profile::ScreenBreak> {
    let read = |offset| FloatOperand::read(module.at((4, offset))?, 0)?.finite();
    let points = module.at((5, 0xc0))?;
    let triangles = module.at((5, 0x2c4))?;
    Ok(resonance_content::battle_profile::ScreenBreak {
        points: (0..43)
            .map(|index| {
                let point = <[FloatOperand; 3]>::read(points, index * 12)?;
                Ok([point[0].finite()?, point[1].finite()?, point[2].finite()?])
            })
            .collect::<Result<_>>()?,
        triangles: (0..62)
            .map(|index| {
                let row = <[u32; 3]>::read(triangles, index * 12)?;
                ensure!(
                    row.iter().all(|&point| point < 43),
                    "invalid screen-break triangle"
                );
                Ok(row.map(|point| point as u8))
            })
            .collect::<Result<_>>()?,
        viewport: [read(0x4f0)?, read(0x4f4)?],
        viewport_center: [read(0x4d8)?, read(0x550)?],
        center_weight: read(0x548)?,
        center_expansion: read(0x54c)?,
        velocity_scale: read(0x55c)?,
        angular_base: read(0x554)?,
        angular_variation: read(0x558)?,
        radians_per_degree: read(0x53c)?,
        secondary_rotation_scale: read(0x540)?,
        draw_depth: read(0x544)?,
    })
}

fn placement(module: &Rel) -> Result<Placement> {
    // 1B5A4 uses the Z component of each vector and these scalar operands.
    let read = |offset| FloatOperand::read(module.at((4, offset))?, 0)?.finite();
    Ok(Placement {
        leader_z: [read(0x108c)?, read(0x1098)?, read(0x10a4)?, read(0x10b0)?],
        front_x: read(0x1130)?,
        row_step: read(0x1104)?,
        member_x: read(0x1134)?,
        member_z: read(0x1138)?,
        other_row_center: read(0x113c)?,
        single_row_z: [read(0x1140)?, read(0x1144)?],
    })
}

fn voice_sequence(module: &Rel, character: usize) -> Result<Vec<VoiceSequence>> {
    let address = (5, 0x59d8 + character * 4);
    let Some(&pointer) = module.pointers.get(&address) else {
        ensure!(
            u32::read(module.at(address)?, 0)? == 0,
            "invalid voice table pointer"
        );
        return Ok(vec![]);
    };
    let mut records = Vec::new();
    for row in module.at(pointer)?.chunks_exact(6) {
        let technique = u16::read(row, 0)?;
        if technique == 0 {
            return Ok(records);
        }
        records.push(VoiceSequence {
            technique,
            chant: u16::read(row, 2)?,
            release: u16::read(row, 4)?,
        });
    }
    anyhow::bail!("unterminated technique voice table")
}

#[cfg(test)]
mod tests;
