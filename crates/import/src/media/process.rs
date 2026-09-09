use anyhow::{Context, Result, bail};
use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

const LOG_LIMIT: usize = 8 * 1024 * 1024;

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

/// Monitor the whole operation, including a streaming producer blocked on stdin.
/// Errors and timeouts kill/reap the child before joining the worker threads.
/// Both diagnostic streams share one bounded log; nothing reaches speakers.
pub(super) fn pipe<F>(
    command: &mut Command,
    log_path: &Path,
    timeout: Duration,
    input: F,
) -> Result<()>
where
    F: FnOnce(&mut ChildStdin) -> Result<()> + Send,
{
    let log = Mutex::new((File::create(log_path)?, 0usize));
    let failed = AtomicBool::new(false);
    let mut child = ChildGuard(
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("launch {command:?}"))?,
    );
    let mut stdin = child.0.stdin.take().context("child stdin missing")?;
    let stdout = child.0.stdout.take().context("child stdout missing")?;
    let stderr = child.0.stderr.take().context("child stderr missing")?;
    let result = thread::scope(|scope| {
        let writer = scope.spawn(|| {
            let result = input(&mut stdin);
            drop(stdin);
            if result.is_err() {
                failed.store(true, Ordering::Release);
            }
            result
        });
        let out = scope.spawn(|| drain(stdout, &log, &failed));
        let err = scope.spawn(|| drain(stderr, &log, &failed));
        let started = Instant::now();
        let status = loop {
            match child.0.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Err(error) => break Err(error.into()),
                Ok(None) => {}
            }
            if failed.load(Ordering::Acquire) {
                break Err(anyhow::anyhow!("converter input or log failed"));
            }
            if started.elapsed() >= timeout {
                break Err(anyhow::anyhow!("converter timed out after {timeout:?}"));
            }
            thread::sleep(Duration::from_millis(25));
        };
        let failed_before_cancel = failed.load(Ordering::Acquire);
        if status.is_err() {
            let _ = child.0.kill();
            let _ = child.0.wait();
        }
        let written = writer
            .join()
            .map_err(|_| anyhow::anyhow!("converter producer panicked"))?;
        let stdout = out
            .join()
            .map_err(|_| anyhow::anyhow!("converter log reader panicked"))?;
        let stderr = err
            .join()
            .map_err(|_| anyhow::anyhow!("converter log reader panicked"))?;
        // Preserve producer diagnostics (e.g. malformed source frame), unless
        // cancellation itself broke the pipe after a timeout.
        stdout?;
        stderr?;
        if failed_before_cancel && status.is_err() {
            written?;
            return status.map(|_| ());
        }
        let status = status?;
        written?;
        if !status.success() {
            bail!("converter exited with {status}");
        }
        Ok(())
    });
    result.with_context(|| format!("media conversion failed; see {}", log_path.display()))
}

fn drain(mut source: impl Read, log: &Mutex<(File, usize)>, failed: &AtomicBool) -> Result<()> {
    let result = (|| {
        let mut buffer = [0u8; 4096];
        loop {
            let count = source.read(&mut buffer)?;
            if count == 0 {
                return Ok(());
            }
            let mut log = log
                .lock()
                .map_err(|_| anyhow::anyhow!("converter log poisoned"))?;
            let allowed = count.min(LOG_LIMIT - log.1);
            log.0.write_all(&buffer[..allowed])?;
            log.1 += allowed;
            if allowed < count {
                bail!("converter exceeded {LOG_LIMIT} diagnostic bytes");
            }
        }
    })();
    if result.is_err() {
        failed.store(true, Ordering::Release);
    }
    result
}

pub(super) fn run(command: &mut Command, log: &Path, input: &[u8]) -> Result<()> {
    pipe(command, log, Duration::from_secs(120), |stdin| {
        Ok(stdin.write_all(input)?)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // This same Rust test executable acts as a converter fixture, avoiding a
    // shell, platform-specific utilities, or real codecs in synthetic tests.
    #[test]
    fn converter_fixture() {
        match std::env::var("RESONANCE_CONVERTER_FIXTURE").as_deref() {
            Ok("blocked") => thread::sleep(Duration::from_secs(30)),
            Ok("noisy") => {
                let _ = std::io::stdout().write_all(&vec![b'x'; LOG_LIMIT * 2]);
            }
            Ok("failure") => {
                eprintln!("fixture decode failure");
                std::process::exit(7);
            }
            _ => {}
        }
    }

    fn fixture(mode: &str) -> (Command, std::path::PathBuf) {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "media::process::tests::converter_fixture",
                "--nocapture",
            ])
            .env("RESONANCE_CONVERTER_FIXTURE", mode);
        let log = std::env::temp_dir().join(format!(
            "resonance-converter-{}-{mode}.log",
            std::process::id()
        ));
        (command, log)
    }

    #[test]
    fn timeout_unblocks_streaming_input_and_reaps_child() {
        let (mut command, log) = fixture("blocked");
        let started = Instant::now();
        let error = pipe(&mut command, &log, Duration::from_millis(150), |stdin| {
            Ok(stdin.write_all(&vec![0; 1024 * 1024])?)
        })
        .unwrap_err();
        assert!(format!("{error:#}").contains("timed out"), "{error:#}");
        assert!(started.elapsed() < Duration::from_secs(5));
        let _ = std::fs::remove_file(log);
    }

    #[test]
    fn diagnostic_flood_is_bounded_and_nonzero_exit_keeps_log() {
        let (mut command, log) = fixture("noisy");
        let error = pipe(&mut command, &log, Duration::from_secs(5), |_| Ok(())).unwrap_err();
        assert!(format!("{error:#}").contains("diagnostic bytes"));
        assert_eq!(std::fs::metadata(&log).unwrap().len(), LOG_LIMIT as u64);
        let _ = std::fs::remove_file(log);
        let (mut command, log) = fixture("failure");
        assert!(pipe(&mut command, &log, Duration::from_secs(5), |_| Ok(())).is_err());
        assert!(
            std::fs::read_to_string(&log)
                .unwrap()
                .contains("fixture decode failure")
        );
        let _ = std::fs::remove_file(log);
    }
}
