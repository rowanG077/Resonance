//! Steam USM sources become the same FFV1/FLAC Matroska files as GC movies.
use crate::{output, pe, source::Source};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    field::document::Definition,
    library::{FIELDS, MOVIES, Mappings},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    process::Command,
};
use symphonia_script::{NativeCall, Program, scenario};

pub(crate) fn cook(source: &Source, library: &Path, selected: Option<u32>) -> Result<()> {
    let image = pe::Image::parse(&source.executable)?;
    let mut sources = BTreeMap::new();
    for (id, row) in image
        .slice(0x8b4780, 26 * 4)?
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
    {
        let path = image.text(pe::word(row, 0)?)?;
        let filename = path
            .strip_prefix("/MOVIE/")
            .context("movie outside native directory")?;
        crate::source::validate_name(filename)?;
        let input = source
            .root
            .join("P12E/Data/TOS_Server/Rawdata/WIN/MOVIE")
            .join(filename);
        if input.is_file() {
            sources.insert(id as u32, (filename.to_owned(), input));
        } else {
            ensure!(
                name(filename).is_err(),
                "missing installed movie {filename}"
            );
        }
    }
    ensure!(!sources.is_empty(), "no installed movies found");
    if let Some(id) = selected {
        ensure!(sources.contains_key(&id), "movie {id} is not installed");
    }
    let voiced = [1, 8]
        .into_iter()
        .filter_map(|id| sources.get(&id).map(|(name, _)| name.clone()))
        .collect::<BTreeSet<_>>();
    let mut written = BTreeSet::new();
    let mut mappings = BTreeMap::new();
    for (&id, (source_name, input)) in &sources {
        if selected.is_some_and(|only| sources[&only].0 != *source_name) {
            continue;
        }
        let name = name(source_name)?;
        let target = format!("movies/{name}.mkv");
        if written.insert(source_name) {
            eprintln!("  Movie: {name} ({source_name})");
            convert(input, &library.join(&target), voiced.contains(source_name))?;
        }
        mappings.insert(id, format!("{name}.mkv"));
    }
    if selected.is_some() && library.join(MOVIES).is_file() {
        let existing: Mappings = serde_json::from_slice(&fs::read(library.join(MOVIES))?)?;
        for (id, path) in existing.iter() {
            mappings.entry(id).or_insert_with(|| path.to_owned());
        }
    }
    let mappings = Mappings::new(mappings)?;
    output::json(&library.join(MOVIES), &mappings)?;
    bind_fields(library, &mappings)
}

fn name(source: &str) -> Result<&'static str> {
    Ok(match source {
        "TOS_GC_OP_720P.USM" => "opening",
        "TOS_PS2_OP_720P.USM" => "opening-alternate",
        "TOS_GC_S01_720P.USM" => "story-introduction",
        "TOS_GC_S03_720P.USM" => "tower-of-salvation-appears",
        "TOS_GC_AS01_720P.USM" => "mutated-tree-appears",
        "TOS_GC_S07_720P.USM" => "mana-cannon-appears",
        "TOS_GC_S08_720P.USM" => "mana-cannon-firing",
        "TOS_GC_S09_720P.USM" => "tower-of-salvation-collapse",
        "TOS_GC_AS02_720P.USM" => "worlds-reunited",
        "TOS_GC_AS03_720P.USM" => "great-seed-awakening",
        "TOS_GC_S10_720P.USM" => "derris-kharlan-revealed",
        "TOS_PS2_AS01_720P.USM" => "colette-angel-transformation",
        "TOS_PS2_AS02_720P.USM" => "rheairds-first-flight",
        other => anyhow::bail!("unnamed movie {other}"),
    })
}

fn convert(source: &Path, destination: &Path, english: bool) -> Result<()> {
    fs::create_dir_all(
        destination
            .parent()
            .context("movie has no output directory")?,
    )?;
    let temporary = destination.with_extension("partial.mkv");
    let mut command = Command::new("ffmpeg");
    command
        .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(source)
        .args(["-map", "0:v:0"]);
    if english {
        command.args(["-map", "0:a:1", "-map", "0:a:0"]);
    } else {
        command.args(["-map", "0:a"]);
    }
    let status = command
        .args([
            "-map_metadata",
            "-1",
            "-map_chapters",
            "-1",
            "-vf",
            "scale=640:-2:flags=lanczos",
            "-fps_mode",
            "passthrough",
            "-c:v",
            "ffv1",
            "-level",
            "3",
            "-coder",
            "1",
            "-g",
            "1",
            "-slices",
            "4",
            "-slicecrc",
            "1",
            "-pix_fmt",
            "bgr0",
            "-threads",
            "4",
            "-c:a",
            "flac",
            "-sample_fmt",
            "s16",
            "-ac",
            "2",
        ])
        .args(["-ar", &resonance_audio::SOURCE_RATE.to_string()])
        .arg(&temporary)
        .status()
        .context("run ffmpeg (Steam movies require FFmpeg with USM support)")?;
    ensure!(
        status.success(),
        "movie conversion failed: {}",
        source.display()
    );
    let movie = resonance_media::MovieDecoder::open(&temporary, 0)?;
    ensure!(movie.format().width == 640, "unexpected movie dimensions");
    ensure!(
        movie.format().sample_rate == resonance_audio::SOURCE_RATE,
        "movie audio must use the native playback rate"
    );
    drop(movie);
    resonance_media::MovieDecoder::frame(&temporary, 0)?;
    fs::rename(temporary, destination)?;
    Ok(())
}

