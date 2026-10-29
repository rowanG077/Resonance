//! The small native participant tracks used by the thrust combination scenes.
use crate::{battle_projectiles, dol};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle_action::{ActionPose, ContactAttachments, MeleeContact},
    battle_martial::{Attack, Flight, gameplay_tick},
    battle_normal::Event,
    battle_projectile::{DamageKind, HitShape},
    battle_scene::SceneAction,
    battle_voice::Sound,
};
use std::collections::BTreeMap;

fn section(bytes: &[u8], start: usize, end: usize) -> Result<&[u8]> {
    bytes
        .get(start..end)
        .context("combination section exceeds package")
}

fn number(bytes: &[u8], at: usize) -> Result<f32> {
    let value = f32::from_bits(dol::u32(bytes, at)?);
    ensure!(value.is_finite(), "non-finite combination value");
    Ok(value)
}

fn pose(row: &[u8]) -> Result<ActionPose> {
    ensure!(
        row.len() == 12 && row[7] == 255 && row[6] & !0x40 == 8,
        "unsupported combination pose binding"
    );
    let rate = number(row, 8)?;
    Ok(ActionPose {
        clip: u16::from(row[2]),
        start: f32::from(row[4]) * rate.abs(),
        end: (row[5] != 0).then_some(f32::from(row[5]) * rate.abs()),
        rate,
        blend: row[3],
        repeat: row[6] & 0x40 != 0,
    })
}

fn contact(row: &[u8], rules: &[u8]) -> Result<MeleeContact> {
    let count = usize::from(row[3]);
    ensure!(
        row.len() == 32
            && row[2] > 0
            && (1..=4).contains(&count)
            && (row[4] < 254 || count == 1)
            && (row[4] >= 254 || row[4 + count..8].iter().all(|&value| value == 0))
            && row[16] == 0
            && row[17] <= 2
            && row[19] <= 1
            && dol::u16(row, 22)? == 0,
        "unsupported combination contact {row:02x?}"
    );
    let rule = rules
        .get(usize::from(row[18]) * 28..(usize::from(row[18]) + 1) * 28)
        .context("combination contact selects a missing rule")?;
    let mut hit = battle_projectiles::scene_hit(rule, row[20])?;
    hit.kind = match row[17] {
        0 => DamageKind::Slash,
        1 => DamageKind::Thrust,
        _ => DamageKind::Magic,
    };
    hit.bypass_guard = row[19] == 0;
    let attachments = match row[4] {
        255 if count == 1 => ContactAttachments::BodyCenter {},
        254 if count == 1 => ContactAttachments::BodyBone(u16::from(row[5])),
        _ => ContactAttachments::Groups(row[4..4 + count].to_vec()),
    };
    Ok(MeleeContact {
        duration: u16::from(row[2]) + 1,
        attachments,
        shape: HitShape::Box,
        radius: number(row, 8)?,
        height: number(row, 12)?,
        hit,
    })
}

fn voice(encoded: u16) -> Option<Sound> {
    match encoded {
        0 => None,
        value if value & 0x8000 != 0 => Some(Sound::Stream(value & 0x7fff)),
        value => Some(Sound::Cue(value + 501)),
    }
}

