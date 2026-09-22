use super::*;
use crate::{cooked::Source, dol};
use resonance_content::{MovieAsset, movie::metadata_path};
use std::collections::BTreeMap;

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

/// Bind every available movie ID to its shared media, including catalogue aliases.
/// The earliest disc supplies overlapping versions; argument order is immaterial.
pub(crate) fn bind_all_movies(
    discs: &[PathBuf],
    output: &Path,
    publications: &BTreeMap<String, Vec<String>>,
) -> Result<Vec<String>> {
    let _publications = crate::publication::Session::start_if_needed(output)?;
    let mut sources = discs
        .iter()
        .map(|path| Ok((crate::disc_number(path)?, path)))
        .collect::<Result<Vec<_>>>()?;
    sources.sort_by_key(|(disc, _)| *disc);
    let mut declarations = BTreeMap::new();
    for (disc, extracted) in sources {
        for (id, path) in directory(&fs::read(extracted.join("sys/main.dol"))?)?
            .into_iter()
            .enumerate()
        {
            if let Some(path) = path
                && let Some(path) =
                    crate::field_resources::find_path(&extracted.join("files"), &path)?
            {
                declarations.entry(id as u32).or_insert((disc, path));
            }
        }
    }
    let mut movies = BTreeMap::new();
    let mut paths = Vec::new();
    for (id, source) in declarations {
        let movie = match movies.entry(source) {
            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::btree_map::Entry::Vacant(entry) => {
                let (disc, path) = entry.key();
                let movie = bind_movie(output, publications, *disc, path, 0)?;
                entry.insert(movie)
            }
        };
        let path = metadata_path(id);
        write_json(&output.join(&path), &json!(movie))?;
        paths.push(path);
    }
    Ok(paths)
}

fn bind_movie(
    output: &Path,
    publications: &BTreeMap<String, Vec<String>>,
    disc: u8,
    path: &str,
    audio_track: u16,
) -> Result<MovieAsset> {
    let source = Source::new(output, publications, disc, path)?;
    let (directories, bytes) = source.candidates("movie.json")?;
    let cooked: CookedMovie = serde_json::from_slice(&bytes)?;
    ensure!(
        cooked.version == 1
            && cooked
                .audio_tracks
                .iter()
                .enumerate()
                .all(|(index, track)| usize::from(track.stream) == index + 1),
        "invalid cooked movie descriptor"
    );
    source.verify_file(&directories, &cooked.path)?;
    select_track(directories[0], &cooked, audio_track)
}

