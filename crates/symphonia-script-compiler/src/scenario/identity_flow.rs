//! Proven constant copies within one straight-line region. This does not infer a
//! permanent type for shared memory or replace a memory read with a constant.
use std::collections::{BTreeMap, BTreeSet};
use symphonia_script::{scenario::Instruction, semantics};

#[derive(Clone, Copy, Default)]
struct Value {
    constant_pc: Option<u32>,
    number: Option<i32>,
    reference: Option<(i64, i64)>,
    copied: bool,
}

#[derive(Default)]
struct State {
    values: Vec<Value>,
    arguments: Vec<Value>,
    expression: Value,
    memory: BTreeMap<i64, Value>,
}
impl State {
    fn store(&mut self, reference: Option<(i64, i64)>, value: Value) {
        let Some((address, width)) = reference else {
            // An indexed/unknown reference can alias any tracked word.
            self.memory.clear();
            return;
        };
        self.memory
            .retain(|&at, _| at + 4 <= address || at >= address + width);
        if width == 4 && value.constant_pc.is_some() {
            self.memory.insert(address, value);
        }
    }
}

pub(super) struct CopyUse {
    pub constant_pc: u32,
    pub value: i32,
    pub use_pc: u32,
    pub address: i64,
    pub native: String,
    pub parameter: String,
    pub domain: String,
}

pub(super) fn analyze(
    code: &BTreeMap<usize, &Instruction>,
    labels: &BTreeMap<usize, String>,
    registry: &semantics::NativeRegistry,
) -> Vec<CopyUse> {
    let mut state = State::default();
    let mut expected_offset = None;
    let mut uses = Vec::new();
    for (&offset, i) in code {
        if expected_offset != Some(offset) || labels.contains_key(&offset) {
            state = State::default();
        }
        expected_offset = Some(offset + i.size_words as usize * 2);
        match i.mnemonic {
            "push.s8" | "push.s16" | "push.u16" | "push.s32" => {
                state.values.push(Value {
                    constant_pc: Some(i.pc),
                    number: Some(i.operands[0] as i32),
                    ..Value::default()
                });
            }
            "load.s8" | "load.s16" | "load.s32" => {
                let address = i.operands[0];
                let width = match i.mnemonic {
                    "load.s8" => 1,
                    "load.s16" => 2,
                    _ => 4,
                };
                let mut value = if width == 4 {
                    state.memory.get(&address).copied().unwrap_or_default()
                } else {
                    Value::default()
                };
                value.reference = Some((address, width));
                value.copied = value.constant_pc.is_some();
                state.values.push(value);
            }
            "calc" => {
                let op = i.operands[0] as u8;
                let Some(spec) = semantics::calculator(op) else {
                    state = State::default();
                    continue;
                };
                if op == 0 {
                    if let Some(value) = state.values.pop() {
                        state.expression = value;
                    } else {
                        state = State::default();
                    }
                } else if spec.inputs == 1 {
                    let Some(mut value) = state.values.pop() else {
                        state = State::default();
                        continue;
                    };
                    value.number = value
                        .number
                        .filter(|_| value.reference.is_none())
                        .and_then(|n| super::constant::unary(op, n));
                    value.constant_pc = value.number.map(|_| i.pc);
                    if spec.writes_reference {
                        state.store(value.reference, value);
                    }
                    state.values.push(value);
                } else if spec.inputs == 2 {
                    let (Some(right), Some(mut left)) = (state.values.pop(), state.values.pop())
                    else {
                        state = State::default();
                        continue;
                    };
                    if op == 0x10 {
                        state.store(left.reference, right);
                        left.constant_pc = right.constant_pc;
                        left.number = right.number;
                        left.copied = right.copied;
                    } else {
                        let pure = left.reference.is_none() && right.reference.is_none();
                        left.number = left
                            .number
                            .zip(right.number)
                            .filter(|_| pure)
                            .and_then(|(left, right)| super::constant::binary(op, left, right));
                        left.constant_pc = left.number.map(|_| i.pc);
                        if spec.writes_reference {
                            state.store(left.reference, left);
                        }
                        if op == 0x0f {
                            left.reference = None;
                        }
                    }
                    state.values.push(left);
                } else {
                    state = State::default();
                }
            }
            "arg" => state.arguments.push(state.expression),
            "proc" => {
                let Some(native) = registry.get(i.operands[0] as u8) else {
                    state = State::default();
                    continue;
                };
                let Some(at) = state.arguments.len().checked_sub(native.arguments.len()) else {
                    state = State::default();
                    continue;
                };
                let arguments = state.arguments.split_off(at);
                for (index, value) in arguments.iter().enumerate() {
                    // These ABI domains are unconditional. Selector-dependent
                    // actor/resource domains require a broader value analysis.
                    let Some(domain) = native
                        .argument_types
                        .get(index)
                        .filter(|t| matches!(t.as_str(), "items::ItemId" | "story::StoryFlag"))
                    else {
                        continue;
                    };
                    if value.copied
                        && let (Some(constant_pc), Some(number), Some((address, 4))) =
                            (value.constant_pc, value.number, value.reference)
                    {
                        uses.push(CopyUse {
                            constant_pc,
                            value: number,
                            use_pc: i.pc,
                            address,
                            native: native.name.clone(),
                            parameter: native.arguments[index].clone(),
                            domain: domain.clone(),
                        });
                    }
                }
                // Even a query could call into a host implementation that writes
                // shared memory. No inferred memory value survives any native.
                state.memory.clear();
                state.expression = Value::default();
                if native.returns_value.is_none() || native.yields || native.control_flow {
                    state = State::default();
                } else if native.returns_value == Some(true) {
                    state.values.push(Value::default());
                }
            }
            // Calls, joins, branches, raw instructions and region boundaries
            // never carry inferred memory identities into another region.
            _ => state = State::default(),
        }
    }
    let mut domains = BTreeMap::<u32, BTreeSet<&str>>::new();
    for usage in &uses {
        domains
            .entry(usage.constant_pc)
            .or_default()
            .insert(&usage.domain);
    }
    let unambiguous: BTreeSet<_> = domains
        .into_iter()
        .filter_map(|(pc, domains)| (domains.len() == 1).then_some(pc))
        .collect();
    uses.retain(|usage| unambiguous.contains(&usage.constant_pc));
    uses
}
