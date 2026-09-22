use super::{AUTHORED_CALL_LIMIT, AUTHORED_LOCAL_LIMIT, Fault, Frame, Status, Value, Vm, VmError};
use symphonia_script::{
    Program,
    authored::{BinaryOp, Conversion, Type, UnaryOp, ValueLayout},
};

fn check_type(program: &Program, value: i32, ty: Type) -> Result<(), Fault> {
    let valid = match ty {
        Type::F32 => f32::from_bits(value as u32).is_finite(),
        Type::Bool => matches!(value, 0 | 1),
        Type::Ticks | Type::TextReference { .. } => value >= 0,
        Type::String => {
            value >= 0
                && program
                    .authored()
                    .is_some_and(|module| (value as usize) < module.strings.len())
        }
        Type::Message => {
            value >= 0
                && program
                    .authored()
                    .is_some_and(|module| (value as usize) < module.texts.len())
        }
        // Handle lifetime and asset identity are checked by the owning host.
        Type::I32 | Type::Handle(_) | Type::Asset(_) | Type::Collection { .. } => true,
        Type::Record { .. } | Type::Array { .. } => false,
    };
    if valid { Ok(()) } else { Err(Fault::Type(ty)) }
}

fn check_argument(program: &Program, values: &[i32], ty: Type) -> Result<(), Fault> {
    if values.len() != ty.slots() {
        return Err(Fault::Type(ty));
    }
    match ty {
        Type::Record { fields, .. } => {
            let mut offset = 0;
            for field in fields {
                let end = offset + field.ty.slots();
                check_argument(program, &values[offset..end], field.ty)?;
                offset = end;
            }
            return Ok(());
        }
        Type::Array { element, .. } => {
            for value in values.chunks_exact(element.slots()) {
                check_argument(program, value, *element)?;
            }
            return Ok(());
        }
        _ => {}
    }
    check_type(program, values[0], ty)?;
    if ty == Type::Message {
        let module = program.authored().ok_or(Fault::Type(ty))?;
        let parameters = module
            .templates
            .get(&(values[0] as u32))
            .map(|template| template.parameters.as_slice())
            .unwrap_or(&[]);
        for (parameter, &value) in parameters.iter().zip(&values[1..]) {
            check_type(program, value, parameter.ty)?;
        }
        if values[1 + parameters.len()..]
            .iter()
            .any(|value| *value != 0)
        {
            return Err(Fault::Type(ty));
        }
    }
    Ok(())
}

fn check_layout(program: &Program, values: &[i32], layout: &ValueLayout) -> Result<(), Fault> {
    match layout {
        ValueLayout::Scalar(ty) => check_argument(program, values, *ty),
        ValueLayout::Sequence(fields) => {
            let mut offset = 0;
            for field in fields {
                let end = offset + field.slots().ok_or(Fault::CallShape)?;
                check_layout(program, &values[offset..end], field)?;
                offset = end;
            }
            Ok(())
        }
        ValueLayout::Array { element, len } => {
            let width = element.slots().ok_or(Fault::CallShape)?;
            for index in 0..usize::from(*len) {
                check_layout(
                    program,
                    &values[index * width..(index + 1) * width],
                    element,
                )?;
            }
            Ok(())
        }
        ValueLayout::Variants(variants) => {
            let variant = variants.get(values[0] as usize).ok_or(Fault::CallShape)?;
            let end = 1 + variant.slots().ok_or(Fault::CallShape)?;
            check_layout(program, &values[1..end], variant)?;
            if values[end..].iter().any(|value| *value != 0) {
                return Err(Fault::CallShape);
            }
            Ok(())
        }
    }
}

impl Vm {
    /// Validate host-supplied flattened arguments before preparation or activation.
    pub fn validate_arguments(
        program: &Program,
        entry: u32,
        arguments: &[i32],
    ) -> Result<(), VmError> {
        let error = |fault| VmError { pc: entry, fault };
        if program.instruction(entry).is_none() {
            return Err(error(Fault::Pc));
        }
        if let Some(module) = program.authored() {
            let function = module
                .functions
                .iter()
                .find(|function| function.entry == entry)
                .ok_or_else(|| error(Fault::Pc))?;
            if usize::from(function.parameters) != arguments.len() {
                return Err(error(Fault::CallShape));
            }
            check_layout(program, arguments, &function.parameter_layout).map_err(error)
        } else if arguments.is_empty() {
            Ok(())
        } else {
            Err(error(Fault::CallShape))
        }
    }

    pub(super) fn check_type(&self, value: i32, ty: Type) -> Result<(), Fault> {
        check_type(&self.program, value, ty)
    }

    pub(super) fn check_argument(&self, values: &[i32], ty: Type) -> Result<(), Fault> {
        check_argument(&self.program, values, ty)
    }

    pub(super) fn local(&self, index: u16) -> Result<usize, Fault> {
        let frame = self.frames.last().ok_or(Fault::Local)?;
        let index = frame.local_base + usize::from(index);
        if index < self.locals.len() {
            Ok(index)
        } else {
            Err(Fault::Local)
        }
    }

    pub(super) fn indexed_local(&mut self, base: u16, len: u16) -> Result<usize, Fault> {
        let index = self.pop()?.number;
        if index < 0 || index >= i32::from(len) {
            return Err(Fault::Bounds);
        }
        self.local(base.checked_add(index as u16).ok_or(Fault::Local)?)
    }

