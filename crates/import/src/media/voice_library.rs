//! Native speech archives and terminal WAV publication. No cooked archive or
//! voice descriptor is read; source identity stays inside the importer.
use super::hash_file;
use anyhow::{Context, Result, ensure};
use resonance_content::field_audio::{Asset, Voice};
use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

pub(crate) struct PreparedVoice {
    pub voice: Voice,
    #[cfg(test)]
    pub source_name: String,
    #[cfg(test)]
    pub source_sha256: String,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Member {
    pub name: String,
    pub empty: bool,
}

pub(crate) struct Archive<'a> {
    output: &'a Path,
    pub source: Asset,
    pub members: Vec<Member>,
    file: fs::File,
    entries: Vec<crate::afs::Entry>,
}

impl<'a> Archive<'a> {
    /// Only an absent original source permits another disc. Corruption never does.
    #[cfg(test)]
    pub fn source(
        output: &'a Path,
        extracted: &Path,
        additional: Option<&Path>,
        source: &str,
    ) -> Result<Self> {
        let files = extracted.join("files");
        let owner = if crate::field_resources::find_path(&files, source)?.is_some() {
            extracted
        } else {
            let additional = additional.with_context(|| {
                format!("voice archive {source} is missing; supply the other extracted disc")
            })?;
            ensure!(
                crate::disc_number(additional)? != crate::disc_number(extracted)?,
                "additional voice source must be the other disc"
            );
            additional
        };
        Self::open(output, owner, source)
    }

    pub fn open(output: &'a Path, extracted: &Path, source: &str) -> Result<Self> {
        crate::disc_number(extracted)?;
        let files = extracted.join("files");
        let path = files.join(crate::field_resources::resolve_path(&files, source)?);
        let source = Asset {
            path: source.into(),
            sha256: hash_file(&path)?,
        };
        let mut file = fs::File::open(path)?;
        let (members, entries) = directory(&mut file)?;
        Ok(Self {
            output,
            source,
            members,
            file,
            entries,
        })
    }

    pub fn voice(&mut self, id: usize) -> Result<PreparedVoice> {
        let member = self
            .members
            .get(id)
            .with_context(|| format!("voice {id} is absent from {}", self.source.path))?;
        ensure!(!member.empty, "voice {id} is an authored empty stream");
        let entry = &self.entries[id];
        ensure!(
            entry.size <= 128 * 1024 * 1024,
            "voice member exceeds encoded read budget"
        );
        self.file.seek(SeekFrom::Start(entry.offset))?;
        let mut bytes = vec![0; entry.size];
        self.file.read_exact(&mut bytes)?;
        let path = format!(
            "audio/speech/{}/{}",
            crate::named::source_name(&self.source.path)?,
            line_name(&member.name, id)?
        );
        publish(self.output, &path, &member.name, &bytes)
    }
}

pub(crate) fn line_name(name: &str, id: usize) -> Result<String> {
    Ok(format!("{}-{id:05}.wav", crate::named::source_name(name)?))
}

pub(crate) fn directory(file: &mut fs::File) -> Result<(Vec<Member>, Vec<crate::afs::Entry>)> {
    let entries = crate::afs::index(file)?;
    let members = entries
        .iter()
        .map(|entry| {
            let mut header = [0; 16];
            file.seek(SeekFrom::Start(entry.offset))?;
            file.read_exact(&mut header[..entry.size.min(16)])?;
            Ok(Member {
                name: entry.name.clone(),
                empty: entry.size >= 16
                    && super::library::is_stream(&header)
                    && crate::read::u32(&header, 12)? == 0,
            })
        })
        .collect::<Result<_>>()?;
    Ok((members, entries))
}

pub(crate) fn publish(
    output: &Path,
    path: &str,
    _name: &str,
    bytes: &[u8],
) -> Result<PreparedVoice> {
    resonance_content::library::validate_path(path)?;
    let format = super::library::stream_format(bytes)
        .context("invalid signature or unsupported CRI voice encoding")?;
    let target = output.join(path);
    let parent = target.parent().context("voice target has no owner")?;
    fs::create_dir_all(parent)?;
    let temporary = tempfile::NamedTempFile::new_in(parent)?;
    let (channels, rate, frames) =
        super::field_audio::decode_voice_wave(temporary.path(), bytes, format)?;
    ensure!(
        frames == crate::read::u32(bytes, 12)?,
        "decoded voice length differs from source"
    );
    let sha256 = hash_file(temporary.path())?;
    crate::publication::install(temporary.path(), &target, &sha256)?;
    crate::publication::export(&target)?;
    Ok(PreparedVoice {
        voice: Voice {
            asset: Asset {
                path: path.into(),
                sha256,
            },
            frames,
            sample_rate: rate,
            source_sample_rate: rate,
            channels,
        },
        #[cfg(test)]
        source_name: _name.into(),
        #[cfg(test)]
        source_sha256: crate::digest(bytes),
    })
}

