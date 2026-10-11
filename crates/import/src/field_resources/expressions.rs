//! Block-local expression values; no initial values are assumed for event memory.
use std::collections::{BTreeMap, BTreeSet};
use symphonia_script::{
    NativeCall, Op, Program, Width, scenario,
    semantics::{self, NativeRegistry},
};

#[derive(Clone, Copy, Default)]
enum Value {
    Constant(i32),
    #[default]
    Unknown,
}

impl Value {
    fn number(self) -> Option<i32> {
        match self {
            Self::Constant(value) => Some(value),
            Self::Unknown => None,
        }
    }

    fn unary(self, op: u8) -> Self {
        let Self::Constant(value) = self else {
            return Self::Unknown;
        };
        Self::Constant(match op {
            1 | 2 | 6 => value,
            3 => value.wrapping_add(1),
            4 => value.wrapping_sub(1),
            5 => value.wrapping_neg(),
            7 => !value,
            8 => i32::from(value == 0),
            _ => unreachable!(),
        })
    }

    fn binary(self, op: u8, right: Self) -> Self {
        if op == 0x10 {
            return right;
        }
        let (Self::Constant(a), Self::Constant(b)) = (self, right) else {
            return Self::Unknown;
        };
        // Compound assignments calculate the same value, then store through
        // the left reference.
        let op = if (0x11..=0x1a).contains(&op) {
            op + 0x20
        } else {
            op
        };
        Self::Constant(match op {
            0x20 => i32::from(a == b),
            0x21 => i32::from(a != b),
            0x22 => i32::from(a <= b),
            0x23 => i32::from(a >= b),
            0x24 => i32::from(a > b),
            0x25 => i32::from(a < b),
            0x26 => i32::from(a != 0 && b != 0),
            0x27 => i32::from(a != 0 || b != 0),
            0x31 => a.wrapping_add(b),
            0x32 => a.wrapping_sub(b),
            0x33 => a.wrapping_mul(b),
            0x34 | 0x35 if b == 0 => return Self::Unknown,
            0x34 => a.wrapping_div(b),
            0x35 => a.wrapping_rem(b),
            0x36 => a & b,
            0x37 => a | b,
            0x38 => a ^ b,
            // PowerPC shifts use six count bits and saturate beyond 31.
            0x39 => a.checked_shl(b as u32 & 63).unwrap_or(0),
            0x3a => a.checked_shr(b as u32 & 63).unwrap_or(a >> 31),
            _ => unreachable!(),
        })
    }
}

#[derive(Clone, Copy)]
enum Reference {
    Scalar,
    Memory(u16, Width),
    Unknown,
}

#[derive(Clone, Copy)]
struct Slot {
    value: Value,
    reference: Reference,
}

impl Slot {
    // A value arriving from another block may carry an event-memory reference.
    const UNKNOWN: Self = Self {
        value: Value::Unknown,
        reference: Reference::Unknown,
    };
}

#[derive(Clone, Default)]
pub(super) struct Expressions {
    values: Vec<Slot>,
    arguments: Vec<Value>,
    result: Value,
    // Enabled only inside a proved pure lookup. Missing bytes remain unknown.
    memory: Option<BTreeMap<u16, u8>>,
}

impl Expressions {
    fn load(&self, reference: Reference) -> Value {
        let Reference::Memory(offset, width) = reference else {
            return Value::Unknown;
        };
        let read = || {
            let memory = self.memory.as_ref()?;
            let mut value = 0u32;
            for byte in 0..width.bytes() {
                value = (value << 8) | u32::from(*memory.get(&offset.checked_add(byte as u16)?)?);
            }
            Some(match width {
                Width::S8 => i32::from(value as i8),
                Width::S16 => i32::from(value as i16),
                Width::S32 => value as i32,
            })
        };
        read().map_or(Value::Unknown, Value::Constant)
    }

    fn store(&mut self, slot: Slot) {
        let Some(memory) = &mut self.memory else {
            return;
        };
        match slot.reference {
            Reference::Scalar => {}
            Reference::Unknown => memory.clear(),
            Reference::Memory(offset, width) => {
                let bytes = slot.value.number().map(i32::to_be_bytes);
                for byte in 0..width.bytes() {
                    let Some(address) = offset.checked_add(byte as u16) else {
                        memory.clear();
                        return;
                    };
                    if let Some(bytes) = bytes {
                        memory.insert(address, bytes[4 - width.bytes() + byte]);
                    } else {
                        memory.remove(&address);
                    }
                }
            }
        }
    }

    pub(super) fn arguments(&self, count: usize) -> Option<Vec<Option<i32>>> {
        let start = self.arguments.len().checked_sub(count)?;
        Some(self.arguments[start..].iter().map(|v| v.number()).collect())
    }

