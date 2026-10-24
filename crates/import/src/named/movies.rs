use super::*;
use resonance_content::movie::Definition;
use std::{io::Read, path::PathBuf};

impl Publisher<'_> {
    /// Convert physical sources one at a time, bounding full-frame scratch space.
    /// Repeated declarations of a movie share one named resource.
    pub fn movies(&mut self, discs: &[PathBuf]) -> Result<()> {
        let _publications = crate::publication::Session::start_if_needed(self.output)?;
        let mut discs = discs
            .iter()
            .map(|path| Ok((crate::disc_number(path)?, path)))
            .collect::<Result<Vec<_>>>()?;
        discs.sort_by_key(|(disc, _)| *disc);
        for (_, extracted) in &discs {
            let files = extracted.join("files");
            let mut pending = vec![files.clone()];
            while let Some(directory) = pending.pop() {
                let mut entries = fs::read_dir(directory)?.collect::<std::io::Result<Vec<_>>>()?;
                entries.sort_by_key(|entry| entry.file_name());
                for entry in entries {
                    let kind = entry.file_type()?;
                    if kind.is_dir() {
                        pending.push(entry.path());
                    } else if kind.is_file() {
                        let mut signature = [0; 16];
                        let mut file = fs::File::open(entry.path())?;
                        if file.metadata()?.len() >= signature.len() as u64 {
                            file.read_exact(&mut signature)?;
                            if crate::media::is_movie(&signature) {
                                let path = entry.path();
                                let source = path
                                    .strip_prefix(&files)?
                                    .to_str()
                                    .context("movie source is not UTF-8")?;
                                self.movie(extracted, source)?;
                            }
                        }
                    }
                }
            }
        }
        let paths = discs
            .iter()
            .map(|(_, path)| (*path).clone())
            .collect::<Vec<_>>();
        self.movie_catalogue(&paths)
    }

    /// Publish directly from the native movie. No old cooked metadata is written
    /// or loaded, and unrelated source owners never share editable files.
    pub(crate) fn movie(&mut self, extracted: &Path, source: &str) -> Result<Vec<String>> {
        let name = movie_name(source)?;
        let path = format!("movies/{name}/video.mkv");
        self.check_spelling(&path)?;
        let destination = self.output.join(&path);
        fs::create_dir_all(destination.parent().context("movie has no owner")?)?;
        let identity = crate::media::movie_input_identity(extracted, source)?;
        let entry = format!("movies/{name}/movie.json");
        crate::publication::produce_once(&destination, &identity, || {
            ensure!(
                !destination.exists(),
                "movie destination already exists: {path}"
            );
            let converted =
                crate::media::cook_movie_file(extracted, Path::new(source), &destination)?;
            self.remember(&path, converted.sha256)?;
            // A silent movie remains usable by VideoReader. Runtime movie IDs
            // must select a soundtrack, checked when binding the catalogue.
            if !converted.audio_tracks.is_empty() {
                self.json(
                    &entry,
                    &Definition {
                        version: 1,
                        path: path.clone(),
                        audio_track: 0,
                    },
                )?;
            }
            Ok(())
        })?;
        let mut paths = vec![path.clone()];
        if self.output.join(&entry).is_file() {
            paths.push(entry);
        }
        Ok(paths)
    }

    pub(crate) fn movie_catalogue(&mut self, discs: &[PathBuf]) -> Result<()> {
        let mut mappings = BTreeMap::new();
        for (id, (disc, source)) in crate::media::movie_catalogue(discs)? {
            let name = movie_name(&source)?;
            ensure!(
                self.output
                    .join(format!("movies/{name}/movie.json"))
                    .is_file(),
                "movie {id} has no converted playable source: disc{disc}/{source}"
            );
            mappings.insert(id, name);
        }
        self.json(library::MOVIES, &Mappings::new(mappings.clone())?)?;
        self.mappings.insert(library::MOVIES.into(), mappings);
        Ok(())
    }
}

fn movie_name(source: &str) -> Result<String> {
    Ok(match source.to_ascii_lowercase().as_str() {
        "mov/op.h4m" => "opening".to_owned(),
        "mov/s01.h4m" => "story-introduction".to_owned(),
        _ => super::audio::source_name(source)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::library::Library;

    #[test]
    #[ignore = "requires both extracted discs; converts every physical movie"]
    fn original_movie_catalogue_keeps_ids_streams_and_track_choices() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let discs = [local.join("extracted/disc2"), local.join("extracted/disc1")];
        let source = tempfile::tempdir()?;
        let output = tempfile::tempdir()?;
        let mut publisher = Publisher::new(source.path(), output.path())?;
        publisher.movies(&discs)?;
        publisher.descriptor("GQSEAF", 0)?;
        let library = Library::open(output.path(), &[])?;
        let mut snapshot = library.snapshot();
        let mappings = snapshot.mappings(library::MOVIES)?.clone();
        let original = crate::media::movie_catalogue(&discs)?;
        assert_eq!(
            original.keys().copied().collect::<BTreeSet<_>>(),
            mappings.iter().map(|(id, _)| id).collect()
        );
        assert!(output.path().join("movies/opening/video.mkv").is_file());
        for (id, (disc, source)) in original {
            let entry = snapshot.movie(id)?;
            let definition: Definition = snapshot.json(&entry, 64 * 1024)?;
            assert_eq!(definition.audio_track, 0);
            assert_eq!(
                definition.path,
                format!("movies/{}/video.mkv", movie_name(&source)?)
            );
            let mut header = [0; 68];
            fs::File::open(local.join(format!("extracted/disc{disc}/files/{source}")))?
                .read_exact(&mut header)?;
            let decoder = resonance_media::MovieDecoder::load(&mut snapshot, id)?;
            let format = decoder.format();
            assert_eq!(
                format.width,
                u32::from(u16::from_be_bytes(header[0x34..0x36].try_into()?))
            );
            assert_eq!(
                format.height,
                u32::from(u16::from_be_bytes(header[0x36..0x38].try_into()?))
            );
            assert_eq!(
                format.sample_rate,
                u32::from_be_bytes(header[0x40..0x44].try_into()?)
            );
            assert_eq!(
                format.frame_duration,
                std::time::Duration::from_micros(u64::from(u32::from_be_bytes(
                    header[0x24..0x28].try_into()?
                )))
            );
        }
        Ok(())
    }
}
