//! Independent native-table coverage for the current speech mappings.
use super::*;
use std::io::{Read, Seek, SeekFrom};

#[test]
#[ignore = "requires both original discs; publishes current speech without an old cooked library"]
fn original_field_voice_ids_bind_both_discs_and_disc_two_fallback() -> Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
    let primary = root.join("disc1");
    let additional = root.join("disc2");
    let executable = fs::read(primary.join("sys/main.dol"))?;
    let mut ids = BTreeSet::new();
    let mut owners = BTreeMap::new();
    let mut expected = BTreeMap::new();
    // Read the original rows independently of AudioSource's lookup directory.
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
        let mut file = fs::File::open(extracted.join("files").join(&source))?;
        let entries = crate::afs::index(&mut file)?;
        owners.insert(group, owner);
        for member in [0, entries.len() - 1] {
            let id = group | member as u32;
            ids.insert(id);
            let entry = &entries[member];
            file.seek(SeekFrom::Start(entry.offset))?;
            let mut bytes = vec![0; entry.size];
            file.read_exact(&mut bytes)?;
            expected.insert(
                id,
                (
                    crate::read::u32(&bytes, 8)?,
                    crate::read::u32(&bytes, 12)?,
                    u16::from(bytes[7]),
                ),
            );
        }
    }
    assert_eq!(owners.values().filter(|&&disc| disc == 1).count(), 8);
    assert_eq!(owners.values().filter(|&&disc| disc == 2).count(), 5);
    assert_eq!(ids.len(), 26);
    let output = tempfile::tempdir()?;
    let mut publisher = Publisher::new(output.path(), output.path())?
        .with_audio_source(AudioSource::read(&primary, Some(&additional))?);
    publisher.speech_lines(ids.iter().copied())?;
    publisher.finish_audio()?;
    publisher.descriptor("GQSEAF", 0)?;
    let mut snapshot = library::Library::open(output.path(), &[])?.snapshot();
    let mut paths = BTreeMap::new();
    for &id in &ids {
        let base = id & 0xffff_0000;
        let mappings = snapshot.resolve(document::SPEECH, base, Some("mappings.json"))?;
        let path = snapshot.resolve(&mappings, id & 0xffff, None)?;
        let bytes = snapshot.read(&path, 128 * 1024 * 1024)?;
        let wave = hound::WavReader::new(std::io::Cursor::new(bytes))?;
        let (rate, frames, channels) = expected[&id];
        assert_eq!(
            (
                wave.spec().sample_rate,
                wave.duration(),
                wave.spec().channels
            ),
            (rate, frames, channels)
        );
        let archive = &publisher.audio_source()?.speech[&base];
        assert_eq!(
            archive.members[usize::from(id as u16)].playback.sample_rate,
            Some(32028)
        );
        paths.insert(id, path);
    }
    let disc_two_ids: Vec<_> = ids
        .into_iter()
        .filter(|id| owners[&(id & 0xffff_0000)] == 2)
        .collect();
    let missing_output = tempfile::tempdir()?;
    let mut missing = Publisher::new(missing_output.path(), missing_output.path())?
        .with_audio_source(AudioSource::read(&primary, None)?);
    assert!(missing.speech_lines(disc_two_ids.iter().copied()).is_err());
    let direct_output = tempfile::tempdir()?;
    let mut direct = Publisher::new(direct_output.path(), direct_output.path())?
        .with_audio_source(AudioSource::read(&additional, None)?);
    direct.speech_lines(disc_two_ids.iter().copied())?;
    for id in disc_two_ids {
        assert_eq!(
            fs::read(output.path().join(&paths[&id]))?,
            fs::read(direct_output.path().join(&paths[&id]))?
        );
    }
    Ok(())
}
