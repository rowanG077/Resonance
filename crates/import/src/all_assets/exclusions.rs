//! Exclude build sidecars only after recognizing their complete contents.
use crate::read::u32 as word;
use anyhow::{Context, Result, bail, ensure};
use std::{fs, io::Read, path::Path};

enum Metadata<'a> {
    ModuleNames(Vec<&'a str>),
    Checksums(Vec<(&'a str, u32)>),
}

fn metadata<'a>(bytes: &'a [u8], extension: &str) -> Result<Metadata<'a>> {
    let text = std::str::from_utf8(bytes).context("non-text build sidecar")?;
    if extension.eq_ignore_ascii_case("str") {
        let text = text
            .strip_suffix('\0')
            .context("unterminated module names")?;
        let names: Vec<_> = text.split('\0').collect();
        ensure!(
            names.iter().all(|name| {
                name.is_ascii()
                    && !name.bytes().any(|b| b.is_ascii_control())
                    && name.ends_with(".plf")
                    && name.as_bytes().get(1..3) == Some(b":\\")
            }),
            "STR contains more than linker module paths"
        );
        Ok(Metadata::ModuleNames(names))
    } else if extension.eq_ignore_ascii_case("sfv") {
        ensure!(
            text.bytes()
                .all(|b| b.is_ascii_graphic() || matches!(b, b' ' | b'\t' | b'\r' | b'\n')),
            "non-text checksum sidecar"
        );
        let mut entries = Vec::new();
        for line in text.lines().map(str::trim) {
            if line.is_empty() || line.starts_with(';') {
                continue;
            }
            let (name, checksum) = line.rsplit_once(' ').context("invalid SFV row")?;
            ensure!(
                !name.is_empty()
                    && !name.bytes().any(|b| b.is_ascii_control())
                    && checksum.len() == 8
                    && checksum.bytes().all(|b| b.is_ascii_hexdigit()),
                "invalid SFV filename or checksum"
            );
            entries.push((name, u32::from_str_radix(checksum, 16)?));
        }
        ensure!(!entries.is_empty(), "empty checksum sidecar");
        Ok(Metadata::Checksums(entries))
    } else {
        bail!("unrecognized build sidecar extension")
    }
}

