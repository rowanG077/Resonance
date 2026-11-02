//! Adapt source input masks using ordinary GC bytecode and its existing native.
//! The engine continues to sample the same logical GameCube actions for both cooks.
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;
use symphonia_script::{
    NativeCall, Program,
    scenario::{self, Control},
};

const RESULT: u16 = 0x20;
const MAPPED: &[(u16, u32)] = &[
    (1, 4),
    (2, 8),
    (4, 2),
    (8, 1),
    (0x10, 0x10000),
    (0x20, 0x200000),
    (0x40, 0x40000),
    (0x100, 0x20),
    (0x200, 0x80),
    (0x400, 0x10),
    (0x800, 0x40),
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
    let analysis = scenario::analyze(bytes)?;
    let calls = analysis
        .instructions
        .values()
        .filter(|i| {
            i.mnemonic == "proc" && i.operands == [i64::from(NativeCall::ReadMappedInput as u8)]
        })
        .collect::<Vec<_>>();
    if calls.is_empty() {
        return Ok(bytes.to_vec());
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
    let header = analysis.header;
    let end = if header.auxiliary_words == 0 {
        bytes.len()
    } else {
        header.auxiliary_offset()
    };
    let source = bytes[header.code_base()..end]
        .chunks_exact(2)
        .map(|w| u16::from_be_bytes([w[0], w[1]]))
        .collect::<Vec<_>>();
    let mut output = Code::default();
    let mut addresses = BTreeMap::new();
    for (pc, &word) in source.iter().enumerate() {
        let pc = pc as u32;
        addresses.insert(
            pc,
            u16::try_from(output.words.len()).context("normalized script exceeds PC range")?,
        );
        if let Some(&mode) = restore.get(&pc) {
            output.push(mode as u32);
            output.words.push(0x3000);
        }
        if raw.contains_key(&pc) {
            output.words.extend([0, 0x1200, RESULT]);
        }
        if let Some(&width) = replace.get(&pc) {
            // Keep the producer's original width so later operand positions map
            // independently of this rewrite.
            output.words.extend(match width {
                1 => vec![1],
                2 => vec![word, 1],
                3 => vec![word, 1, 0],
                _ => unreachable!(),
            });
        } else if !replace
            .iter()
            .any(|(&start, &width)| start < pc && pc < start + width)
        {
            output.words.push(word);
        }
        if let Some(&is_raw) = raw.get(&pc) {
            output.mask(if is_raw { RAW } else { MAPPED });
        }
    }
    for instruction in analysis
        .instructions
        .values()
        .filter(|i| matches!(i.control, Control::Jump | Control::Call | Control::Branch))
    {
        let at = usize::from(addresses[&instruction.pc]) + 1;
        let target = u32::try_from(instruction.operands[0])?;
        output.words[at] = *addresses
            .get(&target)
            .context("missing relocated script target")?;
    }
    let mut result = bytes[..header.code_base()].to_vec();
    let entry = addresses
        .get(&u32::from(header.default_pc))
        .context("missing relocated entry")?;
    result[2..4].copy_from_slice(&entry.to_be_bytes());
    if header.auxiliary_words != 0 {
        let auxiliary = usize::from(header.code_base_words) + output.words.len();
        result[4..6].copy_from_slice(
            &u16::try_from(auxiliary)
                .context("normalized script exceeds auxiliary range")?
                .to_be_bytes(),
        );
    }
    for record in analysis.records.iter().filter(|r| r.active()) {
        let pc = u32::from(
            *addresses
                .get(&record.pc)
                .context("missing relocated event")?,
        );
        result[record.offset + 8..record.offset + 12].copy_from_slice(&pc.to_be_bytes());
    }
    result.extend(output.words.into_iter().flat_map(u16::to_be_bytes));
    if header.auxiliary_words != 0 {
        result.extend_from_slice(&bytes[end..]);
    }
    Program::decode(&result).context("normalized input script")?;
    Ok(result)
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
    fn mask(&mut self, mappings: &[(u16, u32)]) {
        self.words.extend([0, 0x3036]); // Clear the scalar accumulator, leaving RESULT intact.
        for &(from, to) in mappings {
            self.words.extend([0x1200, RESULT]);
            self.push(u32::from(from));
            self.words.push(0x3036);
            let shift = to.trailing_zeros() as i32 - from.trailing_zeros() as i32;
            if shift != 0 {
                self.push(shift.unsigned_abs());
                self.words.push(if shift > 0 { 0x3039 } else { 0x303a });
            }
            self.words.push(0x3037);
        }
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
    }
    impl Host for Sampler {
        const NATIVES: NativeBindings<Self> = NativeBindings::<Self>::new().register(
            NativeCall::ReadMappedInput as u8,
            2,
            true,
            |host, args, _| {
                host.arguments = args.to_vec();
                Ok(NativeResult::Continue(Some(host.mask)))
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
    #[test]
    fn unknown_input_modes_are_not_guessed() {
        assert!(normalize(&source(3)).is_err());
        let mut dynamic = source(0);
        // The mode producer becomes a state load instead of a literal.
        dynamic[32 + 9 * 2..32 + 11 * 2].copy_from_slice(&[0x12, 0, 0x01, 0x20]);
        assert!(normalize(&dynamic).is_err());
    }
}
