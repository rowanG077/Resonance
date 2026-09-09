//! Bounded character-animation decompression.
use anyhow::{Context, Result, ensure};

pub(crate) fn decode(data: &[u8]) -> Result<Vec<u8>> {
    let header = data.get(..9).context("truncated compressed resource")?;
    let packed = u32::from_le_bytes(header[1..5].try_into()?) as usize;
    let size = u32::from_le_bytes(header[5..9].try_into()?) as usize;
    ensure!(
        (1..=16 * 1024 * 1024).contains(&size),
        "invalid expanded resource size"
    );
    let input = data
        .get(9..9 + packed)
        .context("compressed resource exceeds its container")?;
    if header[0] == 0 {
        ensure!(packed == size, "stored resource size mismatch");
        return Ok(input.to_vec());
    }
    ensure!(
        matches!(header[0], 1 | 3),
        "unsupported compression method {}",
        header[0]
    );
    let mut window = [0u8; 4096];
    let mut initialized = [false; 4096];
    let mut position = if header[0] == 1 { 4078 } else { 4079 };
    initialized[..position].fill(true);
    for value in 0..256 {
        for offset in [0, 2, 4, 6] {
            window[value * 8 + offset] = value as u8;
        }
        let start = 2048 + value * 7;
        window[start..start + 7].copy_from_slice(&[
            value as u8,
            255,
            value as u8,
            255,
            value as u8,
            255,
            value as u8,
        ]);
    }
    let mut source = input.iter().copied();
    let mut output = Vec::with_capacity(size);
    let mut flags = 0u16;
    loop {
        flags >>= 1;
        if flags & 0x100 == 0 {
            let Some(byte) = source.next() else { break };
            flags = u16::from(byte) | 0xff00;
        }
        // Unused bits of the last flag byte have no tokens.
        let Some(first) = source.next() else { break };
        let (start, length, repeated) = if flags & 1 != 0 {
            (0, 1, Some(first))
        } else {
            let second = source.next().context("truncated back-reference")?;
            let start = usize::from(first) | (usize::from(second & 0xf0) << 4);
            let length = usize::from(second & 15) + 3;
            if header[0] == 3 && length == 18 {
                if start < 256 {
                    (
                        0,
                        start + 19,
                        Some(source.next().context("truncated repeated byte")?),
                    )
                } else {
                    (0, (start >> 8) + 3, Some(start as u8))
                }
            } else {
                (start, length, None)
            }
        };
        ensure!(
            output.len() + length <= size,
            "compressed token exceeds expanded size"
        );
        for i in 0..length {
            let value = if let Some(value) = repeated {
                value
            } else {
                let index = (start + i) & 4095;
                ensure!(
                    initialized[index],
                    "back-reference reads uninitialized dictionary data"
                );
                window[index]
            };
            output.push(value);
            window[position] = value;
            initialized[position] = true;
            position = (position + 1) & 4095;
        }
    }
    ensure!(
        output.len() == size,
        "expanded resource size mismatch: {} != {size}",
        output.len()
    );
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn resource(method: u8, payload: &[u8], size: u32) -> Vec<u8> {
        let mut data = vec![method];
        data.extend((payload.len() as u32).to_le_bytes());
        data.extend(size.to_le_bytes());
        data.extend(payload);
        data
    }
    #[test]
    fn overlapping_reference_wraps_the_dictionary_after_a_literal() {
        // First literal lands at 4078; copy it and each newly written byte.
        let data = resource(1, &[1, b'A', 0xee, 0xf2], 6);
        assert_eq!(decode(&data).unwrap(), b"AAAAAA");
        assert!(decode(&resource(1, &[0, 0xff, 0xff], 18)).is_err());
    }
    #[test]
    fn variant_three_expands_both_repeated_byte_forms_and_checks_sizes() {
        assert_eq!(
            decode(&resource(3, &[0, 0, 15, 7], 19)).unwrap(),
            vec![7; 19]
        );
        assert_eq!(decode(&resource(3, &[0, 9, 31], 4)).unwrap(), vec![9; 4]);
        assert!(decode(&resource(3, &[0, 0, 15, 7], 18)).is_err());
        assert!(decode(&resource(1, &[0, 0], 3)).is_err());
    }
}
