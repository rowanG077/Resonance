use super::*;
use crate::media::voice_library::Archive;
use std::collections::BTreeSet;

pub(super) fn voices(
    workspace: &Workspace,
    executable: &[u8],
    additional: Option<&Path>,
    ids: &BTreeSet<u32>,
) -> Result<(BTreeMap<u32, Voice>, BTreeMap<u32, serde_json::Value>)> {
    let directory = crate::voice_directory::Directory::read(executable)?;
    let mut voices = BTreeMap::new();
    let mut sources = BTreeMap::new();
    for group in ids
        .iter()
        .map(|id| id & 0xffff_0000)
        .collect::<BTreeSet<_>>()
    {
        let source = directory.path(group)?;
        let mut archive =
            Archive::source(&workspace.output, &workspace.extracted, additional, &source)?;
        for &id in ids.range(group..=(group | 0xffff)) {
            let mut voice = archive.voice((id & 0xffff) as usize)?;
            ensure!(
                voice.source_name.ends_with(".ahx"),
                "field voice {id:#x} is not AHX"
            );
            if voice.source_sample_rate == super::super::SAMPLE_RATE {
                voice.sample_rate = super::super::PLAYBACK_RATE;
            }
            voices.insert(id, voice);
        }
        sources.insert(
            group,
            json!({"game":"GQSEAF", "revision":0, "disc":archive.disc,
            "path":source,"sha256":archive.index.source_sha256}),
        );
    }
    Ok((voices, sources))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Seek, SeekFrom};

    #[test]
    #[ignore = "requires both original discs and cook-all PCM; no codecs or audio device"]
    fn original_field_voice_ids_bind_both_discs_and_disc_two_fallback() -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let primary = root.join("local/extracted/disc1");
        let additional = root.join("local/extracted/disc2");
        let cooked = std::env::var_os("RESONANCE_COOKED")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| root.join("local/all-assets"));
        let executable = fs::read(primary.join("sys/main.dol"))?;
        let mut ids = BTreeSet::new();
        let mut owners = BTreeMap::new();
        let mut expected = BTreeMap::new();
        // Read original rows independently of the directory used by the binder.
        for row in crate::dol::slice(&executable, 0x802a30a0, 18 * 12)?.chunks_exact(12) {
            let pointer = crate::read::u32(row, 0)?;
            if pointer == 0 {
                break;
            }
            let path = crate::dol::text(&executable, pointer)?;
            let Some(file) = path.strip_prefix("ev/") else {
                continue;
            };
            let source = format!("EV/{file}");
            let group = crate::read::u32(row, 4)?;
            let owner = if primary.join("files").join(&source).exists() {
                1
            } else {
                2
            };
            let extracted = if owner == 1 { &primary } else { &additional };
            let archive = Archive::open(&cooked, extracted, &source)?;
            let mut file = fs::File::open(extracted.join("files").join(&source))?;
            let entries = crate::afs::index(&mut file)?;
            assert_eq!(archive.index.members.len(), entries.len());
            owners.insert(group, owner);
            for member in [0, archive.index.members.len() - 1] {
                let id = group | member as u32;
                ids.insert(id);
                let entry = &entries[member];
                file.seek(SeekFrom::Start(entry.offset))?;
                let mut bytes = vec![0; entry.size];
                file.read_exact(&mut bytes)?;
                expected.insert(
                    id,
                    (
                        entry.name.clone(),
                        crate::digest(&bytes),
                        crate::read::u32(&bytes, 8)?,
                        crate::read::u32(&bytes, 12)?,
                    ),
                );
            }
        }
        assert_eq!(owners.values().filter(|&&disc| disc == 1).count(), 8);
        assert_eq!(owners.values().filter(|&&disc| disc == 2).count(), 5);
        let workspace = Workspace::open(&primary, &cooked)?;
        let (bound, sources) = voices(&workspace, &executable, Some(&additional), &ids)?;
        assert_eq!(bound.len(), 26);
        for (&id, voice) in &bound {
            let group = id & 0xffff_0000;
            assert_eq!(sources[&group]["disc"], owners[&group]);
            assert_eq!(voice.sample_rate, 32028);
            let (name, hash, rate, frames) = &expected[&id];
            assert_eq!(&voice.source_name, name);
            assert_eq!(&voice.source_sha256, hash);
            assert_eq!(&voice.source_sample_rate, rate);
            assert_eq!(&voice.frames, frames);
            assert_eq!(voice.asset.path, format!("audio/streams/{hash}.wav"));
        }
        let disc_two_ids = ids
            .into_iter()
            .filter(|id| owners[&(id & 0xffff_0000)] == 2)
            .collect();
        assert!(voices(&workspace, &executable, None, &disc_two_ids).is_err());
        drop(workspace);
        let workspace = Workspace::open(&additional, &cooked)?;
        let (direct, sources) = voices(&workspace, &executable, None, &disc_two_ids)?;
        assert!(sources.values().all(|source| source["disc"] == 2));
        for (id, voice) in direct {
            assert_eq!(
                serde_json::to_value(voice)?,
                serde_json::to_value(&bound[&id])?
            );
        }
        Ok(())
    }
}
