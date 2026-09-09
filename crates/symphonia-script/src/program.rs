use crate::scenario::{self, Control};
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Width {
    S8,
    S16,
    S32,
}
impl Width {
    pub const fn bytes(self) -> usize {
        match self {
            Self::S8 => 1,
            Self::S16 => 2,
            Self::S32 => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Push(i32),
    Load(u16, Width),
    Argument,
    Calculate(u8),
    Jump(u32),
    Call(u32),
    Return,
    BranchFalse(u32),
    Native(u8),
    End,
}

/// Immutable, validated code shared by all instances of an event resource.
#[derive(Debug)]
pub struct Program {
    entry: u32,
    events: BTreeMap<(u32, u32), u32>,
    code: BTreeMap<u32, (Op, u32)>,
    auxiliary: Vec<u8>,
}

#[derive(Debug, Error)]
pub enum ProgramError {
    #[error(transparent)]
    Decode(#[from] scenario::Error),
    #[error("invalid SymphoniaScript: {0}")]
    Invalid(String),
}

impl Program {
    pub fn decode(bytes: &[u8]) -> Result<Self, ProgramError> {
        // Header/code PCs are 16-bit word offsets; bound analysis allocations.
        if bytes.len() > 4 * 1024 * 1024 {
            return Err(ProgramError::Invalid("resource exceeds 4 MiB".into()));
        }
        let analysis = scenario::analyze(bytes)?;
        let invalid = |message: String| ProgramError::Invalid(message);
        if analysis.has_errors() {
            return Err(invalid(
                analysis
                    .diagnostics
                    .iter()
                    .filter(|d| d.severity == scenario::Severity::Error)
                    .map(|d| d.message.as_str())
                    .collect::<Vec<_>>()
                    .join("; "),
            ));
        }
        let header = analysis.header;
        // The registry is an overlay, not a prefix defining code boundaries.
        // Some valid resources use code_base=0 and execute words in that view.
        if header.auxiliary_words != 0
            && (header.auxiliary_offset() < header.code_base()
                || header.auxiliary_offset() > bytes.len())
        {
            return Err(invalid("invalid auxiliary offset".into()));
        }
        let mut code = BTreeMap::new();
        for (&pc, i) in &analysis.instructions {
            if i.raw {
                return Err(invalid(format!("undecoded instruction at PC {pc:#x}")));
            }
            let operand = || i.operands[0];
            let op = match i.mnemonic {
                "push.s8" | "push.s16" | "push.u16" | "push.s32" => Op::Push(operand() as i32),
                "load.s8" => Op::Load(operand() as u16, Width::S8),
                "load.s16" => Op::Load(operand() as u16, Width::S16),
                "load.s32" => Op::Load(operand() as u16, Width::S32),
                "arg" => Op::Argument,
                "calc" if crate::semantics::calculator(operand() as u8).is_some() => {
                    Op::Calculate(operand() as u8)
                }
                "jump" => Op::Jump(operand() as u32),
                "call" => Op::Call(operand() as u32),
                "ret" => Op::Return,
                "branch_false" => Op::BranchFalse(operand() as u32),
                "proc" => Op::Native(operand() as u8),
                "end" => Op::End,
                _ => return Err(invalid(format!("unsupported instruction at PC {pc:#x}"))),
            };
            let targets: Vec<u32> = match i.control {
                Control::Jump => vec![operand() as u32],
                Control::Call | Control::Branch => vec![operand() as u32, i.end_pc()],
                Control::Fallthrough => vec![i.end_pc()],
                Control::Return | Control::Stop => vec![],
            };
            if targets
                .iter()
                .any(|t| !analysis.instructions.contains_key(t))
            {
                return Err(invalid(format!(
                    "instruction at PC {pc:#x} targets operand data"
                )));
            }
            code.insert(pc, (op, i.end_pc()));
        }
        let mut events = BTreeMap::new();
        for record in analysis.records.iter().filter(|r| r.active()) {
            if !code.contains_key(&record.pc)
                || events
                    .insert((record.kind, record.key), record.pc)
                    .is_some()
            {
                return Err(invalid("invalid or duplicate event entry".into()));
            }
        }
        let entry = u32::from(header.default_pc);
        if !code.contains_key(&entry) {
            return Err(invalid("invalid default entry".into()));
        }
        Ok(Self {
            entry,
            events,
            code,
            auxiliary: if header.auxiliary_words == 0 {
                vec![]
            } else {
                bytes[header.auxiliary_offset()..].to_vec()
            },
        })
    }
    pub fn entry(&self) -> u32 {
        self.entry
    }
    pub fn event(&self, kind: u32, key: u32) -> Option<u32> {
        self.events.get(&(kind, key)).copied()
    }
    pub fn instruction(&self, pc: u32) -> Option<(Op, u32)> {
        self.code.get(&pc).copied()
    }
    /// Strings use the resource's auxiliary offset table, not host pointers.
    pub fn string(&self, index: u16) -> Option<&[u8]> {
        let offset = usize::from(index) * 4;
        let start =
            u32::from_be_bytes(self.auxiliary.get(offset..offset + 4)?.try_into().ok()?) as usize;
        let bytes = self.auxiliary.get(start..)?;
        Some(&bytes[..bytes.iter().position(|b| *b == 0)?])
    }
}
