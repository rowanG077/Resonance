//! Must-proven origins of copied 32-bit globals and completed battle results.
//! Values are snapshots, not aliases to the source global. No facts cross an
//! unrelated native/script call, unknown write, conflicting join or task entry.
use std::collections::{BTreeMap, BTreeSet};
use symphonia_script::{scenario::Instruction, semantics};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Origin {
    Global(i64),
    BattleOutcome,
}

type Facts = BTreeMap<i64, Origin>;

#[derive(Clone, Copy, Default)]
struct Value {
    reference: Option<(i64, i64)>,
    origin: Option<Origin>,
}

#[derive(Default)]
struct State {
    values: Vec<Value>,
    memory: Facts,
}

impl State {
    fn store(&mut self, reference: Option<(i64, i64)>, origin: Option<Origin>) {
        let Some((address, width)) = reference else {
            self.memory.clear();
            return;
        };
        self.memory
            .retain(|&at, _| at + 4 <= address || at >= address + width);
        if width == 4
            && let Some(origin) = origin
        {
            self.memory.insert(address, origin);
        }
    }

    fn step(
        &mut self,
        i: &Instruction,
        globals: &BTreeSet<i64>,
        untyped_loads: &BTreeSet<u32>,
    ) -> Option<Origin> {
        match i.mnemonic {
            "load.s8" | "load.s16" | "load.s32" => {
                let address = i.operands[0];
                let width = match i.mnemonic {
                    "load.s8" => 1,
                    "load.s16" => 2,
                    _ => 4,
                };
                let origin = (width == 4 && !untyped_loads.contains(&i.pc))
                    .then(|| {
                        if globals.contains(&address) {
                            Some(Origin::Global(address))
                        } else {
                            self.memory.get(&address).copied()
                        }
                    })
                    .flatten();
                self.values.push(Value {
                    reference: Some((address, width)),
                    origin,
                });
                if !globals.contains(&address) {
                    return origin;
                }
            }
            "push.s8" | "push.s16" | "push.u16" | "push.s32" => {
                self.values.push(Value::default());
            }
            "calc" => {
                let op = i.operands[0] as u8;
                let Some(spec) = semantics::calculator(op) else {
                    *self = Self::default();
                    return None;
                };
                if op == 0 {
                    // Missing values are unknown incoming stack contents. A
                    // completed expression itself performs no memory write.
                    self.values.pop();
                } else if spec.inputs == 1 {
                    let mut value = self.values.pop().unwrap_or_default();
                    if spec.writes_reference {
                        self.store(value.reference, None);
                    }
                    value.origin = None;
                    self.values.push(value);
                } else if spec.inputs == 2 {
                    let right = self.values.pop().unwrap_or_default();
                    let mut left = self.values.pop().unwrap_or_default();
                    if op == 0x10 {
                        self.store(left.reference, right.origin);
                        left.origin = right.origin;
                    } else {
                        if spec.writes_reference {
                            self.store(left.reference, None);
                        }
                        // Even +1 changes the meaning of a stage value. Index
                        // calculations also make the eventual write uncertain.
                        left.origin = None;
                        if op == 0x0f {
                            left.reference = None;
                        }
                    }
                    // The original calculator keeps the left reference even
                    // for ordinary arithmetic; later assignments can use it.
                    self.values.push(left);
                } else {
                    *self = Self::default();
                }
            }
            "arg" | "jump" | "branch_false" | "ret" | "end" => {}
            "proc" if matches!(i.operands[0], 0x0f | 0x37 | 0xcd) => {
                // Original wait command 23 writes the completed battle outcome
                // to shared word 0x24. Other natives remain full barriers, as
                // dialogue/menu completion and arbitrary hosts can reuse it.
                *self = Self::default();
                self.memory.insert(0x24, Origin::BattleOutcome);
                self.values.push(Value {
                    origin: Some(Origin::BattleOutcome),
                    ..Value::default()
                });
            }
            // Natives may yield or change shared memory, even when declared as
            // queries. Script calls and unknown instructions are barriers too.
            _ => *self = Self::default(),
        }
        None
    }
}

