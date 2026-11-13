//! Field preparation ownership and a verified, memory-backed asset reader.
mod field;
pub(crate) mod graphics;
pub(crate) mod party;
mod prefetch;
pub(crate) mod startup;
use anyhow::{Context as ContextExt, Result};
use bevy::{
    asset::io::{
        AssetReader, AssetReaderError, AssetSource, AssetSourceBuilder, AssetSourceId,
        ErasedAssetReader, PathStream, Reader,
    },
    prelude::*,
};
pub(super) use prefetch::{FieldPending, FieldPrefetch};
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

/// Tickets follow actual loaders, including after their requesting scene retires.
#[derive(Default)]
struct LoadScope {
    pending: AtomicUsize,
    files: Option<Arc<Files>>,
}
#[derive(Clone, Default)]
pub(super) struct LoadTasks(Arc<LoadScope>);
pub(super) struct LoadTicket(Arc<LoadScope>);
impl LoadTasks {
    pub fn ticket(&self) -> LoadTicket {
        self.0.pending.fetch_add(1, Ordering::Relaxed);
        LoadTicket(self.0.clone())
    }
    pub fn complete(&self) -> bool {
        self.0.pending.load(Ordering::Acquire) == 0
    }
}
impl Drop for LoadTicket {
    fn drop(&mut self) {
        self.0.pending.fetch_sub(1, Ordering::Release);
    }
}

/// Upload geometry and images without cloning or loading animation state.
fn geometry_only(part: &resonance_content::ScenePart) -> resonance_content::ScenePart {
    resonance_content::ScenePart {
        resource: part.resource,
        mesh: part.mesh.clone(),
        textures: part.textures.clone(),
        materials: part.materials.clone(),
        appearance: part.appearance.clone(),
        translation: part.translation,
        clips: Vec::new(),
        autoplay: false,
        texture_animations: Vec::new(),
        bone_names: part.bone_names.clone(),
        prepared_motions: None,
        prepared_geometry: part.prepared_geometry.clone(),
        material_nodes: part.material_nodes.clone(),
        outline_color: part.outline_color,
        secondary_motion: Default::default(),
    }
}

