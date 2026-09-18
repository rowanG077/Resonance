//! Bind archive IDs to shared native-rate PCM; callers choose the playback clock.
use super::hash_file;
use anyhow::{Context, Result, ensure};
use resonance_content::field_audio::{
    Asset, Voice,
    archive::{Member, MemberKind, VoiceArchive},
};
use std::{
    fs,
    io::{ErrorKind, Read, Seek, SeekFrom},
    path::Path,
};

pub(crate) struct Archive<'a> {
    root: &'a Path,
    pub disc: u8,
    pub source: Asset,
    pub index: VoiceArchive,
    file: fs::File,
    entries: Vec<crate::afs::Entry>,
}

impl<'a> Archive<'a> {
    /// Only a missing source permits another disc; corrupt indexes never do.
    pub fn source(
        root: &'a Path,
        extracted: &Path,
        additional: Option<&Path>,
        source: &str,
    ) -> Result<Self> {
        resonance_content::validate_asset_path(source)?;
        let path = extracted.join("files").join(source);
        let owner = match fs::metadata(&path) {
            Ok(_) => extracted,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                let additional = additional.with_context(|| {
                    format!(
                        "voice archive {} is missing; supply the other extracted disc",
                        path.display()
                    )
                })?;
                ensure!(
                    crate::disc_number(additional)? != crate::disc_number(extracted)?,
                    "additional voice source must be the other disc"
                );
                additional
            }
            Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
        };
        Self::open(root, owner, source)
    }

    pub fn open(root: &'a Path, extracted: &Path, source: &str) -> Result<Self> {
        let disc = crate::disc_number(extracted)?;
        let files = extracted.join("files");
        let path = crate::field_resources::resolve_path(&files, source)?;
        let path = files.join(path);
        let source = Asset {
            path: source.into(),
            sha256: hash_file(&path)?,
        };
        let mut file = fs::File::open(path)?;
        let (index, entries) = directory(&mut file, &source.sha256)?;
        Ok(Self {
            root,
            disc,
            source,
            index,
            file,
            entries,
        })
    }

    pub fn voice(&mut self, id: usize) -> Result<Voice> {
        let member = self
            .index
            .members
            .get(id)
            .with_context(|| format!("voice {id} is absent from {}", self.source.path))?;
        ensure!(
            member.kind == MemberKind::Stream,
            "voice {id} is an authored empty stream"
        );
        let entry = &self.entries[id];
        ensure!(
            entry.size <= 128 * 1024 * 1024,
            "voice member exceeds encoded read budget"
        );
        self.file.seek(SeekFrom::Start(entry.offset))?;
        ensure!(entry.size >= 16, "truncated original voice header");
        let mut bytes = [0; 16];
        self.file.read_exact(&mut bytes)?;
        ensure!(
            crate::media::library::is_stream(&bytes),
            "invalid original voice header"
        );
        let sample_rate = crate::read::u32(&bytes, 8)?;
        let frames = crate::read::u32(&bytes, 12)?;
        let channels = u16::from(bytes[7]);
        self.file.seek(SeekFrom::Start(entry.offset))?;
        let source_hash = super::hash_reader((&mut self.file).take(entry.size as u64))?;
        let metadata = &member.metadata;
        let voice: Voice = serde_json::from_slice(
            &fs::read(self.root.join(metadata))
                .with_context(|| format!("missing cooked voice {metadata}; rerun cook-all"))?,
        )?;
        voice.validate()?;
        ensure!(
            voice.source_name == member.name
                && voice.source_sha256 == source_hash
                && voice.sample_rate == sample_rate
                && voice.source_sample_rate == sample_rate
                && voice.frames == frames
                && voice.channels == channels,
            "voice {id} final descriptor differs from its original stream"
        );
        ensure!(
            voice.asset.path == format!("audio/streams/{}.wav", voice.source_sha256),
            "voice {id} is not from the shared PCM library"
        );
        let path = self.root.join(&voice.asset.path);
        ensure!(
            hash_file(&path)? == voice.asset.sha256,
            "cooked voice {id} digest mismatch"
        );
        let wave = hound::WavReader::open(path)?;
        let spec = wave.spec();
        ensure!(
            wave.duration() == voice.frames
                && spec.sample_rate == voice.source_sample_rate
                && spec.channels == voice.channels
                && spec.bits_per_sample == 16
                && spec.sample_format == hound::SampleFormat::Int,
            "cooked voice {id} PCM format differs from metadata"
        );
        Ok(voice)
    }
}

