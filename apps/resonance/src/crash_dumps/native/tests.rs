#[test]
fn crash_child() {
    let Some(directory) = std::env::var_os("RESONANCE_TEST_CRASH_DIRECTORY") else {
        return;
    };
    #[cfg(unix)]
    unsafe {
        // Avoid also creating a potentially huge OS core in the tests.
        let limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        libc::setrlimit(libc::RLIMIT_CORE, &limit);
    }
    let reports = crate::logging::init_at(std::path::Path::new(&directory));
    std::fs::write(
        std::path::Path::new(&directory).join("session-directory"),
        reports.directory().to_string_lossy().as_bytes(),
    )
    .unwrap();
    let mut guard = super::start(&reports).expect("crash monitor must start");
    std::fs::write(
        std::path::Path::new(&directory).join("socket-directory"),
        guard._socket.path().to_string_lossy().as_bytes(),
    )
    .unwrap();
    bevy::log::info!("native crash test breadcrumb");
    let mode = std::env::var("RESONANCE_TEST_CRASH_MODE").unwrap();
    if mode == "healthy" {
        return;
    }
    if mode.starts_with("caught-panic-then-native") {
        let _ = std::panic::catch_unwind(|| panic!("recoverable monitored panic"));
        assert!(guard.child.try_wait().unwrap().is_none());
        assert!(!reports.prefix().with_extension("native.txt").exists());
    }
    if mode == "report-fails-native" {
        std::fs::create_dir(reports.prefix().with_extension("native.txt")).unwrap();
    }
    if mode == "dump-fails-native" {
        std::fs::create_dir(reports.prefix().with_extension("dmp")).unwrap();
    }
    let threaded = mode.contains("thread");
    let crash = move || {
        if mode.contains("stack-overflow") {
            std::hint::black_box(super::stack_overflow(0));
        }
        if matches!(mode.as_str(), "panic" | "thread-panic" | "panic-off") {
            panic!("deliberate monitored panic");
        }
        super::native_crash();
    };
    if threaded {
        let _ = std::thread::Builder::new()
            .name("crash-test-worker".into())
            .stack_size(128 * 1024)
            .spawn(crash)
            .unwrap()
            .join();
        std::process::exit(1);
    } else {
        crash();
    }
}

#[test]
fn dump_file_creation_failure_stops_the_server_loop() {
    use super::*;

    let directory = tempfile::tempdir().unwrap();
    let prefix = directory.path().join("crash");
    std::fs::create_dir(prefix.with_extension("dmp")).unwrap();
    let shutdown = Arc::new(AtomicBool::new(false));
    let handler = Monitor {
        prefix: prefix.clone(),
        header: "test build".into(),
        context: directory.path().join("context"),
        fault: Mutex::new(Fault::default()),
        connected: Arc::new(AtomicBool::new(true)),
        writing: Arc::new(AtomicBool::new(false)),
        shutdown: shutdown.clone(),
    };
    assert!(handler.create_minidump_file().is_err());
    assert!(shutdown.load(Ordering::Acquire));
    let text = std::fs::read_to_string(prefix.with_extension("native.txt")).unwrap();
    assert!(
        text.contains("Could not create crash dump/report"),
        "{text}"
    );
    assert!(!text.contains("Native dump saved"), "{text}");
}