#[derive(Resource, Clone)]
pub(super) struct Resident {
    /// Keep startup resolution alive independently of the current scene's bytes.
    library: Option<resonance_content::library::Library>,
    pub diagnostics: resonance_content::diagnostics::Diagnostics,
    pub files: Arc<RwLock<Option<Arc<Files>>>>,
    scopes: Arc<Mutex<Vec<std::sync::Weak<LoadScope>>>>,
    cache: Arc<Mutex<Cache>>,
    pub common: Arc<std::sync::OnceLock<Arc<startup::Common>>>,
    pub active: Arc<AtomicBool>,
    pub battle: Arc<AtomicBool>,
    pub memory_reads: Arc<AtomicU64>,
    pub unprepared_reads: Arc<AtomicU64>,
}
impl Default for Resident {
    fn default() -> Self {
        let common = Arc::new(std::sync::OnceLock::new());
        Self {
            library: None,
            diagnostics: resonance_content::diagnostics::Diagnostics::new(true),
            files: Default::default(),
            scopes: Default::default(),
            cache: Arc::new(Mutex::new(Cache {
                common: common.clone(),
                ..Default::default()
            })),
            common,
            active: Default::default(),
            battle: Default::default(),
            memory_reads: Default::default(),
            unprepared_reads: Default::default(),
        }
    }
}
impl Resident {
    fn scope(&self, files: Arc<Files>) -> LoadTasks {
        let scope = Arc::new(LoadScope {
            pending: AtomicUsize::new(0),
            files: Some(files),
        });
        let mut scopes = self.scopes.lock().unwrap();
        scopes.retain(|scope| scope.strong_count() > 0);
        scopes.push(Arc::downgrade(&scope));
        LoadTasks(scope)
    }
    fn scoped_bytes(&self, path: &str) -> (Option<Arc<[u8]>>, bool) {
        let scopes = self.scopes.lock().unwrap();
        let mut prepared = false;
        for scope in scopes.iter().filter_map(std::sync::Weak::upgrade) {
            prepared = true;
            if let Some(bytes) = scope.files.as_ref().and_then(|files| files.read(path).ok()) {
                return (Some(bytes), true);
            }
        }
        (None, prepared)
    }
    /// Restart opens a new library and drops the resident preparations.
    pub fn library(&self, root: &Path) -> Result<resonance_content::library::Library> {
        if let Some(library) = &self.library {
            return Ok(library.clone());
        }
        match self
            .files
            .read()
            .unwrap()
            .as_ref()
            .and_then(|files| files.library())
        {
            Some(library) => Ok(library.clone()),
            None => resonance_content::library::Library::open(root, &[]),
        }
    }
}
#[derive(Default)]
pub(super) struct Cache {
    pub common: Arc<std::sync::OnceLock<Arc<startup::Common>>>,
    pub audio: super::field_audio::Cache,
    pub scripts: Option<resonance_game::authored::FieldScripts>,
    pub service_scripts: symphonia_script_tools::PreparationCache,
    /// The most recently captured field's bytes, while that field is still alive.
    pub previous: std::sync::Weak<Files>,
}
impl Cache {
    pub(super) fn snapshot(
        &self,
        library: &resonance_content::library::Library,
    ) -> Result<resonance_content::library::Snapshot> {
        match self.common.get().filter(|_| self.scripts.is_none()) {
            Some(common) => library.snapshot_with(&common.files),
            None => Ok(library.snapshot()),
        }
    }
    fn configure_scripts(&mut self, root: Option<PathBuf>) {
        if self.scripts.as_ref().map(|scripts| scripts.root()) != root.as_deref() {
            self.scripts = root.map(resonance_game::authored::FieldScripts::new);
        }
    }
}
struct SharedReader(bevy::tasks::futures_lite::io::Cursor<Arc<[u8]>>);
impl SharedReader {
    fn new(bytes: Arc<[u8]>) -> Self {
        Self(bevy::tasks::futures_lite::io::Cursor::new(bytes))
    }
}
impl bevy::tasks::futures_lite::io::AsyncRead for SharedReader {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
        buffer: &mut [u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.0).poll_read(context, buffer)
    }
}
impl bevy::tasks::futures_lite::io::AsyncSeek for SharedReader {
    fn poll_seek(
        mut self: std::pin::Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
        position: std::io::SeekFrom,
    ) -> std::task::Poll<std::io::Result<u64>> {
        std::pin::Pin::new(&mut self.0).poll_seek(context, position)
    }
}
impl Reader for SharedReader {
    fn read_to_end<'a>(
        &'a mut self,
        buffer: &'a mut Vec<u8>,
    ) -> bevy::asset::io::StackFuture<
        'a,
        std::io::Result<usize>,
        { bevy::asset::io::STACK_FUTURE_SIZE },
    > {
        bevy::asset::io::StackFuture::from(async move {
            let position = self.0.position();
            let bytes = self.0.get_ref();
            let start = position.min(bytes.len() as u64) as usize;
            let count = bytes.len() - start;
            buffer.extend_from_slice(&bytes[start..]);
            self.0.set_position(position + count as u64);
            Ok(count)
        })
    }

    fn seekable(
        &mut self,
    ) -> Result<&mut dyn bevy::asset::io::SeekableReader, bevy::asset::io::ReaderNotSeekableError>
    {
        Ok(self)
    }
}