fn variant(
    row: &[u8],
    rules: &[u8],
    contacts: &[u8],
    poses: &[u8],
    commands: &[u8],
) -> Result<Attack> {
    let mut events = Vec::new();
    let mut holds = Vec::new();
    let mut finish_pose = None;
    let mut cursor = dol::u32(row, 20)? as usize * 12;
    loop {
        let source = poses
            .get(cursor..cursor + 12)
            .context("unterminated combination poses")?;
        let at = dol::u16(source, 0)? as i16;
        if at == -2 {
            break;
        }
        if at == -3 {
            ensure!(
                finish_pose.replace(pose(source)?).is_none(),
                "duplicate combination finish pose"
            );
            cursor += 12;
            continue;
        }
        ensure!(at >= 0, "unsupported combination pose control");
        let at = at as u16;
        let elapsed = u32::from(at)
            + holds
                .iter()
                .filter(|(start, _): &&(u16, u8)| *start < at)
                .map(|(_, blend)| u32::from(*blend))
                .sum::<u32>();
        events.push((elapsed.try_into()?, Event::Pose(pose(source)?)));
        holds.push((at, source[3]));
        cursor += 12;
    }
    let mut preceding = 0;
    let mut cursor = dol::u32(row, 16)? as usize * 32;
    loop {
        let source = contacts
            .get(cursor..cursor + 32)
            .context("unterminated combination contacts")?;
        let at = dol::u16(source, 0)? as i16;
        if at == -1 {
            break;
        }
        ensure!(at >= 0, "unsupported combination contact control");
        events.push((
            gameplay_tick(at as u16, preceding, &holds)?,
            Event::Contact(contact(source, rules)?),
        ));
        preceding += 1;
        cursor += 32;
    }
    let mut cursor = dol::u32(row, 24)? as usize * 2;
    let mut command_at = 0;
    loop {
        let at = dol::u16(commands, cursor)? as i16;
        if at == -1 {
            break;
        }
        ensure!(at >= 0, "unsupported combination command control");
        command_at = command_at.max(at as u16);
        let opcode = dol::u16(commands, cursor + 2)?;
        let (event, width) = match opcode {
            0 => (
                Event::Forward(f32::from(dol::u16(commands, cursor + 4)? as i16) / 10.),
                6,
            ),
            1 => (
                Event::Vertical(f32::from(dol::u16(commands, cursor + 4)? as i16) / 10.),
                6,
            ),
            3 => (
                Event::Gravity(f32::from(dol::u16(commands, cursor + 4)? as i16) / 10.),
                6,
            ),
            5 => {
                ensure!(
                    dol::u16(commands, cursor + 4)? <= 1 && dol::u16(commands, cursor + 6)? == 0,
                    "unsupported combination pass-through mode"
                );
                (Event::PassThrough(dol::u16(commands, cursor + 4)? != 0), 8)
            }
            9 => {
                let braking = f32::from(dol::u16(commands, cursor + 4)? as i16) / 10.;
                ensure!(braking >= 0., "negative combination braking");
                (Event::Braking(braking), 6)
            }
            13 => (
                Event::Trail {
                    slot: dol::u16(commands, cursor + 4)?.try_into()?,
                    duration: dol::u16(commands, cursor + 6)?,
                },
                8,
            ),
            27 => {
                ensure!(
                    dol::u16(commands, cursor + 6)? == 2,
                    "unsupported combination voice priority"
                );
                (Event::Voice(voice(dol::u16(commands, cursor + 4)?)), 8)
            }
            28 => {
                ensure!(
                    dol::u16(commands, cursor + 6)? == 0,
                    "unsupported combination sound mode"
                );
                (Event::Sound(dol::u16(commands, cursor + 4)?), 8)
            }
            _ => anyhow::bail!("unsupported combination command {opcode}"),
        };
        let tick = if command_at == 0 {
            if matches!(
                event,
                Event::Forward(_) | Event::Vertical(_) | Event::Gravity(_)
            ) {
                holds.first().map_or(0, |(_, blend)| u16::from(*blend))
            } else {
                0
            }
        } else {
            gameplay_tick(command_at, 0, &holds)?
        };
        events.push((tick, event));
        cursor += width;
    }
    events.sort_by_key(|(at, _)| *at);
    let duration = gameplay_tick(dol::u16(row, 0)?, 0, &holds)?
        .checked_sub(1)
        .context("empty combination duration")?;
    let raw_chain = dol::u16(row, 6)?;
    let chain_at = if raw_chain == 0 {
        0
    } else {
        gameplay_tick(raw_chain, 0, &holds)?
    };
    Ok(Attack {
        duration,
        chain_at: (chain_at <= duration).then_some(chain_at),
        recovery: dol::u16(row, 2)?,
        effect: match dol::u16(row, 10)? {
            u16::MAX => None,
            value => Some(value),
        },
        events,
        callback_hits: BTreeMap::new(),
        pose_sequence: None,
        flight: finish_pose.map(|finish_pose| Flight {
            gravity_at: 0,
            gravity: -1.,
            finish_pose,
        }),
        hammer: None,
        steal: None,
        fierce_fang: None,
        hurricane_hit: None,
    })
}

pub(super) fn read(bytes: &[u8]) -> Result<BTreeMap<u8, SceneAction>> {
    let offsets = [0, 4, 8, 12]
        .map(|at| dol::u32(bytes, at).map(|value| value as usize))
        .into_iter()
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        offsets[0] >= 44
            && offsets.windows(2).all(|pair| pair[0] <= pair[1])
            && (offsets[0] - 16).is_multiple_of(28),
        "invalid combination sections"
    );
    let rules = section(bytes, offsets[0], offsets[1])?;
    let contacts = section(bytes, offsets[1], offsets[2])?;
    let poses = section(bytes, offsets[2], offsets[3])?;
    let commands = bytes
        .get(offsets[3]..)
        .context("combination commands exceed package")?;
    let mut actions = BTreeMap::new();
    for (index, row) in section(bytes, 16, offsets[0])?.chunks_exact(28).enumerate() {
        if dol::u16(row, 0)? != 0 {
            actions.insert(
                index.try_into()?,
                SceneAction::Prepared(Box::new(variant(row, rules, contacts, poses, commands)?)),
            );
        }
    }
    ensure!(!actions.is_empty(), "combination has no actions");
    Ok(actions)
}
