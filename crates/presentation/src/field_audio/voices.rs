//! Weak voice views; active scenes, recent encounters and startup own the PCM.
use super::Clip;
use anyhow::Result;
use resonance_content::field_audio::{Voice as Metadata, document::Voice, speech::Source};
use std::{
    collections::BTreeMap,
    sync::{Arc, Weak},
};

type Key = (String, Option<u32>);
struct Entry {
    clip: Weak<Clip>,
    limit: usize,
}

#[derive(Default)]
pub(super) struct Cache {
    entries: BTreeMap<Key, Entry>,
    #[cfg(test)]
    hits: usize,
}

impl Cache {
    pub(super) fn prepare(
        &mut self,
        voice: &Voice,
        read: &mut impl FnMut(&str, usize) -> Result<Arc<[u8]>>,
    ) -> Result<Arc<Clip>> {
        let path = match &voice.source {
            Source::Silence { silence, .. } => {
                return Ok(Arc::new(Clip::silence(*silence, voice.sample_rate)?));
            }
            Source::Wave(path) => path,
        };
        // Inspect the current source before reapplying the old, tighter bound:
        // replacing a short recording with a longer valid WAV must stay valid.
        let bytes = read(path, 129 * 1024 * 1024)?;
        let key = (path.clone(), voice.sample_rate);
        if let Some(entry) = self.entries.get(&key)
            && let Some(clip) = entry.clip.upgrade()
            && resonance_content::same_bytes(&clip.bytes, &bytes)
        {
            read(path, entry.limit)?;
            #[cfg(test)]
            {
                self.hits += 1;
            }
            return Ok(clip);
        }
        let metadata = Metadata::from_wave(path, voice.sample_rate, &bytes)?;
        let limit = metadata.frames as usize * usize::from(metadata.channels) * 2 + 1024 * 1024;
        let clip = Clip::capture(&metadata, read)?;
        {
            self.entries.insert(
                key,
                Entry {
                    clip: Arc::downgrade(&clip),
                    limit,
                },
            );
        }
        Ok(clip)
    }

    pub(super) fn prune(&mut self) {
        self.entries
            .retain(|_, entry| entry.clip.strong_count() > 0);
    }
    pub(super) fn adopt(&mut self, other: Self) {
        self.entries.extend(other.entries);
        self.prune();
    }
    #[cfg(test)]
    pub(super) fn hits(&self) -> usize {
        self.hits
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field_audio::named_tests::{fixture, wav, write};
    use resonance_content::{diagnostics::Diagnostics, library::Library, prepared::Files};
    const PATH: &str = "audio/speech/cache-test.wav";

    fn prepare(
        cache: &mut Cache,
        library: &Library,
        rate: Option<u32>,
    ) -> Result<(Arc<Clip>, Files)> {
        let mut snapshot = library.snapshot();
        let clip = cache.prepare(
            &Voice {
                source: Source::Wave(PATH.into()),
                sample_rate: rate,
            },
            &mut |path, limit| snapshot.read(path, limit),
        )?;
        Ok((
            clip,
            Files::from_snapshot(snapshot, || false, Diagnostics::new(true))?,
        ))
    }

    #[test]
    fn cached_speech_reuses_views_and_tracks_wave_and_clock_edits() -> Result<()> {
        let root = fixture()?;
        write(root.path(), PATH, &wav(&[1024, -1024, 2048, -2048], 32000)?)?;
        let library = Library::open(root.path(), &[])?;
        let mut cache = Cache::default();
        let (first, first_files) = prepare(&mut cache, &library, None)?;
        let (alias, alias_files) = prepare(&mut cache, &library, None)?;
        assert!(Arc::ptr_eq(&first, &alias));
        assert_eq!(cache.hits(), 1);
        assert_eq!(
            first_files.keys().collect::<Vec<_>>(),
            alias_files.keys().collect::<Vec<_>>()
        );
        let (clocked, _) = prepare(&mut cache, &library, Some(24000))?;
        assert_eq!(clocked.rate, 24000);
        assert_eq!(first.rate, 32000);
        assert!(!Arc::ptr_eq(&first, &clocked));
        write(root.path(), PATH, &wav(&[-4096, 4096, -8192, 8192], 22050)?)?;
        let (edited, _) = prepare(&mut cache, &library, None)?;
        assert!(!Arc::ptr_eq(&first, &edited));
        assert_eq!(edited.rate, 22050);
        assert!(edited.value(0) < 0 && first.value(0) > 0);
        let (clocked_edit, _) = prepare(&mut cache, &library, Some(24000))?;
        assert_eq!(clocked_edit.rate, 24000);
        assert!(!Arc::ptr_eq(&clocked, &clocked_edit));
        // This valid replacement exceeds the old recording's tight read limit.
        write(root.path(), PATH, &wav(&vec![512; 800_000], 32000)?)?;
        let (longer, _) = prepare(&mut cache, &library, None)?;
        assert_eq!(longer.sample_count(), 800_000);
        std::fs::remove_file(root.path().join(PATH))?;
        assert!(prepare(&mut cache, &library, None).is_err());
        write(root.path(), PATH, b"invalid wave")?;
        assert!(prepare(&mut cache, &library, None).is_err());
        write(root.path(), PATH, &wav(&[1024, -1024], 32000)?)?;
        assert!(prepare(&mut cache, &library, Some(0)).is_err());
        Ok(())
    }
}
