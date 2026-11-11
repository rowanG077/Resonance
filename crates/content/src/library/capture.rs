//! Independent preparation views over one transition's captured namespace.
use super::*;
use std::sync::Mutex;

pub struct Capture(Arc<Mutex<Snapshot>>);

impl Snapshot {
    pub fn concurrent(self) -> Capture {
        Capture(Arc::new(Mutex::new(self)))
    }

    pub(super) fn observe_shared(
        &mut self,
        path: &str,
        limit: usize,
        optional: bool,
    ) -> Result<Option<Arc<[u8]>>> {
        validate_path(path)?;
        let source = self.source.as_ref().unwrap();
        let path_lock = source
            .lock()
            .unwrap()
            .read_locks
            .entry(path.to_owned())
            .or_default()
            .clone();
        let _reading = path_lock.lock().unwrap();
        let mut reader = {
            let source = source.lock().unwrap();
            let mut reader = source.library.snapshot();
            reader.source = source.source.clone();
            reader.common = source.common.clone();
            reader.reused = source.reused.clone();
            if let Some(bytes) = source.bytes.get(path) {
                reader.bytes.insert(path.to_owned(), bytes.clone());
                reader.overrides.extend(
                    source
                        .overrides
                        .iter()
                        .filter(|row| row.path == path)
                        .cloned(),
                );
            }
            if source.missing.contains(path) {
                reader.missing.insert(path.to_owned());
            }
            reader
        };
        // Slow storage and the injected I/O delay must not hold the namespace
        // mutex. A path lock still admits exactly one first reader per file.
        let bytes = if optional {
            reader.read_optional(path, limit)?
        } else {
            Some(reader.read(path, limit)?)
        };
        let disk_bytes = reader.disk_bytes;
        let overrides = reader.overrides;
        {
            let mut source = source.lock().unwrap();
            source.disk_bytes += disk_bytes;
            if let Some(bytes) = &bytes {
                if source
                    .bytes
                    .insert(path.to_owned(), bytes.clone())
                    .is_none()
                {
                    source.overrides.extend(overrides.clone());
                }
            } else {
                source.missing.insert(path.to_owned());
            }
        }
        self.disk_bytes += disk_bytes;
        self.overrides.extend(overrides);
        match &bytes {
            Some(bytes) => {
                self.bytes.insert(path.to_owned(), bytes.clone());
            }
            None => {
                self.missing.insert(path.to_owned());
            }
        }
        Ok(bytes)
    }
}

impl Capture {
    pub fn view(&self) -> Snapshot {
        let source = self.0.lock().unwrap();
        let mut snapshot = source.library.snapshot();
        snapshot.common = source.common.clone();
        snapshot.source = Some(self.0.clone());
        snapshot
    }

