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
#[derive(Default)]
pub(super) struct Cache {
    pub bytes: resonance_content::prepared::Cache,
    pub audio: super::field_audio::Cache,
    pub scripts: Option<resonance_game::authored::FieldScripts>,
    pub service_scripts: symphonia_script_tools::PreparationCache,
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
    super::model_preview::register(app, root);
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
pub(super) type FieldPending = Task<super::new_game::FieldPackage>;
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
        let cache = resident.cache.clone();
        Self::spawn(move |stop| {
            let mut cache = cache.lock().unwrap();
            cache.configure_scripts(script_root);
            let files = Arc::new(Files::load(
                &root,
                &[&super::new_game::manifest_path(destination.map)],
                &mut cache.bytes,
                || stop.load(Ordering::Relaxed),
            )?);
            super::new_game::Session::load_start(
                &root,
                files,
                super::new_game::Start::Dungeon(destination),
                None,
                &mut cache,
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
        let cache = resident.cache.clone();
        let diagnostics = resident.diagnostics.clone();
        Self::spawn(move |stop| {
            let checkpoint = checkpoint
                .map(|bytes| resonance_persistence::decode::<super::saves::SceneCheckpoint>(&bytes))
                .transpose()?;
            let map = checkpoint.as_ref().map_or(5, |save| save.state.map());
            let mut paths = vec![super::new_game::manifest_path(map)];
            if map == 5 {
                paths.push(super::new_game::manifest_path(340));
            }
            let mut cache = cache.lock().unwrap();
            cache.configure_scripts(script_root);
            let files = Arc::new(Files::load_with_diagnostics(
                &root,
                &paths.iter().map(String::as_str).collect::<Vec<_>>(),
                &mut cache.bytes,
                || stop.load(Ordering::Relaxed),
                diagnostics,
            )?);
            let identity = resonance_persistence::Identity::load(&files)?;
            let checkpoint = checkpoint
                .map(|save| save.admit(&identity).map(|(_, state)| state))
                .transpose()?;
            let mut session = match checkpoint {
                Some(super::saves::SceneCheckpoint::World(checkpoint)) => {
                    super::new_game::Session::load_world_prepared(
                        &root,
                        files,
                        checkpoint,
                        &mut cache,
                        || stop.load(Ordering::Relaxed),
                    )?
                }
                checkpoint => super::new_game::Session::load_prepared(
                    &root,
                    files,
                    checkpoint.map(|c| match c {
                        super::saves::SceneCheckpoint::Field(c) => c,
                        _ => unreachable!(),
                    }),
                    initial_preferences,
                    &mut cache,
                )?,
            };
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
        let diagnostics = resident.diagnostics.clone();
        Self::spawn(move |stop| {
            let mut cache = cache.lock().unwrap();
            cache.configure_scripts(script_root);
            if let Some(previous) = previous {
                previous.refresh_scripts(&mut cache)
            } else {
                super::new_game::FieldPackage::prepare_with_diagnostics(
                    &root,
                    map,
                    &mut cache,
                    || stop.load(Ordering::Relaxed),
                    diagnostics,
                )
            }
        })
    }
}
pub(super) type WorldPending = Task<super::overworld::Package>;
impl WorldPending {
    pub fn overworld(
        root: PathBuf,
        fields: std::collections::BTreeSet<u32>,
        resident: &Resident,
    ) -> Result<Self> {
        let cache = resident.cache.clone();
        Self::spawn(move |stop| {
            let mut cache = cache.lock().unwrap();
            let world = Arc::new(resonance_game::overworld::Prepared::load(
                &root,
                &mut cache.bytes,
                fields,
                || stop.load(Ordering::Relaxed),
            )?);
            let audio = cache.audio.load("worlds/audio.json", &world.files)?;
            Ok(super::overworld::Package { world, audio })
        })
    }
}
impl BattlePending {
    pub fn battle(root: PathBuf, entry: super::battle::Entry, resident: &Resident) -> Result<Self> {
        let cache = resident.cache.clone();
        Self::spawn(move |stop| {
            let super::battle::Entry {
                files: retained,
                party,
                setup,
                options,
                data,
                menus,
                gameplay_random,
            } = entry;
            let mut cache = cache.lock().unwrap();
            let (files, descriptor) = resonance_game::battle::audio::prepare(
                &root,
                (*retained).clone(),
                &mut cache.bytes,
                || stop.load(Ordering::Relaxed),
            )?;
            let audio =
                super::battle_audio::Assets::load(&files, descriptor.as_ref(), &mut cache.audio)?;
            let inputs = resonance_game::battle::encounter::Inputs::load(
                &root,
                &files,
                &menus,
                &data,
                &party,
                setup,
                &mut cache.bytes,
                || stop.load(Ordering::Relaxed),
            )?;
            let assets = resonance_game::battle::encounter::Assets {
                inputs,
                audio: descriptor,
            };
            let entry_seed = options.random_seed;
            let prepared = assets.prepare(&menus, options, |sound| audio.bind(sound))?;
            let catalogue = assets.catalogue.clone();
            Ok(super::battle::Package {
                assets: Arc::new(assets),
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
    super::materials::TitleOutput::update(&mut outputs, |b| b.z = f32::from(held));
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::{
        diagnostics::Diagnostics,
        field_preload::{File, Role, SHARED_PATH, Shared, VERSION},
    };
    use sha2::{Digest, Sha256};
    use std::{collections::BTreeMap, fs};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "resonance-runtime-reader-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir(&root).unwrap();
            fs::write(
                root.join(resonance_content::field_preload::SHARED_PATH),
                serde_json::to_vec(&resonance_content::field_preload::Shared::<
                    resonance_content::field_preload::File,
                > {
                    version: resonance_content::field_preload::VERSION,
                    files: BTreeMap::new(),
                })
                .unwrap(),
            )
            .unwrap();
            Self(root)
        }
        fn reader(&self, resident: Resident) -> ReaderAdapter {
            ReaderAdapter {
                resident,
                fallback: AssetSource::get_default_reader(self.0.to_string_lossy().into_owned())(),
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn installed_snapshot_serves_memory_after_activation_and_blocks_disk_fallback() {
        let fixture = Fixture::new();
        fs::write(fixture.0.join("extra.bin"), b"disk").unwrap();
        for paranoid in [false, true] {
            let diagnostics = Diagnostics::new(paranoid);
            let resident = Resident {
                diagnostics: diagnostics.clone(),
                ..Default::default()
            };
            let reader = fixture.reader(resident);
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

    #[test]
    fn diagnostic_reader_omits_payloads_after_file_or_inventory_failure() {
        let fixture = Fixture::new();
        fs::write(fixture.0.join("bad.bin"), b"broken").unwrap();
        for failure in ["payload", "inventory_json", "inventory_version"] {
            let shared = Shared {
                version: if failure == "inventory_version" {
                    VERSION + 1
                } else {
                    VERSION
                },
                files: BTreeMap::from([(
                    "bad.bin".into(),
                    File {
                        sha256: format!("{:x}", Sha256::digest(b"sample")),
                        bytes: 6,
                        roles: [Role::Data].into(),
                    },
                )]),
            };
            let inventory = if failure == "inventory_json" {
                b"invalid JSON".to_vec()
            } else {
                serde_json::to_vec(&shared).unwrap()
            };
            fs::write(fixture.0.join(SHARED_PATH), inventory).unwrap();
            let diagnostics = Diagnostics::default();
            let files = Files::load_with_diagnostics(
                &fixture.0,
                &[],
                &mut Default::default(),
                || false,
                diagnostics.clone(),
            )
            .unwrap();
            assert!(files.read("bad.bin").is_err());
            assert_eq!(diagnostics.entries().len(), 1);
            let resident = Resident {
                diagnostics: diagnostics.clone(),
                ..Default::default()
            };
            *resident.files.write().unwrap() = Some(Arc::new(files));
            let reader = fixture.reader(resident);
            assert!(
                bevy::tasks::block_on(AssetReader::read(&reader, Path::new("bad.bin"))).is_err()
            );
            assert_eq!(diagnostics.entries().len(), 2);
        }
    }
}
