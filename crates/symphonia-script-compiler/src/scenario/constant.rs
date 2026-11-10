//! Read-only evaluation of original instruction encodings; never executes calls,
//! shared memory accesses or reference writes.
use symphonia_script::semantics;

pub(super) fn unary(op: u8, value: i32) -> Option<i32> {
    Some(match op {
        5 => value.wrapping_neg(),
        6 => value,
        7 => !value,
        8 => i32::from(value == 0),
        _ => return None,
    })
}

pub(super) fn binary(op: u8, left: i32, right: i32) -> Option<i32> {
    let spec = semantics::calculator(op)?;
    if spec.writes_reference || spec.inputs != 2 || spec.form != semantics::OperatorForm::Binary {
        return None;
    }
    semantics::calculator_binary_value(op, left, right)
}

pub(super) fn assembly(source: &str) -> Option<i32> {
    let mut stack = Vec::new();
    for line in source.lines() {
        let (opcode, operand) = line.split_once(' ')?;
        let n = super::catalog::number(operand.trim())?;
        if matches!(opcode, "push.s8" | "push.s16" | "push.u16" | "push.s32") {
            stack.push(i32::try_from(n).ok()?);
        } else if opcode == "calc" {
            let op = u8::try_from(n).ok()?;
            let right = stack.pop()?;
            let value = if (5..=8).contains(&op) {
                unary(op, right)?
            } else {
                binary(op, stack.pop()?, right)?
            };
            stack.push(value);
        } else {
            return None;
        }
    }
    (stack.len() == 1).then(|| stack[0])
}
