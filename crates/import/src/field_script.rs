//! Correct the malformed actor spawn in the Iselia escape scene.
use std::borrow::Cow;

const ISELIA_ESCAPE_SCRIPT: &str =
    "558aa7d8f672649e7881d0de369dd8c0ceb5714bbbf3ac5343265c3ed330cc91";
const MISSING_ACTOR_ID: usize = 87_458;
// Replace the empty identity expression with a jump past this spawn statement.
const SKIP_SPAWN: [u8; 4] = [0x20, 0x01, 0xaa, 0x46];

pub(crate) fn prepare(bytes: &[u8]) -> Cow<'_, [u8]> {
    if crate::digest(bytes) != ISELIA_ESCAPE_SCRIPT {
        return Cow::Borrowed(bytes);
    }
    let mut corrected = bytes.to_vec();
    corrected[MISSING_ACTOR_ID..MISSING_ACTOR_ID + SKIP_SPAWN.len()].copy_from_slice(&SKIP_SPAWN);
    Cow::Owned(corrected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    #[ignore = "requires both extracted discs; no devices or cooking"]
    fn iselia_escape_correction_matches_both_disc_assets() -> anyhow::Result<()> {
        for disc in [1, 2] {
            let root = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../../local/extracted/disc{disc}"));
            let archive =
                crate::field::MapArchive::open(&crate::field::source_for_id(&root, 193)?)?;
            let source = archive.section(6)?;
            let corrected = prepare(source);
            assert_eq!(
                crate::digest(&corrected),
                "ba1842cecce0830b42f99b09a91564cd1440c77e254a78535617a53a7fac88af"
            );
            symphonia_script::Program::decode(&corrected)?;
            assert!(matches!(prepare(&corrected), Cow::Borrowed(_)));
            let mut different_asset = source.to_vec();
            different_asset[0] ^= 1;
            assert!(matches!(prepare(&different_asset), Cow::Borrowed(_)));
        }
        Ok(())
    }
}