    /// All preparation workers must have returned and dropped their views.
    /// The result owns their union, including missing paths and stream handles.
    pub fn finish(self) -> Result<Snapshot> {
        let source = Arc::try_unwrap(self.0)
            .map_err(|_| anyhow::anyhow!("preparation views are still running"))?;
        let mut snapshot = source
            .into_inner()
            .map_err(|_| anyhow::anyhow!("preparation snapshot failed"))?;
        snapshot.read_locks.clear();
        // Worker scheduling cannot change the diagnostic order. Stable sorting
        // preserves each path's original mod override chain.
        snapshot.overrides.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn library() -> Result<tempfile::TempDir> {
        let dir = tempfile::tempdir()?;
        fs::write(
            dir.path().join(DESCRIPTOR),
            serde_json::to_vec(&Descriptor {
                version: VERSION,
                game: GAME.into(),
            })?,
        )?;
        Ok(dir)
    }

    #[test]
    fn concurrent_views_share_first_reads_and_finish_with_the_complete_union() -> Result<()> {
        let dir = library()?;
        fs::write(dir.path().join("first.bin"), b"before")?;
        fs::write(dir.path().join("second.bin"), b"second")?;
        let library = Library::open(dir.path(), &[])?;
        let capture = library.snapshot().concurrent();
        let mut first = capture.view();
        let original = first.read("first.bin", 64)?;
        fs::write(dir.path().join("first.bin"), b"after")?;
        let (same, second) = std::thread::scope(|scope| -> Result<_> {
            let mut view = capture.view();
            let left = scope.spawn(move || view.read("first.bin", 64));
            let mut view = capture.view();
            let right = scope.spawn(move || view.read("second.bin", 64));
            Ok((left.join().unwrap()?, right.join().unwrap()?))
        })?;
        assert!(Arc::ptr_eq(&original, &same));
        assert_eq!(second.as_ref(), b"second");
        assert!(first.read("first.bin", 2).is_err());
        assert!(!first.contains_key("second.bin"));
        drop(first);
        let snapshot = capture.finish()?;
        assert_eq!(snapshot.disk_bytes, 12);
        assert_eq!(snapshot.bytes.len(), 2);
        assert_eq!(snapshot.bytes["first.bin"].as_ref(), b"before");
        assert_eq!(library.snapshot().read("first.bin", 64)?.as_ref(), b"after");
        Ok(())
    }

    #[test]
    fn simultaneous_reads_capture_each_path_once_and_merge_all_workers() -> Result<()> {
        let dir = library()?;
        let payload = vec![7; 256 * 1024];
        fs::write(dir.path().join("shared.bin"), &payload)?;
        for worker in 0..4 {
            fs::write(dir.path().join(format!("worker-{worker}.bin")), &payload)?;
        }
        let library = Library::open(dir.path(), &[])?;
        let capture = library.snapshot().concurrent();
        let barrier = std::sync::Barrier::new(4);
        let results = std::thread::scope(|scope| -> Result<Vec<_>> {
            let workers = (0..4)
                .map(|worker| {
                    let mut view = capture.view();
                    let barrier = &barrier;
                    scope.spawn(move || -> Result<_> {
                        barrier.wait();
                        let shared = view.read("shared.bin", 256 * 1024)?;
                        let own = format!("worker-{worker}.bin");
                        view.read(&own, 256 * 1024)?;
                        assert_eq!(view.bytes.len(), 2);
                        assert!(view.read("shared.bin", 1).is_err());
                        Ok(shared)
                    })
                })
                .collect::<Vec<_>>();
            workers
                .into_iter()
                .map(|worker| worker.join().unwrap())
                .collect()
        })?;
        assert!(results.iter().all(|bytes| Arc::ptr_eq(bytes, &results[0])));
        let mut snapshot = capture.finish()?;
        assert_eq!(snapshot.bytes.len(), 5);
        assert_eq!(snapshot.disk_bytes, 5 * payload.len() as u64);
        assert!(snapshot.read_locks.is_empty());
        fs::write(dir.path().join("shared.bin"), b"changed")?;
        assert_eq!(
            snapshot.read("shared.bin", payload.len())?.as_ref(),
            payload
        );
        assert_eq!(
            library.snapshot().read("shared.bin", 64)?.as_ref(),
            b"changed"
        );
        Ok(())
    }

    #[test]
    fn missing_optional_paths_and_stream_selection_are_consistent_between_workers() -> Result<()> {
        let dir = library()?;
        fs::write(dir.path().join("stream.bin"), b"original")?;
        let library = Library::open(dir.path(), &[])?;
        let capture = library.snapshot().concurrent();
        let mut first = capture.view();
        assert!(first.read_optional("optional.bin", 64)?.is_none());
        let mut first_stream = StreamReader::new(first.open_stream("stream.bin")?)?;
        fs::write(dir.path().join("optional.bin"), b"new")?;
        fs::write(dir.path().join("replacement.bin"), b"replacement")?;
        fs::rename(
            dir.path().join("replacement.bin"),
            dir.path().join("stream.bin"),
        )?;
        let mut second = capture.view();
        assert!(second.read_optional("optional.bin", 64)?.is_none());
        assert!(second.read("optional.bin", 64).is_err());
        let mut second_stream = StreamReader::new(second.open_stream("stream.bin")?)?;
        let mut a = String::new();
        let mut b = String::new();
        first_stream.read_to_string(&mut a)?;
        second_stream.read_to_string(&mut b)?;
        assert_eq!(a, "original");
        assert_eq!(a, b);
        drop(first);
        drop(second);
        let snapshot = capture.finish()?;
        assert!(snapshot.missing.contains("optional.bin"));
        assert!(snapshot.streams.contains_key("stream.bin"));
        assert_eq!(
            Library::open(dir.path(), &[])?
                .snapshot()
                .read_optional("optional.bin", 64)?
                .unwrap()
                .as_ref(),
            b"new"
        );
        Ok(())
    }

    #[test]
    fn workers_reuse_the_same_requested_seed_bytes_without_retaining_its_owner() -> Result<()> {
        use crate::{diagnostics::Diagnostics, prepared::Files};
        let dir = library()?;
        fs::write(dir.path().join("shared.bin"), b"original")?;
        let library = Library::open(dir.path(), &[])?;
        let mut seed = library.snapshot();
        let bytes = seed.read("shared.bin", 64)?;
        let seed = Arc::new(Files::from_snapshot(
            seed,
            || false,
            Diagnostics::default(),
        )?);
        let owner = Arc::downgrade(&seed);
        let mut snapshot = library.snapshot();
        snapshot.reuse(seed.clone())?;
        let capture = snapshot.concurrent();
        fs::write(dir.path().join("shared.bin"), b"edited")?;
        let mut view = capture.view();
        assert!(Arc::ptr_eq(&bytes, &view.read("shared.bin", 64)?));
        assert!(
            view.reuse(seed.clone()).is_err(),
            "a view cannot choose its own seed"
        );
        drop(view);
        let output = Files::from_snapshot(capture.finish()?, || false, Diagnostics::default())?;
        drop(seed);
        assert!(owner.upgrade().is_none());
        assert_eq!(&*output.read("shared.bin")?, b"original");
        Ok(())
    }
}
