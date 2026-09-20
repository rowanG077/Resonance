use resonance_content::MovieAsset;
use resonance_media::{MovieDecoder, MovieEvent};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

#[test]
#[ignore = "requires a locally cooked story movie; never opens an audio device"]
fn cancellation_during_stream_probing_does_not_use_incomplete_formats() {
    let root = std::env::var_os("RESONANCE_COOKED_TEST_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets")
        });
    let asset: MovieAsset =
        serde_json::from_slice(&fs::read(root.join("story-intro.json")).unwrap()).unwrap();
    // Exercise both cancellation before the worker opens the input and during
    // container probing, before any codec state is ready.
    for delay in [0, 1, 2, 4, 8, 16] {
        for _ in 0..8 {
            let decoder = MovieDecoder::open(&root.join(&asset.path), asset.clone()).unwrap();
            thread::sleep(Duration::from_millis(delay));
            let started = Instant::now();
            drop(decoder);
            assert!(started.elapsed() < Duration::from_secs(5));
        }
    }
}

#[test]
#[ignore = "requires the cooked opening and original H4M; silent comparison"]
fn decodes_every_opening_frame_and_preserves_lossless_audio() {
    check_movie("intro");
}

#[test]
#[ignore = "requires the cooked story movie and original H4M; silent comparison"]
fn decodes_every_story_frame_and_preserves_lossless_audio() {
    check_movie("story-intro");
}

fn check_movie(name: &str) {
    let root = std::env::var_os("RESONANCE_COOKED_TEST_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets")
        });
    let document: serde_json::Value = serde_json::from_slice(
        &fs::read(root.join(format!("{name}.json"))).expect("cook-intro first"),
    )
    .unwrap();
    let asset: MovieAsset = serde_json::from_value(document.clone()).unwrap();
    let physical: serde_json::Value = serde_json::from_slice(
        &fs::read(root.join(&asset.path).parent().unwrap().join("movie.json")).unwrap(),
    )
    .unwrap();
    let extracted = std::env::var_os("RESONANCE_EXTRACTED_TEST_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1")
        });
    let mut source = h4m::AudioDecoder::new(
        std::io::BufReader::new(
            fs::File::open(
                extracted
                    .join("files")
                    .join(physical["source"].as_str().unwrap()),
            )
            .unwrap(),
        ),
        asset.audio_track + 1,
    )
    .unwrap();
    let mut expected = Sha256::new();
    while let Some(pcm) = source.next_block().unwrap() {
        for frame in pcm.chunks_exact(2) {
            for sample in frame.iter().rev() {
                expected.update(sample.to_le_bytes());
            }
        }
    }
    let decoder = MovieDecoder::open(&root.join(&asset.path), asset.clone()).unwrap();
    let started = Instant::now();
    let mut frames = 0;
    let mut audio_frames = 0;
    let mut actual = Sha256::new();
    loop {
        assert!(
            started.elapsed() < Duration::from_secs(120),
            "movie decoder stalled"
        );
        match decoder.try_next().unwrap() {
            Some(MovieEvent::Video(video)) => {
                assert_eq!(video.index, frames);
                assert_eq!(
                    video.rgba.len(),
                    asset.width as usize * asset.height as usize * 4
                );
                assert!(video.rgba.chunks_exact(4).all(|pixel| pixel[3] == 255));
                if name == "intro" && video.index == 1137 {
                    assert_eq!(video.rgba[(214 * 640 + 499) * 4], 217);
                }
                frames += 1;
            }
            Some(MovieEvent::Audio(audio)) => {
                assert_eq!(audio.start_frame, audio_frames);
                audio_frames += audio.samples.len() as u64 / 2;
                for sample in audio.samples {
                    actual.update(((sample * 32768.0).round() as i16).to_le_bytes());
                }
            }
            Some(MovieEvent::End) => break,
            None => thread::sleep(Duration::from_millis(1)),
        }
    }
    assert_eq!(frames, asset.frames);
    assert_eq!(audio_frames, asset.audio_frames);
    assert_eq!(
        actual.finalize(),
        expected.finalize(),
        "FLAC playback changed source PCM"
    );
    // A full output queue must not prevent cancellation when the user skips.
    let decoder = MovieDecoder::open(&root.join(&asset.path), asset).unwrap();
    thread::sleep(Duration::from_millis(300));
    let started = Instant::now();
    drop(decoder);
    assert!(started.elapsed() < Duration::from_secs(5));
}
