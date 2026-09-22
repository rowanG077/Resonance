//! One final writer per destination. The coordinator retains identities, never payloads.
use anyhow::{Context, Result, ensure};
use std::{
    cell::RefCell,
    collections::{BTreeMap, btree_map::Entry},
    fs,
    io::Write,
    marker::PhantomData,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, Sender},
    thread::{self, JoinHandle},
};

type Reply = Sender<std::result::Result<bool, String>>;
enum Event {
    Claim(PathBuf, String, Reply),
    Finished(PathBuf, std::result::Result<(), String>),
    Stop,
}
enum Status {
    Pending(Vec<Reply>),
    Finished(std::result::Result<(), String>),
}

#[derive(Clone)]
pub(crate) struct Client {
    root: PathBuf,
    events: Sender<Event>,
}

thread_local! {
    static CURRENT: RefCell<Option<Client>> = const { RefCell::new(None) };
}

pub(crate) fn current() -> Option<Client> {
    CURRENT.with_borrow(Clone::clone)
}

pub(crate) fn inherit(client: Option<Client>) -> Scope {
    Scope(CURRENT.replace(client), PhantomData)
}

pub(crate) struct Scope(Option<Client>, PhantomData<*mut ()>);
impl Drop for Scope {
    fn drop(&mut self) {
        CURRENT.replace(self.0.take());
    }
}

pub(crate) struct Session {
    scope: Option<Scope>,
    events: Sender<Event>,
    worker: Option<JoinHandle<()>>,
}

impl Session {
    pub(crate) fn start_if_needed(root: &Path) -> Result<Option<Self>> {
        if let Some(client) = current() {
            fs::create_dir_all(root)?;
            ensure!(
                root.canonicalize()?.starts_with(&client.root),
                "nested publication root escapes active session: {}",
                root.display()
            );
            Ok(None)
        } else {
            Self::start(root).map(Some)
        }
    }

