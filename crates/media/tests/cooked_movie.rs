use resonance_content::library::Library;
use resonance_media::{MovieDecoder, MovieEvent};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

#[test]
#[ignore = "requires a locally cooked story movie; never opens an audio device"]
fn cancellation_after_admission_releases_active_decoders() {
    let root = std::env::var_os("RESONANCE_COOKED_TEST_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets")
        });
    let library = Library::open(&root, &[]).unwrap();
    // Admission probes the retained stream before starting workers. Exercise
    // immediate cancellation and cancellation at different decode positions.
    for delay in [0, 1, 2, 4, 8, 16] {
        for _ in 0..8 {
            let decoder = MovieDecoder::load(&mut library.snapshot(), 1).unwrap();
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
    check_movie(0);
}

#[test]
#[ignore = "requires the cooked story movie and original H4M; silent comparison"]
fn decodes_every_story_frame_and_preserves_lossless_audio() {
    check_movie(1);
}

fn check_movie(id: u32) {
    let root = std::env::var_os("RESONANCE_COOKED_TEST_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets")
        });
    let library = Library::open(&root, &[]).unwrap();
    let mut snapshot = library.snapshot();
    let entry = snapshot.movie(id).unwrap();
    let definition: resonance_content::movie::Definition =
        snapshot.json(&entry, 64 * 1024).unwrap();
    let extracted = std::env::var_os("RESONANCE_EXTRACTED_TEST_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1")
        });
    let original = extracted.join("files/MOV").join(match id {
        0 => "op.h4m",
        1 => "s01.h4m",
        _ => panic!("no original movie recipe for {id}"),
    });
    let mut header = [0; 68];
    fs::File::open(&original)
        .unwrap()
        .read_exact(&mut header)
        .unwrap();
    let source_frames = u32::from_be_bytes(header[0x1c..0x20].try_into().unwrap());
    let width = u16::from_be_bytes(header[0x34..0x36].try_into().unwrap()) as usize;
    let height = u16::from_be_bytes(header[0x36..0x38].try_into().unwrap()) as usize;
    let mut source = h4m::AudioDecoder::new(
        std::io::BufReader::new(fs::File::open(original).unwrap()),
        definition.audio_track + 1,
    )
    .unwrap();
    let mut source_audio_frames = 0;
    let mut expected = Sha256::new();
    while let Some(pcm) = source.next_block().unwrap() {
        source_audio_frames += pcm.len() as u64 / 2;
        for frame in pcm.chunks_exact(2) {
            for sample in frame.iter().rev() {
                expected.update(sample.to_le_bytes());
            }
        }
    }
    let decoder = MovieDecoder::load(&mut library.snapshot(), id).unwrap();
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
                assert_eq!(video.rgba.len(), width * height * 4);
                assert!(video.rgba.chunks_exact(4).all(|pixel| pixel[3] == 255));
                if id == 0 && video.index == 1137 {
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
    assert_eq!(frames, source_frames);
    assert_eq!(audio_frames, source_audio_frames);
    assert_eq!(
        actual.finalize(),
        expected.finalize(),
        "FLAC playback changed source PCM"
    );
    // A full output queue must not prevent cancellation when the user skips.
    let decoder = MovieDecoder::load(&mut library.snapshot(), id).unwrap();
    thread::sleep(Duration::from_millis(300));
    let started = Instant::now();
    drop(decoder);
    assert!(started.elapsed() < Duration::from_secs(5));
}
