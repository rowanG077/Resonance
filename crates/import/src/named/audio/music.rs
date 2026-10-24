//! Render music as independently replaceable WAV files, keeping the native
//! synth and its sample graph behind the importer boundary.
use super::*;
use resonance_audio::{SOURCE_RATE, sequence::recording};
use resonance_content::music;
use sha2::{Digest, Sha256};
use std::{io::Write, sync::Arc};

struct Wave {
    temporary: tempfile::NamedTempFile,
    hash: String,
    variant: music::Variant,
}

struct RecordingInput {
    disc: u8,
    source: String,
    name: String,
    loaded: Arc<resonance_audio::package::Loaded>,
    reverbs: [[f32; 5]; 2],
    battle: Option<[[f32; 5]; 2]>,
    identity: String,
}

impl RecordingInput {
    fn read(source: &AudioSource, id: u16, title: bool) -> Result<Self> {
        let name = if title {
            "title".to_owned()
        } else {
            source
                .music
                .get(&u32::from(id))
                .with_context(|| format!("music {id} has no source identity"))?
                .clone()
        };
        let loaded = Arc::new(
            source
                .original
                .music(id, (!title).then_some(source.field_reverbs))?,
        );
        let reverbs = loaded.reverbs();
        let battle = (!title && source.original.music_inherits_environment(id)?)
            .then_some(source.battle_reverbs);
        let identity = recording_identity(&loaded, reverbs, battle)?;
        Ok(Self {
            disc: source.disc,
            source: source.original.music_source(id)?,
            name,
            loaded,
            reverbs,
            battle,
            identity,
        })
    }
}