struct ReaderAdapter {
    resident: Resident,
    fallback: Box<dyn ErasedAssetReader>,
}
impl AssetReader for ReaderAdapter {
    async fn read<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        let key = path.to_string_lossy();
        resonance_content::validate_asset_path(&key).map_err(asset_error)?;
        let (bytes, prepared) = {
            let files = self.resident.files.read().unwrap();
            (
                files.as_ref().and_then(|f| f.read(key.as_ref()).ok()),
                files.is_some() || self.resident.common.get().is_some(),
            )
        };
        let mut scoped_prepared = false;
        if let Some(bytes) = bytes
            .or_else(|| {
                self.resident
                    .common
                    .get()
                    .and_then(|common| common.files.read(&key).ok())
            })
            .or_else(|| {
                let (bytes, prepared) = self.resident.scoped_bytes(&key);
                scoped_prepared = prepared;
                bytes
            })
        {
            self.resident.memory_reads.fetch_add(1, Ordering::Relaxed);
            return Ok(Box::new(SharedReader::new(bytes)) as Box<dyn Reader>);
        }
        if prepared || scoped_prepared {
            self.resident
                .unprepared_reads
                .fetch_add(1, Ordering::Relaxed);
            self.resident
                .diagnostics
                .report(
                    "asset reader",
                    anyhow::anyhow!("unprepared scene asset {}", path.display()),
                )
                .map_err(asset_error)?;
            return Err(AssetReaderError::NotFound(path.to_owned()));
        }
        match self.fallback.read(path).await {
            Ok(reader) => Ok(reader),
            Err(error) => {
                self.resident
                    .diagnostics
                    .report(
                        "asset reader",
                        anyhow::anyhow!("read scene asset {}: {error}", path.display()),
                    )
                    .map_err(asset_error)?;
                Err(error)
            }
        }
    }

    async fn read_meta<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        if self.resident.files.read().unwrap().is_some() || self.resident.common.get().is_some() {
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

fn asset_error(error: anyhow::Error) -> AssetReaderError {
    AssetReaderError::Io(std::io::Error::other(format!("{error:#}")).into())
}

pub(super) fn install(
    app: &mut App,
    root: &Path,
    library: Option<&resonance_content::library::Library>,
) {
    prefetch::install(app);
    let supported = super::texture::Supported::default();
    app.insert_resource(supported.clone());
    super::model_preview::register(app, supported.clone());
    let resident = Resident {
        library: library.cloned(),
        diagnostics: app
            .world()
            .get_resource::<super::diagnostics::Diagnostics>()
            .map(|policy| policy.0.clone())
            .unwrap_or_else(|| resonance_content::diagnostics::Diagnostics::new(true)),
        ..Default::default()
    };
    app.insert_resource(resident.clone());
    let root = root.to_string_lossy().to_string();
    app.register_asset_source(
        AssetSourceId::Default,
        AssetSourceBuilder::new(move || {
            Box::new(super::texture::Textures {
                reader: Box::new(ReaderAdapter {
                    resident: resident.clone(),
                    fallback: AssetSource::get_default_reader(root.clone())(),
                }),
                supported: supported.clone(),
            })
        }),
    );
}

pub(super) type Pending = Task<super::new_game::Session>;
pub(super) type BattlePending = Task<super::battle::Package>;
#[derive(Resource)]
pub(super) struct Task<T: Send + 'static> {
    receiver: Mutex<mpsc::Receiver<Result<T>>>,
    cancelled: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    pub started: Instant,
    name: String,
}
impl Pending {
    pub fn start(
        root: PathBuf,
        script_root: Option<PathBuf>,
        checkpoint: Option<Vec<u8>>,
        initial_preferences: Option<resonance_content::menu_data::CustomizeSettings>,
        resident: &Resident,
    ) -> Result<Self> {
        let library = resident.library(&root)?;
        let cache = resident.cache.clone();
        let diagnostics = resident.diagnostics.clone();
        Self::spawn("session start / saved checkpoint", move |stop| {
            let checkpoint = checkpoint
                .map(|bytes| resonance_persistence::decode::<super::saves::SceneCheckpoint>(&bytes))
                .transpose()?;
            let mut cache = cache.lock().unwrap();
            cache.configure_scripts(script_root);
            let checkpoint = match checkpoint.map(|save| save.into_parts().1) {
                Some(super::saves::SceneCheckpoint::World(saved)) => {
                    let package = Arc::new(super::overworld::Package::capture(
                        &library,
                        &mut cache,
                        || stop.load(Ordering::Relaxed),
                        diagnostics,
                    )?);
                    return super::new_game::Session::load_world_prepared(package, saved);
                }
                Some(super::saves::SceneCheckpoint::Field(saved)) => Some(saved),
                None => None,
            };
            let new_game = checkpoint.is_none();
            let map = checkpoint.as_ref().map_or(5, |saved| saved.map_id);
            let package = Arc::new(super::new_game::FieldPackage::capture(
                &library,
                map,
                &mut cache,
                || stop.load(Ordering::Relaxed),
                diagnostics,
            )?);
            let mut session =
                super::new_game::Session::load_prepared(package, checkpoint, initial_preferences)?;
            if new_game {
                session.prepare_movie(|| stop.load(Ordering::Relaxed))?;
            }
            Ok(session)
        })
    }
}
pub(super) type WorldPending = Task<super::overworld::Package>;
impl WorldPending {
    pub fn overworld(root: PathBuf, resident: &Resident) -> Result<Self> {
        let library = resident.library(&root)?;
        let cache = resident.cache.clone();
        let diagnostics = resident.diagnostics.clone();
        Self::spawn("overworld", move |stop| {
            let mut cache = cache.lock().unwrap();
            super::overworld::Package::capture(
                &library,
                &mut cache,
                || stop.load(Ordering::Relaxed),
                diagnostics,
            )
        })
    }
}
impl BattlePending {
    pub fn battle(entry: super::battle::Entry, resident: &Resident) -> Result<Self> {
        let cache = resident.cache.clone();
        let diagnostics = resident.diagnostics.clone();
        Self::spawn(
            format!(
                "battle {:?}; map {}; seed {}",
                entry.setup, entry.options.map, entry.options.random_seed
            ),
            move |stop| {
                let super::battle::Entry {
                    mut snapshot,
                    party,
                    setup,
                    options,
                    data,
                    menus,
                    gameplay_random,
                } = entry;
                let mut cache = cache.lock().unwrap();
                let entry_seed = options.random_seed;
                let cancelled = || stop.load(Ordering::Relaxed);
                let common = cache.common.get().and_then(|common| common.battle.clone());
                let catalogue = match &common {
                    Some(common) => common.audio.clone(),
                    None => Arc::new(
                        resonance_content::battle_audio::document::Catalogue::capture(
                            &mut snapshot,
                            &cancelled,
                        )?,
                    ),
                };
                let inputs = match &common {
                    Some(common) => resonance_game::battle::encounter::Assets::capture_with_common(
                        &mut snapshot,
                        &menus,
                        &data,
                        &party,
                        setup,
                        diagnostics.clone(),
                        &cancelled,
                        common,
                    )?,
                    None => resonance_game::battle::encounter::Assets::capture(
                        &mut snapshot,
                        &menus,
                        &data,
                        &party,
                        setup,
                        diagnostics.clone(),
                        &cancelled,
                    )?,
                };
                let (mut assets, prepared, selected) =
                    inputs.prepare_named(&menus, options, &catalogue)?;
                let audio = super::battle_audio::Assets::capture_named(
                    &catalogue,
                    &selected,
                    &mut snapshot,
                    diagnostics.clone(),
                    &mut cache.audio,
                    &cancelled,
                )?;
                super::battle::capture_view_files(&mut snapshot)?;
                assets.files = Files::from_snapshot(snapshot, cancelled, diagnostics)?;
                let catalogue = assets.catalogue.clone();
                // View retains its diagnostic and skip policy for an invalid stage.
                let scenery_models = if assets.stage.validate().is_ok() {
                    resonance_game::battle::stage::Model::load(&assets.files, &assets.stage)?
                } else {
                    Default::default()
                };
                Ok(super::battle::Package {
                    assets: Arc::new(assets),
                    scenery_models,
                    prepared,
                    audio,
                    party,
                    menus,
                    data,
                    catalogue,
                    gameplay_random,
                    entry_seed,
                })
            },
        )
    }
}

