//! Targeted corrections and gameplay changes to field scripts.
use std::borrow::Cow;

// Iselia ranch escape (field 193): skip an actor-creation statement whose actor
// ID expression is empty. Evaluating it otherwise aborts the scene in the VM.
// GameCube behavior at this malformed statement has not been verified. This is
// a targeted workaround for our checked VM, not a confirmed original-game bug.
const ISELIA_ESCAPE_SCRIPT: &str =
    "558aa7d8f672649e7881d0de369dd8c0ceb5714bbbf3ac5343265c3ed330cc91";
const MISSING_ACTOR_ID: usize = 87_458;
// Replace the empty identity expression with a jump past this spawn statement.
const SKIP_SPAWN: [u8; 4] = [0x20, 0x01, 0xaa, 0x46];
// House of Salvation long-range attack tutorial (field 81): remove a comparison
// against SetEventBit's nonexistent return value, retaining the flag, fade and
// input restoration. Static inspection shows why GameCube survives: its unchecked
// comparison reads the stack-depth field as the missing operand, and the following
// discard restores that depth. Our checked VM rejects this stack underflow.
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
const ALTESSA_GNOMETTE_SCRIPT: &str =
    "db9ee52a3104e18b97886c9496948ab2c83a3d4fbd7fef924718ee663fca1a0b";
const SHADOW_TEMPLE_SCRIPT: &str =
    "9290f4f4a581909b18432ed3f8ed951d93827f24df44b231d3a5f8725bd73640";
const SHADOW_SEAL_SCRIPT: &str = "7e2d342e196d6b4a1048add87f8170296b383bce04e2e5dbff6da6ac46ed3098";
const NIFLHEIM_SCRIPT: &str = "36e39a2ec87a074e22c2d88f7693e9196af6403e3f59ebb0511c26dee9b141b1";

/// Keep Niflheim enemies visible at widescreen edges by replacing distance-based
/// model hiding with the existing ring-contact state used by the same branches.
/// GameCube used a narrower 4:3 view; keep visible enemies in widescreen.
fn adjust_niflheim(bytes: &[u8]) -> Vec<u8> {
    let mut corrected = bytes.to_vec();
    // The room pauses distant enemies and uses model hiding as its state flag.
    // Its radius falls inside a widescreen view. Use the ring-contact flag,
    // which the same branches already set, without changing their AI or collision.
    const RING_CONTACT_DISABLED: u16 = 48;
    for offset in [0x7846, 0x7866, 0x795e, 0x797e] {
        corrected[offset..offset + 2].copy_from_slice(&RING_CONTACT_DISABLED.to_be_bytes());
    }
    corrected
}

/// Intentional Shadow Temple gameplay change: use player-anchored movement for
/// collected fragments so they cannot lag behind, drift while pushing blocks,
/// or leave the map. The scripts still control pickup, lamp release and delivery.
/// GameCube uses path-following here; anchoring is an intentional replacement
/// requested for easier handling, not required to reproduce its behavior.
fn anchor_shadow_followers(bytes: &[u8]) -> Vec<u8> {
    use symphonia_script::{NativeCall, scenario};

    let header = scenario::parse_header(bytes).expect("known field script");
    let mut corrected = bytes.to_vec();
    let code = &mut corrected[header.code_base()..header.auxiliary_offset()];
    // Change the following selector in both assignments and queries. Lamp
    // release, pickup eligibility and scripted delivery remain script-owned.
    for (words, operand) in [
        (
            vec![
                34,
                0x3000,
                0x4000,
                12,
                0x3000,
                0x4000,
                0x2000 | NativeCall::SetActorProperty as u16,
            ],
            3,
        ),
        (
            vec![
                34,
                0x3000,
                0x4000,
                0x2000 | NativeCall::GetActorProperty as u16,
                12,
                0x3021,
            ],
            4,
        ),
    ] {
        let pattern: Vec<_> = words.into_iter().flat_map(u16::to_be_bytes).collect();
        for start in (0..=code.len() - pattern.len()).step_by(2) {
            if code[start..start + pattern.len()] == pattern {
                let offset = start + operand * 2;
                code[offset..offset + 2].copy_from_slice(
                    &u16::from(resonance_content::field::ANCHORED_PLAYER_BEHAVIOR).to_be_bytes(),
                );
            }
        }
    }
    corrected
}

