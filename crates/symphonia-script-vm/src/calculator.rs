use crate::{Fault, Memory, Value, Vm};

fn store(memory: &mut Memory, value: Value) -> Result<(), Fault> {
    if let Some((offset, width)) = value.reference {
        memory.write(offset, width, value.number)?;
    }
    Ok(())
}
impl Vm {
    pub(super) fn calculate(&mut self, op: u8, memory: &mut Memory) -> Result<(), Fault> {
        calculate(op, &mut self.values, &mut self.expression, memory)
    }
}

pub(crate) fn calculate(
    op: u8,
    values: &mut Vec<Value>,
    expression: &mut Option<i32>,
    memory: &mut Memory,
) -> Result<(), Fault> {
    if op == 0 {
        *expression = Some(values.pop().ok_or(Fault::ValueUnderflow)?.number);
        return Ok(());
    }
    if (1..=8).contains(&op) {
        let value = values.last_mut().ok_or(Fault::ValueUnderflow)?;
        let old = value.number;
        value.number = match op {
            1 | 3 => old.wrapping_add(1),
            2 | 4 => old.wrapping_sub(1),
            5 => old.wrapping_neg(),
            6 => old,
            7 => !old,
            8 => i32::from(old == 0),
            _ => unreachable!(),
        };
        // Only ++/-- store. Postfix leaves the old value on the stack.
        if op <= 4 {
            store(memory, *value)?;
        }
        if op <= 2 {
            value.number = old;
        }
        return Ok(());
    }
    let right = values.pop().ok_or(Fault::ValueUnderflow)?.number;
    let left = values.last_mut().ok_or(Fault::ValueUnderflow)?;
    if op == 0x0f {
        if let Some((offset, width)) = left.reference {
            let address = i64::from(offset) + i64::from(right) * width.bytes() as i64;
            let address = u16::try_from(address).map_err(|_| Fault::Index)?;
            left.number = memory.read(address, width)?;
            left.reference = Some((address, width));
        }
        return Ok(());
    }
    let a = left.number;
    left.number = symphonia_script::semantics::calculator_binary_value(op, a, right)
        .ok_or(Fault::Calculator(op))?;

    if (0x10..=0x1a).contains(&op) {
        store(memory, *left)?;
    }
    Ok(())
}
