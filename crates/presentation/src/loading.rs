//! Field preparation ownership and a verified, memory-backed asset reader.
use anyhow::{Context, Result};
use bevy::{
    asset::io::{
        AssetReader, AssetReaderError, AssetSource, AssetSourceBuilder, AssetSourceId,
        ErasedAssetReader, PathStream, Reader, VecReader,
    },
    prelude::*,
};
use resonance_content::prepared::Files;
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
    cache: Arc<Mutex<Cache>>,
    pub active: Arc<AtomicBool>,
    pub memory_reads: Arc<AtomicU64>,
    pub late_reads: Arc<AtomicU64>,
}
#[derive(Default)]
pub(super) struct Cache {
    pub bytes: resonance_content::prepared::Cache,
    pub audio: super::field_audio::Cache,
    pub scripts: Option<resonance_game::authored::FieldScripts>,
}
impl Cache {
    fn configure_scripts(&mut self, root: Option<PathBuf>) {
        if self.scripts.as_ref().map(|scripts| scripts.root()) != root.as_deref() {
            self.scripts = root.map(resonance_game::authored::FieldScripts::new);
        }
    }
}
impl Resident {
    pub fn refresh_scripts(
        &self,
        root: Option<PathBuf>,
        package: &super::new_game::FieldPackage,
    ) -> Result<super::new_game::FieldPackage> {
        let mut cache = self.cache.lock().unwrap();
        cache.configure_scripts(root);
        package.refresh_scripts(&mut cache)
    }
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
    super::model_preview::register(app, root);
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

pub(super) type Pending = Task<super::new_game::Session>;
pub(super) type FieldPending = Task<super::new_game::FieldPackage>;
#[derive(Resource)]
pub(super) struct Task<T: Send + 'static> {
    receiver: Mutex<mpsc::Receiver<Result<T>>>,
    cancelled: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    pub started: Instant,
}
impl Pending {
    pub fn start(
        root: PathBuf,
        script_root: Option<PathBuf>,
        checkpoint: Option<Vec<u8>>,
        resident: &Resident,
    ) -> Result<Self> {
        let cache = resident.cache.clone();
        Self::spawn(move |stop| {
            let identity = super::new_game::Session::identity(&root)?;
            let checkpoint: Option<resonance_game::field::FieldCheckpoint> = checkpoint
                .map(|bytes| {
                    resonance_persistence::decode(&bytes, &identity).map(|(_, state)| state)
                })
                .transpose()?;
            let map = checkpoint.as_ref().map_or(5, |c| c.map_id);
            let mut paths = vec![super::new_game::manifest_path(map)];
            if map == 5 {
                paths.push(super::new_game::manifest_path(340));
            }
            let mut cache = cache.lock().unwrap();
            cache.configure_scripts(script_root);
            let files = Arc::new(Files::load(
                &root,
                &paths.iter().map(String::as_str).collect::<Vec<_>>(),
                &mut cache.bytes,
                || stop.load(Ordering::Relaxed),
            )?);
            let mut session =
                super::new_game::Session::load_prepared(&root, files, checkpoint, &mut cache)?;
            anyhow::ensure!(
                session.identity == identity,
                "cooked content changed during field preparation"
            );
            if map == 5 {
                session.prepare_movie(&root, || stop.load(Ordering::Relaxed))?;
            }
            Ok(session)
        })
    }
}
impl FieldPending {
    pub fn field(
        root: PathBuf,
        script_root: Option<PathBuf>,
        map: u32,
        previous: Option<Arc<super::new_game::FieldPackage>>,
        resident: &Resident,
    ) -> Result<Self> {
        let cache = resident.cache.clone();
        Self::spawn(move |stop| {
            let mut cache = cache.lock().unwrap();
            cache.configure_scripts(script_root);
            if let Some(previous) = previous {
                previous.refresh_scripts(&mut cache)
            } else {
                super::new_game::FieldPackage::prepare(&root, map, &mut cache, || {
                    stop.load(Ordering::Relaxed)
                })
            }
        })
    }
}
impl<T: Send + 'static> Task<T> {
    pub(super) fn spawn(
        job: impl FnOnce(Arc<AtomicBool>) -> Result<T> + Send + 'static,
    ) -> Result<Self> {
        let (send, receive) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = cancelled.clone();
        let worker = std::thread::Builder::new()
            .name("field-preparation".into())
            .spawn(move || {
                let _ = send.send(job(stop));
            })?;
        Ok(Self {
            receiver: Mutex::new(receive),
            cancelled,
            worker: Some(worker),
            started: Instant::now(),
        })
    }
    pub fn poll(&self) -> Result<Option<Result<T>>> {
        match self.receiver.lock().unwrap().try_recv() {
            Ok(value) => Ok(Some(value)),
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(error) => Err(error).context("field preparation worker stopped"),
        }
    }
}
impl<T: Send + 'static> Drop for Task<T> {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub(super) fn black_hold(
    pending: Option<Res<Pending>>,
    field: Option<Res<FieldPending>>,
    session: Option<Res<super::new_game::Session>>,
    resident: Res<Resident>,
    mut outputs: ResMut<Assets<super::materials::TitleOutput>>,
) {
    let held = pending.is_some()
        || field.is_some()
        || session.is_some() && !resident.active.load(Ordering::Acquire);
    super::materials::TitleOutput::update(&mut outputs, |b| b.z = f32::from(held));
}
