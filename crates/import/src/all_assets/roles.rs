//! Native resource declarations select readers for archives without a format signature.
use crate::{dol, event_bank_directory, field_resources::resolve_path, music_directory, rel::Rel};
use anyhow::{Context, Result, ensure};
use std::{collections::BTreeMap, fs, path::Path};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Role {
    Field,
    Victory,
    BattleSkit,
    VoiceBank,
    Font,
    SoundBank,
    Song,
    Credits,
}

pub(super) fn credits_path(extracted: &Path, executable: &[u8]) -> Result<String> {
    declared_path(
        &extracted.join("files"),
        &super::credits::Resources::read(executable)?.text,
    )
}

const VOICE_BANK: usize = 0x23b4;
const VICTORY: usize = 0x1e4;
const RESIDENT_BANKS: [u32; 2] = [0x8017d7a0, 0x8017d7ac];
const PARTY_BANKS: u32 = 0x8017d7b8;
const TOON: u32 = 0x8017cf7c;
const TITLE: u32 = 0x8017d870;
const EFFECTS: u32 = 0x8017a568;
const STATUS_PORTRAITS: u32 = 0x80199dbc;

pub(crate) fn toon_path(extracted: &Path, executable: &[u8]) -> Result<String> {
    declared_path(&extracted.join("files"), &dol::text(executable, TOON)?)
}

pub(crate) fn title_path(extracted: &Path, executable: &[u8]) -> Result<String> {
    declared_path(&extracted.join("files"), &dol::text(executable, TITLE)?)
}

#[cfg(test)]
pub(crate) fn effects_path(extracted: &Path, executable: &[u8]) -> Result<String> {
    declared_path(&extracted.join("files"), &effects_declaration(executable)?)
}

pub(crate) fn effects_declaration(executable: &[u8]) -> Result<String> {
    dol::text(executable, EFFECTS)
}

#[cfg(test)]
fn status_portraits_path(extracted: &Path, executable: &[u8]) -> Result<String> {
    declared_path(
        &extracted.join("files"),
        &status_portraits_declaration(executable)?,
    )
}

pub(crate) fn status_portraits_declaration(executable: &[u8]) -> Result<String> {
    let declaration = dol::text(executable, STATUS_PORTRAITS)?;
    // A leading slash names the original disc root, not the host filesystem root.
    let path = declaration
        .strip_prefix('/')
        .or_else(|| declaration.strip_prefix("./"))
        .unwrap_or(&declaration);
    resonance_content::validate_asset_path(path)?;
    Ok(path.to_owned())
}

/// Startup loads instruments first, then the common sound effects.
pub(crate) fn resident_banks(extracted: &Path, executable: &[u8]) -> Result<[String; 2]> {
    let files = extracted.join("files");
    let [instruments, common] = RESIDENT_BANKS;
    Ok([
        declared_path(&files, &dol::text(executable, instruments)?)?,
        declared_path(&files, &dol::text(executable, common)?)?,
    ])
}

pub(crate) fn party_banks(extracted: &Path, executable: &[u8]) -> Result<Vec<String>> {
    (0..9)
        .map(|index| {
            declared_path(
                &extracted.join("files"),
                &dol::text(executable, PARTY_BANKS + index * 20)?,
            )
        })
        .collect()
}

pub(crate) fn audio_paths(extracted: &Path, executable: &[u8]) -> Result<BTreeMap<String, Role>> {
    let files = extracted.join("files");
    let mut paths = BTreeMap::new();
    for address in RESIDENT_BANKS
        .into_iter()
        .chain((0..9).map(|i| PARTY_BANKS + i * 20))
    {
        insert(
            &mut paths,
            &files,
            &dol::text(executable, address)?,
            Role::SoundBank,
        )?;
    }
    for entry in event_bank_directory::Directory::read(executable)?.entries {
        insert(&mut paths, &files, &entry.source_path()?, Role::SoundBank)?;
    }
    for entry in music_directory::Directory::read(executable)?.active() {
        insert(&mut paths, &files, &entry.source_path()?, Role::Song)?;
    }
    Ok(paths)
}