/// Delay the gnomette's removal at Altessa's house until the screen is black,
/// preventing his visible disappearance just before the fade. Check his
/// departure flag because the conversation where he stays shares the same fade.
/// The unmodified departure also disappears early in Dolphin. This improves an
/// original scene defect; it does not compensate for a Resonance-only mismatch.
fn depart_after_blackout(bytes: &[u8]) -> Vec<u8> {
    use symphonia_script::{NativeCall, scenario};

    const GNOMETTE: u16 = 900;
    const DEPARTED: u16 = 372;
    const FADE_AND_WAIT: std::ops::Range<usize> = 0x118b4..0x118d0;
    let header = scenario::parse_header(bytes).expect("known field script");
    let resume = ((FADE_AND_WAIT.end - header.code_base()) / 2) as u16;
    let mut transition = bytes[FADE_AND_WAIT.clone()].to_vec();
    // Both conversations share this fade, including the branch where he stays.
    // Remove him only after black, and only when his departure flag was set.
    transition.extend(
        [
            0x0100,
            DEPARTED,
            0x3000,
            0x4000,
            0x2000 | NativeCall::TestEventBit as u16,
            0x0001,
            0x3020,
            0x3000,
            0x2004,
            resume,
            0x0100,
            GNOMETTE,
            0x3000,
            0x4000,
            0x2000 | NativeCall::DespawnActor as u16,
            0x2001,
            resume,
        ]
        .into_iter()
        .flat_map(u16::to_be_bytes),
    );
    let mut corrected = bytes.to_vec();
    for offset in [0x11608, 0x118a4] {
        corrected[offset..offset + 2]
            .copy_from_slice(&(0x2000 | NativeCall::DiscardValue as u16).to_be_bytes());
    }
    corrected[FADE_AND_WAIT.start..FADE_AND_WAIT.start + 4].copy_from_slice(
        &[0x2001, header.auxiliary_words - header.code_base_words]
            .into_iter()
            .flat_map(u16::to_be_bytes)
            .collect::<Vec<_>>(),
    );
    corrected[4..6]
        .copy_from_slice(&(header.auxiliary_words + (transition.len() / 2) as u16).to_be_bytes());
    corrected.splice(
        header.auxiliary_offset()..header.auxiliary_offset(),
        transition,
    );
    corrected
}

/// Clear the Toize Valley Mine boulder's player-hit flag on each new attempt.
/// Otherwise an earlier hit remains latched and suppresses subsequent collisions
/// with Bacura, preventing the puzzle from being solved on retries or after loading.
/// Dolphin gameplay reproduced the same failure after being hit, returning and
/// retrying. GameCube succeeds when the flag is clear; this fixes its retained-flag
/// retry bug instead of relying on another interaction to clear the flag.
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

/// Remove Presea's carried wood before placing the ground prop at Meltokio castle.
/// Removing Presea alone leaves that prop alive, producing two logs. Explicitly
/// replacing it leaves one prop for the later conversation and pickup.
/// The captured GameCube route also contains two wood actors, but the old carried
/// prop renders off-screen. That masks the missing cleanup there. Explicit removal
/// avoids depending on this incidental placement; the precise cause of the
/// different orphan render position has not been established.
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

/// Apply patches only to exact known script hashes because their offsets depend
/// on that byte layout. Unrecognized and already-patched scripts pass through.
pub(crate) fn prepare(bytes: &[u8]) -> Cow<'_, [u8]> {
    let (offset, replacement) = match crate::digest(bytes).as_str() {
        ISELIA_ESCAPE_SCRIPT => (MISSING_ACTOR_ID, SKIP_SPAWN),
        SALVATION_TUTORIAL_SCRIPT => (VOID_COMPARISON, SKIP_COMPARISON),
        MELTOKIO_CASTLE_SCRIPT => return Cow::Owned(place_castle_wood(bytes)),
        TOIZE_BOULDER_SCRIPT => return Cow::Owned(reset_boulder_attempt(bytes)),
        ALTESSA_GNOMETTE_SCRIPT => return Cow::Owned(depart_after_blackout(bytes)),
        NIFLHEIM_SCRIPT => return Cow::Owned(adjust_niflheim(bytes)),
        SHADOW_TEMPLE_SCRIPT | SHADOW_SEAL_SCRIPT => {
            return Cow::Owned(anchor_shadow_followers(bytes));
        }
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
    fn shadow_following_patch_preserves_lamp_and_delivery_scripts() -> anyhow::Result<()> {
        for disc in [1, 2] {
            let root = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../../local/extracted/disc{disc}"));
            for (map, expected_changes) in [(126, 18), (127, 26)] {
                let archive =
                    crate::field::MapArchive::open(&crate::field::source_for_id(&root, map)?)?;
                let source = archive.section(6)?;
                let corrected = prepare(source);
                assert_eq!(source.len(), corrected.len());
                let changes: Vec<_> = source
                    .iter()
                    .zip(corrected.iter())
                    .filter(|(before, after)| before != after)
                    .collect();
                assert_eq!(changes.len(), expected_changes);
                assert!(changes.iter().all(|(before, after)| **before == 12
                    && **after == resonance_content::field::ANCHORED_PLAYER_BEHAVIOR));
                let header = symphonia_script::scenario::parse_header(source)?;
                assert_eq!(
                    &source[header.auxiliary_offset()..],
                    &corrected[header.auxiliary_offset()..]
                );
                symphonia_script::Program::decode(&corrected)?;
                assert!(matches!(prepare(&corrected), Cow::Borrowed(_)));
            }
        }
        Ok(())
    }

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
