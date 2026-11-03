use bevy::{
    log::{BoxedLayer, LogPlugin, tracing_subscriber},
    prelude::{App, Resource},
};
use std::{
    backtrace::Backtrace,
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

const MAX_BYTES: u64 = 5 * 1024 * 1024;
const ARCHIVES: usize = 4;
const TAIL_BYTES: u64 = 64 * 1024;
const TEXT_REPORTS: usize = 5;

/// A stable identity shared by text reports, native dumps and the session log.
#[derive(Clone)]
pub struct Reports(Arc<ReportState>);

struct ReportState {
    pub prefix: PathBuf,
    header: String,
    log: Option<FileLog>,
    sequence: AtomicUsize,
    _session: Option<File>,
}

impl Reports {
    pub fn prefix(&self) -> &Path {
        &self.0.prefix
    }
    pub fn directory(&self) -> &Path {
        self.0.prefix.parent().unwrap()
    }
    pub fn header(&self) -> &str {
        &self.0.header
    }

    pub fn error(&self, error: &anyhow::Error) {
        self.write(&format!(
            "Application error: {error:#}\n\nOriginating error backtrace:\n{:#}",
            error.backtrace()
        ));
    }

    fn write(&self, reason: &str) {
        let sequence = self.0.sequence.fetch_add(1, Ordering::Relaxed);
        let path = self.0.prefix.with_extension(format!("{sequence}.txt"));
        // No logging framework or blocking logger lock in a panic hook. A panic
        // inside a formatter must not deadlock trying to acquire its own lock.
        if let Some(log) = &self.0.log
            && let Ok(mut state) = log.0.try_lock()
            && let Some(file) = state.as_mut()
            && file
                .append(format!("\n{reason}\nCrash report: {}\n", path.display()).as_bytes())
                .is_err()
        {
            // Rotation closes the active file first. Disable a failed writer
            // here just as the normal logging path does, so a later event
            // cannot turn a disk error into a second panic.
            *state = None;
        }
        match write_report(
            self.prefix(),
            &format!("{sequence}.txt"),
            self.header(),
            reason,
            &resonance_presentation::crash_context(),
        ) {
            Ok(_) => eprintln!("Crash report saved to {}", path.display()),
            Err(error) => eprintln!("Could not write crash report {}: {error}", path.display()),
        }
    }
}

/// Common artifact lifecycle for panic, application-error and native reports.
pub fn write_report(
    prefix: &Path,
    extension: &str,
    header: &str,
    reason: &str,
    context: &str,
) -> io::Result<PathBuf> {
    let path = prefix.with_extension(extension);
    let mut file = private_file(&path)?;
    writeln!(
        file,
        "{header}\nTime: {}\n{reason}\n\nLatest game context (sampled before the failure):\n{context}\n\nRecent log:\n{}",
        time::OffsetDateTime::now_utc(),
        log_tail(prefix.parent().unwrap_or(Path::new(".")))
    )?;
    file.sync_all()?;
    prune_reports(prefix);
    Ok(path)
}

pub fn private_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

/// Keep an active session alive across both the game and its crash monitor.
/// Cleanup uses an exclusive lock, so it cannot remove either process's files.
pub fn hold_session(directory: &Path) -> io::Result<File> {
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(directory.join(".lock"))?;
    lock.try_lock_shared().map_err(io::Error::other)?;
    Ok(lock)
}

fn create_session(root: &Path, directory: &Path) -> io::Result<File> {
    fs::create_dir_all(root)?;
    // Publish the directory only after holding its lock. A simultaneous launch
    // can never mistake a partially initialized session for a completed one.
    let staging = tempfile::Builder::new()
        .prefix(".session-")
        .tempdir_in(root)?;
    let lock = private_file(&staging.path().join(".lock"))?;
    lock.try_lock_shared().map_err(io::Error::other)?;
    fs::rename(staging.path(), directory)?;
    let _ = staging.keep();
    Ok(lock)
}

fn is_session(name: &std::ffi::OsStr) -> bool {
    name.to_str()
        .and_then(|name| name.strip_prefix("session-"))
        .is_some_and(|identity| {
            identity.split('-').count() == 2
                && identity
                    .split('-')
                    .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        })
}

/// Keep five completed crash sessions and five completed healthy sessions.
/// Active sessions and directories we did not create are never removed.
fn prune_sessions(current: &Path) {
    if !current.file_name().is_some_and(is_session) {
        return;
    }
    let Some(root) = current.parent() else { return };
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    let mut sessions = [Vec::new(), Vec::new()];
    for entry in entries.flatten() {
        if !is_session(&entry.file_name())
            || entry.path() == current
            || !entry.file_type().is_ok_and(|kind| kind.is_dir())
        {
            continue;
        }
        let path = entry.path();
        let Ok(lock) = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path.join(".lock"))
        else {
            continue;
        };
        if lock.try_lock().is_err() {
            continue;
        }
        let crashed = fs::read_dir(&path).is_ok_and(|files| {
            files.flatten().any(|file| {
                file.file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with("crash."))
            })
        });
        sessions[usize::from(crashed)].push((entry.file_name(), path, lock));
    }
    for group in &mut sessions {
        group.sort_unstable_by(|left, right| left.0.cmp(&right.0));
        let excess = group.len().saturating_sub(5);
        for (_, path, lock) in group.drain(..excess) {
            // Windows must release our handle before removing the directory.
            // Completed session directories are never reopened by a new game.
            drop(lock);
            let _ = fs::remove_dir_all(path);
        }
    }
}

