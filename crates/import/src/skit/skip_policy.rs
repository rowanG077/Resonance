//! Extract the bounded skit skip-denial branch from the GQSEAF executable.
//!
//! Validate the instruction sequence: each matching ID reaches `li r5,0`;
//! every other ID preserves caller r5 before
//! `stb r5,-30831(r13)`. This is offline extraction, not a runtime interpreter.
use anyhow::{Result, ensure};
use std::collections::BTreeSet;

const START: u32 = 0x8009_b2ac;
const BYTES: usize = 84;

pub(super) fn read(executable: &[u8]) -> Result<BTreeSet<u16>> {
    decode(crate::dol::slice(executable, START, BYTES)?)
}

fn decode(block: &[u8]) -> Result<BTreeSet<u16>> {
    ensure!(block.len() == BYTES, "invalid skit skip policy extent");
    let word = |offset| crate::read::u32(block, offset);
    ensure!(
        word(4)? == 0x988d_8792 && word(76)? == 0x38a0_0000 && word(80)? == 0x98ad_8791,
        "unsupported skit skip permission storage"
    );
    let mut ids = BTreeSet::new();
    for offset in [0, 12, 20, 28, 36, 44, 52, 60, 68] {
        let comparison = word(offset)?;
        ensure!(
            comparison & 0xffff_0000 == 0x2c03_0000,
            "unsupported skit skip comparison"
        );
        let id = comparison as u16;
        ensure!(
            (1..=860).contains(&id) && ids.insert(id),
            "invalid or duplicate skit skip ID {id}"
        );
        let branch = if offset == 0 { 8 } else { offset + 4 };
        let expected = if offset == 68 {
            // Last non-match bypasses the clear; equality falls through to it.
            0x4082_0008
        } else {
            0x4182_0000 | (76 - branch) as u32
        };
        ensure!(word(branch)? == expected, "unsupported skit skip branch");
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skip_policy_extracts_ids_and_rejects_changed_control_flow() -> Result<()> {
        let words = [
            0x2c03_000b_u32,
            0x988d_8792,
            0x4182_0044,
            0x2c03_000c,
            0x4182_003c,
            0x2c03_000d,
            0x4182_0034,
            0x2c03_000e,
            0x4182_002c,
            0x2c03_000f,
            0x4182_0024,
            0x2c03_0010,
            0x4182_001c,
            0x2c03_0011,
            0x4182_0014,
            0x2c03_0012,
            0x4182_000c,
            0x2c03_0013,
            0x4082_0008,
            0x38a0_0000,
            0x98ad_8791,
        ];
        let bytes: Vec<_> = words.into_iter().flat_map(u32::to_be_bytes).collect();
        assert_eq!(decode(&bytes)?, (11..=19).collect());
        let mut changed = bytes.clone();
        changed[3] = 20;
        assert_eq!(decode(&changed)?, (12..=20).collect());
        for offset in [4, 8, 72, 76, 80] {
            let mut invalid = bytes.clone();
            invalid[offset + 3] ^= 4;
            assert!(decode(&invalid).is_err());
        }
        assert!(decode(&bytes[..80]).is_err());
        Ok(())
    }

    #[test]
    #[ignore = "requires both original extracted discs; CPU only"]
    fn original_skit_skip_branches_preserve_caller_permission() -> Result<()> {
        let expected = [251, 452, 472, 618, 657, 680, 696, 827, 833].into();
        for disc in [1, 2] {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../../local/extracted/disc{disc}/sys/main.dol"));
            assert_eq!(read(&std::fs::read(path)?)?, expected, "disc {disc}");
        }
        Ok(())
    }
}
