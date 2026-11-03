use super::*;

#[test]
fn unwritable_primary_uses_a_complete_session_in_the_fallback_directory() {
    const CHILD: &str = "RESONANCE_LOG_FALLBACK_TEST";
    if let Some(root) = std::env::var_os(CHILD) {
        let root = Path::new(&root);
        let blocked = root.join("blocked");
        fs::write(&blocked, "a file cannot contain a logs directory").unwrap();
        let fallback = root.join("fallback");
        let reports = init_with_fallback(&blocked.join("logs"), Some(&fallback));
        assert_eq!(reports.directory().parent(), Some(fallback.as_path()));
        assert!(hold_session(reports.directory()).is_ok());
        bevy::log::info!("fallback session breadcrumb");
        reports.error(&anyhow::anyhow!("startup failure in fallback session"));
        assert!(
            fs::read_to_string(reports.prefix().with_extension("0.txt"))
                .unwrap()
                .contains("startup failure in fallback session")
        );
        assert!(
            fs::read_to_string(reports.directory().join("resonance.log"))
                .unwrap()
                .contains("fallback session breadcrumb")
        );
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "logging::tests::unwritable_primary_uses_a_complete_session_in_the_fallback_directory",
            "--nocapture",
        ])
        .env(CHILD, directory.path())
        .env("RUST_LOG", "info")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("Using fallback crash-report directory")
    );
    assert_eq!(
        fs::read(directory.path().join("blocked")).unwrap(),
        b"a file cannot contain a logs directory"
    );
}

fn session(root: &Path) -> PathBuf {
    fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.file_name().is_some_and(is_session))
        .unwrap()
}

#[test]
fn application_error_reports_keep_the_originating_stack() {
    const CHILD: &str = "RESONANCE_ERROR_BACKTRACE_TEST";
    if let Some(directory) = std::env::var_os(CHILD) {
        #[inline(never)]
        fn originating_error() -> anyhow::Error {
            anyhow::anyhow!("missing content").context("opening scene")
        }
        init_at(Path::new(&directory)).error(&originating_error());
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "logging::tests::application_error_reports_keep_the_originating_stack",
        ])
        .env(CHILD, directory.path())
        .env("RUST_LIB_BACKTRACE", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let path = fs::read_dir(session(directory.path()))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|extension| extension == "txt"))
        .unwrap();
    let report = fs::read_to_string(path).unwrap();
    assert!(
        report.contains("opening scene: missing content"),
        "{report}"
    );
    // Windows PDBs are kept by the team, not bundled with the game. Wine's
    // DbgHelp can also miss private symbols even with the PDB present.
    #[cfg(not(windows))]
    assert!(report.contains("originating_error"), "{report}");
    assert!(
        report.lines().any(|line| line
            .trim_start()
            .strip_prefix("0:")
            .is_some_and(|frame| frame.trim_start().starts_with("0x"))),
        "raw frame addresses are required for offline lookup: {report}"
    );
    #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
    assert!(report.contains("Executable image base: 0x"), "{report}");
    #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
    assert!(
        report.lines().any(|line| line
            .strip_prefix("Executable debug ID: ")
            .is_some_and(|id| debugid::DebugId::from_breakpad(id).is_ok())),
        "native debug identity is required for offline symbol matching: {report}"
    );
    assert!(!report.contains("disabled backtrace"), "{report}");
    assert!(!report.contains("reporting boundary"), "{report}");
}

#[test]
fn appends_across_launches_and_rotates_with_bounded_retention() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path();
    {
        let mut log = RollingFile::open(path, 4, 2).unwrap();
        log.append(b"ab").unwrap();
    }
    let mut log = RollingFile::open(path, 4, 2).unwrap();
    log.append(b"cdefghijklmnopq").unwrap();
    assert_eq!(fs::read(path.join("resonance.log")).unwrap(), b"q");
    assert_eq!(fs::read(path.join("resonance.1.log")).unwrap(), b"mnop");
    assert_eq!(fs::read(path.join("resonance.2.log")).unwrap(), b"ijkl");
    assert_eq!(fs::read_dir(path).unwrap().count(), 3);
}

#[test]
fn rotation_preserves_unrelated_files() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("other.log"), "keep me").unwrap();
    let mut log = RollingFile::open(directory.path(), 4, 1).unwrap();
    log.append(b"123456789").unwrap();
    assert_eq!(
        fs::read_to_string(directory.path().join("other.log")).unwrap(),
        "keep me"
    );
}