pub(crate) fn directory(
    file: &mut fs::File,
    source_hash: &str,
) -> Result<(VoiceArchive, Vec<crate::afs::Entry>)> {
    let entries = crate::afs::index(file)?;
    let members = entries
        .iter()
        .enumerate()
        .map(|(id, entry)| {
            let mut header = [0; 16];
            file.seek(SeekFrom::Start(entry.offset))?;
            file.read_exact(&mut header[..entry.size.min(16)])?;
            let empty = entry.size >= 16
                && crate::media::library::is_stream(&header)
                && crate::read::u32(&header, 12)? == 0;
            Ok(Member {
                name: entry.name.clone(),
                metadata: format!("audio/archives/{source_hash}/{id}.json"),
                kind: if empty {
                    MemberKind::Empty
                } else {
                    MemberKind::Stream
                },
            })
        })
        .collect::<Result<_>>()?;
    let index = VoiceArchive {
        version: VoiceArchive::VERSION,
        source_sha256: source_hash.into(),
        members,
    };
    index.validate()?;
    Ok((index, entries))
}

/// A nonempty silent stream still supplies the authored subtitle clock.
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
    let mut encoded = vec![0; 16];
    encoded[..8].copy_from_slice(&[
        0x80,
        0,
        0,
        12,
        if name.ends_with(".adx") { 3 } else { 0x10 },
        18,
        4,
        1,
    ]);
    encoded[8..12].copy_from_slice(&rate.to_be_bytes());
    encoded[12..16].copy_from_slice(&(pcm.len() as u32).to_be_bytes());
    encoded.extend(pcm.iter().flat_map(|sample| sample.to_le_bytes()));
    let source_sha256 = crate::digest(&encoded);
    let mut empty = encoded[..16].to_vec();
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
    let path = format!("audio/streams/{source_sha256}.wav");
    fs::create_dir_all(root.join("audio/streams"))?;
    resonance_asset_writer::wav::write_pcm16(&root.join(&path), 1, rate, pcm.iter().copied())?;
    let voice = Voice {
        asset: Asset {
            sha256: hash_file(&root.join(&path))?,
            path,
        },
        frames: pcm.len() as u32,
        sample_rate: rate,
        source_sample_rate: rate,
        channels: 1,
        source_name: name.into(),
        source_sha256,
    };
    crate::write_atomic(
        &root.join(format!("audio/archives/{}/0.json", crate::digest(&afs))),
        &serde_json::to_vec(&voice)?,
    )?;
    Ok(voice)
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
        let expected = fixture(root, &primary, 1, source, "test.ahx", 32000, &[0; 4])?;
        fixture(root, &additional, 2, source, "test.ahx", 32000, &[0; 8])?;
        let mut archive = Archive::source(root, &primary, Some(&additional), source)?;
        let secondary = Archive::source(root, &additional, None, source)?;
        assert_eq!((archive.disc, secondary.disc), (1, 2));
        let reverse = "EV/first-disc-only.afs";
        fixture(root, &primary, 1, reverse, "first.ahx", 32000, &[0; 4])?;
        assert_eq!(
            Archive::source(root, &additional, Some(&primary), reverse)?.disc,
            1
        );
        assert_ne!(archive.source.sha256, secondary.source.sha256);
        let original = primary.join("files").join(source);
        let alias = "EV/alias.afs";
        crate::write_atomic(&additional.join("files").join(alias), &fs::read(&original)?)?;
        let mut aliased = Archive::source(root, &additional, None, alias)?;
        assert_eq!(aliased.source.path, alias);
        assert_eq!(
            serde_json::to_value(&aliased.index)?,
            serde_json::to_value(&archive.index)?
        );
        assert!(!root.join("sources.json").exists());
        let bound = archive.voice(0)?;
        assert_eq!(
            serde_json::to_value(&bound)?,
            serde_json::to_value(&expected)?
        );
        assert!(silent(root, &bound)?);
        assert!(archive.voice(1).is_err()); // Original zero-frame member.
        assert!(archive.voice(2).is_err());
        // A final descriptor failure cannot switch to the other disc.
        let metadata = root.join(&archive.index.members[0].metadata);
        let saved = fs::read(&metadata)?;
        fs::remove_file(&metadata)?;
        assert!(
            Archive::source(root, &primary, Some(&additional), source)?
                .voice(0)
                .is_err()
        );
        let mut changed: Voice = serde_json::from_slice(&saved)?;
        changed.frames += 1;
        fs::write(&metadata, serde_json::to_vec(&changed)?)?;
        assert!(archive.voice(0).is_err());
        fs::write(&metadata, &saved)?;
        fs::write(&original, b"changed")?;
        assert!(Archive::source(root, &primary, Some(&additional), source).is_err());
        fs::remove_file(&original)?;
        assert_eq!(
            Archive::source(root, &primary, Some(&additional), source)?.disc,
            2
        );
        assert!(Archive::source(root, &primary, None, source).is_err());
        fs::create_dir(&original)?;
        assert!(Archive::source(root, &primary, Some(&additional), source).is_err());
        fs::remove_dir(&original)?;
        for boot in [b"GQSEAF\0\0", b"GQSEAF\x01\x01", b"GQSPAF\x01\0"] {
            fs::write(additional.join("sys/boot.bin"), boot)?;
            assert!(Archive::source(root, &primary, Some(&additional), source).is_err());
        }
        fs::write(root.join(&bound.asset.path), b"changed PCM")?;
        assert!(aliased.voice(0).is_err());
        Ok(())
    }

    #[test]
    #[ignore = "requires both extracted discs and cooked voice indexes; no codecs or playback"]
    fn original_voice_indexes_preserve_all_member_order_and_share_source_aliases() -> Result<()> {
        use std::{
            collections::BTreeSet,
            io::{Read, Seek, SeekFrom},
        };
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let root = std::env::var_os("RESONANCE_COOKED")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| local.join("all-assets"));
        let sources = super::super::sound_library::source_index(&root)?;
        let mut indexes = BTreeSet::new();
        let mut count = 0;
        for (label, outputs) in sources {
            if !outputs
                .iter()
                .any(|path| path.starts_with("audio/archives/") && path.ends_with("/archive.json"))
            {
                continue;
            }
            let (disc, source) = label.split_once('/').context("invalid source label")?;
            ensure!(matches!(disc, "disc1" | "disc2"), "invalid source disc");
            let extracted = local.join("extracted").join(disc);
            let archive = Archive::open(&root, &extracted, source)?;
            let fresh = indexes.insert(VoiceArchive::path(&archive.index.source_sha256)?);
            let mut file = fs::File::open(
                local
                    .join("extracted")
                    .join(disc)
                    .join("files")
                    .join(source),
            )?;
            let entries = crate::afs::index(&mut file)?;
            assert_eq!(archive.index.members.len(), entries.len(), "{label}");
            for (member, entry) in archive.index.members.iter().zip(entries) {
                assert_eq!(member.name, entry.name, "{label}");
                let mut header = [0; 16];
                file.seek(SeekFrom::Start(entry.offset))?;
                file.read_exact(&mut header[..entry.size.min(16)])?;
                let empty = entry.size >= 16
                    && crate::media::library::is_stream(&header)
                    && crate::read::u32(&header, 12)? == 0;
                assert_eq!(
                    member.kind == MemberKind::Empty,
                    empty,
                    "{label}/{}",
                    member.name
                );
                if fresh {
                    let metadata = fs::read(root.join(&member.metadata))?;
                    if empty {
                        let value: serde_json::Value = serde_json::from_slice(&metadata)?;
                        assert_eq!(value["frames"], 0);
                        assert_eq!(value["silence"], true);
                    } else {
                        let voice: Voice = serde_json::from_slice(&metadata)?;
                        voice.validate()?;
                        assert_eq!(voice.source_name, member.name);
                        assert_eq!(voice.frames, crate::read::u32(&header, 12)?);
                        assert_eq!(voice.source_sample_rate, crate::read::u32(&header, 8)?);
                        assert_eq!(voice.channels, u16::from(header[7]));
                        let wave = hound::WavReader::open(root.join(&voice.asset.path))?;
                        assert_eq!(wave.duration(), voice.frames);
                        assert_eq!(wave.spec().sample_rate, voice.source_sample_rate);
                        assert_eq!(wave.spec().channels, voice.channels);
                    }
                }
            }
            count += 1;
        }
        assert_eq!(count, 21);
        assert!(
            indexes.len() < count,
            "identical source archives should share an index"
        );
        Ok(())
    }
}
