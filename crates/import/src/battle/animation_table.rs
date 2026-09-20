//! Animation pools have separate cursor dispatch and unconditional motion binds.
//! Callback-selected roots are not all statically known; retain unreferenced bytes.
use crate::read::{Storage, f32 as float, u16 as half};
use anyhow::{Context, Result};
use resonance_content::battle::actions::{
    AnimationBinding, AnimationCommand, AnimationInstruction, AnimationProgram, AnimationStep,
    AnimationTrigger,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
};

pub(super) const ROW_BYTES: usize = 12;

#[derive(Serialize, Deserialize)]
pub(crate) struct Parsed {
    records: Vec<Record>,
    unreferenced_storage: Vec<Storage>,
    #[cfg(test)]
    #[serde(skip)]
    pub covered: Vec<Range<usize>>,
}

#[derive(Serialize, Deserialize)]
struct Record {
    offset: usize,
    time: i16,
    #[serde(skip_serializing_if = "Option::is_none")]
    dispatch: Option<Dispatch>,
    #[serde(skip_serializing_if = "Option::is_none")]
    forced_bind: Option<AnimationCommand>,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Dispatch {
    End,
    Texture {
        layers: [u8; 2],
    },
    Rate {
        rate: f32,
    },
    Loop {
        target: u8,
    },
    Play {
        trigger: AnimationTrigger,
        command: AnimationCommand,
    },
    Stalled {
        opcode: u8,
    },
}

struct Reader<'a> {
    bytes: &'a [u8],
    records: BTreeMap<usize, Record>,
    covered: Vec<Range<usize>>,
}

