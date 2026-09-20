//! Authored UV commands, before the actor supplies its model or atlas binding.
use super::*;
use serde::Serialize;

const ROW_BYTES: usize = 10;

/// The same ten-byte row can be initialized, advanced into, or used as a loop
/// target. Preserve its operands before those execution paths interpret it.
#[derive(Debug, serde::Deserialize, Serialize)]
pub(crate) struct Record {
    pub timing: u8,
    pub control: u8,
    pub values: [i16; 4],
}

impl Record {
    fn read(row: &[u8]) -> Result<Self> {
        ensure!(row.len() == ROW_BYTES, "truncated effect UV row");
        Ok(Self {
            timing: row[0],
            control: row[1],
            values: [2, 4, 6, 8].map(|at| i16::from_be_bytes([row[at], row[at + 1]])),
        })
    }

    fn command(self) -> Result<Command> {
        Ok(match self.timing {
            254 => Command::End {
                values: self.values,
            },
            255 => Command::Loop {
                target: self.control,
            },
            duration @ 1..=127 => Command::Frame {
                duration,
                update: if self.values[0] == -32000 {
                    Update::Palette {
                        index: self.values[1] as u8,
                    }
                } else {
                    Update::Rect { rect: self.values }
                },
            },
            duration @ 128..=253 => Command::Scroll {
                duration: duration & 127,
                values: self.values,
            },
            opcode => bail!("unsupported effect UV command {opcode}"),
        })
    }

    #[cfg(test)]
    pub(super) fn bytes(&self) -> Vec<u8> {
        [self.timing, self.control]
            .into_iter()
            .chain(self.values.into_iter().flat_map(i16::to_be_bytes))
            .collect()
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Command {
    Frame { duration: u8, update: Update },
    Scroll { duration: u8, values: [i16; 4] },
    Loop { target: u8 },
    End { values: [i16; 4] },
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Update {
    Rect { rect: [i16; 4] },
    Palette { index: u8 },
}

/// Include stored rows after a terminal command, independently of track entrypoints.
pub(super) fn rows(bank: &[u8]) -> Result<BTreeMap<u16, Record>> {
    let start = usize::from(half(bank, 14)?);
    let table = usize::from(half(bank, 16)?);
    ensure!(start >= 20 && start <= table, "invalid effect UV pool");
    let end = if start == table {
        start
    } else {
        [8, 10, 12, 16, 18]
            .into_iter()
            .map(|field| half(bank, field).map(usize::from))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .filter(|&offset| offset > start)
            .min()
            .context("unbounded effect UV pool")?
    };
    let pool = bank
        .get(start..end)
        .context("effect UV pool exceeds bank")?;
    ensure!(
        pool.len().is_multiple_of(ROW_BYTES),
        "effect UV pool has an unexplained partial row"
    );
    pool.chunks_exact(ROW_BYTES)
        .enumerate()
        .map(|(index, row)| Ok((u16::try_from(index * ROW_BYTES)?, Record::read(row)?)))
        .collect()
}

pub(super) fn commands(bank: &[u8], offset: u16) -> Result<Vec<Command>> {
    ensure!(
        offset.is_multiple_of(ROW_BYTES as u16) && usize::from(offset) / ROW_BYTES < 128,
        "invalid authored UV row offset"
    );
    let start = usize::from(half(bank, 14)?) + usize::from(offset);
    let mut commands = Vec::new();
    for index in 0..RECORD_LIMIT {
        let row = bank
            .get(start + index * ROW_BYTES..start + (index + 1) * ROW_BYTES)
            .context("unterminated effect UV animation")?;
        let command = Record::read(row)?.command()?;
        let terminal = !matches!(command, Command::Frame { .. });
        commands.push(command);
        if terminal {
            return Ok(commands);
        }
    }
    bail!("effect UV animation exceeds record limit")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physical_rows_preserve_all_operands_without_runtime_admission() {
        let mut bank = vec![0; 50];
        bank[14..16].copy_from_slice(&20u16.to_be_bytes());
        bank[16..18].copy_from_slice(&50u16.to_be_bytes());
        bank[18..20].copy_from_slice(&50u16.to_be_bytes());
        bank[20] = 128;
        bank[30] = 255;
        assert_eq!(commands(&bank, 0).unwrap().len(), 1);
        let physical = rows(&bank).unwrap();
        assert_eq!(physical.len(), 3);
        assert_eq!(physical[&10].timing, 255);
        assert_eq!(physical[&20].timing, 0);
        assert!(commands(&bank, 20).is_err());
        for timing in 0..=255 {
            let row = [timing, 219, 0x83, 0, 0xff, 0x1b, 1, 2, 0x80, 0];
            let record = Record::read(&row).unwrap();
            assert_eq!(record.bytes(), row);
            if let Ok(Command::Frame {
                update: Update::Palette { index },
                ..
            }) = record.command()
            {
                assert_eq!(index, 27);
            }
        }
        bank[16..18].copy_from_slice(&49u16.to_be_bytes());
        assert!(rows(&bank).is_err());
    }
}
