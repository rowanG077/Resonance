//! Authored gain ramps for the compressor applied to the final stereo mix.
//!
//! The MusyX command supplies a 32768 threshold and ten release blocks. Each
//! block contains 160 samples (5 ms at the nominal 32 kHz mixing rate). These
//! are time-domain gains, not sample-rate-conversion filter taps or DSP code.
use anyhow::{Result, ensure};
use serde::Serialize;
use std::{fs, path::Path};

const SAMPLES_PER_BLOCK: usize = 160;
const RELEASE_BLOCKS: usize = 10;
const ATTACK_RAMPS: usize = RELEASE_BLOCKS + 1;
const TABLE_BYTES: usize = (ATTACK_RAMPS + RELEASE_BLOCKS) * SAMPLES_PER_BLOCK * 2;

#[derive(Serialize)]
struct Compressor {
    version: u8,
    nominal_sample_rate_hz: u32,
    samples_per_block: usize,
    /// Compression triggers when either channel's absolute sample exceeds this.
    threshold_pcm: u16,
    release_blocks: usize,
    /// Both stereo channels use sample * gain >> gain_fractional_bits.
    gain_fractional_bits: u8,
    /// A triggered block selects its previous release count, then resets to ten.
    attack: Vec<GainRamp>,
    /// A quiet block decrements the release count before selecting its ramp.
    /// Consequently release plays rows 9 down to 0, then bypasses compression.
    release: Vec<GainRamp>,
}

#[derive(Serialize)]
struct GainRamp {
    release_blocks_remaining: usize,
    /// Unsigned Q15: 32768 means unity, even though its signed bit pattern is -32768.
    gain_q15: Vec<u16>,
}

pub(crate) fn cook(extracted: &Path, output: &Path) -> Result<Vec<String>> {
    let executable = fs::read(extracted.join("sys/main.dol"))?;
    let bytes = crate::dol::slice(&executable, 0x802a9040, TABLE_BYTES)?;
    let table = decode(bytes)?;
    let path = format!("audio/tables/{}/compressor.json", crate::digest(bytes));
    crate::write_atomic(&output.join(&path), &serde_json::to_vec_pretty(&table)?)?;
    Ok(vec![path])
}

fn decode(bytes: &[u8]) -> Result<Compressor> {
    ensure!(
        bytes.len() == TABLE_BYTES,
        "invalid compressor gain table length"
    );
    let ramps = |bytes: &[u8]| {
        bytes
            .chunks_exact(SAMPLES_PER_BLOCK * 2)
            .enumerate()
            .map(|(index, row)| GainRamp {
                release_blocks_remaining: index,
                gain_q15: row
                    .chunks_exact(2)
                    .map(|value| u16::from_be_bytes([value[0], value[1]]))
                    .collect(),
            })
            .collect()
    };
    let (attack, release) = bytes.split_at(ATTACK_RAMPS * SAMPLES_PER_BLOCK * 2);
    // The DSP's unsigned gain interpretation and ramp order are confirmed by
    // AXUCode::RunCompressor in Dolphin's Core/HW/DSPHLE/UCodes/AX.cpp.
    Ok(Compressor {
        version: 1,
        nominal_sample_rate_hz: 32_000,
        samples_per_block: SAMPLES_PER_BLOCK,
        threshold_pcm: 32_768,
        release_blocks: RELEASE_BLOCKS,
        gain_fractional_bits: 15,
        attack: ramps(attack),
        release: ramps(release),
    })
}
