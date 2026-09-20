use super::*;
use crate::media::voice_library::{Archive, silent};
use resonance_content::skit::SkitMedia;
use std::collections::BTreeSet;
use symphonia_script::NativeCall;

pub(super) fn cook(
    extracted: &Path,
    output: &Path,
    executable: &[u8],
    scripts: &BTreeMap<u16, SkitResourcePaths>,
) -> Result<BTreeMap<u32, SkitMedia>> {
    let directory = crate::voice_directory::Directory::read(executable)?;
    let mut requested = BTreeSet::new();
    for script in scripts.values() {
        let bytes = fs::read(output.join(&script.script))?;
        for arguments in
            crate::field_resources::literal_arguments(&bytes, NativeCall::PlayMovie, 1)?
        {
            let id = arguments[0]
                .with_context(|| format!("dynamic skit media request in {}", script.script))?;
            if id != -1 {
                requested.insert(id as u32);
            }
        }
    }
    bind(extracted, output, &directory, requested)
}

fn bind(
    extracted: &Path,
    output: &Path,
    directory: &crate::voice_directory::Directory,
    requested: BTreeSet<u32>,
) -> Result<BTreeMap<u32, SkitMedia>> {
    let mut result = BTreeMap::new();
    let mut groups = BTreeMap::<u32, Vec<u16>>::new();
    for id in requested {
        ensure!(
            id & 0xf0000000 != 0x80000000,
            "skit direct movie/stream request {id:#x} is not supported"
        );
        groups.entry(id & 0xffff0000).or_default().push(id as u16);
    }
    for (group, members) in groups {
        let path = directory.path(group)?;
        let archive = Archive::source(output, extracted, None, &path)?;
        for index in members {
            let mut voice = archive.voice(usize::from(index))?;
            voice.sample_rate = (u64::from(voice.source_sample_rate) * 32028 / 32000) as u32;
            let media = SkitMedia {
                frames: voice.frames,
                sample_rate: voice.sample_rate,
                source_sha256: voice.source_sha256.clone(),
                // US skits use silent streams as subtitle clocks. Check the actual
                // PCM; filenames and region do not establish silence.
                voice: if silent(output, &voice)? {
                    None
                } else {
                    Some(voice)
                },
            };
            ensure!(
                result.insert(group + index as u32, media).is_none(),
                "duplicate skit media ID"
            );
        }
    }
    println!(
        "Bound {} skit media clocks ({} audible tracks)",
        result.len(),
        result.values().filter(|m| m.voice.is_some()).count()
    );
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voice_directory::{Directory, Entry};
    use resonance_content::field_audio::{
        Asset, Voice,
        archive::{Member, MemberKind, VoiceArchive},
    };
    use std::io::{Read, Seek, SeekFrom};

    #[test]
    fn requested_members_use_native_groups_without_filename_rules() -> Result<()> {
        let root = crate::temporary_path(&std::env::temp_dir().join("resonance-skit-media"));
        let result = (|| -> Result<()> {
            let extracted = root.join("source");
            let source = "DATA/renamed.bin";
            crate::write_atomic(&extracted.join("sys/boot.bin"), b"GQSEAF\0\0")?;
            crate::write_atomic(&extracted.join("files").join(source), b"original archive")?;
            let hash = crate::digest(b"original stream");
            let pcm = format!("audio/streams/{hash}.wav");
            fs::create_dir_all(root.join("audio/streams"))?;
            crate::media::write_pcm16(&root.join(&pcm), 1, 32000, [1, -1, 0, 0])?;
            let voice = Voice {
                asset: Asset {
                    path: pcm.clone(),
                    sha256: crate::digest(&fs::read(root.join(pcm))?),
                },
                frames: 4,
                sample_rate: 32000,
                source_sample_rate: 32000,
                channels: 1,
                source_name: "renamed.blob".into(),
                source_sha256: hash,
            };
            let source_hash = crate::digest(b"original archive");
            let archive = VoiceArchive {
                version: VoiceArchive::VERSION,
                source_sha256: source_hash.clone(),
                members: [MemberKind::Stream, MemberKind::Empty]
                    .into_iter()
                    .enumerate()
                    .map(|(id, kind)| Member {
                        name: voice.source_name.clone(),
                        metadata: format!("audio/archives/{source_hash}/{id}.json"),
                        kind,
                    })
                    .collect(),
            };
            let index = VoiceArchive::path(&source_hash)?;
            crate::write_atomic(&root.join(&index), &serde_json::to_vec(&archive)?)?;
            crate::write_atomic(
                &root.join(&archive.members[0].metadata),
                &serde_json::to_vec(&voice)?,
            )?;
            crate::write_atomic(
                &root.join("sources.json"),
                &serde_json::to_vec(&BTreeMap::from([(
                    format!("disc1/{source}"),
                    vec![index.clone()],
                )]))?,
            )?;
            let directory = Directory {
                entries: [
                    (Some("data/renamed.bin"), 0xa0000),
                    (Some("data/shadowed.bin"), 0xa0000),
                    (Some("data/other-disc.bin"), 0xb0000),
                    (None, 0),
                ]
                .into_iter()
                .map(|(file, logical_base)| Entry {
                    file: file.map(str::to_owned),
                    logical_base,
                    resource_id: 0,
                    storage: 0,
                })
                .collect(),
            };
            let selected = BTreeSet::from([0xa0000]);
            let media = bind(&extracted, &root, &directory, selected.clone())?;
            assert_eq!(media.len(), 1);
            assert_eq!(
                (media[&0xa0000].frames, media[&0xa0000].sample_rate),
                (4, 32028)
            );
            assert_eq!(
                media[&0xa0000].voice.as_ref().unwrap().source_name,
                "renamed.blob"
            );
            assert!(bind(&extracted, &root, &directory, BTreeSet::new())?.is_empty());
            for id in [0xa0001, 0xb0000, 0xc0000, 0x80000000] {
                assert!(bind(&extracted, &root, &directory, BTreeSet::from([id])).is_err());
            }
            fs::remove_file(root.join(index))?;
            assert!(bind(&extracted, &root, &directory, selected).is_err());
            Ok(())
        })();
        fs::remove_dir_all(root)?;
        result
    }

    #[test]
    #[ignore = "requires both original discs and cook-all PCM; no codecs or audio device"]
    fn original_skit_clocks_bind_shared_silent_pcm_on_both_discs() -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let output = root.join("local/all-assets");
        let catalog: SkitCatalog =
            serde_json::from_slice(&fs::read(output.join("game/skits.json"))?)?;
        let mut previous = None;
        for disc in [1, 2] {
            let extracted = root.join(format!("local/extracted/disc{disc}"));
            let executable = fs::read(extracted.join("sys/main.dol"))?;
            let media = cook(&extracted, &output, &executable, &catalog.resources)?;
            assert!(!media.is_empty() && media.len() <= 3103);
            let mut rates = BTreeMap::new();
            let mut checked = 0;
            // Check IDs and clocks against source words, without the binding reader.
            for row in crate::dol::slice(&executable, 0x802a30a0, 18 * 12)?.chunks_exact(12) {
                let pointer = crate::read::u32(row, 0)?;
                if pointer == 0 {
                    break;
                }
                let path = crate::dol::text(&executable, pointer)?;
                let Some(file) = path.strip_prefix("cv/") else {
                    continue;
                };
                if !file.starts_with("cht_") {
                    continue;
                }
                let source = format!("CV/{file}");
                let group = crate::read::u32(row, 4)?;
                let mut file = fs::File::open(extracted.join("files").join(source))?;
                for (index, entry) in crate::afs::index(&mut file)?.iter().enumerate() {
                    file.seek(SeekFrom::Start(entry.offset))?;
                    let mut header = [0; 16];
                    file.read_exact(&mut header)?;
                    let source_rate = crate::read::u32(&header, 8)?;
                    let rate = (u64::from(source_rate) * 32028 / 32000) as u32;
                    *rates.entry(rate).or_insert(0) += 1;
                    let Some(track) = media.get(&(group + index as u32)) else {
                        continue;
                    };
                    assert_eq!(track.frames, crate::read::u32(&header, 12)?);
                    assert_eq!(track.sample_rate, rate);
                    assert!(track.frames > 0 && track.voice.is_none());
                    checked += 1;
                }
            }
            assert_eq!(rates, BTreeMap::from([(32028, 2838), (48042, 265)]));
            assert_eq!(checked, media.len());
            let current = serde_json::to_value(media)?;
            if let Some(previous) = &previous {
                assert_eq!(&current, previous);
            }
            previous = Some(current);
        }
        Ok(())
    }
}
