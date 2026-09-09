use crate::{Fault, Memory, Value, Vm};

fn store(memory: &mut Memory, value: Value) -> Result<(), Fault> {
    if let Some((offset, width)) = value.reference {
        memory.write(offset, width, value.number)?;
    }
    Ok(())
}
impl Vm {
    pub(super) fn calculate(&mut self, op: u8, memory: &mut Memory) -> Result<(), Fault> {
        if op == 0 {
            self.expression = Some(self.pop()?.number);
            return Ok(());
        }
        if (1..=8).contains(&op) {
            let value = self.values.last_mut().ok_or(Fault::ValueUnderflow)?;
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
        let right = self.pop()?.number;
        let left = self.values.last_mut().ok_or(Fault::ValueUnderflow)?;
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
        left.number = match op {
            0x10 => right,
            0x11 | 0x31 => a.wrapping_add(right),
            0x12 | 0x32 => a.wrapping_sub(right),
            0x13 | 0x33 => a.wrapping_mul(right),
            0x14 | 0x34 if right == 0 => return Err(Fault::DivisionByZero),
            0x14 | 0x34 => a.wrapping_div(right),
            0x15 | 0x35 if right == 0 => return Err(Fault::DivisionByZero),
            0x15 | 0x35 => a.wrapping_rem(right),
            0x16 | 0x36 => a & right,
            0x17 | 0x37 => a | right,
            0x18 | 0x38 => a ^ right,
            // PowerPC variable shifts use six count bits, with >=32 shifting
            // out the full word (Rust wrapping_shl would incorrectly mask to 5).
            0x19 | 0x39 => a.checked_shl((right as u32) & 63).unwrap_or(0),
            0x1a | 0x3a => a
                .checked_shr((right as u32) & 63)
                .unwrap_or(if a < 0 { -1 } else { 0 }),
            0x20 => i32::from(a == right),
            0x21 => i32::from(a != right),
            0x22 => i32::from(a <= right),
            0x23 => i32::from(a >= right),
            0x24 => i32::from(a > right),
            0x25 => i32::from(a < right),
            0x26 => i32::from(a != 0 && right != 0),
            0x27 => i32::from(a != 0 || right != 0),
            _ => return Err(Fault::Calculator(op)),
        };
        if (0x10..=0x1a).contains(&op) {
            store(memory, *left)?;
        }
        Ok(())
    }
}
