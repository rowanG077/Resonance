//! Six-byte authored events; scheduling never consumes a repeated payload's own time.
use crate::read::u16 as half;
use anyhow::{Context, Result};
use resonance_content::battle::effect_inventory::EffectAttachment;
use serde::{Deserialize, Serialize};

pub(crate) const RECORD_BYTES: usize = 6;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(crate) struct Record {
    pub tick: i16,
    pub command: Command,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Command {
    Emit {
        actor: u8,
        attachment: EffectAttachment,
        modifier: u16,
    },
    Sound {
        sound: u8,
        priority: u16,
    },
    ModifyRetained {
        slot: u8,
        modifier: u16,
    },
    Repeat {
        count: u8,
        interval: i16,
    },
    End {
        operand_byte: u8,
        operand_word: u16,
    },
}

impl Record {
    pub(crate) fn read(bytes: &[u8]) -> Result<Self> {
        let row = bytes
            .get(..RECORD_BYTES)
            .context("truncated effect event")?;
        let argument = half(row, 4)?;
        Ok(Self {
            tick: half(row, 0)? as i16,
            command: match row[2] {
                252 => Command::Sound {
                    sound: row[3],
                    priority: argument,
                },
                253 => Command::ModifyRetained {
                    slot: row[3],
                    modifier: argument,
                },
                254 => Command::End {
                    operand_byte: row[3],
                    operand_word: argument,
                },
                255 => Command::Repeat {
                    count: row[3],
                    interval: argument as i16,
                },
                actor => Command::Emit {
                    actor,
                    attachment: match row[3] {
                        0 => EffectAttachment::Emitter,
                        bone @ 1..=249 => EffectAttachment::Bone(bone),
                        group => EffectAttachment::BoneGroup(group - 250),
                    },
                    modifier: argument,
                },
            },
        })
    }

    #[cfg(test)]
    pub(crate) fn source_bytes(self) -> [u8; RECORD_BYTES] {
        let (opcode, byte, word) = match self.command {
            Command::Emit {
                actor,
                attachment,
                modifier,
            } => (
                actor,
                match attachment {
                    EffectAttachment::Emitter => 0,
                    EffectAttachment::Bone(bone) => bone,
                    EffectAttachment::BoneGroup(group) => group + 250,
                },
                modifier,
            ),
            Command::Sound { sound, priority } => (252, sound, priority),
            Command::ModifyRetained { slot, modifier } => (253, slot, modifier),
            Command::Repeat { count, interval } => (255, count, interval as u16),
            Command::End {
                operand_byte,
                operand_word,
            } => (254, operand_byte, operand_word),
        };
        let [high, low] = self.tick.to_be_bytes();
        let [word_high, word_low] = word.to_be_bytes();
        [high, low, opcode, byte, word_high, word_low]
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct Event {
    pub tick: i16,
    pub command: Command,
    pub repeat: Option<Repeat>,
}

#[derive(Debug, Serialize)]
pub(crate) struct Repeat {
    pub count: u8,
    pub interval: i16,
}

pub(crate) struct Timeline<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Timeline<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    pub(crate) fn offset(&self) -> usize {
        self.offset
    }

    fn record(&mut self) -> Result<Record> {
        let record = Record::read(
            self.bytes
                .get(self.offset..)
                .context("unterminated effect timeline")?,
        )?;
        self.offset += RECORD_BYTES;
        Ok(record)
    }

    pub(crate) fn next(&mut self) -> Result<Event> {
        let record = self.record()?;
        let (command, repeat) = match record.command {
            Command::Repeat { count, interval } => (
                self.record()
                    .context("missing repeated effect command")?
                    .command,
                Some(Repeat { count, interval }),
            ),
            command => (command, None),
        };
        Ok(Event {
            tick: record.tick,
            command,
            repeat,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_rows_preserve_inactive_operands_without_changing_dispatch() -> Result<()> {
        for opcode in [0, 251, 252, 253, 254, 255] {
            for attachment in [0, 1, 249, 250, 255] {
                let row = [0x80, 0x01, opcode, attachment, 0xab, 0xcd];
                let record: Record =
                    serde_json::from_value(serde_json::to_value(Record::read(&row)?)?)?;
                assert_eq!(record.source_bytes(), row);
            }
        }
        let bytes = [
            0, 16, 255, 3, 0, 8, 0x80, 0x01, 252, 87, 0xab, 0xcd, 0, 20, 254, 255, 0x12, 0x34,
        ];
        let mut timeline = Timeline::new(&bytes);
        let event = timeline.next()?;
        assert_eq!(event.tick, 16);
        assert_eq!(
            event
                .repeat
                .as_ref()
                .map(|repeat| (repeat.count, repeat.interval)),
            Some((3, 8))
        );
        assert!(matches!(
            event.command,
            Command::Sound {
                sound: 87,
                priority: 0xabcd
            }
        ));
        assert_eq!(timeline.offset(), 12);
        assert!(matches!(
            timeline.next()?.command,
            Command::End {
                operand_byte: 255,
                operand_word: 0x1234
            }
        ));
        assert!(timeline.next().is_err());
        assert!(Timeline::new(&bytes[..11]).next().is_err());
        assert!(Record::read(&bytes[..5]).is_err());

        let mut bank = [0; 48];
        bank[..5].copy_from_slice(b"ef1\0\x01");
        bank[10..12].copy_from_slice(&20u16.to_be_bytes());
        bank[16..20].copy_from_slice(&[0, 44, 0, 46]);
        bank[20..38].copy_from_slice(&bytes);
        bank[38..44].copy_from_slice(&[0, 30, 252, 1, 0, 0]); // Unreferenced suffix.
        let id = super::super::EffectId {
            bank: super::super::EffectBank::Common,
            id: 0,
        };
        let mut cooker = super::super::test_cooker();
        cooker.program(&bank, id)?;
        let program = cooker.result.program(id).unwrap();
        assert_eq!(program.end_tick, 20);
        assert_eq!(program.emissions.len(), 1);
        assert_eq!(program.emissions[0].tick, 16);
        assert!(matches!(
            program.emissions[0].command,
            super::super::EffectCommand::Sound {
                sound: 87,
                priority: 0xcd
            }
        ));
        for (at, value) in [(20, 255), (23, 0), (28, 255), (28, 254)] {
            let mut changed = bank;
            changed[at] = value;
            assert!(super::super::test_cooker().program(&changed, id).is_err());
            if (at, value) == (28, 255) {
                assert!(super::super::source::program(&changed, 0).is_err());
            }
            for row in changed[20..44].chunks_exact(RECORD_BYTES) {
                assert_eq!(Record::read(row)?.source_bytes(), row);
            }
        }
        Ok(())
    }
}