#[test]
fn real_crashes_create_dumps_and_text_reports() {
    use std::{
        fs,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    for mode in [
        "panic",
        "access-violation",
        "thread-panic",
        "thread-access-violation",
        "thread-stack-overflow",
        "panic-off",
        "caught-panic-then-native",
        "caught-panic-then-native-off",
        "dump-fails-native",
        "finalize-fails-native",
        "report-fails-native",
        "slow-native",
        "watchdog-native",
        "healthy",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let stderr = fs::File::create(directory.path().join("stderr")).unwrap();
        let started = Instant::now();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "crash_dumps::native::tests::crash_child",
                "--nocapture",
            ])
            .env("RESONANCE_TEST_CRASH_DIRECTORY", directory.path())
            .env("RESONANCE_TEST_CRASH_MODE", mode)
            .env(
                "RUST_LOG",
                if mode.ends_with("off") { "off" } else { "info" },
            )
            .env("RUST_BACKTRACE", "0")
            .stdout(Stdio::null())
            .stderr(stderr)
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(40);
        #[cfg(unix)]
        let mut blocked_report = false;
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() > deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("crash test timed out: {mode}");
            }
            #[cfg(unix)]
            if mode == "watchdog-native"
                && !blocked_report
                && let Ok(session) = fs::read_to_string(directory.path().join("session-directory"))
            {
                let session = std::path::Path::new(&session);
                if session.join("crash.dmp").is_file() {
                    let report = session.join("crash.native.txt");
                    fs::rename(&report, session.join("saved-native.txt")).unwrap();
                    use std::os::unix::ffi::OsStrExt;
                    let path = std::ffi::CString::new(report.as_os_str().as_bytes()).unwrap();
                    // No reader: reopening this FIFO for writing would block
                    // the old watchdog indefinitely, before it could exit.
                    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
                    blocked_report = true;
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let stderr = fs::read_to_string(directory.path().join("stderr")).unwrap();
        if mode == "slow-native" {
            assert!(
                started.elapsed() >= Duration::from_secs(7),
                "client left before slow dump completed: {stderr}"
            );
        }
        assert_eq!(status.success(), mode == "healthy", "{mode}: {stderr}");
        let socket =
            fs::read_to_string(directory.path().join("socket-directory")).unwrap_or_else(|error| {
                panic!("crash monitor did not start for {mode}: {error}; {stderr}")
            });
        let cleanup_deadline = Instant::now() + Duration::from_secs(2);
        while mode != "watchdog-native"
            && std::path::Path::new(&socket).exists()
            && Instant::now() < cleanup_deadline
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        if mode == "watchdog-native" {
            // The hard timeout deliberately performs no filesystem cleanup.
            let _ = fs::remove_dir_all(&socket);
        } else {
            assert!(
                !std::path::Path::new(&socket).exists(),
                "monitor left IPC files behind: {mode}: {socket}, {stderr}"
            );
        }
        let session = std::path::PathBuf::from(
            fs::read_to_string(directory.path().join("session-directory")).unwrap(),
        );
        if mode == "healthy" {
            assert!(!fs::read_dir(&session).unwrap().any(|e| {
                e.unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with("crash.")
            }));
            continue;
        }
        if mode == "watchdog-native" {
            assert!(started.elapsed() >= Duration::from_secs(30), "{stderr}");
            #[cfg(unix)]
            {
                assert!(blocked_report, "did not exercise blocked report I/O");
                fs::remove_file(session.join("crash.native.txt")).unwrap();
                fs::rename(
                    session.join("saved-native.txt"),
                    session.join("crash.native.txt"),
                )
                .unwrap();
            }
            let report = fs::read_to_string(session.join("crash.native.txt")).unwrap();
            assert!(report.contains("Dump status: incomplete"), "{report}");
            assert!(!report.contains("Native dump saved"), "{report}");
            assert_eq!(
                fs::read(session.join("crash.dmp")).unwrap(),
                b"incomplete dump"
            );
            assert!(!session.join("crash.core").is_file());
            continue;
        }
        let paths: Vec<_> = fs::read_dir(&session)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        if matches!(mode, "panic" | "thread-panic" | "panic-off") {
            let report = paths
                .iter()
                .find(|p| p.to_string_lossy().ends_with(".0.txt"))
                .unwrap();
            assert!(
                fs::read_to_string(report)
                    .unwrap()
                    .contains("deliberate monitored panic")
            );
            assert!(
                !paths
                    .iter()
                    .any(|p| p.to_string_lossy().ends_with(".native.txt"))
            );
            assert!(
                !paths
                    .iter()
                    .any(|p| p.extension().is_some_and(|e| e == "dmp" || e == "core"))
            );
            continue;
        }
        if mode == "dump-fails-native" {
            let report = fs::read_dir(&session)
                .unwrap()
                .map(|e| e.unwrap().path())
                .find(|p| p.to_string_lossy().ends_with(".native.txt"))
                .unwrap();
            let text = fs::read_to_string(report).unwrap();
            assert!(
                text.contains("Could not create crash dump/report"),
                "{text}; {stderr}"
            );
            continue;
        }
        if mode == "finalize-fails-native" {
            let text = fs::read_to_string(session.join("crash.native.txt")).unwrap();
            assert!(text.contains("Native dump finalization failed"), "{text}");
            assert!(
                text.contains("injected dump finalization failure"),
                "{text}"
            );
            assert!(!text.contains("Native dump saved"), "{text}");
            #[cfg(windows)]
            assert!(
                stderr.contains("Could not write native crash dump"),
                "the Windows client must receive failure after finalization: {stderr}"
            );
            continue;
        }
        #[cfg(target_os = "linux")]
        if mode.starts_with("thread-stack-overflow")
            && !paths
                .iter()
                .any(|p| p.extension().is_some_and(|e| e == "dmp"))
        {
            let report = paths
                .iter()
                .find(|p| p.to_string_lossy().ends_with(".native.txt"))
                .unwrap();
            let text = fs::read_to_string(report).unwrap();
            assert!(text.contains("signal/exception=0xb"), "{text}");
            assert!(text.contains("PC: 0x") && text.contains("SP: 0x"), "{text}");
            assert!(text.contains("ELF virtual address: 0x"), "{text}");
            assert!(session.join("memory-maps.txt").is_file());
            assert!(!session.join("crash.core").exists());
            continue;
        }
        let dump = paths
            .iter()
            .find(|p| p.extension().is_some_and(|e| e == "dmp"))
            .unwrap_or_else(|| panic!("no dump for {mode}: {stderr}"));
        let bytes = fs::read(dump).unwrap();
        assert_eq!(&bytes[..4], b"MDMP", "{mode}: {stderr}");
        let word =
            |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        let streams = word(8);
        let directory_offset = word(12);
        let mut kinds = Vec::new();
        for stream in 0..streams {
            let start = directory_offset + stream * 12;
            kinds.push(word(start));
            assert!(word(start + 8) + word(start + 4) <= bytes.len());
        }
        for required in [3, 4, 6, 7] {
            assert!(
                kinds.contains(&required),
                "missing dump stream {required}: {mode}, {stderr}"
            );
        }
        if mode == "report-fails-native" {
            assert!(
                stderr.contains("Could not write native text report"),
                "{stderr}"
            );
            assert!(stderr.contains("Native dump saved"), "{stderr}");
            continue;
        }
        let report = paths
            .iter()
            .find(|p| p.to_string_lossy().ends_with(".native.txt"))
            .unwrap();
        let text = fs::read_to_string(report).unwrap();
        assert!(
            text.contains("Native dump saved"),
            "{mode}: {text}; {stderr}"
        );
        assert!(text.contains("Revision:"), "{text}");
        assert_eq!(
            text.contains("native crash test breadcrumb"),
            !mode.ends_with("off"),
            "{text}"
        );
        assert!(text.contains("Fault:"), "{text}");
        assert!(text.contains("signal/exception="), "{text}");
        assert!(!text.contains("Context unavailable"), "{text}");
        if mode.starts_with("caught-panic-then-native") {
            #[cfg(target_os = "linux")]
            assert!(text.contains("signal/exception=0xb"), "{text}");
            #[cfg(target_os = "windows")]
            assert!(text.contains("signal/exception=0xc0000005"), "{text}");
            #[cfg(target_os = "macos")]
            assert!(text.contains("signal/exception=0x1"), "{text}");
            assert!(
                paths
                    .iter()
                    .any(|p| p.to_string_lossy().ends_with(".0.txt"))
            );
        }
    }
}

#[test]
fn monitor_entry() {
    if let Some(socket) = std::env::var_os("RESONANCE_TEST_MONITOR_SOCKET") {
        super::monitor(
            std::path::Path::new(&socket),
            std::path::Path::new(&std::env::var_os("RESONANCE_TEST_MONITOR_PREFIX").unwrap()),
            std::path::Path::new(&std::env::var_os("RESONANCE_TEST_MONITOR_HEADER").unwrap()),
        )
        .unwrap();
    }
}