fn battle_module(files: &Path) -> Result<Rel> {
    let module = resolve_path(files, "US_r_Top2Btl.rel")?;
    Rel::read(&files.join(module))
}

pub(crate) fn voice_bank_path(extracted: &Path) -> Result<String> {
    let files = extracted.join("files");
    declared_path(&files, &battle_module(&files)?.text((4, VOICE_BANK))?)
}

#[cfg(test)]
pub(crate) fn victory_path(extracted: &Path) -> Result<String> {
    let files = extracted.join("files");
    declared_path(&files, &battle_module(&files)?.text((4, VICTORY))?)
}

pub(super) fn battle_paths(extracted: &Path) -> Result<BTreeMap<String, Role>> {
    let files = extracted.join("files");
    let rel = battle_module(&files)?;
    let mut paths = BTreeMap::new();
    for (offset, role) in [
        (0x1d0, Role::BattleSkit),
        (VICTORY, Role::Victory),
        (VOICE_BANK, Role::VoiceBank),
    ] {
        insert(&mut paths, &files, &rel.text((4, offset))?, role)
            .with_context(|| format!("{role:?} resource declaration at 4:{offset:#x}"))?;
    }
    Ok(paths)
}

fn insert(
    paths: &mut BTreeMap<String, Role>,
    files: &Path,
    declaration: &str,
    role: Role,
) -> Result<()> {
    let path = declared_path(files, declaration)?;
    ensure!(
        paths
            .insert(path.clone(), role)
            .is_none_or(|previous| previous == role),
        "conflicting resource roles for {path:?}"
    );
    Ok(())
}

