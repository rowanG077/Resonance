use super::*;
use crate::{cooked::Source, dol};

#[derive(Clone, Copy)]
pub enum MovieSource {
    Opening,
    StoryIntroduction,
}
impl MovieSource {
    fn identity(self) -> (usize, &'static str) {
        match self {
            Self::Opening => (0, "intro"),
            Self::StoryIntroduction => (1, "story-intro"),
        }
    }
}

const MOVIES: u32 = 0x801f9c80;

fn directory(executable: &[u8]) -> Result<Vec<Option<String>>> {
    dol::slice(executable, MOVIES, 19 * 4)?
        .chunks_exact(4)
        .map(|row| dol::optional_text(executable, be_u32(row, 0)?))
        .collect()
}

pub(crate) fn cook_directory(executable: &[u8], output: &Path) -> Result<Vec<String>> {
    let path = "embedded/movies.json";
    write_json(&output.join(path), &json!(directory(executable)?))?;
    Ok(vec![path.into()])
}

/// Bind a runtime movie to the shared library without decoding or copying media.
pub fn cook_movie(
    extracted: &Path,
    output: &Path,
    audio_stream: u16,
    source: MovieSource,
) -> Result<()> {
    let (id, name) = source.identity();
    let workspace = Workspace::open(extracted, output)?;
    let declarations: Vec<Option<String>> =
        Source::open(&workspace.output, workspace.disc, "sys/main.dol")?
            .document("embedded/movies.json")?;
    let path = crate::all_assets::roles::declared_path(
        &workspace.extracted.join("files"),
        declarations
            .get(id)
            .and_then(Option::as_deref)
            .context("missing movie declaration")?,
    )?;
    let movie = bind_movie(&workspace.output, workspace.disc, &path, audio_stream)?;
    write_json(
        &workspace.output.join(format!("{name}.json")),
        &json!(movie),
    )?;
    if matches!(source, MovieSource::StoryIntroduction) {
        crate::field::refresh_preloads(&workspace.output)?;
    }
    Ok(())
}