/// Keep the latest five panic/error reports within this session.
pub fn prune_reports(prefix: &Path) {
    let Some(directory) = prefix.parent() else {
        return;
    };
    if let Ok(entries) = fs::read_dir(directory) {
        let mut reports: Vec<_> = entries
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name();
                let sequence = name
                    .to_str()?
                    .strip_prefix("crash.")?
                    .strip_suffix(".txt")?
                    .parse::<usize>()
                    .ok()?;
                Some((sequence, entry.path()))
            })
            .collect();
        reports.sort_unstable_by_key(|(sequence, _)| *sequence);
        for (_, path) in reports
            .iter()
            .take(reports.len().saturating_sub(TEXT_REPORTS))
        {
            let _ = fs::remove_file(path);
        }
    }
    prune_sessions(directory);
}

pub fn log_tail(directory: &Path) -> String {
    // Include the most recent archive when a crash occurs just after rotation.
    let mut tail = Vec::new();
    for name in ["resonance.1.log", "resonance.log"] {
        if let Ok(mut file) = File::open(directory.join(name)) {
            let length = file.metadata().map_or(0, |m| m.len());
            let _ = file.seek(SeekFrom::Start(length.saturating_sub(TAIL_BYTES)));
            let _ = file.take(TAIL_BYTES).read_to_end(&mut tail);
        }
    }
    String::from_utf8_lossy(&tail[tail.len().saturating_sub(TAIL_BYTES as usize)..]).into_owned()
}

pub fn init() -> Reports {
    let fallback = resonance_persistence::default_directory()
        .ok()
        .map(|root| root.join("logs"));
    init_with_fallback(Path::new("logs"), fallback.as_deref())
}

#[cfg(windows)]
fn executable_image_base() -> usize {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetModuleHandleW(name: *const u16) -> *mut std::ffi::c_void;
    }
    // SAFETY: NULL requests the already-loaded executable; no handle ownership
    // is transferred. Save the ASLR base before a panic for offline PDB lookup.
    unsafe { GetModuleHandleW(std::ptr::null()) as usize }
}

#[cfg(target_os = "macos")]
fn executable_image_base() -> usize {
    unsafe extern "C" {
        fn _dyld_get_image_header(index: u32) -> *const std::ffi::c_void;
    }
    // SAFETY: dyld image zero is the main executable; the returned header stays
    // loaded for the process lifetime. This is the ASLR load address for atos.
    unsafe { _dyld_get_image_header(0) as usize }
}

