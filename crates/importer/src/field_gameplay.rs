//! Deliberate gameplay corrections, applied once while publishing scenarios.
//! Cooked scripts remain freely editable; the runtime never reapplies these.
use anyhow::{Context, Result, ensure};
use std::borrow::Cow;
use symphonia_constants::FieldId;

const WOOD_PLACEMENT: std::ops::Range<usize> = 0x37d4..0x3862;

/// Check only instructions this correction replaces, never a whole-file identity.
fn expect_words(bytes: &[u8], offset: usize, expected: &[u16]) -> Result<()> {
    ensure!(
        bytes
            .get(offset..offset + expected.len() * 2)
            .is_some_and(|actual| {
                actual
                    .chunks_exact(2)
                    .zip(expected)
                    .all(|(a, b)| u16::from_be_bytes([a[0], a[1]]) == *b)
            }),
        "gameplay correction expects original instructions at byte {offset:#x}; source was edited"
    );
    Ok(())
}

pub(crate) fn prepare(id: usize, bytes: &[u8]) -> Result<Cow<'_, [u8]>> {
    let correction: fn(&[u8]) -> Result<Vec<u8>> = match u16::try_from(id)
        .ok()
        .and_then(|id| FieldId::try_from(id).ok())
    {
        Some(FieldId::TempleOfDarkness0126 | FieldId::TempleOfDarkness0127) => {
            anchor_shadow_followers
        }
        Some(FieldId::AltessasHouse0139) => depart_after_blackout,
        Some(FieldId::Meltokio0377) => place_castle_wood,
        Some(FieldId::ToizeValleyMine0413) => reset_boulder_attempt,
        Some(FieldId::Niflheim0524) => adjust_niflheim,
        _ => return Ok(Cow::Borrowed(bytes)),
    };
    symphonia_script::Program::decode(bytes)
        .with_context(|| format!("field {id} native script"))?;
    correction(bytes)
        .map(Cow::Owned)
        .with_context(|| format!("field {id} authored gameplay correction"))
}

fn header(bytes: &[u8]) -> Result<symphonia_script::scenario::Header> {
    let header = symphonia_script::scenario::parse_header(bytes)?;
    ensure!(
        header.auxiliary_words > header.code_base_words && header.auxiliary_offset() <= bytes.len(),
        "invalid gameplay correction code range"
    );
    Ok(header)
}

