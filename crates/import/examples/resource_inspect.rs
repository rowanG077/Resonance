//! Read-only inspection of grouped source resources; no game or audio device.
#[path = "../src/dol.rs"]
mod dol;
use anyhow::{Context, Result};
use std::{fs, path::Path};
fn word(data: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_be_bytes(
        data.get(at..at + 4).context("word range")?.try_into()?,
    ))
}
fn main() -> Result<()> {
    let root = Path::new("local/extracted/disc1");
    let executable = fs::read(root.join("sys/main.dol"))?;
    for id in std::env::args().skip(1) {
        let id = u32::from_str_radix(id.trim_start_matches("0x"), 16)?;
        if id >= 0x80000000 {
            if id == 0x801e76b0 {
                for index in 0..17 {
                    let ptr = word(dol::slice(&executable, id + index * 4, 4)?, 0)?;
                    let bytes = dol::slice(&executable, ptr, 64)?;
                    let end = bytes
                        .iter()
                        .position(|b| *b == 0)
                        .context("name terminator")?;
                    println!("bone {index}: {}", std::str::from_utf8(&bytes[..end])?);
                }
            }
            let bytes = dol::slice(&executable, id, 16)?;
            println!(
                "{id:#x}: {:02x?}; f32 {:?}",
                bytes,
                bytes
                    .chunks_exact(4)
                    .map(|b| f32::from_be_bytes(b.try_into().unwrap()))
                    .collect::<Vec<_>>()
            );
            continue;
        }
        let group = (id >> 16)
            .checked_sub(1)
            .context("not a grouped resource")?;
        let ptr = word(dol::slice(&executable, 0x801f86b8 + group * 12, 4)?, 0)?;
        let path = dol::slice(&executable, ptr, 128)?;
        let path = std::str::from_utf8(
            &path[..path
                .iter()
                .position(|b| *b == 0)
                .context("path terminator")?],
        )?;
        let data = fs::read(root.join("files").join(path))?;
        let index = (id & 0xffff) as usize;
        let offset = word(&data, 4 + index * 8)? as usize;
        let size = word(&data, 8 + index * 8)? as usize;
        let bytes = data.get(offset..offset + size).context("entry range")?;
        if word(bytes, 0)? == 31 {
            for slot in [2usize, 8, 19] {
                let offset = word(bytes, 4 + slot * 4)? as usize;
                if offset != 0 {
                    let clip = &bytes[offset..];
                    if word(clip, 0)? == 0x007b7960 {
                        println!(
                            " clip {} header={:02x?}",
                            4 + slot * 4,
                            &clip[..clip.len().min(80)]
                        );
                        let names = word(clip, 20)? as usize;
                        println!(
                            " names={:?}",
                            String::from_utf8_lossy(&clip[names..clip.len().min(names + 100)])
                        );
                    }
                }
            }
        }
        println!(
            "{id:#x} {path} offset={offset:#x} size={size:#x} header={:02x?}",
            &bytes[..bytes.len().min(128)]
        );
        for s in bytes
            .split(|c| !c.is_ascii_graphic())
            .filter(|s| s.len() > 6)
            .take(12)
        {
            println!("  {}", String::from_utf8_lossy(s));
        }
    }
    Ok(())
}