#[cfg(target_os = "linux")]
fn executable_image_base() -> usize {
    unsafe extern "C" fn find_main(
        info: *mut libc::dl_phdr_info,
        _: usize,
        result: *mut std::ffi::c_void,
    ) -> libc::c_int {
        // SAFETY: dl_iterate_phdr supplies a valid header and our stack-local
        // result pointer. The main executable has an empty loader name.
        unsafe {
            let info = &*info;
            if info.dlpi_name.is_null() || *info.dlpi_name == 0 {
                *result.cast::<usize>() = info.dlpi_addr as usize;
                return 1;
            }
        }
        0
    }
    let mut base = 0usize;
    // SAFETY: The callback does not retain pointers or unwind. This runs once
    // at startup, saving the ELF load bias for offline stripped-backtrace lookup.
    unsafe { libc::dl_iterate_phdr(Some(find_main), std::ptr::from_mut(&mut base).cast()) };
    base
}

pub(super) fn init_at(root: &Path) -> Reports {
    init_with_fallback(root, None)
}

fn open_session(root: &Path, name: &str) -> io::Result<(PathBuf, File, FileLog)> {
    let directory = root.join(name);
    let lock = create_session(root, &directory)?;
    let log = match RollingFile::open(&directory, MAX_BYTES, ARCHIVES) {
        Ok(log) => log,
        Err(error) => {
            drop(lock);
            let _ = fs::remove_dir_all(&directory);
            return Err(error);
        }
    };
    Ok((directory, lock, FileLog(Arc::new(Mutex::new(Some(log))))))
}

fn init_with_fallback(root: &Path, fallback: Option<&Path>) -> Reports {
    let now = time::OffsetDateTime::now_utc();
    let name = format!(
        "session-{:020}-{}",
        now.unix_timestamp_nanos(),
        std::process::id()
    );
    let mut opened = None;
    for candidate in std::iter::once(root).chain(fallback.filter(|path| *path != root)) {
        match open_session(candidate, &name) {
            Ok(session) => {
                if candidate != root {
                    eprintln!(
                        "Using fallback crash-report directory: {}",
                        session.0.display()
                    );
                }
                opened = Some(session);
                break;
            }
            Err(error) => {
                eprintln!("Could not open session in {}: {error}", candidate.display());
            }
        }
    }
    let (directory, session, log) = match opened {
        Some((directory, lock, log)) => (directory, Some(lock), Some(log)),
        None => {
            eprintln!("No writable crash-report directory; continuing with terminal logging");
            (root.join(name), None, None)
        }
    };
    prune_sessions(&directory);
    let mut app = App::new();
    if let Some(log) = &log {
        app.insert_resource(log.clone());
    }
    app.add_plugins(LogPlugin {
        custom_layer: file_layer,
        ..Default::default()
    });
    let prefix = directory.join("crash");
    let debug_id = crate::crash_dumps::executable_debug_id()
        .unwrap_or_else(|error| format!("unavailable ({error:#})"));
    let header = format!(
        "Resonance {}\nRevision: {}\nSource: {}\nCompiler: {}\nTarget: {}; profile: {}\nOS: {} {}; CPU threads: {:?}\nSession: {}\nExecutable: {:?}\nExecutable debug ID: {debug_id}\nWorking directory: {:?}\nArguments: {:?}\nGraphics/log overrides: {:?}\n",
        env!("CARGO_PKG_VERSION"),
        env!("RESONANCE_REVISION"),
        env!("RESONANCE_SOURCE"),
        env!("RESONANCE_RUSTC"),
        env!("RESONANCE_TARGET"),
        env!("RESONANCE_PROFILE"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::thread::available_parallelism(),
        directory.display(),
        std::env::current_exe(),
        std::env::current_dir(),
        std::env::args_os().collect::<Vec<_>>(),
        [
            "RUST_LOG",
            "WGPU_BACKEND",
            "WGPU_ADAPTER_NAME",
            "WGPU_FORCE_FALLBACK_ADAPTER",
            "XDG_SESSION_TYPE"
        ]
        .map(|key| (key, std::env::var_os(key)))
    );
    #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
    let header = format!(
        "{header}Executable image base: {:#x}\n",
        executable_image_base()
    );
    let reports = Reports(Arc::new(ReportState {
        prefix,
        header,
        log,
        sequence: AtomicUsize::new(0),
        _session: session,
    }));
    let previous = std::panic::take_hook();
    let panic_reports = reports.clone();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        panic_reports.write(&format!(
            "Rust panic on thread '{}' ({:?}): {info}\nBacktrace:\n{:#}",
            thread.name().unwrap_or("unnamed"),
            thread.id(),
            Backtrace::force_capture()
        ));
        previous(info);
    }));
    bevy::log::info!(
        "Starting session in {}\n{}",
        reports.directory().display(),
        reports.header()
    );
    reports
}