fn motion(row: &[u8]) -> Result<AnimationCommand> {
    let row = row
        .get(..ROW_BYTES)
        .context("truncated animation descriptor")?;
    Ok(AnimationCommand::Play {
        clip: row[2],
        blend: row[3],
        start: row[4],
        end: (row[5] != 0).then_some(row[5]),
        layer: row[6] & 63,
        looping: row[6] & 64 != 0,
        mirror: row[6] & 128 != 0,
        resource: row[7] as i8,
        rate: float(row, 8)?,
    })
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            records: BTreeMap::new(),
            covered: Vec::new(),
        }
    }

    fn row(&self, offset: usize, size: usize) -> Result<&[u8]> {
        let end = offset
            .checked_add(size)
            .context("animation offset overflow")?;
        self.bytes
            .get(offset..end)
            .context("animation access outside table")
    }

    fn record(&mut self, offset: usize) -> Result<&mut Record> {
        let time = half(self.row(offset, 2)?, 0)? as i16;
        self.covered.push(offset..offset + 2);
        Ok(self.records.entry(offset).or_insert(Record {
            offset,
            time,
            dispatch: None,
            forced_bind: None,
        }))
    }

    fn motion(&mut self, offset: usize) -> Result<AnimationCommand> {
        let command = motion(self.row(offset, ROW_BYTES)?)?;
        self.covered.push(offset..offset + ROW_BYTES);
        Ok(command)
    }

    fn bind(&mut self, offset: usize) -> Result<()> {
        // Neither initialization nor a loop redispatches the target's time/opcode.
        let command = self.motion(offset)?;
        self.record(offset)?.forced_bind = Some(command);
        Ok(())
    }

    fn dispatch(&mut self, offset: usize) -> Result<Dispatch> {
        let record = self.record(offset)?;
        if let Some(dispatch) = record.dispatch {
            return Ok(dispatch);
        }
        let time = record.time;
        let dispatch = if time == -2 {
            Dispatch::End
        } else {
            let opcode = self.row(offset, 3)?[2];
            self.covered.push(offset..offset + 3);
            match opcode {
                255 => {
                    let row = self.row(offset, 5)?;
                    let layers = [row[3], row[4]];
                    self.covered.push(offset..offset + 5);
                    Dispatch::Texture { layers }
                }
                254 => {
                    let rate = float(self.row(offset, ROW_BYTES)?, 8)?;
                    self.covered.push(offset + 8..offset + ROW_BYTES);
                    Dispatch::Rate { rate }
                }
                target if time == -1 => Dispatch::Loop { target },
                opcode if time < -4 => Dispatch::Stalled { opcode },
                _ => Dispatch::Play {
                    trigger: match time {
                        -3 => AnimationTrigger::Finished,
                        -4 => AnimationTrigger::Grounded,
                        time => AnimationTrigger::Tick(time),
                    },
                    command: self.motion(offset)?,
                },
            }
        };
        self.records.get_mut(&offset).unwrap().dispatch = Some(dispatch);
        Ok(dispatch)
    }

    fn follow(&mut self, root: usize) -> Result<()> {
        self.bind(root)?;
        self.follow_cursor(root, 1)
    }

    fn follow_cursor(&mut self, root: usize, mut cursor: i8) -> Result<()> {
        let mut visited = [false; 256];
        while !std::mem::replace(&mut visited[cursor as u8 as usize], true) {
            let offset = root
                .checked_add_signed(isize::from(cursor) * ROW_BYTES as isize)
                .context("animation cursor outside table")?;
            match self.dispatch(offset)? {
                Dispatch::End | Dispatch::Stalled { .. } => break,
                Dispatch::Loop { target } => {
                    let offset = root
                        .checked_add(usize::from(target) * ROW_BYTES)
                        .context("animation target overflow")?;
                    self.bind(offset)?;
                    cursor = target.wrapping_add(1) as i8;
                }
                _ => cursor = cursor.wrapping_add(1),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
fn selected(bytes: &[u8]) -> Result<AnimationProgram> {
    let program = selected_at(bytes, 0)?;
    let parsed: Parsed = serde_json::from_value(serde_json::to_value(decode(bytes, [0])?)?)?;
    assert_eq!(
        serde_json::to_value(parsed.selected(0)?)?,
        serde_json::to_value(&program)?
    );
    Ok(program)
}

impl Parsed {
    pub(crate) fn selected(&self, root: usize) -> Result<AnimationProgram> {
        self.selected_with_continuations(root, &[])
    }

    pub(crate) fn selected_with_continuations(
        &self,
        root: usize,
        continuations: &[(u8, i8)],
    ) -> Result<AnimationProgram> {
        let records: BTreeMap<_, _> = self
            .records
            .iter()
            .map(|record| (record.offset, record))
            .collect();
        project(root, continuations, |offset| {
            records
                .get(&offset)
                .copied()
                .context("missing cooked animation record")
        })
    }
}

/// Keep dispatch addresses separate from the unsigned, unconditional loop binds.
pub(crate) fn selected_at(bytes: &[u8], root: usize) -> Result<AnimationProgram> {
    selected_with_continuations(bytes, root, &[])
}

/// Callback entries change the base while retaining the current signed cursor.
pub(crate) fn selected_with_continuations(
    bytes: &[u8],
    root: usize,
    continuations: &[(u8, i8)],
) -> Result<AnimationProgram> {
    let mut reader = Reader::new(bytes);
    reader
        .follow(root)
        .context("selected animation traversal")?;
    for &(rows, cursor) in continuations {
        reader.follow_cursor(
            root.checked_add(usize::from(rows) * ROW_BYTES)
                .context("animation continuation overflow")?,
            cursor,
        )?;
    }
    project(root, continuations, |offset| {
        reader
            .records
            .get(&offset)
            .context("missing selected animation record")
    })
}

fn project<'a>(
    root: usize,
    continuations: &[(u8, i8)],
    mut record: impl FnMut(usize) -> Result<&'a Record>,
) -> Result<AnimationProgram> {
    let binding = |record: &Record| -> Result<AnimationBinding> {
        Ok(AnimationBinding {
            time: record.time,
            command: record
                .forced_bind
                .context("animation has no unconditional binding")?,
        })
    };
    let mut program = AnimationProgram {
        initial: Some(binding(record(root)?)?.command),
        ..Default::default()
    };
    for (rows, mut cursor) in std::iter::once((0, 1)).chain(continuations.iter().copied()) {
        let base = root
            .checked_add(usize::from(rows) * ROW_BYTES)
            .context("animation continuation overflow")?;
        let mut visited = [false; 256];
        while !std::mem::replace(&mut visited[cursor as u8 as usize], true) {
            let offset = base
                .checked_add_signed(isize::from(cursor) * ROW_BYTES as isize)
                .context("animation cursor outside table")?;
            let current = record(offset)?;
            let dispatch = current
                .dispatch
                .context("animation has no cursor dispatch")?;
            let index = i16::from(rows) + i16::from(cursor);
            let step =
                |trigger, command| AnimationInstruction::Step(AnimationStep { trigger, command });
            let instruction = match dispatch {
                Dispatch::End => AnimationInstruction::End,
                Dispatch::Stalled { .. } => AnimationInstruction::Stalled,
                Dispatch::Play { trigger, command } => step(trigger, command),
                Dispatch::Texture { layers } => step(
                    AnimationTrigger::Tick(current.time),
                    AnimationCommand::Texture { layers },
                ),
                Dispatch::Rate { rate } => step(
                    AnimationTrigger::Tick(current.time),
                    AnimationCommand::Rate { rate },
                ),
                Dispatch::Loop { target } => step(
                    AnimationTrigger::LoopAfterFinished(target),
                    AnimationCommand::Loop { step: target },
                ),
            };
            program.instructions.insert(index, instruction);
            match dispatch {
                Dispatch::End | Dispatch::Stalled { .. } => break,
                Dispatch::Loop { target } => {
                    let offset = base
                        .checked_add(usize::from(target) * ROW_BYTES)
                        .context("animation target overflow")?;
                    program.loop_targets.insert(
                        i16::from(rows) + i16::from(target),
                        binding(record(offset)?)?,
                    );
                    cursor = target.wrapping_add(1) as i8;
                }
                _ => cursor = cursor.wrapping_add(1),
            }
        }
    }
    Ok(program)
}

/// Roots and returned record offsets are relative to the entire animation pool.
pub(crate) fn decode(bytes: &[u8], roots: impl IntoIterator<Item = usize>) -> Result<Parsed> {
    decode_with_records(bytes, roots, [], [])
}

/// Callback bases can change while retaining a cursor. Each complete pool row
/// therefore keeps its dispatch and any valid unconditional binding independently.
pub(crate) fn decode_pool(bytes: &[u8], roots: impl IntoIterator<Item = usize>) -> Result<Parsed> {
    let mut reader = Reader::new(bytes);
    for offset in (0..bytes.len() / ROW_BYTES).map(|row| row * ROW_BYTES) {
        reader.dispatch(offset)?;
        if let Ok(command) = reader.motion(offset) {
            reader.record(offset)?.forced_bind = Some(command);
        }
    }
    for root in roots {
        reader.follow(root)?;
    }
    Ok(finish(reader))
}

/// Authored table records are decoded without assuming they are program entries.
pub(crate) fn decode_with_records(
    bytes: &[u8],
    roots: impl IntoIterator<Item = usize>,
    physical_offsets: impl IntoIterator<Item = usize>,
    dormant_roots: impl IntoIterator<Item = usize>,
) -> Result<Parsed> {
    let mut reader = Reader::new(bytes);
    for offset in physical_offsets {
        reader.dispatch(offset)?;
    }
    let roots: BTreeSet<_> = roots.into_iter().collect();
    for &root in &roots {
        reader.follow(root)?;
    }
    // Only the caller's proven unreachable entries may be optional. Preserve
    // malformed inactive operands without admitting a partially decoded program.
    for root in dormant_roots
        .into_iter()
        .collect::<BTreeSet<_>>()
        .difference(&roots)
    {
        let mut dormant = Reader::new(bytes);
        if dormant.follow(*root).is_err() {
            continue;
        }
        reader.covered.extend(dormant.covered);
        for (offset, record) in dormant.records {
            reader
                .records
                .entry(offset)
                .and_modify(|existing| {
                    existing.dispatch = existing.dispatch.or(record.dispatch);
                    existing.forced_bind = existing.forced_bind.or(record.forced_bind);
                })
                .or_insert(record);
        }
    }
    Ok(finish(reader))
}

fn finish(mut reader: Reader<'_>) -> Parsed {
    reader.covered.sort_unstable_by_key(|span| span.start);
    let mut covered: Vec<Range<usize>> = Vec::new();
    for span in reader.covered {
        if let Some(last) = covered.last_mut()
            && span.start <= last.end
        {
            last.end = last.end.max(span.end);
        } else {
            covered.push(span);
        }
    }
    let mut end = 0;
    let mut unreferenced_storage = Vec::new();
    for span in covered
        .iter()
        .cloned()
        .chain([reader.bytes.len()..reader.bytes.len()])
    {
        if end < span.start {
            unreferenced_storage.push(Storage {
                offset: end,
                bytes: reader.bytes[end..span.start].to_vec(),
            });
        }
        end = span.end;
    }
    Parsed {
        records: reader.records.into_values().collect(),
        unreferenced_storage,
        #[cfg(test)]
        covered,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn row(time: i16, opcode: u8) -> [u8; ROW_BYTES] {
        let mut row = [0; ROW_BYTES];
        row[..2].copy_from_slice(&time.to_be_bytes());
        row[2] = opcode;
        row[6] = 8;
        row[7] = 255;
        row[8..].copy_from_slice(&0.5f32.to_be_bytes());
        row
    }

    fn step(program: &AnimationProgram, index: i8) -> &AnimationStep {
        let AnimationInstruction::Step(step) = &program.instructions[&i16::from(index)] else {
            panic!("expected step")
        };
        step
    }

    #[test]
    fn selected_program_preserves_initial_binding_and_reachable_dispatch() -> Result<()> {
        for (time, opcode) in [(-2, 13), (-1, 254), (-3, 255), (120, 12)] {
            let mut bytes = row(time, opcode).to_vec();
            bytes.extend((-2i16).to_be_bytes());
            let parsed = decode(&bytes, [0])?;
            let program = selected(&bytes)?;
            assert_eq!(program.instructions.len(), 1);
            assert!(matches!(
                program.instructions[&1],
                AnimationInstruction::End
            ));
            assert_eq!(
                serde_json::to_value(program.initial)?,
                serde_json::to_value(parsed.records[0].forced_bind)?
            );
            assert!(
                matches!(program.initial, Some(AnimationCommand::Play { clip, resource: -1, .. }) if clip == opcode)
            );
        }
        let mut bytes = [
            row(0, 12),
            row(2, 255),
            row(3, 254),
            row(-3, 13),
            row(-4, 14),
            row(-1, 0),
        ]
        .concat();
        bytes[15..17].copy_from_slice(&[3, 7]);
        bytes[17..24].fill(255);
        let program = selected(&bytes)?;
        assert!(matches!(
            step(&program, 1).command,
            AnimationCommand::Texture { layers: [3, 7] }
        ));
        assert!(matches!(
            step(&program, 2).command,
            AnimationCommand::Rate { rate: 0.5 }
        ));
        assert!(matches!(
            step(&program, 3).trigger,
            AnimationTrigger::Finished
        ));
        assert!(matches!(
            step(&program, 4).trigger,
            AnimationTrigger::Grounded
        ));
        assert!(matches!(
            step(&program, 5).trigger,
            AnimationTrigger::LoopAfterFinished(0)
        ));
        assert_eq!(program.loop_targets[&0].time, 0);
        assert!(matches!(
            program.loop_targets[&0].command,
            AnimationCommand::Play { clip: 12, .. }
        ));
        Ok(())
    }

    #[test]
    fn selected_program_keeps_signed_waits_and_unconditionally_bound_targets() -> Result<()> {
        for (time, opcode) in [(-1, 255), (-4, 254), (-5, 13)] {
            let mut bytes = [row(0, 12), row(time, opcode)].concat();
            bytes.extend((-2i16).to_be_bytes());
            let program = selected(&bytes)?;
            if time == -5 {
                assert!(matches!(
                    program.instructions[&1],
                    AnimationInstruction::Stalled
                ));
            } else {
                assert!(
                    matches!(step(&program, 1).trigger, AnimationTrigger::Tick(value) if value == time)
                );
            }
        }
        for (time, opcode) in [(-2, 13), (7, 254), (-3, 255), (-1, 0)] {
            let mut bytes = [row(7, 12), row(-1, 3), row(-2, 0), row(time, opcode)].concat();
            bytes.extend((-2i16).to_be_bytes());
            let program = selected(&bytes)?;
            assert!(!program.instructions.contains_key(&2));
            assert!(!program.instructions.contains_key(&3));
            assert_eq!(program.loop_targets[&3].time, time);
            assert!(
                matches!(program.loop_targets[&3].command, AnimationCommand::Play { clip, .. } if clip == opcode)
            );
        }
        let bytes = [row(7, 12), row(-1, 0)].concat();
        assert_eq!(selected(&bytes)?.loop_targets[&0].time, 7);
        assert!(selected(&(-2i16).to_be_bytes()).is_err());
        Ok(())
    }

    #[test]
    fn special_opcodes_use_signed_time_before_loop_dispatch() -> Result<()> {
        let mut bytes = [row(0, 12), row(-1, 255), row(-4, 254)].concat();
        bytes[15..17].copy_from_slice(&[3, 7]);
        bytes[17..24].fill(255); // Inactive texture operands need not be finite.
        bytes[27..32].fill(0xaa);
        bytes.extend((-2i16).to_be_bytes());
        bytes.extend([0x81, 0x82]);
        let parsed = decode(&bytes, [0])?;
        let value = serde_json::to_value(&parsed)?;
        assert_eq!(value["records"][1]["time"], -1);
        assert_eq!(
            value["records"][1]["dispatch"],
            json!({"kind":"texture","layers":[3,7]})
        );
        assert_eq!(value["records"][2]["time"], -4);
        assert_eq!(
            value["records"][2]["dispatch"],
            json!({"kind":"rate","rate":0.5})
        );
        assert_eq!(parsed.covered, [0..17, 24..27, 32..38]);
        assert_eq!(
            value["unreferenced_storage"],
            json!([
                {"offset":17,"bytes":vec![255;7]},
                {"offset":27,"bytes":vec![0xaa;5]},
                {"offset":38,"bytes":[0x81,0x82]},
            ])
        );
        let cooked: Parsed = serde_json::from_value(value)?;
        assert_eq!(
            serde_json::to_value(cooked.selected(0)?)?,
            serde_json::to_value(selected(&bytes)?)?
        );
        Ok(())
    }

    #[test]
    fn forced_targets_skip_prior_end_and_ignore_their_dispatch_meaning() -> Result<()> {
        for (time, opcode) in [(-2, 13), (7, 254), (-3, 255), (-1, 0)] {
            let mut bytes = [row(0, 12), row(-1, 3), row(-2, 0), row(time, opcode)].concat();
            bytes.extend((-2i16).to_be_bytes());
            let parsed = decode(&bytes, [0])?;
            let value = serde_json::to_value(&parsed)?;
            let target = &value["records"][2];
            assert_eq!(target["offset"], 36);
            assert_eq!(target["time"], time);
            assert!(target.get("dispatch").is_none());
            assert_eq!(target["forced_bind"]["kind"], "play");
            assert_eq!(target["forced_bind"]["clip"], opcode);
            assert_eq!(parsed.covered, [0..15, 36..50]);
            assert_eq!(
                value["unreferenced_storage"],
                json!([{"offset":15,"bytes":&bytes[15..36]}])
            );
        }
        Ok(())
    }

    #[test]
    fn signed_cursor_wraps_within_the_pool_and_shared_roots_merge() -> Result<()> {
        let mut bytes = vec![0; 256 * ROW_BYTES];
        bytes[..2].copy_from_slice(&(-2i16).to_be_bytes());
        bytes[127 * ROW_BYTES..128 * ROW_BYTES].copy_from_slice(&row(0, 12));
        bytes[128 * ROW_BYTES..129 * ROW_BYTES].copy_from_slice(&row(-1, 128));
        bytes[255 * ROW_BYTES..].copy_from_slice(&row(-2, 13));
        let selected = selected_at(&bytes, 127 * ROW_BYTES)?;
        assert!(matches!(
            selected.instructions[&-127],
            AnimationInstruction::End
        ));
        assert_eq!(selected.loop_targets[&128].time, -2);
        let parsed = decode(&bytes, [127 * ROW_BYTES, 127 * ROW_BYTES])?;
        assert_eq!(parsed.records.len(), 4);
        assert_eq!(parsed.records[0].offset, 0);
        assert!(matches!(parsed.records[0].dispatch, Some(Dispatch::End)));
        assert_eq!(parsed.covered, [0..2, 1524..1539, 3060..3072]);
        let cooked: Parsed = serde_json::from_value(serde_json::to_value(parsed)?)?;
        assert_eq!(
            serde_json::to_value(cooked.selected(127 * ROW_BYTES)?)?,
            serde_json::to_value(selected)?
        );
        assert!(cooked.selected(0).is_err()); // A dispatched terminator is not a root binding.
        Ok(())
    }

    #[test]
    fn bounds_follow_actual_reads_and_closed_loops_need_no_terminator() -> Result<()> {
        let bytes = [row(0, 12), row(-1, 0)].concat();
        decode(&bytes, [0])?;
        let mut bytes = bytes;
        bytes[14] = 2;
        bytes.extend((-2i16).to_be_bytes());
        assert!(decode(&bytes, [0]).is_err()); // A forced bind needs all12 bytes.
        assert!(decode(&bytes, [usize::MAX]).is_err());
        bytes[14] = 127;
        assert!(decode(&bytes, [0]).is_err());
        assert!(decode(&(-2i16).to_be_bytes(), [0]).is_err());
        let mut bytes = row(0, 12).to_vec();
        bytes.extend([0xff, 0xfb, 1]); // An unrecognized negative wait stalls.
        let parsed = decode(&bytes, [0])?;
        assert!(matches!(
            parsed.records[1].dispatch,
            Some(Dispatch::Stalled { opcode: 1 })
        ));
        bytes[12..14].fill(0);
        assert!(decode(&bytes, [0]).is_err());
        let unreferenced = decode(&[255; 15], [])?;
        assert!(unreferenced.records.is_empty());
        assert_eq!(unreferenced.unreferenced_storage[0].bytes, [255; 15]);
        Ok(())
    }

    #[test]
    fn authored_records_do_not_invent_roots_or_follow_inactive_branches() -> Result<()> {
        let bytes = [row(0, 12), row(-2, 0), row(-1, 99), row(-1, 254)].concat();
        let parsed = decode_with_records(&bytes, [0], [0, 12, 24, 36], [])?;
        assert_eq!(parsed.records.len(), 4);
        assert!(parsed.records[0].forced_bind.is_some());
        assert!(parsed.records[0].dispatch.is_some());
        assert!(
            parsed.records[1..]
                .iter()
                .all(|record| record.forced_bind.is_none())
        );
        assert!(matches!(
            parsed.records[2].dispatch,
            Some(Dispatch::Loop { target: 99 })
        ));
        assert!(matches!(
            parsed.records[3].dispatch,
            Some(Dispatch::Rate { rate: 0.5 })
        ));
        assert_eq!(parsed.covered, [0..14, 24..27, 36..39, 44..48]);
        let cooked: Parsed = serde_json::from_value(serde_json::to_value(parsed)?)?;
        assert_eq!(
            serde_json::to_value(cooked.selected(0)?)?,
            serde_json::to_value(selected(&bytes)?)?
        );
        assert!(cooked.selected(24).is_err());

        let mut bytes = [
            row(0, 12),
            row(-2, 0),
            row(0, 30),
            row(-1, 3),
            row(-2, 0),
            row(-2, 255),
            row(-2, 0),
            row(0, 31),
            row(-1, 99),
            row(0, 255),
            row(-2, 0),
        ]
        .concat();
        bytes[116..120].fill(255); // Inactive texture operands are not motion rates.
        let physical = || (0..bytes.len()).step_by(ROW_BYTES);
        let parsed =
            decode_with_records(&bytes, [0], physical(), [0, 24, 24, 84, 108, usize::MAX])?;
        let value = serde_json::to_value(&parsed)?;
        let cooked: Parsed = serde_json::from_value(value)?;
        assert_eq!(
            serde_json::to_value(cooked.selected(24)?)?,
            serde_json::to_value(selected_at(&bytes, 24)?)?
        );
        assert!(matches!(
            cooked.selected(24)?.loop_targets[&3].command,
            AnimationCommand::Play { clip: 255, .. }
        ));
        // The failed loop traversal must not publish its otherwise valid root bind.
        assert!(cooked.selected(84).is_err());
        assert!(cooked.selected(108).is_err());
        assert!(
            parsed
                .unreferenced_storage
                .iter()
                .any(|storage| storage.offset == 113 && storage.bytes.ends_with(&[255; 4]))
        );
        for root in [84, 108, usize::MAX] {
            assert!(decode_with_records(&bytes, [root], physical(), [root]).is_err());
        }
        let pool = [
            row(0, 12),
            row(-2, 0),
            row(-2, 0),
            row(-2, 0),
            row(-2, 0),
            row(0, 31),
            row(-1, 3),
        ]
        .concat();
        let cooked = decode_pool(&pool, [0])?;
        assert!(selected_at(&pool, 5 * ROW_BYTES).is_err());
        assert_eq!(
            serde_json::to_value(cooked.selected_with_continuations(0, &[(2, 4)])?)?,
            serde_json::to_value(selected_with_continuations(&pool, 0, &[(2, 4)])?)?,
        );
        Ok(())
    }
}
