//! Bind archive IDs to shared native-rate PCM; callers choose the playback clock.
use super::hash_file;
use anyhow::{Context, Result, ensure};
use resonance_content::field_audio::{
    Asset, Voice,
    archive::{MemberKind, VoiceArchive},
};
use std::{fs, io::ErrorKind, path::Path};

pub(crate) struct Archive<'a> {
    root: &'a Path,
    pub disc: u8,
    pub source: Asset,
    pub index: VoiceArchive,
}

impl<'a> Archive<'a> {
    /// Only a missing source permits disc 2 fallback; corrupt indexes never do.
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
                        "voice archive {} is missing; extract Disc 2 and supply --additional-disc",
                        path.display()
                    )
                })?;
                ensure!(
                    crate::disc_number(additional)? == 2,
                    "additional voice disc must be disc 2"
                );
                additional
            }
            Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
        };
        let archive = Self::open(root, crate::disc_number(owner)?, source)?;
        ensure!(
            hash_file(&owner.join("files").join(source))? == archive.index.source_sha256,
            "cooked voice archive {source} differs from its source; rerun cook-all"
        );
        Ok(archive)
    }

    pub fn open(root: &'a Path, disc: u8, source: &str) -> Result<Self> {
        ensure!((1..=2).contains(&disc), "invalid voice archive disc");
        resonance_content::validate_asset_path(source)?;
        let sources = super::sound_library::source_index(root)?;
        let label = format!("disc{disc}/{source}");
        let outputs = sources
            .get(&label)
            .with_context(|| format!("missing cooked voice source {label}; rerun cook-all"))?;
        let mut indexes = outputs
            .iter()
            .filter(|path| path.starts_with("audio/archives/") && path.ends_with("/archive.json"));
        let path = indexes
            .next()
            .with_context(|| format!("missing cooked voice index for {label}; rerun cook-all"))?;
        ensure!(
            indexes.next().is_none(),
            "ambiguous cooked voice index for {label}"
        );
        resonance_content::validate_asset_path(path)?;
        let index: VoiceArchive =
            serde_json::from_slice(&fs::read(root.join(path)).with_context(|| {
                format!(
                    "missing cooked voice index {path}; run cook-all --output {} first",
                    root.display()
                )
            })?)?;
        index.validate()?;
        ensure!(
            VoiceArchive::path(&index.source_sha256)? == *path,
            "wrong cooked voice archive {path}"
        );
        let source = Asset {
            path: source.into(),
            sha256: index.source_sha256.clone(),
        };
        Ok(Self {
            root,
            disc,
            source,
            index,
        })
    }

    pub fn voice(&self, id: usize) -> Result<Voice> {
        let member = self
            .index
            .members
            .get(id)
            .with_context(|| format!("voice {id} is absent from {}", self.source.path))?;
        ensure!(
            member.kind == MemberKind::Stream,
            "voice {id} is an authored empty stream"
        );
        let metadata = &member.metadata;
        let voice: Voice = serde_json::from_slice(
            &fs::read(self.root.join(metadata))
                .with_context(|| format!("missing cooked voice {metadata}; rerun cook-all"))?,
        )?;
        voice.validate()?;
        ensure!(
            voice.source_name == member.name && voice.sample_rate == voice.source_sample_rate,
            "voice {id} changed source identity or native clock"
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
mod tests {
    use super::*;
    use resonance_content::field_audio::{Asset, archive::Member};
    use std::collections::BTreeMap;

    #[test]
    fn source_identity_and_missing_archive_control_disc_fallback() -> Result<()> {
        let root = crate::temporary_path(&std::env::temp_dir().join("resonance-field-voices"));
        fs::create_dir(&root)?;
        let result = (|| -> Result<()> {
            let source = "EV/test.afs";
            let primary = root.join("disc1");
            let additional = root.join("disc2");
            let hash = crate::digest(b"source AHX");
            let path = format!("audio/streams/{hash}.wav");
            fs::create_dir_all(root.join("audio/streams"))?;
            super::super::write_pcm16(&root.join(&path), 1, 32000, [0; 4])?;
            let voice = Voice {
                asset: Asset {
                    sha256: hash_file(&root.join(&path))?,
                    path,
                },
                frames: 4,
                sample_rate: 32000,
                source_sample_rate: 32000,
                channels: 1,
                source_name: "test.ahx".into(),
                source_sha256: hash,
            };
            let mut sources = BTreeMap::new();
            for (disc, extracted, bytes) in [
                (1, &primary, b"primary".as_slice()),
                (2, &additional, b"secondary".as_slice()),
            ] {
                crate::write_atomic(
                    &extracted.join("sys/boot.bin"),
                    &[b'G', b'Q', b'S', b'E', b'A', b'F', disc - 1, 0],
                )?;
                crate::write_atomic(&extracted.join("files").join(source), bytes)?;
                let asset = Asset {
                    path: source.into(),
                    sha256: crate::digest(bytes),
                };
                let metadata = format!("audio/archives/{}/0.json", asset.sha256);
                let index = VoiceArchive {
                    version: VoiceArchive::VERSION,
                    source_sha256: asset.sha256,
                    members: vec![Member {
                        name: voice.source_name.clone(),
                        metadata: metadata.clone(),
                        kind: MemberKind::Stream,
                    }],
                };
                let index_path = VoiceArchive::path(&index.source_sha256)?;
                sources.insert(format!("disc{disc}/{source}"), vec![index_path.clone()]);
                crate::write_atomic(&root.join(index_path), &serde_json::to_vec(&index)?)?;
                crate::write_atomic(&root.join(metadata), &serde_json::to_vec(&voice)?)?;
            }
            let alias = "EV/alias.afs";
            let primary_index = sources[&format!("disc1/{source}")].clone();
            sources.insert(format!("disc2/{alias}"), primary_index.clone());
            crate::write_atomic(&additional.join("files").join(alias), b"primary")?;
            crate::write_atomic(&root.join("sources.json"), &serde_json::to_vec(&sources)?)?;
            let archive = Archive::source(&root, &primary, Some(&additional), source)?;
            assert_eq!(archive.disc, 1);
            assert_eq!(Archive::source(&root, &additional, None, source)?.disc, 2);
            let aliased = Archive::source(&root, &additional, None, alias)?;
            assert_eq!(aliased.source.path, alias);
            assert_eq!(aliased.index.source_sha256, archive.index.source_sha256);
            assert_eq!(
                serde_json::to_value(&aliased.index)?,
                serde_json::to_value(&archive.index)?
            );
            let secondary = Archive::source(&root, &additional, None, source)?;
            assert_ne!(secondary.index.source_sha256, archive.index.source_sha256);
            let secondary_index = sources[&format!("disc2/{source}")].clone();
            sources
                .get_mut(&format!("disc1/{source}"))
                .unwrap()
                .extend(secondary_index);
            crate::write_atomic(&root.join("sources.json"), &serde_json::to_vec(&sources)?)?;
            assert!(Archive::source(&root, &primary, Some(&additional), source).is_err());
            sources.insert(format!("disc1/{source}"), primary_index.clone());
            crate::write_atomic(&root.join("sources.json"), &serde_json::to_vec(&sources)?)?;
            let bound = archive.voice(0)?;
            assert!(silent(&root, &bound)?);
            assert_eq!((bound.frames, bound.sample_rate), (4, 32000));
            // A cooked-index failure must never switch to the other disc.
            let index = root.join(&primary_index[0]);
            let saved = fs::read(&index)?;
            fs::remove_file(&index)?;
            assert!(Archive::source(&root, &primary, Some(&additional), source).is_err());
            fs::write(&index, saved)?;
            let original = primary.join("files").join(source);
            fs::write(&original, b"changed")?;
            assert!(Archive::source(&root, &primary, Some(&additional), source).is_err());
            fs::remove_file(&original)?;
            assert_eq!(
                Archive::source(&root, &primary, Some(&additional), source)?.disc,
                2
            );
            assert!(Archive::source(&root, &primary, None, source).is_err());
            fs::create_dir(&original)?;
            assert!(Archive::source(&root, &primary, Some(&additional), source).is_err());
            fs::remove_dir(&original)?;
            for boot in [b"GQSEAF\0\0", b"GQSEAF\x01\x01", b"GQSPAF\x01\0"] {
                fs::write(additional.join("sys/boot.bin"), boot)?;
                assert!(Archive::source(&root, &primary, Some(&additional), source).is_err());
            }
            fs::remove_file(root.join(&bound.asset.path))?;
            assert!(archive.voice(0).is_err());
            Ok(())
        })();
        fs::remove_dir_all(root)?;
        result
    }

    #[test]
    #[ignore = "requires both extracted discs and cooked voice indexes; no codecs or playback"]
    fn original_voice_indexes_preserve_all_member_order_and_share_source_aliases() -> Result<()> {
        use std::{
            collections::BTreeSet,
            io::{Read, Seek, SeekFrom},
        };
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let root = local.join("all-assets");
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
            let disc_number = match disc {
                "disc1" => 1,
                "disc2" => 2,
                _ => anyhow::bail!("invalid source disc"),
            };
            let archive = Archive::open(&root, disc_number, source)?;
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
                    && crate::battle::audio::all::is_stream(&header)
                    && crate::read::u32(&header, 12)? == 0;
                assert_eq!(
                    member.kind == MemberKind::Empty,
                    empty,
                    "{label}/{}",
                    member.name
                );
            }
            indexes.insert(VoiceArchive::path(&archive.index.source_sha256)?);
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
