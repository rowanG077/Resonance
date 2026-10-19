//! Decode character learning lists.
use crate::dol;
use anyhow::{Context, Result};

const LEARNING: u32 = 0x80202dc8;

pub(crate) fn learning(executable: &[u8]) -> Result<Vec<Vec<u8>>> {
    dol::slice(executable, LEARNING, 11 * 41)?
        .chunks_exact(41)
        .map(|row| {
            Ok(row
                .get(1..1 + usize::from(row[0]))
                .context("learning count exceeds table capacity")?
                .to_vec())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn executable(address: u32, bytes: &[u8]) -> Vec<u8> {
        let mut dol = vec![0; 0x100];
        dol[..4].copy_from_slice(&0x100_u32.to_be_bytes());
        dol[0x48..0x4c].copy_from_slice(&address.to_be_bytes());
        dol[0x90..0x94].copy_from_slice(&(bytes.len() as u32).to_be_bytes());
        dol.extend_from_slice(bytes);
        dol
    }

    #[test]
    fn learning_keeps_only_active_techniques() -> Result<()> {
        let mut bytes = vec![0; 11 * 41];
        bytes[..3].copy_from_slice(&[2, 66, 98]);
        bytes[40] = 255;
        let lists = learning(&executable(LEARNING, &bytes))?;
        assert_eq!(lists[0], [66, 98]);
        assert!(lists[1].is_empty());
        bytes[0] = 41;
        assert!(learning(&executable(LEARNING, &bytes)).is_err());
        assert!(learning(&executable(LEARNING, &bytes[..bytes.len() - 1])).is_err());
        Ok(())
    }
}