/// Nonempty silence still supplies the authored subtitle clock.
pub(crate) fn silent(root: &Path, voice: &Voice) -> Result<bool> {
    let mut wave = hound::WavReader::open(root.join(&voice.asset.path))?;
    for sample in wave.samples::<i16>() {
        if sample? != 0 {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(test)]
pub(crate) fn fixture(
    root: &Path,
    extracted: &Path,
    disc: u8,
    source: &str,
    name: &str,
    rate: u32,
    pcm: &[i16],
) -> Result<Voice> {
    // Real fixed-predictor ADX, so fixtures exercise the native decoder as well
    // as source selection. The second member is an authored zero-frame stream.
    let mut encoded = vec![0; 26 + pcm.len().div_ceil(32) * 18];
    encoded[..8].copy_from_slice(&[0x80, 0, 0, 22, 2, 18, 4, 1]);
    encoded[8..12].copy_from_slice(&rate.to_be_bytes());
    encoded[12..16].copy_from_slice(&(pcm.len() as u32).to_be_bytes());
    encoded[18..20].copy_from_slice(&0x0300u16.to_be_bytes());
    encoded[20..26].copy_from_slice(b"(c)CRI");
    for (index, &sample) in pcm.iter().enumerate() {
        ensure!(
            (-8..=7).contains(&sample),
            "fixture sample exceeds predictor-zero nibble"
        );
        let at = 26 + index / 32 * 18 + 2 + index % 32 / 2;
        encoded[at] |= (sample as u8 & 15) << (4 * (1 - index % 2));
    }
    let mut empty = encoded[..26].to_vec();
    empty[12..16].fill(0);
    let names = 32 + encoded.len() + empty.len();
    let mut afs = vec![0; names + 96];
    afs[..4].copy_from_slice(b"AFS\0");
    for (at, value) in [
        (4, 2),
        (8, 32),
        (12, encoded.len()),
        (16, 32 + encoded.len()),
        (20, empty.len()),
        (24, names),
        (28, 96),
    ] {
        afs[at..at + 4].copy_from_slice(&(value as u32).to_le_bytes());
    }
    afs[32..32 + encoded.len()].copy_from_slice(&encoded);
    afs[32 + encoded.len()..names].copy_from_slice(&empty);
    for at in [names, names + 48] {
        afs[at..at + name.len()].copy_from_slice(name.as_bytes());
    }
    crate::write_atomic(
        &extracted.join("sys/boot.bin"),
        &[b'G', b'Q', b'S', b'E', b'A', b'F', disc - 1, 0],
    )?;
    crate::write_atomic(&extracted.join("files").join(source), &afs)?;
    Ok(Archive::open(root, extracted, source)?.voice(0)?.voice)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_identity_and_missing_archive_control_disc_fallback() -> Result<()> {
        let work = tempfile::tempdir()?;
        let root = work.path();
        let source = "EV/test.afs";
        let primary = root.join("disc1");
        let additional = root.join("disc2");
        let first = root.join("first");
        let second = root.join("second");
        let fallback = root.join("fallback");
        let expected = fixture(&first, &primary, 1, source, "test.adx", 32000, &[0; 4])?;
        fixture(&second, &additional, 2, source, "test.adx", 32000, &[0; 8])?;
        let mut archive = Archive::source(&first, &primary, Some(&additional), source)?;
        let mut secondary = Archive::source(&second, &additional, None, source)?;
        assert_eq!(secondary.voice(0)?.voice.frames, 8);
        let reverse = "EV/first-disc-only.afs";
        fixture(
            &first,
            &primary,
            1,
            reverse,
            "first.adx",
            32000,
            &[1, -1, 0, 0],
        )?;
        let voice = Archive::source(&fallback, &additional, Some(&primary), reverse)?.voice(0)?;
        assert_eq!(voice.source_name, "first.adx");
        assert!(!silent(&fallback, &voice.voice)?);
        assert_ne!(archive.source.sha256, secondary.source.sha256);
        let original = primary.join("files").join(source);
        let alias = "EV/alias.afs";
        crate::write_atomic(&additional.join("files").join(alias), &fs::read(&original)?)?;
        let mut aliased = Archive::source(&first, &additional, None, alias)?;
        assert_eq!(aliased.source.path, alias);
        assert_eq!(aliased.members, archive.members);
        let bound = archive.voice(0)?.voice;
        assert_eq!(
            serde_json::to_value(&bound)?,
            serde_json::to_value(&expected)?
        );
        assert_eq!(bound.asset.path, "audio/speech/ev-test/test-00000.wav");
        assert!(silent(&first, &bound)?);
        assert_ne!(aliased.voice(0)?.voice.asset.path, bound.asset.path);
        assert!(archive.voice(1).is_err());
        assert!(archive.voice(2).is_err());
        // The next cook reads the original source again, never an old descriptor.
        fs::remove_file(first.join(&bound.asset.path))?;
        assert_eq!(archive.voice(0)?.voice.frames, 4);
        fs::write(&original, b"changed")?;
        assert!(Archive::source(&first, &primary, Some(&additional), source).is_err());
        fs::remove_file(&original)?;
        assert_eq!(
            Archive::source(&fallback, &primary, Some(&additional), source)?
                .voice(0)?
                .voice
                .frames,
            8
        );
        assert!(Archive::source(&fallback, &primary, None, source).is_err());
        fs::create_dir(&original)?;
        assert!(Archive::source(&fallback, &primary, Some(&additional), source).is_err());
        fs::remove_dir(&original)?;
        for boot in [b"GQSEAF\0\0", b"GQSEAF\x01\x01", b"GQSPAF\x01\0"] {
            fs::write(additional.join("sys/boot.bin"), boot)?;
            assert!(Archive::source(&fallback, &primary, Some(&additional), source).is_err());
        }
        Ok(())
    }
}

#[test]
#[ignore = "requires both original discs; decodes every native speech archive to current WAV owners"]
fn original_voice_archives_preserve_member_order_empty_slots_and_pcm_dimensions() -> Result<()> {
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
    let output = tempfile::tempdir()?;
    let mut count = 0;
    let mut owners = std::collections::BTreeMap::new();
    for disc in [1, 2] {
        let extracted = local.join(format!("disc{disc}"));
        let files = extracted.join("files");
        let mut pending = vec![files.clone()];
        while let Some(directory) = pending.pop() {
            for entry in fs::read_dir(directory)? {
                let entry = entry?;
                if entry.file_type()?.is_dir() {
                    pending.push(entry.path());
                    continue;
                }
                if !entry.file_type()?.is_file() {
                    continue;
                }
                let mut file = fs::File::open(entry.path())?;
                let mut magic = [0; 4];
                if file.read(&mut magic)? != 4 || &magic != b"AFS\0" {
                    continue;
                }
                let source = crate::relative_source_path(&files, &entry.path())?;
                let owner = output.path().join(format!("disc-{disc}"));
                let mut archive = Archive::open(&owner, &extracted, &source)?;
                file.seek(SeekFrom::Start(0))?;
                let entries = crate::afs::index(&mut file)?;
                assert_eq!(archive.members.len(), entries.len(), "{source}");
                for (id, entry) in entries.iter().enumerate() {
                    assert_eq!(archive.members[id].name, entry.name, "{source}/{id}");
                    file.seek(SeekFrom::Start(entry.offset))?;
                    let mut header = [0; 16];
                    file.read_exact(&mut header)?;
                    let frames = crate::read::u32(&header, 12)?;
                    assert_eq!(archive.members[id].empty, frames == 0);
                    if frames == 0 {
                        assert!(archive.voice(id).is_err());
                        continue;
                    }
                    let decoded = archive.voice(id)?;
                    let expected_path = format!(
                        "audio/speech/{}/{}",
                        crate::named::source_name(&source)?,
                        line_name(&entry.name, id)?
                    );
                    assert_eq!(decoded.voice.asset.path, expected_path);
                    let wave = hound::WavReader::open(owner.join(&expected_path))?;
                    assert_eq!(
                        (
                            wave.duration(),
                            wave.spec().sample_rate,
                            wave.spec().channels
                        ),
                        (frames, crate::read::u32(&header, 8)?, u16::from(header[7]))
                    );
                    let key = (source.to_ascii_lowercase(), id);
                    if let Some(previous) = owners.insert(
                        key,
                        (
                            decoded.source_sha256.clone(),
                            decoded.voice.asset.sha256.clone(),
                        ),
                    ) {
                        if previous.0 == decoded.source_sha256 {
                            assert_eq!(previous.1, decoded.voice.asset.sha256);
                        }
                    }
                }
                count += 1;
            }
        }
    }
    assert_eq!(count, 21);
    Ok(())
}
