//! Shared opening strike and its late contact variant.
use crate::{read::Field, rel::Rel};
use anyhow::{Context, Result, bail, ensure};
use resonance_content::{battle_action::ActionPose, battle_normal::Event, battle_unison::Opener};

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
    let pose = ActionPose {
        clip: u16::from(row[2]),
        blend: row[3],
        start: f32::from(row[4]) * 0.5,
        end: (row[5] != 0).then_some(f32::from(row[5]) * 0.5),
        rate: f32::read(row, 8)?,
        repeat: false,
    };
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
    (0..9)
        .map(|character| {
            let row = module.at((
                5,
                if character == 2 {
                    LATE_CONTACT
                } else {
                    CONTACT
                },
            ))?;
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
            let duration = u16::read(config, 0)?;
            Ok(Opener {
                duration,
                recovery: u16::read(config, 2)?,
                reach: [u16::read(config, 20)?, u16::read(config, 22)?],
                // Genis and Raine retain their slower opening interval.
                contact_ticks: if matches!(character, 2 | 3) {
                    45
                } else {
                    duration
                },
                events,
            })
        })
        .collect()
}
