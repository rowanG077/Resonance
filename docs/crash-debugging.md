# Debugging user crashes

Ask for the entire [crash session folder](installation.md#logs-and-crash-reports) as a ZIP, extract it, and download the exact release's `-symbols` archive (rebuilding can change addresses).
Run from the source checkout; the command checks build IDs, resolves text backtraces and stackwalks any native dump:

```sh
cargo install --locked --version 0.27.0 minidump-stackwalk
python tools/ci/crash_symbols.py --analyze /path/to/session-TIMESTAMP-PID --symbols-archive /path/to/release-symbols.tar.gz
```

Windows symbols ZIPs also work. Logs and recent game context accompany the reports; native dumps are best effort and complete only with a `Native dump saved:` record in `crash.native.txt`.
