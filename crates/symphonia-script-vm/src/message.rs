//! Evaluate editable message expressions without decoding a bytecode program.
use crate::{Fault, Memory, NativeResult, Value};
use symphonia_script::message::Expression;

pub fn evaluate(expression: &Expression, memory: &mut Memory) -> Result<i32, Fault> {
    evaluate_with_host(expression, memory, &mut |function, _, _| {
        Err(format!("unsupported dialogue call {function}"))
    })
}

/// Native calls use the ordinary host bindings. A root call that deliberately
/// pushes nothing exposes the retail terminal expression-end depth alias (-1).
/// Missing values inside a larger expression remain an explicit underflow.
pub fn evaluate_with_host<F>(
    expression: &Expression,
    memory: &mut Memory,
    host: &mut F,
) -> Result<i32, Fault>
where
    F: FnMut(&str, &[i32], &mut Memory) -> Result<NativeResult, String>,
{
    fn value<F>(
        expression: &Expression,
        memory: &mut Memory,
        budget: &mut u32,
        root: bool,
        host: &mut F,
    ) -> Result<Value, Fault>
    where
        F: FnMut(&str, &[i32], &mut Memory) -> Result<NativeResult, String>,
    {
        *budget = budget.checked_sub(1).ok_or(Fault::Budget(1024))?;
        match expression {
            Expression::Constant(number) => Ok(Value::scalar(*number)),
            Expression::State { offset, width } => Ok(Value {
                number: memory.read(*offset, *width)?,
                reference: Some((*offset, *width)),
            }),
            Expression::Calculate {
                operator,
                arguments,
            } => {
                let spec = symphonia_script::semantics::calculator(*operator)
                    .filter(|spec| spec.opcode != 0 && usize::from(spec.inputs) == arguments.len())
                    .ok_or(Fault::Calculator(*operator))?;
                let mut operands = arguments
                    .iter()
                    .map(|argument| value(argument, memory, budget, false, host))
                    .collect::<Result<Vec<_>, _>>()?;
                crate::calculator::calculate(spec.opcode, &mut operands, &mut None, memory)?;
                operands.pop().ok_or(Fault::ValueUnderflow)
            }
            Expression::Call {
                function,
                arguments,
            } => {
                let arguments = arguments
                    .iter()
                    .map(|argument| {
                        value(argument, memory, budget, false, host).map(|value| value.number)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                match host(function, &arguments, memory).map_err(Fault::Host)? {
                    NativeResult::Continue(Some(number)) => {
                        memory.write(0x20, symphonia_script::Width::S32, number)?;
                        Ok(Value::scalar(number))
                    }
                    NativeResult::LegacyNoResult if root => Ok(Value::scalar(-1)),
                    NativeResult::LegacyNoResult => Err(Fault::ValueUnderflow),
                    _ => Err(Fault::NativeResult),
                }
            }
        }
    }
    Ok(value(expression, memory, &mut 1024, true, host)?.number)
}
