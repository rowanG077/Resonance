//! Field preparation ownership and a verified, memory-backed asset reader.
use anyhow::{Context, Result};
use bevy::{
    asset::io::{
        AssetReader, AssetReaderError, AssetSource, AssetSourceBuilder, AssetSourceId,
        ErasedAssetReader, PathStream, Reader, VecReader,
    },
    prelude::*,
};
use resonance_content::prepared::{Cache, Files};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        mpsc,
    },
    thread::JoinHandle,
    time::Instant,
};

/// Asset load state describes the published asset, not every outstanding loader
/// for its path. Hold a ticket in each explicit load job until it actually exits.
#[derive(Default)]
pub(super) struct LoadTasks(Arc<AtomicUsize>);
pub(super) struct LoadTicket(Arc<AtomicUsize>);
impl LoadTasks {
    pub fn ticket(&self) -> LoadTicket {
        self.0.fetch_add(1, Ordering::Relaxed);
        LoadTicket(self.0.clone())
    }
    pub fn complete(&self) -> bool {
        self.0.load(Ordering::Acquire) == 0
    }
}
impl Drop for LoadTicket {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Release);
    }
}

#[derive(Resource, Clone, Default)]
pub(super) struct Resident {
    pub files: Arc<RwLock<Option<Arc<Files>>>>,
    pub active: Arc<AtomicBool>,
    pub memory_reads: Arc<AtomicU64>,
    pub late_reads: Arc<AtomicU64>,
}
struct ReaderAdapter {
    resident: Resident,
    fallback: Box<dyn ErasedAssetReader>,
}
impl AssetReader for ReaderAdapter {
    async fn read<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        let bytes = self
            .resident
            .files
            .read()
            .unwrap()
            .as_ref()
            .and_then(|f| f.bytes.get(&path.to_string_lossy().to_string()).cloned());
        if let Some(bytes) = bytes {
            self.resident.memory_reads.fetch_add(1, Ordering::Relaxed);
            if self.resident.active.load(Ordering::Acquire) {
                self.resident.late_reads.fetch_add(1, Ordering::Relaxed);
                return Err(AssetReaderError::Io(
                    std::io::Error::other(format!(
                        "late field asset read after activation: {}",
                        path.display()
                    ))
                    .into(),
                ));
            }
            return Ok(Box::new(VecReader::new(bytes.to_vec())) as Box<dyn Reader>);
        }
        if self.resident.files.read().unwrap().is_some() {
            self.resident.late_reads.fetch_add(1, Ordering::Relaxed);
            return Err(AssetReaderError::Io(
                std::io::Error::other(format!("undeclared field asset {}", path.display())).into(),
            ));
        }
        self.fallback.read(path).await
    }
    async fn read_meta<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        if self.resident.files.read().unwrap().is_some() {
            return Err(AssetReaderError::NotFound(path.to_owned()));
        }
        self.fallback.read_meta(path).await
    }
    async fn read_directory<'a>(
        &'a self,
        path: &'a Path,
    ) -> Result<Box<PathStream>, AssetReaderError> {
        self.fallback.read_directory(path).await
    }
    async fn is_directory<'a>(&'a self, path: &'a Path) -> Result<bool, AssetReaderError> {
        self.fallback.is_directory(path).await
    }
}

pub(super) fn install(app: &mut App, root: &Path) {
    let resident = Resident::default();
    app.insert_resource(resident.clone());
    let root = root.to_string_lossy().to_string();
    app.register_asset_source(
        AssetSourceId::Default,
        AssetSourceBuilder::new(move || {
            Box::new(ReaderAdapter {
                resident: resident.clone(),
                fallback: AssetSource::get_default_reader(root.clone())(),
            })
        }),
    );
}

pub(super) struct Prepared {
    pub session: super::new_game::Session,
    pub files: Arc<Files>,
}
#[derive(Resource)]
pub(super) struct Pending {
    receiver: Mutex<mpsc::Receiver<Result<Prepared>>>,
    cancelled: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    pub started: Instant,
}
impl Pending {
    pub fn new(root: PathBuf) -> Result<Self> {
        let (send, receive) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = cancelled.clone();
        let worker = std::thread::Builder::new()
            .name("field-preparation".into())
            .spawn(move || {
                let result = (|| -> Result<_> {
                    let files = Arc::new(Files::load(
                        &root,
                        &[
                            "fields/new-game-setup.preload.json",
                            "fields/iselia-classroom.preload.json",
                        ],
                        &mut Cache::default(),
                        || stop.load(Ordering::Relaxed),
                    )?);
                    let mut session = super::new_game::Session::load_prepared(&root, &files)?;
                    session.prepare_movie(&root, || stop.load(Ordering::Relaxed))?;
                    Ok(Prepared { session, files })
                })();
                let _ = send.send(result);
            })?;
        Ok(Self {
            receiver: Mutex::new(receive),
            cancelled,
            worker: Some(worker),
            started: Instant::now(),
        })
    }
    pub fn poll(&self) -> Result<Option<Result<Prepared>>> {
        match self.receiver.lock().unwrap().try_recv() {
            Ok(value) => Ok(Some(value)),
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(error) => Err(error).context("field preparation worker stopped"),
        }
    }
}
impl Drop for Pending {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub(super) fn black_hold(
    pending: Option<Res<Pending>>,
    session: Option<Res<super::new_game::Session>>,
    resident: Res<Resident>,
    mut outputs: ResMut<Assets<super::materials::TitleOutput>>,
) {
    let held = pending.is_some() || session.is_some() && !resident.active.load(Ordering::Acquire);
    super::materials::TitleOutput::update(&mut outputs, |b| b.z = f32::from(held));
}
