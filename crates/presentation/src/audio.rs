use anyhow::{Context, Result, ensure};
use bevy::prelude::*;
use resonance_audio::{
    package::{Loaded, Package},
    sequence, volume,
};
use resonance_content::TitleAudio;
use resonance_playback::{ChannelCount, Decodable, SampleRate, Source};
use std::{fs, path::Path, sync::Arc, time::Duration};

mod playback;
pub(super) use playback::{GameAudio, PlaybackAssets};

#[derive(Resource, Default)]
pub(super) struct MenuSounds {
    pub control: Option<playback::SoundControl>,
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
/// Bevy owns output; this source neither reads original resources nor loops PCM.
#[derive(Asset, TypePath, Clone)]
pub(super) struct TitleMusic {
    music: Arc<Loaded>,
    master_lead_ms: u16,
}

impl TitleMusic {
    pub fn load(root: &Path) -> Result<Option<Self>> {
        let path = root.join("title-audio.json");
        if !path.is_file() {
            return Ok(None);
        }
        let info: TitleAudio = serde_json::from_slice(&fs::read(path)?)?;
        info.validate()?;
        Ok(Some(Self {
            music: Arc::new(Package::load(root, &info.path)?),
            master_lead_ms: 1185,
        }))
    }

    pub(super) fn entry(mut self, full_intro: bool) -> Self {
        // Independent clocks/fade states in the two accepted oracle fixtures.
        self.master_lead_ms = if full_intro { 1200 } else { 1185 };
        self
    }
}

pub(super) struct MusicFrames {
    stream: sequence::stream::Stream,
    studio: resonance_audio::reverb::Studio,
    fade: volume::Startup,
    frame: u64,
    pcm: [i16; 320],
    cursor: usize,
    stopped: bool,
}

impl MusicFrames {
    fn start(music: &TitleMusic) -> Result<Self> {
        Ok(Self {
            stream: sequence::stream::Stream::cold(music.music.clone(), true)?,
            studio: resonance_audio::reverb::Studio::new(music.music.reverbs)?,
            fade: volume::Startup::new(2000, 100, music.master_lead_ms)?,
            frame: 0,
            pcm: [0; 320],
            cursor: 320,
            stopped: false,
        })
    }
    fn stop(&mut self) -> Result<()> {
        self.stopped = true;
        self.stream.stop()
    }
    fn render(&mut self) -> Result<()> {
        let controls = std::array::from_fn(|i| sequence::LiveControls {
            volume: self.fade.value_at(self.frame + i as u64 * 32),
            ..Default::default()
        });
        let mut buses = [[[0; 2]; 3]; 160];
        ensure!(
            self.stream.render_block(controls, &mut buses)? == 160,
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
        self.frame += 160;
        self.cursor = 0;
        Ok(())
    }
}

impl Iterator for MusicFrames {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.stopped {
            return None;
        }
        if self.cursor == 320 {
            self.render().expect("title synthesis failed");
        }
        let sample = f32::from(self.pcm[self.cursor]) / 32768.;
        self.cursor += 1;
        Some(sample)
    }
}

impl Source for MusicFrames {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> ChannelCount {
        ChannelCount::new(2).unwrap()
    }
    fn sample_rate(&self) -> SampleRate {
        SampleRate::new(32028).unwrap()
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

impl Decodable for TitleMusic {
    type Decoder = MusicFrames;
    fn decoder(&self) -> MusicFrames {
        MusicFrames::start(self).expect("could not initialize title synthesis")
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
    full_intro: bool,
    events: &[CueEvent],
) -> Result<()> {
    ensure!(
        (1..=32000 * 120).contains(&frames),
        "music recording exceeds 120 seconds"
    );
    ensure!(
        events.len() <= 1024
            && events.windows(2).all(|w| w[0].frame <= w[1].frame)
            && events.iter().all(|event| event.frame < frames),
        "invalid cue recording schedule"
    );
    let assets = PlaybackAssets::load(root)?;
    ensure!(assets.has_music(), "missing cooked title music");
    let (audio, control) = assets.session(full_intro);
    let mut decoder = audio.decoder();
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = output.with_extension("partial.wav");
    let mut writer = hound::WavWriter::create(
        &temporary,
        hound::WavSpec {
            channels: 2,
            sample_rate: 32028,
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
            let sample = decoder
                .next()
                .context("audio source ended before requested window")?;
            writer.write_sample((sample * 32768.0) as i16)?;
        }
    }
    ensure!(
        control.rendered_frames() == u64::from(frames),
        "audio source frame accounting differs"
    );
    decoder.stop()?;
    writer.finalize()?;
    fs::rename(temporary, output)?;
    fs::write(
        output.with_extension("json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "version":2,"source":"resonance_native_audio","audio_device":false,"frames":frames,
            "sample_rate":32028,"channels":2,"full_intro_entry":full_intro,
            "cue_events":events.iter().map(|event|serde_json::json!({"frame":event.frame,"cue":event.cue})).collect::<Vec<_>>(),
        }))?,
    )?;
    println!("Recorded {frames} frames from the native music/cue source without an audio device");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn assets() -> std::path::PathBuf {
        std::env::var_os("RESONANCE_TEST_ASSETS").map_or_else(
            || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/cooked"),
            Into::into,
        )
    }

    #[test]
    #[ignore = "requires cooked menu programs and pinned muted Dolphin recordings; never opens a device"]
    fn program_cues_match_dolphin_and_respect_live_group_volume() {
        use resonance_audio::cue::{Studio, package::Manifest};
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let cooked = assets();
        let metadata: resonance_content::TitleSounds =
            serde_json::from_slice(&fs::read(cooked.join("title-sounds.json")).unwrap()).unwrap();
        let bank = Manifest::load(&cooked, &metadata.path, &metadata.sha256).unwrap();
        let cases: serde_json::Value = serde_json::from_slice(
            &fs::read(root.join("tools/oracle/cases/menu-program-audio.json")).unwrap(),
        )
        .unwrap();
        for case in cases["cases"].as_array().unwrap() {
            let name = case["cue"].as_str().unwrap();
            let label = case["id"].as_str().unwrap_or(name);
            let volume = case["volume"].as_u64().unwrap_or(127);
            assert!(volume <= 127);
            let start = case["window"]["reference_start_frame"].as_u64().unwrap() as u32;
            let frames = case["window"]["frames"].as_u64().unwrap() as u32;
            let read = |key: &str| {
                let fixture = &case[key];
                let bytes = fs::read(root.join(fixture["path"].as_str().unwrap())).unwrap();
                assert_eq!(
                    format!("{:x}", Sha256::digest(&bytes)),
                    fixture["sha256"].as_str().unwrap()
                );
                let mut wave = hound::WavReader::new(std::io::Cursor::new(bytes)).unwrap();
                assert_eq!((wave.spec().sample_rate, wave.spec().channels), (32028, 2));
                assert!(frames > 0 && wave.duration() >= start + frames);
                wave.seek(start).unwrap();
                wave.samples::<i16>()
                    .take(frames as usize * 2)
                    .map(Result::unwrap)
                    .collect::<Vec<_>>()
            };
            let reference = read("reference");
            let baseline = read("baseline");
            assert_ne!(reference, baseline, "{name} reference contains no cue");
            let cue = bank.cues[name].clone();
            let mut studio = Studio::new(bank.reverbs).unwrap();
            studio.set_group_volume(volume as f32 / 127.).unwrap();
            studio.play(cue.clone()).unwrap();
            for frame in 0..frames as usize {
                for (channel, actual) in studio.next_frame().into_iter().enumerate() {
                    let index = frame * 2 + channel;
                    let expected = i32::from(reference[index]) - i32::from(baseline[index]);
                    assert!(
                        (i32::from(actual) - expected).abs() <= 1,
                        "{label} frame {frame}, channel {channel}: {actual} != {expected}"
                    );
                }
            }
            if case["volume"].is_number() {
                continue;
            }
            let mut muted = Studio::new(bank.reverbs).unwrap();
            muted.set_group_volume(0.).unwrap();
            muted.play(cue).unwrap();
            for _ in 0..320 {
                assert_eq!(muted.next_frame(), [0; 2]);
            }
            muted.set_group_volume(1.).unwrap();
            assert!(
                (0..3200).any(|_| muted.next_frame() != [0; 2]),
                "{name} did not resume after the group gain changed"
            );
        }
    }

    #[test]
    #[ignore = "requires local cooked audio and the pinned silent Dolphin recording; never opens a device"]
    fn music_and_menu_cues_match_the_independent_dolphin_mix() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let reference_path = root.join("oracle/title-customize-confirmation-ready-silent/user/Dump/Audio/GQSEAF_2026-09-07_10-26-58_dspdump.wav");
        assert_eq!(
            format!("{:x}", Sha256::digest(fs::read(&reference_path).unwrap())),
            "d81d394be8502b14c82f7103401c892b02269eeb2fd7b4b071a9cf1804fbe9eb"
        );
        let mut reference = hound::WavReader::open(reference_path).unwrap();
        assert_eq!(reference.duration(), 1_954_136);
        assert_eq!(reference.spec().channels, 2);
        assert_eq!(reference.spec().sample_rate, 32028);
        reference.seek(1_383_592).unwrap();
        let mut expected = reference.samples::<i16>();
        let (audio, control) = PlaybackAssets::load(&assets()).unwrap().session(false);
        // Same source and stereo mixer used by Bevy, consumed without a device.
        let (mixer, mut output) = resonance_playback::Offline::new();
        let source = audio.decoder();
        let _sink = mixer.play(false, move || Ok(Box::new(source))).unwrap();
        // Entire available title interval, including both complete cue tails.
        for frame in 0..570_544 {
            let cue = match frame {
                327_040 => Some("navigate"),
                487_360 => Some("confirm"),
                _ => None,
            };
            if let Some(cue) = cue {
                control.play(cue).unwrap();
            }
            for channel in 0..2 {
                let actual = (output.next().unwrap() * 32768.0) as i16;
                assert_eq!(
                    actual,
                    expected.next().unwrap().unwrap(),
                    "frame {frame}, channel {channel}"
                );
            }
        }
        drop(output); // Cancels and joins the bounded synthesis worker.
    }

