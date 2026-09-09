//! Lossless decoder, assembler, disassembler, and verifier for scenario blobs.

// All narrowing conversions below follow explicit VM-width range checks.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::too_many_lines
)]

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use thiserror::Error;

const UNUSED_PROCEDURES: &[u8] = &[0x05, 0x06, 0x07, 0x08, 0x09, 0x5b, 0xef];

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum Error {
    #[error("scenario blob is shorter than its 8-byte header")]
    ShortHeader,
    #[error("line {line}: {message}")]
    Source { line: usize, message: String },
    #[error("source is missing .code_base")]
    MissingCodeBase,
    #[error(".code_base does not match the first serialized header word (0x{actual:04X})")]
    CodeBaseMismatch { actual: u16 },
    #[error("slice 0x{offset:X}:0x{end:X} exceeds input size 0x{length:X}")]
    SliceOutOfRange {
        offset: usize,
        end: usize,
        length: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    pub code_base_words: u16,
    pub default_pc: u16,
    pub auxiliary_words: u16,
    pub registry_count: u16,
}

impl Header {
    #[must_use]
    pub const fn code_base(self) -> usize {
        self.code_base_words as usize * 2
    }

    #[must_use]
    pub const fn auxiliary_offset(self) -> usize {
        self.auxiliary_words as usize * 2
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistryRecord {
    pub index: usize,
    pub kind: u32,
    pub key: u32,
    pub pc: u32,
    pub offset: usize,
}

impl RegistryRecord {
    #[must_use]
    pub const fn active(self) -> bool {
        // Actor interactions, region triggers, and explicitly spawned events.
        self.kind <= 2
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<usize>,
}

impl Diagnostic {
    fn error(code: &'static str, message: impl Into<String>, offset: Option<usize>) -> Self {
        Self {
            severity: Severity::Error,
            code,
            message: message.into(),
            offset,
        }
    }

    fn warning(code: &'static str, message: impl Into<String>, offset: Option<usize>) -> Self {
        Self {
            severity: Severity::Warning,
            code,
            message: message.into(),
            offset,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    Fallthrough,
    Jump,
    Call,
    Branch,
    Return,
    Stop,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instruction {
    pub pc: u32,
    pub offset: usize,
    pub size_words: u32,
    pub mnemonic: &'static str,
    pub operands: Vec<i64>,
    pub raw: bool,
    pub control: Control,
}

impl Instruction {
    #[must_use]
    pub const fn end_pc(&self) -> u32 {
        self.pc + self.size_words
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BasicBlock {
    pub start_pc: u32,
    pub end_pc: u32,
    pub instruction_pcs: Vec<u32>,
    pub successors: Vec<u32>,
}

#[derive(Debug, Clone)]
pub struct Analysis {
    pub data: Vec<u8>,
    pub header: Header,
    pub records: Vec<RegistryRecord>,
    pub instructions: BTreeMap<u32, Instruction>,
    pub roots: BTreeSet<u32>,
    pub labels: BTreeSet<u32>,
    pub diagnostics: Vec<Diagnostic>,
}

impl Analysis {
    #[must_use]
    pub fn code_end(&self) -> usize {
        let auxiliary = self.header.auxiliary_offset();
        if self.header.code_base() < auxiliary && auxiliary <= self.data.len() {
            auxiliary
        } else {
            self.data.len()
        }
    }

    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|item| item.severity == Severity::Error)
    }

    #[must_use]
    pub fn basic_blocks(&self) -> Vec<BasicBlock> {
        let mut leaders: BTreeSet<u32> = self
            .roots
            .intersection(&self.instructions.keys().copied().collect())
            .copied()
            .collect();
        for instruction in self.instructions.values() {
            if instruction.control != Control::Fallthrough {
                leaders.extend(
                    successors(instruction)
                        .into_iter()
                        .filter(|pc| self.instructions.contains_key(pc)),
                );
            }
        }

        let mut blocks = Vec::new();
        for &leader in &leaders {
            let mut pcs = Vec::new();
            let mut pc = leader;
            let mut block_successors = Vec::new();
            while let Some(instruction) = self.instructions.get(&pc) {
                pcs.push(pc);
                block_successors = successors(instruction);
                if instruction.control != Control::Fallthrough {
                    break;
                }
                let next_pc = instruction.end_pc();
                if leaders.contains(&next_pc) && next_pc != leader {
                    block_successors = vec![next_pc];
                    break;
                }
                if !self.instructions.contains_key(&next_pc) {
                    block_successors.clear();
                    break;
                }
                pc = next_pc;
            }
            if let Some(terminal) = pcs.last().and_then(|pc| self.instructions.get(pc)) {
                blocks.push(BasicBlock {
                    start_pc: leader,
                    end_pc: terminal.end_pc(),
                    instruction_pcs: pcs,
                    successors: block_successors,
                });
            }
        }
        blocks
    }

    #[must_use]
    pub fn summary(&self) -> AnalysisSummary<'_> {
        AnalysisSummary {
            size: self.data.len(),
            header: HeaderSummary {
                code_base_words: self.header.code_base_words,
                code_base: self.header.code_base(),
                default_pc: self.header.default_pc,
                auxiliary_words: self.header.auxiliary_words,
                auxiliary_offset: self.header.auxiliary_offset(),
                registry_count: self.header.registry_count,
            },
            active_records: self
                .records
                .iter()
                .filter(|record| record.active())
                .collect(),
            roots: self.roots.iter().copied().collect(),
            instruction_count: self.instructions.len(),
            cfg: self.basic_blocks(),
            diagnostics: &self.diagnostics,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct HeaderSummary {
    code_base_words: u16,
    code_base: usize,
    default_pc: u16,
    auxiliary_words: u16,
    auxiliary_offset: usize,
    registry_count: u16,
}

#[derive(Debug, Serialize)]
pub struct AnalysisSummary<'a> {
    size: usize,
    header: HeaderSummary,
    active_records: Vec<&'a RegistryRecord>,
    roots: Vec<u32>,
    instruction_count: usize,
    cfg: Vec<BasicBlock>,
    diagnostics: &'a [Diagnostic],
}

fn be_u16(data: &[u8], offset: usize) -> Option<u16> {
    data.get(offset..offset + 2)
        .map(|bytes| u16::from_be_bytes([bytes[0], bytes[1]]))
}

fn be_u32(data: &[u8], offset: usize) -> Option<u32> {
    data.get(offset..offset + 4)
        .map(|bytes| u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

pub fn parse_header(data: &[u8]) -> Result<Header, Error> {
    if data.len() < 8 {
        return Err(Error::ShortHeader);
    }
    Ok(Header {
        code_base_words: be_u16(data, 0).expect("checked length"),
        default_pc: be_u16(data, 2).expect("checked length"),
        auxiliary_words: be_u16(data, 4).expect("checked length"),
        registry_count: be_u16(data, 6).expect("checked length"),
    })
}

fn parse_records(data: &[u8], header: Header) -> (Vec<RegistryRecord>, Vec<Diagnostic>) {
    let mut records = Vec::new();
    let mut diagnostics = Vec::new();
    for index in 0..usize::from(header.registry_count) {
        let offset = 8 + index * 12;
        if offset + 12 > data.len() {
            diagnostics.push(Diagnostic::error(
                "truncated-registry",
                format!("registry record {index} extends beyond the blob"),
                Some(offset),
            ));
            break;
        }
        records.push(RegistryRecord {
            index,
            kind: be_u32(data, offset).expect("checked length"),
            key: be_u32(data, offset + 4).expect("checked length"),
            pc: be_u32(data, offset + 8).expect("checked length"),
            offset,
        });
    }
    (records, diagnostics)
}

fn signed(value: u32, bits: u32) -> i64 {
    let sign = 1_u32 << (bits - 1);
    if value & sign == 0 {
        i64::from(value)
    } else {
        i64::from(value) - (1_i64 << bits)
    }
}

fn known_procedure(opcode: u8) -> bool {
    (1..=0xf4).contains(&opcode) && !UNUSED_PROCEDURES.contains(&opcode)
}

fn known_calculator(opcode: u8) -> bool {
    (0x00..=0x08).contains(&opcode)
        || (0x0f..=0x1a).contains(&opcode)
        || (0x20..=0x27).contains(&opcode)
        || (0x31..=0x3a).contains(&opcode)
}

fn instruction(
    pc: u32,
    offset: usize,
    size_words: u32,
    mnemonic: &'static str,
    operands: Vec<i64>,
    raw: bool,
    control: Control,
) -> Instruction {
    Instruction {
        pc,
        offset,
        size_words,
        mnemonic,
        operands,
        raw,
        control,
    }
}

fn decode_instruction(
    data: &[u8],
    header: Header,
    pc: u32,
    code_end: usize,
) -> (Instruction, Vec<Diagnostic>) {
    let offset = header.code_base().saturating_add(pc as usize * 2);
    let Some(word) = be_u16(data, offset).filter(|_| offset + 2 <= code_end) else {
        return (
            instruction(pc, offset, 1, ".word", vec![0], true, Control::Stop),
            vec![Diagnostic::error(
                "truncated-word",
                format!("PC 0x{pc:X} has no complete word"),
                Some(offset),
            )],
        );
    };
    let group = word & 0xf000;
    let mode = word & 0x0f00;
    let opcode = (word & 0xff) as u8;
    let raw_word = || {
        instruction(
            pc,
            offset,
            1,
            ".word",
            vec![i64::from(word)],
            true,
            Control::Fallthrough,
        )
    };
    let raw = |code, message: String| {
        (
            raw_word(),
            vec![Diagnostic::warning(code, message, Some(offset))],
        )
    };
    let following = |count: usize| -> Result<Vec<u16>, Vec<Diagnostic>> {
        let mut values = Vec::new();
        for index in 0..count {
            let operand_offset = offset + 2 + index * 2;
            if operand_offset + 2 > code_end {
                return Err(vec![Diagnostic::error(
                    "truncated-operand",
                    format!("instruction at PC 0x{pc:X} lacks operand word {index}"),
                    Some(offset),
                )]);
            }
            values.push(be_u16(data, operand_offset).expect("bounded by code view"));
        }
        Ok(values)
    };

    if word == 0x20ff {
        return (
            instruction(pc, offset, 1, "end", vec![], false, Control::Stop),
            vec![],
        );
    }
    match group {
        0x0000 => {
            if mode == 0 {
                return (
                    instruction(
                        pc,
                        offset,
                        1,
                        "push.s8",
                        vec![signed(u32::from(opcode), 8)],
                        false,
                        Control::Fallthrough,
                    ),
                    vec![],
                );
            }
            if opcode != 0 {
                return raw(
                    "noncanonical-literal",
                    format!("literal word 0x{word:04X} has nonzero low byte"),
                );
            }
            let count = match mode {
                0x0100 | 0x0300 => 1,
                0x0200 => 2,
                _ => {
                    return raw(
                        "unknown-literal-mode",
                        format!("literal word 0x{word:04X} uses unknown mode"),
                    );
                }
            };
            let values = match following(count) {
                Ok(values) => values,
                Err(errors) => return (raw_word(), errors),
            };
            let (mnemonic, value) = match mode {
                0x0100 => ("push.s16", signed(u32::from(values[0]), 16)),
                0x0200 => (
                    "push.s32",
                    signed(u32::from(values[0]) | (u32::from(values[1]) << 16), 32),
                ),
                _ => ("push.u16", i64::from(values[0])),
            };
            (
                instruction(
                    pc,
                    offset,
                    count as u32 + 1,
                    mnemonic,
                    vec![value],
                    false,
                    Control::Fallthrough,
                ),
                vec![],
            )
        }
        0x1000 => {
            if opcode != 0 || !matches!(mode, 0 | 0x0100 | 0x0200) {
                return raw(
                    "noncanonical-load",
                    format!("load word 0x{word:04X} is noncanonical"),
                );
            }
            match following(1) {
                Ok(values) => {
                    let mnemonic = match mode {
                        0 => "load.s8",
                        0x0100 => "load.s16",
                        _ => "load.s32",
                    };
                    (
                        instruction(
                            pc,
                            offset,
                            2,
                            mnemonic,
                            vec![i64::from(values[0])],
                            false,
                            Control::Fallthrough,
                        ),
                        vec![],
                    )
                }
                Err(errors) => (raw_word(), errors),
            }
        }
        0x2000 => {
            if mode != 0 {
                return raw(
                    "noncanonical-procedure",
                    format!("procedure word 0x{word:04X} has mode bits"),
                );
            }
            if matches!(opcode, 1 | 2 | 4) {
                return match following(1) {
                    Ok(values) => {
                        let (mnemonic, control) = match opcode {
                            1 => ("jump", Control::Jump),
                            2 => ("call", Control::Call),
                            _ => ("branch_false", Control::Branch),
                        };
                        (
                            instruction(
                                pc,
                                offset,
                                2,
                                mnemonic,
                                vec![i64::from(values[0])],
                                false,
                                control,
                            ),
                            vec![],
                        )
                    }
                    Err(errors) => (raw_word(), errors),
                };
            }
            if opcode == 3 {
                return (
                    instruction(pc, offset, 1, "ret", vec![], false, Control::Return),
                    vec![],
                );
            }
            let mut diagnostics = Vec::new();
            if !known_procedure(opcode) {
                diagnostics.push(Diagnostic::warning(
                    "unknown-procedure",
                    format!("procedure 0x{opcode:02X} is absent from the GQSEAF registry"),
                    Some(offset),
                ));
            }
            (
                instruction(
                    pc,
                    offset,
                    1,
                    "proc",
                    vec![i64::from(opcode)],
                    false,
                    Control::Fallthrough,
                ),
                diagnostics,
            )
        }
        0x3000 => {
            if mode != 0 {
                return raw(
                    "noncanonical-calculator",
                    format!("calculator word 0x{word:04X} has mode bits"),
                );
            }
            let mut diagnostics = Vec::new();
            if !known_calculator(opcode) {
                diagnostics.push(Diagnostic::warning(
                    "unknown-calculator",
                    format!("calculator opcode 0x{opcode:02X} is not supported"),
                    Some(offset),
                ));
            }
            (
                instruction(
                    pc,
                    offset,
                    1,
                    "calc",
                    vec![i64::from(opcode)],
                    false,
                    Control::Fallthrough,
                ),
                diagnostics,
            )
        }
        0x4000 => {
            if word == 0x4000 {
                (
                    instruction(pc, offset, 1, "arg", vec![], false, Control::Fallthrough),
                    vec![],
                )
            } else {
                raw(
                    "noncanonical-argument",
                    format!("argument word 0x{word:04X} has ignored bits"),
                )
            }
        }
        _ => raw(
            "unknown-group",
            format!("word 0x{word:04X} uses unknown instruction group"),
        ),
    }
}

fn successors(instruction: &Instruction) -> Vec<u32> {
    match instruction.control {
        Control::Stop | Control::Return => vec![],
        Control::Jump => vec![instruction.operands[0] as u32],
        Control::Call | Control::Branch => {
            vec![instruction.operands[0] as u32, instruction.end_pc()]
        }
        Control::Fallthrough => vec![instruction.end_pc()],
    }
}

fn stack_requirement(instruction: &Instruction) -> (i32, Option<i32>) {
    if instruction.mnemonic.starts_with("push.") || instruction.mnemonic.starts_with("load.") {
        return (0, Some(1));
    }
    if instruction.mnemonic == "branch_false" {
        return (1, Some(0));
    }
    if instruction.mnemonic == "calc" {
        let opcode = instruction.operands[0] as u8;
        return if opcode == 0 {
            (1, Some(-1))
        } else if opcode <= 8 {
            (1, Some(0))
        } else if known_calculator(opcode) {
            (2, Some(-1))
        } else {
            (0, None)
        };
    }
    if instruction.mnemonic == "proc" || instruction.raw {
        (0, None)
    } else {
        (0, Some(0))
    }
}

fn analyze_stack(analysis: &mut Analysis) {
    let mut seen: HashMap<u32, Option<i32>> = HashMap::new();
    let mut work: Vec<(u32, Option<i32>)> =
        analysis.roots.iter().map(|&root| (root, Some(0))).collect();
    let mut reported: HashSet<(&'static str, u32)> = HashSet::new();
    while let Some((pc, depth)) = work.pop() {
        let Some(current) = analysis.instructions.get(&pc) else {
            continue;
        };
        if let Some(previous) = seen.get(&pc) {
            if let (Some(previous), Some(depth)) = (*previous, depth)
                && previous != depth
                && reported.insert(("stack-merge", pc))
            {
                analysis.diagnostics.push(Diagnostic::warning(
                    "stack-merge",
                    format!("PC 0x{pc:X} is reached with stack depths {previous} and {depth}"),
                    Some(current.offset),
                ));
            }
            continue;
        }
        seen.insert(pc, depth);
        let (required, delta) = stack_requirement(current);
        let next_depth = match (depth, delta) {
            (Some(depth), _) if depth < required => {
                if reported.insert(("stack-underflow", pc)) {
                    analysis.diagnostics.push(Diagnostic::warning(
                        "stack-underflow",
                        format!("PC 0x{pc:X} requires {required} value(s), depth is {depth}"),
                        Some(current.offset),
                    ));
                }
                None
            }
            (Some(depth), Some(delta)) => Some(depth + delta),
            _ => None,
        };
        work.extend(
            successors(current)
                .into_iter()
                .map(|successor| (successor, next_depth)),
        );
    }
}

pub fn analyze(data: &[u8]) -> Result<Analysis, Error> {
    let header = parse_header(data)?;
    let (records, diagnostics) = parse_records(data, header);
    let mut roots = BTreeSet::from([u32::from(header.default_pc)]);
    roots.extend(
        records
            .iter()
            .filter(|record| record.active())
            .map(|record| record.pc),
    );
    let mut analysis = Analysis {
        data: data.to_vec(),
        header,
        records,
        instructions: BTreeMap::new(),
        labels: roots.clone(),
        roots,
        diagnostics,
    };
    if header.code_base() > data.len() {
        analysis.diagnostics.push(Diagnostic::error(
            "code-base-out-of-range",
            format!(
                "code base 0x{:X} exceeds blob size 0x{:X}",
                header.code_base(),
                data.len()
            ),
            Some(0),
        ));
        return Ok(analysis);
    }
    if header.auxiliary_offset() > data.len() {
        analysis.diagnostics.push(Diagnostic::warning(
            "auxiliary-out-of-range",
            format!(
                "auxiliary offset 0x{:X} exceeds blob size",
                header.auxiliary_offset()
            ),
            Some(4),
        ));
    }
    let code_end = analysis.code_end();
    let pc_valid = |pc: u32| {
        header
            .code_base()
            .checked_add(pc as usize * 2)
            .is_some_and(|offset| offset >= header.code_base() && offset + 2 <= code_end)
    };
    for &root in &analysis.roots {
        if !pc_valid(root) {
            analysis.diagnostics.push(Diagnostic::error(
                "entry-out-of-range",
                format!("entry PC 0x{root:X} lies outside the code view"),
                None,
            ));
        }
    }
    let mut occupied: HashMap<u32, u32> = HashMap::new();
    let mut work: Vec<u32> = analysis
        .roots
        .iter()
        .copied()
        .filter(|pc| pc_valid(*pc))
        .collect();
    while let Some(pc) = work.pop() {
        if analysis.instructions.contains_key(&pc) {
            continue;
        }
        if let Some(owner) = occupied.get(&pc) {
            analysis.diagnostics.push(Diagnostic::warning(
                "overlapping-entry",
                format!("PC 0x{pc:X} enters operand data owned by PC 0x{owner:X}"),
                Some(header.code_base() + pc as usize * 2),
            ));
            continue;
        }
        let (decoded, decoded_diagnostics) = decode_instruction(data, header, pc, code_end);
        if let Some(conflict) =
            (pc..decoded.end_pc()).find(|word_pc| occupied.contains_key(word_pc))
        {
            let owner = occupied[&conflict];
            analysis.diagnostics.push(Diagnostic::warning(
                "overlapping-instruction",
                format!("instruction at PC 0x{pc:X} overlaps instruction at PC 0x{owner:X}"),
                Some(decoded.offset),
            ));
            continue;
        }
        for word_pc in pc..decoded.end_pc() {
            occupied.insert(word_pc, pc);
        }
        let next = successors(&decoded);
        if matches!(
            decoded.control,
            Control::Jump | Control::Call | Control::Branch
        ) {
            analysis.labels.extend(next.iter().copied());
        }
        for successor in next {
            if pc_valid(successor) {
                work.push(successor);
            } else {
                analysis.diagnostics.push(Diagnostic::error(
                    "target-out-of-range",
                    format!("PC 0x{pc:X} targets out-of-range PC 0x{successor:X}"),
                    Some(decoded.offset),
                ));
            }
        }
        analysis.diagnostics.extend(decoded_diagnostics);
        analysis.instructions.insert(pc, decoded);
    }
    analyze_stack(&mut analysis);
    Ok(analysis)
}

fn format_number(value: i64, bits: u32, signed_value: bool) -> String {
    if signed_value && value < 0 {
        return value.to_string();
    }
    let mask = (1_i128 << bits) - 1;
    format!(
        "0x{:0width$X}",
        i128::from(value) & mask,
        width = bits as usize / 4
    )
}

fn format_instruction(instruction: &Instruction, labels: &BTreeSet<u32>) -> String {
    match instruction.mnemonic {
        "jump" | "call" | "branch_false" => {
            let target = instruction.operands[0] as u32;
            let operand = if labels.contains(&target) {
                format!("L_{target:04X}")
            } else {
                format_number(i64::from(target), 16, false)
            };
            format!("{} {operand}", instruction.mnemonic)
        }
        "ret" | "arg" | "end" => instruction.mnemonic.to_owned(),
        ".word" => format!(
            ".word {}",
            format_number(instruction.operands[0], 16, false)
        ),
        "push.s8" => format!(
            "push.s8 {}",
            format_number(instruction.operands[0], 8, true)
        ),
        "push.s16" => format!(
            "push.s16 {}",
            format_number(instruction.operands[0], 16, true)
        ),
        "push.s32" => format!(
            "push.s32 {}",
            format_number(instruction.operands[0], 32, true)
        ),
        "push.u16" => format!(
            "push.u16 {}",
            format_number(instruction.operands[0], 16, false)
        ),
        "load.s8" | "load.s16" | "load.s32" => format!(
            "{} {}",
            instruction.mnemonic,
            format_number(instruction.operands[0], 16, false)
        ),
        "proc" | "calc" => format!(
            "{} {}",
            instruction.mnemonic,
            format_number(instruction.operands[0], 8, false)
        ),
        other => unreachable!("unhandled mnemonic {other}"),
    }
}

pub fn disassemble(data: &[u8]) -> Result<(String, Analysis), Error> {
    let analysis = analyze(data)?;
    let header = analysis.header;
    let mut output = String::new();
    writeln!(output, ".scenario").unwrap();
    writeln!(
        output,
        ".code_base {}",
        format_number(i64::from(header.code_base_words), 16, false)
    )
    .unwrap();
    writeln!(
        output,
        "; header: code_base=0x{:X}, default_pc=0x{:X}, auxiliary=0x{:X}, registry_scan={}",
        header.code_base(),
        header.default_pc,
        header.auxiliary_offset(),
        header.registry_count
    )
    .unwrap();
    for record in analysis.records.iter().filter(|record| record.active()) {
        writeln!(
            output,
            "; registry[{}]: kind={}, key=0x{:08X}, pc=L_{:04X}",
            record.index, record.kind, record.key, record.pc
        )
        .unwrap();
    }
    output.push('\n');
    let by_offset: HashMap<usize, &Instruction> = analysis
        .instructions
        .values()
        .map(|item| (item.offset, item))
        .collect();
    let emittable_labels: BTreeSet<u32> = analysis
        .labels
        .intersection(&analysis.instructions.keys().copied().collect())
        .copied()
        .collect();
    let mut labels_by_offset: BTreeMap<usize, Vec<u32>> = BTreeMap::new();
    for &pc in &emittable_labels {
        let offset = header.code_base() + pc as usize * 2;
        if offset <= data.len() {
            labels_by_offset.entry(offset).or_default().push(pc);
        }
    }
    let mut offset = 0;
    while offset < data.len() {
        if let Some(labels) = labels_by_offset.get(&offset) {
            for pc in labels {
                writeln!(output, "L_{pc:04X}:").unwrap();
            }
        }
        if let Some(item) = by_offset.get(&offset) {
            let rendered = format_instruction(item, &emittable_labels);
            let raw_words = (0..item.size_words as usize)
                .filter_map(|index| be_u16(data, offset + index * 2))
                .map(|word| format!("{word:04X}"))
                .collect::<Vec<_>>()
                .join(" ");
            writeln!(output, "    {rendered:<28} ; +0x{offset:06X}  {raw_words}").unwrap();
            offset += item.size_words as usize * 2;
        } else if let Some(word) = be_u16(data, offset) {
            writeln!(
                output,
                "    .word 0x{word:04X}             ; +0x{offset:06X}"
            )
            .unwrap();
            offset += 2;
        } else {
            writeln!(
                output,
                "    .byte 0x{:02X}             ; +0x{offset:06X}",
                data[offset]
            )
            .unwrap();
            offset += 1;
        }
    }
    Ok((output, analysis))
}

#[derive(Debug)]
struct SourceItem {
    line: usize,
    mnemonic: String,
    operands: Vec<String>,
}

fn source_error(line: usize, message: impl Into<String>) -> Error {
    Error::Source {
        line,
        message: message.into(),
    }
}

fn parse_int(token: &str, line: usize) -> Result<i64, Error> {
    let (negative, unsigned) = token
        .strip_prefix('-')
        .map_or((false, token), |rest| (true, rest));
    let parsed = if let Some(hex) = unsigned
        .strip_prefix("0x")
        .or_else(|| unsigned.strip_prefix("0X"))
    {
        i64::from_str_radix(hex, 16)
    } else if let Some(octal) = unsigned
        .strip_prefix("0o")
        .or_else(|| unsigned.strip_prefix("0O"))
    {
        i64::from_str_radix(octal, 8)
    } else if let Some(binary) = unsigned
        .strip_prefix("0b")
        .or_else(|| unsigned.strip_prefix("0B"))
    {
        i64::from_str_radix(binary, 2)
    } else {
        unsigned.parse()
    };
    parsed
        .map(|value| if negative { -value } else { value })
        .map_err(|_| source_error(line, format!("invalid integer {token:?}")))
}

fn valid_label(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first == '_' || first.is_ascii_alphabetic())
        && chars.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn source_size(mnemonic: &str, line: usize) -> Result<usize, Error> {
    match mnemonic {
        "push.s16" | "push.u16" | "load.s8" | "load.s16" | "load.s32" | "jump" | "call"
        | "branch_false" => Ok(4),
        "push.s32" => Ok(6),
        "push.s8" | "proc" | "calc" | "ret" | "arg" | "end" | ".word" => Ok(2),
        ".byte" => Ok(1),
        _ => Err(source_error(
            line,
            format!("unknown mnemonic or directive {mnemonic:?}"),
        )),
    }
}

fn checked_value(
    token: &str,
    item: &SourceItem,
    bits: u32,
    signed_value: bool,
    labels: &HashMap<String, usize>,
    code_base: usize,
) -> Result<u64, Error> {
    let number = if let Some(&byte_offset) = labels.get(token) {
        let Some(delta) = byte_offset.checked_sub(code_base) else {
            return Err(source_error(
                item.line,
                format!("label {token:?} is outside the wordcode view"),
            ));
        };
        if delta % 2 != 0 {
            return Err(source_error(
                item.line,
                format!("label {token:?} is outside the wordcode view"),
            ));
        }
        (delta / 2) as i64
    } else {
        parse_int(token, item.line)?
    };
    let minimum = if signed_value {
        -(1_i64 << (bits - 1))
    } else {
        0
    };
    let maximum = if signed_value {
        (1_i64 << (bits - 1)) - 1
    } else {
        (1_i64 << bits) - 1
    };
    if !(minimum..=maximum).contains(&number) {
        return Err(source_error(
            item.line,
            format!(
                "value {number} does not fit {}{bits} bits",
                if signed_value { "signed " } else { "" }
            ),
        ));
    }
    Ok((i128::from(number) & ((1_i128 << bits) - 1)) as u64)
}

fn operand(item: &SourceItem, expected: usize) -> Result<Option<&str>, Error> {
    if item.operands.len() != expected {
        let description = if expected == 0 {
            "no operand"
        } else {
            "one operand"
        };
        return Err(source_error(
            item.line,
            format!("{} takes {description}", item.mnemonic),
        ));
    }
    Ok(item.operands.first().map(String::as_str))
}

pub fn assemble(source: &str) -> Result<Vec<u8>, Error> {
    let mut labels = HashMap::new();
    let mut items = Vec::new();
    let mut offset = 0;
    let mut code_base_words = None;
    for (line_index, original) in source.lines().enumerate() {
        let line_number = line_index + 1;
        let line = original.split(';').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if let Some(name) = line.strip_suffix(':').map(str::trim) {
            if !valid_label(name) {
                return Err(source_error(line_number, format!("invalid label {name:?}")));
            }
            if labels.insert(name.to_owned(), offset).is_some() {
                return Err(source_error(
                    line_number,
                    format!("duplicate label {name:?}"),
                ));
            }
            continue;
        }
        let parts: Vec<&str> = line
            .split(|character: char| character.is_ascii_whitespace() || character == ',')
            .filter(|part| !part.is_empty())
            .collect();
        let mnemonic = parts[0].to_ascii_lowercase();
        let operands = parts[1..]
            .iter()
            .map(|part| (*part).to_owned())
            .collect::<Vec<_>>();
        if mnemonic == ".scenario" {
            if !operands.is_empty() {
                return Err(source_error(line_number, ".scenario takes no operand"));
            }
            continue;
        }
        if mnemonic == ".code_base" {
            if operands.len() != 1 {
                return Err(source_error(line_number, ".code_base takes one operand"));
            }
            if code_base_words.is_some() {
                return Err(source_error(line_number, "duplicate .code_base"));
            }
            let value = parse_int(&operands[0], line_number)?;
            if !(0..=0xffff).contains(&value) {
                return Err(source_error(line_number, "code base is out of range"));
            }
            code_base_words = Some(value as u16);
            continue;
        }
        let size = source_size(&mnemonic, line_number)?;
        items.push(SourceItem {
            line: line_number,
            mnemonic,
            operands,
        });
        offset += size;
    }
    let code_base_words = code_base_words.ok_or(Error::MissingCodeBase)?;
    let code_base = usize::from(code_base_words) * 2;
    let mut output = Vec::new();
    for item in &items {
        let value = |bits, signed_value| -> Result<u64, Error> {
            checked_value(
                operand(item, 1)?.expect("one operand"),
                item,
                bits,
                signed_value,
                &labels,
                code_base,
            )
        };
        match item.mnemonic.as_str() {
            ".byte" => output.push(value(8, false)? as u8),
            ".word" => output.extend_from_slice(&(value(16, false)? as u16).to_be_bytes()),
            "push.s8" => output.extend_from_slice(&(value(8, true)? as u16).to_be_bytes()),
            "push.s16" => {
                output.extend_from_slice(&0x0100_u16.to_be_bytes());
                output.extend_from_slice(&(value(16, true)? as u16).to_be_bytes());
            }
            "push.s32" => {
                let encoded = value(32, true)? as u32;
                output.extend_from_slice(&0x0200_u16.to_be_bytes());
                output.extend_from_slice(&(encoded as u16).to_be_bytes());
                output.extend_from_slice(&((encoded >> 16) as u16).to_be_bytes());
            }
            "push.u16" => {
                output.extend_from_slice(&0x0300_u16.to_be_bytes());
                output.extend_from_slice(&(value(16, false)? as u16).to_be_bytes());
            }
            "load.s8" | "load.s16" | "load.s32" => {
                let mode = match item.mnemonic.as_str() {
                    "load.s8" => 0,
                    "load.s16" => 0x0100,
                    _ => 0x0200,
                };
                output.extend_from_slice(&(0x1000_u16 | mode).to_be_bytes());
                output.extend_from_slice(&(value(16, false)? as u16).to_be_bytes());
            }
            "proc" | "calc" => {
                let base = if item.mnemonic == "proc" {
                    0x2000
                } else {
                    0x3000
                };
                output.extend_from_slice(&(base | value(8, false)? as u16).to_be_bytes());
            }
            "jump" | "call" | "branch_false" => {
                let opcode = match item.mnemonic.as_str() {
                    "jump" => 1,
                    "call" => 2,
                    _ => 4,
                };
                output.extend_from_slice(&(0x2000_u16 | opcode).to_be_bytes());
                output.extend_from_slice(&(value(16, false)? as u16).to_be_bytes());
            }
            "ret" => {
                operand(item, 0)?;
                output.extend_from_slice(&0x2003_u16.to_be_bytes());
            }
            "arg" => {
                operand(item, 0)?;
                output.extend_from_slice(&0x4000_u16.to_be_bytes());
            }
            "end" => {
                operand(item, 0)?;
                output.extend_from_slice(&0x20ff_u16.to_be_bytes());
            }
            _ => unreachable!(),
        }
    }
    if let Some(actual) = be_u16(&output, 0).filter(|actual| *actual != code_base_words) {
        return Err(Error::CodeBaseMismatch { actual });
    }
    Ok(output)
}

pub fn read_slice(data: &[u8], offset: usize, size: Option<usize>) -> Result<&[u8], Error> {
    let end = size.map_or(data.len(), |size| offset.saturating_add(size));
    if offset > data.len() || end > data.len() {
        return Err(Error::SliceOutOfRange {
            offset,
            end,
            length: data.len(),
        });
    }
    Ok(&data[offset..end])
}
