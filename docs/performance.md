# Performance and silent diagnostics

```sh
cargo run --release -p resonance -- --silent --resolution 1920x1080 \
  --perf-overlay --perf-dump local/performance/session.jsonl
```

Use a fresh output filename. F3 toggles the overlay; F4 saves recent samples.
JSONL records frame timings, phase, gameplay tick, fixed render/actual window
sizes, asset counts, sampler-cache activity, memory reads and `unprepared_reads`.
Summaries are flushed
each second and on normal exit. Keep generated measurements under `local/`.

Compare release runs at the same physical resolution, device/backend and workload,
without concurrent builds or captures. Development builds optimize all crates at
level 2 and retain assertions. Record loading holds separately from field-time
hitches. Once a verified snapshot is installed, undeclared asset reads fail;
selected menu pages may decode its resident bytes while waiting for their own
images and GPU draws. Field pipelines still prepare before activation.
See [field preparation](field-preloading.md).

Frame time measures main-loop wall-clock intervals. APP measures the main Bevy
schedule, excluding separate render/GPU work. Neither measures display scanout.
Percentiles use a rolling window; 1%/0.1% LOW are reciprocal P99/P99.9 frame times,
not averages of the slowest frames. `WARMUP` means fewer than 1,000 samples.
Headless captures include GPU readback and manual pacing, so their FPS does not
measure windowed play.

Render resolution is fixed at startup. Forced window/DPI changes affect only the
final fit, preserving camera framing and render targets. Gameplay requests
`AutoNoVsync`; available present modes and the compositor still affect throughput.
Movies wake at audio-clock deadlines; extra input/window events can cause updates.

## Repeatable probes

All commands below are silent or device-free. Use a new output directory per run.

```sh
# New Game through its setup choice, story movie, classroom dialogue and free control.
cargo run -p resonance-presentation --example new_game_capture -- crates/presentation/examples/scenarios/new-game.json local/checks/new-game local/all-assets --paranoid
# The same scenario at a higher rendering resolution; --gamepad tests controller input.
cargo run -p resonance-presentation --example new_game_capture -- crates/presentation/examples/scenarios/new-game.json local/checks/1080 local/all-assets 1920x1080 --paranoid
# Real window, field preparation and forced window-size changes.
cargo run --release -p resonance-presentation --example window_probe -- local/checks/window 1920x1080
# Five-minute classroom throughput run at a fixed physical window size.
cargo run --release -p resonance-presentation --example frame_benchmark -- local/checks/benchmark 1920x1080 300
# Both movies for over ten minutes, with 200 ms consumer stalls.
cargo run -p resonance-media --example playback_probe -- local/all-assets 640 200 512
# Device-rate quality reference written to WAV.
cargo run -p resonance-media --example resample_probe -- local/checks/resampler
# Real movie window; optional final argument "stalls" injects 200 ms hitches.
cargo run -p resonance-presentation --example movie_probe -- local/checks/movie
# Permanently muted device-recovery test.
cargo test -p resonance-presentation muted_device_loss_reopens_output_without_restarting_sources -- --ignored --nocapture
```

The window probe temporarily permits resize requests to exercise the fixed-target
contract. It does not enable resizing in ordinary play. The benchmark requires
the compositor to honor the requested physical window size.
Audio starvation, output underruns, backend xruns and callback deadline misses
must be counted separately; a short callback alone does not prove clean delivery.
