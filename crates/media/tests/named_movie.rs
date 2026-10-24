use resonance_content::{
    library::{Library, Mod},
    movie,
};
use resonance_media::{
    MovieDecoder, MovieEvent,
    encode::{AUDIO_BLOCK, MovieWriter},
};
use std::{
    fs,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = Self(std::env::temp_dir().join(format!(
            "named-movie-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        )));
        fs::create_dir_all(&root.0).unwrap();
        root
    }
    fn write(&self, path: &str, bytes: &[u8]) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn movie(path: &Path, width: u32, rate: u32, frames: u32, value: u8) -> Vec<i16> {
    let samples = u64::from(frames) * 40_000 * u64::from(rate) / 1_000_000;
    let pcm = (0..samples * 2)
        .map(|i| ((i * 7919) as i16).wrapping_add(i16::from(value)))
        .collect::<Vec<_>>();
    // Unknown length in the header exercises EOF-based preparation/playback.
    let mut writer =
        MovieWriter::new(fs::File::create(path).unwrap(), width, 16, 40_000, rate).unwrap();
    let (mut vi, mut ai) = (0, 0);
    while vi < frames || ai < samples as usize {
        if ai < samples as usize
            && (vi == frames || ai as u64 * 1_000_000 <= u64::from(vi) * 40_000 * u64::from(rate))
        {
            let end = (ai + AUDIO_BLOCK).min(samples as usize);
            writer.audio(&pcm[ai * 2..end * 2]).unwrap();
            ai = end;
        } else {
            writer
                .video(
                    &vec![value; width as usize * 16 * 3],
                    Duration::from_micros(u64::from(vi) * 40_000),
                )
                .unwrap();
            vi += 1;
        }
    }
    writer.finish().unwrap();
    pcm
}

fn check(decoder: MovieDecoder, frames: u32, value: u8, pcm: &[i16]) {
    let times = decoder.video_timestamps().unwrap();
    assert_eq!(times.len(), frames as usize);
    let mut count = 0;
    let mut actual = Vec::new();
    let start = Instant::now();
    loop {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "movie decoder stalled"
        );
        match decoder.try_next().unwrap() {
            Some(MovieEvent::Video(frame)) => {
                assert_eq!(frame.index, count);
                assert_eq!(frame.timestamp, times[count as usize]);
                assert!(
                    frame
                        .rgba
                        .chunks_exact(4)
                        .all(|p| p == [value, value, value, 255])
                );
                count += 1;
            }
            Some(MovieEvent::Audio(chunk)) => {
                assert_eq!(chunk.start_frame, actual.len() as u64 / 2);
                actual.extend(
                    chunk
                        .samples
                        .into_iter()
                        .map(|s| (s * 32768.).round() as i16),
                );
            }
            Some(MovieEvent::End) => break,
            None => thread::sleep(Duration::from_millis(1)),
        }
    }
    assert_eq!(count, frames);
    assert_eq!(actual, pcm);
}

#[test]
fn named_movie_replacements_mods_and_retained_handles_use_current_media() {
    let root = Root::new();
    root.write(
        "content.json",
        br#"{"version":1,"game":"GQSEAF","revision":0}"#,
    );
    root.write("movies/mappings.json", br#"{"0":"opening","7":"opening"}"#);
    root.write(
        "movies/opening/movie.json",
        br#"{"version":1,"path":"movies/opening/video.mkv","audio_track":0}"#,
    );
    let video = root.0.join("movies/opening/video.mkv");
    let original = movie(&video, 16, 32028, 40, 23);
    let library = Library::open(&root.0, &[]).unwrap();
    let selected = movie::capture(&mut library.snapshot(), 0).unwrap();
    // Replace the pathname after admission and before either worker starts.
    // Both workers must still decode the selected file, without reopening it.
    fs::rename(&video, video.with_extension("previous")).unwrap();
    let replacement = movie(&video, 32, 22050, 12, 71);
    let decoder = MovieDecoder::from_file(selected.file, selected.definition.audio_track).unwrap();
    assert_eq!(decoder.format().width, 16);
    assert_eq!(decoder.format().sample_rate, 32028);
    assert_eq!(decoder.format().audio_frames, None);
    check(decoder, 40, 23, &original);
    let decoder = MovieDecoder::load(&mut library.snapshot(), 7).unwrap();
    assert_eq!(decoder.format().width, 32);
    assert_eq!(decoder.format().sample_rate, 22050);
    check(decoder, 12, 71, &replacement);

    let modified = Root::new();
    modified.write("movies/mappings.json", br#"{"0":"alternate"}"#);
    modified.write(
        "movies/alternate/movie.json",
        br#"{"version":1,"path":"movies/opening/video.mkv"}"#,
    );
    modified.write("movies/opening/video.mkv", b"");
    let changed = movie(
        &modified.0.join("movies/opening/video.mkv"),
        16,
        48000,
        2,
        199,
    );
    let modded = Library::open(
        &root.0,
        &[Mod {
            name: "alternate".into(),
            root: modified.0.clone(),
        }],
    )
    .unwrap();
    let mut snapshot = modded.snapshot();
    let selected = movie::capture(&mut snapshot, 0).unwrap();
    assert!(snapshot.overrides().iter().any(|o| o.id == Some(0)));
    assert!(
        snapshot
            .overrides()
            .iter()
            .any(|o| o.path == "movies/opening/video.mkv")
    );
    // This also guards against workers accidentally using the unmodified base.
    fs::rename(
        modified.0.join("movies/opening/video.mkv"),
        modified.0.join("selected.mkv"),
    )
    .unwrap();
    check(
        MovieDecoder::from_file(selected.file, 0).unwrap(),
        2,
        199,
        &changed,
    );
    check(
        MovieDecoder::load(&mut library.snapshot(), 0).unwrap(),
        12,
        71,
        &replacement,
    );
    root.write(
        "movies/opening/movie.json",
        br#"{"version":1,"path":"movies/opening/video.mkv","audio_track":2}"#,
    );
    let error = MovieDecoder::load(&mut library.snapshot(), 0)
        .err()
        .unwrap();
    assert!(format!("{error:#}").contains("movie has no audio track 2"));
}

#[test]
fn unknown_lengths_validate_decoded_audio_and_video_durations() {
    let root = Root::new();
    let path = root.0.join("mismatched.mkv");
    let mut writer =
        MovieWriter::new(fs::File::create(&path).unwrap(), 16, 16, 40_000, 32_000).unwrap();
    writer.video(&vec![0; 16 * 16 * 3], Duration::ZERO).unwrap();
    for _ in 0..8 {
        writer.audio(&vec![0; AUDIO_BLOCK * 2]).unwrap();
    }
    writer.finish().unwrap();
    let decoder = MovieDecoder::open(&path, 0).unwrap();
    assert_eq!(decoder.format().audio_frames, None);
    let started = Instant::now();
    loop {
        assert!(started.elapsed() < Duration::from_secs(5));
        match decoder.try_next() {
            Err(error) => {
                assert!(
                    error
                        .to_string()
                        .contains("audio and video durations disagree")
                );
                break;
            }
            Ok(Some(MovieEvent::End)) => panic!("accepted mismatched stream durations"),
            Ok(None) => thread::sleep(Duration::from_millis(1)),
            _ => {}
        }
    }
}
