use resonance_content::MovieAsset;
use resonance_media::{
    MovieDecoder, MovieEvent, VideoReader,
    encode::{AUDIO_BLOCK, MovieWriter},
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU32, Ordering},
    thread,
    time::{Duration, Instant},
};

struct Temporary(PathBuf);
impl Temporary {
    fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        Self(std::env::temp_dir().join(format!(
            "resonance-codecs-{}-{}.mkv",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
fn pixels(frame: usize) -> Vec<u8> {
    (0..16 * 16 * 3)
        .map(|i| ((i * 37 + frame * 53 + i / 7) % 256) as u8)
        .collect()
}

#[test]
fn reads_independent_rgb_fixture_including_non_keyframes() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/rgb-gop-v3.mkv");
    let mut reader = VideoReader::open(&path).unwrap();
    for index in 0..6 {
        let frame = reader.next_frame().unwrap().unwrap();
        assert_eq!(frame.index, index);
        assert_eq!(
            frame.timestamp,
            Duration::from_millis(u64::from(index) * 40)
        );
        let rgb: Vec<u8> = frame
            .rgba
            .chunks_exact(4)
            .flat_map(|p| p[..3].iter().copied())
            .collect();
        assert_eq!(rgb, pixels(index as usize));
    }
    assert!(reader.next_frame().unwrap().is_none());
}

#[test]
fn encoded_movie_preserves_rgb_pcm_and_timestamps_and_cancels_when_full() {
    let path = Temporary::new();
    let frames = 40u32;
    let audio_frames = u64::from(frames) * 33366 * 32028 / 1_000_000;
    let pcm: Vec<i16> = (0..audio_frames * 2)
        .map(|i| (i.wrapping_mul(7919) as i16).wrapping_sub(17000))
        .collect();
    let mut writer =
        MovieWriter::new(fs::File::create(&path.0).unwrap(), 16, 16, 33366, 32028).unwrap();
    let (mut vi, mut ai) = (0u32, 0usize);
    while vi < frames || ai < pcm.len() / 2 {
        if ai < pcm.len() / 2
            && (vi == frames || ai as u64 * 1_000_000 <= u64::from(vi) * 33366 * 32028)
        {
            let end = (ai + AUDIO_BLOCK).min(pcm.len() / 2);
            writer.audio(&pcm[ai * 2..end * 2]).unwrap();
            ai = end;
        } else {
            writer
                .video(
                    &pixels(vi as usize),
                    Duration::from_micros(u64::from(vi) * 33366),
                )
                .unwrap();
            vi += 1;
        }
    }
    writer.finish().unwrap();
    let asset = MovieAsset {
        version: 1,
        path: "movies/test.mkv".into(),
        sha256: "0".repeat(64),
        width: 16,
        height: 16,
        frames,
        frame_micros: 33366,
        sample_rate: 32028,
        channels: 2,
        audio_frames,
    };
    let decoder = MovieDecoder::open(&path.0, asset.clone()).unwrap();
    let start = Instant::now();
    let (mut count, mut actual) = (0, Vec::new());
    loop {
        assert!(start.elapsed() < Duration::from_secs(10));
        match decoder.try_next().unwrap() {
            Some(MovieEvent::Video(frame)) => {
                assert_eq!(frame.index, count);
                assert_eq!(
                    frame.timestamp,
                    Duration::from_micros(u64::from(count) * 33366)
                );
                let rgb: Vec<_> = frame
                    .rgba
                    .chunks_exact(4)
                    .flat_map(|p| p[..3].iter().copied())
                    .collect();
                assert_eq!(rgb, pixels(count as usize));
                count += 1;
            }
            Some(MovieEvent::Audio(chunk)) => {
                assert_eq!(chunk.start_frame, actual.len() as u64 / 2);
                actual.extend(chunk.samples.iter().map(|v| (v * 32768.).round() as i16));
            }
            Some(MovieEvent::End) => break,
            None => thread::sleep(Duration::from_millis(1)),
        }
    }
    assert_eq!(count, frames);
    assert_eq!(actual, pcm);
    drop(decoder);
    let decoder = MovieDecoder::open(&path.0, asset.clone()).unwrap();
    thread::sleep(Duration::from_millis(100));
    let start = Instant::now();
    drop(decoder);
    assert!(start.elapsed() < Duration::from_secs(2));
    // A valid container that ends before its declared manifest must report an
    // error, never successful completion.
    let mut wrong = asset;
    wrong.frames += 1;
    let decoder = MovieDecoder::open(&path.0, wrong).unwrap();
    let start = Instant::now();
    loop {
        assert!(start.elapsed() < Duration::from_secs(10));
        match decoder.try_next() {
            Err(_) => break,
            Ok(Some(MovieEvent::End)) => panic!("accepted an early movie end"),
            Ok(None) => thread::sleep(Duration::from_millis(1)),
            _ => {}
        }
    }
}
