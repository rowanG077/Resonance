use super::*;
use std::{
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
};

pub(super) struct Archive {
    pub name: String,
    pub source: Option<PathBuf>,
    pub members: Vec<Member>,
}

pub(super) struct Member {
    pub name: String,
    pub original: crate::afs::Entry,
    pub empty: bool,
    pub playback: Playback,
}

/// Merge the native lookup tables before assigning ordinary speech owners.
/// Native lookup order still chooses the first row within one table; the same
/// logical archive must have one definition across the supplied inputs.
pub(super) fn catalogue(roots: &[&Path]) -> Result<(BTreeMap<u32, Archive>, Option<u32>)> {
    let mut declarations = BTreeMap::<u32, String>::new();
    let mut streams = None;
    for root in roots {
        let executable = fs::read(root.join("sys/main.dol"))?;
        let directory = crate::voice_directory::Directory::read(&executable)?;
        let mut seen = BTreeSet::new();
        for entry in directory.active() {
            if entry.resource_id == 0x78 {
                ensure!(
                    streams
                        .replace(entry.logical_base)
                        .is_none_or(|base| base == entry.logical_base),
                    "conflicting battle stream archive bindings"
                );
            }
            if !seen.insert(entry.logical_base) {
                continue;
            }
            let source = entry.source_path()?;
            if let Some(previous) = declarations.get(&entry.logical_base) {
                ensure!(
                    previous.eq_ignore_ascii_case(&source),
                    "conflicting speech archive {} bindings: {previous} and {source}",
                    entry.logical_base
                );
            } else {
                declarations.insert(entry.logical_base, source);
            }
        }
    }
    let mut archives = BTreeMap::new();
    let mut owners = BTreeMap::<String, String>::new();
    for (id, source) in declarations {
        let name = source_name(&source)?;
        if let Some(previous) = owners.insert(name.clone(), source.clone()) {
            ensure!(
                previous.eq_ignore_ascii_case(&source),
                "ambiguous speech archive name {name}"
            );
        }
        let physical = crate::source_paths::shared_source(roots.iter().copied(), &source)?;
        let members = if let Some(path) = &physical {
            let mut file = fs::File::open(path)?;
            crate::afs::index(&mut file)?
                .into_iter()
                .enumerate()
                .map(|(id, member)| Member::read(&mut file, id, member))
                .collect::<Result<_>>()?
        } else {
            Vec::new()
        };
        archives.insert(
            id,
            Archive {
                name,
                source: physical,
                members,
            },
        );
    }
    Ok((archives, streams))
}

impl Member {
    pub(super) fn read(
        file: &mut fs::File,
        id: usize,
        original: crate::afs::Entry,
    ) -> Result<Self> {
        ensure!(original.size >= 16, "truncated original speech header");
        file.seek(SeekFrom::Start(original.offset))?;
        let mut header = [0; 16];
        file.read_exact(&mut header)?;
        let format = crate::media::library::stream_format(&header)
            .context("unsupported original speech encoding")?;
        let rate = crate::read::u32(&header, 8)?;
        Ok(Self {
            name: crate::media::voice_library::line_name(&original.name, id)?,
            empty: crate::read::u32(&header, 12)? == 0,
            playback: Playback {
                sample_rate: (matches!(format, crate::media::VoiceFormat::Ahx)
                    && rate == crate::media::SAMPLE_RATE)
                    .then_some(crate::media::PLAYBACK_RATE),
            },
            original,
        })
    }
}

impl Archive {
    fn member(&self, id: u16) -> Result<&Member> {
        ensure!(
            self.source.is_some(),
            "speech archive {} requires the other original disc",
            self.name
        );
        let member = self
            .members
            .get(usize::from(id))
            .with_context(|| format!("speech member {id} is absent from {}", self.name))?;
        ensure!(
            !member.empty,
            "speech member {id} in {} is authored empty",
            self.name
        );
        Ok(member)
    }

