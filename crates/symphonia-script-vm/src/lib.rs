//! SymphoniaScript execution with checked memory and explicit native host calls.
mod authored;
mod calculator;
mod memory;
mod native;
mod tasks;
pub use memory::Memory;
pub use native::{
    Host, NativeBinding, NativeBindings, NativeHandler, NativeResult, NativeSignature,
};
use std::{collections::VecDeque, sync::Arc};
use symphonia_script::authored::{
    CALL_FRAME_LIMIT as AUTHORED_CALL_LIMIT, LOCAL_SLOT_LIMIT as AUTHORED_LOCAL_LIMIT,
    SourceLocation, Type, VALUE_SLOT_LIMIT as AUTHORED_VALUE_LIMIT,
};
use symphonia_script::{Op, Program, Width};
pub use tasks::Tasks;
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
    #[error("VM was cancelled; its pending completion is invalid")]
    Cancelled,
    #[error("authored local index outside its frame")]
    Local,
    #[error("authored array index outside its bounds")]
    Bounds,
    #[error("authored execution exceeds its stack or local storage limit")]
    AuthoredLimit,
    #[error("authored call or return has an invalid stack shape")]
    CallShape,
    #[error("authored arithmetic overflow")]
    Overflow,
    #[error("authored shift count must be between 0 and 31")]
    Shift,
    #[error("authored floating-point operation produced a non-finite value")]
    NonFinite,
    #[error("authored value does not match {0:?}")]
    Type(Type),
    #[error("authored native {0:#04x} does not match its compiler declaration")]
    NativeDeclaration(u8),
    #[error("synchronous native {0:#04x} attempted to suspend")]
    UnexpectedSuspend(u8),
    #[error("pending completion is not a task join")]
    TaskCompletion,
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
    SuspendedTask { handle: i32 },
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
    Pending { pc: u32, completion: Completion },
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy)]
enum Completion {
    Native {
        returns_value: bool,
        result_type: Option<Type>,
    },
    Task {
        results: u16,
    },
}

