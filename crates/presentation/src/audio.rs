use anyhow::{Context, Result, ensure};
use bevy::prelude::*;
use resonance_audio::{
    BLOCK_FRAMES, CONTROLS_PER_BLOCK,
    package::{Loaded, Package},
    sequence,
};
use resonance_content::{TitleAudio, diagnostics::Diagnostics};
use resonance_playback::Decodable;
use std::{fs, path::Path, sync::Arc};

mod playback;
pub(super) use playback::{GameAudio, PlaybackAssets};

#[derive(Resource, Default)]
pub(super) struct MenuSounds {
    pub control: Option<playback::SoundControl>,
}

pub(super) fn check(sounds: Res<MenuSounds>, mut exit: MessageWriter<AppExit>) {
    if let Some(control) = &sounds.control
        && control.check().is_err()
    {
        exit.write(AppExit::error());
    }
}

/// Capture mode must not own an audio device; silent mode cannot be overridden.
pub(super) fn validate_startup(app: &App, silent: bool, capture: bool) -> Result<()> {
    if capture {
        ensure!(
            app.world()
                .get_non_send::<super::audio_output::Device>()
                .is_none(),
            "audio device was enabled for a deterministic capture"
        );
        ensure!(
            !app.is_plugin_added::<bevy::winit::WinitPlugin>(),
            "window event loop was enabled for a headless capture"
        );
    } else if silent {
        ensure!(
            app.world()
                .get_non_send::<super::audio_output::Device>()
                .is_some_and(|device| device.silent),
            "silent mode was overridden during startup; refusing to run"
        );
    }
    Ok(())
}

/// Immutable cooked instruments and score, shared with each synthesis worker.
/// Bevy owns output; this source reads prepared audio and does not loop PCM.
#[derive(Clone)]
pub(super) struct TitleMusic {
    music: Arc<Loaded>,
}

impl TitleMusic {
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join("title-audio.json");
        let info: TitleAudio = serde_json::from_slice(
            &fs::read(&path).with_context(|| format!("reading {}", path.display()))?,
        )?;
        info.validate()?;
        Ok(Self {
            music: Arc::new(Package::load_verified(
                root,
                &info.path,
                &info.sha256,
                &mut Default::default(),
            )?),
        })
    }
}

pub(super) struct MusicFrames {
    stream: sequence::stream::Stream,
    studio: resonance_audio::reverb::Studio,
    pcm: [i16; 320],
    cursor: usize,
}