    pub(super) fn read(&self, id: u16) -> Result<Vec<u8>> {
        let member = self.member(id)?;
        ensure!(
            member.original.size <= 128 * 1024 * 1024,
            "speech member exceeds encoded read budget"
        );
        let mut file = fs::File::open(self.source.as_ref().unwrap())?;
        file.seek(SeekFrom::Start(member.original.offset))?;
        let mut bytes = vec![0; member.original.size];
        file.read_exact(&mut bytes)?;
        Ok(bytes)
    }
}

impl Publisher<'_> {
    /// Publish native archive members directly under their named owners. The
    /// ordinary WAV header supplies channel, rate and duration facts at runtime.
    pub fn speech_lines(&mut self, ids: impl IntoIterator<Item = u32>) -> Result<()> {
        for id in ids.into_iter().collect::<BTreeSet<_>>() {
            let base = id & 0xffff_0000;
            let member_id = id as u16;
            let archive = self
                .audio_source()?
                .speech
                .get(&base)
                .with_context(|| format!("voice {id} has no source archive"))?;
            let member = archive.member(member_id)?;
            let name = archive.name.clone();
            let line = member.name.clone();
            let path = format!("audio/speech/{name}/{line}");
            if !self.published.contains_key(&path) {
                let bytes = archive.read(member_id)?;
                let format = crate::media::library::stream_format(&bytes)
                    .context("invalid signature or unsupported CRI voice encoding")?;
                self.check_spelling(&path)?;
                let target = self.output.join(&path);
                let directory = target.parent().context("speech path has no owner")?;
                fs::create_dir_all(directory)?;
                let temporary = tempfile::NamedTempFile::new_in(directory)?;
                crate::media::field_audio::decode_voice_wave(temporary.path(), &bytes, format)
                    .with_context(|| format!("decode speech {name}/{line}"))?;
                let hash = crate::media::hash_file(temporary.path())?;
                crate::publication::install(temporary.path(), &target, &hash)?;
                self.remember(&path, hash)?;
            }
            self.map(document::SPEECH, base, &name)?;
            self.map(
                &format!("audio/speech/{name}/mappings.json"),
                u32::from(member_id),
                &line,
            )?;
        }
        Ok(())
    }
}

