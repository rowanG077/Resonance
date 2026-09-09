//! Read-only executable constants for comparing native implementations.
#[path = "../src/dol.rs"]
mod dol;
use anyhow::{Context, Result};
use std::fs;
fn main() -> Result<()> {
    let data = fs::read("local/extracted/disc1/sys/main.dol")?;
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "--code") {
        let address = u32::from_str_radix(&args[1], 16)?;
        let size = usize::from_str_radix(&args[2], 16)?;
        fs::write(&args[3], dol::slice(&data, address, size)?)?;
        return Ok(());
    }
    for arg in std::env::args().skip(1) {
        let address = u32::from_str_radix(arg.trim_start_matches("0x"), 16)?;
        let bytes = dol::slice(&data, address, 16)?;
        let word: [u8; 4] = bytes[..4].try_into().context("word")?;
        println!(
            "{address:08x}: {:02x?}; u32={:#x} f32={} text={:?}",
            bytes,
            u32::from_be_bytes(word),
            f32::from_be_bytes(word),
            String::from_utf8_lossy(bytes)
        );
    }
    Ok(())
}
