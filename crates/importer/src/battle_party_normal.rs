//! Decode the party's seven directional attack tables into elapsed events.
use crate::{battle_party_native as native, dol, rel::Rel};
use anyhow::{Context, Result, ensure};
use resonance_content::battle_action::WeaponSlot;
use resonance_content::battle_normal::{Attack, Continuation, Event, WeaponFlight};

pub(crate) fn read(module: &Rel) -> Result<Vec<[Attack; 7]>> {
    const ROOT: usize = 0x3a48;
    let at = |pointer: (usize, usize)| module.tail(pointer.0, pointer.1);
    let flights = weapon_flights(module.tail(5, 0x1cbc)?)?;
    (0..9)
        .map(|character| {
            let entry = ROOT + character * 12;
            let directory = module.pointer(5, entry)?;
            let rules = at(module.pointer(5, entry + 4)?)?;
            let selectors = at(module.pointer(5, entry + 8)?)?;
            (0..7)
                .map(|variant| {
                    let part = |index: usize| {
                        at(module.pointer(directory.0, directory.1 + variant * 16 + index * 4)?)
                    };
                    decode(
                        part(0)?,
                        rules,
                        part(1)?,
                        part(2)?,
                        part(3)?,
                        continuation(selectors, variant)?,
                        &flights,
                    )
                    .with_context(|| format!("party {} normal attack {variant}", character + 1))
                })
                .collect::<Result<Vec<_>>>()?
                .try_into()
                .map_err(|_| anyhow::anyhow!("invalid normal attack count"))
        })
        .collect()
}

fn continuation(bytes: &[u8], index: usize) -> Result<Continuation> {
    let row = bytes
        .get(index * 4..index * 4 + 4)
        .context("truncated normal selector")?;
    ensure!(
        row[0] as usize == index && row[1] & 0x80 == 0 && row[3] == 0,
        "unsupported normal selector"
    );
    ensure!(row[2] < 7 || row[2] == 255, "invalid normal fallback");
    Ok(Continuation {
        directions: row[1],
        fallback: (row[2] != 255).then_some(row[2]),
    })
}

fn weapon_flights(bytes: &[u8]) -> Result<[WeaponFlight; 3]> {
    let mut profiles = Vec::with_capacity(3);
    for index in 0..3 {
        let row = bytes
            .get(index * 16..index * 16 + 16)
            .context("truncated weapon flight profile")?;
        ensure!(
            half(row, 2)? == 0,
            "unsupported weapon flight profile flags"
        );
        let profile = WeaponFlight {
            outbound_ticks: u16::try_from(signed(row, 0)?)
                .context("negative weapon flight duration")?,
            speed: float(row, 4)?,
            return_speed: float(row, 8)?,
            direction_y: float(row, 12)?,
        };
        ensure!(
            profile.outbound_ticks > 0 && profile.speed > 0. && profile.return_speed > 0.,
            "invalid weapon flight profile"
        );
        profiles.push(profile);
    }
    Ok(profiles.try_into().unwrap())
}