#[derive(Debug)]
struct Frame {
    function: u16,
    local_base: usize,
    value_base: usize,
    argument_base: usize,
    return_pc: Option<u32>,
    call_pc: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFrame {
    pub function: String,
    pub pc: u32,
    pub location: Option<SourceLocation>,
}

pub struct Vm {
    program: Arc<Program>,
    pc: u32,
    status: Status,
    values: Vec<Value>,
    arguments: Vec<i32>,
    calls: Vec<u32>,
    locals: Vec<i32>,
    frames: Vec<Frame>,
    // Retail arg/branch read the slot just popped by expression_end.
    expression: Option<i32>,
    trace_limit: usize,
    trace: VecDeque<TraceEntry>,
}
impl Vm {
    pub fn new(program: Arc<Program>, entry: u32) -> Result<Self, VmError> {
        Self::with_arguments(program, entry, &[])
    }
    /// Start an authored function with flattened parameters. Legacy entries take none.
    pub fn with_arguments(
        program: Arc<Program>,
        entry: u32,
        arguments: &[i32],
    ) -> Result<Self, VmError> {
        Self::validate_arguments(&program, entry, arguments)?;
        let mut locals = Vec::new();
        let mut frames = Vec::new();
        if let Some(module) = program.authored() {
            let (index, function) = module
                .functions
                .iter()
                .enumerate()
                .find(|(_, function)| function.entry == entry)
                .ok_or(VmError {
                    pc: entry,
                    fault: Fault::Pc,
                })?;
            if usize::from(function.locals) > AUTHORED_LOCAL_LIMIT || index > u16::MAX as usize {
                return Err(VmError {
                    pc: entry,
                    fault: Fault::AuthoredLimit,
                });
            }
            locals.resize(usize::from(function.locals), 0);
            locals[..arguments.len()].copy_from_slice(arguments);
            frames.push(Frame {
                function: index as u16,
                local_base: 0,
                value_base: 0,
                argument_base: 0,
                return_pc: None,
                call_pc: entry,
            });
        }
        Ok(Self {
            program,
            pc: entry,
            status: Status::Running,
            values: Vec::with_capacity(64),
            arguments: Vec::with_capacity(ARGUMENT_STACK_LIMIT),
            calls: Vec::with_capacity(16),
            locals,
            frames,
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
    /// Stop execution and invalidate a pending completion. The host must separately
    /// cancel its owned operation/resources; cancellation never invokes a native.
    /// Returns false when the VM was already completed, failed, or cancelled.
    pub fn cancel(&mut self) -> bool {
        if !matches!(self.status, Status::Running | Status::Pending { .. }) {
            return false;
        }
        self.status = Status::Cancelled;
        self.values.clear();
        self.arguments.clear();
        self.calls.clear();
        self.locals.clear();
        self.frames.clear();
        self.expression = None;
        true
    }
    /// Scalar/flattened return slots are available after an authored root returns.
    pub fn result(&self) -> Option<Vec<i32>> {
        (self.program.authored().is_some() && matches!(self.status, Status::Halted))
            .then(|| self.values.iter().map(|value| value.number).collect())
    }
    /// Source information stays in the shared program, not every error allocation.
    pub fn source_trace(&self, pc: u32) -> Vec<SourceFrame> {
        let Some(module) = self.program.authored() else {
            return Vec::new();
        };
        let mut current_pc = pc;
        self.frames
            .iter()
            .rev()
            .map(|frame| {
                let result = SourceFrame {
                    function: module.functions[usize::from(frame.function)].name.clone(),
                    pc: current_pc,
                    location: module.locations.get(&current_pc).cloned(),
                };
                current_pc = frame.call_pc;
                result
            })
            .collect()
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
    /// Check the authored ABI before activating a prepared program.
    pub fn validate_bindings<H: Host>(program: &Program) -> Result<(), VmError> {
        if let Some(module) = program.authored() {
            for declaration in &module.natives {
                if H::AUTHORED_NATIVES
                    .get(declaration.opcode)
                    .and_then(|binding| binding.declaration)
                    != Some(*declaration)
                {
                    return Err(VmError {
                        pc: program.entry(),
                        fault: Fault::NativeDeclaration(declaration.opcode),
                    });
                }
            }
        }
        Ok(())
    }
    /// Budget is per resume, never cumulative. Budget exhaustion is a terminal
    /// script fault so a runaway event cannot silently change update ordering.
    pub fn run<H: Host>(
        &mut self,
        host: &mut H,
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
            Status::Cancelled => {
                return Err(VmError {
                    pc: self.pc,
                    fault: Fault::Cancelled,
                });
            }
            Status::Running => {}
        }
        if let Err(mut error) = Self::validate_bindings::<H>(&self.program) {
            self.status = Status::Failed;
            error.pc = self.pc;
            return Err(error);
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
        if matches!(self.status, Status::Cancelled) {
            return Err(VmError {
                pc: self.pc,
                fault: Fault::Cancelled,
            });
        }
        let Status::Pending {
            pc,
            completion:
                Completion::Native {
                    returns_value,
                    result_type,
                },
        } = self.status
        else {
            return Err(VmError {
                pc: self.pc,
                fault: Fault::NotSuspended,
            });
        };
        self.return_value(result, returns_value, result_type, memory)
            .map_err(|fault| VmError { pc, fault })?;
        self.status = Status::Running;
        Ok(())
    }
    /// Supply an owned child's flattened result exactly once after a pending join.
    pub fn complete_task(&mut self, results: &[i32]) -> Result<(), VmError> {
        if matches!(self.status, Status::Cancelled) {
            return Err(VmError {
                pc: self.pc,
                fault: Fault::Cancelled,
            });
        }
        let Status::Pending {
            pc,
            completion: Completion::Task { results: count },
        } = self.status
        else {
            return Err(VmError {
                pc: self.pc,
                fault: Fault::TaskCompletion,
            });
        };
        self.task_result(results, count)
            .map_err(|fault| VmError { pc, fault })?;
        self.status = Status::Running;
        Ok(())
    }
    fn task_result(&mut self, results: &[i32], count: u16) -> Result<(), Fault> {
        if results.len() != usize::from(count) {
            return Err(Fault::CallShape);
        }
        if self.values.len() + results.len() > AUTHORED_VALUE_LIMIT {
            return Err(Fault::AuthoredLimit);
        }
        self.values
            .extend(results.iter().copied().map(Value::scalar));
        Ok(())
    }
    fn return_value(
        &mut self,
        value: Option<i32>,
        expected: bool,
        result_type: Option<Type>,
        memory: &mut Memory,
    ) -> Result<(), Fault> {
        if value.is_some() != expected {
            return Err(Fault::NativeResult);
        }
        if let Some(value) = value {
            if let Some(ty) = result_type {
                if ty.slots() != 1 {
                    return Err(Fault::NativeResult);
                }
                self.check_argument(&[value], ty)?;
            }
            self.push(Value::scalar(value))?;
            if self.program.authored().is_none() {
                memory.write(0x20, Width::S32, value)?;
            }
        }
        Ok(())
    }
    fn push(&mut self, value: Value) -> Result<(), Fault> {
        let authored = self.program.authored().is_some();
        if self.values.len() >= if authored { AUTHORED_VALUE_LIMIT } else { 64 } {
            return Err(if authored {
                Fault::AuthoredLimit
            } else {
                Fault::ValueOverflow
            });
        }
        self.values.push(value);
        Ok(())
    }
    fn pop(&mut self) -> Result<Value, Fault> {
        if self
            .frames
            .last()
            .is_some_and(|frame| self.values.len() <= frame.value_base)
        {
            return Err(Fault::ValueUnderflow);
        }
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
                let is_authored = self.program.authored().is_some();
                let bindings = if is_authored {
                    &H::AUTHORED_NATIVES
                } else {
                    &H::NATIVES
                };
                let binding = bindings.get(opcode).ok_or(Fault::Native(opcode))?;
                let signature = binding.signature;
                let start = self
                    .arguments
                    .len()
                    .checked_sub(signature.arguments)
                    .ok_or(Fault::ArgumentUnderflow(signature.arguments))?;
                if self
                    .frames
                    .last()
                    .is_some_and(|frame| start < frame.argument_base)
                {
                    return Err(Fault::ArgumentUnderflow(signature.arguments));
                }
                // Nested calls consume only their own tail, preserving outer args.
                let arguments = &self.arguments[start..];
                if let Some(declaration) = binding.declaration {
                    let mut offset = 0;
                    for &ty in declaration.parameters {
                        self.check_argument(&arguments[offset..offset + ty.slots()], ty)?;
                        offset += ty.slots();
                    }
                }
                let result = (binding.handler)(host, arguments, memory).map_err(Fault::Host)?;
                self.arguments.truncate(start);
                match result {
                    NativeResult::Continue(value) => self.return_value(
                        value,
                        signature.returns_value,
                        binding.declaration.and_then(|d| d.result),
                        memory,
                    )?,
                    NativeResult::Values(values) => {
                        let ty = binding
                            .declaration
                            .and_then(|declaration| declaration.result)
                            .filter(|ty| is_authored && ty.slots() == values.len())
                            .ok_or(Fault::NativeResult)?;
                        self.check_argument(&values, ty)?;
                        self.task_result(&values, ty.slots() as u16)?;
                    }
                    NativeResult::Suspend => {
                        if binding.declaration.is_some_and(|d| !d.suspends) {
                            return Err(Fault::UnexpectedSuspend(opcode));
                        }
                        self.status = Status::Pending {
                            pc,
                            completion: Completion::Native {
                                returns_value: signature.returns_value,
                                result_type: binding.declaration.and_then(|d| d.result),
                            },
                        };
                        return Ok(Some(RunEvent::Suspended { opcode }));
                    }
                }
            }
            Op::LoadLocal(index) => {
                let value = self.locals[self.local(index)?];
                self.push(Value::scalar(value))?;
            }
            Op::StoreLocal(index) => {
                let index = self.local(index)?;
                self.locals[index] = self.pop()?.number;
            }
            Op::LoadLocalIndexed { base, len } => {
                let index = self.indexed_local(base, len)?;
                self.push(Value::scalar(self.locals[index]))?;
            }
            Op::StoreLocalIndexed { base, len } => {
                let value = self.pop()?.number;
                let index = self.indexed_local(base, len)?;
                self.locals[index] = value;
            }
            Op::Pop => {
                self.pop()?;
            }
            Op::ArgumentValue => {
                if self.arguments.len() == ARGUMENT_STACK_LIMIT {
                    return Err(Fault::ArgumentOverflow);
                }
                let value = self.pop()?.number;
                self.arguments.push(value);
            }
            Op::BranchFalseStack(target) => {
                let condition = self.pop()?.number;
                self.check_type(condition, Type::Bool)?;
                if condition == 0 {
                    self.pc = target;
                }
            }
            Op::CallFunction(index) => self.call_function(index, pc, next)?,
            Op::SpawnFunction(index) => {
                let function = self
                    .program
                    .authored()
                    .and_then(|module| module.functions.get(usize::from(index)))
                    .ok_or(Fault::Pc)?;
                let count = usize::from(function.parameters);
                let start = self
                    .arguments
                    .len()
                    .checked_sub(count)
                    .ok_or(Fault::ArgumentUnderflow(count))?;
                if self
                    .frames
                    .last()
                    .is_some_and(|frame| start < frame.argument_base)
                {
                    return Err(Fault::ArgumentUnderflow(count));
                }
                let handle = host
                    .spawn(index, &self.arguments[start..])
                    .map_err(Fault::Host)?;
                self.arguments.truncate(start);
                self.push(Value::scalar(handle))?;
            }
            Op::JoinTask { results } => {
                let handle = self.pop()?.number;
                if let Some(values) = host.join(handle).map_err(Fault::Host)? {
                    self.task_result(&values, results)?;
                } else {
                    self.status = Status::Pending {
                        pc,
                        completion: Completion::Task { results },
                    };
                    return Ok(Some(RunEvent::SuspendedTask { handle }));
                }
            }
            Op::ReturnValues(count) => {
                if self.return_values(count)? {
                    self.pc = pc;
                    return Ok(Some(RunEvent::Halted));
                }
            }
            Op::Unary(op) => self.unary(op)?,
            Op::Binary(op) => self.binary(op)?,
            Op::Convert(op) => self.convert(op)?,
        }
        Ok(None)
    }
}
