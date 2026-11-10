//! Recognize complete, straight-line clears of reviewed reusable flag banks.
//! Slots describe storage, without assigning a quest meaning to each bit.
use super::Definitions;
use std::collections::BTreeMap;
use symphonia_script::{scenario::Instruction, semantics::NativeRegistry};

#[derive(Clone, Copy)]
pub(super) struct Slot {
    pub base: i32,
    pub index: i32,
}

pub(super) fn analyze(
    code: &BTreeMap<usize, &Instruction>,
    labels: &BTreeMap<usize, String>,
    registry: &NativeRegistry,
    definitions: &Definitions,
) -> BTreeMap<u32, Slot> {
    let mut result = BTreeMap::new();
    let Some(clear) = registry.get_by_name("clear_event_bit") else {
        return result;
    };
    for (&start, first) in code {
        if !matches!(first.mnemonic, "push.s16" | "push.u16" | "push.s32") {
            continue;
        }
        let base = first.operands[0] as i32;
        // World entry documents these three 50-bit reusable banks. A declared
        // family such as Darkness's five Shadows does not establish this shape.
        if !matches!(base, 150 | 1700 | 1800)
            || definitions.name("story::StoryFlagBase", base).is_none()
        {
            continue;
        }
        let Some(calls) = sequence(start, base, clear.opcode, code, labels) else {
            continue;
        };
        for (index, pc) in calls.into_iter().enumerate() {
            result.insert(
                pc,
                Slot {
                    base,
                    index: index as i32,
                },
            );
        }
    }
    result
}

fn sequence(
    start: usize,
    base: i32,
    opcode: u8,
    code: &BTreeMap<usize, &Instruction>,
    labels: &BTreeMap<usize, String>,
) -> Option<Vec<u32>> {
    let mut offset = start;
    let mut calls = Vec::with_capacity(50);
    for slot in 0..50 {
        for part in 0..4 {
            if offset != start && labels.contains_key(&offset) {
                return None;
            }
            let i = code.get(&offset)?;
            let matches = !i.raw
                && match part {
                    0 => {
                        matches!(i.mnemonic, "push.s16" | "push.u16" | "push.s32")
                            && i.operands[0] as i32 == base + slot
                    }
                    1 => i.mnemonic == "calc" && i.operands[0] == 0,
                    2 => i.mnemonic == "arg",
                    _ => i.mnemonic == "proc" && i.operands[0] == i64::from(opcode),
                };
            if !matches {
                return None;
            }
            offset += i.size_words as usize * 2;
            if part == 3 {
                calls.push(i.pc);
            }
        }
    }
    Some(calls)
}
