//! Packed source sound IDs become ordinary shared cue IDs in the cooked code.
//! Pure expressions are replayed with ordinary calculator operations; no VM
//! instructions, scratch globals or edition-dependent native handlers are needed.
use anyhow::{Context, Result, ensure};
use std::{collections::BTreeMap, ops::Range};
use symphonia_script::{NativeCall, Op, Program, scenario, semantics::NativeRegistry};

pub(crate) const CALLS: [(NativeCall, usize, usize); 4] = [
    (NativeCall::PlaySoundSimple, 2, 0),
    (NativeCall::PlaySound, 4, 0),
    (NativeCall::PlayActorSound, 4, 1),
    (NativeCall::SetActorAmbientSound, 4, 1),
];

pub(crate) fn normalize(bytes: &[u8]) -> Result<Vec<u8>> {
    let analysis = scenario::analyze(bytes)?;
    if !analysis.instructions.values().any(|i| {
        i.mnemonic == "proc"
            && CALLS
                .iter()
                .any(|(call, _, _)| i.operands == [i64::from(*call as u8)])
    }) {
        return Ok(bytes.to_vec());
    }
    let program = Program::decode(bytes)?;
    let registry = NativeRegistry::gqseaf();
    let known = CALLS
        .iter()
        .map(|&(call, arity, _)| {
            Ok((
                call as u8,
                scenario::calls::analyzed_arguments(&program, &analysis, call, arity)?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut replacements = BTreeMap::new();
    for block in analysis.basic_blocks() {
        let mut values: Vec<Option<Range<u32>>> = Vec::new();
        let mut expression = None;
        let mut arguments = Vec::new();
        for pc in block.instruction_pcs {
            let (op, next) = program.instruction(pc).unwrap();
            match op {
                Op::Push(_) | Op::Load(_, _) => values.push(Some(pc..next)),
                Op::Calculate(0) => expression = values.pop().flatten(),
                Op::Calculate(op @ 1..=8) => {
                    let span = values
                        .pop()
                        .flatten()
                        .filter(|span| op >= 5 && span.end == pc)
                        .map(|span| span.start..next);
                    values.push(span);
                }
                Op::Calculate(op) => {
                    let right = values.pop().flatten();
                    let left = values.pop().flatten();
                    let pure =
                        op == 0x0f || (0x20..=0x27).contains(&op) || (0x31..=0x3a).contains(&op);
                    values.push(
                        left.zip(right)
                            .filter(|(left, right)| {
                                pure && left.end == right.start && right.end == pc
                            })
                            .map(|(left, _)| left.start..next),
                    );
                }
                Op::Argument => arguments.push((pc, expression.clone())),
                Op::Native(call) => {
                    let Some(signature) = registry.get(call) else {
                        values.clear();
                        arguments.clear();
                        expression = None;
                        continue;
                    };
                    if let Some(&(_, arity, position)) =
                        CALLS.iter().find(|(native, _, _)| *native as u8 == call)
                    {
                        let start = arguments
                            .len()
                            .checked_sub(arity)
                            .context("sound arguments cross a branch")?;
                        let (argument, origin) = &arguments[start + position];
                        let constant = known[&call][&pc][position];
                        let original = if let Some(value) = constant {
                            literal(value)
                        } else {
                            let origin = origin.as_ref().with_context(|| {
                                format!("sound ID at PC {pc:#x} is not a pure expression")
                            })?;
                            let base = analysis.header.code_base();
                            bytes[base + origin.start as usize * 2..base + origin.end as usize * 2]
                                .as_chunks::<2>()
                                .0
                                .iter()
                                .map(|word| u16::from_be_bytes(*word))
                                .collect()
                        };
                        let mut code = Vec::new();
                        if let Some(value) = constant {
                            let mapped = if value as u16 == u16::MAX {
                                -1
                            } else {
                                i32::from(crate::audio_effects::id(value as u16)?)
                            };
                            code.extend(literal(mapped));
                        } else {
                            // The native consumer uses the low unsigned word,
                            // even when an S16 script load sign-extends a stream
                            // selector. For u = x & FFFF, lower to
                            // u + 512 - (u >= 8000)*1200 - (u == FFFF)*F000.
                            // This retains the native FFFF stop sentinel as -1.
                            code.extend_from_slice(&original);
                            code.extend(literal(0xffff));
                            code.push(0x3036);
                            code.extend(literal(512));
                            code.push(0x3031);
                            code.extend_from_slice(&original);
                            code.extend(literal(0xffff));
                            code.push(0x3036);
                            code.extend(literal(0x8000));
                            code.push(0x3023);
                            code.extend(literal(0x1200));
                            code.extend([0x3033, 0x3032]);
                            code.extend_from_slice(&original);
                            code.extend(literal(0xffff));
                            code.push(0x3036);
                            code.extend(literal(0xffff));
                            code.push(0x3020);
                            code.extend(literal(0xf000));
                            code.extend([0x3033, 0x3032]);
                        }
                        code.extend([0x3000, 0x4000]);
                        // Later arguments and the caller see the original expression.
                        code.extend(original);
                        code.push(0x3000);
                        if let Some(previous) = replacements.insert(*argument, code.clone()) {
                            ensure!(previous == code, "sound argument has conflicting owners");
                        }
                    }
                    arguments.truncate(arguments.len().saturating_sub(signature.arguments.len()));
                    match signature.returns_value {
                        Some(true) => values.push(None),
                        Some(false) => (),
                        None => values.clear(),
                    }
                    expression = None;
                }
                _ => {
                    values.clear();
                    arguments.clear();
                    expression = None;
                }
            }
        }
    }
    crate::script::rewrite(bytes, &analysis, replacements)
}

fn literal(value: i32) -> Vec<u16> {
    if (-128..=127).contains(&value) {
        vec![u16::from(value as u8)]
    } else if (0..=65535).contains(&value) {
        vec![0x0300, value as u16]
    } else {
        vec![0x0200, value as u16, (value >> 16) as u16]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use symphonia_script::Width;
    use symphonia_script_vm::{Host, Memory, NativeBindings, NativeResult, RunEvent, Vm};

    #[derive(Default)]
    struct Sounds(Vec<i32>);
    impl Host for Sounds {
        const NATIVES: NativeBindings<Self> = NativeBindings::<Self>::new()
            .register(
                NativeCall::PlaySoundSimple as u8,
                2,
                false,
                |host, args, _| {
                    host.0 = args.to_vec();
                    Ok(NativeResult::Continue(None))
                },
            )
            .register(NativeCall::PlaySound as u8, 4, false, |host, args, _| {
                host.0 = args.to_vec();
                Ok(NativeResult::Continue(None))
            })
            .register(
                NativeCall::PlayActorSound as u8,
                4,
                false,
                |host, args, _| {
                    host.0 = args.to_vec();
                    Ok(NativeResult::Continue(None))
                },
            )
            .register(
                NativeCall::SetActorAmbientSound as u8,
                4,
                false,
                |host, args, _| {
                    host.0 = args.to_vec();
                    Ok(NativeResult::Continue(None))
                },
            );
    }

    #[test]
    fn sounds_preserve_caller_state_and_reused_arguments_without_vm_extensions() -> Result<()> {
        for (call, arity, position) in CALLS {
            for dynamic in [false, true] {
                for id in [-1, 0xffff, 0, 28, 0x123, 0x8001, -32767, 0x1001c] {
                    let mut code = literal(321);
                    code.extend([0x3000, 0x4000, 0x1200, 0x110]); // Outer argument and value.
                    let mut expected = Vec::new();
                    for argument in 0..arity {
                        if argument == position {
                            if dynamic {
                                // Indexed source memory is pure, while still depending on
                                // the actual event state rather than an importer constant.
                                code.extend([0x1200, 0x100, 0x1200, 0x108, 0x300f]);
                            } else {
                                code.extend(literal(id));
                            }
                            code.push(0x3000);
                        } else if argument < position {
                            code.extend([42, 0x3000]);
                        }
                        // Later parameters deliberately reuse the original expression.
                        code.push(0x4000);
                        expected.push(if argument < position { 42 } else { id });
                    }
                    expected[position] = if id as u16 == u16::MAX {
                        -1
                    } else {
                        i32::from(crate::audio_effects::id(id as u16)?)
                    };
                    code.extend([0x2000 | call as u16, 0x20ff]);
                    let mut bytes = [4u16, 0, 0, 0]
                        .into_iter()
                        .flat_map(u16::to_be_bytes)
                        .collect::<Vec<_>>();
                    bytes.extend(code.into_iter().flat_map(u16::to_be_bytes));
                    let cooked = normalize(&bytes)?;
                    let program = Arc::new(Program::decode(&cooked)?);
                    let mut vm = Vm::new(program.clone(), program.entry())?;
                    let mut memory = Memory::default();
                    for (at, value) in [
                        (0x20, 777),
                        (0x100, 999),
                        (0x104, id),
                        (0x108, 1),
                        (0x110, 1234),
                    ] {
                        memory.write(at, Width::S32, value)?;
                    }
                    let mut host = Sounds::default();
                    assert_eq!(
                        vm.run(&mut host, &mut memory, 1000)?.event,
                        RunEvent::Halted
                    );
                    assert_eq!(host.0, expected, "{call:?}, {id:#x}, dynamic={dynamic}");
                    assert_eq!((vm.value_depth(), vm.argument_depth()), (1, 1));
                    assert_eq!(vm.expression(), Some(id));
                    assert_eq!(memory.read(0x20, Width::S32)?, 777);
                    assert_eq!(memory.read(0x104, Width::S32)?, id);
                    assert_eq!(memory.read(0x110, Width::S32)?, 1234);
                }
            }
        }
        Ok(())
    }
}