pub(super) fn validate_build_metadata(path: &Path) -> Result<()> {
    let bytes = fs::read(path)?;
    let directory = path.parent().context("sidecar has no directory")?;
    match metadata(
        &bytes,
        path.extension().and_then(|e| e.to_str()).unwrap_or(""),
    )? {
        Metadata::ModuleNames(names) => {
            // REL nameOffset/nameSize address this external linker string table.
            let mut offset = 0;
            for name in names {
                let filename = name.rsplit('\\').next().unwrap();
                let module = directory.join(filename).with_extension("rel");
                let mut header = [0; 28];
                fs::File::open(&module)?.read_exact(&mut header)?;
                ensure!(
                    word(&header, 20)? as usize == offset
                        && word(&header, 24)? as usize == name.len() + 1,
                    "module name does not match {}",
                    module.display()
                );
                offset += name.len() + 1;
            }
        }
        Metadata::Checksums(entries) => {
            for (name, expected) in entries {
                ensure!(
                    Path::new(name)
                        .components()
                        .all(|part| matches!(part, std::path::Component::Normal(_))),
                    "checksum path leaves its source directory"
                );
                let mut crc = !0_u32;
                for byte in fs::read(directory.join(name))? {
                    crc ^= u32::from(byte);
                    for _ in 0..8 {
                        const IEEE_CRC32_REVERSED: u32 = 0xedb8_8320;
                        crc = (crc >> 1) ^ IEEE_CRC32_REVERSED.wrapping_mul(crc & 1);
                    }
                }
                ensure!(!crc == expected, "SFV checksum mismatch for {name}");
            }
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires both extracted discs and the physical cook; no media conversion"]
fn original_exclusions_identify_metadata_native_containers_and_deferred_battle() -> Result<()> {
    use std::collections::BTreeMap;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
    let cooked = std::env::var_os("RESONANCE_COOKED")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("all-assets"));
    let excluded: BTreeMap<String, String> =
        serde_json::from_slice(&fs::read(cooked.join("excluded.json"))?)?;
    let sources: BTreeMap<String, Vec<String>> =
        serde_json::from_slice(&fs::read(cooked.join("sources.json"))?)?;
    let deferred: Vec<serde_json::Value> =
        serde_json::from_slice(&fs::read(cooked.join("deferred.json"))?)?;
    let check_deferred = |source: &str, bytes: &[u8]| -> Result<()> {
        let entry = deferred
            .iter()
            .find(|entry| entry["path"] == source)
            .context("deferred source has no coverage explanation")?;
        assert_eq!(entry["source_sha256"], crate::digest(bytes));
        assert!(!entry["reason"].as_str().unwrap().is_empty());
        Ok(())
    };
    let mut counts = [0; 4];
    for (source, reason) in &excluded {
        let (disc, relative) = source.split_once('/').context("invalid exclusion source")?;
        let extracted = root.join("extracted").join(disc);
        let path = if relative.starts_with("sys/") {
            extracted.join(relative)
        } else {
            extracted.join("files").join(relative)
        };
        let bytes = fs::read(&path)?;
        match reason.as_str() {
            "build_metadata" => {
                validate_build_metadata(&path).with_context(|| source.clone())?;
                counts[0] += 1;
            }
            "native_code" if relative == "sys/apploader.img" => {
                // SDK loader header + loader program + reboot trailer, with no tail.
                assert_eq!(
                    bytes.len(),
                    32 + word(&bytes, 20)? as usize + word(&bytes, 24)? as usize
                );
                assert!(
                    (0x8120_0000..0x8120_0000 + word(&bytes, 20)?).contains(&word(&bytes, 16)?)
                );
                assert!(bytes.windows(21).any(|s| s == b"Apploader Initialized"));
                counts[1] += 1;
            }
            "native_code" => {
                if relative.ends_with(".rel") {
                    crate::rel::Rel::read(&path)?;
                } else {
                    assert_eq!(relative, "sys/main.dol");
                }
                let outputs = &sources[source];
                // Whole battle modules remain explicitly deferred. Other native
                // containers must publish their recovered tables and artwork.
                if outputs.is_empty() {
                    check_deferred(source, &bytes)?;
                }
                for output in outputs {
                    assert!(cooked.join(output).exists(), "{output}");
                }
                counts[2] += 1;
            }
            "disc_metadata" => {
                match relative {
                    "sys/boot.bin" => {
                        assert_eq!(bytes.len(), 0x440);
                        assert_eq!(word(&bytes, 0x1c)?, 0xc233_9f3d);
                        assert_eq!(
                            word(&bytes, 0x428)? as u64,
                            extracted.join("sys/fst.bin").metadata()?.len()
                        );
                    }
                    "sys/bi2.bin" => {
                        assert_eq!(bytes.len(), nod::disc::BI2_SIZE);
                        assert!(bytes[0x2c..].iter().all(|&b| b == 0));
                    }
                    "sys/fst.bin" => {
                        let fst = nod::disc::fst::Fst::new(&bytes).map_err(anyhow::Error::msg)?;
                        for (_, node, name) in fst.iter().filter(|(_, node, _)| node.is_file()) {
                            assert_eq!(
                                extracted.join("files").join(&name).metadata()?.len(),
                                u64::from(node.length())
                            );
                            assert!(
                                sources.contains_key(&format!("{disc}/{name}")),
                                "FST file absent from cook: {name}"
                            );
                        }
                    }
                    _ => bail!("unrecognized disc metadata: {source}"),
                }
                counts[3] += 1;
            }
            "battle_semantics" => {
                check_deferred(source, &bytes)?;
            }
            _ => bail!("unrecognized exclusion: {source}: {reason}"),
        }
    }
    assert_eq!(counts, [15, 2, 30, 6]);
    for (extension, bytes) in [
        ("str", b"texture payload".as_slice()),
        ("str", b"c:\\build\\module.plf\0opaque\0"),
        ("str", b"c:\\build\\module.plf"),
        ("sfv", b"texture payload"),
        ("sfv", b"image.dat 123456789"),
        ("sfv", b"image.dat NOTACRC!"),
        ("sfv", b";hidden\0payload\nimage.dat 12345678"),
    ] {
        assert!(metadata(bytes, extension).is_err());
    }
    Ok(())
}
