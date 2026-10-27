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
For visual inspection, **F10** cycles ½×, ⅕×, ⅒× and normal speed. It opens the
performance overlay with the selected rate; **F3** hides the overlay. The game
clock and audio transport slow together, while rendering stays uncapped. The
fixed timestep and interpolation calculation are unchanged. At ⅒× this exposes
roughly 167 ms between committed ticks, making unsmoothed motion easy to spot.
Movies still present decoded frames without blending. Use normal speed for FPS
comparisons: slow motion intentionally reduces the amount of simulation work.

The authoritative simulation remains at 60000/1001 Hz. An isolated branch executes
one ordinary update with the latest latched input. Render frames reuse the branch
until its source session or inputs change, including held axes and latched presses.
Service completion tokens are compared with their fork-time values, so asynchronous
callbacks also invalidate it. The fixed clock's overstep fraction blends committed
and predicted presentation. Predictions never replace real state or dispatch services;
a committed update needs a fresh next-tick endpoint even with unchanged input.
Input presses, RNG, operation scopes/IDs, audio, saves and transitions stay owned
by the committed simulation.

The branch retains the field, skit, menu, overworld controller, or battle visual
endpoint until invalidated. Two fixed 64 MiB arenas are reserved once. One owns the scene
endpoint and cached field skeleton/secondary-physics endpoints; the spare is
reused for transient skeleton, physics and UI evaluation. Cache hits only blend
the retained bone poses. Invalidating a prediction releases all its endpoints
before rotating slots. Nested Rust collections,
VM stacks, strings and operation tokens use those arenas, including the first
prediction. Immutable assets are shared. Allocations retain arena ownership, so
an escaped value prevents reuse instead of allowing its memory to be overwritten.
No arena grows. If a branch exceeds its reserved slot, emergency system storage
lets ordinary Rust collections finish safely. The scope discards that result and
returns an allocation error; it never publishes an oversized prediction. A failed
prediction logs once and disables smoothing for the rest of the run. Live simulation
continues. The allocator keeps an ownership header on system allocations as well.
Rendering and GPU publication remain outside these scopes.

Battle predictions execute the same CPU visit as committed combat, including the
lifecycle, candidate rewards, models, event playback, feedback, and particles.
All mutable state belongs to the branch; prepared assets are shared. Audio,
rumble, HUD requests, and persistent results are dispatched only by committed
visits. Render frames reuse cached bone, scenery, and accessory samples.

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
calls, secondary solver invariance, local joint offsets, affine scale/shear,
collapsed parents, rotating particle dimensions, matrix translation boundaries
and UI content changes. The cooked-field test additionally
runs four predictions per real step, including a ring action, and reports CPU
fork-plus-step timing and peak arena use:

```sh
RESONANCE_WORLD_ASSETS="$PWD/local/all-assets" cargo test -p resonance-presentation \
  speculative_ticks_match_real_field_steps_and_report_cpu_cost -- \
  --ignored --nocapture
```

The battle check compares each predicted frame and particle against the next
committed visit, checks that prediction preserves live state and RNG, and reports
the same CPU and arena metrics:

```sh
RESONANCE_TEST_ASSETS="$PWD/local/all-assets" cargo test -p resonance-presentation \
  speculative_battle_visits_match_committed_models_effects_and_rng -- \
  --ignored --nocapture
```

The Demon Fang regression checks both sword attachment and constant local joint
offsets at five render fractions throughout the cooked animation:

```sh
RESONANCE_TEST_ASSETS="$PWD/local/all-assets" cargo test -p resonance-presentation \
  demon_fang_predicts_weapon_trails_without_changing_committed_battle -- \
  --ignored --nocapture
```

A muted finite live probe exercises title-to-field handoff, dialogue and resize
with smoothing enabled:

```sh
RESONANCE_PROBE_SMOOTHING=1 cargo run -p resonance-presentation --example window_probe -- \
  /tmp/resonance-smoothing-probe 1280x720 local/all-assets
```

