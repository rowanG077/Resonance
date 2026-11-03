//! Recover the compiler-initialized atlas windows without executing x86 code.
use super::*;

pub(super) fn read(image: &pe::Image<'_>) -> Result<BTreeMap<usize, (u32, u32)>> {
    // The controller selects a three-row burst bank and a four-row pulse bank
    // per scene at 0x4b0028 and 0x4b0084..0x4b029a. Recipe keys retain the
    // common catalogue identities; their values come from this executable.
    ensure!(
        image.slice(0x4b0028, 7)? == [0x8d, 0x9c, 0xdd, 0x34, 0xff, 0xff, 0xff]
            && image.slice(0x4b0081, 3)? == [0xc1, 0xe7, 0x05],
        "unknown cinematic atlas bank selection"
    );
    let words = decode(image.slice(0x4afee5, (0x4b0022 - 0x4afee5) as usize)?)?;
    Ok(words
        .chunks_exact(2)
        .enumerate()
        .map(|(i, pair)| (0x27c + i * 8, (pair[0], pair[1])))
        .collect())
}

fn decode(code: &[u8]) -> Result<Vec<u32>> {
    let mut registers = [None; 8];
    let mut rows = BTreeMap::new();
    let mut at = 0;
    while let Some(&opcode) = code.get(at) {
        let bytes = &code[at..];
        let size = match opcode {
            0xb8..=0xbf => {
                registers[usize::from(opcode - 0xb8)] = Some(pe::word(bytes, 1)?);
                5
            }
            0xdd if bytes.get(1) == Some(&0xd8) => 2, // discard the prior FP value
            0x8d if bytes.starts_with(&[0x8d, 0xb8, 0xf6, 0xfd, 0xff, 0xff]) => {
                registers[7] = None; // scene index, not a window parameter
                6
            }
            0x8d if bytes.starts_with(&[0x8d, 0x1c, 0x7f]) => {
                registers[3] = None; // three times scene index
                3
            }
            0x89 | 0xc7 => {
                let mode = *bytes.get(1).context("truncated window store")?;
                let (displacement, prefix) = match mode & 0xc7 {
                    0x45 => (i32::from(*bytes.get(2).context("missing disp8")? as i8), 3),
                    0x85 => (pe::word(bytes, 2)? as i32, 6),
                    _ => anyhow::bail!("window store does not address EBP"),
                };
                let register = usize::from((mode >> 3) & 7);
                let (value, size) = if opcode == 0xc7 {
                    ensure!(register == 0, "unexpected window store opcode");
                    (pe::word(bytes, prefix)?, prefix + 4)
                } else {
                    (
                        registers[register].context("window store uses an unknown register")?,
                        prefix,
                    )
                };
                ensure!(
                    (-0xd0..=-0x2c).contains(&displacement) && (displacement + 0xd0) % 4 == 0,
                    "window store exceeds its array"
                );
                ensure!(
                    rows.insert(displacement, value).is_none(),
                    "duplicate window store"
                );
                size
            }
            _ => anyhow::bail!("unexpected atlas initializer opcode {opcode:#x} at {at:#x}"),
        };
        at += size;
        ensure!(at <= code.len(), "truncated atlas initializer");
    }
    ensure!(rows.len() == 42, "incomplete cinematic atlas window array");
    Ok(rows.into_values().collect())
}