#[test]
fn rotates_before_an_event_that_would_cross_the_limit() {
    let directory = tempfile::tempdir().unwrap();
    let mut log = RollingFile::open(directory.path(), 8, 1).unwrap();
    log.append(b"first\n").unwrap();
    log.append(b"second\n").unwrap();
    assert_eq!(fs::read(log.path(0)).unwrap(), b"second\n");
    assert_eq!(fs::read(log.path(1)).unwrap(), b"first\n");
}

#[test]
fn write_failure_disables_file_logging_without_panicking() {
    let directory = tempfile::tempdir().unwrap();
    let log = RollingFile::open(directory.path(), 4, 1).unwrap();
    // A directory at the rotation destination makes rotation fail.
    fs::create_dir(directory.path().join("resonance.1.log")).unwrap();
    let mut writer = FileLog(Arc::new(Mutex::new(Some(log))));
    writer.write_all(b"12345").unwrap();
    assert!(writer.0.lock().unwrap().is_none());
    writer.write_all(b"still running").unwrap();
}

#[test]
fn published_session_is_locked_for_both_game_and_monitor() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("session-00000000000000000001-1");
    let game = create_session(root.path(), &directory).unwrap();
    let monitor = hold_session(&directory).unwrap();
    let cleanup = OpenOptions::new()
        .read(true)
        .write(true)
        .open(directory.join(".lock"))
        .unwrap();
    assert!(cleanup.try_lock().is_err());
    drop(game);
    assert!(cleanup.try_lock().is_err());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
    drop(monitor);
}

#[test]
fn session_logs_are_isolated_during_rotation() {
    let root = tempfile::tempdir().unwrap();
    let first = root.path().join("session-00000000000000000001-1");
    let second = root.path().join("session-00000000000000000002-2");
    let _first = create_session(root.path(), &first).unwrap();
    let _second = create_session(root.path(), &second).unwrap();
    let mut first_log = RollingFile::open(&first, 4, 4).unwrap();
    let mut second_log = RollingFile::open(&second, 4, 4).unwrap();
    for event in [b"aaaa", b"bbbb", b"cccc", b"dddd", b"eeee", b"ffff"] {
        first_log.append(event).unwrap();
    }
    second_log.append(b"BUG!").unwrap();
    assert_eq!(fs::read(second.join("resonance.log")).unwrap(), b"BUG!");
    assert_eq!(fs::read(first.join("resonance.log")).unwrap(), b"ffff");
}

#[test]
fn retention_preserves_crashes_healthy_sessions_and_live_monitors_separately() {
    // Other tests fork subprocesses in parallel. Until exec closes their
    // inherited descriptors, they can briefly keep our flock locks alive.
    // Create the locks in a dedicated process with no concurrent forks.
    const CHILD: &str = "RESONANCE_RETENTION_TEST";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "logging::tests::retention_preserves_crashes_healthy_sessions_and_live_monitors_separately", "--nocapture"])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let root = tempfile::tempdir().unwrap();
    for timestamp in 0..8 {
        for crashed in [false, true] {
            let directory = root.path().join(format!(
                "session-{timestamp:020}-{}",
                usize::from(crashed) + 1
            ));
            drop(create_session(root.path(), &directory).unwrap());
            fs::write(
                directory.join(if crashed {
                    "crash.native.txt"
                } else {
                    "resonance.log"
                }),
                "keep",
            )
            .unwrap();
        }
    }
    let live = root.path().join("session-00000000000000000000-3");
    let game = create_session(root.path(), &live).unwrap();
    let monitor = hold_session(&live).unwrap();
    drop(game); // A crash monitor may outlive its game.
    let current = root.path().join("session-00000000000000000009-1");
    let _current = create_session(root.path(), &current).unwrap();
    let unrelated = root.path().join("session-00000000000000000000-99");
    fs::create_dir(&unrelated).unwrap(); // No ownership/lock file.
    fs::write(root.path().join("old-crash.txt"), "keep").unwrap();
    prune_sessions(&current);
    for timestamp in 0..8 {
        for crashed in [false, true] {
            assert_eq!(
                root.path()
                    .join(format!(
                        "session-{timestamp:020}-{}",
                        usize::from(crashed) + 1
                    ))
                    .exists(),
                timestamp >= 3
            );
        }
    }
    assert!(live.exists() && unrelated.exists() && root.path().join("old-crash.txt").exists());
    drop(monitor);
    prune_sessions(&current);
    assert!(!live.exists());
}