    pub(super) fn step(&mut self, op: Op, registry: &NativeRegistry) {
        match op {
            Op::Push(value) => self.values.push(Slot {
                value: Value::Constant(value),
                reference: Reference::Scalar,
            }),
            Op::Load(offset, width) => {
                let reference = Reference::Memory(offset, width);
                self.values.push(Slot {
                    value: self.load(reference),
                    reference,
                });
            }
            Op::Argument => self.arguments.push(self.result),
            Op::Calculate(0) => self.result = self.values.pop().unwrap_or(Slot::UNKNOWN).value,
            Op::Calculate(op @ 1..=8) => {
                if let Some(mut slot) = self.values.pop() {
                    if op <= 4 {
                        self.store(Slot {
                            value: slot.value.unary(if op % 2 == 1 { 3 } else { 4 }),
                            ..slot
                        });
                    }
                    slot.value = slot.value.unary(op);
                    self.values.push(slot);
                }
            }
            Op::Calculate(op) => {
                let right = self.values.pop();
                let left = self.values.pop();
                let (Some(mut left), Some(right)) = (left, right) else {
                    self.values.push(Slot::UNKNOWN);
                    return;
                };
                left.value = if op == 0x0f {
                    match left.reference {
                        Reference::Scalar => left.value,
                        Reference::Unknown => Value::Unknown,
                        Reference::Memory(offset, width) => {
                            left.reference = right
                                .value
                                .number()
                                .and_then(|index| {
                                    u16::try_from(
                                        i64::from(offset) + i64::from(index) * width.bytes() as i64,
                                    )
                                    .ok()
                                })
                                .map_or(Reference::Unknown, |offset| {
                                    Reference::Memory(offset, width)
                                });
                            self.load(left.reference)
                        }
                    }
                } else {
                    left.value.binary(op, right.value)
                };
                if (0x10..=0x1a).contains(&op) {
                    self.store(left);
                }
                self.values.push(left);
            }
            Op::Native(op) => {
                if let Some(memory) = &mut self.memory {
                    memory.clear();
                }
                let Some(signature) = registry.get(op) else {
                    *self = Self::default();
                    return;
                };
                self.arguments.truncate(
                    self.arguments
                        .len()
                        .saturating_sub(signature.arguments.len()),
                );
                match signature.returns_value {
                    Some(true) => self.values.push(Slot {
                        value: Value::Unknown,
                        reference: Reference::Scalar,
                    }),
                    Some(false) => {}
                    None => self.values.clear(),
                }
                self.result = Value::Unknown;
            }
            // This analysis only observes decoded legacy programs. Authored
            // operations do not expose legacy stack or memory facts.
            _ => {
                *self = Self::default();
            }
        }
    }
}

struct Flow {
    incoming: BTreeMap<u32, BTreeSet<u32>>,
    entries: BTreeSet<u32>,
}

impl Flow {
    fn read(program: &Program, analysis: &scenario::Analysis) -> Self {
        let mut incoming = BTreeMap::<_, BTreeSet<_>>::new();
        for &pc in analysis.instructions.keys() {
            let (op, next) = program.instruction(pc).unwrap();
            let targets = match op {
                Op::Jump(target) => [Some(target), None],
                Op::Call(target) | Op::BranchFalse(target) => [Some(target), Some(next)],
                Op::Return | Op::End => [None, None],
                _ => [Some(next), None],
            };
            for target in targets.into_iter().flatten() {
                incoming.entry(target).or_default().insert(pc);
            }
        }
        Self {
            incoming,
            entries: analysis
                .records
                .iter()
                .filter(|r| r.active())
                .map(|r| r.pc)
                .chain([program.entry()])
                .collect(),
        }
    }

    fn covers(&self, entry: u32, runs: &[Run]) -> bool {
        let visited: BTreeSet<_> = runs
            .iter()
            .flat_map(|r| r.visited.iter().copied())
            .collect();
        visited.iter().all(|pc| {
            *pc == entry
                || (!self.entries.contains(pc)
                    && self
                        .incoming
                        .get(pc)
                        .is_none_or(|sources| sources.is_subset(&visited)))
        })
    }
}

#[derive(Clone, Copy)]
enum Stop {
    Argument(u32, i32),
    Return(u32),
}

struct Run {
    stop: Stop,
    visited: BTreeSet<u32>,
    expressions: Expressions,
}

