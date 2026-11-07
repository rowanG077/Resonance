//! Adapt source input masks using ordinary GC bytecode and its existing native.
//! The engine continues to sample the same logical GameCube actions for both cooks.
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;
use symphonia_script::{NativeCall, Op, Program, scenario, semantics::NativeRegistry};

const RESULT: u16 = 0x20;
// Native variable storage occupies 0..0x2000; +0x2000 is the message buffer.
// The native skit/field owner copies exactly that variable region (5551ba,
// 4c44e0). Reserve GC-only transient storage outside it. Skit memory begins at
// context zero because the existing variable copy excludes this byte.
const SOURCE_VARIABLE_BYTES: u32 = 0x2000;
const INPUT_CONTEXT: u16 = symphonia_script::COMPILER_TRANSIENT_BYTE;
const MAPPED: &[(u16, u32)] = &[
    (1, 4),
    (2, 8),
    (4, 2),
    (8, 1),
    (0x10, 0x10000),
    (0x40, 0x40000),
    (0x100, 0x20),
    (0x200, 0x80),
    (0x1000, 0x20000),
];
const RAW: &[(u16, u32)] = &[
    (1, 4),
    (2, 8),
    (4, 2),
    (8, 1),
    (0x10, 0x20),
    (0x20, 0x200),
    (0x40, 0x100),
    (0x100, 0x1000),
    (0x200, 0x2000),
    (0x400, 0x4000),
    (0x800, 0x8000),
    (0x1000, 0x10),
];

