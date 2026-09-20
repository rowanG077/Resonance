//! Battle preparation selects resident voices from the complete cooked library.
use crate::media::voice_library::Archive;
use anyhow::{Context, Result, ensure};
use resonance_content::field_audio::{Asset, Voice};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub(super) fn source(extracted: &Path, executable: &[u8]) -> Result<String> {
    let directory = crate::voice_directory::Directory::read(executable)?;
    // Battle stream playback uses the resident archive handle, independently of voice ID groups.
    let mut entries = directory.active().filter(|entry| entry.resource_id == 0x78);
    let entry = entries
        .next()
        .context("missing resident battle voice archive")?;
    ensure!(
        entries.next().is_none(),
        "ambiguous resident battle voice archive"
    );
    crate::all_assets::roles::declared_path(&extracted.join("files"), &entry.source_path()?)
}

pub(super) fn voices(
    root: &Path,
    disc: u8,
    source: &str,
    ids: &BTreeSet<u32>,
) -> Result<(Asset, BTreeMap<u32, Voice>)> {
    let archive = Archive::open(root, disc, source)?;
    let mut voices = BTreeMap::new();
    for &id in ids {
        let voice = archive.voice(id as usize)?;
        ensure!(
            voice.source_name.ends_with(".adx"),
            "battle voice {id} is not ADX"
        );
        voices.insert(id, voice);
    }
    Ok((archive.source, voices))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::hash_file;
    use resonance_content::field_audio::archive::{Member, MemberKind, VoiceArchive};
    use std::fs;

    #[test]
    fn binds_renamed_declared_voice_archive_and_rejects_missing_or_changed_data() -> Result<()> {
        let root = crate::temporary_path(&std::env::temp_dir().join("resonance-voice-binding"));
        fs::create_dir(&root)?;
        let result = (|| -> Result<()> {
            let extracted = root.join("extracted");
            fs::create_dir_all(extracted.join("files/Data"))?;
            fs::write(extracted.join("files/Data/Voices.bin"), [])?;
            let mut executable = vec![0; 0x220];
            for (at, value) in [
                (0, 0x100u32),
                (0x48, 0x802a30a0),
                (0x90, 0x120),
                (0x100, 0x802a3180),
            ] {
                executable[at..at + 4].copy_from_slice(&value.to_be_bytes());
            }
            executable[0x108..0x10a].copy_from_slice(&0x78u16.to_be_bytes());
            executable[0x1e0..0x1f0].copy_from_slice(b"data/voices.bin\0");
            let source_path = source(&extracted, &executable)?;
            assert_eq!(source_path, "Data/Voices.bin");
            executable[0x108..0x10a].fill(0);
            assert!(source(&extracted, &executable).is_err());
            let ids = BTreeSet::from([0]);
            assert!(
                voices(&root, 1, &source_path, &ids)
                    .unwrap_err()
                    .to_string()
                    .contains("run cook-all")
            );
            let source_hash = crate::digest(b"authored ADX");
            let path = format!("audio/streams/{source_hash}.wav");
            fs::create_dir_all(root.join("audio/streams"))?;
            crate::media::write_pcm16(&root.join(&path), 1, 22050, [5, -5, 7, -7])?;
            let voice = Voice {
                asset: Asset {
                    path,
                    sha256: hash_file(&root.join(format!("audio/streams/{source_hash}.wav")))?,
                },
                frames: 4,
                sample_rate: 22050,
                source_sample_rate: 22050,
                channels: 1,
                source_name: "test.adx".into(),
                source_sha256: source_hash,
            };
            let source = Asset {
                path: source_path.clone(),
                sha256: crate::digest(b"archive"),
            };
            let metadata = format!("audio/archives/{}/0.json", source.sha256);
            let mut archive = VoiceArchive {
                version: VoiceArchive::VERSION,
                source_sha256: source.sha256,
                members: vec![Member {
                    name: "test.adx".into(),
                    metadata: metadata.clone(),
                    kind: MemberKind::Stream,
                }],
            };
            let index_path = VoiceArchive::path(&archive.source_sha256)?;
            crate::write_atomic(
                &root.join("sources.json"),
                &serde_json::to_vec(&BTreeMap::from([(
                    format!("disc1/{source_path}"),
                    vec![index_path.clone()],
                )]))?,
            )?;
            let index = root.join(index_path);
            crate::write_atomic(&index, &serde_json::to_vec(&archive)?)?;
            crate::write_atomic(&root.join(&metadata), &serde_json::to_vec(&voice)?)?;
            let (_, bound) = voices(&root, 1, &source_path, &ids)?;
            assert_eq!(bound[&0].asset.path, voice.asset.path);
            assert_eq!(bound[&0].asset.sha256, voice.asset.sha256);
            assert!(voices(&root, 1, &source_path, &BTreeSet::from([1])).is_err());
            archive.members[0].kind = MemberKind::Empty;
            crate::write_atomic(&index, &serde_json::to_vec(&archive)?)?;
            assert!(voices(&root, 1, &source_path, &ids).is_err());
            archive.members[0].kind = MemberKind::Stream;
            crate::write_atomic(&index, &serde_json::to_vec(&archive)?)?;
            fs::write(root.join(&voice.asset.path), b"broken")?;
            assert!(voices(&root, 1, &source_path, &ids).is_err());
            fs::remove_file(root.join(&metadata))?;
            assert!(voices(&root, 1, &source_path, &ids).is_err());
            Ok(())
        })();
        fs::remove_dir_all(root)?;
        result
    }

    #[test]
    #[ignore = "requires cook-all's complete battle voice library; no extraction or codecs"]
    fn original_cooked_battle_voice_library_binds_every_stream_on_both_discs() -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets");
        let mut prior = BTreeMap::new();
        for disc in [1, 2] {
            let extracted = root.parent().unwrap().join(format!("extracted/disc{disc}"));
            let source = source(&extracted, &fs::read(extracted.join("sys/main.dol"))?)?;
            let archive = Archive::open(&root, disc, &source)?;
            let ids = (0..archive.index.members.len() as u32).collect();
            let (_, voices) = voices(&root, disc, &source, &ids)?;
            assert!(!voices.is_empty());
            let assets: BTreeMap<_, _> = voices
                .into_iter()
                .map(|(id, voice)| (id, (voice.asset.path, voice.asset.sha256)))
                .collect();
            if disc == 1 {
                prior = assets;
            } else {
                assert_eq!(assets, prior);
            }
        }
        Ok(())
    }
}