/// Execute only a finite, pure lookup continuation. Unknown branches, native
/// effects and cyclic paths cannot establish a resource declaration.
fn run(
    program: &Program,
    registry: &NativeRegistry,
    mut pc: u32,
    mut expressions: Expressions,
    query: Option<(u32, i32)>,
    call: NativeCall,
) -> Option<Run> {
    const LOOKUP_INSTRUCTIONS: usize = 512;
    // SelectPartyMember also copies its return value to this event-memory word.
    const PARTY_QUERY_RESULT: u16 = 0x20;
    let mut visited = BTreeSet::new();
    let stop = loop {
        if !visited.insert(pc) || visited.len() > LOOKUP_INSTRUCTIONS {
            return None;
        }
        let (op, next) = program.instruction(pc)?;
        match op {
            Op::Jump(target) => {
                pc = target;
                continue;
            }
            Op::BranchFalse(target) => {
                pc = if expressions.result.number()? == 0 {
                    target
                } else {
                    next
                };
                continue;
            }
            Op::Call(_) | Op::End => return None,
            Op::Return => {
                if !expressions.values.is_empty() || !expressions.arguments.is_empty() {
                    return None;
                }
                break Stop::Return(pc);
            }
            Op::Native(opcode) => {
                if let Some((query_pc, member)) = query
                    && query_pc == pc
                    && opcode == NativeCall::SelectPartyMember as u8
                    && expressions.arguments(1)? == [Some(-1)]
                {
                    expressions.step(op, registry);
                    expressions.values.last_mut()?.value = Value::Constant(member);
                    expressions.store(Slot {
                        reference: Reference::Memory(PARTY_QUERY_RESULT, Width::S32),
                        value: Value::Constant(member),
                    });
                } else if opcode == call as u8 {
                    break Stop::Argument(pc, expressions.arguments(1)?[0]?);
                } else {
                    return None;
                }
            }
            Op::Calculate(opcode) => {
                if expressions.values.len() < usize::from(semantics::calculator(opcode)?.inputs) {
                    return None;
                }
                expressions.step(op, registry);
            }
            _ => expressions.step(op, registry),
        }
        pc = next;
    };
    Some(Run {
        stop,
        visited,
        expressions,
    })
}

fn argument_values(runs: &[Run]) -> Option<BTreeMap<u32, BTreeSet<i32>>> {
    let mut values = BTreeMap::<_, BTreeSet<_>>::new();
    for run in runs {
        let Stop::Argument(pc, value) = run.stop else {
            return None;
        };
        values.entry(pc).or_default().insert(value);
    }
    Some(values)
}

/// A playable party query has nine possible results. This is the valid party
/// contract, not an assumption about arbitrary or corrupt event memory. Every
/// result must traverse a complete lookup to a call or the same balanced return.
/// No map name, resource ID, local address or lookup output is prescribed.
pub(super) fn finite_arguments(
    program: &Program,
    analysis: &scenario::Analysis,
    call: NativeCall,
) -> BTreeMap<u32, BTreeSet<i32>> {
    let registry = NativeRegistry::gqseaf();
    let flow = Flow::read(program, analysis);
    let mut output = BTreeMap::new();
    for &entry in analysis.instructions.keys() {
        if !matches!(program.instruction(entry), Some((Op::Load(..), _))) {
            continue;
        }
        let mut at = entry;
        let mut setup = [(0, Op::End); 5];
        for instruction in &mut setup {
            let Some((op, next)) = program.instruction(at) else {
                break;
            };
            *instruction = (at, op);
            at = next;
        }
        let [
            (_, Op::Load(..)),
            (_, Op::Push(-1)),
            (_, Op::Calculate(0)),
            (_, Op::Argument),
            (query, Op::Native(op)),
        ] = setup
        else {
            continue;
        };
        if op != NativeCall::SelectPartyMember as u8 {
            continue;
        }
        let runs = (1..=9)
            .map(|member| {
                run(
                    program,
                    &registry,
                    entry,
                    Expressions {
                        memory: Some(BTreeMap::new()),
                        ..Expressions::default()
                    },
                    Some((query, member)),
                    call,
                )
            })
            .collect::<Option<Vec<_>>>();
        let Some(runs) = runs else {
            continue;
        };
        if !flow.covers(entry, &runs) {
            continue;
        }
        if let Some(values) = argument_values(&runs) {
            output.extend(values);
            continue;
        }
        let Stop::Return(return_pc) = runs[0].stop else {
            continue;
        };
        if !runs
            .iter()
            .all(|r| matches!(r.stop, Stop::Return(pc) if pc == return_pc))
        {
            continue;
        }
        // Only actual calls to this pure helper may use its output. An extra
        // branch/event entry at the return continuation would bypass the lookup.
        for &caller in analysis.instructions.keys() {
            let (op, next) = program.instruction(caller).unwrap();
            if op != Op::Call(entry)
                || flow.entries.contains(&next)
                || flow
                    .incoming
                    .get(&next)
                    .is_none_or(|sources| sources.len() != 1 || !sources.contains(&caller))
            {
                continue;
            }
            let callers = runs
                .iter()
                .map(|r| run(program, &registry, next, r.expressions.clone(), None, call))
                .collect::<Option<Vec<_>>>();
            if let Some(callers) = callers
                && flow.covers(next, &callers)
                && let Some(values) = argument_values(&callers)
            {
                output.extend(values);
            }
        }
    }
    output
}
