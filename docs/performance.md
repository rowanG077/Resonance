# Performance and speculative smoothing

Use `--perf-overlay` and `--perf-dump PATH` to inspect frame timing. F3 toggles
the overlay and F4 saves recent samples. Compare runs at the same resolution,
backend, settings and workload, without concurrent builds or captures.

On Linux, run windowed probes on a private virtual display when the desktop must
remain available:

```sh
nix develop --command python3 tools/with_virtual_display.py --size 1920x1080 -- \
  cargo run --release -p resonance-presentation --example window_probe -- \
  local/performance/background 1920x1080 local/all-assets
```

The runner starts Xvfb on an automatically selected display, overrides the child
process's display variables and stops its server afterward. It does not connect
to the desktop's display. Game resolution and antialiasing still come from the
probe's arguments/settings. Record the private X11 display as a different
presentation environment when comparing timings; use the same environment for
both branches and avoid other heavy workloads during final measurements.

For throughput profiling without a display server, `scenario_benchmark` renders
live gameplay into a private image. It retains the ordinary fixed update, input,
prediction, preparation and render schedules; this is separate from deterministic
oracle capture. Cases contain `save`, a version-2 `replay`, and an optional
`encounter` with `formation` and `arena`.

```sh
nix develop --command env RESONANCE_PROBE_SMOOTHING=1 cargo run --profile release-fast \
  -p resonance-presentation --example scenario_benchmark -- \
  local/all-assets local/performance/rheairds cases/rheairds-flight.json 1920x1080 90 profile
```

This example explicitly selects MSAA 2× and isolates save/config writes under its
output directory. Omit `profile` for throughput comparisons. Profiling additionally
records `gpu_render_profile`: independent asynchronous timestamp pairs spanning
the complete render graph, with render-frame IDs and workload-relative times.
They exclude CPU preparation, buffer transfers queued before the graph, subsequent
readbacks, and surface presentation. Completed samples are drained rather than
sampling only the latest result. Unsupported queries, invalid results, saturated
readback slots and dropped samples are explicit; do not accept a p99 result with
unaccounted missing GPU samples. Offscreen throughput and desktop presentation
remain distinct measurements.

Benchmark markers are serviced in `FixedPostUpdate` immediately after the input
cursor acknowledges the native update. Catch-up updates in the same render frame
cannot move a marker to a later native tick. Battle comparisons should wait for an
explicit `combat_tick` before the measurement interval and compare committed
state and RNG at both boundaries. Marker timestamps share the frame log's origin.

Set `RESONANCE_PROBE_GPU_ONLY=1` with `profile` to retain stage wall timings and
the independent GPU timestamp pairs without Bevy's per-pass pipeline-statistics
queries. The probe records this as `pass_diagnostics: false`; compare like modes.
`scene_items` counts native frame items with resolved geometry and texture
bindings, including UI and offscreen views. Pipelines may still be pending, and
adjacent streamed items can share a draw call. `probe.json` records the compute
pool size.
On Linux the profiler also records main-thread and render-thread CPU time using
thread CPU clocks. Timing checkpoints run on their schedule's calling thread.
These counters exclude worker execution and GPU work; a wall/CPU difference can
include worker waits as well as descheduling. They diagnose stalls and do not
replace wall-clock frame-time percentiles.

Historical benchmarks and retired approaches are recorded in
[the performance experiment history](performance-experiments.md).

## Native renderer

With the native frame drawing everything, Bevy's core pipeline, sprite and PBR
renderers are no longer installed, and neither is OpenGL, which only ever ran
through them. What the native frame still uses from Bevy is the render world
itself (device, extraction, mesh and image render assets, the pipeline cache,
window swap chains and screenshots), the blit pipeline and the fullscreen
triangle (`renderer::FramePlugin`). Command buffers are submitted by an
exclusive system, so the frame's deferred command buffers are applied before
Bevy presents its windows. Only wgpu-core is patched, from the
`rowan/wgpu-core-30.0.1-patches` branch of github.com/rowanG077/wgpu
(`[patch.crates-io]` in `Cargo.toml`). The former experiment switches and renderer
audit were removed; the native renderer is the supported configuration.

UI batches and dynamic battle geometry publish packed vertices directly to the
native frame. UI no longer creates mutable mesh assets. Each stream snapshots its
ordered source revisions; unchanged sources reuse their CPU bytes and skip GPU
uploads. The profiler's `geometry_streams` reports vertex counts, copied vertices
and actual upload bytes separately for UI and surface geometry.

The existing `scene_upload_copies` and `scene_upload_bytes` fields count queued
writes to title and world material-record tables, including gaps in coalesced
writes. `title_record_*` reports the title-table subset. These counters and
`geometry_streams` leave out native view/coverage/dissolve uniforms, draw and joint
tables, refraction pulses, and Bevy mesh/image uploads; they do not measure total
transfer traffic. The existing field names remain stable for profile consumers.

Pipeline preparation uses the same pipeline keys as real draws, without creating
placeholder materials. Scene admission records camera-visible entities before
resolving their materials, then waits for the render world to draw them. A missing
material therefore remains an unmet requirement.

Without a GPU, validate on Mesa's software Vulkan driver:

```sh
VK_ICD_FILENAMES=/path/to/lvp_icd.json WGPU_SETTINGS_PRIO=webgpu \
  cargo run --profile release-fast -p resonance-presentation --example ci_render -- target/smoke
```