    pub(crate) fn start(root: &Path) -> Result<Self> {
        fs::create_dir_all(root)?;
        let root = root.canonicalize()?;
        let (events, incoming) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("asset-publications".into())
            .spawn(move || coordinate(incoming))?;
        Ok(Self {
            scope: Some(inherit(Some(Client {
                root,
                events: events.clone(),
            }))),
            events,
            worker: Some(worker),
        })
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        drop(self.scope.take());
        let _ = self.events.send(Event::Stop);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn coordinate(incoming: Receiver<Event>) {
    let mut destinations = BTreeMap::<PathBuf, (String, Status)>::new();
    while let Ok(event) = incoming.recv() {
        match event {
            Event::Claim(path, hash, reply) => match destinations.entry(path) {
                Entry::Vacant(entry) => {
                    let status = if reply.send(Ok(true)).is_ok() {
                        Status::Pending(Vec::new())
                    } else {
                        Status::Finished(Err("publication owner disconnected".into()))
                    };
                    entry.insert((hash, status));
                }
                Entry::Occupied(mut entry) => {
                    let path = entry.key();
                    let (previous, status) = entry.get();
                    if *previous != hash {
                        let _ = reply.send(Err(format!(
                            "conflicting publication {}: {previous} versus {hash}",
                            path.display()
                        )));
                    } else if let Status::Finished(result) = status {
                        let _ = reply.send(result.clone().map(|()| false));
                    } else if let Status::Pending(waiters) = &mut entry.get_mut().1 {
                        waiters.push(reply);
                    }
                }
            },
            Event::Finished(path, result) => {
                if let Some((_, status)) = destinations.get_mut(&path) {
                    if let Status::Pending(waiters) = status {
                        for waiter in waiters.drain(..) {
                            let _ = waiter.send(result.clone().map(|()| false));
                        }
                    }
                    *status = Status::Finished(result);
                }
            }
            Event::Stop => break,
        }
    }
}

struct Permit {
    owner: Option<(Sender<Event>, PathBuf)>,
}

impl Permit {
    fn finish(mut self, result: &Result<()>) {
        if let Some((events, path)) = self.owner.take() {
            let _ = events.send(Event::Finished(
                path,
                result
                    .as_ref()
                    .copied()
                    .map_err(|error| format!("{error:#}")),
            ));
        }
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        if let Some((events, path)) = self.owner.take() {
            let error = format!("publication owner aborted: {}", path.display());
            let _ = events.send(Event::Finished(path, Err(error)));
        }
    }
}

impl Client {
    fn claim(&self, path: &Path, hash: &str) -> Result<Option<Permit>> {
        // Resolve existing parents, including aliases, before the final file exists.
        let path = path
            .parent()
            .context("publication has no parent")?
            .canonicalize()?
            .join(path.file_name().context("publication has no filename")?);
        ensure!(
            path.starts_with(&self.root),
            "publication escapes output root: {}",
            path.display()
        );
        match fs::symlink_metadata(&path) {
            Ok(metadata) => ensure!(
                !metadata.file_type().is_symlink(),
                "publication is a symlink: {}",
                path.display()
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let (reply, result) = mpsc::channel();
        self.events
            .send(Event::Claim(path.clone(), hash.into(), reply))?;
        let owner = result
            .recv()
            .context("publication coordinator stopped")?
            .map_err(anyhow::Error::msg)?;
        Ok(owner.then(|| Permit {
            owner: Some((self.events.clone(), path)),
        }))
    }
}

/// The owner installs synchronously with its current buffer/private file. Never
/// acquire another destination or schedule dependent work inside `install`.
pub(crate) fn publish(path: &Path, hash: &str, install: impl FnOnce() -> Result<()>) -> Result<()> {
    if let Some(client) = current() {
        if let Some(permit) = client.claim(path, hash)? {
            let result = install();
            permit.finish(&result);
            result?;
        }
    } else {
        install()?;
    }
    Ok(())
}

/// A completed final file, valid for this cooking session. No encoded payload is retained.
#[derive(Clone, Debug)]
pub(crate) struct File {
    path: PathBuf,
    hash: String,
}

impl File {
    pub(crate) fn write(path: &Path, bytes: &[u8]) -> Result<Self> {
        fs::create_dir_all(path.parent().context("path has no parent")?)?;
        let hash = crate::digest(bytes);
        publish(path, &hash, || {
            let temporary = crate::temporary_path(path);
            let result = (|| -> Result<()> {
                let mut file = fs::File::create(&temporary)?;
                file.write_all(bytes)?;
                file.sync_all()?;
                fs::rename(&temporary, path)?;
                Ok(())
            })();
            if result.is_err() {
                let _ = fs::remove_file(temporary);
            }
            result
        })?;
        Ok(Self {
            path: std::path::absolute(path)?,
            hash,
        })
    }

    /// Install another final name without invoking its encoder or asset reader.
    pub(crate) fn share(&self, destination: &Path) -> Result<Self> {
        let destination = std::path::absolute(destination)?;
        if destination != self.path {
            fs::create_dir_all(destination.parent().context("path has no parent")?)?;
            let temporary = crate::temporary_path(&destination);
            let linked =
                fs::hard_link(&self.path, &temporary).or_else(|error| match error.kind() {
                    std::io::ErrorKind::CrossesDevices
                    | std::io::ErrorKind::Unsupported
                    | std::io::ErrorKind::PermissionDenied => {
                        // Opaque final-file copying also supports filesystems without links.
                        fs::copy(&self.path, &temporary).map(|_| ())
                    }
                    _ => Err(error),
                });
            if let Err(error) = linked {
                let _ = fs::remove_file(&temporary);
                return Err(error).context("share completed publication");
            }
            install(&temporary, &destination, &self.hash)?;
        }
        Ok(Self {
            path: destination,
            hash: self.hash.clone(),
        })
    }
}

/// Install a completed streaming output without buffering it again. The caller
/// validates its format first; only its private temporary is removed on failure.
pub(crate) fn install(temporary: &Path, destination: &Path, hash: &str) -> Result<()> {
    let result = publish(destination, hash, || {
        fs::rename(temporary, destination).context("install final publication")
    });
    let cleanup = match fs::remove_file(temporary) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    };
    result.and(cleanup)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn completed_files_share_atomic_destinations_without_reencoding() -> Result<()> {
        let root = tempfile::tempdir()?;
        let _session = Session::start(root.path())?;
        let source = root.path().join("source");
        let target = root.path().join("aliases/target");
        let file = File::write(&source, b"encoded asset")?;
        file.share(&source)?;
        file.share(&target)?;
        assert_eq!(fs::read(&target)?, b"encoded asset");
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(fs::metadata(&source)?.ino(), fs::metadata(&target)?.ino());
        }
        assert!(File::write(&target, b"conflict").is_err());
        assert_eq!(fs::read_dir(target.parent().unwrap())?.count(), 1);
        Ok(())
    }

    #[test]
    fn nested_workers_share_one_writer_and_reject_conflicts() -> Result<()> {
        let root = crate::temporary_path(&std::env::temp_dir().join("publication-test"));
        let session = Session::start(&root)?;
        assert!(Session::start_if_needed(&root)?.is_none());
        let child = root.join("child");
        assert!(Session::start_if_needed(&child)?.is_none());
        fs::remove_dir(child)?;
        let writes = AtomicUsize::new(0);
        let path = root.join("shared");
        let mut dag = crate::all_assets::pool::Dag::new();
        for _ in 0..4 {
            dag.add("parent", [], |_, _| {
                let mut nested = crate::all_assets::pool::Dag::new();
                for _ in 0..4 {
                    nested.add("child", [], |_, _| {
                        publish(&path, &crate::digest(b"same"), || {
                            writes.fetch_add(1, Ordering::Relaxed);
                            fs::write(&path, b"same")?;
                            Ok(())
                        })
                    });
                }
                for result in nested.run(2, || (), |_| {})? {
                    result?;
                }
                Ok(())
            });
        }
        for result in dag.run(4, || (), |_| {})? {
            result?;
        }
        crate::write_atomic(&path, b"same")?;
        assert_eq!(writes.load(Ordering::Relaxed), 1);
        assert!(
            crate::write_atomic(&path, b"different")
                .unwrap_err()
                .to_string()
                .contains("conflicting publication")
        );
        assert_eq!(fs::read(&path)?, b"same");
        drop(session);
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn failed_owner_wakes_waiters_and_streaming_conflicts_remove_private_files() -> Result<()> {
        let root = crate::temporary_path(&std::env::temp_dir().join("publication-failure"));
        let session = Session::start(&root)?;
        let client = current().unwrap();
        let target = root.join("failed");
        let owner = client.claim(&target, "hash")?.unwrap();
        let (reply, waiter) = mpsc::channel();
        client
            .events
            .send(Event::Claim(target, "hash".into(), reply))?;
        drop(owner);
        assert!(
            waiter
                .recv_timeout(std::time::Duration::from_secs(2))?
                .unwrap_err()
                .contains("owner aborted")
        );
        let target = root.join("io-error");
        assert!(publish(&target, "hash", || anyhow::bail!("disk full")).is_err());
        assert!(
            publish(&target, "hash", || panic!("failed destination retried"))
                .unwrap_err()
                .to_string()
                .contains("disk full")
        );
        let target = root.join("movie");
        crate::write_atomic(&target, b"first")?;
        let temporary = crate::temporary_path(&target);
        fs::write(&temporary, b"first")?;
        install(&temporary, &target, &crate::digest(b"first"))?;
        assert!(!temporary.exists());
        fs::write(&temporary, b"second")?;
        assert!(install(&temporary, &target, &crate::digest(b"second")).is_err());
        assert!(!temporary.exists());
        assert_eq!(fs::read(target)?, b"first");
        drop(session);
        fs::remove_dir_all(root)?;
        Ok(())
    }
}
