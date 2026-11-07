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

pub(crate) fn cook(source: &Source, library: &Path) -> Result<()> {
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
    let voiced = [1, 8]
        .into_iter()
        .filter_map(|id| sources.get(&id).map(|(name, _)| name.clone()))
        .collect::<BTreeSet<_>>();
    let mut written = BTreeSet::new();
    let mut mappings = BTreeMap::new();
    for (&id, (source_name, input)) in &sources {
        let name = name(source_name)?;
        let target = format!("movies/{name}.mkv");
        if written.insert(source_name) {
            eprintln!("  Movie: {name} ({source_name})");
            convert(input, &library.join(&target), voiced.contains(source_name))?;
        }
        mappings.insert(id, format!("{name}.mkv"));
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
