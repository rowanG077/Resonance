//! Shared opening strike and its late contact variant.
use crate::{read::Field, rel::Rel};
use anyhow::{Context, Result, bail, ensure};
use resonance_content::{battle_normal::Event, battle_unison::Opener};

const CONFIG: usize = 0x960;
const HIT: usize = 0x978;
const POSE: usize = 0x994;
const CONTACT: usize = 0x9ac;
const LATE_CONTACT: usize = 0x9ec;
const COMMANDS: usize = 0xa2c;

pub(super) fn read(module: &Rel) -> Result<Vec<Opener>> {
    let config = module.at((5, CONFIG))?;
    let rules = module.at((5, HIT))?;
    let row = module.at((5, POSE))?;
    ensure!(
        u16::read(row, 0)? == 0 && row[6] == 8 && row[7] == 255,
        "unsupported Unison opening pose"
    );
    let pose = crate::battle_action::action_pose(row)?;
    // The opening update consumes the first blend visit. Later authored ages
    // therefore start after the remaining blend visits.
    let elapsed = |tick: u16| -> Result<u16> {
        tick.checked_add(if tick == 0 {
            0
        } else {
            u16::from(pose.blend.saturating_sub(1))
        })
        .context("Unison opening timeline overflow")
    };
    let mut events = vec![(0, Event::Pose(pose))];
    let commands = module.at((5, COMMANDS))?;
    let mut cursor = 0;
    loop {
        let tick = i16::read(commands, cursor)?;
        if tick == -1 {
            break;
        }
        let tick = u16::try_from(tick).context("unsupported Unison command time")?;
        let (event, width) = match u16::read(commands, cursor + 2)? {
            0 => (
                Event::Forward(f32::from(i16::read(commands, cursor + 4)?) / 10.),
                6,
            ),
            13 => (
                Event::Trail {
                    slot: u16::read(commands, cursor + 4)?
                        .try_into()
                        .context("invalid Unison trail slot")?,
                    duration: u16::read(commands, cursor + 6)?,
                },
                8,
            ),
            opcode => bail!("unsupported Unison opening command {opcode}"),
        };
        events.push((elapsed(tick)?, event));
        cursor += width;
    }
    let contact_events = |offset| -> Result<_> {
        let row = module.at((5, offset))?;
        ensure!(
            i16::read(row, 32)? == -1,
            "multiple Unison opening contacts"
        );
        let mut events = events.clone();
        events.push((
            elapsed(u16::read(row, 0)?)?,
            Event::Contact(crate::battle_action::contact(rules, row)?),
        ));
        events.sort_by_key(|(tick, _)| *tick);
        Ok(events)
    };
    let duration = u16::read(config, 0)?;
    let common = Opener {
        duration,
        recovery: u16::read(config, 2)?,
        reach: <[u16; 2]>::read(config, 20)?,
        contact_ticks: duration,
        events: contact_events(CONTACT)?,
    };
    let mut openers = vec![common; 9];
    openers[2].events = contact_events(LATE_CONTACT)?;
    // Genis and Raine retain their slower opening interval.
    for character in [2, 3] {
        openers[character].contact_ticks = 45;
    }
    Ok(openers)
}
