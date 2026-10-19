//! Correct malformed statements in specific original field scripts.
use std::borrow::Cow;

const ISELIA_ESCAPE_SCRIPT: &str =
    "558aa7d8f672649e7881d0de369dd8c0ceb5714bbbf3ac5343265c3ed330cc91";
const MISSING_ACTOR_ID: usize = 87_458;
// Replace the empty identity expression with a jump past this spawn statement.
const SKIP_SPAWN: [u8; 4] = [0x20, 0x01, 0xaa, 0x46];
const SALVATION_TUTORIAL_SCRIPT: &str =
    "68e1728a0222d3f80f868b9312e3b7619a4d70a76743e0b0f37a55ee68012b98";
const VOID_COMPARISON: usize = 0x107a;
// SetEventBit returns no value. Skip its unused comparison, retaining the setter
// and the following fade/input restoration. The jump preserves all script PCs.
const SKIP_COMPARISON: [u8; 4] = [0x20, 0x01, 0x08, 0x12];
const MELTOKIO_CASTLE_SCRIPT: &str =
    "a7813b2f7a4b5db7a9abcedcfab82f55d9b3fc6f82bbbf6c19999bd897603e11";
const WOOD_PLACEMENT: std::ops::Range<usize> = 0x37d4..0x3862;
const TOIZE_BOULDER_SCRIPT: &str =
    "873a57293b90ac866b903bb655e30151cb44655c628124816f0d4d891299d960";

fn reset_boulder_attempt(bytes: &[u8]) -> Vec<u8> {
    use symphonia_script::{NativeCall, scenario};

    // A previous boulder hit must not suppress Bacura collisions on every retry.
    // Clear the hit guard when starting a new attempt, including after loading.
    const PLAYER_HIT: u16 = 612;
    const START_ATTEMPT: std::ops::Range<usize> = 0x161a..0x1624;
    let header = scenario::parse_header(bytes).expect("known field script");
    let mut reset: Vec<_> = [
        0x0100,
        PLAYER_HIT,
        0x3000,
        0x4000,
        0x2000 | NativeCall::ClearEventBit as u16,
    ]
    .into_iter()
    .flat_map(u16::to_be_bytes)
    .collect();
    reset.extend_from_slice(&bytes[START_ATTEMPT.clone()]);
    reset.extend(
        [
            0x2001,
            ((START_ATTEMPT.end - header.code_base()) / 2) as u16,
        ]
        .into_iter()
        .flat_map(u16::to_be_bytes),
    );
    let mut corrected = bytes.to_vec();
    corrected[START_ATTEMPT.start..START_ATTEMPT.start + 4].copy_from_slice(
        &[0x2001, header.auxiliary_words - header.code_base_words]
            .into_iter()
            .flat_map(u16::to_be_bytes)
            .collect::<Vec<_>>(),
    );
    // Insert before auxiliary data so existing code and data-relative PCs stay valid.
    corrected[4..6]
        .copy_from_slice(&(header.auxiliary_words + (reset.len() / 2) as u16).to_be_bytes());
    corrected.splice(header.auxiliary_offset()..header.auxiliary_offset(), reset);
    corrected
}

fn place_castle_wood(bytes: &[u8]) -> Vec<u8> {
    use symphonia_script::NativeCall;

    // The carried prop survives Presea's removal. Remove it before creating the
    // ground prop, so the next conversation and pickup address a single log.
    const WOOD: i32 = 210;
    let mut words = Vec::new();
    for (call, args) in [
        (NativeCall::DespawnActor, &[WOOD][..]),
        (
            NativeCall::CreateSceneActor,
            &[WOOD, 28, 3768, 388, 268, -1179648, 0, 0][..],
        ),
    ] {
        for &value in args {
            words.extend([0x0200, value as u16, (value >> 16) as u16, 0x3000, 0x4000]);
        }
        words.push(0x2000 | call as u16);
    }
    // Creation already disables grounding, collision and shadows. Skip the
    // redundant setters, retaining every following instruction's address.
    let code_base = usize::from(u16::from_be_bytes([bytes[0], bytes[1]])) * 2;
    words.extend([0x2001, ((WOOD_PLACEMENT.end - code_base) / 2) as u16]);
    let replacement: Vec<_> = words.into_iter().flat_map(u16::to_be_bytes).collect();
    assert!(replacement.len() <= WOOD_PLACEMENT.len());
    let mut corrected = bytes.to_vec();
    corrected[WOOD_PLACEMENT.start..WOOD_PLACEMENT.start + replacement.len()]
        .copy_from_slice(&replacement);
    corrected
}