impl<T: Send + 'static> Task<T> {
    pub(super) fn spawn(
        name: impl Into<String>,
        job: impl FnOnce(Arc<AtomicBool>) -> Result<T> + Send + 'static,
    ) -> Result<Self> {
        let name = name.into();
        let worker_name = name.clone();
        info!(job = %name, "Starting background preparation");
        let (send, receive) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = cancelled.clone();
        let worker = std::thread::Builder::new()
            .name("scene-preparation".into())
            .spawn(move || {
                let _span = info_span!("scene_preparation", job = %worker_name).entered();
                let started = Instant::now();
                let result = job(stop.clone()).with_context(|| format!("preparing {worker_name}"));
                match &result {
                    Ok(_) => info!(
                        elapsed_ms = started.elapsed().as_millis(),
                        "Preparation completed"
                    ),
                    Err(error) if stop.load(Ordering::Relaxed) => {
                        debug!("Preparation cancelled: {error:#}")
                    }
                    Err(error) => error!(
                        elapsed_ms = started.elapsed().as_millis(),
                        "Preparation failed: {error:#}"
                    ),
                }
                let _ = send.send(result);
            })?;
        Ok(Self {
            receiver: Mutex::new(receive),
            cancelled,
            worker: Some(worker),
            started: Instant::now(),
            name,
        })
    }
    pub fn poll(&self) -> Result<Option<Result<T>>> {
        match self.receiver.lock().unwrap().try_recv() {
            Ok(value) => Ok(Some(value)),
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(error) => Err(error).with_context(|| format!("preparation worker for {} stopped after {:.3}s (possibly panicked; see crash report)", self.name, self.started.elapsed().as_secs_f64())),
        }
    }

    /// CPU preparation runs independently of application and render updates.
    pub fn is_running(&self) -> bool {
        self.worker
            .as_ref()
            .is_some_and(|worker| !worker.is_finished())
    }
}
impl<T: Send + 'static> Drop for Task<T> {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take()
            && let Err(payload) = worker.join()
        {
            let message = payload
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| payload.downcast_ref::<&str>().copied())
                .unwrap_or("non-string panic payload");
            error!(job = %self.name, "Preparation worker panicked: {message}");
        }
    }
}