struct Block<'a> {
    code: Vec<&'a Instruction>,
    successors: Vec<usize>,
}

pub(super) fn analyze(
    code: &BTreeMap<usize, &Instruction>,
    labels: &BTreeMap<usize, String>,
    tasks: &BTreeSet<usize>,
    globals: &BTreeSet<i64>,
    untyped_loads: &BTreeSet<u32>,
) -> BTreeMap<u32, Origin> {
    let mut leaders: BTreeSet<_> = labels
        .keys()
        .filter(|at| code.contains_key(at))
        .copied()
        .collect();
    let mut previous_end = None;
    for (&at, i) in code {
        if previous_end != Some(at) {
            leaders.insert(at);
        }
        let next = at + i.size_words as usize * 2;
        previous_end = Some(next);
        if matches!(i.mnemonic, "jump" | "branch_false" | "call" | "ret" | "end")
            && code.contains_key(&next)
        {
            leaders.insert(next);
        }
    }
    let starts: Vec<_> = leaders.into_iter().collect();
    let indices: BTreeMap<_, _> = starts.iter().enumerate().map(|(n, &at)| (at, n)).collect();
    let mut blocks = Vec::with_capacity(starts.len());
    for (index, &start) in starts.iter().enumerate() {
        let limit = starts.get(index + 1).copied().unwrap_or(usize::MAX);
        let instructions: Vec<_> = code.range(start..limit).map(|(_, &i)| i).collect();
        let last = instructions.last().unwrap();
        let next = last.offset + last.size_words as usize * 2;
        let mut successors = BTreeSet::new();
        if matches!(last.mnemonic, "jump" | "branch_false") {
            let base = last.offset - last.pc as usize * 2;
            let target = base + last.operands[0] as usize * 2;
            if let Some(&index) = indices.get(&target) {
                successors.insert(index);
            }
        }
        if !matches!(last.mnemonic, "jump" | "ret" | "end")
            && let Some(&index) = indices.get(&next)
        {
            successors.insert(index);
        }
        blocks.push(Block {
            code: instructions,
            successors: successors.into_iter().collect(),
        });
    }
    let mut incoming = vec![0; blocks.len()];
    for block in &blocks {
        for &next in &block.successors {
            incoming[next] += 1;
        }
    }
    let roots: BTreeSet<_> = starts
        .iter()
        .enumerate()
        .filter_map(|(index, at)| (incoming[index] == 0 || tasks.contains(at)).then_some(index))
        .collect();
    let mut entries: Vec<Option<Facts>> = vec![None; blocks.len()];
    let mut pending = roots.clone();
    for &index in &roots {
        entries[index] = Some(Facts::new());
    }
    while let Some(index) = pending.pop_first() {
        let mut state = State {
            memory: entries[index].clone().unwrap(),
            ..State::default()
        };
        for i in &blocks[index].code {
            state.step(i, globals, untyped_loads);
        }
        for &next in &blocks[index].successors {
            if roots.contains(&next) {
                continue;
            }
            let changed = if let Some(old) = &mut entries[next] {
                let before = old.len();
                old.retain(|address, origin| state.memory.get(address) == Some(origin));
                old.len() != before
            } else {
                entries[next] = Some(state.memory.clone());
                true
            };
            if changed {
                pending.insert(next);
            }
        }
    }
    // Record only the final fixed point, never optimistic intermediate visits.
    // Unreachable cycles start each block with no facts, so their hypothetical
    // incoming stack/memory cannot manufacture a type for a retained snapshot.
    let mut origins = BTreeMap::new();
    for (block, entry) in blocks.iter().zip(entries) {
        let mut state = State {
            memory: entry.unwrap_or_default(),
            ..State::default()
        };
        for i in &block.code {
            if let Some(origin) = state.step(i, globals, untyped_loads) {
                origins.insert(i.pc, origin);
            }
        }
    }
    origins
}
