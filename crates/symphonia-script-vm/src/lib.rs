//! SymphoniaScript execution with checked memory and explicit native host calls.
mod calculator;
mod memory;
mod native;
pub use memory::Memory;
pub use native::{
    Host, NativeBinding, NativeBindings, NativeHandler, NativeResult, NativeSignature,
};
use std::{collections::VecDeque, sync::Arc};
use symphonia_script::{Op, Program, Width};
use thiserror::Error;

const ARGUMENT_STACK_LIMIT: usize = 64;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum Fault {
    #[error("invalid instruction address")]
    Pc,
    #[error("value stack underflow")]
    ValueUnderflow,
    #[error("value stack exceeds 64 entries")]
    ValueOverflow,
    #[error("argument stack underflow (need {0})")]
    ArgumentUnderflow(usize),
    #[error("argument stack exceeds 64 entries")]
    ArgumentOverflow,
    #[error("call stack exceeds 16 entries")]
    CallOverflow,
    #[error("script memory access out of range at {0:#06x}")]
    Memory(u16),
    #[error("indexed address outside script memory")]
    Index,
    #[error("division by zero")]
    DivisionByZero,
    #[error("unsupported calculator {0:#04x}")]
    Calculator(u8),
    #[error("unsupported native {0:#04x}")]
    Native(u8),
    #[error("native return value does not match its binding")]
    NativeResult,
    #[error("native handler failed: {0}")]
    Host(String),
    #[error("instruction budget exhausted ({0})")]
    Budget(u32),
    #[error("VM is suspended; complete its native call before running again")]
    Suspended,
    #[error("VM has no pending native call")]
    NotSuspended,
    #[error("VM previously failed")]
    Failed,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("SymphoniaScript PC {pc:#06x}: {fault}")]