    pub(super) fn call_function(&mut self, index: u16, pc: u32, next: u32) -> Result<(), Fault> {
        let function = self
            .program
            .authored()
            .and_then(|module| module.functions.get(usize::from(index)))
            .ok_or(Fault::Pc)?;
        let start = self
            .arguments
            .len()
            .checked_sub(usize::from(function.parameters))
            .ok_or(Fault::ArgumentUnderflow(usize::from(function.parameters)))?;
        if self
            .frames
            .last()
            .is_some_and(|frame| start < frame.argument_base)
        {
            return Err(Fault::ArgumentUnderflow(usize::from(function.parameters)));
        }
        let base = self.locals.len();
        let end = base + usize::from(function.locals);
        if end > AUTHORED_LOCAL_LIMIT || self.frames.len() >= AUTHORED_CALL_LIMIT {
            return Err(Fault::AuthoredLimit);
        }
        self.locals.resize(end, 0);
        self.locals[base..base + usize::from(function.parameters)]
            .copy_from_slice(&self.arguments[start..]);
        self.arguments.truncate(start);
        self.frames.push(Frame {
            function: index,
            local_base: base,
            value_base: self.values.len(),
            argument_base: start,
            return_pc: Some(next),
            call_pc: pc,
        });
        self.pc = function.entry;
        Ok(())
    }

    pub(super) fn return_values(&mut self, count: u16) -> Result<bool, Fault> {
        let frame = self.frames.last().ok_or(Fault::CallShape)?;
        let function = &self.program.authored().ok_or(Fault::CallShape)?.functions
            [usize::from(frame.function)];
        if function.results != count
            || self.values.len() != frame.value_base + usize::from(count)
            || self.arguments.len() != frame.argument_base
        {
            return Err(Fault::CallShape);
        }
        let frame = self.frames.pop().unwrap();
        self.locals.truncate(frame.local_base);
        if let Some(pc) = frame.return_pc {
            self.pc = pc;
            Ok(false)
        } else {
            self.status = Status::Halted;
            Ok(true)
        }
    }

    pub(super) fn unary(&mut self, op: UnaryOp) -> Result<(), Fault> {
        let value = self.pop()?.number;
        let value = match op {
            UnaryOp::NegI32 => value.checked_neg().ok_or(Fault::Overflow)?,
            UnaryOp::NegF32 => float(-f32::from_bits(value as u32))?,
            UnaryOp::Not => {
                self.check_type(value, Type::Bool)?;
                i32::from(value == 0)
            }
            UnaryOp::BitNot => !value,
        };
        self.push(Value::scalar(value))
    }

    pub(super) fn binary(&mut self, op: BinaryOp) -> Result<(), Fault> {
        use BinaryOp::*;
        let b = self.pop()?.number;
        let a = self.pop()?.number;
        let value = match op {
            AddI32 => a.checked_add(b).ok_or(Fault::Overflow)?,
            SubI32 => a.checked_sub(b).ok_or(Fault::Overflow)?,
            MulI32 => a.checked_mul(b).ok_or(Fault::Overflow)?,
            DivI32 | RemI32 => {
                if b == 0 {
                    return Err(Fault::DivisionByZero);
                }
                if op == DivI32 {
                    a.checked_div(b)
                } else {
                    a.checked_rem(b)
                }
                .ok_or(Fault::Overflow)?
            }
            Shl | Shr => {
                if !(0..32).contains(&b) {
                    return Err(Fault::Shift);
                }
                if op == Shl { a << b } else { a >> b }
            }
            BitAnd => a & b,
            BitOr => a | b,
            BitXor => a ^ b,
            Eq => i32::from(a == b),
            Ne => i32::from(a != b),
            LtI32 => i32::from(a < b),
            LeI32 => i32::from(a <= b),
            GtI32 => i32::from(a > b),
            GeI32 => i32::from(a >= b),
            _ => {
                let a = f32::from_bits(a as u32);
                let b = f32::from_bits(b as u32);
                if !a.is_finite() || !b.is_finite() {
                    return Err(Fault::NonFinite);
                }
                match op {
                    AddF32 => float(a + b)?,
                    SubF32 => float(a - b)?,
                    MulF32 => float(a * b)?,
                    DivF32 | RemF32 => {
                        if b == 0.0 {
                            return Err(Fault::DivisionByZero);
                        }
                        float(if op == DivF32 { a / b } else { a % b })?
                    }
                    EqF32 => i32::from(a == b),
                    NeF32 => i32::from(a != b),
                    LtF32 => i32::from(a < b),
                    LeF32 => i32::from(a <= b),
                    GtF32 => i32::from(a > b),
                    GeF32 => i32::from(a >= b),
                    _ => unreachable!(),
                }
            }
        };
        self.push(Value::scalar(value))
    }

    pub(super) fn convert(&mut self, op: Conversion) -> Result<(), Fault> {
        let value = self.pop()?.number;
        let value = match op {
            Conversion::I32ToTicks => {
                self.check_type(value, Type::Ticks)?;
                value
            }
            Conversion::I32ToF32 => float(value as f32)?,
            Conversion::F32ToI32 => {
                let value = f32::from_bits(value as u32);
                if !value.is_finite() {
                    return Err(Fault::NonFinite);
                }
                // Compare as f64: i32::MAX rounds upward when represented as f32.
                if f64::from(value) < f64::from(i32::MIN)
                    || f64::from(value) >= f64::from(i32::MAX) + 1.0
                {
                    return Err(Fault::Overflow);
                }
                value as i32
            }
        };
        self.push(Value::scalar(value))
    }
}

fn float(value: f32) -> Result<i32, Fault> {
    if value.is_finite() {
        Ok(value.to_bits() as i32)
    } else {
        Err(Fault::NonFinite)
    }
}