/// Hash typed render inputs without serializing PCM into a package or retaining
/// it in a second cache. This identity is scoped to the active cook only.
fn recording_identity(
    loaded: &resonance_audio::package::Loaded,
    reverbs: [[f32; 5]; 2],
    battle: Option<[[f32; 5]; 2]>,
) -> Result<String> {
    struct Hash(Sha256);
    impl Write for Hash {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.update(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut hash = Hash(Sha256::new());
    serde_json::to_writer(
        &mut hash,
        &(
            &loaded.resources().programs,
            loaded.score(),
            loaded.tables(),
            loaded.reverbs(),
            reverbs,
            battle,
        ),
    )?;
    hash.0
        .update((loaded.resources().samples.len() as u64).to_le_bytes());
    let mut encoded = [0u8; 8192];
    for (&id, sample) in &loaded.resources().samples {
        hash.0.update(id.to_le_bytes());
        hash.0.update([sample.key]);
        hash.0.update(sample.rate.to_le_bytes());
        hash.0.update(sample.loop_start.to_le_bytes());
        hash.0.update(sample.loop_length.to_le_bytes());
        for pcm in [&sample.pcm, &sample.loop_pcm] {
            hash.0.update((pcm.len() as u64).to_le_bytes());
            for chunk in pcm.chunks(encoded.len() / 2) {
                for (value, bytes) in chunk.iter().zip(encoded.chunks_exact_mut(2)) {
                    bytes.copy_from_slice(&value.to_le_bytes());
                }
                hash.0.update(&encoded[..chunk.len() * 2]);
            }
        }
    }
    Ok(format!("{:x}", hash.0.finalize()))
}

impl Publisher<'_> {
    /// Publish requested native song IDs. Production cooking supplies the full
    /// catalogue; tests may select representative loops and inherited studios.
    pub fn music_recordings(
        &mut self,
        ids: impl IntoIterator<Item = u16>,
        cancelled: &impl Fn() -> bool,
    ) -> Result<()> {
        ensure!(!cancelled(), "music publication cancelled");
        let ids: BTreeSet<_> = ids
            .into_iter()
            .filter(|id| !self.rendered_music.contains(id))
            .collect();
        if ids.is_empty() {
            return Ok(());
        }
        for id in ids {
            ensure!(!cancelled(), "music publication cancelled");
            let source = self.audio_source()?;
            let inputs = std::iter::once(source)
                .chain(source.additional.as_deref())
                .filter(|source| source.music.contains_key(&u32::from(id)))
                .map(|source| RecordingInput::read(source, id, false))
                .collect::<Result<Vec<_>>>()?;
            ensure!(!inputs.is_empty(), "music {id} has no original source");
            self.publish_recordings(id, inputs, false, cancelled)
                .with_context(|| format!("render music {id}"))?;
            self.rendered_music.insert(id);
        }
        Ok(())
    }

    pub(super) fn title_recording(&mut self, cancelled: &impl Fn() -> bool) -> Result<()> {
        ensure!(!cancelled(), "music publication cancelled");
        let source = self.audio_source()?;
        let inputs = std::iter::once(source)
            .chain(source.additional.as_deref())
            .map(|source| RecordingInput::read(source, 1, true))
            .collect::<Result<Vec<_>>>()?;
        self.publish_recordings(1, inputs, true, cancelled)
    }

    fn publish_recordings(
        &mut self,
        id: u16,
        inputs: Vec<RecordingInput>,
        title: bool,
        cancelled: &impl Fn() -> bool,
    ) -> Result<()> {
        let first = inputs
            .first()
            .context("recording needs an original source")?;
        let owner = format!("audio/music/{}", first.name);
        for input in &inputs {
            ensure!(
                input.identity == first.identity,
                "conflicting original definitions for {} {id} on discs {} and {}; resolve the source binding in the importer",
                if title { "title music" } else { "music" },
                first.disc,
                input.disc
            );
        }
        self.record_music(
            &owner,
            first.loaded.clone(),
            first.reverbs,
            first.battle,
            cancelled,
        )?;
        if !title {
            self.map(music::MAPPINGS, u32::from(id), &first.name)?;
        }
        for input in inputs {
            ensure!(!cancelled(), "music publication cancelled");
            let report = format!(
                "reports/audio-music/disc-{}/{}.json",
                input.disc,
                if title {
                    "title".to_owned()
                } else {
                    format!("song-{id:03}")
                }
            );
            self.json(
                &report,
                &serde_json::json!({
                    "song": id, "source": input.source, "recording": format!("{owner}/music.json"),
                }),
            )?;
            let outputs = self
                .audio_outputs
                .entry(format!("disc{}/{}", input.disc, input.source))
                .or_default();
            outputs.extend([
                report,
                format!("{owner}/music.json"),
                format!("{owner}/music.wav"),
            ]);
            let battle = format!("{owner}/battle.wav");
            if self.output.join(&battle).try_exists()? {
                outputs.insert(battle);
            }
        }
        Ok(())
    }

    pub(crate) fn record_music(
        &mut self,
        owner: &str,
        loaded: Arc<resonance_audio::package::Loaded>,
        reverbs: [[f32; 5]; 2],
        battle: Option<[[f32; 5]; 2]>,
        cancelled: &impl Fn() -> bool,
    ) -> Result<()> {
        ensure!(!cancelled(), "music publication cancelled");
        let path = self.output.join(format!("{owner}/music.json"));
        self.check_spelling(&format!("{owner}/music.json"))?;
        fs::create_dir_all(path.parent().context("music has no owner")?)?;
        let identity = recording_identity(&loaded, reverbs, battle)?;
        crate::publication::produce_once(&path, &identity, || {
            self.render_music(owner, loaded, reverbs, battle, cancelled)
        })?;
        ensure!(!cancelled(), "music publication cancelled");
        Ok(())
    }

    fn render_music(
        &mut self,
        owner: &str,
        loaded: Arc<resonance_audio::package::Loaded>,
        reverbs: [[f32; 5]; 2],
        battle: Option<[[f32; 5]; 2]>,
        cancelled: &impl Fn() -> bool,
    ) -> Result<()> {
        let main = self.music_wave(
            &format!("{owner}/music.wav"),
            loaded.clone(),
            reverbs,
            cancelled,
        )?;
        let battle = battle
            .map(|parameters| {
                self.music_wave(
                    &format!("{owner}/battle.wav"),
                    loaded,
                    parameters,
                    cancelled,
                )
            })
            .transpose()?
            .filter(|wave| wave.hash != main.hash || wave.variant.repeat != main.variant.repeat);
        let main = self.install_music_wave(main)?;
        let battle = battle
            .map(|wave| self.install_music_wave(wave))
            .transpose()?;
        self.json(
            &format!("{owner}/music.json"),
            &music::Definition {
                version: music::VERSION,
                audio: main.audio,
                repeat: main.repeat,
                battle,
            },
        )
    }

    fn music_wave(
        &mut self,
        path: &str,
        loaded: Arc<resonance_audio::package::Loaded>,
        reverbs: [[f32; 5]; 2],
        cancelled: &impl Fn() -> bool,
    ) -> Result<Wave> {
        self.check_spelling(path)?;
        let destination = self.output.join(path);
        let directory = destination.parent().context("music path has no owner")?;
        fs::create_dir_all(directory)?;
        let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
        let mut writer = hound::WavWriter::new(
            std::io::BufWriter::new(temporary.as_file_mut()),
            hound::WavSpec {
                channels: 2,
                sample_rate: SOURCE_RATE,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )?;
        let mut scratch = tempfile::tempfile()?;
        let rendered = recording::render(loaded, reverbs, &mut writer, &mut scratch, cancelled)?;
        writer.finalize()?;
        ensure!(!cancelled(), "music publication cancelled");
        let repeat = rendered
            .repeat
            .map(|repeat| -> Result<_> {
                Ok(music::Loop {
                    start_frame: repeat.start.try_into()?,
                    end_frame: repeat.end.try_into()?,
                })
            })
            .transpose()?;
        // Validate the completed stream using the same bounded reader used
        // at playback, before installing its final ordinary filename.
        let prepared = resonance_audio::recording::Prepared::new(
            temporary.reopen()?,
            repeat.map(|repeat| repeat.start_frame..repeat.end_frame),
        )?;
        ensure!(
            u64::from(prepared.frames()) == rendered.frames,
            "rendered music frame count differs"
        );
        drop(prepared);
        let hash = crate::media::hash_file(temporary.path())?;
        Ok(Wave {
            temporary,
            hash,
            variant: music::Variant {
                audio: path.into(),
                repeat,
            },
        })
    }

    fn install_music_wave(&mut self, wave: Wave) -> Result<music::Variant> {
        let path = &wave.variant.audio;
        if !self.already_published(path, &wave.hash)? {
            crate::publication::install(
                wave.temporary.path(),
                &self.output.join(path),
                &wave.hash,
            )?;
            self.remember(path, wave.hash)?;
        }
        Ok(wave.variant)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::library::Library;

    fn verify_performance(
        id: u32,
        variant: &str,
        loaded: Arc<resonance_audio::package::Loaded>,
        reverbs: [[f32; 5]; 2],
        track: Arc<resonance_audio::recording::Prepared>,
    ) -> Result<()> {
        use resonance_audio::{
            BLOCK_FRAMES, CONTROLS_PER_BLOCK,
            reverb::Studio,
            sequence::{LiveControls, stream::Stream},
        };
        use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};

        // Obtain the selected native passage as a transient cook result. It is
        // deliberately absent from the public, independently editable WAV.
        let mut wave = hound::WavWriter::new(
            tempfile::tempfile()?,
            hound::WavSpec {
                channels: 2,
                sample_rate: SOURCE_RATE,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )?;
        let rendered = recording::render(
            loaded.clone(),
            reverbs,
            &mut wave,
            &mut tempfile::tempfile()?,
            || false,
        )?;
        wave.finalize()?;
        assert_eq!(rendered.frames, u64::from(track.frames()));
        assert_eq!(
            rendered.repeat,
            track.repeat().map(|r| u64::from(r.start)..u64::from(r.end))
        );
        let mut stream = Stream::new(loaded.clone(), !loaded.score().loop_events.is_empty())?;
        let mut block = [[[0; 2]; 3]; BLOCK_FRAMES];
        let mut cursor = BLOCK_FRAMES;
        let mut studio = Studio::new(reverbs)?;
        let mut source = tempfile::tempfile()?;
        let mut source_writer = BufWriter::new(source.try_clone()?);
        let mut source_reader = None;
        let mut player = track.start(SOURCE_RATE)?;
        let mut end_window = Vec::new();
        let mut start_window = Vec::new();
        let lookahead = if rendered.repeat.is_some() { 320 } else { 0 };
        for frame in 0..rendered.frames + lookahead {
            let buses = if let Some(repeat) = rendered
                .source_repeat
                .as_ref()
                .filter(|repeat| frame >= repeat.end)
            {
                if frame == repeat.end {
                    source_writer.flush()?;
                    source.seek(SeekFrom::Start(repeat.start * 24))?;
                    source_reader = Some(BufReader::new(source.try_clone()?));
                }
                let reader = source_reader.as_mut().unwrap();
                if (frame - repeat.end).is_multiple_of(repeat.end - repeat.start) {
                    reader.seek(SeekFrom::Start(repeat.start * 24))?;
                }
                let mut values = [0u8; 24];
                reader.read_exact(&mut values)?;
                std::array::from_fn(|bus| {
                    std::array::from_fn(|channel| {
                        let at = (bus * 2 + channel) * 4;
                        i32::from_le_bytes(values[at..at + 4].try_into().unwrap())
                    })
                })
            } else {
                if cursor == BLOCK_FRAMES {
                    stream
                        .render_block([LiveControls::default(); CONTROLS_PER_BLOCK], &mut block)?;
                    cursor = 0;
                }
                let buses = block[cursor];
                cursor += 1;
                if rendered.source_repeat.is_some() {
                    for sample in buses.iter().flatten() {
                        source_writer.write_all(&sample.to_le_bytes())?;
                    }
                }
                buses
            };
            let pcm = studio
                .process(buses)
                .map(|s| s.clamp(i16::MIN as i32, i16::MAX as i32) as f32);
            assert_eq!(
                player.next_frame()?,
                Some(pcm),
                "music {id} ({variant}) frame {frame}; source {:?}",
                rendered.source_repeat
            );
            if let Some(repeat) = &rendered.repeat {
                if (repeat.start - lookahead..repeat.start + lookahead).contains(&frame) {
                    start_window.push(pcm);
                }
                if (repeat.end - lookahead..repeat.end + lookahead).contains(&frame) {
                    end_window.push(pcm);
                }
            }
        }
        if rendered.repeat.is_some() {
            assert_eq!(start_window.len(), 640);
            assert_eq!(start_window, end_window, "unaltered seam must recur");
        } else {
            assert!(player.next_frame()?.is_none());
        }
        if id == 95 {
            assert!(rendered.source_repeat.is_none());
            let reference = resonance_audio::sequence::render_preview(
                loaded,
                reverbs,
                track.frames() + BLOCK_FRAMES as u32,
            )?;
            let repeat = track.repeat().unwrap();
            let pass = |frame| {
                reference
                    .loop_starts
                    .iter()
                    .position(|&f| f == frame)
                    .unwrap()
            };
            assert_eq!(pass(repeat.end) - pass(repeat.start), 3);
        }
        if id == 3 || id == 5 || id == 78 {
            assert!(rendered.source_repeat.is_some());
        }
        eprintln!(
            "Music {id} ({variant}): {} frames, WAV repeat {:?}, native dry passage {:?}; every PCM frame and the repeated seam match the independent studio",
            rendered.frames, rendered.repeat, rendered.source_repeat
        );
        Ok(())
    }

    #[test]
    #[ignore = "requires extracted discs; renders and measures the complete native music catalogue"]
    fn native_music_recording_catalogue_coverage_and_cost() -> Result<()> {
        // An empty converter directory proves music has no staged inputs.
        let source = tempfile::tempdir()?;
        let extraction = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = tempfile::tempdir()?;
        let names = AudioSource::read(&extraction.join("disc1"), Some(&extraction.join("disc2")))?;
        let mut publisher = Publisher::new(source.path(), output.path())?.with_audio_source(names);
        let ids: Vec<_> = publisher.audio_source()?.music_ids().into_iter().collect();
        let started = std::time::Instant::now();
        publisher.title_recording(&|| false)?;
        eprintln!(
            "Recorded original title in {:.2}s",
            started.elapsed().as_secs_f64()
        );
        let mut owners = vec!["audio/music/title".to_owned()];
        let mut failures = Vec::new();
        for &id in &ids {
            let song_started = std::time::Instant::now();
            if let Err(error) = publisher.music_recordings([id.try_into()?], &|| false) {
                let failure = format!("song {id}: {error:#}");
                eprintln!("FAILED {failure}");
                failures.push(failure);
                continue;
            }
            owners.push(format!(
                "audio/music/{}",
                publisher.mappings[music::MAPPINGS][&id]
            ));
            eprintln!(
                "Recorded native song {id} in {:.2}s",
                song_started.elapsed().as_secs_f64()
            );
        }
        publisher.finish_audio()?;
        publisher.descriptor("GQSEAF", 0)?;
        let library = Library::open(output.path(), &[])?;
        let mut snapshot = library.snapshot();
        assert_eq!(
            snapshot.mappings(music::MAPPINGS)?.iter().count(),
            ids.len() - failures.len()
        );
        let mut bytes = 0u64;
        let mut frames = 0u64;
        for owner in owners {
            let captured =
                music::capture(&mut snapshot, &format!("{owner}/music.json"), &|| false)?;
            let mut variants = vec![(captured.audio, captured.definition.repeat)];
            if let Some((file, variant)) = captured.battle.zip(captured.definition.battle) {
                variants.push((file, variant.repeat));
            }
            for (file, repeat) in variants {
                bytes += file.metadata()?.len();
                let recording = resonance_audio::recording::Prepared::new(
                    file,
                    repeat.map(|r| r.start_frame..r.end_frame),
                )?;
                frames += u64::from(recording.frames());
                eprintln!(
                    "{owner}: {} frames, repeat {:?}",
                    recording.frames(),
                    recording.repeat()
                );
            }
        }
        assert!(!output.path().join("audio/instruments").exists());
        eprintln!(
            "Complete music catalogue: {} songs plus title, {frames} frames, {bytes} WAV bytes, {:.2}s elapsed",
            ids.len() - failures.len(),
            started.elapsed().as_secs_f64()
        );
        ensure!(
            failures.is_empty(),
            "music catalogue failures:\n{}",
            failures.join("\n")
        );
        Ok(())
    }

    #[test]
    #[ignore = "requires both extracted discs; renders native music without a device"]
    fn native_recordings_preserve_performances_loops_and_context_variants() -> Result<()> {
        // An empty converter directory proves music has no staged inputs.
        let source = tempfile::tempdir()?;
        let extraction = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = tempfile::tempdir()?;
        let names = AudioSource::read(&extraction.join("disc1"), Some(&extraction.join("disc2")))?;
        let mut publisher = Publisher::new(source.path(), output.path())?.with_audio_source(names);
        for id in [0, 3, 5, 78, 95, 111] {
            publisher.music_recordings([id], &|| false)?;
            eprintln!("Published native music {id} as WAV");
        }
        publisher.finish_audio()?;
        let publications = publisher.published.len();
        publisher.music_recordings([78, 0, 78, 95, 111], &|| false)?;
        assert_eq!(publisher.published.len(), publications);
        publisher.descriptor("GQSEAF", 0)?;
        let library = Library::open(output.path(), &[])?;
        let mut snapshot = library.snapshot();
        let mut frames = 0u64;
        for id in [0, 3, 5, 78, 95, 111] {
            let path = snapshot.resolve(music::MAPPINGS, id, Some("music.json"))?;
            let captured = music::capture(&mut snapshot, &path, &|| false)?;
            assert!(captured.definition.battle.is_none());
            let source = publisher.audio_source()?;
            let loaded = Arc::new(
                source
                    .original
                    .music(id as u16, Some(source.field_reverbs))?,
            );
            let reverbs = if id == 0 || id == 111 {
                publisher.audio_source()?.field_reverbs
            } else {
                loaded.reverbs()
            };
            let inherited = id == 0 || id == 111;
            let alternate = inherited.then(|| captured.audio.try_clone()).transpose()?;
            let mut variants = vec![("main", captured.audio, captured.definition.repeat, reverbs)];
            if let Some(file) = alternate {
                variants.push((
                    "battle",
                    file,
                    captured.definition.repeat,
                    publisher.audio_source()?.battle_reverbs,
                ));
            }
            for (variant, file, repeat, reverbs) in variants {
                let track = resonance_audio::recording::Prepared::new(
                    file,
                    repeat.map(|r| r.start_frame..r.end_frame),
                )?;
                frames += u64::from(track.frames());
                verify_performance(id, variant, loaded.clone(), reverbs, track)?;
            }
            let owner = output.path().join(&path).parent().unwrap().to_owned();
            assert!(!owner.join("sound.json").exists());
        }
        assert!(!output.path().join("audio/instruments").exists());
        eprintln!(
            "Verified {frames} music frames against native source performances with continuously baked effects, both inherited environments and no public synthesis dependencies"
        );
        Ok(())
    }

    #[test]
    #[ignore = "requires both extracted discs; verifies shared recordings against both native inherited environments"]
    fn identical_inherited_music_uses_one_recording() -> Result<()> {
        // An empty converter directory proves music has no staged inputs.
        let source = tempfile::tempdir()?;
        let extraction = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = tempfile::tempdir()?;
        let names = AudioSource::read(&extraction.join("disc1"), Some(&extraction.join("disc2")))?;
        let mut publisher = Publisher::new(source.path(), output.path())?.with_audio_source(names);
        publisher.music_recordings([0, 111], &|| false)?;
        publisher.finish_audio()?;
        publisher.descriptor("GQSEAF", 0)?;
        let mut snapshot = Library::open(output.path(), &[])?.snapshot();
        for id in [0, 111] {
            let path = snapshot.resolve(music::MAPPINGS, id, Some("music.json"))?;
            let captured = music::capture(&mut snapshot, &path, &|| false)?;
            assert!(captured.definition.battle.is_none());
            assert!(
                !output
                    .path()
                    .join(&path)
                    .parent()
                    .unwrap()
                    .join("battle.wav")
                    .exists()
            );
            let source = publisher.audio_source()?;
            let loaded = Arc::new(
                source
                    .original
                    .music(id as u16, Some(source.field_reverbs))?,
            );
            let track = resonance_audio::recording::Prepared::new(captured.audio, None)?;
            for (variant, reverbs) in [
                ("field", publisher.audio_source()?.field_reverbs),
                ("battle", publisher.audio_source()?.battle_reverbs),
            ] {
                verify_performance(id, variant, loaded.clone(), reverbs, track.clone())?;
            }
        }
        Ok(())
    }
}
