//! A child process isolates CPU-heavy cooking and captures both output streams.
use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand};
use resonance_importer::ImportProgress;
use std::{
    fs,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        Mutex,
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
};

const PREFIX: &str = "@resonance-progress ";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryStatus {
    Missing,
    Outdated,
    Ready,
}

/// Check the format before attempting to load any game assets. Older layouts
/// may not have the same final-owner files or completion marker as current ones.
pub fn library_status(assets: &Path) -> LibraryStatus {
    if !fs::symlink_metadata(assets).is_ok_and(|m| m.is_dir()) {
        return LibraryStatus::Missing;
    }
    let descriptor = assets.join(resonance_content::library::DESCRIPTOR);
    if !fs::symlink_metadata(&descriptor).is_ok_and(|m| m.is_file()) {
        return LibraryStatus::Missing;
    }
    let valid = fs::read(descriptor)
        .ok()
        .and_then(|bytes| {
            serde_json::from_slice::<resonance_content::library::Descriptor>(&bytes).ok()
        })
        .is_some_and(|descriptor| descriptor.validate().is_ok());
    if !valid {
        return LibraryStatus::Outdated;
    }
    let marker = assets.join(resonance_importer::IMPORT_STATE);
    match fs::symlink_metadata(&marker) {
        Ok(metadata) if !metadata.is_file() => return LibraryStatus::Missing,
        Ok(_) => {
            if !fs::read_to_string(marker).is_ok_and(|state| state == "complete\n") {
                return LibraryStatus::Missing;
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {} // Legacy library.
        Err(_) => return LibraryStatus::Missing,
    }
    // These owners are written in the final publication pass. Their presence
    // also recognizes completed imports from before the marker was introduced.
    if ["movies/mappings/collection.json", "world/world/actors.json"]
        .iter()
        .all(|path| fs::symlink_metadata(assets.join(path)).is_ok_and(|m| m.is_file()))
    {
        LibraryStatus::Ready
    } else {
        LibraryStatus::Missing
    }
}

pub fn imported(assets: &Path) -> bool {
    library_status(assets) == LibraryStatus::Ready
}

pub fn worker_budget() -> usize {
    let available = thread::available_parallelism().map_or(1, |n| n.get());
    // Leave two CPUs for the renderer, input, and desktop on larger machines.
    let default = available
        .saturating_sub(if available >= 4 { 2 } else { 1 })
        .max(1);
    std::env::var("RESONANCE_IMPORT_WORKERS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .map_or(default, |n| n.min(available))
}

#[derive(Parser)]
struct WorkerArgs {
    #[command(subcommand)]
    source: Source,
    #[arg(long)]
    assets: PathBuf,
    #[arg(long)]
    mods: PathBuf,
    #[arg(long)]
    scratch: PathBuf,
}

#[derive(Clone, Subcommand)]
pub enum Source {
    Discs { disc1: PathBuf, disc2: PathBuf },
    Extracted { directory: PathBuf },
}

/// Dispatch before initializing logging, graphics, or the crash monitor.
pub fn worker_command() -> Option<Result<()>> {
    (std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--import-worker"))).then(
        || {
            worker(WorkerArgs::parse_from(
                std::iter::once(std::ffi::OsString::from("resonance-import-worker"))
                    .chain(std::env::args_os().skip(2)),
            ))
        },
    )
}

fn emit(progress: ImportProgress) {
    println!(
        "{PREFIX}{}",
        serde_json::to_string(&progress).expect("serialize import progress")
    );
}

fn worker(args: WorkerArgs) -> Result<()> {
    // Validate both images before extracting either, or cooking any assets.
    if let Source::Discs { disc1, disc2 } = &args.source {
        for (i, disc) in [disc1, disc2].into_iter().enumerate() {
            resonance_importer::validate_disc(disc, i as u8)?;
            let disc = disc.canonicalize()?;
            ensure!(
                !disc.starts_with(&args.assets)
                    && !disc.starts_with(&args.scratch)
                    && !disc.starts_with(&args.mods.join("000100-ghost-wolf")),
                "Disc images must be outside the import destination."
            );
        }
    }
    ensure!(
        !imported(&args.assets),
        "A current imported library already exists at this destination."
    );
    let mod_target = args.mods.join("000100-ghost-wolf");
    ensure!(
        !mod_target.starts_with(&args.assets) && !args.assets.starts_with(&mod_target),
        "Asset and mod destinations must be separate."
    );
    let extracted = match &args.source {
        Source::Extracted { directory } => {
            ensure!(
                resonance_importer::extracted_discs_available(directory),
                "Extracted discs are incomplete or unavailable. Choose both disc images instead."
            );
            directory.clone()
        }
        Source::Discs { disc1, disc2 } => {
            let extracted = args.scratch.join("extracted");
            resonance_importer::extract_disc(disc1, &extracted.join("disc1"), Some(0), &emit)?;
            resonance_importer::extract_disc(disc2, &extracted.join("disc2"), Some(1), &emit)?;
            extracted
        }
    };
    // Full cooks replace the destination. Failure leaves it incomplete; a retry
    // simply replaces the incomplete output.
    resonance_importer::cook_all_with_progress(&extracted, &args.assets, &emit)?;
    emit(ImportProgress::Publishing);
    let marker = args.assets.join(resonance_importer::IMPORT_STATE);
    fs::write(&marker, b"running\n")?;
    if !mod_target.exists() {
        if let Err(error) = resonance_importer::install_ghost_wolf(&args.assets, &mod_target) {
            let _ = fs::remove_dir_all(&mod_target);
            return Err(error).context("install bundled Ghost Wolf mod");
        }
    }
    fs::write(marker, b"complete\n")?;
    emit(ImportProgress::Finished);
    Ok(())
}

pub enum Message {
    Progress(ImportProgress),
    Output(String),
}

pub struct Job {
    child: Child,
    messages: Mutex<Receiver<Message>>,
    output_closed: bool,
    // Owned by the UI process: even cancellation kills/reaps the child before
    // TempDir removes the extracted source files.
    _scratch: tempfile::TempDir,
}

impl Job {
    pub fn start(source: &Source, assets: &Path, mods: &Path, workers: usize) -> Result<Self> {
        let assets = destination(assets)?;
        let mods = destination(mods)?;
        ensure!(
            assets.file_name().is_some(),
            "Choose an asset directory, not a filesystem root"
        );
        let parent = assets.parent().context("asset directory has no parent")?;
        fs::create_dir_all(parent)?;
        let scratch = tempfile::Builder::new()
            .prefix(".resonance-import-")
            .tempdir_in(parent)?;
        let mut command = Command::new(std::env::current_exe()?);
        command
            .arg("--import-worker")
            .arg("--assets")
            .arg(assets)
            .arg("--mods")
            .arg(mods)
            .arg("--scratch")
            .arg(scratch.path());
        match source {
            Source::Discs { disc1, disc2 } => {
                command.arg("discs").arg(disc1).arg(disc2);
            }
            Source::Extracted { directory } => {
                command.arg("extracted").arg(directory);
            }
        }
        let mut child = command
            .env("RESONANCE_IMPORT_WORKERS", workers.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("start background importer")?;
        let (tx, rx) = mpsc::sync_channel(2048);
        read_output(child.stdout.take().unwrap(), tx.clone(), true);
        read_output(child.stderr.take().unwrap(), tx, false);
        Ok(Self {
            child,
            messages: Mutex::new(rx),
            output_closed: false,
            _scratch: scratch,
        })
    }

    pub fn drain(&mut self) -> Vec<Message> {
        let receiver = self.messages.lock().unwrap();
        let mut messages = Vec::new();
        for _ in 0..4096 {
            match receiver.try_recv() {
                Ok(message) => messages.push(message),
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.output_closed = true;
                    break;
                }
                Err(mpsc::TryRecvError::Empty) => break,
            }
        }
        messages
    }
    pub fn status(&mut self) -> std::io::Result<Option<ExitStatus>> {
        // Reap promptly, but retain the readers until both streams reach EOF.
        // In particular, the final error context must not disappear on exit.
        Ok(self.child.try_wait()?.filter(|_| self.output_closed))
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn destination(path: &Path) -> Result<PathBuf> {
    let path = std::path::absolute(path)?;
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => ensure!(
                !metadata.file_type().is_symlink(),
                "Import paths must not contain symbolic links"
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    let name = path
        .file_name()
        .context("Choose an import directory, not a filesystem root")?;
    let parent = path.parent().context("import parent directory")?;
    fs::create_dir_all(parent)?;
    Ok(parent.canonicalize()?.join(name))
}

fn read_output(input: impl Read + Send + 'static, tx: SyncSender<Message>, structured: bool) {
    thread::spawn(move || {
        let mut input = BufReader::new(input);
        loop {
            // Bound individual lines as well as the channel and on-screen log.
            let mut bytes = Vec::new();
            match input.by_ref().take(16 * 1024).read_until(b'\n', &mut bytes) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            let line = String::from_utf8_lossy(&bytes).trim_end().to_owned();
            let event = if structured {
                line.strip_prefix(PREFIX)
                    .and_then(|s| serde_json::from_str(s).ok())
            } else {
                None
            };
            let message = event
                .map(Message::Progress)
                .unwrap_or_else(|| Message::Output(line));
            if tx.send(message).is_err() {
                break;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrong_second_disc_is_rejected_before_extraction_or_destination_changes() -> Result<()> {
        let root = tempfile::tempdir()?;
        let mut header = vec![0u8; 0x8000];
        header[..6].copy_from_slice(b"GQSEAF");
        header[0x1c..0x20].copy_from_slice(&0xC2339F3Du32.to_be_bytes());
        let disc1 = root.path().join("disc1.iso");
        let disc2 = root.path().join("wrong-disc2.iso");
        fs::write(&disc1, &header)?;
        fs::write(&disc2, &header)?;
        resonance_importer::validate_disc(&disc1, 0)?;
        let assets = root.path().join("assets");
        fs::create_dir(&assets)?;
        fs::write(assets.join("keep"), b"previous incomplete import")?;
        let scratch = root.path().join("scratch");
        fs::create_dir(&scratch)?;
        let error = worker(WorkerArgs {
            source: Source::Discs { disc1, disc2 },
            assets: assets.clone(),
            mods: root.path().join("mods"),
            scratch: scratch.clone(),
        })
        .unwrap_err();
        assert!(error.to_string().contains("Expected disc 2, found disc 1"));
        assert_eq!(
            fs::read(assets.join("keep"))?,
            b"previous incomplete import"
        );
        assert_eq!(fs::read_dir(scratch)?.count(), 0);
        Ok(())
    }

    #[test]
    fn readiness_checks_format_legacy_libraries_and_interrupted_cooks() -> Result<()> {
        use resonance_content::library::{GAME, VERSION};
        let root = tempfile::tempdir()?;
        let assets = root.path().join("assets");
        fs::create_dir(&assets)?;
        let descriptor = assets.join("content.json");
        fs::write(
            &descriptor,
            serde_json::to_vec(&serde_json::json!({"version": VERSION - 1, "game": GAME}))?,
        )?;
        assert_eq!(library_status(&assets), LibraryStatus::Outdated);
        assert!(!imported(&assets));
        fs::write(
            &descriptor,
            serde_json::to_vec(&serde_json::json!({"version": VERSION, "game": GAME}))?,
        )?;
        assert!(!imported(&assets));
        for path in ["movies/mappings/collection.json", "world/world/actors.json"] {
            fs::create_dir_all(assets.join(path).parent().unwrap())?;
            fs::write(assets.join(path), b"{}")?;
        }
        assert!(imported(&assets));
        fs::write(assets.join(resonance_importer::IMPORT_STATE), b"running\n")?;
        assert!(!imported(&assets));
        fs::write(assets.join(resonance_importer::IMPORT_STATE), b"complete\n")?;
        assert!(imported(&assets));
        for version in [VERSION - 1, VERSION + 1] {
            fs::write(
                &descriptor,
                serde_json::to_vec(&serde_json::json!({"version": version, "game": GAME}))?,
            )?;
            assert_eq!(library_status(&assets), LibraryStatus::Outdated);
            assert!(
                !imported(&assets),
                "a completion marker cannot override the format version"
            );
        }
        Ok(())
    }

    #[test]
    fn reimport_reuses_extracted_discs_and_only_replaces_cooked_assets() -> Result<()> {
        let root = tempfile::tempdir()?;
        let extracted = root.path().join("local/extracted-discs");
        fs::create_dir_all(extracted.join("disc1/sys"))?;
        fs::create_dir_all(extracted.join("disc1/files"))?;
        fs::write(
            extracted.join("disc1/sys/main.dol"),
            b"incomplete executable fixture",
        )?;
        assert!(!resonance_importer::extracted_discs_available(&extracted));
        for name in [
            "sce_084_090.afs",
            "sce_091_100.afs",
            "sce_101_110.afs",
            "sce_111_120.afs",
            "sce_121_132.afs",
        ] {
            fs::create_dir_all(extracted.join("disc2/files/EV"))?;
            fs::write(extracted.join("disc2/files/EV").join(name), b"fixture")?;
        }
        for name in ["as2.h4m", "as3.h4m", "s09.h4m", "s10.h4m"] {
            fs::create_dir_all(extracted.join("disc2/files/MOV"))?;
            fs::write(extracted.join("disc2/files/MOV").join(name), b"fixture")?;
        }
        assert!(resonance_importer::extracted_discs_available(&extracted));
        let assets = root.path().join("assets");
        fs::create_dir(&assets)?;
        fs::write(assets.join("obsolete.ktx2"), b"old texture")?;
        fs::write(
            assets.join("content.json"),
            serde_json::to_vec(&serde_json::json!({
                "version": resonance_content::library::VERSION - 1,
                "game": resonance_content::library::GAME,
            }))?,
        )?;
        for path in ["movies/mappings/collection.json", "world/world/actors.json"] {
            fs::create_dir_all(assets.join(path).parent().unwrap())?;
            fs::write(assets.join(path), b"{}")?;
        }
        fs::write(assets.join(resonance_importer::IMPORT_STATE), b"complete\n")?;
        assert_eq!(library_status(&assets), LibraryStatus::Outdated);
        let error = worker(WorkerArgs {
            source: Source::Extracted {
                directory: extracted.clone(),
            },
            assets: assets.clone(),
            mods: root.path().join("mods"),
            scratch: root.path().join("unused-scratch"),
        })
        .unwrap_err();
        // The deliberately incomplete executable fails during cooking, proving
        // that the worker reused the source without trying to extract an image.
        assert!(
            error.to_string().contains("game and interface"),
            "{error:#}"
        );
        assert!(!assets.join("obsolete.ktx2").exists());
        assert_eq!(
            fs::read(assets.join(resonance_importer::IMPORT_STATE))?,
            b"running\n"
        );
        assert_eq!(
            fs::read(extracted.join("disc1/sys/main.dol"))?,
            b"incomplete executable fixture"
        );
        assert!(resonance_importer::extracted_discs_available(&extracted));
        assert!(!root.path().join("unused-scratch").exists());
        Ok(())
    }
}
