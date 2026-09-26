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
    pub late_reads: Arc<AtomicU64>,
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
            late_reads: Default::default(),
        }
    }
}
#[derive(Default)]
pub(super) struct Cache {
    pub bytes: resonance_content::prepared::Cache,
    pub audio: super::field_audio::Cache,
    pub scripts: Option<resonance_game::authored::FieldScripts>,
    pub battle_scripts: symphonia_script_tools::PreparationCache,
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
        let (bytes, prepared, rejected) = {
            let files = self.resident.files.read().unwrap();
            (
                files
                    .as_ref()
                    .and_then(|f| f.bytes.get(key.as_ref()).cloned()),
                files.is_some(),
                files.as_ref().is_some_and(|f| f.is_rejected(&key)),
            )
        };
        if let Some(bytes) = bytes {
            self.resident.memory_reads.fetch_add(1, Ordering::Relaxed);
            if self.resident.active.load(Ordering::Acquire) {
                self.resident.late_reads.fetch_add(1, Ordering::Relaxed);
                self.resident
                    .diagnostics
                    .report(
                        "asset reader",
                        anyhow::anyhow!(
                            "late scene asset read after activation: {}",
                            path.display()
                        ),
                    )
                    .map_err(asset_error)?;
            }
            return Ok(Box::new(VecReader::new(bytes.to_vec())) as Box<dyn Reader>);
        }
        if prepared {
            self.resident.late_reads.fetch_add(1, Ordering::Relaxed);
            if rejected {
                // The verifier already diagnosed this payload. Let the consumer
                // substitute or omit it rather than bypassing its rejection.
                return Err(asset_error(anyhow::anyhow!(
                    "rejected scene asset {}",
                    path.display()
                )));
            }
            self.resident
                .diagnostics
                .report(
                    "asset reader",
                    anyhow::anyhow!(
                        "undeclared scene asset {}; reading from disk",
                        path.display()
                    ),
                )
                .map_err(asset_error)?;
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
/// One worker publishes audio readiness before continuing visual preparation.
/// The final package shares this same bank; Playback remains the only owner.
pub(super) struct BattlePending {
    task: Task<super::battle::Package>,
    audio: Mutex<Option<mpsc::Receiver<BattleAudio>>>,
}
pub(super) struct BattleAudio {
    pub assets: Arc<super::battle_audio::Assets>,
    pub settings: super::battle_audio::Settings,
    pub track: u16,
}
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
        let diagnostics = resident.diagnostics.clone();
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
            let files = Arc::new(Files::load_with_diagnostics(
                &root,
                &paths.iter().map(String::as_str).collect::<Vec<_>>(),
                &mut cache.bytes,
                || stop.load(Ordering::Relaxed),
                diagnostics,
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
impl BattlePending {
    pub fn battle(root: PathBuf, entry: super::battle::Entry, resident: &Resident) -> Result<Self> {
        let cache = resident.cache.clone();
        let (send_audio, receive_audio) = mpsc::sync_channel(1);
        let task = Task::spawn(move |stop| {
            let super::battle::Entry {
                files: retained,
                party,
                setup,
                options,
                data,
                libc_seed,
            } = entry;
            let mut cache = cache.lock().unwrap();
            let menus = Arc::new(
                retained.json::<resonance_content::menu_data::MenuData>("game/menu-data.json")?,
            );
            // Main mode9 starts its selected song before loading the battle REL.
            // Publish the verified bank without waiting for models or GPU work.
            let (files, descriptor) = resonance_game::battle::audio::prepare(
                &root,
                (*retained).clone(),
                &mut cache.bytes,
                || stop.load(Ordering::Relaxed),
            )?;
            let audio = super::battle_audio::Assets::load(&files, &mut cache.audio)?;
            let settings = &party.settings.preferences;
            send_audio
                .send(BattleAudio {
                    assets: audio.clone(),
                    settings: super::battle_audio::Settings {
                        music: settings.volumes.music,
                        effects: settings.volumes.effects,
                        battle_effects: settings.volumes.battle_effects,
                        voice: settings.volumes.battle_voice,
                        stereo: settings.stereo,
                    },
                    track: resonance_game::battle::entry::music(
                        &setup,
                        u32::from(options.map),
                        options.world_music,
                    ),
                })
                .context("battle audio handoff cancelled")?;
            let inputs = resonance_game::battle::encounter::Inputs::load(
                &root,
                &files,
                &menus,
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
            let prepared = assets.prepare(
                &menus,
                &party,
                options,
                &mut cache.battle_scripts,
                |sound| audio.bind(sound),
            )?;
            let catalogue = Arc::new(assets.files.json(resonance_content::arte::PATH)?);
            Ok(super::battle::Package {
                assets: Arc::new(assets),
                prepared,
                audio,
                party,
                menus,
                data,
                catalogue,
                libc_seed,
                entry_seed,
            })
        })?;
        Ok(Self {
            task,
            audio: Mutex::new(Some(receive_audio)),
        })
    }
    pub fn poll_audio(&self) -> Option<BattleAudio> {
        let mut receiver = self.audio.lock().unwrap();
        let result = receiver.as_ref()?.try_recv();
        match result {
            Ok(audio) => {
                *receiver = None;
                Some(audio)
            }
            Err(mpsc::TryRecvError::Empty) => None,
            // The final task carries the original preparation error.
            Err(mpsc::TryRecvError::Disconnected) => {
                *receiver = None;
                None
            }
        }
    }
    pub fn poll(&self) -> Result<Option<Result<super::battle::Package>>> {
        // The worker may finish between the host's two polls. Do not expose a
        // Scene until its preceding audio handoff has been consumed.
        if self.audio.lock().unwrap().is_some() {
            return Ok(None);
        }
        self.task.poll()
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
    let battle_visible = battle.is_some_and(|battle| battle.failed() || battle.owns_presentation());
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
        field_preload::{File, Role},
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
    fn diagnostic_reader_allows_late_verified_and_undeclared_disk_reads_while_paranoid_rejects() {
        let fixture = Fixture::new();
        fs::write(fixture.0.join("extra.bin"), b"disk").unwrap();
        for paranoid in [false, true] {
            let diagnostics = Diagnostics::new(paranoid);
            let resident = Resident {
                diagnostics: diagnostics.clone(),
                ..Default::default()
            };
            let mut files = Files::default();
            files
                .bytes
                .insert("prepared.bin".into(), Arc::from(b"verified".as_slice()));
            *resident.files.write().unwrap() = Some(Arc::new(files));
            resident.active.store(true, Ordering::Release);
            let reader = fixture.reader(resident);
            for (path, expected) in [
                ("prepared.bin", b"verified".as_slice()),
                ("extra.bin", b"disk"),
            ] {
                let result = bevy::tasks::block_on(AssetReader::read(&reader, Path::new(path)));
                if paranoid {
                    assert!(result.is_err());
                } else {
                    let mut bytes = Vec::new();
                    bevy::tasks::block_on(result.unwrap().read_to_end(&mut bytes)).unwrap();
                    assert_eq!(bytes, expected);
                }
            }
            assert_eq!(diagnostics.entries().len(), 2);
            assert!(
                bevy::tasks::block_on(AssetReader::read(&reader, Path::new("../extra.bin")))
                    .is_err()
            );
        }
    }

    #[test]
    fn diagnostic_reader_never_reopens_a_payload_rejected_by_verification() {
        let fixture = Fixture::new();
        fs::write(fixture.0.join("bad.bin"), b"broken").unwrap();
        let diagnostics = Diagnostics::default();
        let mut cache = Default::default();
        let files = Files::load_with_diagnostics(
            &fixture.0,
            &[],
            &mut cache,
            || false,
            diagnostics.clone(),
        )
        .unwrap()
        .with_dependencies(
            &fixture.0,
            BTreeMap::from([(
                "bad.bin".into(),
                File {
                    sha256: format!("{:x}", Sha256::digest(b"sample")),
                    bytes: 6,
                    roles: [Role::Data].into(),
                },
            )]),
            &mut cache,
            || false,
        )
        .unwrap();
        let resident = Resident {
            diagnostics: diagnostics.clone(),
            ..Default::default()
        };
        *resident.files.write().unwrap() = Some(Arc::new(files));
        let reader = fixture.reader(resident);
        assert!(bevy::tasks::block_on(AssetReader::read(&reader, Path::new("bad.bin"))).is_err());
        assert_eq!(diagnostics.entries().len(), 1);
    }

    #[test]
    #[ignore = "requires current cooked opening assets; no GPU or audio device"]
    fn battle_audio_is_published_before_later_model_preparation_failure() -> Result<()> {
        let root = std::env::var_os("RESONANCE_TEST_ASSETS").map_or_else(
            || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets"),
            PathBuf::from,
        );
        let mut cache = Default::default();
        let files = Files::load(&root, &["fields/map-332.preload.json"], &mut cache, || {
            false
        })?;
        // Retain real field descriptors, but provide only the audio closure on
        // disk. The later encounter model verification must fail independently.
        let saved: resonance_game::field::FieldCheckpoint =
            serde_json::from_slice(&std::fs::read(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../local/battle-rewrite/stage3-opening-checkpoint.json"),
            )?)?;
        let mut party = saved.progress.party;
        party.battles.previous_formation = Some(0);
        let data = Arc::new(files.json("game/session-data.json")?);
        let fixture = Fixture::new();
        let descriptor: resonance_content::battle_audio::Audio =
            files.json(resonance_content::battle_audio::PATH)?;
        for path in descriptor.files.keys() {
            let target = fixture.0.join(path);
            std::fs::create_dir_all(target.parent().unwrap())?;
            std::fs::copy(root.join(path), target)?;
        }
        let task = BattlePending::battle(
            fixture.0.clone(),
            super::super::battle::Entry {
                files: Arc::new(files),
                party,
                setup: resonance_events::battle::Setup {
                    encounter: 1,
                    arena: 0,
                    music: None,
                    defeat: resonance_events::battle::DefeatPolicy::GameOver,
                },
                options: resonance_game::battle::encounter::PrepareOptions {
                    random_seed: 55023,
                    map: 332,
                    world_music: 0,
                    story: 3000,
                    overlimit_boost: false,
                },
                data,
                libc_seed: 1,
            },
            &Resident::default(),
        )?;
        let start = Instant::now();
        let audio = loop {
            if let Some(audio) = task.poll_audio() {
                break audio;
            }
            anyhow::ensure!(start.elapsed().as_secs() < 120, "audio handoff timed out");
            std::thread::sleep(std::time::Duration::from_millis(1));
        };
        assert_eq!(audio.track, 85);
        assert!(audio.assets.music_ready(85));
        assert!(
            task.poll_audio().is_none(),
            "audio handoff must be consumed once"
        );
        loop {
            if let Some(result) = task.poll()? {
                assert!(
                    result.is_err(),
                    "audio-only root must reject remaining model assets"
                );
                break;
            }
            anyhow::ensure!(start.elapsed().as_secs() < 120, "model failure timed out");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        Ok(())
    }
}