fn file_layer(app: &mut App) -> Option<BoxedLayer> {
    let log = app.world().get_resource::<FileLog>()?.clone();
    Some(Box::new(
        tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_thread_names(true)
            .with_thread_ids(true)
            .with_file(true)
            .with_line_number(true)
            .with_writer(move || log.clone()),
    ))
}

#[derive(Clone, Resource)]
struct FileLog(Arc<Mutex<Option<RollingFile>>>);

impl Write for FileLog {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(file) = state.as_mut()
            && let Err(error) = file.append(bytes)
        {
            // Logging failures must not take down the game or recursively log.
            eprintln!("Could not write game log: {error}; continuing with terminal logging");
            *state = None;
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        // File writes go straight to the OS; there is no userspace buffer.
        Ok(())
    }
}

struct RollingFile {
    directory: PathBuf,
    file: Option<File>,
    bytes: u64,
    max_bytes: u64,
    archives: usize,
}

impl RollingFile {
    fn open(directory: &Path, max_bytes: u64, archives: usize) -> io::Result<Self> {
        assert!(max_bytes > 0 && archives > 0);
        fs::create_dir_all(directory)?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(directory.join("resonance.log"))?;
        let bytes = file.metadata()?.len();
        Ok(Self {
            directory: directory.to_owned(),
            file: Some(file),
            bytes,
            max_bytes,
            archives,
        })
    }

    fn path(&self, archive: usize) -> PathBuf {
        self.directory.join(if archive == 0 {
            "resonance.log".to_owned()
        } else {
            format!("resonance.{archive}.log")
        })
    }

    fn rotate(&mut self) -> io::Result<()> {
        // Close before renaming, including on Windows.
        self.file.take();
        ignore_missing(fs::remove_file(self.path(self.archives)))?;
        for archive in (0..self.archives).rev() {
            ignore_missing(fs::rename(self.path(archive), self.path(archive + 1)))?;
        }
        self.file = Some(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.path(0))?,
        );
        self.bytes = 0;
        Ok(())
    }

    fn append(&mut self, mut bytes: &[u8]) -> io::Result<()> {
        // Keep ordinary events intact across rotation. Only oversized events
        // need to be split between files.
        if bytes.len() as u64 <= self.max_bytes
            && bytes.len() as u64 > self.max_bytes.saturating_sub(self.bytes)
        {
            self.rotate()?;
        }
        while !bytes.is_empty() {
            if self.bytes >= self.max_bytes {
                self.rotate()?;
            }
            // Split even unusually large events to keep the disk limit bounded.
            let count = (self.max_bytes - self.bytes).min(bytes.len() as u64) as usize;
            self.file
                .as_mut()
                .ok_or_else(|| io::Error::other("log file closed after rotation failure"))?
                .write_all(&bytes[..count])?;
            self.bytes += count as u64;
            bytes = &bytes[count..];
        }
        Ok(())
    }
}

fn ignore_missing(result: io::Result<()>) -> io::Result<()> {
    match result {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

#[cfg(test)]
mod tests;