fn bind_movie(
    output: &Path,
    disc: u8,
    path: &str,
    audio_stream: u16,
) -> Result<resonance_content::MovieAsset> {
    let source = Source::open(output, disc, path)?;
    let (directories, bytes) = source.candidates("movie.json")?;
    let cooked: CookedMovie = serde_json::from_slice(&bytes)?;
    ensure!(cooked.version == 1, "unsupported physical movie recipe");
    let (audio_track, track) = cooked
        .audio_tracks
        .iter()
        .enumerate()
        .find(|(_, track)| track.stream == audio_stream)
        .context("selected movie audio stream is unavailable")?;
    source.verify_file(&directories, &cooked.path)?;
    let movie = resonance_content::MovieAsset {
        version: 2,
        path: format!("{}/{}", directories[0], cooked.path),
        sha256: cooked.sha256,
        width: cooked.width,
        height: cooked.height,
        frames: cooked.frames,
        frame_micros: cooked.frame_micros,
        sample_rate: track.sample_rate,
        channels: track.channels,
        audio_frames: u64::from(track.frames),
        audio_track: u16::try_from(audio_track)?,
    };
    movie.validate()?;
    ensure!(
        hash_file(&output.join(&movie.path))? == movie.sha256,
        "cooked movie content changed; rerun cook-all"
    );
    Ok(movie)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    #[test]
    fn preparation_follows_declarations_without_original_media_or_codecs() -> Result<()> {
        let root = crate::temporary_path(&std::env::temp_dir().join("movie-binding"));
        let extracted = root.join("extracted");
        let output = root.join("cooked");
        let result = (|| -> Result<()> {
            fs::create_dir_all(extracted.join("sys"))?;
            fs::write(extracted.join("sys/boot.bin"), b"GQSEAF\0\0")?;
            fs::create_dir_all(extracted.join("files/Art"))?;
            fs::write(extracted.join("files/Art/Renamed.bin"), [])?;
            let directory = "assets/example";
            fs::create_dir_all(output.join(directory))?;
            let bytes = b"shared movie payload";
            fs::write(output.join(directory).join("movie.mkv"), bytes)?;
            let mut movie = CookedMovie {
                version: 1,
                source: "previous-name.h4m".into(),
                source_sha256: "0".repeat(64),
                path: "movie.mkv".into(),
                sha256: crate::digest(bytes),
                width: 2,
                height: 2,
                frames: 25,
                frame_micros: 40_000,
                audio_tracks: (1..=2)
                    .map(|stream| MovieAudioTrack {
                        stream,
                        channels: 2,
                        sample_rate: 32_000,
                        frames: 32_000,
                    })
                    .collect(),
            };
            let metadata = output.join(directory).join("movie.json");
            write_json(&metadata, &json!(movie))?;
            write_json(
                &output.join("embedded/movies.json"),
                &json!(["art/renamed.bin"]),
            )?;
            write_json(
                &output.join("sources.json"),
                &json!({
                    "disc1/sys/main.dol": ["embedded/movies.json"],
                    "disc1/Art/Renamed.bin": [directory],
                }),
            )?;
            for stream in [1, 2] {
                cook_movie(&extracted, &output, stream, MovieSource::Opening)?;
                let asset: resonance_content::MovieAsset =
                    serde_json::from_slice(&fs::read(output.join("intro.json"))?)?;
                assert_eq!(asset.audio_track, stream - 1);
                assert_eq!(asset.path, format!("{directory}/movie.mkv"));
            }
            assert!(!output.join("movies").exists() && !output.join("intermediate").exists());
            assert!(cook_movie(&extracted, &output, 3, MovieSource::Opening).is_err());
            assert!(cook_movie(&extracted, &output, 1, MovieSource::StoryIntroduction).is_err());
            fs::write(output.join(directory).join("movie.mkv"), b"changed")?;
            assert!(cook_movie(&extracted, &output, 1, MovieSource::Opening).is_err());
            movie.path = "../movie.mkv".into();
            write_json(&metadata, &json!(movie))?;
            assert!(cook_movie(&extracted, &output, 1, MovieSource::Opening).is_err());
            Ok(())
        })();
        fs::remove_dir_all(root)?;
        result
    }

    #[test]
    #[ignore = "requires both extracted discs and cook-all; no codecs or devices"]
    fn original_movie_directory_and_all_shared_tracks() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let output = local.join("all-assets");
        let mut physical = BTreeSet::new();
        let mut opening = Vec::new();
        for disc in [1, 2] {
            let extracted = local.join(format!("extracted/disc{disc}"));
            let executable = fs::read(extracted.join("sys/main.dol"))?;
            let declared = directory(&executable)?;
            let cooked: Vec<Option<String>> =
                Source::open(&output, disc, "sys/main.dol")?.document("embedded/movies.json")?;
            assert_eq!(cooked, declared);
            assert_eq!(declared.len(), 19);
            for (left, right) in [(0, 9), (0, 11), (1, 12), (16, 17)] {
                assert_eq!(declared[left], declared[right]);
            }
            let mut bound = BTreeSet::new();
            for path in declared.into_iter().flatten() {
                let Some(path) =
                    crate::field_resources::find_path(&extracted.join("files"), &path)?
                else {
                    continue;
                };
                if !bound.insert(path.clone()) {
                    continue;
                }
                let mut bytes = [0; 68];
                fs::File::open(extracted.join("files").join(&path))?.read_exact(&mut bytes)?;
                let header = Header::parse(&bytes)?;
                for stream in 1..=header.audio_streams {
                    let asset = bind_movie(&output, disc, &path, stream)?;
                    assert_eq!(asset.audio_track, stream - 1);
                    assert_eq!(asset.frames, header.frames);
                    assert_eq!(
                        (asset.width as usize, asset.height as usize),
                        (header.width, header.height)
                    );
                    assert_eq!(
                        (asset.channels, asset.sample_rate),
                        (header.channels, header.sample_rate)
                    );
                    if path == "MOV/op.h4m" && stream == 1 {
                        opening.push(asset.audio_frames);
                    }
                    physical.insert(asset.path);
                }
                assert!(bind_movie(&output, disc, &path, header.audio_streams + 1).is_err());
            }
        }
        assert_eq!(opening, [3_879_328, 3_876_120]);
        assert_eq!(physical.len(), 11);
        // Every physical movie is reachable; absent declarations remain in the table.
        let sources: BTreeMap<String, Vec<String>> =
            serde_json::from_slice(&fs::read(output.join("sources.json"))?)?;
        let installed: BTreeSet<_> = sources
            .values()
            .flatten()
            .filter(|path| output.join(path).join("movie.json").is_file())
            .map(|path| format!("{path}/movie.mkv"))
            .collect();
        assert_eq!(physical, installed);
        Ok(())
    }
}