`RESONANCE_PROBE_TITLE=1` holds the title for a finite 20-second run.
`RESONANCE_PROBE_SPEEDS=1` runs a 20-second title check that presses F10 through
all four speeds and captures the speed overlay after each change.
`RESONANCE_PROBE_TITLE_CAPTURE=1` instead advances at four renders per fixed update
and captures the title lighting transition at ticks 614–620. This diagnostic is
for visual comparison, not throughput. `RESONANCE_PROBE_PROFILE=1` adds render
diagnostics and `smoothing_ns` stage times (simulation, field poses, title poses,
affine propagation) to the frame log. Window titles identify smoothing ON/OFF.

Use a fresh output directory. Run timings without concurrent builds or captures;
CPU prediction timings do not measure compositor or input-to-display latency.

Local validation on 2026-10-27 used an M2 Max, Mesa 26.2.3 Vulkan, the optimized
development profile, profiling enabled, a 1280×720 render target and a stable
1707×960 physical window. Formation 1 in arena 13 ran for 30 seconds from the
same checkpoint; these figures exclude the first five seconds:

| Battle smoothing | Average FPS | P99 frame time |
| --- | ---: | ---: |
| Disabled | 392.9 | 3.878 ms |
| Enabled | 410.7 | 3.822 ms |

The enabled run built 1,502 predictions and reused them 8,765 times. Prediction
CPU cost was 0.237 ms median and 0.577 ms P95. Both runs recorded zero late reads,
audio underruns and callback deadline misses. Treat these as local regression
samples; repeat paired runs before drawing conclusions from small differences.
The title-to-field/dialogue/resize probe also completed, with a stable classroom
interval averaging 335.5 FPS and 0.108 ms median prediction cost.

Freshly cooked integration checks compared 960 field predictions and 120 battle
predictions against committed updates, with zero system allocator calls. Peak
arena use was 0.20 MiB for field 308 and 0.45 MiB for arena 13. These samples
exercise ordinary content; the separate overflow tests cover slot exhaustion.

## Repeatable probes

All commands below are silent or device-free. Use a new output directory per run.

```sh
# Full New Game, classroom interaction, doorway event and restart.
cargo run -p resonance-presentation --example new_game_capture -- local/checks/new-game
# High-resolution capture; independent of native-resolution oracle APIs.
cargo run -p resonance-presentation --example display_capture -- local/checks/1080 1920x1080 raine-question-oracle
# Real window, field preparation and forced window-size changes.
cargo run --release -p resonance-presentation --example window_probe -- local/checks/window 1920x1080
# Five-minute classroom throughput run at a fixed render resolution.
cargo run --release -p resonance-presentation --example frame_benchmark -- local/checks/benchmark 1920x1080 300
# Ten seconds in a saved field, then 30 seconds in formation 1, arena 13.
cargo run --release -p resonance-presentation --example frame_benchmark -- local/checks/battle 1920x1080 30 plain local/checks/save.json 1 13
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
contract. It does not enable resizing in ordinary play. The benchmark records
the compositor's actual physical window size and rejects changes during measurement.
Compare runs only when both render and physical window sizes match.
Use `plain` for normal throughput. `profile` adds render-stage diagnostics and
their overhead; compare profiled runs separately.
`RESONANCE_TEST_ASSETS` selects the benchmark's cooked assets. For the encounter
probe, `RESONANCE_BENCH_REPUBLISH=1` repeats battle publication on every display
frame to measure its cost against publishing once per simulation update.
An optional replay JSON after the save, formation and arena arguments drives the
encounter with ordinary input and starts it as soon as the field is ready.
Its capture steps write timing and effect-pool
markers to `markers.jsonl` without taking screenshots or changing the clock;
the replay's end replaces the benchmark duration. Use `battle_capture` separately
for visual comparisons so screenshot work does not affect frame measurements.
Audio starvation, output underruns, backend xruns and callback deadline misses
must be counted separately; a short callback alone does not prove clean delivery.

Performance dumps include cumulative `prediction_built_reused` counters (builds,
cache hits). Compare their deltas over the measured interval to verify reuse;
`--speculative` should normally build about once per committed tick with stable
input, regardless of render FPS.