/// Keep Niflheim enemies visible at widescreen edges by replacing distance-based
/// model hiding with the existing ring-contact state used by the same branches.
/// Also adapt victory bookkeeping for combined battles: award
/// `10 * formation_count - ticks / 120` Soulfire and count every defeated group.
/// Counting only one group leaves red floors without an exit.
fn adjust_niflheim(bytes: &[u8]) -> Result<Vec<u8>> {
    use symphonia_script::NativeCall;

    for offset in [0x7846, 0x7866, 0x795e, 0x797e] {
        expect_words(bytes, offset, &[12])?;
    }
    expect_words(bytes, 0x8a52, &[0x1200, 0x08c0, 10, 0x3011, 0x3000])?;
    expect_words(bytes, 0x8a84, &[0x1200, 0x09b8, 0x3002, 0x3000])?;
    let mut corrected = bytes.to_vec();
    // The room pauses distant enemies and uses model hiding as its state flag.
    // Its radius falls inside a widescreen view. Use the ring-contact flag,
    // which the same branches already set, without changing their AI or collision.
    const RING_CONTACT_DISABLED: u16 = 48;
    for offset in [0x7846, 0x7866, 0x795e, 0x797e] {
        corrected[offset..offset + 2].copy_from_slice(&RING_CONTACT_DISABLED.to_be_bytes());
    }
    // Award ten Soulfire per formation; the unchanged time cost is ticks / 120.
    // Red floors count defeated field groups, including every joined formation.
    const SOULFIRE_BONUS: std::ops::Range<usize> = 0x8a52..0x8a5c;
    const DEFEATED_GROUPS: std::ops::Range<usize> = 0x8a84..0x8a8c;
    let header = header(bytes)?;
    ensure!(
        header.code_base() <= 0x7846 && header.auxiliary_offset() >= DEFEATED_GROUPS.end,
        "Niflheim correction lies outside script code"
    );
    let count = [
        0x0100,
        symphonia_constants::FieldSystemCommand::LastBattleFormationCount.raw(),
        0x3000,
        0x4000,
        0x0000,
        0x3000,
        0x4000,
        0x2000 | NativeCall::FieldSystem as u16,
    ];
    let mut extra = Vec::new();
    for (range, prefix, suffix) in [
        (
            SOULFIRE_BONUS,
            &[0x1200, 0x08c0, 0x000a][..],
            &[0x3033, 0x3011][..],
        ),
        (DEFEATED_GROUPS, &[0x1200, 0x09b8][..], &[0x3012][..]),
    ] {
        let target = header.auxiliary_words - header.code_base_words + extra.len() as u16;
        corrected[range.start..range.start + 4].copy_from_slice(
            &[0x2001, target]
                .into_iter()
                .flat_map(u16::to_be_bytes)
                .collect::<Vec<_>>(),
        );
        extra.extend_from_slice(prefix);
        extra.extend(count);
        extra.extend_from_slice(suffix);
        extra.extend([
            0x3000,
            0x2001,
            ((range.end - header.code_base()) / 2) as u16,
        ]);
    }
    let auxiliary = header
        .auxiliary_words
        .checked_add(u16::try_from(extra.len())?)
        .context("gameplay correction exceeds script code limit")?;
    corrected[4..6].copy_from_slice(&auxiliary.to_be_bytes());
    corrected.splice(
        header.auxiliary_offset()..header.auxiliary_offset(),
        extra.into_iter().flat_map(u16::to_be_bytes),
    );
    Ok(corrected)
}

/// Intentional Shadow Temple gameplay change: use player-anchored movement for
/// collected fragments so they cannot lag behind, drift while pushing blocks,
/// or leave the map. The scripts still control pickup, lamp release and delivery.
/// This is an intentional gameplay customization, not native behavior.
fn anchor_shadow_followers(bytes: &[u8]) -> Result<Vec<u8>> {
    use symphonia_script::NativeCall;

    let header = header(bytes)?;
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
        for start in (0..=code.len().saturating_sub(pattern.len())).step_by(2) {
            if code.get(start..start + pattern.len()) == Some(pattern.as_slice()) {
                let offset = start + operand * 2;
                code[offset..offset + 2].copy_from_slice(
                    &u16::from(resonance_content::field::ANCHORED_PLAYER_BEHAVIOR).to_be_bytes(),
                );
            }
        }
    }
    Ok(corrected)
}

/// Delay the gnomette's removal at Altessa's house until the screen is black,
/// preventing his visible disappearance just before the fade. Check his
/// departure flag because the conversation where he stays shares the same fade.
/// This fixes an original cosmetic defect.
fn depart_after_blackout(bytes: &[u8]) -> Result<Vec<u8>> {
    use symphonia_script::NativeCall;

    for offset in [0x11608, 0x118a4] {
        expect_words(bytes, offset, &[0x2000 | NativeCall::DespawnActor as u16])?;
    }
    expect_words(
        bytes,
        0x118b4,
        &[
            1, 0x3000, 0x4000, 30, 0x3000, 0x4000, 0x2063, 0, 0x3000, 0x4000, 30, 0x3000, 0x4000,
            0x2064,
        ],
    )?;
    const GNOMETTE: u16 = 900;
    const DEPARTED: u16 = 372;
    const FADE_AND_WAIT: std::ops::Range<usize> = 0x118b4..0x118d0;
    let header = header(bytes)?;
    ensure!(
        header.code_base() <= 0x11608 && header.auxiliary_offset() >= FADE_AND_WAIT.end,
        "Gnomette correction lies outside script code"
    );
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
    let auxiliary = header
        .auxiliary_words
        .checked_add(u16::try_from(transition.len() / 2)?)
        .context("gameplay correction exceeds script code limit")?;
    corrected[4..6].copy_from_slice(&auxiliary.to_be_bytes());
    corrected.splice(
        header.auxiliary_offset()..header.auxiliary_offset(),
        transition,
    );
    Ok(corrected)
}