pub(crate) fn prepare(bytes: &[u8]) -> Cow<'_, [u8]> {
    let (offset, replacement) = match crate::digest(bytes).as_str() {
        ISELIA_ESCAPE_SCRIPT => (MISSING_ACTOR_ID, SKIP_SPAWN),
        SALVATION_TUTORIAL_SCRIPT => (VOID_COMPARISON, SKIP_COMPARISON),
        MELTOKIO_CASTLE_SCRIPT => return Cow::Owned(place_castle_wood(bytes)),
        TOIZE_BOULDER_SCRIPT => return Cow::Owned(reset_boulder_attempt(bytes)),
        _ => return Cow::Borrowed(bytes),
    };
    let mut corrected = bytes.to_vec();
    corrected[offset..offset + replacement.len()].copy_from_slice(&replacement);
    Cow::Owned(corrected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    #[ignore = "requires both extracted discs; no devices or cooking"]
    fn boulder_retry_correction_preserves_field_data() -> anyhow::Result<()> {
        for disc in [1, 2] {
            let root = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../../local/extracted/disc{disc}"));
            let archive =
                crate::field::MapArchive::open(&crate::field::source_for_id(&root, 413)?)?;
            let source = archive.section(6)?;
            assert_eq!(crate::digest(source), TOIZE_BOULDER_SCRIPT);
            let corrected = prepare(source);
            let before = symphonia_script::scenario::parse_header(source)?;
            let after = symphonia_script::scenario::parse_header(&corrected)?;
            assert_eq!(
                &source[before.auxiliary_offset()..],
                &corrected[after.auxiliary_offset()..]
            );
            symphonia_script::Program::decode(&corrected)?;
            assert!(matches!(prepare(&corrected), Cow::Borrowed(_)));
        }
        Ok(())
    }

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

    #[test]
    #[ignore = "requires both extracted discs; no devices or cooking"]
    fn salvation_tutorial_correction_matches_both_disc_assets() -> anyhow::Result<()> {
        for disc in [1, 2] {
            let root = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../../local/extracted/disc{disc}"));
            let archive = crate::field::MapArchive::open(&crate::field::source_for_id(&root, 81)?)?;
            let source = archive.section(6)?;
            assert_eq!(crate::digest(source), SALVATION_TUTORIAL_SCRIPT);
            let corrected = prepare(source);
            assert_eq!(
                crate::digest(&corrected),
                "5fdd55f73ebc60fbf014ec4c9223eefff9484d4ffeb19fbebea9df4ab493a035"
            );
            assert_eq!(corrected.len(), source.len());
            assert_eq!(&corrected[..VOID_COMPARISON], &source[..VOID_COMPARISON]);
            assert_eq!(
                &corrected[VOID_COMPARISON + SKIP_COMPARISON.len()..],
                &source[VOID_COMPARISON + SKIP_COMPARISON.len()..]
            );
            let program = symphonia_script::Program::decode(&corrected)?;
            assert_eq!(
                program.instruction(0x080e),
                Some((symphonia_script::Op::Native(0x68), 0x080f))
            );
            assert_eq!(
                program.instruction(0x080f),
                Some((symphonia_script::Op::Jump(0x0812), 0x0811))
            );
            assert!(matches!(prepare(&corrected), Cow::Borrowed(_)));
            let mut different_asset = source.to_vec();
            different_asset[0] ^= 1;
            assert!(matches!(prepare(&different_asset), Cow::Borrowed(_)));
        }
        Ok(())
    }
}