pub(crate) fn declared_path(files: &Path, declaration: &str) -> Result<String> {
    let path = declaration.strip_prefix("./").unwrap_or(declaration);
    let path = resolve_path(files, path)?;
    let source = files.join(&path);
    ensure!(
        source.metadata()?.is_file(),
        "resource is not a file: {path:?}"
    );
    fs::File::open(source).with_context(|| format!("unreadable resource {path:?}"))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resident_textures_follow_renamed_declarations() -> Result<()> {
        let root = crate::temporary_path(&std::env::temp_dir().join("renamed-resident-textures"));
        let result = (|| -> Result<()> {
            fs::create_dir_all(root.join("files/Art"))?;
            fs::write(root.join("files/Art/Other.data"), [])?;
            for (address, read) in [
                (TOON, toon_path as fn(&Path, &[u8]) -> Result<String>),
                (TITLE, title_path),
                (EFFECTS, effects_path),
                (STATUS_PORTRAITS, status_portraits_path),
            ] {
                let declaration = b"./art/other.data\0";
                let mut executable = vec![0; 0x100 + declaration.len()];
                for (at, value) in [
                    (0, 0x100),
                    (0x48, address),
                    (0x90, declaration.len() as u32),
                ] {
                    executable[at..at + 4].copy_from_slice(&value.to_be_bytes());
                }
                executable[0x100..].copy_from_slice(declaration);
                assert_eq!(read(&root, &executable)?, "Art/Other.data");
                executable[0x100..].fill(b'x');
                assert!(read(&root, &executable).is_err());
                executable[0x100..].fill(0);
                executable[0x100..0x10b].copy_from_slice(b"absent.tpl\0");
                assert!(read(&root, &executable).is_err());
            }
            Ok(())
        })();
        let cleanup = fs::remove_dir_all(root);
        result?;
        cleanup?;
        Ok(())
    }

    #[test]
    #[ignore = "requires both extracted discs; no texture conversion"]
    fn original_resident_texture_declarations_match_native_image_selection() -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        for disc in ["disc1", "disc2"] {
            let extracted = root.join(disc);
            let executable = fs::read(extracted.join("sys/main.dol"))?;
            for (address, path, expected) in [
                (0x8017cf7c, toon_path(&extracted, &executable)?, "toon.tpl"),
                (
                    0x8017d870,
                    title_path(&extracted, &executable)?,
                    "title.tpl",
                ),
            ] {
                assert_eq!(dol::text(&executable, address)?, expected);
                assert_eq!(path, expected);
                let bytes = fs::read(extracted.join("files").join(path))?;
                let images = crate::tpl::parse_tpl(&bytes)?;
                if address == 0x8017cf7c {
                    assert_eq!(
                        (images[0].width, images[0].height, images[0].format),
                        (256, 32, 8)
                    );
                } else {
                    assert!(images.len() > 17);
                    assert_eq!(crate::resource::read(&executable)?.source(35)?, expected);
                }
            }
        }
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    #[ignore = "requires both extracted discs and cook-all; no texture conversion"]
    fn status_portraits_follow_renamed_source_declarations() -> Result<()> {
        use std::os::unix::fs::symlink;
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let cooked = local.join("all-assets");
        let root = crate::temporary_path(&std::env::temp_dir().join("portrait-declaration"));
        let result = (|| -> Result<()> {
            fs::create_dir_all(root.join("files/Art"))?;
            fs::create_dir_all(root.join("cooked"))?;
            symlink(cooked.join("assets"), root.join("cooked/assets"))?;
            for disc in [1, 2] {
                let extracted = local.join(format!("extracted/disc{disc}"));
                let mut executable = fs::read(extracted.join("sys/main.dol"))?;
                assert_eq!(dol::text(&executable, STATUS_PORTRAITS)?, "/field/s.z");
                let path = status_portraits_path(&extracted, &executable)?;
                let source = crate::cooked::Source::open(&cooked, disc, &path)?;
                let expected = source.resolve("cabinet.json")?;
                symlink(
                    extracted.join("files").join(path),
                    root.join("files/Art/P.dat"),
                )?;
                fs::write(
                    root.join("cooked/sources.json"),
                    serde_json::to_vec(&BTreeMap::from([(
                        format!("disc{disc}/Art/P.dat"),
                        source.publications(),
                    )]))?,
                )?;
                let bytes = dol::slice(&executable, STATUS_PORTRAITS, 11)?;
                let offset = bytes.as_ptr() as usize - executable.as_ptr() as usize;
                executable[offset..offset + 11].copy_from_slice(b"/art/p.dat\0");
                assert_eq!(status_portraits_declaration(&executable)?, "art/p.dat");
                let renamed = status_portraits_path(&root, &executable)?;
                assert_eq!(renamed, "Art/P.dat");
                let output = root.join("cooked");
                let source = crate::cooked::Source::open(&output, disc, &renamed)?;
                assert_eq!(source.resolve("cabinet.json")?, expected);
                fs::remove_file(root.join("files/Art/P.dat"))?;
                assert!(status_portraits_path(&root, &executable).is_err());
            }
            Ok(())
        })();
        if root.exists() {
            fs::remove_dir_all(root)?;
        }
        result
    }

    #[test]
    fn declarations_resolve_case_aliases_and_reject_invalid_paths() -> Result<()> {
        let root = crate::temporary_path(&std::env::temp_dir().join("battle-resource-roles"));
        let result = (|| -> Result<()> {
            fs::create_dir_all(root.join("Source/Directory"))?;
            fs::write(root.join("Source/Record.pkg"), [])?;
            assert_eq!(
                declared_path(&root, "./source/record.pkg")?,
                "Source/Record.pkg"
            );
            for path in [
                "./source/missing.pkg",
                "./source/directory",
                "./source/record.pkg/child",
                "./../record.pkg",
                "/source/record.pkg",
                "./source//record.pkg",
                "./source/",
                "./",
            ] {
                assert!(declared_path(&root, path).is_err(), "{path}");
            }
            fs::write(root.join("Source/RECORD.pkg"), [])?;
            assert!(declared_path(&root, "./source/record.pkg").is_err());
            Ok(())
        })();
        let cleanup = fs::remove_dir_all(root);
        result?;
        cleanup?;
        Ok(())
    }

    #[test]
    fn resident_battle_files_use_their_declarations_instead_of_filenames() -> Result<()> {
        for (offset, read) in [
            (VOICE_BANK, voice_bank_path as fn(&Path) -> Result<String>),
            (VICTORY, victory_path),
        ] {
            let root = crate::temporary_path(&std::env::temp_dir().join("renamed-voice-bank"));
            let result = (|| -> Result<()> {
                let files = root.join("files");
                fs::create_dir_all(files.join("Audio"))?;
                fs::write(files.join("Audio/Other.bin"), [])?;
                let declaration = b"./audio/other.bin\0";
                let size = offset + declaration.len();
                let mut bytes = vec![0; 0x100 + size];
                for (at, value) in [(12, 5), (16, 0x4c), (0x6c, 0x100), (0x70, size as u32)] {
                    bytes[at..at + 4].copy_from_slice(&value.to_be_bytes());
                }
                bytes[0x100 + offset..].copy_from_slice(declaration);
                let module = files.join("us_r_top2btl.rel");
                fs::write(&module, &bytes)?;
                assert_eq!(read(&root)?, "Audio/Other.bin");
                fs::remove_file(files.join("Audio/Other.bin"))?;
                assert!(read(&root).is_err());
                bytes[0x100 + offset..].fill(b'x');
                fs::write(&module, &bytes)?;
                assert!(read(&root).is_err());
                bytes[0x100 + offset..].fill(0);
                bytes[0x100 + offset..0x100 + offset + 8].copy_from_slice(b"../oops\0");
                fs::write(module, bytes)?;
                assert!(read(&root).is_err());
                Ok(())
            })();
            let cleanup = fs::remove_dir_all(root);
            result?;
            cleanup?;
        }
        Ok(())
    }

    #[test]
    fn audio_declarations_ignore_extensions_and_reject_conflicts_and_missing_files() -> Result<()> {
        let root = crate::temporary_path(&std::env::temp_dir().join("renamed-audio"));
        let result = (|| -> Result<()> {
            fs::create_dir_all(root.join("files/A"))?;
            for name in ["One.song", "Two.bin", "Three.snd"] {
                fs::write(root.join("files/A").join(name), [])?;
            }
            let mut executable = vec![0; 0x200 + 0x4c + 113 * 8];
            for (at, value) in [
                (0, 0x100u32),
                (0x48, RESIDENT_BANKS[0]),
                (0x90, 0x100),
                (4, 0x200),
                (0x4c, 0x801f97e0),
                (0x94, 0x4c + 113 * 8),
            ] {
                executable[at..at + 4].copy_from_slice(&value.to_be_bytes());
            }
            for (at, name) in [
                (0x100, "a/one.song"),
                (0x10c, "a/two.bin"),
                (0x1e0, "a/three.snd"),
                (0x1f0, "a/missing"),
            ] {
                executable[at..at + name.len()].copy_from_slice(name.as_bytes());
            }
            for i in 0..9 {
                executable[0x118 + i * 20..0x121 + i * 20].copy_from_slice(b"a/two.bin");
            }
            for row in executable[0x200..0x240].chunks_exact_mut(8) {
                row[..4].copy_from_slice(&RESIDENT_BANKS[1].to_be_bytes());
            }
            for row in executable[0x24c..].chunks_exact_mut(8) {
                row[..2].copy_from_slice(&(-1i16).to_be_bytes());
            }
            for (i, id, pointer) in [(0, 7i16, 0xe0), (1, 8, 0xe0), (3, 9, 0xf0)] {
                let at = 0x24c + i * 8;
                executable[at..at + 2].copy_from_slice(&id.to_be_bytes());
                executable[at + 4..at + 8]
                    .copy_from_slice(&(RESIDENT_BANKS[0] + pointer).to_be_bytes());
            }
            assert_eq!(
                resident_banks(&root, &executable)?,
                ["A/One.song", "A/Two.bin"]
            );
            assert_eq!(
                audio_paths(&root, &executable)?,
                BTreeMap::from([
                    ("A/One.song".into(), Role::SoundBank),
                    ("A/Two.bin".into(), Role::SoundBank),
                    ("A/Three.snd".into(), Role::Song),
                ])
            );
            let mut conflict = executable.clone();
            conflict[0x250..0x254].copy_from_slice(&RESIDENT_BANKS[0].to_be_bytes());
            assert!(audio_paths(&root, &conflict).is_err());
            fs::remove_file(root.join("files/A/Three.snd"))?;
            assert!(audio_paths(&root, &executable).is_err());
            Ok(())
        })();
        let cleanup = fs::remove_dir_all(root);
        result?;
        cleanup?;
        Ok(())
    }

    #[test]
    #[ignore = "requires both original extracted discs; no audio conversion"]
    fn original_audio_declarations_cover_native_banks_and_songs() -> Result<()> {
        use crate::read::{u16 as half, u32 as word};
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        for disc in [1, 2] {
            let extracted = root.join(format!("disc{disc}"));
            let files = extracted.join("files");
            let executable = fs::read(extracted.join("sys/main.dol"))?;
            let mut expected = BTreeMap::new();
            let mut declaration = |address, role| -> Result<()> {
                let authored = dol::text(&executable, address)?;
                let path = resolve_path(&files, authored.strip_prefix("./").unwrap_or(&authored))?;
                expected.insert(path, role);
                Ok(())
            };
            for offset in [
                0x818, 0x824, 0x830, 0x844, 0x858, 0x86c, 0x880, 0x894, 0x8a8, 0x8bc, 0x8d0,
            ] {
                declaration(0x8017cf88 + offset, Role::SoundBank)?;
            }
            for row in dol::slice(&executable, 0x801f97e0, 8 * 8)?.chunks_exact(8) {
                declaration(word(row, 0)?, Role::SoundBank)?;
            }
            let mut ids = 0;
            for row in dol::slice(&executable, 0x801f982c, 113 * 8)?.chunks_exact(8) {
                if half(row, 0)? == u16::MAX {
                    break;
                }
                declaration(word(row, 4)?, Role::Song)?;
                ids += 1;
            }
            assert_eq!(ids, 112);
            let actual = audio_paths(&extracted, &executable)?;
            assert_eq!(actual, expected);
            assert_eq!(
                actual
                    .values()
                    .filter(|&&role| role == Role::SoundBank)
                    .count(),
                19
            );
            assert_eq!(
                actual.values().filter(|&&role| role == Role::Song).count(),
                112
            );
            assert_eq!(
                resident_banks(&extracted, &executable)?,
                ["S/inst.snd", "S/se.snd"]
            );
            for (path, role) in &actual {
                let bytes = fs::read(files.join(path))?;
                match role {
                    Role::SoundBank => {
                        resonance_audio_cook::bank::Bank::parse(&bytes)?;
                    }
                    Role::Song => {
                        resonance_audio_cook::song::Song::parse(&bytes)?;
                    }
                    _ => unreachable!(),
                }
            }
            // Unreferenced physical resources require structural discovery too.
            let mut undeclared = [0; 2];
            for directory in ["S", "BTL"] {
                for entry in fs::read_dir(files.join(directory))? {
                    let entry = entry?;
                    let path = entry.path();
                    let index = match path.extension().and_then(|extension| extension.to_str()) {
                        Some("snd") => 0,
                        Some("song") => 1,
                        _ => continue,
                    };
                    let source = format!("{directory}/{}", entry.file_name().to_string_lossy());
                    undeclared[index] += usize::from(!actual.contains_key(&source));
                }
            }
            assert_eq!(undeclared, [70, 6]);
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires both original extracted discs; no archive conversion"]
    fn original_battle_archive_declarations_resolve_on_both_discs() -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        for disc in [1, 2] {
            let extracted = root.join(format!("disc{disc}"));
            let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel"))?;
            let paths = battle_paths(&extracted)?;
            assert_eq!(paths.len(), 3);
            for (offset, source, physical, role) in [
                (
                    0x1d0,
                    "./Btl/BTLskit.dat",
                    "BTL/BTLskit.dat",
                    Role::BattleSkit,
                ),
                (0x1e4, "./Btl/BTLwin.bfp", "BTL/BTLwin.bfp", Role::Victory),
                (
                    VOICE_BANK,
                    "./Btl/BTLvbank.dat",
                    "BTL/BTLvbank.dat",
                    Role::VoiceBank,
                ),
            ] {
                assert_eq!(rel.text((4, offset))?, source);
                assert_eq!(paths.get(physical), Some(&role));
            }
            assert_eq!(voice_bank_path(&extracted)?, "BTL/BTLvbank.dat");
        }
        Ok(())
    }
}