/// Clear the Toize Valley Mine boulder's player-hit flag on each new attempt.
/// Otherwise an earlier hit remains latched and suppresses subsequent collisions
/// with Bacura, preventing the puzzle from being solved on retries or after loading.
/// This fixes the original retained-flag retry bug.
fn reset_boulder_attempt(bytes: &[u8]) -> Result<Vec<u8>> {
    use symphonia_script::NativeCall;

    expect_words(bytes, 0x161a, &[0x0100, 301, 0x3000, 0x4000, 0x2068])?;
    // A previous boulder hit must not suppress Bacura collisions on every retry.
    // Clear the hit guard when starting a new attempt, including after loading.
    const PLAYER_HIT: u16 = 612;
    const START_ATTEMPT: std::ops::Range<usize> = 0x161a..0x1624;
    let header = header(bytes)?;
    ensure!(
        header.code_base() <= START_ATTEMPT.start && header.auxiliary_offset() >= START_ATTEMPT.end,
        "boulder correction lies outside script code"
    );
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
    let auxiliary = header
        .auxiliary_words
        .checked_add(u16::try_from(reset.len() / 2)?)
        .context("gameplay correction exceeds script code limit")?;
    corrected[4..6].copy_from_slice(&auxiliary.to_be_bytes());
    corrected.splice(header.auxiliary_offset()..header.auxiliary_offset(), reset);
    Ok(corrected)
}

/// Remove Presea's carried wood before placing the ground prop at Meltokio castle.
/// Removing Presea alone leaves that prop alive, producing two logs. Explicitly
/// replacing it leaves one prop for the later conversation and pickup.
/// Explicit cleanup avoids relying on the original orphan being off-screen.
fn place_castle_wood(bytes: &[u8]) -> Result<Vec<u8>> {
    use symphonia_script::NativeCall;

    expect_words(
        bytes,
        WOOD_PLACEMENT.start,
        &[
            0x0100, 0x00d2, 0x3000, 0x4000, 0x001c, 0x3000, 0x4000, 0x0100, 0x0eb8, 0x3000, 0x4000,
            0x0100, 0x0143, 0x0041, 0x3031, 0x3000, 0x4000, 0x0100, 0x00b2, 0x005a, 0x3031, 0x3000,
            0x4000, 0x0200, 0x0000, 0xffee, 0x3000, 0x4000, 0x0000, 0x3000, 0x4000, 0x0000, 0x3000,
            0x4000, 0x20b9, 0x0100, 0x00d2, 0x3000, 0x4000, 0x000b, 0x3000, 0x4000, 0x0001, 0x3000,
            0x4000, 0x201d, 0x3000, 0x0100, 0x00d2, 0x3000, 0x4000, 0x0009, 0x3000, 0x4000, 0x0001,
            0x3000, 0x4000, 0x201d, 0x3000, 0x0100, 0x00d2, 0x3000, 0x4000, 0x000a, 0x3000, 0x4000,
            0x0001, 0x3000, 0x4000, 0x201d, 0x3000,
        ],
    )?;
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
    let header = header(bytes)?;
    ensure!(
        header.code_base() <= WOOD_PLACEMENT.start
            && header.auxiliary_offset() >= WOOD_PLACEMENT.end,
        "wood correction lies outside script code"
    );
    words.extend([
        0x2001,
        ((WOOD_PLACEMENT.end - header.code_base()) / 2) as u16,
    ]);
    let replacement: Vec<_> = words.into_iter().flat_map(u16::to_be_bytes).collect();
    assert!(replacement.len() <= WOOD_PLACEMENT.len());
    let mut corrected = bytes.to_vec();
    corrected[WOOD_PLACEMENT.start..WOOD_PLACEMENT.start + replacement.len()]
        .copy_from_slice(&replacement);
    Ok(corrected)
}
