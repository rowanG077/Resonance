//! Field preparation ownership and a verified, memory-backed asset reader.
use anyhow::{Context as ContextExt, Result};
use bevy::{
    asset::io::{
        AssetReader, AssetReaderError, AssetSource, AssetSourceBuilder, AssetSourceId,
        ErasedAssetReader, PathStream, Reader, VecReader,
    },
    prelude::*,
};
use resonance_content::prepared::Files;
use std::{
    collections::VecDeque,
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

#[derive(Resource, Clone)]
pub(super) struct Resident {
    pub diagnostics: resonance_content::diagnostics::Diagnostics,
    pub files: Arc<RwLock<Option<Arc<Files>>>>,
    cache: Arc<Mutex<Cache>>,
    pub active: Arc<AtomicBool>,
    /// Battle preparation and rendering temporarily own the shared asset reader.
    pub battle: Arc<AtomicBool>,
    pub memory_reads: Arc<AtomicU64>,
    pub unprepared_reads: Arc<AtomicU64>,
}
impl Default for Resident {
    fn default() -> Self {
        Self {
            diagnostics: resonance_content::diagnostics::Diagnostics::new(true),
            files: Default::default(),
            cache: Default::default(),
            active: Default::default(),
            battle: Default::default(),
            memory_reads: Default::default(),
            unprepared_reads: Default::default(),
        }
    }
}
impl Resident {
    /// Preparations share the application's library and enabled mod layers.
    /// Restart opens a new library and drops all resident preparations.
    pub fn library(&self, root: &Path) -> Result<resonance_content::library::Library> {
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
    pub audio: super::field_audio::Cache,
    pub scripts: Option<resonance_game::authored::FieldScripts>,
    pub service_scripts: symphonia_script_tools::PreparationCache,
    /// The two most recently used fields, scoped to this running application.
    /// Direct capture APIs remain fresh for tools inspecting edits.
    pub fields: VecDeque<Arc<super::new_game::FieldPackage>>,
}
impl Cache {
    fn configure_scripts(&mut self, root: Option<PathBuf>) {
        if self.scripts.as_ref().map(|scripts| scripts.root()) != root.as_deref() {
            self.scripts = root.map(resonance_game::authored::FieldScripts::new);
            self.fields.clear();
        }
    }

    fn field(
        &mut self,
        library: &resonance_content::library::Library,
        map: u32,
        cancelled: impl Fn() -> bool,
        diagnostics: resonance_content::diagnostics::Diagnostics,
    ) -> Result<Arc<super::new_game::FieldPackage>> {
        anyhow::ensure!(!cancelled(), "field preparation cancelled");
        // Explicit external script projects are reread during preparation.
        if self.scripts.is_some() {
            return Ok(Arc::new(super::new_game::FieldPackage::capture(
                library,
                map,
                self,
                cancelled,
                diagnostics,
            )?));
        }
        let package = if let Some(index) = self.fields.iter().position(|p| p.assets.map_id == map) {
            self.fields.remove(index).unwrap()
        } else {
            Arc::new(super::new_game::FieldPackage::capture(
                library,
                map,
                self,
                &cancelled,
                diagnostics,
            )?)
        };
        self.fields.push_back(package.clone());
        while self.fields.len() > 2 {
            self.fields.pop_front();
        }
        Ok(package)
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
                files.is_some(),
            )
        };
        if let Some(bytes) = bytes {
            self.resident.memory_reads.fetch_add(1, Ordering::Relaxed);
            return Ok(Box::new(VecReader::new(bytes.to_vec())) as Box<dyn Reader>);
        }
        if prepared {
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

fn asset_error(error: anyhow::Error) -> AssetReaderError {
    AssetReaderError::Io(std::io::Error::other(format!("{error:#}")).into())
}

pub(super) fn install(app: &mut App, root: &Path) {
    super::model_preview::register(app);
    let resident = Resident {
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
            Box::new(ReaderAdapter {
                resident: resident.clone(),
                fallback: AssetSource::get_default_reader(root.clone())(),
            })
        }),
    );
}

pub(super) type Pending = Task<super::new_game::Session>;
pub(super) type FieldPending = Task<Arc<super::new_game::FieldPackage>>;
pub(super) type BattlePending = Task<super::battle::Package>;
#[derive(Resource)]
pub(super) struct Task<T: Send + 'static> {
    receiver: Mutex<mpsc::Receiver<Result<T>>>,
    cancelled: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    pub started: Instant,
}
impl Pending {
    pub fn dungeon(
        root: PathBuf,
        script_root: Option<PathBuf>,
        destination: super::dungeons::Destination,
        resident: &Resident,
    ) -> Result<Self> {
        let library = resident.library(&root)?;
        let cache = resident.cache.clone();
        let diagnostics = resident.diagnostics.clone();
        Self::spawn(move |stop| {
            let mut cache = cache.lock().unwrap();
            cache.configure_scripts(script_root);
            let package = cache.field(
                &library,
                destination.map,
                || stop.load(Ordering::Relaxed),
                diagnostics,
            )?;
            super::new_game::Session::load_start(
                package,
                super::new_game::Start::Dungeon(destination),
                None,
            )
        })
    }

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
        Self::spawn(move |stop| {
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
            let package =
                cache.field(&library, map, || stop.load(Ordering::Relaxed), diagnostics)?;
            let mut session =
                super::new_game::Session::load_prepared(package, checkpoint, initial_preferences)?;
            if new_game {
                session.prepare_movie(|| stop.load(Ordering::Relaxed))?;
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
        resident: &Resident,
    ) -> Result<Self> {
        let library = resident.library(&root)?;
        let cache = resident.cache.clone();
        let diagnostics = resident.diagnostics.clone();
        Self::spawn(move |stop| {
            let mut cache = cache.lock().unwrap();
            cache.configure_scripts(script_root);
            cache.field(&library, map, || stop.load(Ordering::Relaxed), diagnostics)
        })
    }
}
pub(super) type WorldPending = Task<super::overworld::Package>;
impl WorldPending {
    pub fn overworld(root: PathBuf, resident: &Resident) -> Result<Self> {
        let library = resident.library(&root)?;
        let cache = resident.cache.clone();
        let diagnostics = resident.diagnostics.clone();
        Self::spawn(move |stop| {
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
        Self::spawn(move |stop| {
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
            let catalogue = resonance_content::battle_audio::document::Catalogue::capture(
                &mut snapshot,
                &cancelled,
            )?;
            let inputs = resonance_game::battle::encounter::Assets::capture(
                &mut snapshot,
                &menus,
                &data,
                &party,
                setup,
                diagnostics.clone(),
                &cancelled,
            )?;
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
            .name("scene-preparation".into())
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
            Err(error) => Err(error).context("scene preparation worker stopped"),
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
    battle: Option<Res<super::battle::Owner>>,
    title_return: Option<Res<super::game_over::Returning>>,
) {
    let battle_visible = battle.is_some();
    let held = title_return.is_some()
        || pending.is_some()
        || field.is_some()
        || session.is_some() && !resident.active.load(Ordering::Acquire) && !battle_visible;
    let portal = title_return.is_none()
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
    use super::*;
    use resonance_content::diagnostics::Diagnostics;
    use std::fs;

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