#[test]
fn crash_reporting_disables_a_failed_rotation_and_still_saves_text() {
    let directory = tempfile::tempdir().unwrap();
    let log = FileLog(Arc::new(Mutex::new(Some(
        RollingFile::open(directory.path(), 4, 1).unwrap(),
    ))));
    fs::create_dir(directory.path().join("resonance.1.log")).unwrap();
    let reports = Reports(Arc::new(ReportState {
        prefix: directory.path().join("crash"),
        header: "test build".into(),
        log: Some(log.clone()),
        sequence: AtomicUsize::new(0),
        _session: None,
    }));
    reports.error(&anyhow::anyhow!("original crash"));
    assert!(log.0.lock().unwrap().is_none());
    log.clone().write_all(b"later log event").unwrap();
    assert!(
        fs::read_to_string(reports.prefix().with_extension("0.txt"))
            .unwrap()
            .contains("original crash")
    );
}

#[test]
fn returned_errors_keep_causal_chain_and_reports_are_bounded() {
    let directory = tempfile::tempdir().unwrap();
    let reports = Reports(Arc::new(ReportState {
        prefix: directory.path().join("crash"),
        header: "test build".into(),
        log: None,
        sequence: AtomicUsize::new(0),
        _session: None,
    }));
    let error = anyhow::anyhow!("bad texture")
        .context("field 332")
        .context("opening scene");
    for _ in 0..12 {
        reports.error(&error);
    }
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 5);
    for sequence in 0..12 {
        assert_eq!(
            reports
                .prefix()
                .with_extension(format!("{sequence}.txt"))
                .exists(),
            sequence >= 7
        );
    }
    let path = reports.prefix().with_extension("11.txt");
    let report = fs::read_to_string(&path).unwrap();
    assert!(
        report.contains("opening scene: field 332: bad texture"),
        "{report}"
    );
    assert!(report.contains("Originating error backtrace:"), "{report}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn panic_does_not_deadlock_when_logger_is_locked() {
    const CHILD: &str = "RESONANCE_LOCKED_LOGGER_TEST";
    if let Some(path) = std::env::var_os(CHILD) {
        let reports = init_at(Path::new(&path));
        let _guard = reports.0.log.as_ref().unwrap().0.lock().unwrap();
        panic!("panic while logger is locked");
    }
    let directory = tempfile::tempdir().unwrap();
    let stderr = File::create(directory.path().join("stderr")).unwrap();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "logging::tests::panic_does_not_deadlock_when_logger_is_locked",
            "--nocapture",
        ])
        .env(CHILD, directory.path())
        .stdout(std::process::Stdio::null())
        .stderr(stderr)
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(!status.success());
            break;
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("panic reporter deadlocked on logger");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let report = fs::read_dir(session(directory.path()))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.to_string_lossy().ends_with(".0.txt"))
        .unwrap();
    assert!(
        fs::read_to_string(report)
            .unwrap()
            .contains("panic while logger is locked")
    );
}

#[test]
fn events_and_panics_are_persisted_with_filtering() {
    const CHILD: &str = "RESONANCE_PANIC_TEST_DIRECTORY";
    if let Some(directory) = std::env::var_os(CHILD) {
        init_at(Path::new(&directory));
        bevy::log::info!("pre-crash event");
        bevy::log::debug!("filtered debug event");
        for sequence in 0..5 {
            let _ = std::panic::catch_unwind(|| panic!("earlier caught panic {sequence}"));
        }
        panic!("deliberate crash log test");
    }
    for filter in ["info", "off"] {
        let directory = tempfile::tempdir().unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "logging::tests::events_and_panics_are_persisted_with_filtering",
                "--nocapture",
            ])
            .env(CHILD, directory.path())
            .env("RUST_LOG", filter)
            .env("RUST_BACKTRACE", "0")
            .output()
            .unwrap();
        assert!(!output.status.success());
        let session = session(directory.path());
        let log = fs::read_to_string(session.join("resonance.log")).unwrap();
        assert!(log.contains("deliberate crash log test"), "{log}");
        assert!(log.contains("tests.rs:"), "{log}");
        assert!(log.contains("Backtrace:"), "{log}");
        assert!(log.contains("events_and_panics_are_persisted"), "{log}");
        assert!(!log.contains('\x1b'), "{log}");
        assert!(!log.contains("filtered debug event"), "{log}");
        assert_eq!(log.contains("pre-crash event"), filter == "info", "{log}");
        let report_path = fs::read_dir(&session)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| path.to_string_lossy().ends_with(".5.txt"))
            .unwrap();
        assert_eq!(
            fs::read_dir(&session)
                .unwrap()
                .filter(|entry| entry
                    .as_ref()
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "txt"))
                .count(),
            5
        );
        let report = fs::read_to_string(report_path).unwrap();
        for required in [
            "deliberate crash log test",
            "Revision:",
            "Target:",
            "Recent log:",
            "Latest game context",
            "Backtrace:",
        ] {
            assert!(report.contains(required), "{report}");
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("deliberate crash log test"));
        assert_eq!(stderr.contains("pre-crash event"), filter == "info");
    }
}
