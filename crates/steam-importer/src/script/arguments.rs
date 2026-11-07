//! Argument ownership and safe expression replay shared by scenario rewrites.
use symphonia_script::{Op, Program, scenario::Analysis, semantics::NativeRegistry};

#[derive(Clone)]
pub(crate) struct Expression {
    pub words: Vec<u16>,
    end: u32,
    memory: bool,
}

#[derive(Clone)]
pub(crate) struct Argument {
    pub pc: u32,
    pub expression: Option<Expression>,
    pub end: Option<u32>,
}

pub(crate) struct Call {
    pub pc: u32,
    pub opcode: u8,
    /// None means these arguments cannot be resolved within one basic block.
    pub arguments: Option<Vec<Argument>>,
}

fn invalidate_memory(values: &mut [Option<Expression>]) {
    for value in values {
        if value.as_ref().is_some_and(|value| value.memory) {
            *value = None;
        }
    }
}

pub(crate) fn read(bytes: &[u8], program: &Program, analysis: &Analysis) -> Vec<Call> {
    let registry = NativeRegistry::gqseaf();
    let words = |start: u32, end: u32| {
        let base = analysis.header.code_base();
        bytes[base + start as usize * 2..base + end as usize * 2]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|word| u16::from_be_bytes(*word))
            .collect::<Vec<_>>()
    };
    let mut calls = Vec::new();
    for block in analysis.basic_blocks() {
        let mut values: Vec<Option<Expression>> = Vec::new();
        let mut expression = None;
        let mut expression_end = None;
        let mut arguments = Vec::new();
        for pc in block.instruction_pcs {
            let (op, next) = program.instruction(pc).unwrap();
            match op {
                Op::Push(_) | Op::Load(_, _) => values.push(Some(Expression {
                    words: words(pc, next),
                    end: next,
                    memory: matches!(op, Op::Load(_, _)),
                })),
                Op::Calculate(0) => {
                    expression = values.pop().flatten();
                    expression_end = Some(pc);
                }
                Op::Calculate(op @ 1..=8) => {
                    if op <= 4 {
                        invalidate_memory(&mut values);
                    }
                    let value = values
                        .pop()
                        .flatten()
                        .filter(|value| op >= 5 && value.end == pc)
                        .map(|mut value| {
                            value.words.extend(words(pc, next));
                            value.end = next;
                            value
                        });
                    values.push(value);
                }
                Op::Calculate(op) => {
                    if (0x10..=0x1a).contains(&op) {
                        invalidate_memory(&mut values);
                    }
                    let right = values.pop().flatten();
                    let left = values.pop().flatten();
                    let pure =
                        op == 0x0f || (0x20..=0x27).contains(&op) || (0x31..=0x3a).contains(&op);
                    values.push(
                        left.zip(right)
                            .filter(|(_, right)| pure && right.end == pc)
                            .map(|(mut left, right)| {
                                left.words.extend(right.words);
                                left.words.extend(words(pc, next));
                                left.end = next;
                                left.memory |= right.memory;
                                left
                            }),
                    );
                }
                Op::Argument => arguments.push(Argument {
                    pc,
                    expression: expression.clone(),
                    end: expression_end,
                }),
                Op::Native(opcode) => {
                    if let Some(signature) = registry.get(opcode) {
                        let count = signature.arguments.len();
                        let captured = if let Some(start) = arguments.len().checked_sub(count) {
                            Some(arguments.split_off(start))
                        } else {
                            arguments.clear();
                            None
                        };
                        calls.push(Call {
                            pc,
                            opcode,
                            arguments: captured,
                        });
                        // Reuse a native's scalar return without executing it twice.
                        // A later native invalidates memory-dependent values.
                        invalidate_memory(&mut values);
                        match signature.returns_value {
                            Some(true) => values.push(Some(Expression {
                                words: vec![0x1200, 0x20],
                                end: next,
                                memory: true,
                            })),
                            Some(false) => (),
                            None => values.clear(),
                        }
                    } else {
                        values.clear();
                        arguments.clear();
                    }
                    expression = None;
                    expression_end = None;
                }
                _ => {
                    values.clear();
                    arguments.clear();
                    expression = None;
                    expression_end = None;
                }
            }
        }
    }
    calls
}