pub struct VmError {
    pub pc: u32,
    pub fault: Fault,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunEvent {
    Halted,
    Suspended { opcode: u8 },
}
#[derive(Debug, Clone, Copy)]
pub struct RunOutcome {
    pub event: RunEvent,
    pub steps: u32,
}
#[derive(Debug, Clone)]
pub struct TraceEntry {
    pub pc: u32,
    pub op: Op,
    pub value_depth: usize,
    pub argument_depth: usize,
}
#[derive(Debug, Clone, Copy)]
struct Value {
    number: i32,
    reference: Option<(u16, Width)>,
}
impl Value {
    fn scalar(number: i32) -> Self {
        Self {
            number,
            reference: None,
        }
    }
}
#[derive(Debug, Clone, Copy)]
enum Status {
    Running,
    Halted,
    Pending { pc: u32, returns_value: bool },
    Failed,
}

pub struct Vm {
    program: Arc<Program>,
    pc: u32,
    status: Status,
    values: Vec<Value>,
    arguments: Vec<i32>,
    calls: Vec<u32>,
    // Retail arg/branch read the slot just popped by expression_end.
    expression: Option<i32>,
    trace_limit: usize,
    trace: VecDeque<TraceEntry>,
}
impl Vm {
    pub fn new(program: Arc<Program>, entry: u32) -> Result<Self, VmError> {
        if program.instruction(entry).is_none() {
            return Err(VmError {
                pc: entry,
                fault: Fault::Pc,
            });
        }
        Ok(Self {
            program,
            pc: entry,
            status: Status::Running,
            values: Vec::with_capacity(64),
            arguments: Vec::with_capacity(ARGUMENT_STACK_LIMIT),
            calls: Vec::with_capacity(16),
            expression: None,
            trace_limit: 0,
            trace: VecDeque::new(),
        })
    }
    pub fn pc(&self) -> u32 {
        self.pc
    }
    pub fn value_depth(&self) -> usize {
        self.values.len()
    }
    pub fn argument_depth(&self) -> usize {
        self.arguments.len()
    }
    pub fn expression(&self) -> Option<i32> {
        self.expression
    }
    pub fn trace(&self) -> &VecDeque<TraceEntry> {
        &self.trace
    }
    pub fn set_trace_limit(&mut self, limit: usize) {
        self.trace_limit = limit.min(65536);
        while self.trace.len() > self.trace_limit {
            self.trace.pop_front();
        }
    }
    /// Budget is per resume, never cumulative. Budget exhaustion is a terminal
    /// script fault so a runaway event cannot silently change update ordering.
    pub fn run(
        &mut self,
        host: &mut impl Host,
        memory: &mut Memory,
        budget: u32,
    ) -> Result<RunOutcome, VmError> {
        match self.status {
            Status::Halted => {
                return Ok(RunOutcome {
                    event: RunEvent::Halted,
                    steps: 0,
                });
            }
            Status::Pending { .. } => {
                return Err(VmError {
                    pc: self.pc,
                    fault: Fault::Suspended,
                });
            }
            Status::Failed => {
                return Err(VmError {
                    pc: self.pc,
                    fault: Fault::Failed,
                });
            }
            Status::Running => {}
        }
        for steps in 1..=budget {
            let pc = self.pc;
            match self.step(host, memory) {
                Ok(Some(event)) => return Ok(RunOutcome { event, steps }),
                Ok(None) => {}
                Err(fault) => {
                    self.status = Status::Failed;
                    return Err(VmError { pc, fault });
                }
            }
        }
        self.status = Status::Failed;
        Err(VmError {
            pc: self.pc,
            fault: Fault::Budget(budget),
        })
    }
    /// Deferred results are supplied exactly once, when the host operation
    /// completes. A waiting dialogue never needs a fabricated placeholder.
    pub fn complete(&mut self, result: Option<i32>, memory: &mut Memory) -> Result<(), VmError> {
        let Status::Pending { pc, returns_value } = self.status else {
            return Err(VmError {
                pc: self.pc,
                fault: Fault::NotSuspended,
            });
        };
        self.return_value(result, returns_value, memory)
            .map_err(|fault| VmError { pc, fault })?;
        self.status = Status::Running;
        Ok(())
    }
    fn return_value(
        &mut self,
        value: Option<i32>,
        expected: bool,
        memory: &mut Memory,
    ) -> Result<(), Fault> {
        if value.is_some() != expected {
            return Err(Fault::NativeResult);
        }
        if let Some(value) = value {
            self.push(Value::scalar(value))?;
            memory.write(0x20, Width::S32, value)?;
        }
        Ok(())
    }
    fn push(&mut self, value: Value) -> Result<(), Fault> {
        if self.values.len() == 64 {
            return Err(Fault::ValueOverflow);
        }
        self.values.push(value);
        Ok(())
    }
    fn pop(&mut self) -> Result<Value, Fault> {
        self.values.pop().ok_or(Fault::ValueUnderflow)
    }
    fn step<H: Host>(
        &mut self,
        host: &mut H,
        memory: &mut Memory,
    ) -> Result<Option<RunEvent>, Fault> {
        let pc = self.pc;
        let (op, next) = self.program.instruction(pc).ok_or(Fault::Pc)?;
        if self.trace_limit > 0 {
            if self.trace.len() == self.trace_limit {
                self.trace.pop_front();
            }
            self.trace.push_back(TraceEntry {
                pc,
                op,
                value_depth: self.values.len(),
                argument_depth: self.arguments.len(),
            });
        }
        self.pc = next;
        match op {
            Op::Push(n) => self.push(Value::scalar(n))?,
            Op::Load(offset, width) => self.push(Value {
                number: memory.read(offset, width)?,
                reference: Some((offset, width)),
            })?,
            Op::Argument => {
                if self.arguments.len() == ARGUMENT_STACK_LIMIT {
                    return Err(Fault::ArgumentOverflow);
                }
                self.arguments
                    .push(self.expression.ok_or(Fault::ValueUnderflow)?);
            }
            Op::Calculate(op) => self.calculate(op, memory)?,
            Op::Jump(target) => self.pc = target,
            Op::Call(target) => {
                if self.calls.len() == 16 {
                    return Err(Fault::CallOverflow);
                }
                self.calls.push(next);
                self.pc = target;
            }
            Op::BranchFalse(target) => {
                if self.expression.ok_or(Fault::ValueUnderflow)? == 0 {
                    self.pc = target;
                }
            }
            Op::Return if !self.calls.is_empty() => self.pc = self.calls.pop().unwrap(),
            Op::Return | Op::End => {
                self.pc = pc;
                self.status = Status::Halted;
                return Ok(Some(RunEvent::Halted));
            }
            Op::Native(opcode) => {
                let bindings = &H::NATIVES;
                let binding = bindings.get(opcode).ok_or(Fault::Native(opcode))?;
                let signature = binding.signature;
                let start = self
                    .arguments
                    .len()
                    .checked_sub(signature.arguments)
                    .ok_or(Fault::ArgumentUnderflow(signature.arguments))?;
                // Nested calls consume only their own tail, preserving outer args.
                let arguments = &self.arguments[start..];
                let result = (binding.handler)(host, arguments, memory).map_err(Fault::Host)?;
                self.arguments.truncate(start);
                match result {
                    NativeResult::Continue(value) => {
                        self.return_value(value, signature.returns_value, memory)?
                    }
                    NativeResult::Suspend => {
                        self.status = Status::Pending {
                            pc,
                            returns_value: signature.returns_value,
                        };
                        return Ok(Some(RunEvent::Suspended { opcode }));
                    }
                }
            }
        }
        Ok(None)
    }
}