pub(super) fn normalize(bytes: &[u8]) -> Result<Vec<u8>> {
    let coordinates = crate::script_actor_coordinates::normalize(bytes)?;
    let bytes = coordinates.as_slice();
    let analysis = scenario::analyze(bytes)?;
    let calls = analysis
        .instructions
        .values()
        .filter(|i| {
            i.mnemonic == "proc" && i.operands == [i64::from(NativeCall::ReadMappedInput as u8)]
        })
        .collect::<Vec<_>>();
    let mut replacements = BTreeMap::new();
    let program = Program::decode(bytes)?;
    validate_context_reservation(&program, &analysis)?;
    // Instrument the argument's existing scalar, before expression-end consumes
    // it. Assignment * 0 + the original scalar preserves its reference, number,
    // stack depth, expression register and pending outer native arguments.
    for call in crate::script::arguments::read(bytes, &program, &analysis) {
        let opcode = call.opcode;
        if !matches!(opcode, 0x39 | 0x3a | 0xb8) {
            continue;
        }
        let arguments = call
            .arguments
            .context("input lifecycle arguments cross a branch")?;
        let argument = if opcode == NativeCall::ReadMappedInput as u8 {
            &arguments[0]
        } else {
            arguments.last().unwrap()
        };
        let producer = argument.end.context("input lifecycle has no expression")?;
        ensure!(
            program.instruction(producer) == Some((Op::Calculate(0), argument.pc)),
            "input lifecycle expression is not adjacent to its argument"
        );
        if opcode == NativeCall::ReadMappedInput as u8 {
            // Keep the source scalar on the value stack while passing zero.
            replacements.insert(producer, vec![0, 0x3000, 0x4000, 0x3000]);
            replacements.insert(argument.pc, vec![]);
        } else {
            let mut code = Code::default();
            code.set_context(if opcode == NativeCall::SpawnEvent as u8 {
                5
            } else {
                0
            });
            code.words.push(0x3000);
            replacements.insert(producer, code.words);
        }
    }
    if calls.is_empty() {
        return crate::script::rewrite(bytes, &analysis, replacements);
    }
    let mut raw = BTreeMap::new();
    let mut replace = BTreeMap::new();
    let mut restore = BTreeMap::new();
    let mut inferred = None;
    for call in calls {
        // Installed calls use constant modes, sometimes composed with a pause
        // flag. Resolve that expression instead of guessing at nearby constants.
        let previous = analysis
            .instructions
            .range(..call.pc)
            .rev()
            .take(3)
            .map(|(_, i)| i)
            .collect::<Vec<_>>();
        let literal = (previous.len() == 3
            && previous[0].mnemonic == "arg"
            && previous[0].end_pc() == call.pc
            && previous[1].mnemonic == "calc"
            && previous[1].operands == [0]
            && previous[1].end_pc() == previous[0].pc
            && previous[2].end_pc() == previous[1].pc
            && matches!(
                previous[2].mnemonic,
                "push.s8" | "push.s16" | "push.u16" | "push.s32"
            ))
        .then(|| previous[2]);
        let mode = if let Some(producer) = literal {
            i32::try_from(producer.operands[0])?
        } else {
            if inferred.is_none() {
                inferred = Some(scenario::calls::analyzed_arguments(
                    &Program::decode(bytes)?,
                    &analysis,
                    NativeCall::ReadMappedInput,
                    2,
                )?);
            }
            inferred
                .as_ref()
                .unwrap()
                .get(&call.pc)
                .and_then(|args| args[1])
                .with_context(|| format!("input mode at PC {:#x} is dynamic", call.pc))?
        };
        ensure!(
            matches!(mode, 0 | 1 | 2 | 0x8000 | 0x8001),
            "unknown input mode {mode:#x}"
        );
        let raw_pressed = mode == 2;
        raw.insert(call.pc, raw_pressed);
        if raw_pressed {
            let producer = literal.context("raw input mode needs a direct literal")?;
            // Steam mode 2 means raw presses; GC mode 1 means presses. Retain
            // the source expression register after passing the changed argument.
            replace.insert(producer.pc, producer.size_words);
            restore.insert(previous[0].end_pc(), mode);
        }
    }
    for (pc, width) in replace {
        let instruction = &analysis.instructions[&pc];
        let word =
            u16::from_be_bytes(bytes[instruction.offset..instruction.offset + 2].try_into()?);
        replacements.insert(
            pc,
            match width {
                1 => vec![1],
                2 => vec![word, 1],
                3 => vec![word, 1, 0],
                _ => unreachable!(),
            },
        );
    }
    for (pc, is_raw) in raw {
        let mut code = Code::default();
        if let Some(mode) = restore.remove(&pc) {
            code.push(mode as u32);
            code.words.push(0x3000);
        }
        code.words.extend([
            0,
            0x1200,
            RESULT,
            0x2000 | NativeCall::ReadMappedInput as u16,
        ]);
        code.mask(if is_raw { RAW } else { MAPPED });
        if !is_raw {
            // Only these three physical controls depend on native global context.
            // a0 is ignored by the original input service; it is not a context ID.
            for (from, to, contexts, invert) in [
                (0x400, 0x10, &[1, 3, 5][..], true),
                (0x400, 0x40, &[1, 3, 5][..], false),
                (0x800, 0x40, &[1, 3, 5][..], true),
                (0x800, 0x10, &[5][..], false),
                (0x20, 0x200000, &[1, 3, 5][..], false),
            ] {
                code.bit(from, to);
                code.context(contexts);
                if invert {
                    code.words.push(0x3008);
                }
                code.words.extend([0x3033, 0x3037]);
            }
        }
        code.finish();
        replacements.insert(pc, code.words);
    }
    ensure!(
        restore.is_empty(),
        "raw input argument is not adjacent to its call"
    );
    crate::script::rewrite(bytes, &analysis, replacements)
}

// The native owner copies variable bytes 0..0x2000, then uses +0x2000 for
// message expansion, +0x2160 for event records and +0x581c for VM tasks. A valid
// source variable/array reference belongs to the variable region; unchecked
// native out-of-array access is not additional declared variable storage.
// Check direct and statically resolved indexes against that ownership boundary.
// Dynamic arrays retain the same valid-source bounds invariant.
fn validate_context_reservation(program: &Program, analysis: &scenario::Analysis) -> Result<()> {
    use symphonia_script::Width;
    let registry = NativeRegistry::gqseaf();
    let check = |offset: u16, width: Width| {
        ensure!(
            u32::from(offset) + width.bytes() as u32 <= SOURCE_VARIABLE_BYTES,
            "source variable at {offset:#x} exceeds native variable region"
        );
        Ok::<_, anyhow::Error>(())
    };
    for block in analysis.basic_blocks() {
        let mut stack: Vec<(Option<(u16, Width)>, Option<i32>)> = Vec::new();
        for pc in block.instruction_pcs {
            match program.instruction(pc).unwrap().0 {
                Op::Push(value) => stack.push((None, Some(value))),
                Op::Load(offset, width) => {
                    check(offset, width)?;
                    stack.push((Some((offset, width)), None));
                }
                Op::Calculate(0) => {
                    stack.pop();
                }
                Op::Calculate(op @ 1..=8) => {
                    if let Some((_, value)) = stack.last_mut() {
                        if op != 6 {
                            *value = None;
                        }
                    }
                }
                Op::Calculate(op) => {
                    let right = stack.pop().unwrap_or_default();
                    let mut left = stack.pop().unwrap_or_default();
                    if op == 0x0f {
                        left.0 = if let (Some((offset, width)), Some(index)) = (left.0, right.1) {
                            let address =
                                i64::from(offset) + i64::from(index) * width.bytes() as i64;
                            let address = u16::try_from(address)
                                .context("source index exceeds script memory")?;
                            check(address, width)?;
                            Some((address, width))
                        } else {
                            None
                        };
                    }
                    left.1 = if op == 0x10 { right.1 } else { None };
                    stack.push(left);
                }
                Op::Native(opcode) => match registry.get(opcode).and_then(|s| s.returns_value) {
                    Some(true) => stack.push((None, None)),
                    Some(false) => (),
                    None => stack.clear(),
                },
                _ => (),
            }
        }
    }
    Ok(())
}