fn decode(
    config: &[u8],
    rules: &[u8],
    contacts: &[u8],
    poses: &[u8],
    commands: &[u8],
    continuation: Continuation,
    flights: &[WeaponFlight; 3],
) -> Result<Attack> {
    let config = config
        .get(..24)
        .context("truncated normal attack configuration")?;
    let mut events = Vec::new();
    stream(poses, 12, -2, |at, row| {
        ensure!(
            row[7] == 255 && row[6] & !0x40 == 8,
            "unsupported normal attack pose"
        );
        events.push((at, Event::Pose(native::action_pose(row)?)));
        Ok(())
    })?;
    stream(contacts, 32, -1, |at, row| {
        let event = if row[2] as i8 > 0 {
            Event::Contact(native::contact(rules, row)?)
        } else {
            ensure!(
                row[3] == 1 && row[19] == 1 && half(row, 22)? == 0,
                "unsupported thrown weapon contact"
            );
            Event::Throw {
                slot: WeaponSlot(row[4]),
                flight: flights[match row[2] as i8 {
                    -3 => 0,
                    -4 => 1,
                    -5 => 2,
                    emission => anyhow::bail!("unsupported normal contact emission {emission}"),
                }],
                radius: float(row, 8)?,
                hit: native::hit(rules, row)?,
            }
        };
        events.push((at, event));
        Ok(())
    })?;
    let effect = half(config, 18)?;
    if effect != 0 {
        events.push((0, Event::Effect(effect)));
    }
    let mut cursor = 0;
    loop {
        let at = signed(commands, cursor)?;
        if at == -1 {
            break;
        }
        let at = u16::try_from(at).context("unsupported normal command loop")?;
        let opcode = half(commands, cursor + 2)?;
        let operand = half(commands, cursor + 4)?;
        let (event, width) = match opcode {
            0 => (Event::Forward(f32::from(operand as i16) / 10.), 6),
            1 => (Event::Vertical(f32::from(operand as i16) / 10.), 6),
            7 => {
                ensure!(operand & 255 <= 1, "invalid normal weapon visibility");
                (
                    Event::WeaponVisibility {
                        slot: WeaponSlot((operand >> 8) as u8),
                        visible: operand & 255 != 0,
                    },
                    6,
                )
            }
            27 => {
                ensure!(
                    (2..=3).contains(&half(commands, cursor + 6)?),
                    "invalid normal voice priority"
                );
                (Event::Voice(native::sound(operand)), 8)
            }
            28 => {
                ensure!(
                    half(commands, cursor + 6)? == 1,
                    "unsupported normal sound mode"
                );
                (Event::Sound(operand), 8)
            }
            other => anyhow::bail!("unsupported normal command {other}"),
        };
        events.push((at, event));
        cursor += width;
    }
    let holds: Vec<_> = events
        .iter()
        .filter_map(|(at, event)| match event {
            Event::Pose(pose) => Some((
                *at,
                if *at == 0 {
                    pose.blend
                } else {
                    pose.blend.saturating_sub(1)
                },
            )),
            _ => None,
        })
        .collect();
    let elapsed = |at: u16| -> Result<u16> {
        let delay: u32 = holds
            .iter()
            .filter(|(start, _)| *start < at)
            .map(|(_, blend)| u32::from(*blend))
            .sum();
        (u32::from(at) + delay)
            .try_into()
            .context("normal attack timeline overflow")
    };
    let opening_hold = holds
        .first()
        .filter(|(at, _)| *at == 0)
        .map_or(0, |(_, blend)| u16::from(*blend));
    for (at, event) in &mut events {
        *at = if *at == 0 && matches!(event, Event::Forward(_) | Event::Vertical(_)) {
            opening_hold
        } else {
            elapsed(*at)?
        };
    }
    events.sort_by_key(|(at, _)| *at);
    let duration = elapsed(half(config, 0)?)?;
    let chain = half(config, 4)?;
    let retry = half(config, 6)?;
    let recovery = half(config, 2)?;
    Ok(Attack {
        continuation,
        duration,
        chain_at: (chain != 0).then(|| elapsed(chain)).transpose()?,
        chain_retry_at: (retry != 0).then(|| elapsed(retry)).transpose()?,
        recovery,
        reach: [half(config, 20)?, half(config, 22)?],
        events,
    })
}

fn stream(
    bytes: &[u8],
    width: usize,
    end: i16,
    mut row: impl FnMut(u16, &[u8]) -> Result<()>,
) -> Result<()> {
    for offset in (0..bytes.len()).step_by(width) {
        let at = signed(bytes, offset)?;
        if at == end {
            return Ok(());
        }
        row(
            u16::try_from(at).context("unsupported normal stream loop")?,
            bytes
                .get(offset..offset + width)
                .context("truncated normal stream")?,
        )?;
    }
    anyhow::bail!("unterminated normal stream")
}

fn half(bytes: &[u8], offset: usize) -> Result<u16> {
    dol::u16(bytes, offset)
}
fn signed(bytes: &[u8], offset: usize) -> Result<i16> {
    Ok(half(bytes, offset)? as i16)
}
fn float(bytes: &[u8], offset: usize) -> Result<f32> {
    Ok(f32::from_bits(dol::u32(bytes, offset)?))
}