#[test]
#[ignore = "requires both original discs; checks speech without a staged cook"]
fn native_speech_publishes_named_pcm_directly_from_both_discs() -> Result<()> {
    use resonance_content::{field_audio::Voice, library::Library};
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
    let empty = tempfile::tempdir()?;
    let output = tempfile::tempdir()?;
    let mut publisher = Publisher::new(empty.path(), output.path())?.with_audio_source(
        AudioSource::read(&extracted.join("disc1"), Some(&extracted.join("disc2")))?,
    );
    let mut ids = BTreeSet::new();
    let mut empty_ids = Vec::new();
    let mut discs = BTreeSet::new();
    for (&base, archive) in &publisher.audio_source()?.speech {
        let source = archive
            .source
            .as_ref()
            .context("missing archive on both original discs")?;
        discs.insert(if source.starts_with(extracted.join("disc1")) {
            1
        } else {
            2
        });
        let playable: Vec<_> = archive
            .members
            .iter()
            .enumerate()
            .filter_map(|(id, member)| (!member.empty).then_some(base | id as u32))
            .collect();
        ids.extend(playable.first().copied());
        ids.extend(playable.last().copied());
        empty_ids.extend(
            archive
                .members
                .iter()
                .enumerate()
                .filter_map(|(id, member)| member.empty.then_some(base | id as u32)),
        );
    }
    assert_eq!(discs, [1, 2].into());
    assert!(!ids.is_empty());
    publisher.speech_lines(ids.iter().copied())?;
    let publications = publisher.published.len();
    publisher.speech_lines(ids.iter().copied())?;
    assert_eq!(publisher.published.len(), publications);
    for id in empty_ids {
        let error = publisher.speech_lines([id]).unwrap_err();
        assert!(error.to_string().contains("authored empty"));
    }
    assert!(publisher.speech_lines([u32::MAX]).is_err());
    publisher.finish_audio()?;
    publisher.descriptor("GQSEAF", 0)?;
    let mut snapshot = Library::open(output.path(), &[])?.snapshot();
    let mut formats = BTreeSet::new();
    let mut samples = 0;
    for id in &ids {
        let base = id & 0xffff_0000;
        let member = *id as u16;
        let archive = &publisher.audio_source()?.speech[&base];
        let path = snapshot.resolve(document::SPEECH, base, Some("mappings.json"))?;
        let path = snapshot.resolve(&path, u32::from(member), None)?;
        assert_eq!(
            path,
            format!(
                "audio/speech/{}/{}",
                archive.name,
                archive.members[usize::from(member)].name
            )
        );
        let bytes = snapshot.read(&path, 128 * 1024 * 1024)?;
        let voice = Voice::from_wave(&path, None, &bytes)?;
        let encoded = archive.read(member)?;
        assert_eq!(voice.frames, crate::read::u32(&encoded, 12)?);
        assert_eq!(voice.source_sample_rate, crate::read::u32(&encoded, 8)?);
        assert_eq!(voice.sample_rate, voice.source_sample_rate);
        assert_eq!(voice.channels, u16::from(encoded[7]));
        let expected = match crate::media::library::stream_format(&encoded).unwrap() {
            crate::media::VoiceFormat::Ahx => {
                formats.insert("ahx");
                let mut decoder = ahx_rs::Decoder::new(encoded.as_slice())?;
                let mut pcm = Vec::new();
                while let Some(block) = decoder.next_block()? {
                    pcm.extend_from_slice(block);
                }
                pcm
            }
            crate::media::VoiceFormat::Adx => {
                formats.insert("adx");
                let mut decoder = crate::media::adx::Decoder::new(&encoded)?;
                let mut pcm = Vec::new();
                while let Some(block) = decoder.next_block()? {
                    pcm.extend_from_slice(block);
                }
                pcm
            }
        };
        let actual = hound::WavReader::new(std::io::Cursor::new(&*bytes))?
            .samples::<i16>()
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(actual, expected, "native speech {id}");
        samples += actual.len();
    }
    assert_eq!(formats, ["adx", "ahx"].into());
    assert_eq!(fs::read_dir(empty.path())?.count(), 0);
    assert!(!output.path().join("audio/streams").exists());
    assert!(!output.path().join("audio/archives").exists());
    eprintln!(
        "Verified {} named speech lines, {samples} PCM samples, both original discs and both codecs without staged inputs",
        ids.len()
    );
    Ok(())
}

#[test]
#[ignore = "requires original disc 1 and RESONANCE_CRI_ADX_REFERENCE captured PCM; publishes one current stream"]
fn native_battle_stream_matches_the_captured_cri_prefill() -> Result<()> {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let expected = fs::read(
        std::env::var_os("RESONANCE_CRI_ADX_REFERENCE")
            .context("set RESONANCE_CRI_ADX_REFERENCE to captured BE PCM")?,
    )?;
    ensure!(
        crate::digest(&expected)
            == "a0a169f74cec7b22fc359a342488c119747aa037c972924ac1c5d6d1704f2a42",
        "unexpected source prefill reference"
    );
    let source = AudioSource::read(&extracted, None)?;
    let base = source
        .streams
        .context("missing native battle stream catalogue")?;
    let output = tempfile::tempdir()?;
    let mut publisher = Publisher::new(&extracted, output.path())?.with_audio_source(source);
    publisher.speech_lines([base | 257])?;
    publisher.finish_audio()?;
    publisher.descriptor("GQSEAF", 0)?;
    let mut snapshot = resonance_content::library::Library::open(output.path(), &[])?.snapshot();
    let mapping = snapshot.resolve(document::SPEECH, base, Some("mappings.json"))?;
    let path = snapshot.resolve(&mapping, 257, None)?;
    let mut wave = hound::WavReader::open(output.path().join(path))?;
    let samples = wave
        .samples::<i16>()
        .take(4096)
        .collect::<Result<Vec<_>, _>>()?;
    let observed = samples
        .iter()
        .flat_map(|sample| sample.to_be_bytes())
        .collect::<Vec<_>>();
    assert_eq!(observed, expected);
    assert!(publisher.speech_lines([base | 32766]).is_err());
    Ok(())
}
