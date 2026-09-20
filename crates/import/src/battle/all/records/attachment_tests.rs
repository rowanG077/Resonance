use super::{attachment_recipe, half, word};
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs,
    io::{Cursor, Read},
    path::Path,
};

fn roundtrip(bytes: &[u8]) -> Result<Value> {
    let value = attachment_recipe(bytes)?;
    let number = |value: &Value| value.as_i64().context("missing attachment number");
    let mut restored = [0; 64];
    let channels = value["uv_channels"]
        .as_array()
        .context("missing UV channels")?;
    assert_eq!(channels.len(), 2);
    for (index, channel) in channels.iter().enumerate() {
        restored[index] = u8::try_from(number(&channel["count"])?)?;
        restored[2 + index] = i8::try_from(number(&channel["texture"])?)? as u8;
        restored[4 + index] = match channel["mode"].as_str() {
            Some(mode) => [
                "disabled", "frames", "scroll_v", "reserved", "sequence", "scroll_u",
            ]
            .iter()
            .position(|&name| name == mode)
            .context("unknown UV mode")? as u8,
            None => i8::try_from(number(&channel["mode"]["inactive"])?)? as u8,
        };
        restored[6 + index] = i8::try_from(number(&channel["frames_or_step"])?)? as u8;
        restored[8 + index] = i8::try_from(number(&channel["period"])?)? as u8;
        assert_eq!(
            channel["initial_scroll"],
            half(bytes, 28 + index * 2)? as i16
        );
    }
    for (at, key) in [(10, "effect_interval"), (11, "sequence_length")] {
        restored[at] = u8::try_from(number(&value[key])?)?;
    }
    let storage = &value["unused_storage"];
    assert_eq!(storage.as_array().map(Vec::len), Some(1));
    assert_eq!(storage[0]["offset"], 12);
    assert_eq!(storage[0]["bytes"].as_array().map(Vec::len), Some(1));
    restored[12] = u8::try_from(number(&storage[0]["bytes"][0])?)?;
    let trail = &value["trail"];
    restored[13] = i8::try_from(number(&trail["texture"])?)? as u8;
    restored[14] = u8::try_from(number(&trail["palette"])?)?;
    restored[15] = u8::try_from(number(&trail["flags"])?)?;
    for index in 0..4 {
        restored[16 + index] = u8::try_from(number(&trail["color"][index])?)?;
        restored[20 + index * 2..22 + index * 2]
            .copy_from_slice(&i16::try_from(number(&trail["uv"][index])?)?.to_be_bytes());
    }
    let frames = value["frame_order"]
        .as_array()
        .context("missing full frame union")?;
    assert_eq!(frames.len(), 36);
    for (target, frame) in restored[28..].iter_mut().zip(frames) {
        *target = u8::try_from(number(frame)?)?;
    }
    assert_eq!(restored, bytes, "attachment JSON lost source bytes");
    Ok(value)
}

#[test]
fn attachment_settings_preserve_union_storage_and_validate_only_active_sequences() -> Result<()> {
    let mut bytes = std::array::from_fn::<_, 64, _>(|index| (index as u8).wrapping_mul(73));
    bytes[..2].copy_from_slice(&[3, 2]);
    bytes[4..6].copy_from_slice(&[2, 4]);
    bytes[11] = 1;
    bytes[28..32].copy_from_slice(&[0x80, 0x01, 0x7f, 0xff]);
    let value = roundtrip(&bytes)?;
    assert_eq!(value["uv_channels"][0]["initial_scroll"], -32767);
    assert_eq!(value["uv_channels"][1]["initial_scroll"], 32767);
    assert_eq!(value["frame_order"][35], bytes[63]);
    bytes[11] = 36;
    roundtrip(&bytes)?;
    for stored in [0, u8::MAX] {
        bytes[12] = stored;
        roundtrip(&bytes)?;
    }
    for length in [0, 37, 255] {
        bytes[11] = length;
        assert!(attachment_recipe(&bytes).is_err());
    }
    bytes[1] = 0; // The sequence is now inactive, including its extreme length.
    roundtrip(&bytes)?;
    bytes[..2].fill(255);
    for modes in [[0, 1], [3, 5], [6, 127], [128, 255]] {
        bytes[4..6].copy_from_slice(&modes);
        roundtrip(&bytes)?;
    }
    bytes[4..6].fill(4);
    bytes[..2].fill(0);
    roundtrip(&bytes)?;
    assert!(attachment_recipe(&bytes[..63]).is_err());
    assert!(attachment_recipe(&[bytes.as_slice(), &[0]].concat()).is_err());
    Ok(())
}