/// Physical conversion validates the source and payload; binding only selects metadata.
fn select_track(directory: &str, cooked: &CookedMovie, audio_track: u16) -> Result<MovieAsset> {
    resonance_content::validate_asset_path(directory)?;
    resonance_content::validate_asset_path(&cooked.path)?;
    let audio = cooked
        .audio_tracks
        .get(usize::from(audio_track))
        .context("selected movie audio track is unavailable")?;
    let movie = MovieAsset {
        version: 2,
        path: format!("{directory}/{}", cooked.path),
        sha256: cooked.sha256.clone(),
        width: cooked.width,
        height: cooked.height,
        frames: cooked.frames,
        frame_micros: cooked.frame_micros,
        sample_rate: audio.sample_rate,
        channels: audio.channels,
        audio_frames: u64::from(audio.frames),
        audio_track,
    };
    movie.validate()?;
    Ok(movie)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn disc(root: &Path, disc: u8, declarations: &[(usize, &str)]) -> Result<PathBuf> {
        let extracted = root.join(format!("source-{disc}"));
        fs::create_dir_all(extracted.join("sys"))?;
        let mut boot = *b"GQSEAF\0\0";
        boot[6] = disc - 1;
        fs::write(extracted.join("sys/boot.bin"), boot)?;
        let mut executable = vec![0u8; 0x400];
        for (at, word) in [(0, 0x100u32), (0x48, MOVIES), (0x90, 0x300)] {
            executable[at..at + 4].copy_from_slice(&word.to_be_bytes());
        }
        let mut offset = 0x150;
        for &(id, path) in declarations {
            let at = 0x100 + id * 4;
            executable[at..at + 4].copy_from_slice(&(MOVIES + offset as u32 - 0x100).to_be_bytes());
            executable[offset..offset + path.len()].copy_from_slice(path.as_bytes());
            offset += path.len() + 1;
            let source = extracted.join("files").join(path);
            fs::create_dir_all(source.parent().unwrap())?;
            // Binding must not attempt to decode these already-converted sources.
            fs::write(source, b"source existence only")?;
        }
        fs::write(extracted.join("sys/main.dol"), executable)?;
        Ok(extracted)
    }

    #[test]
    fn catalogue_binds_aliases_disc_variants_and_playable_tracks_without_decoding() -> Result<()> {
        let root = crate::temporary_path(&std::env::temp_dir().join("movie-binding"));
        let result = (|| -> Result<()> {
            let first = disc(&root, 1, &[(0, "Art/Renamed.bin"), (3, "Art/Renamed.bin")])?;
            let second = disc(&root, 2, &[(0, "Art/Renamed.bin"), (2, "Art/Renamed.bin")])?;
            let mut sources = BTreeMap::new();
            for (disc, directory) in [(1, "assets/first"), (2, "assets/second")] {
                let path = "Art/Renamed.bin";
                let movie = CookedMovie {
                    version: 1,
                    source: path.into(),
                    source_sha256: crate::digest(b"source existence only"),
                    path: "movie.mkv".into(),
                    sha256: crate::digest(b"shared payload"),
                    width: 8,
                    height: 8,
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
                fs::create_dir_all(root.join(directory))?;
                fs::write(root.join(directory).join("movie.mkv"), b"shared payload")?;
                write_json(&root.join(directory).join("movie.json"), &json!(movie))?;
                sources.insert(format!("disc{disc}/{path}"), vec![directory.to_owned()]);
            }
            assert_eq!(
                bind_all_movies(&[second, first], &root, &sources)?,
                [0, 2, 3].map(metadata_path)
            );
            assert!(!root.join("sources.json").exists());
            for id in [0, 3] {
                let movie: MovieAsset =
                    serde_json::from_slice(&fs::read(root.join(metadata_path(id)))?)?;
                assert_eq!(movie.path, "assets/first/movie.mkv");
                assert_eq!(movie.audio_track, 0);
            }
            let second: MovieAsset =
                serde_json::from_slice(&fs::read(root.join(metadata_path(2)))?)?;
            assert_eq!(second.path, "assets/second/movie.mkv");
            assert_eq!(
                bind_movie(&root, &sources, 2, "Art/Renamed.bin", 1)?.channels,
                2
            );
            assert!(bind_movie(&root, &sources, 1, "Art/Renamed.bin", 2).is_err());
            let mut unsupported: CookedMovie =
                serde_json::from_slice(&fs::read(root.join("assets/second/movie.json"))?)?;
            unsupported.audio_tracks[0].channels = 1;
            assert!(select_track("assets/second", &unsupported, 0).is_err());
            unsupported.audio_tracks.clear();
            assert!(select_track("assets/second", &unsupported, 0).is_err());
            assert!(!root.join("intermediate").exists());
            fs::remove_file(root.join("assets/first/movie.mkv"))?;
            assert!(bind_movie(&root, &sources, 1, "Art/Renamed.bin", 0).is_err());
            Ok(())
        })();
        fs::remove_dir_all(root)?;
        result
    }

    #[test]
    #[ignore = "requires both extracted discs and cook-all; no codecs or devices"]
    fn original_movie_directory_and_all_shared_tracks() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let output = std::env::var_os("RESONANCE_COOKED")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| local.join("all-assets"));
        let sources: BTreeMap<String, Vec<String>> =
            serde_json::from_slice(&fs::read(output.join("sources.json"))?)?;
        let mut physical = BTreeSet::new();
        let mut opening = Vec::new();
        for disc in [1, 2] {
            let extracted = local.join(format!("extracted/disc{disc}"));
            let declared = directory(&fs::read(extracted.join("sys/main.dol"))?)?;
            let cooked: Vec<Option<String>> =
                Source::open(&output, disc, "sys/main.dol")?.document("embedded/movies.json")?;
            assert_eq!(cooked, declared);
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
                for track in 0..header.audio_streams.max(1) {
                    let asset = bind_movie(&output, &sources, disc, &path, track)?;
                    assert_eq!(asset.audio_track, track);
                    assert_eq!(asset.frames, header.frames);
                    assert_eq!(
                        (asset.width as usize, asset.height as usize),
                        (header.width, header.height)
                    );
                    assert_eq!(
                        (asset.channels, asset.sample_rate),
                        (header.channels, header.sample_rate)
                    );
                    if path == "MOV/op.h4m" && track == 0 {
                        opening.push(asset.audio_frames);
                    }
                    physical.insert(asset.path);
                }
                assert!(
                    bind_movie(&output, &sources, disc, &path, header.audio_streams.max(1))
                        .is_err()
                );
            }
        }
        assert_eq!(opening, [3_879_328, 3_876_120]);
        assert_eq!(physical.len(), 11);
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