`ci_render --windowed` checks window presentation and capture, for example
under `xvfb-run`; offscreen benchmarks do not present. GPU tests point the
native frame at their own image with `test_support::native_scene`.

## Speculative smoothing

Enable **Enable experimental high FPS** in Customize, or force it on for a run
with `--speculative`. The menu preference defaults to Off and is saved in
`config.toml`; replays and deterministic captures keep smoothing disabled.

Field, battle and overworld predictions are rebuilt as soon as their source or
inputs change, including on frames that commit a tick. Every eligible frame
interpolates its remaining fractional tick; unchanged endpoints are reused.

```sh
cargo run --release -p resonance -- --silent --speculative --perf-overlay \
  --perf-dump local/performance/speculative.jsonl
```

With the menu preference Off, compare the same scene and input sequence with
and without `--speculative`.
For visual inspection, **F10** cycles ½×, ⅕×, ⅒× and normal speed. It opens the
performance overlay with the selected rate; **F3** hides the overlay. The game
clock and audio transport slow together, while rendering follows the selected
VSync mode. The
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
endpoint until invalidated. Two fixed 64 MiB arenas are reserved once. One owns
the scene endpoint and cached field skeleton/secondary-physics endpoints; the
scratch arena is reused for transient skeleton, physics and UI evaluation.
Cached future bone poses are blended at the render fraction. Invalidating a
prediction releases all its endpoints before reusing scene storage. Nested Rust
collections, VM stacks, strings and operation tokens use those arenas, including the first
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
The entry shatter samples fractional time after the battle is ready. Damage,
recovery, and combo lettering follow the battle feedback clock between updates.

Field rendering samples complete future poses, including clip transitions,
scripted bone adjustments, camera-facing bones, wings and secondary dynamics.
Field actors and equipment advance their live cloth solvers from committed poses.
A disposable copy samples the next tick. Guides use the interpolated authored
hierarchy, and equipment outlines share the primary layer’s solved joints. Matrix interpolation preserves affine endpoints, including shear,
reflection and collapsed scales. The same separation applies to menu previews.

Title animation/cameras, startup fades, UI movement/fades, skit portraits,
overworld models/cameras/animations and game-rendered cinematics use predicted
endpoints. Matching UI geometry blends without morphing unrelated text. Billboard
births and deaths cross-fade, preserving continuous light from particle trails.
Atlas-frame changes remain discrete, even when adjacent rows differ by a small UV
offset; newly cooked title/world assets explicitly distinguish scrolls from atlases.
Overworld UV/tint endpoints stay in retained material records; a mesh tag supplies
the render fraction. Common world fog lives in the view buffer. Scenery uses a
fixed tile anchor and the camera moves within that space, preserving native torus
copy selection while avoiding per-frame transform uploads for static terrain.
Committed and predicted poses share the same anchor, including seam crossings;
cinematics retain their camera-relative space. World surface UV transforms,
tints and linear fog depth are prepared at vertices. Fields, battles and
byte-filtered effects keep their original fragment arithmetic and varying layout.
World recipes also select a stable unlit shader variant, omitting field lighting,
unused normal/tangent varyings and nonlinear fog. Cached render frames update only
the draw tags that interpolate UVs or tint; material endpoint publication and
accessory visibility follow source/input revisions.
`WorldSurface` and `TitleSurface` endpoints are rows of shared storage
buffers (144 and 256 bytes) that the native frame binds once per frame; draws
select a row. Extraction snapshots updates for the current render frame, and
retired rows are reused. A retained CPU mirror coalesces changed rows into one
buffer upload while preserving unchanged rows inside the uploaded span. Shared
sampler views use source-image invalidation and a source-to-variant index;
unchanged frames do not scan all resident image aliases. Views whose source
upload is delayed remain pending, and image retirement removes their index and
GPU alias.

Closed, fully prepared catalogue previews retain their model and assets while
parking mesh, material, skin and view-visibility components. Reopening restores
them before pose and visibility updates. Incomplete preparation keeps its live
graph, and replacing or failing a preview safely retires parked handles.
Native hierarchy propagation resolves TRS, authored affine matrices, world
overrides and translation boundaries in one serial walk, with serial ancestor
dirty marking. This avoids correcting the same hierarchy in a second pass and
avoids transient TRS writes that falsely dirty unchanged affine globals. Apps
using Bevy's ordinary transform plugin retain the separate affine correction.
Prewarmed inactive worlds retain their entities, hierarchy and asset handles,
while suspending mesh, material, skin and view-visibility components. They therefore
leave skin-bound, visibility and material extraction queries while fields,
battles or the other world are active. World entry restores those components
and lets the ordinary visibility pass select the current camera's draws.
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
RESONANCE_TEST_ASSETS="$PWD/local/all-assets" cargo test -p resonance-presentation \
  speculative_ticks_match_real_field_steps_and_report_cpu_cost -- \
  --ignored --nocapture
```

The Demon Fang check compares each prediction with the following committed
frame and particles, checks live state and RNG isolation, and reports the CPU
and arena metrics. When the attack requests a weapon ribbon, it checks that the attached
sword tip moves between the committed and predicted poses at half a tick.

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
diagnostics and `smoothing_ns` stage times (prediction, field/world poses, title/UI,
hierarchy propagation) to the frame log. Window titles identify smoothing ON/OFF.

Use a fresh output directory. Run timings without concurrent builds or captures;
CPU prediction timings do not measure compositor or input-to-display latency.