/// Walk source containers, including alias-independent physical members. No
/// selected actor list, renderer, texture converter, or published JSON is used.
fn packages(bytes: &[u8], depth: u8) -> Result<usize> {
    ensure!(depth <= 16, "attachment container nesting exceeds 16");
    if bytes.starts_with(b"MSCF") {
        let mut cabinet = cab::Cabinet::new(Cursor::new(bytes))?;
        let names: Vec<_> = cabinet
            .folder_entries()
            .flat_map(|folder| folder.file_entries())
            .map(|entry| entry.name().to_owned())
            .collect();
        let mut count = 0;
        for name in names {
            let mut member = Vec::new();
            cabinet
                .read_file(&name)?
                .take(64 * 1024 * 1024 + 1)
                .read_to_end(&mut member)?;
            ensure!(
                member.len() <= 64 * 1024 * 1024,
                "oversized attachment member"
            );
            count += packages(&member, depth + 1).with_context(|| name.clone())?;
        }
        return Ok(count);
    }
    let Ok(ranges) = crate::field::sections(bytes) else {
        return Ok(0);
    };
    if (5..=7).contains(&ranges.len())
        && ranges[1].as_ref().is_some_and(|range| {
            matches!(
                word(&bytes[range.clone()], 0x20).ok(),
                Some(0x005b_bc61 | 0x00b7_49e0)
            )
        })
    {
        roundtrip(&bytes[ranges[0].clone().context("attachment has no settings")?])?;
        return Ok(1);
    }
    let physical: BTreeSet<_> = ranges
        .into_iter()
        .flatten()
        .map(|range| (range.start, range.end))
        .collect();
    physical
        .into_iter()
        .map(|(start, end)| packages(&bytes[start..end], depth + 1))
        .sum()
}

#[test]
#[ignore = "requires both original weapon/enemy/magic archives; no media conversion"]
fn original_attachment_settings_reconstruct_all_physical_sources_on_both_discs() -> Result<()> {
    use crate::battle::{
        all::read_range,
        archive_directories::{self, Archive},
        embedded::Layout,
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
    let mut both = Vec::new();
    for disc in [1, 2] {
        let files = root.join(format!("disc{disc}/files"));
        let rel = crate::rel::Rel::read(&files.join("US_r_Top2Btl.rel"))?;
        let ranges = |kind: Archive| -> Result<_> {
            archive_directories::read(
                &rel,
                Layout::RETAIL.archives,
                kind,
                &format!("BTL/{}", kind.file()),
                files.join("BTL").join(kind.file()).metadata()?.len(),
            )?
            .into_ranges()
        };
        let mut counts = [0; 5]; // Weapon packages/recipes, enemies/recipes, Pow recipes.
        for (id, range) in ranges(Archive::Weapon)? {
            let bytes = read_range(&files.join("BTL/BTLwepon.dat"), range)?;
            let count = packages(&bytes, 0).with_context(|| format!("disc{disc} weapon{id}"))?;
            ensure!(count > 0, "weapon{id} contains no attachment settings");
            counts[0] += 1;
            counts[1] += count;
        }
        let usual = fs::read(files.join("BTL/BTLusual.dat"))?;
        let directory = crate::battle::actions::member(&usual, 10)?;
        let offsets = directory
            .chunks_exact(4)
            .map(|row| word(row, 0))
            .collect::<Result<Vec<_>>>()?;
        let enemy = files.join("BTL/BTLenemy.dat");
        for (id, range) in
            crate::battle::all::physical_ranges(&offsets, 0, enemy.metadata()?.len())?
        {
            let bytes = crate::compression::decode(&read_range(&enemy, range)?)?;
            let starts = (0x160..0x180)
                .step_by(4)
                .map(|at| word(&bytes, at))
                .collect::<Result<BTreeSet<_>>>()?;
            for start in starts.into_iter().filter(|&start| start != 0) {
                let member =
                    crate::battle::enemy_inventory::offset_section(&bytes, start as usize)?;
                let count = packages(member, 0)
                    .with_context(|| format!("disc{disc} enemy{id} attachment{start:x}"))?;
                ensure!(count > 0, "enemy{id} attachment contains no settings");
                counts[3] += count;
            }
            counts[2] += 1;
        }
        let magic = ranges(Archive::Magic)?;
        for kind in resonance_content::battle::unison::PowWeapon::ALL {
            let id = kind.native() - 200;
            let range = magic
                .iter()
                .find(|(slot, _)| *slot == id)
                .context("missing Pow package")?
                .1
                .clone();
            let bytes = read_range(&files.join("BTL/BTLmagic.dat"), range)?;
            let start = word(&bytes, 260)? as usize;
            ensure!(start >= 276, "missing Pow carried member");
            let end = (4..276)
                .step_by(4)
                .map(|at| word(&bytes, at))
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .map(|at| at as usize)
                .filter(|&at| at > start)
                .min()
                .unwrap_or(bytes.len());
            let count = packages(
                bytes
                    .get(start..end)
                    .context("Pow member outside package")?,
                0,
            )?;
            assert_eq!(count, 1, "native{} carried attachment", kind.native());
            counts[4] += count;
        }
        assert!(counts[0] > 0 && counts[1] >= counts[0] && counts[3] > 0);
        assert_eq!((counts[2], counts[4]), (251, 3));
        eprintln!(
            "disc{disc}: physical weapon packages/recipes, enemies/recipes, Pow recipes {counts:?}"
        );
        both.push(counts);
    }
    assert_eq!(both[0], both[1]);
    Ok(())
}
