//! Packed source sound IDs become ordinary shared cue IDs in the cooked code.
//! Pure expressions are replayed with ordinary calculator operations; no VM
//! instructions, scratch globals or edition-dependent native handlers are needed.
use anyhow::{Context, Result, ensure};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
use symphonia_script::{NativeCall, Program, scenario};

/// Decode once while collecting a scene's audio dependencies.
pub(crate) struct Script {
    program: Program,
    analysis: scenario::Analysis,
}

impl Script {
    pub fn read(bytes: &[u8]) -> Result<Self> {
        Ok(Self {
            program: Program::decode(bytes)?,
            analysis: scenario::analyze(bytes)?,
        })
    }

    pub fn arguments(
        &self,
        call: NativeCall,
        arity: usize,
        position: usize,
    ) -> Result<Vec<Option<i32>>> {
        Ok(
            scenario::calls::analyzed_arguments(&self.program, &self.analysis, call, arity)?
                .into_values()
                .map(|args| args[position])
                .collect(),
        )
    }

    /// Publish rejected native selectors as explicit silence, then select cues.
    /// The caller can refresh dynamic selections after discovering further aliases.
    pub fn effects(
        &self,
        source: &crate::source::Source,
        library: &Path,
        available: &mut BTreeSet<u32>,
        selected: &mut BTreeSet<u32>,
    ) -> Result<bool> {
        let mut dynamic = false;
        for (call, arity, position) in CALLS {
            let values = self.arguments(call, arity, position)?;
            dynamic |= values.iter().any(Option::is_none);
            for &id in values.iter().flatten().filter(|&&id| id >= 0) {
                if !available.contains(&(id as u32)) {
                    crate::audio_effects::bind_unused(source, library, id as u32)
                        .with_context(|| format!("{call:?}"))?;
                    available.insert(id as u32);
                }
            }
            selected.extend(select(values, available)?);
        }
        Ok(dynamic)
    }
}

pub(crate) fn select(
    values: impl IntoIterator<Item = Option<i32>>,
    available: &BTreeSet<u32>,
) -> Result<BTreeSet<u32>> {
    let mut selected = BTreeSet::new();
    for value in values {
        match value {
            Some(id) if id >= 0 => {
                ensure!(available.contains(&(id as u32)), "unmapped audio ID {id}");
                selected.insert(id as u32);
            }
            Some(_) => (), // Stop/pause/resume sentinels do not select recordings.
            None => selected.extend(available),
        }
    }
    Ok(selected)
}

pub(crate) fn message_voices(
    messages: &[symphonia_script::message::Message],
) -> impl Iterator<Item = Option<i32>> + '_ {
    messages
        .iter()
        .flat_map(|message| &message.tokens)
        .filter_map(|token| match token {
            symphonia_script::message::Token::Control {
                opcode: 9,
                expression,
            } => Some(expression.constant()),
            _ => None,
        })
}

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
    for call in crate::script::arguments::read(bytes, &program, &analysis) {
        let Some(&(_, _, position)) = CALLS
            .iter()
            .find(|(native, _, _)| *native as u8 == call.opcode)
        else {
            continue;
        };
        let arguments = call.arguments.context("sound arguments cross a branch")?;
        let argument = arguments[position].pc;
        let origin = &arguments[position].expression;
        let (pc, call) = (call.pc, call.opcode);
        let constant = known[&call][&pc][position];
        let original = if let Some(value) = constant {
            literal(value)
        } else {
            let origin = origin
                .as_ref()
                .with_context(|| format!("sound ID at PC {pc:#x} is not a pure expression"))?;
            origin.words.clone()
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
        if let Some(previous) = replacements.insert(argument, code.clone()) {
            ensure!(previous == code, "sound argument has conflicting owners");
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