#[derive(Default)]
struct Code {
    words: Vec<u16>,
}
impl Code {
    fn push(&mut self, value: u32) {
        if value <= 127 {
            self.words.push(value as u16);
        } else if value <= 65535 {
            self.words.extend([0x0300, value as u16]);
        } else {
            self.words
                .extend([0x0200, value as u16, (value >> 16) as u16]);
        }
    }
    fn set_context(&mut self, value: u16) {
        self.words
            .extend([0x1000, INPUT_CONTEXT, value, 0x3010, 0, 0x3033, 0x3031]);
    }
    fn context(&mut self, values: &[u16]) {
        for (index, &value) in values.iter().enumerate() {
            self.words.extend([0x1000, INPUT_CONTEXT, value, 0x3020]);
            if index != 0 {
                self.words.push(0x3027);
            }
        }
    }
    fn bit(&mut self, from: u16, to: u32) {
        self.words.extend([0x1200, RESULT]);
        self.push(u32::from(from));
        self.words.push(0x3036);
        let shift = to.trailing_zeros() as i32 - from.trailing_zeros() as i32;
        if shift != 0 {
            self.push(shift.unsigned_abs());
            self.words.push(if shift > 0 { 0x3039 } else { 0x303a });
        }
    }
    fn mask(&mut self, mappings: &[(u16, u32)]) {
        self.words.extend([0, 0x3036]); // Clear the scalar accumulator, leaving RESULT intact.
        for &(from, to) in mappings {
            self.bit(from, to);
            self.words.push(0x3037);
        }
    }
    fn finish(&mut self) {
        // Store the same converted value in the implicit return cell and retain
        // one scalar on the caller's stack. No expression/argument registers or
        // source globals are borrowed as scratch storage.
        self.words.extend([0x3010, 0x3031]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use symphonia_script::Width;
    use symphonia_script_vm::{Host, Memory, NativeBindings, NativeResult, RunEvent, Vm};

    #[derive(Default)]
    struct Sampler {
        mask: i32,
        arguments: Vec<i32>,
        timer_reads: usize,
        lifecycle: Vec<(u8, i32, i32)>,
    }
    impl Host for Sampler {
        const NATIVES: NativeBindings<Self> = NativeBindings::<Self>::new()
            .register(
                NativeCall::ReadMappedInput as u8,
                2,
                true,
                |host, args, _| {
                    host.arguments = args.to_vec();
                    Ok(NativeResult::Continue(Some(host.mask)))
                },
            )
            .register(NativeCall::GetScenarioTicks as u8, 0, true, |host, _, _| {
                host.timer_reads += 1;
                Ok(NativeResult::Continue(Some(1234)))
            })
            .register(
                NativeCall::SpawnEvent as u8,
                1,
                true,
                |host, args, memory| {
                    host.lifecycle.push((
                        0x39,
                        args[0],
                        memory
                            .read(INPUT_CONTEXT, Width::S8)
                            .map_err(|e| e.to_string())?,
                    ));
                    Ok(NativeResult::Continue(Some(777)))
                },
            )
            .register(
                NativeCall::ReleaseResourceInstance as u8,
                1,
                false,
                |host, args, memory| {
                    host.lifecycle.push((
                        0x3a,
                        args[0],
                        memory
                            .read(INPUT_CONTEXT, Width::S8)
                            .map_err(|e| e.to_string())?,
                    ));
                    Ok(NativeResult::Continue(None))
                },
            );
    }
    fn source(mode: u16) -> Vec<u8> {
        let mut code = vec![
            0x0100, 321, 0x3000, 0x4000, // outer native argument
            0x1200, 0x100, // outer assignment reference
            0, 0x3000, 0x4000, // player
        ];
        if mode & 0x8000 != 0 {
            code.extend([0x0300, mode & 15, 0x0300, 0x8000, 0x3037]);
        } else {
            code.extend([0x0300, mode]);
        }
        code.extend([0x3000, 0x4000, 0x20b8]);
        let target = code.len() as u32 + 4;
        code.extend([0x2001, target as u16, 7, 0x20ff, 0x20ff]);
        let mut bytes = [16u16, 0, 0, 2]
            .into_iter()
            .flat_map(u16::to_be_bytes)
            .collect::<Vec<_>>();
        for record in [[1u32, 42, 0], [1, 43, target]] {
            bytes.extend(record.into_iter().flat_map(u32::to_be_bytes));
        }
        bytes.extend(code.into_iter().flat_map(u16::to_be_bytes));
        let auxiliary = (bytes.len() / 2) as u16;
        bytes[4..6].copy_from_slice(&auxiliary.to_be_bytes());
        bytes.extend_from_slice(b"\0\0\0\x04Buttons\0");
        bytes
    }
    #[test]
    fn input_conversion_uses_existing_vm_and_preserves_caller_state() -> Result<()> {
        for (mode, mask, expected) in [
            (0, 0x100, 0x20),                 // accept
            (1, 0x200, 0x80),                 // cancel
            (0x8001, 0x10, 0x10000),          // skit, ignoring the pause gate
            (0, 0x400, 0x10),                 // ring
            (0, 0x800, 0x40),                 // menu
            (0, 0x100 | 1 | 8, 0x20 | 4 | 1), // simultaneous actions
            (2, 0x100, 0x1000),               // raw press of accept
            (2, 0x1000, 0x10),                // raw press of start
        ] {
            let bytes = normalize(&source(mode))?;
            let header = scenario::parse_header(&bytes)?;
            let messages = symphonia_script::message::parse(&bytes[header.auxiliary_offset()..])?;
            assert_eq!(
                messages[0].tokens,
                [symphonia_script::message::Token::Text {
                    text: "Buttons".into()
                }]
            );
            let program = Arc::new(Program::decode(&bytes)?);
            assert_eq!(
                program
                    .instruction(program.event(1, 43).unwrap())
                    .unwrap()
                    .0,
                symphonia_script::Op::End
            );
            let mut vm = Vm::new(program.clone(), program.entry())?;
            let mut memory = Memory::default();
            memory.write(0x100, Width::S32, 1234)?;
            let mut host = Sampler {
                mask,
                ..Default::default()
            };
            assert_eq!(
                vm.run(&mut host, &mut memory, 1000)?.event,
                RunEvent::Halted
            );
            assert_eq!(
                host.arguments,
                [0, if mode == 2 { 1 } else { i32::from(mode) }]
            );
            assert_eq!(memory.read(RESULT, Width::S32)?, expected);
            assert_eq!(memory.read(0x100, Width::S32)?, 1234);
            assert_eq!((vm.value_depth(), vm.argument_depth()), (2, 1));
            assert_eq!(vm.expression(), Some(i32::from(mode)));
        }
        Ok(())
    }
    fn fixture(code: Vec<u16>) -> Vec<u8> {
        [4u16, 0, 0, 0]
            .into_iter()
            .chain(code)
            .flat_map(u16::to_be_bytes)
            .collect()
    }

    #[test]
    fn native_context_profiles_ignore_player_and_keep_raw_independent() -> Result<()> {
        for context in [0, 1, 2, 3, 5, 9] {
            for player in [0, 1, 2, 3, 4, 9] {
                for (mask, default, restricted, event, raw) in [
                    (0x400, 0x10, 0x40, 0x40, 0x4000),
                    (0x800, 0x40, 0, 0x10, 0x8000),
                    (0x20, 0, 0x200000, 0x200000, 0x200),
                    (0x400 | 0x800 | 0x20, 0x50, 0x200040, 0x200050, 0xc200),
                ] {
                    for mode in [0, 1, 0x8001, 2] {
                        let mut source = source(mode);
                        // The original player literal is intentionally sampled once.
                        source[32 + 6 * 2..32 + 7 * 2].copy_from_slice(&u16::to_be_bytes(player));
                        let bytes = normalize(&source)?;
                        let program = Arc::new(Program::decode(&bytes)?);
                        let mut vm = Vm::new(program.clone(), program.entry())?;
                        let mut memory = Memory::default();
                        memory.write(INPUT_CONTEXT, Width::S8, context)?;
                        let mut host = Sampler {
                            mask,
                            ..Default::default()
                        };
                        assert_eq!(
                            vm.run(&mut host, &mut memory, 2000)?.event,
                            RunEvent::Halted
                        );
                        let expected = if mode == 2 {
                            raw
                        } else {
                            match context {
                                1 | 3 => restricted,
                                5 => event,
                                _ => default,
                            }
                        };
                        assert_eq!(
                            memory.read(RESULT, Width::S32)?,
                            expected,
                            "context {context}, player {player}, mode {mode}, mask {mask:#x}"
                        );
                        assert_eq!(
                            host.arguments,
                            [0, if mode == 2 { 1 } else { i32::from(mode) }]
                        );
                        assert_eq!(memory.read(INPUT_CONTEXT, Width::S8)?, context);
                        assert_eq!((vm.value_depth(), vm.argument_depth()), (2, 1));
                        assert_eq!(vm.expression(), Some(i32::from(mode)));
                    }
                }
            }
        }
        Ok(())
    }

    #[test]
    fn context_writers_preserve_the_caller() -> Result<()> {
        let bytes = normalize(&fixture(vec![
            0x0100, 321, 0x3000, 0x4000, // Pending outer argument.
            0x1200, 0x100, // Live outer assignment reference.
            0x1200, 0x108, 0x3001, 0x3000, 0x4000, 0x2039,
            0x3000, // Consume the native return as in source.
            0, 0x3000, 0x4000, 0x203a, 0x0300, 1500, 0x3000, 0x4000, 0x2039, 0x20ff,
        ]))?;
        let program = Arc::new(Program::decode(&bytes)?);
        let mut vm = Vm::new(program.clone(), program.entry())?;
        let mut memory = Memory::default();
        memory.write(0x100, Width::S32, 5678)?;
        memory.write(0x108, Width::S32, 42)?;
        let mut host = Sampler::default();
        assert_eq!(
            vm.run(&mut host, &mut memory, 1000)?.event,
            RunEvent::Halted
        );
        assert_eq!(
            host.lifecycle,
            [(0x39, 42, 5), (0x3a, 0, 0), (0x39, 1500, 5)]
        );
        assert_eq!(memory.read(0x108, Width::S32)?, 43); // Source postfix runs once.
        assert_eq!(memory.read(0x100, Width::S32)?, 5678);
        assert_eq!(memory.read(RESULT, Width::S32)?, 777);
        assert_eq!(memory.read(INPUT_CONTEXT, Width::S8)?, 5);
        assert_eq!((vm.value_depth(), vm.argument_depth()), (2, 1));
        assert_eq!(vm.expression(), Some(1500));
        Ok(())
    }

    #[test]
    fn menu_and_skit_calls_remain_source_bytecode() -> Result<()> {
        // Scene-close state belongs to the common runtime boundary. Keep the
        // original calls, arguments and continuation ordering untouched here.
        let bytes = fixture(vec![
            0x0300, 9999, 0x3000, 0x4000, 0x2067, 0x3000, 0x0300, 628, 0x3000, 0x4000, 1, 0x3000,
            0x4000, 0, 0x3000, 0x4000, 0x2083, 2, 0x3000, 0x4000, 0x20ed, 0x3000, 0x4000, 0x20ee,
            0x20ff,
        ]);
        assert_eq!(normalize(&bytes)?, bytes);
        Ok(())
    }

    #[test]
    fn native_variable_copy_keeps_private_skit_context_at_zero() -> Result<()> {
        let mut parent = Memory::default();
        parent.write(INPUT_CONTEXT, Width::S8, 5)?;
        parent.write(0x100, Width::S32, 5678)?;
        parent.write(0x1fff, Width::S8, 73)?;
        let mut child = Memory::default();
        child.copy_from(&parent, 0..SOURCE_VARIABLE_BYTES as u16)?;
        assert_eq!(child.read(INPUT_CONTEXT, Width::S8)?, 0);
        assert_eq!(child.read(0x100, Width::S32)?, 5678);
        assert_eq!(child.read(0x1fff, Width::S8)?, 73);
        let bytes = normalize(&source(1))?;
        let program = Arc::new(Program::decode(&bytes)?);
        let mut host = Sampler {
            mask: 0x400,
            ..Default::default()
        };
        Vm::new(program.clone(), program.entry())?.run(&mut host, &mut child, 2000)?;
        assert_eq!(child.read(RESULT, Width::S32)?, 0x10);
        assert_eq!(parent.read(INPUT_CONTEXT, Width::S8)?, 5);
        Ok(())
    }

    // Original field237 listener1502, through its first accepted timestamp.
    // Steam gra_d02_bin0001.bin_ SHA256 7fc722dce11eec687b33239f36a826622326b9c4f64c8b6c63a14cdf5627b13c,
    // PC0x6604; GC section6 SHA256 a6c74cfb1db5814c16be2d7a6e93613cc202fa9420721b680e48fabab3d490ec,
    // PC0x63ff. This semantic transcription omits the infinite loop and the
    // later jump-animation callback, and bounds the false branch to fixture end.
    fn gnomelette_listener(source_mask: u16, timestamp: u16) -> Vec<u8> {
        let mut code = vec![
            0x1200,
            0x600,
            1,
            0x3000,
            0x4000, // Native player argument, ignored on Steam.
            1,
            0x0300,
            0x8000,
            0x3037,
            0x3000,
            0x4000,
            0x20b8,
            0x3010,
            0x3000,
            0x1200,
            0x600,
            0x0300,
            source_mask,
            0x3036,
            0x1200,
            timestamp,
            0,
            0x3020,
            0x3026,
            0x3000,
            0x2004,
            0,
            0x1200,
            timestamp,
            0x2000 | NativeCall::GetScenarioTicks as u16,
            0x3010,
            0x3000,
            0x20ff,
        ];
        code[26] = (code.len() - 1) as u16;
        fixture(code)
    }

    #[test]
    fn actual_gnomelette_listener_records_ring_in_spawn_context() -> Result<()> {
        let writer = normalize(&fixture(vec![0x0300, 1502, 0x3000, 0x4000, 0x2039, 0x20ff]))?;
        let writer = Arc::new(Program::decode(&writer)?);
        for (edition, source_mask, timestamp, steam) in [
            ("original GC", 0x400, 0x844, false),
            ("original Steam", 0x40, 0x84c, true),
        ] {
            let source = gnomelette_listener(source_mask, timestamp);
            let bytes = if steam { normalize(&source)? } else { source };
            let program = Arc::new(Program::decode(&bytes)?);
            for (mask, expected_reads) in [(0x400, 1), (0x800, 0)] {
                let mut memory = Memory::default();
                let mut host = Sampler {
                    mask,
                    ..Default::default()
                };
                Vm::new(writer.clone(), writer.entry())?.run(&mut host, &mut memory, 1000)?;
                assert_eq!(memory.read(INPUT_CONTEXT, Width::S8)?, 5);
                let mut vm = Vm::new(program.clone(), program.entry())?;
                assert_eq!(
                    vm.run(&mut host, &mut memory, 2000)?.event,
                    RunEvent::Halted
                );
                assert_eq!(
                    host.timer_reads, expected_reads,
                    "{edition}, input {mask:#x}"
                );
                assert_eq!(
                    memory.read(timestamp, Width::S32)?,
                    if expected_reads == 1 { 1234 } else { 0 }
                );
            }
        }
        Ok(())
    }

    #[test]
    fn source_variables_stay_inside_the_native_owned_region() {
        for words in [
            vec![0x1000, INPUT_CONTEXT, 0x20ff],
            vec![0x1200, INPUT_CONTEXT - 3, 0x20ff],
            vec![0x1000, 0x1fff, 1, 0x300f, 0x20ff],
        ] {
            assert!(
                normalize(&fixture(words))
                    .unwrap_err()
                    .to_string()
                    .contains("exceeds native variable region")
            );
        }
    }

    #[test]
    fn unknown_input_modes_are_not_guessed() {
        assert!(normalize(&source(3)).is_err());
        let mut dynamic = source(0);
        // The mode producer becomes a state load instead of a literal.
        dynamic[32 + 9 * 2..32 + 11 * 2].copy_from_slice(&[0x12, 0, 0x01, 0x20]);
        assert!(normalize(&dynamic).is_err());
    }
}
