//! Decode the party's seven directional attack tables into elapsed events.
use crate::{read::Field, rel::Rel};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle_action::ActionPose,
    battle_normal::{Attack, Continuation, Event, WeaponFlight},
};

pub(crate) fn read(module: &Rel) -> Result<Vec<[Attack; 7]>> {
    const ROOT: usize = 0x3a48;
    let flights = weapon_flights(module.at((5, 0x1cbc))?)?;
    (0..9)
        .map(|character| {
            let entry = ROOT + character * 12;
            let directory = module.pointer(5, entry)?;
            let rules = module.at(module.pointer(5, entry + 4)?)?;
            let selectors = module.at(module.pointer(5, entry + 8)?)?;
            (0..7)
                .map(|variant| {
                    let part = |index: usize| {
                        module
                            .at(module
                                .pointer(directory.0, directory.1 + variant * 16 + index * 4)?)
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
            u16::read(row, 2)? == 0,
            "unsupported weapon flight profile flags"
        );
        let profile = WeaponFlight {
            outbound_ticks: u16::try_from(i16::read(row, 0)?)
                .context("negative weapon flight duration")?,
            speed: f32::read(row, 4)?,
            return_speed: f32::read(row, 8)?,
            direction_y: f32::read(row, 12)?,
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
        events.push((
            at,
            Event::Pose(ActionPose {
                clip: u16::from(row[2]),
                start: f32::from(row[4]) * 0.5,
                end: (row[5] != 0).then_some(f32::from(row[5]) * 0.5),
                rate: f32::read(row, 8)?,
                blend: row[3],
                repeat: row[6] & 0x40 != 0,
            }),
        ));
        Ok(())
    })?;
    stream(contacts, 32, -1, |at, row| {
        let event = if row[2] as i8 > 0 {
            Event::Contact(crate::battle_action::contact(rules, row)?)
        } else {
            ensure!(
                row[3] == 1 && row[19] == 1 && u16::read(row, 22)? == 0,
                "unsupported thrown weapon contact"
            );
            Event::Throw {
                slot: row[4],
                flight: flights[match row[2] as i8 {
                    -3 => 0,
                    -4 => 1,
                    -5 => 2,
                    emission => anyhow::bail!("unsupported normal contact emission {emission}"),
                }],
                radius: f32::read(row, 8)?,
                hit: crate::battle_action::hit(rules, row)?,
            }
        };
        events.push((at, event));
        Ok(())
    })?;
    let effect = u16::read(config, 18)?;
    if effect != 0 {
        events.push((0, Event::Effect(effect)));
    }
    let mut cursor = 0;
    loop {
        let at = i16::read(commands, cursor)?;
        if at == -1 {
            break;
        }
        let at = u16::try_from(at).context("unsupported normal command loop")?;
        let opcode = u16::read(commands, cursor + 2)?;
        let operand = u16::read(commands, cursor + 4)?;
        let (event, width) = match opcode {
            0 => (Event::Forward(f32::from(operand as i16) / 10.), 6),
            1 => (Event::Vertical(f32::from(operand as i16) / 10.), 6),
            7 => {
                ensure!(operand & 255 <= 1, "invalid normal weapon visibility");
                (
                    Event::WeaponVisibility {
                        slot: (operand >> 8) as u8,
                        visible: operand & 255 != 0,
                    },
                    6,
                )
            }
            27 => {
                ensure!(
                    (2..=3).contains(&u16::read(commands, cursor + 6)?),
                    "invalid normal voice priority"
                );
                (Event::Voice(crate::battle_voice::sound(operand)), 8)
            }
            28 => {
                ensure!(
                    u16::read(commands, cursor + 6)? == 1,
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
    let duration = elapsed(u16::read(config, 0)?)?;
    let chain = u16::read(config, 4)?;
    let recovery = u16::read(config, 2)?;
    Ok(Attack {
        continuation,
        duration,
        chain_at: (chain != 0).then(|| elapsed(chain)).transpose()?,
        recovery,
        recovery_pose: ActionPose {
            clip: u16::from(config[9]),
            start: 0.,
            end: None,
            rate: f32::read(config, 12)?,
            blend: recovery
                .try_into()
                .context("normal recovery blend exceeds limit")?,
            repeat: true,
        },
        reach: [u16::read(config, 20)?, u16::read(config, 22)?],
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
        let at = i16::read(bytes, offset)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::battle_action::{ContactAttachments, Power};

    #[test]
    fn normal_selectors_keep_character_continuations_and_terminal_fallbacks() -> Result<()> {
        let rows = [0, 14, 2, 0, 1, 0, 255, 0];
        assert_eq!(
            continuation(&rows, 0)?,
            Continuation {
                directions: 14,
                fallback: Some(2)
            }
        );
        assert_eq!(continuation(&rows, 1)?, Continuation::default());
        assert!(continuation(&[0, 14, 7, 0], 0).is_err());
        Ok(())
    }

    #[test]
    fn thrown_contacts_select_the_authored_flight_profile() -> Result<()> {
        let mut bytes = Vec::new();
        for (ticks, vertical) in [(14_u16, 0_f32), (10, -0.7), (8, 0.4)] {
            bytes.extend(ticks.to_be_bytes());
            bytes.extend([0, 0]);
            for value in [25_f32, 25., vertical] {
                bytes.extend(value.to_be_bytes());
            }
        }
        let flights = weapon_flights(&bytes)?;
        let mut config = [0; 24];
        config[0..2].copy_from_slice(&30_u16.to_be_bytes());
        config[12..16].copy_from_slice(&0.5_f32.to_be_bytes());
        let mut contact = [0; 34];
        contact[3] = 1;
        contact[19] = 1;
        contact[32..].copy_from_slice(&(-1_i16).to_be_bytes());
        for (emission, expected) in [-3_i8, -4, -5].into_iter().zip(flights) {
            contact[2] = emission as u8;
            let attack = decode(
                &config,
                &[0; 28],
                &contact,
                &(-2_i16).to_be_bytes(),
                &(-1_i16).to_be_bytes(),
                Continuation::default(),
                &flights,
            )?;
            let Event::Throw { flight, .. } = attack.events[0].1 else {
                panic!("missing throw")
            };
            assert_eq!(flight, expected);
        }
        assert_eq!(flights.map(|profile| profile.outbound_ticks), [14, 10, 8]);
        assert_eq!(flights.map(|profile| profile.speed), [25.; 3]);
        assert_eq!(flights.map(|profile| profile.return_speed), [25.; 3]);
        assert_eq!(flights.map(|profile| profile.direction_y), [0., -0.7, 0.4]);
        Ok(())
    }

    #[test]
    fn normal_contacts_keep_weapon_selection_and_the_inclusive_elapsed_window() -> Result<()> {
        let mut config = [0; 24];
        config[0..2].copy_from_slice(&30_u16.to_be_bytes());
        config[2..4].copy_from_slice(&10_u16.to_be_bytes());
        config[4..6].copy_from_slice(&15_u16.to_be_bytes());
        config[12..16].copy_from_slice(&0.5_f32.to_be_bytes());
        config[20..22].copy_from_slice(&120_u16.to_be_bytes());
        let mut rule = [0; 28];
        rule[3] = 30;
        rule[4] = 120;
        let mut poses = vec![0, 0, 30, 4, 2, 0, 8, 255];
        poses.extend(0.5_f32.to_be_bytes());
        poses.extend((-2_i16).to_be_bytes());
        let mut contact = vec![0; 32];
        contact[0..2].copy_from_slice(&8_u16.to_be_bytes());
        contact[2] = 10;
        contact[3] = 1;
        contact[19] = 1;
        contact.extend((-1_i16).to_be_bytes());
        let flights = [WeaponFlight {
            outbound_ticks: 1,
            speed: 1.,
            return_speed: 1.,
            direction_y: 0.,
        }; 3];
        let attack = decode(
            &config,
            &rule,
            &contact,
            &poses,
            &(-1_i16).to_be_bytes(),
            Continuation::default(),
            &flights,
        )?;
        assert_eq!(
            (attack.duration, attack.chain_at, attack.recovery),
            (34, Some(19), 10)
        );
        let Event::Contact(contact) = &attack.events[1].1 else {
            panic!("missing contact")
        };
        assert_eq!((attack.events[1].0, contact.duration), (12, 11));
        assert!(
            matches!(&contact.attachments, ContactAttachments::Groups(groups) if groups == &[0])
        );
        assert_eq!(
            (contact.hit.power, contact.hit.hitstun, contact.hit.cooldown),
            (Power::Normal, 30, 120)
        );
        let Event::Pose(pose) = attack.events[0].1 else {
            panic!("missing pose")
        };
        assert_eq!((pose.clip, pose.start), (30, 1.));
        Ok(())
    }
}
