//! Evaluate editable message expressions without decoding a bytecode program.
use crate::{Fault, Memory, Value};
use symphonia_script::message::Expression;

pub fn evaluate(expression: &Expression, memory: &mut Memory) -> Result<i32, Fault> {
    fn value(
        expression: &Expression,
        memory: &mut Memory,
        budget: &mut u32,
    ) -> Result<Value, Fault> {
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
                    .map(|argument| value(argument, memory, budget))
                    .collect::<Result<Vec<_>, _>>()?;
                crate::calculator::calculate(spec.opcode, &mut operands, &mut None, memory)?;
                operands.pop().ok_or(Fault::ValueUnderflow)
            }
            Expression::Call {
                function,
                arguments,
            } => {
                for argument in arguments {
                    value(argument, memory, budget)?;
                }
                // Dialogue's expression host has no game-service bindings. Keep
                // unsupported calls explicit rather than guessing their value.
                Err(Fault::Host(format!("unsupported dialogue call {function}")))
            }
        }
    }
    Ok(value(expression, memory, &mut 1024)?.number)
}