impl MusicFrames {
    fn start(music: &TitleMusic) -> Result<Self> {
        Ok(Self {
            stream: sequence::stream::Stream::new(music.music.clone(), true)?,
            studio: resonance_audio::reverb::Studio::new(music.music.reverbs())?,
            pcm: [0; 320],
            cursor: 320,
        })
    }
    fn next_frame(&mut self) -> Result<[f32; 2]> {
        if self.cursor == self.pcm.len() {
            self.render()?;
        }
        let frame =
            std::array::from_fn(|channel| f32::from(self.pcm[self.cursor + channel]) / 32768.);
        self.cursor += 2;
        Ok(frame)
    }
    fn render(&mut self) -> Result<()> {
        let controls = [sequence::LiveControls::default(); CONTROLS_PER_BLOCK];
        let mut buses = [[[0; 2]; 3]; BLOCK_FRAMES];
        ensure!(
            self.stream.render_block(controls, &mut buses)? == BLOCK_FRAMES,
            "title score ended"
        );
        for (output, buses) in self.pcm.chunks_exact_mut(2).zip(buses) {
            output.copy_from_slice(
                &self
                    .studio
                    .process(buses)
                    .map(|s| s.clamp(i16::MIN as i32, i16::MAX as i32) as i16),
            );
        }
        self.cursor = 0;
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct CueEvent {
    pub frame: u32,
    pub cue: String,
}

impl std::str::FromStr for CueEvent {
    type Err = String;
    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        let (frame, cue) = value.split_once(':').ok_or("expected FRAME:CUE")?;
        if cue.is_empty() || cue.len() > 128 {
            return Err("invalid cue name".into());
        }
        Ok(Self {
            frame: frame.parse().map_err(|_| "invalid cue frame")?,
            cue: cue.into(),
        })
    }
}

/// Consume the player's actual native source manually. No App or audio plugin is
/// constructed, so recording never opens a window or speaker device.
pub fn record_title_music(
    root: &Path,
    output: &Path,
    frames: u32,
    events: &[CueEvent],
) -> Result<()> {
    ensure!(
        (1..=resonance_audio::SOURCE_RATE * 120).contains(&frames),
        "music recording exceeds 120 seconds"
    );
    ensure!(
        events.len() <= 1024
            && events.windows(2).all(|w| w[0].frame <= w[1].frame)
            && events.iter().all(|event| event.frame < frames),
        "invalid cue recording schedule"
    );
    let assets = PlaybackAssets::load(root, Diagnostics::new(true))?;
    let (audio, control) = assets.session();
    let mut decoder = audio.decoder();
    control.check()?;
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = output.with_extension("partial.wav");
    let mut writer = hound::WavWriter::create(
        &temporary,
        hound::WavSpec {
            channels: 2,
            sample_rate: resonance_audio::SOURCE_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    let mut next_event = 0;
    for frame in 0..frames {
        while let Some(event) = events.get(next_event)
            && event.frame == frame
        {
            control.play(&event.cue)?;
            next_event += 1;
        }
        for _ in 0..2 {
            let sample = decoder.next();
            control.check()?;
            let sample = sample.context("audio source ended before requested window")?;
            writer.write_sample((sample * 32768.0) as i16)?;
        }
    }
    ensure!(
        control.rendered_frames() == u64::from(frames),
        "audio source frame accounting differs"
    );
    decoder.stop();
    writer.finalize()?;
    fs::rename(temporary, output)?;
    fs::write(
        output.with_extension("json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "version":2,"source":"resonance_native_audio","audio_device":false,"frames":frames,
            "sample_rate":resonance_audio::SOURCE_RATE,"channels":2,
            "cue_events":events.iter().map(|event|serde_json::json!({"frame":event.frame,"cue":event.cue})).collect::<Vec<_>>(),
        }))?,
    )?;
    println!("Recorded {frames} frames from the native music/cue source without an audio device");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_audio::volume;

    fn assets() -> std::path::PathBuf {
        std::env::var_os("RESONANCE_TEST_ASSETS").map_or_else(
            || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets"),
            Into::into,
        )
    }

    #[test]
    #[ignore = "requires cooked menu programs; never opens a device"]
    fn menu_programs_preserve_authored_gain_stereo_and_output_completion() -> Result<()> {
        use resonance_audio::sequence::{LiveControls, stream::Stream};
        let root = assets();
        let metadata: resonance_content::TitleAudio =
            serde_json::from_slice(&fs::read(root.join("title-sounds.json"))?)?;
        let manifest: resonance_audio::cue::package::Manifest =
            serde_json::from_slice(&fs::read(root.join(metadata.path))?)?;
        for name in ["back", "confirm", "error", "navigate"] {
            assert!(
                manifest.cues.contains_key(name),
                "missing menu program {name}"
            );
        }
        for (name, cue) in manifest.cues {
            let loaded = Arc::new(Package::load(&root, &cue.program.path)?);
            let render = |volume, pan| -> Result<Vec<sequence::BusFrame>> {
                let mut stream = Stream::new(loaded.clone(), false)?;
                let control = LiveControls {
                    volume,
                    pan,
                    ..Default::default()
                };
                let mut output = Vec::new();
                loop {
                    let mut block = [[[0; 2]; 3]; BLOCK_FRAMES];
                    let length = stream.render_block([control; CONTROLS_PER_BLOCK], &mut block)?;
                    output.extend_from_slice(&block[..length]);
                    assert_eq!(
                        stream.submitted_until(),
                        output.len() as u64,
                        "{name} output fence"
                    );
                    if length == 0 {
                        block.fill([[1; 2]; 3]);
                        assert_eq!(
                            stream.render_block([control; CONTROLS_PER_BLOCK], &mut block)?,
                            0
                        );
                        assert_eq!(
                            block, [[[0; 2]; 3]; BLOCK_FRAMES],
                            "{name} stale output after completion"
                        );
                        return Ok(output);
                    }
                    assert!(
                        output.len() <= 60 * manifest.sample_rate as usize,
                        "{name} exceeded the test’s 60-second completion watchdog"
                    );
                }
            };
            let energy = |pcm: &[sequence::BusFrame], channel: usize| -> f64 {
                pcm.iter()
                    .map(|frame| f64::from(frame[0][channel]).powi(2))
                    .sum()
            };
            let baseline = render(1., None)?;
            let total = energy(&baseline, 0) + energy(&baseline, 1);
            assert!(total > 0., "{name} must be audible");
            for (volume, pan) in [(0., None), (0.5, None), (1., Some(0)), (1., Some(127))] {
                let actual = render(volume, pan)?;
                assert_eq!(
                    actual.len(),
                    baseline.len(),
                    "{name} controls changed completion"
                );
                let left = energy(&actual, 0);
                let right = energy(&actual, 1);
                match pan {
                    Some(0) => assert!(left > 8. * right, "{name} must pan left"),
                    Some(127) => assert!(right > 8. * left, "{name} must pan right"),
                    _ if volume == 0. => assert!(
                        actual.iter().all(|frame| *frame == [[0; 2]; 3]),
                        "{name} mute"
                    ),
                    _ => assert!(
                        (0.01..0.8).contains(&((left + right) / total)),
                        "{name} half volume must attenuate without muting"
                    ),
                }
            }
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires cooked menu program samples; never opens a device"]
    fn every_menu_sample_preserves_complete_decoded_pcm() -> Result<()> {
        let root = assets();
        let metadata: resonance_content::TitleAudio =
            serde_json::from_slice(&fs::read(root.join("title-sounds.json"))?)?;
        let manifest: resonance_audio::cue::package::Manifest =
            serde_json::from_slice(&fs::read(root.join(metadata.path))?)?;
        for (name, cue) in manifest.cues {
            let loaded = Package::load(&root, &cue.program.path)?;
            let package: Package = serde_json::from_slice(&fs::read(root.join(cue.program.path))?)?;
            for (id, asset) in package.samples {
                let mut wav = hound::WavReader::open(root.join(asset.path))?;
                assert_eq!(wav.spec().channels, 1);
                let expected = wav
                    .samples::<i16>()
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                let sample = &loaded.resources().samples[&id];
                assert_eq!(sample.pcm, expected, "{name} sample {id} decode");
            }
        }
        Ok(())
    }

    #[test]
    fn title_session_mixes_native_startup_and_overlapping_cues() -> Result<()> {
        use resonance_audio::{
            cue,
            data::{Command, EventKind, ScoreOrigin, VoiceSource},
        };
        let reverbs = [[0., 0., 1., 0., 0.]; 2];
        let (resources, score, tables) = crate::field_audio::test_score_data(ScoreOrigin::Sequence);
        let music = TitleMusic {
            music: Arc::new(Loaded::new(resources, score, tables, reverbs)?),
        };
        let cue = |amplitude, length, id| -> Result<_> {
            let (mut resources, mut score, tables) =
                crate::field_audio::test_score_data(ScoreOrigin::SoundEffect);
            let sample = Arc::make_mut(resources.samples.get_mut(&1).unwrap());
            sample.pcm = vec![amplitude; length];
            sample.loop_length = 0;
            sample.loop_pcm.clear();
            *resources.programs.get_mut(&1).unwrap().last_mut().unwrap() = Command::End;
            let EventKind::Notes { source, .. } = &mut score.first_events[0].kind else {
                unreachable!()
            };
            *source = VoiceSource::SoundEffect { id };
            Ok(Arc::new(Loaded::new(resources, score, tables, reverbs)?))
        };
        // Distinct, unsaturated signals make every overlapping contribution observable.
        let names = ["early", "middle", "late"];
        let cues = names
            .into_iter()
            .zip([(2000, 1600), (-1400, 3600), (3000, 4800)])
            .enumerate()
            .map(|(id, (name, (amplitude, length)))| {
                Ok((name.to_owned(), cue(amplitude, length, id as u16)?))
            })
            .collect::<Result<_>>()?;
        let assets = playback::test_assets(
            music,
            cue::package::Loaded {
                sample_rate: resonance_audio::SOURCE_RATE,
                reverbs,
                cues,
            },
        );
        const FRAMES: usize = 6400;
        let render = |names: &[&str]| -> Result<Vec<f32>> {
            let (audio, control) = assets.clone().session();
            let (mixer, mut output) = resonance_playback::Offline::new();
            let source = audio.decoder();
            let sink = mixer.play(false, move || Ok(Box::new(source)))?;
            for name in names {
                control.play(name)?;
            }
            let pcm = (0..FRAMES * 2)
                .map(|_| output.next().context("title output ended"))
                .collect::<Result<Vec<_>>>()?;
            assert_eq!(sink.rendered_frames(), FRAMES as u64);
            assert_eq!(control.rendered_frames(), FRAMES as u64);
            control.check()?;
            Ok(pcm)
        };
        let music = render(&[])?;
        let isolated = names
            .map(|name| render(&[name]))
            .into_iter()
            .collect::<Result<Vec<_>>>()?;
        let mixed = render(&names)?;
        let fade_frames = volume::frames_from_millis(volume::TITLE_STARTUP_MS)? as usize;
        for (frame, stereo) in music.chunks_exact(2).enumerate() {
            let gain = (frame as f64 / fade_frames as f64).min(1.);
            // At most two PCM units are lost to the envelope and bus gain quantizations.
            let expected = 12000. / 32768. * gain;
            assert!(
                stereo
                    .iter()
                    .all(|&actual| (f64::from(actual) - expected).abs() <= 2. / 32768.)
            );
        }
        assert_eq!(&music[..2], &[0.; 2]);
        assert_eq!(music[fade_frames * 2], music[fade_frames * 2 + 2]);
        let contributions: Vec<Vec<f32>> = isolated
            .iter()
            .map(|solo| {
                solo.iter()
                    .zip(&music)
                    .map(|(solo, music)| solo - music)
                    .collect()
            })
            .collect();
        assert!(
            contributions
                .iter()
                .all(|cue| cue.iter().any(|value| value.abs() > 0.005))
        );
        assert!(
            (0..FRAMES * 2).any(|i| contributions.iter().all(|cue| cue[i].abs() > 1. / 32768.)),
            "fixture never exercised simultaneous overlap"
        );
        for (i, actual) in mixed.iter().enumerate() {
            let expected = music[i] + contributions.iter().map(|cue| cue[i]).sum::<f32>();
            // Only floating-point addition order differs: all effects are dry and no bus clips.
            assert!(
                (actual - expected).abs() <= 4. * f32::EPSILON,
                "sample {i}: {actual} != {expected}"
            );
        }
        assert_eq!(
            &mixed[(FRAMES - 160) * 2..],
            &music[(FRAMES - 160) * 2..],
            "completed cues still contributed output"
        );
        Ok(())
    }

    #[test]
    #[ignore = "requires cooked title music; never opens a device"]
    fn title_music_preserves_playback_across_the_first_score_loop() -> Result<()> {
        let title = TitleMusic::load(&assets())?;
        let package = &title.music;
        // A bounded musical window, with loop boundaries reported by the score
        // renderer rather than pinned to an external loading/fade timeline.
        let preview = sequence::render_preview(
            package.clone(),
            package.reverbs(),
            40 * resonance_audio::SOURCE_RATE,
        )?;
        let first_loop = *preview.loop_starts.first().context("title did not loop")? as usize;
        let second = resonance_audio::SOURCE_RATE as usize;
        assert!(first_loop >= second && first_loop + second <= preview.pcm.len() / 2);
        for range in [
            first_loop - second..first_loop,
            first_loop..first_loop + second,
        ] {
            assert!(
                preview.pcm[range.start * 2..range.end * 2]
                    .iter()
                    .any(|&sample| sample != 0)
            );
        }
        let mut music = MusicFrames::start(&title)?;
        for (frame, expected) in preview.pcm.chunks_exact(2).enumerate() {
            assert_eq!(
                music.next_frame()?,
                [
                    f32::from(expected[0]) / 32768.,
                    f32::from(expected[1]) / 32768.
                ],
                "title frame {frame}"
            );
        }
        Ok(())
    }

    #[test]
    fn silent_startup_requires_a_muted_output_endpoint() {
        let app = App::new();
        assert!(validate_startup(&app, true, false).is_err());
        assert!(validate_startup(&app, true, true).is_ok());
    }
}
