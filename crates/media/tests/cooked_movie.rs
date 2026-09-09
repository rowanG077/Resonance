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
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../local/cooked");
    let asset: MovieAsset =
        serde_json::from_slice(&fs::read(root.join("story-intro.json")).unwrap()).unwrap();
    // Exercise both cancellation before the worker opens the input and during
    // FFmpeg's stream probe. Invalid pixel formats previously aborted in swscale.
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
#[ignore = "requires locally cooked GQSEAF opening and its intermediate audio"]
fn decodes_every_opening_frame_and_preserves_lossless_audio() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../local/cooked");
    let document: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("intro.json")).expect("cook-intro first"))
            .unwrap();
    let asset: MovieAsset = serde_json::from_value(document.clone()).unwrap();
    let stream = document["recipe"]["audio_stream"].as_u64().unwrap();
    let mut wave =
        hound::WavReader::open(root.join(format!("intermediate/intro/audio-{stream}-stereo.wav")))
            .unwrap();
    let mut expected = Sha256::new();
    for sample in wave.samples::<i16>() {
        expected.update(sample.unwrap().to_le_bytes());
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
                if video.index == 1137 {
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
