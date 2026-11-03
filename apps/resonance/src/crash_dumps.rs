//! The compromised game only sends its saved CPU context. File creation,
//! symbol/module enumeration and dump writing happen in a separate process.
use crate::logging::Reports;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

/// The native debug identity survives distribution stripping and matches the
/// MODULE record in the exact release's portable symbols. Read it before a fault.
pub fn executable_debug_id() -> anyhow::Result<String> {
    #[cfg(target_os = "linux")]
    return linux::debug_id();
    #[cfg(windows)]
    return windows::debug_id();
    #[cfg(target_os = "macos")]
    {
        use object::{
            macho,
            read::{ReadCache, macho::MachHeader as _},
        };
        let cache = ReadCache::new(std::fs::File::open(std::env::current_exe()?)?);
        // Only read the Mach header and load commands; parsing the whole object
        // would also pull its potentially large symbol table into memory.
        let header = macho::MachHeader64::<object::Endianness>::parse(&cache, 0)?;
        let endian = header.endian()?;
        let mut commands = header.load_commands(endian, &cache, 0)?;
        while let Some(command) = commands.next()? {
            if let Some(uuid) = command.uuid()? {
                let id: String = uuid.uuid.iter().map(|byte| format!("{byte:02X}")).collect();
                return Ok(format!("{id}0"));
            }
        }
        anyhow::bail!("executable has no Mach-O UUID");
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    anyhow::bail!("executable debug ID unsupported on this platform")
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
mod native {
    use super::*;
    use anyhow::{Context, Result, bail};
    #[cfg(not(windows))]
    use minidumper::SocketName;
    use minidumper::{LoopAction, ServerHandler};
    use std::{
        fs::{self, File},
        io::Write,
        path::{Path, PathBuf},
        process::{Child, Command, Stdio},
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
        },
        time::{Duration, Instant},
    };

    pub struct Guard {
        _handler: crash_handler::CrashHandler,
        child: Child,
        _socket: tempfile::TempDir,
    }
    impl Drop for Guard {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    pub fn install(reports: &Reports) -> Option<Guard> {
        match start(reports) {
            Ok(guard) => Some(guard),
            Err(error) => {
                bevy::log::warn!(
                    "Native crash dumps unavailable: {error:#}; text crash reports remain enabled"
                );
                eprintln!("Native crash dumps unavailable: {error:#}");
                None
            }
        }
    }

    fn start(reports: &Reports) -> Result<Guard> {
        // Short private socket path: Unix and Windows AF_UNIX paths have small
        // fixed limits. Do not place it under an arbitrarily long game path.
        let socket_directory = tempfile::Builder::new().prefix("rs-crash-").tempdir()?;
        let socket = socket_directory.path().join("socket");
        let prefix = std::path::absolute(reports.prefix())?;
        fs::create_dir_all(prefix.parent().context("crash report directory missing")?)?;
        let header = socket_directory.path().join("header");
        fs::write(&header, reports.header())?;
        let mut command = Command::new(std::env::current_exe()?);
        #[cfg(not(test))]
        command
            .arg("--crash-monitor")
            .arg(&socket)
            .arg(&prefix)
            .arg(&header);
        #[cfg(test)]
        command
            .args([
                "--exact",
                "crash_dumps::native::tests::monitor_entry",
                "--nocapture",
            ])
            .env("RESONANCE_TEST_MONITOR_SOCKET", &socket)
            .env("RESONANCE_TEST_MONITOR_PREFIX", &prefix)
            .env("RESONANCE_TEST_MONITOR_HEADER", &header);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let child = command.spawn().context("starting crash monitor")?;
        // On every setup error, reap the helper before removing its socket.
        struct Starting(Option<Child>);
        impl Drop for Starting {
            fn drop(&mut self) {
                if let Some(child) = &mut self.0 {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }
        let mut starting = Starting(Some(child));
        let deadline = Instant::now() + Duration::from_secs(5);
        let client = loop {
            #[cfg(not(windows))]
            let connection = minidumper::Client::with_name(SocketName::path(
                socket.to_str().context("non-UTF-8 crash socket path")?,
            ));
            #[cfg(windows)]
            let connection = super::windows::Client::connect(&socket);
            match connection {
                Ok(client) => break client,
                Err(error) => {
                    if starting.0.as_mut().unwrap().try_wait()?.is_some() {
                        bail!("crash monitor exited during startup: {error}");
                    }
                    if Instant::now() >= deadline {
                        return Err(error).context("connecting to crash monitor");
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
        };
        // SAFETY: The callback captures an already-connected IPC client only.
        // No formatting, allocations, filesystem operations, tracing, or game
        // locks are used in the compromised signal/exception context. This is
        // the request_dump use case supported by minidumper/crash-handler.
        let event = unsafe {
            crash_handler::make_crash_event(move |context| {
                let values = fault_details(context);
                let mut bytes = [0u8; 64];
                for (chunk, value) in bytes.as_chunks_mut::<8>().0.iter_mut().zip(values) {
                    chunk.copy_from_slice(&value.to_le_bytes());
                }
                let _ = client.send_message(0, bytes);
                // macOS sends the crash context over a separate Mach port;
                // acknowledge metadata first so its text report is complete.
                #[cfg(target_os = "macos")]
                let _ = client.ping();
                let success = client.request_dump(context).is_ok();
                // minidumper 0.11's Mach acknowledgement expires after five
                // seconds, even on a successful send. The socket remains open
                // until the monitor finishes (or its watchdog exits). Waiting
                // for that closure keeps the task and its memory alive during
                // slow dumps, using the same watchdog as the other platforms.
                #[cfg(target_os = "macos")]
                let _ = client.ping();
                if !success {
                    crash_handler::write_stderr(
                        "Could not write native crash dump; see crash report and system crash reporter\n",
                    );
                }
                success.into()
            })
        };
        let handler = crash_handler::CrashHandler::attach(event)?;
        #[cfg(target_os = "linux")]
        handler.set_ptracer(Some(starting.0.as_ref().unwrap().id()));
        resonance_presentation::write_crash_context_to(socket_directory.path().join("context"))
            .context("publishing game context for crash monitor")?;
        // Rust panics have their own text reports and may be caught. Keep the
        // monitor available for the native fault that actually ends the session.
        bevy::log::info!(
            "Native crash monitor ready; dumps: {}.dmp",
            prefix.display()
        );
        Ok(Guard {
            _handler: handler,
            child: starting.0.take().unwrap(),
            _socket: socket_directory,
        })
    }

    // Fixed-size, allocation-free metadata sent before the full crash context.
    // Pointer-bearing Windows exception records stay in the dump writer.
    fn fault_details(context: &crash_handler::CrashContext) -> [u64; 8] {
        #[cfg(target_os = "linux")]
        let [pc, sp, frame] = linux_registers(context);
        #[cfg(target_os = "linux")]
        return [
            context.pid as u64,
            context.tid as u64,
            context.siginfo.ssi_signo as u64,
            context.siginfo.ssi_code as u64,
            linux_fault_address(context),
            pc,
            sp,
            frame,
        ];
        #[cfg(target_os = "windows")]
        return [
            context.process_id as u64,
            context.thread_id as u64,
            context.exception_code as u32 as u64,
            0,
            0,
            0,
            0,
            0,
        ];
        #[cfg(target_os = "macos")]
        return [
            context.task as u64,
            context.thread as u64,
            context.exception.map_or(0, |e| e.kind as u64),
            context.exception.map_or(0, |e| e.code),
            context.exception.and_then(|e| e.subcode).unwrap_or(0),
            0,
            0,
            0,
        ];
    }

    #[cfg(target_os = "linux")]
    fn linux_registers(context: &crash_handler::CrashContext) -> [u64; 3] {
        #[cfg(target_arch = "aarch64")]
        return [
            context.context.uc_mcontext.pc,
            context.context.uc_mcontext.sp,
            context.context.uc_mcontext.regs[30],
        ];
        #[cfg(target_arch = "x86_64")]
        return [libc::REG_RIP, libc::REG_RSP, libc::REG_RBP]
            .map(|index| context.context.uc_mcontext.gregs[index as usize] as u64);
        #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
        [0; 3]
    }

    #[derive(Default, Clone, Copy)]
    struct Fault {
        process: u32,
        thread: u64,
        signal: u64,
        code: u64,
        address: u64,
        pc: u64,
        sp: u64,
        frame: u64,
    }
    impl std::fmt::Display for Fault {
        fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(
                out,
                "{}; process/task={}; thread={}; signal/exception={:#x}; code={:#x}; address/subcode={:#x}",
                std::env::consts::OS,
                self.process,
                self.thread,
                self.signal,
                self.code,
                self.address
            )?;
            if self.pc != 0 || self.sp != 0 {
                write!(
                    out,
                    "\nPC: {:#x}\nSP: {:#x}\nFrame/link register: {:#x}",
                    self.pc, self.sp, self.frame
                )?;
            }
            Ok(())
        }
    }

    #[cfg(target_os = "linux")]
    fn linux_fault_address(context: &crash_handler::CrashContext) -> u64 {
        const {
            assert!(
                std::mem::size_of::<libc::siginfo_t>()
                    == std::mem::size_of::<libc::signalfd_siginfo>()
            );
        }
        const {
            assert!(
                std::mem::align_of::<libc::siginfo_t>()
                    <= std::mem::align_of::<libc::signalfd_siginfo>()
            );
        }
        if context.siginfo.ssi_code <= 0
            || !matches!(
                context.siginfo.ssi_signo as i32,
                libc::SIGSEGV | libc::SIGBUS | libc::SIGILL | libc::SIGFPE | libc::SIGTRAP
            )
        {
            return 0;
        }
        // SAFETY: crash-handler copies the kernel siginfo_t bytes into this
        // same-sized/aligned storage. Its signalfd_siginfo type has a different
        // union layout; read the real si_addr without dereferencing that address.
        unsafe {
            (&*std::ptr::from_ref(&context.siginfo).cast::<libc::siginfo_t>()).si_addr() as u64
        }
    }

    pub(super) struct Monitor {
        prefix: PathBuf,
        header: String,
        context: PathBuf,
        fault: Mutex<Fault>,
        connected: Arc<AtomicBool>,
        writing: Arc<AtomicBool>,
    }
    impl Monitor {
        fn finalize_dump(&self, dump: &mut minidumper::MinidumpBinary) -> Result<()> {
            #[cfg(test)]
            if std::env::var("RESONANCE_TEST_CRASH_MODE").as_deref() == Ok("finalize-fails-native")
            {
                bail!("injected dump finalization failure");
            }
            #[cfg(windows)]
            if super::windows::normalize(dump)? {
                self.status("Restored missing executable PDB identity from PE debug data");
            }
            #[cfg(target_os = "linux")]
            super::linux::normalize(
                dump,
                self.fault
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .address,
            )?;
            dump.file.sync_all().context("flushing native dump")
        }

        /// Completion includes normalization and durable flushing. Windows must
        /// acknowledge this result so a failed capture leaves OS handling available.
        pub(super) fn complete_dump(
            &self,
            result: Result<minidumper::MinidumpBinary, minidumper::Error>,
        ) -> bool {
            match result {
                Ok(mut dump) => match self.finalize_dump(&mut dump) {
                    Ok(()) => {
                        self.status(&format!("Native dump saved: {}", dump.path.display()));
                        true
                    }
                    Err(error) => {
                        self.status(&format!("Native dump finalization failed: {error:#}"));
                        false
                    }
                },
                Err(error) => {
                    self.status(&format!(
                        "Native dump failed: {error:#?}; partial dump removed"
                    ));
                    let _ = fs::remove_file(self.prefix.with_extension("dmp"));
                    false
                }
            }
        }

        fn status(&self, message: &str) {
            let path = self.prefix.with_extension("native.txt");
            if let Ok(mut file) = std::fs::OpenOptions::new().append(true).open(&path) {
                let _ = writeln!(file, "{message}");
                let _ = file.sync_all();
            }
            eprintln!("{message}");
        }
    }
    impl ServerHandler for Monitor {
        fn create_minidump_file(&self) -> std::io::Result<(File, PathBuf)> {
            self.writing.store(true, Ordering::Release);
            #[cfg(test)]
            if std::env::var("RESONANCE_TEST_CRASH_MODE").as_deref() == Ok("slow-native") {
                std::thread::sleep(Duration::from_secs(7));
            }
            let result = (|| {
                let path = self.prefix.with_extension("dmp");
                if let Err(error) = crate::logging::write_report(
                    &self.prefix,
                    "native.txt",
                    &self.header,
                    &format!(
                        "Native crash dump requested\nFault: {}\nDump: {}\nDump status: incomplete until a successful completion record is appended.",
                        self.fault.lock().unwrap_or_else(|error| error.into_inner()),
                        path.display()
                    ),
                    &fs::read_to_string(&self.context)
                        .unwrap_or_else(|error| format!("Context unavailable: {error}")),
                ) {
                    self.status(&format!(
                        "Could not write native text report: {error}; attempting dump anyway"
                    ));
                }
                #[cfg(test)]
                if std::env::var("RESONANCE_TEST_CRASH_MODE").as_deref() == Ok("watchdog-native") {
                    // Exercise a stalled writer after the independent report
                    // exists, including Windows clients waiting for a pipe ACK.
                    fs::write(&path, b"incomplete dump")?;
                    std::thread::sleep(Duration::from_secs(60));
                }
                // Attempt the independent CPU/module report before reading the stack.
                #[cfg(target_os = "linux")]
                {
                    let fault = *self.fault.lock().unwrap_or_else(|error| error.into_inner());
                    match super::linux::write_fault_location(&self.prefix, fault.process, fault.pc)
                    {
                        Ok(location) => self.status(&location),
                        Err(error) => self.status(&format!(
                            "Native code location unavailable: {error:#}; saved PC/SP remain usable"
                        )),
                    }
                }
                let file = crate::logging::private_file(&path)?;
                Ok((file, path))
            })();
            if let Err(error) = &result {
                self.status(&format!("Could not create crash dump/report: {error}"));
            }
            result
        }

        fn on_minidump_created(
            &self,
            result: Result<minidumper::MinidumpBinary, minidumper::Error>,
        ) -> LoopAction {
            self.complete_dump(result);
            LoopAction::Exit
        }
        fn on_message(&self, kind: u32, bytes: Vec<u8>) {
            if kind != 0 || bytes.len() != 64 {
                return;
            }
            let mut values = [0u64; 8];
            for (value, chunk) in values.iter_mut().zip(bytes.as_chunks::<8>().0.iter()) {
                *value = u64::from_le_bytes(*chunk);
            }
            *self.fault.lock().unwrap_or_else(|error| error.into_inner()) = Fault {
                process: values[0] as u32,
                thread: values[1],
                signal: values[2],
                code: values[3],
                address: values[4],
                pc: values[5],
                sp: values[6],
                frame: values[7],
            };
            self.writing.store(true, Ordering::Release);
        }
        fn on_client_connected(&self, _: usize) -> LoopAction {
            self.connected.store(true, Ordering::Release);
            LoopAction::Continue
        }
        fn on_client_disconnected(&self, count: usize) -> LoopAction {
            if count == 0 {
                LoopAction::Exit
            } else {
                LoopAction::Continue
            }
        }
    }

    fn clean_socket_files(socket: &Path, header: &Path) {
        // The monitor outlives the game after a native crash. Remove its known
        // private IPC files without touching reports or recursively deleting
        // anything supplied to the internal command line.
        if socket.file_name() != Some(std::ffi::OsStr::new("socket"))
            || header.file_name() != Some(std::ffi::OsStr::new("header"))
            || socket.parent() != header.parent()
        {
            return;
        }
        for path in [
            socket.to_owned(),
            header.to_owned(),
            header.with_file_name("context"),
        ] {
            let _ = fs::remove_file(path);
        }
        if let Some(directory) = socket.parent() {
            let _ = fs::remove_dir(directory);
        }
    }

    fn watchdog_exit(code: i32) -> ! {
        // Bypass atexit handlers and DLL detach callbacks as well as Rust
        // destructors: any of those could wait on the stalled dump writer.
        #[cfg(unix)]
        unsafe {
            libc::_exit(code)
        }
        #[cfg(windows)]
        {
            #[link(name = "kernel32")]
            unsafe extern "system" {
                fn TerminateProcess(process: *mut std::ffi::c_void, code: u32) -> i32;
            }
            // SAFETY: -1 is the current-process pseudo handle. All handles
            // close in the kernel; no user-mode cleanup runs.
            unsafe { TerminateProcess(-1isize as *mut std::ffi::c_void, code as u32) };
            std::process::abort();
        }
    }

    pub fn monitor(socket: &Path, prefix: &Path, header: &Path) -> Result<()> {
        struct Cleanup<'a>(&'a Path, &'a Path);
        impl Drop for Cleanup<'_> {
            fn drop(&mut self) {
                clean_socket_files(self.0, self.1);
            }
        }
        let _cleanup = Cleanup(socket, header);
        let _session =
            crate::logging::hold_session(prefix.parent().context("missing session directory")?)?;
        #[cfg(not(windows))]
        let mut server = minidumper::Server::with_name(SocketName::path(
            socket.to_str().context("non-UTF-8 crash socket path")?,
        ))?;
        #[cfg(windows)]
        let mut server = super::windows::Server::bind(socket)?;
        let connected = Arc::new(AtomicBool::new(false));
        let writing = Arc::new(AtomicBool::new(false));
        let handler = Monitor {
            prefix: prefix.to_owned(),
            header: fs::read_to_string(header)?,
            context: header.with_file_name("context"),
            fault: Mutex::new(Fault::default()),
            connected: connected.clone(),
            writing: writing.clone(),
        };
        std::thread::Builder::new()
            .name("crash-monitor-watchdog".into())
            .spawn(move || {
                let started = Instant::now();
                let mut dump_started = None;
                loop {
                    std::thread::sleep(Duration::from_secs(1));
                    if !connected.load(Ordering::Acquire)
                        && started.elapsed() > Duration::from_secs(10)
                    {
                        watchdog_exit(1);
                    }
                    if writing.load(Ordering::Acquire) && dump_started.is_none() {
                        dump_started = Some(Instant::now());
                    }
                    if dump_started.is_some_and(|time| time.elapsed() > Duration::from_secs(30)) {
                        // No filesystem operations, logging, locks or cleanup:
                        // the writer may be stuck on that same I/O. Process exit
                        // closes IPC and releases the crashed client. Leave any
                        // partial artifacts for inspection.
                        watchdog_exit(2);
                    }
                }
            })?;
        #[cfg(not(windows))]
        let result = server.run(Box::new(handler), &AtomicBool::new(false), None);
        #[cfg(windows)]
        let result = server.run(handler);
        if let Err(error) = result {
            // File creation failures can exit before on_minidump_created.
            if let Ok(mut report) = std::fs::OpenOptions::new()
                .append(true)
                .open(prefix.with_extension("native.txt"))
            {
                let _ = writeln!(report, "Crash monitor failed: {error:#}");
                let _ = report.sync_all();
            }
            eprintln!("Crash monitor failed: {error:#}");
            return Err(error.into());
        }
        Ok(())
    }

    #[inline(never)]
    fn native_crash() {
        // Force a real stack frame. Windows leaf functions can omit unwind
        // metadata, which would test stack scanning rather than CFI recovery.
        let frame = std::hint::black_box([0u8; 256]);
        // SAFETY: Only invoked by the explicit internal crash self-test or a
        // disposable unit-test subprocess. CI resolves this exact source frame.
        unsafe {
            std::ptr::write_volatile(std::ptr::dangling_mut::<u8>(), 42);
        }
        std::hint::black_box(frame);
    }

    #[inline(never)]
    fn panic_crash() {
        panic!("deliberate crash self-test panic");
    }

    #[inline(never)]
    #[allow(unconditional_recursion)]
    fn stack_overflow(depth: usize) -> usize {
        let pad = [depth as u8; 4096];
        let result = stack_overflow(std::hint::black_box(depth + 1));
        std::hint::black_box(pad);
        result
    }

    #[inline(never)]
    pub(super) fn self_test(directory: &Path, mode: &str) -> Result<()> {
        anyhow::ensure!(
            matches!(mode, "access-violation" | "stack-overflow" | "panic"),
            "unknown crash self-test mode"
        );
        #[cfg(unix)]
        unsafe {
            // Synthetic CI crashes must not also leave a huge OS-managed core.
            let limit = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            libc::setrlimit(libc::RLIMIT_CORE, &limit);
        }
        let reports = crate::logging::init_at(directory);
        let _guard = start(&reports)?;
        if mode == "panic" {
            panic_crash();
        } else if mode == "stack-overflow" {
            let _ = std::thread::Builder::new()
                .name("crash-self-test".into())
                .stack_size(128 * 1024)
                .spawn(|| std::hint::black_box(stack_overflow(0)))?
                .join();
        } else {
            native_crash();
        }
        bail!("crash self-test unexpectedly returned")
    }

    #[cfg(test)]
    mod tests;
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
pub use native::install;

pub fn self_test_command() -> Option<anyhow::Result<()>> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new("--crash-self-test")) {
        return None;
    }
    Some((|| {
        let directory = args
            .next()
            .ok_or_else(|| anyhow::anyhow!("missing self-test directory"))?;
        let mode = args
            .next()
            .ok_or_else(|| anyhow::anyhow!("missing self-test mode"))?;
        anyhow::ensure!(args.next().is_none(), "unexpected self-test argument");
        #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
        return native::self_test(
            std::path::Path::new(&directory),
            mode.to_str()
                .ok_or_else(|| anyhow::anyhow!("invalid self-test mode"))?,
        );
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        anyhow::bail!("native crash self-test unsupported on this platform");
    })())
}

pub fn monitor_command() -> Option<anyhow::Result<()>> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new("--crash-monitor")) {
        return None;
    }
    Some((|| {
        let socket = args
            .next()
            .ok_or_else(|| anyhow::anyhow!("missing monitor socket"))?;
        let prefix = args
            .next()
            .ok_or_else(|| anyhow::anyhow!("missing monitor report path"))?;
        let header = args
            .next()
            .ok_or_else(|| anyhow::anyhow!("missing monitor header"))?;
        anyhow::ensure!(args.next().is_none(), "unexpected monitor argument");
        #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
        return native::monitor(
            std::path::Path::new(&socket),
            std::path::Path::new(&prefix),
            std::path::Path::new(&header),
        );
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        anyhow::bail!("native crash dumps are unsupported on this platform");
    })())
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub fn install(_: &Reports) -> Option<()> {
    None
}