    #[test]
    #[ignore = "requires locally cooked cues/music and independent muted Dolphin recordings"]
    fn startup_fade_overlapping_cues_and_first_loop_match_dolphin() {
        use sha2::{Digest, Sha256};
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        for name in [
            "navigation-audio-startup",
            "navigation-audio-player-overlap",
            "title-music-loop",
        ] {
            let case: serde_json::Value = serde_json::from_slice(
                &fs::read(root.join(format!("tools/oracle/cases/{name}.json"))).unwrap(),
            )
            .unwrap();
            let bytes = fs::read(root.join(case["reference"].as_str().unwrap())).unwrap();
            assert_eq!(
                format!("{:x}", Sha256::digest(&bytes)),
                case["reference_sha256"].as_str().unwrap()
            );
            let mut reference = hound::WavReader::new(std::io::Cursor::new(bytes)).unwrap();
            reference
                .seek(case["window"]["reference_start_frame"].as_u64().unwrap() as u32)
                .unwrap();
            let mut expected = reference.samples::<i16>();
            let (source, control) = PlaybackAssets::load(&assets()).unwrap().session(false);
            let mut output = source.decoder();
            let events = case["cue_events"].as_array().map_or(&[][..], Vec::as_slice);
            for frame in 0..case["window"]["frames"].as_u64().unwrap() {
                for event in events {
                    if event["frame"].as_u64().unwrap() == frame {
                        control.play(event["cue"].as_str().unwrap()).unwrap();
                    }
                }
                for channel in 0..2 {
                    assert_eq!(
                        (output.next().unwrap() * 32768.) as i16,
                        expected.next().unwrap().unwrap(),
                        "{name}, frame {frame}, channel {channel}"
                    );
                }
            }
            output.stop().unwrap();
        }
    }

    #[test]
    fn silent_startup_requires_a_muted_output_endpoint() {
        let app = App::new();
        assert!(validate_startup(&app, true, false).is_err());
        assert!(validate_startup(&app, true, true).is_ok());
    }
}
