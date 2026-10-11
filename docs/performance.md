# Performance and silent diagnostics

```sh
cargo run --release -p resonance -- --silent --resolution 1920x1080 \
  --perf-overlay --perf-dump local/performance/session.jsonl
```

Use a fresh output filename. F3 toggles the overlay; F4 saves recent samples.
JSONL records frame timings, phase, gameplay tick, fixed render/actual window
sizes, asset counts, sampler-cache activity and late reads. Summaries are flushed
each second and on normal exit. Keep generated measurements under `local/`.

Compare release runs at the same physical resolution, device/backend and workload,
without concurrent builds or captures. Development builds optimize all crates at
level 2 and retain assertions. Record loading holds separately from field-time
hitches. Any late read or unprepared pipeline after activation is a preparation
failure. See [field preparation](field-preloading.md).

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

## Speculative smoothing

```sh
cargo run --release -p resonance -- --silent --speculative --perf-overlay \
  --perf-dump local/performance/speculative.jsonl
```

Compare the same scene and input sequence with and without `--speculative`.

The authoritative simulation remains at 60000/1001 Hz. An isolated branch executes
one ordinary update with the latest latched input. Render frames reuse the branch
until its source session or inputs change, including held axes and latched presses.
Service completion tokens are compared with their fork-time values, so asynchronous
callbacks also invalidate it. The fixed clock's overstep fraction blends committed
and predicted presentation. Predictions never replace real state or dispatch services;
a committed update needs a fresh next-tick endpoint even with unchanged input.
Input presses, RNG, operation scopes/IDs, audio, saves and transitions stay owned
by the committed simulation.

The branch retains the full field, skit, menu or overworld controller until
invalidated. Two fixed 64 MiB arenas are reserved once. One owns the scene
endpoint and cached field skeleton/secondary-physics endpoints; the spare is
reused for transient skeleton, physics and UI evaluation. Cache hits only blend
the retained bone poses. Invalidating a prediction releases all its endpoints
before rotating slots. Nested Rust collections,
VM stacks, strings and operation tokens use those arenas, including the first
prediction. Immutable assets are shared. Allocations retain arena ownership, so
an escaped value prevents reuse instead of allowing its memory to be overwritten.
No arena grows and exhaustion never falls back to the heap. Rendering and GPU
asset publication happen outside these scopes and may allocate as before.

The allocator adapter applies an ownership header to ordinary system allocations.
A slot-lifetime error or a failed simulation prediction logs once and disables
smoothing for the remainder of the run. **Arena exhaustion is different:**
infallible Rust collection allocations abort on exhaustion. There is currently
no complete fallible clone/step path or proven upper bound for every script and
asset. The fixed size is a hard budget, not a guarantee that arbitrary content
fits. This remains a production-readiness limitation; keep the feature opt-in
until that failure path is addressed. A previous peak-use estimate alone would
not make the first oversized prediction safe.

Field rendering samples complete future poses, including clip transitions,
scripted bone adjustments, camera-facing bones, wings and secondary dynamics.
The real solver sees only committed authored poses. A disposable solver copy
advances to the future endpoint; attachment and outline composition follows
interpolation. Matrix interpolation preserves affine endpoints, including shear,
reflection and collapsed scales. The same separation applies to menu previews.

Title animation/cameras, startup fades, UI movement/fades, skit portraits,
overworld models/cameras/animations and game-rendered cinematics use predicted
endpoints. Matching UI geometry blends without morphing unrelated text. Billboard
births and deaths cross-fade, preserving continuous light from particle trails.
Atlas-frame changes remain discrete, even when adjacent rows differ by a small UV
offset; newly cooked title/world assets explicitly distinguish scrolls from atlases.
Movies, scene handoffs, camera cuts and large jumps remain discrete. Captures,
replays, loading and test pause/skip disable smoothing.

Regression tests check committed-state isolation, prediction/real-step agreement,
fixed-slot and endpoint reuse, callback/input invalidation, zero system allocator
calls, secondary solver invariance, matrix
translation boundaries and UI content changes. The cooked-field test additionally
runs four predictions per real step, including a ring action, and reports CPU
fork-plus-step timing and peak arena use:

```sh
RESONANCE_WORLD_ASSETS="$PWD/local/all-assets" cargo test -p resonance-presentation \
  speculative_ticks_match_real_field_steps_and_report_cpu_cost -- \
  --ignored --nocapture
```

A muted finite live probe exercises title-to-field handoff, dialogue and resize
with smoothing enabled:

```sh
RESONANCE_PROBE_SMOOTHING=1 cargo run -p resonance-presentation --example window_probe -- \
  /tmp/resonance-smoothing-probe 1280x720 local/all-assets
```

`RESONANCE_PROBE_TITLE=1` holds the title for a finite 20-second run.
`RESONANCE_PROBE_TITLE_CAPTURE=1` instead advances at four renders per fixed update
and captures the title lighting transition at ticks 614–620. This diagnostic is
for visual comparison, not throughput. `RESONANCE_PROBE_PROFILE=1` adds render
diagnostics and `smoothing_ns` stage times (simulation, field poses, title poses,
affine propagation) to the frame log. Window titles identify smoothing ON/OFF.

Use a fresh output directory. Run timings without concurrent builds or captures;
CPU prediction timings do not measure compositor or input-to-display latency.

## Repeatable probes

All commands below are silent or device-free. Use a new output directory per run.

```sh
# Full New Game, classroom interaction, doorway event and restart.
cargo run -p resonance-presentation --example new_game_capture -- local/checks/new-game
# High-resolution capture; independent of native-resolution oracle APIs.
cargo run -p resonance-presentation --example display_capture -- local/checks/1080 1920x1080 raine-question-oracle
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

Performance dumps include cumulative `prediction_built_reused` counters (builds,
cache hits). Compare their deltas over the measured interval to verify reuse;
`--speculative` should normally build about once per committed tick with stable
input, regardless of render FPS.