fn bind_fields(library: &Path, movies: &Mappings) -> Result<()> {
    let fields: Mappings = serde_json::from_slice(&fs::read(library.join(FIELDS))?)?;
    let available = movies.iter().map(|(id, _)| id).collect::<BTreeSet<_>>();
    for (_, name) in fields.iter() {
        let path = library.join(format!("fields/{name}/field.json"));
        let mut definition: Definition = serde_json::from_slice(&fs::read(&path)?)?;
        let bytes = fs::read(library.join(&definition.script))?;
        let analysis = scenario::analyze(&bytes)?;
        if !analysis.instructions.values().any(|instruction| {
            instruction.mnemonic == "proc"
                && instruction.operands == [i64::from(NativeCall::PlayMovieBlocking as u8)]
        }) {
            if !definition.movies.is_empty() {
                definition.movies.clear();
                output::json(&path, &definition)?;
            }
            continue;
        }
        let program = Program::decode(&bytes)?;
        let args = scenario::calls::analyzed_arguments(
            &program,
            &analysis,
            NativeCall::PlayMovieBlocking,
            1,
        )?;
        let mut selected = BTreeSet::new();
        for args in args.into_values() {
            match args[0] {
                Some(id) if id >= 0 && available.contains(&(id as u32)) => {
                    selected.insert(id as u32);
                }
                None => selected.extend(&available),
                _ => {}
            }
        }
        if definition.movies != selected {
            definition.movies = selected;
            output::json(&path, &definition)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_media::MovieDecoder;
    use std::{
        path::PathBuf,
        time::{Duration, Instant},
    };

    fn audio_tracks(path: &Path) -> Result<serde_json::Value> {
        let output = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "a",
                "-show_entries",
                "stream=sample_rate,channels,duration",
                "-of",
                "json",
            ])
            .arg(path)
            .output()?;
        ensure!(
            output.status.success(),
            "ffprobe failed: {}",
            path.display()
        );
        Ok(serde_json::from_slice(&output.stdout)?)
    }

    #[test]
    #[ignore = "requires installed Steam and FFmpeg9; private output only, no full cook or device"]
    fn installed_colette_movie_normalizes_native_audio_and_preserves_video() -> Result<()> {
        let root = PathBuf::from(
            std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
        );
        let source = root.join("P12E/Data/TOS_Server/Rawdata/WIN/MOVIE/TOS_PS2_AS01_720P.USM");
        let temporary = tempfile::tempdir()?;
        let destination = std::env::var_os("RESONANCE_MOVIE_TEST_OUTPUT")
            .map(PathBuf::from)
            .unwrap_or_else(|| temporary.path().join("colette-angel-transformation.mkv"));
        ensure!(!destination.exists(), "private movie output must be new");
        convert(&source, &destination, false)?;
        let original = audio_tracks(&source)?;
        let cooked = audio_tracks(&destination)?;
        let original = original["streams"]
            .as_array()
            .context("original audio tracks")?;
        let cooked = cooked["streams"]
            .as_array()
            .context("cooked audio tracks")?;
        assert!(!original.is_empty());
        assert_eq!(
            cooked.len(),
            original.len(),
            "preserve all source language tracks"
        );
        for (index, (before, after)) in original.iter().zip(cooked).enumerate() {
            assert_eq!(
                after["sample_rate"],
                resonance_audio::SOURCE_RATE.to_string()
            );
            assert_eq!(after["channels"], 2);
            let decoder = MovieDecoder::open(&destination, index as u16)?;
            assert_eq!(decoder.format().sample_rate, resonance_audio::SOURCE_RATE);
            let started = Instant::now();
            let mut frames = 0u64;
            while !decoder.audio_complete() {
                if let Some(chunk) = decoder.try_audio()? {
                    assert_eq!(chunk.start_frame, frames);
                    assert!(chunk.samples.len().is_multiple_of(2));
                    frames += chunk.samples.len() as u64 / 2;
                } else {
                    ensure!(
                        started.elapsed() < Duration::from_secs(30),
                        "audio decode timed out"
                    );
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
            let source_seconds = before["duration"]
                .as_str()
                .context("source audio duration")?
                .parse::<f64>()?;
            assert!(
                (frames as f64 / f64::from(resonance_audio::SOURCE_RATE) - source_seconds).abs()
                    < 0.001
            );
            eprintln!("track{index}: native stereo, {frames} decoded frames");
        }
        let library = PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY")
                .context("set RESONANCE_TEST_LIBRARY to current Steam104 for video comparison")?,
        );
        let mappings: Mappings = serde_json::from_slice(&fs::read(library.join(MOVIES))?)?;
        let baseline = library.join("movies").join(mappings.target(11)?);
        let previous = MovieDecoder::open(&baseline, 0)?;
        let normalized = MovieDecoder::open(&destination, 0)?;
        assert_eq!(previous.video_timestamps()?, normalized.video_timestamps()?);
        assert_eq!(
            previous.format().frame_duration,
            normalized.format().frame_duration
        );
        assert_eq!(
            MovieDecoder::frame(&baseline, 3)?.rgba,
            MovieDecoder::frame(&destination, 3)?.rgba
        );
        eprintln!("private normalized movie: {}", destination.display());
        Ok(())
    }
}