pub(super) fn black_hold(
    startup: startup::Status,
    pending: Option<Res<Pending>>,
    field: Option<Res<FieldPending>>,
    session: Option<Res<super::new_game::Session>>,
    resident: Res<Resident>,
    mut outputs: ResMut<Assets<super::materials::TitleOutput>>,
    battle: Option<Res<super::battle::Owner>>,
    title_return: Option<Res<super::game_over::Returning>>,
) {
    let battle_visible = battle.is_some();
    let held = !startup.ready()
        || title_return.is_some()
        || pending.is_some()
        || field.is_some()
        || session.is_some() && !resident.active.load(Ordering::Acquire) && !battle_visible;
    let portal = startup.ready()
        && title_return.is_none()
        && session
            .as_ref()
            .and_then(|s| s.overworld())
            .is_some_and(|scene| scene.session.portal().is_some());
    super::materials::TitleOutput::update(&mut outputs, |b| {
        b.z = f32::from(held && !portal);
        if held && portal {
            // Keep the portal's whiteout while the destination artwork loads.
            b.x = 0.;
            b.y = 1.;
        }
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn loader_tickets_keep_inputs_after_owner_cancellation_without_scene_history() {
        let resident = Resident::default();
        let mut files = Files::default();
        files.insert("late.bin".into(), Arc::from(&b"captured"[..]));
        let files = Arc::new(files);
        let retired = Arc::downgrade(&files);
        let loads = resident.scope(files);
        let ticket = loads.ticket();
        assert!(!loads.complete());
        drop(loads);
        assert_eq!(
            resident.scoped_bytes("late.bin").0.unwrap().as_ref(),
            b"captured"
        );
        drop(ticket);
        assert!(retired.upgrade().is_none());
        assert!(resident.scoped_bytes("late.bin").0.is_none());
        let next = resident.scope(Arc::new(Files::default()));
        assert_eq!(resident.scopes.lock().unwrap().len(), 1);
        drop(next);
    }

    use super::*;
    use resonance_content::diagnostics::Diagnostics;
    use std::fs;

    #[test]
    fn failed_preparation_keeps_job_identity_and_cause() {
        let task = Task::<()>::spawn("field 332", |_| {
            Err(anyhow::anyhow!("missing mesh dragon.glb"))
        })
        .unwrap();
        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if let Some(result) = task.poll().unwrap() {
                let error = format!("{:#}", result.unwrap_err());
                assert!(error.contains("field 332"), "{error}");
                assert!(error.contains("missing mesh dragon.glb"), "{error}");
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
    }

    #[test]
    fn panicked_preparation_identifies_the_failed_job() {
        let task = Task::<()>::spawn("battle formation 77", |_| {
            panic!("broken battle preparation")
        })
        .unwrap();
        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if let Err(error) = task.poll() {
                let error = format!("{error:#}");
                assert!(error.contains("battle formation 77"), "{error}");
                assert!(error.contains("stopped"), "{error}");
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
    }

    #[test]
    fn installed_snapshot_serves_memory_after_activation_and_blocks_disk_fallback() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("extra.bin"), b"disk").unwrap();
        for paranoid in [false, true] {
            let diagnostics = Diagnostics::new(paranoid);
            let resident = Resident {
                diagnostics: diagnostics.clone(),
                ..Default::default()
            };
            let reader = ReaderAdapter {
                resident,
                fallback: AssetSource::get_default_reader(
                    root.path().to_string_lossy().into_owned(),
                )(),
            };
            let mut disk =
                bevy::tasks::block_on(AssetReader::read(&reader, Path::new("extra.bin"))).unwrap();
            let mut bytes = Vec::new();
            bevy::tasks::block_on(disk.read_to_end(&mut bytes)).unwrap();
            assert_eq!(bytes, b"disk");
            assert!(!diagnostics.has_errors());
            let mut files = Files::default();
            files.insert("prepared.bin".into(), Arc::from(b"verified".as_slice()));
            *reader.resident.files.write().unwrap() = Some(Arc::new(files));
            reader.resident.active.store(true, Ordering::Release);
            let mut memory =
                bevy::tasks::block_on(AssetReader::read(&reader, Path::new("prepared.bin")))
                    .unwrap();
            let mut bytes = Vec::new();
            bevy::tasks::block_on(memory.read_to_end(&mut bytes)).unwrap();
            assert_eq!(bytes, b"verified");
            assert!(!diagnostics.has_errors());
            assert!(
                bevy::tasks::block_on(AssetReader::read(&reader, Path::new("extra.bin"))).is_err()
            );
            assert_eq!(reader.resident.memory_reads.load(Ordering::Relaxed), 1);
            assert_eq!(reader.resident.unprepared_reads.load(Ordering::Relaxed), 1);
            assert_eq!(diagnostics.entries().len(), 1);
            assert!(
                bevy::tasks::block_on(AssetReader::read(&reader, Path::new("../extra.bin")))
                    .is_err()
            );
        }
    }
}
