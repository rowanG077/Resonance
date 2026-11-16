# Performance experiment history

These notes record experiments and measurements from the renderer migration.
Some configurations and switches described here were subsequently removed.
See [performance and speculative smoothing](performance.md) for current usage
and architecture.

## Experiments on Bevy's renderer (2026-11-13)

Before the native frame renderer, a series of opt-in experiments restructured
the data Bevy's renderer consumed. Each moved results by a few percent; none
reached the 2 ms P99 target, and all were removed with Bevy's renderer on
2026-11-14. Their artifacts remain under `local/high-fps-background-20261112`.

- Retained materials: immutable surface recipes over frame-coherent material
  records, so material changes no longer rebuilt bind groups. The records
  survive as the rows the native frame reads.
- Dynamic geometry arena: battle particles, shadows and wings streamed
  through one storage buffer instead of mesh-asset updates. The native frame
  streams the same vertices through its own vertex buffer; field billboards
  and backdrops, an opt-in extension, stay meshes.
- Scene staging: a reusable staging belt in place of queue writes. A staging
  belt now lives in our wgpu-core fork instead.
- Retained unlit view bindings, minimal native view bindings and the native
  preparation bypass: cheaper view preparation for Bevy's PBR pipelines.
- Retained mesh bindings and native draw bundles: cached Bevy mesh bind groups
  and recorded draw commands.
- Parallel native prediction: the prediction visit on a worker thread. Moving
  it alone did not remove the main thread's budget constraint.
- Retained CPU pose evaluation: cached field samples, overworld endpoints and
  compiled static skeleton branches.
- Ordered world draw consolidation: indexed packets of contiguous compatible
  world surfaces in Bevy's transparent phase.
- Shared skeleton palettes: a Bevy skin patch sharing one palette between
  meshes of the same skeleton.

## Sleeping hidden overworld models (2026-11-13)

Hidden world variants now keep their native simulation, clip clocks, entity IDs,
hierarchy and resident assets while avoiding graphics publication. Their mesh,
material, skin and view-visibility components are parked outside render queries.
The visibility owner publishes current state and restores the same components
before transform propagation and culling on the first visible frame. Whole-world
retirement preserves this per-model ownership. Boot prewarming remains exhaustive.
It adds no visual buffering or input delay.

The correctness batch covered 505 unchanged presentation tests plus the new
repeated hide/wake and whole-world-retirement test, GPU scene re-entry, and exact
RGBA/native observer-state comparisons for on-foot enemies, Rheiards flight and
turning. The pixel comparisons passed again after enabling the default. The
standalone observer now shares the game's world-renderer resource initialization,
includes the fog component required by the camera, and rejects a capture without
the active native projection. All accepted captures had zero late asset reads.

A frozen optimized binary compared ordinary publication with sleeping hidden
models in all five existing overworld scenarios. Both used private offscreen
1920×1080 rendering, MSAA 2×, smoothing on, normal speed, one compute worker,
retained materials/battle geometry/unlit cluster bindings, and GPU-only profiling.
Prediction was synchronous. Other experimental paths were disabled.

| Case | Ordinary FPS | Sleeping FPS | Ordinary frame p99 | Sleeping frame p99 |
| --- | ---: | ---: | ---: | ---: |
| world-foot | 498.3 | 590.3 | 3.252 ms | 2.552 ms |
| world-foot-walk | 485.9 | 557.5 | 3.191 ms | 2.651 ms |
| rheairds-hover | 435.1 | 461.2 | 3.451 ms | 3.272 ms |
| rheairds-flight | 420.4 | 458.5 | 3.531 ms | 3.432 ms |
| rheairds-turning | 408.1 | 437.1 | 3.791 ms | 3.652 ms |

All 142,323 accepted measured frames passed recording, resolution, sample count,
normal speed, prepared-read and complete GPU-query checks. Full saved native
state matched in every pair. The walking replay originally ended with a location
prompt owning control, so its quicksave was rejected. That run is preserved as a
diagnostic. Its post-measurement input now cancels the prompt while saving; timed
movement and fixed-update marker boundaries are unchanged. The complete save
comparison was retained and passed on the corrected pair.

Counters show about 907 meshes parked on foot and 984 in the Rheiards cases.
Foot-idle hierarchy evaluations fell from 3,071 to 1,239 per render frame on
average; p99 fell from 10,460 to 6,590. Main-thread CPU p99 fell from 2.860 to
2.128 ms on foot and from 2.711 to 1.954 ms in flight. All pairs improved total
frame p99, with the largest gains on foot. These single pairs still include
ordinary desktop activity. Draw submission and visible-world preparation remain;
the 2 ms target is not established by these profiled results.

Artifacts and frozen source/binary hashes are under
`local/high-fps-background-20261112/hidden-world-v1-matrix`; default-build pixel
checks and unprofiled integration are under the adjacent `hidden-world-v2-*`
directories. The full 500 FPS p99 goal still includes the field and battle cases.
The next rendering step is actual ordered geometry/material draw consolidation.

The enabled-by-default optimized build then ran without profiling, with the same
1080p/MSAA 2×/normal-speed/smoothing/one-worker and retained material/geometry
settings. Full saved native state matched the profiled sleeping reference in
all five cases:

| Unprofiled default case | Mean FPS | Frame p99 |
| --- | ---: | ---: |
| world-foot | 592.2 | 2.556 ms |
| world-foot-walk | 567.4 | 2.559 ms |
| rheairds-hover | 479.2 | 3.163 ms |
| rheairds-flight | 482.7 | 3.214 ms |
| rheairds-turning | 476.3 | 3.188 ms |

None meets 2 ms p99. These runs verify the default sleeping policy in the selected
fast configuration; retained title materials/geometry and one compute worker
remain explicit experimental settings. Desktop presentation is still unverified.
The active goal remains the full original field, battle and overworld matrix.
Final integration on the default configuration, with experimental streaming flags
disabled, passed all 506 presentation tests and GPU field/world/battle re-entry.


## Display-list frame renderer (2026-11-13)

The earlier experiments restructured data above Bevy's renderer and moved
results by a few percent each. Profiles showed why: frames were bound by the
render thread, and half of it was wgpu-core bookkeeping (a staging buffer and a
gpu-allocator search per `write_buffer`, dense trackers), plus a fixed cost per
render pass on this driver. A standalone wgpu program draws the same 400 ordered
draws in 0.26 ms CPU.

`crates/presentation/src/native.rs` is the game's renderer. In `Last` it selects
the display's active cameras (those drawing into the scene image, the output
camera and offscreen preview and reflection cameras) and compiles their visible
draws into one display list: sorted draws, an 80-byte record per draw, joint
palettes and streamed vertices. Camera activity stays unchanged throughout the
frame; the renderer plugin removes Bevy's per-view visibility checks and camera
extraction. The render world (`native/render.rs`) uploads each table once and
records one pass per target; dissolves, refraction, scene-sampling particles,
SMAA and battle-entry captures add a scene pass. It draws straight into a
window's swap chain: setting the window's `needs_initial_present` has Bevy
acquire the texture without a camera and present it after the frame's commands
are submitted. Content it cannot draw is skipped and named in the log.

Draw order sorts authored orders at the origin depth plus their rank, and
unordered draws at the view depth of their mesh's AABB center, as Bevy's
transparent sort did.

Until Bevy's renderer was removed, `RESONANCE_NATIVE_AUDIT=1` validated this:
Bevy kept drawing every frame, the display list was rendered into separate
images in the same frame, and differences in its draw list or order were
logged. Rheairds flight and Palmacosta Square captures matched exactly (largest
channel difference 4/255). Ten of eleven Meteor battle captures matched; the
eleventh differed by four unordered draws at identical depth, which Bevy broke
by phase insertion order.

Hidden overworld roots' descendants are disabled while a battle presents. About
21,000 of 25,900 entities left every default query, so transform dirty marking
dropped from 11% to 2.6% of the main thread and visibility propagation from 8% to
2.2%. They are re-enabled before any overworld system can run.

These measurements also used deferred prediction: committed update and prediction
each cost about 0.47 ms on the main thread in battle and ran on separate render
frames. That policy has since been removed because displaying the committed pose
without its remaining fractional tick produced uneven motion. Prediction now runs
on committed frames too, so their CPU cost includes both updates. Until a
prediction is available, the renderer keeps the current scene image so a fade or
feedback effect can start correctly on the next committed tick.

Uploads changed in our wgpu-core fork:

- Uploads of 64 KiB or less through `write_buffer` and `create_staging_buffer`
  are slices of shared 4 MiB staging chunks. Each chunk stays mapped and returns
  to a free list when the submissions that read it have completed.
- The staging pool keeps up to 256 buffers per size class. At 64, bursts of
  small uploads while earlier frames were still in flight missed the pool on
  most frames.
- A pool trim only destroys buffers that were idle for two consecutive
  windows, and always keeps four per class. Trimmed buffers caused misses that
  cost a fresh allocation plus first-touch page faults; the slowest 2% of
  display-list uploads fell from about 0.6 ms to 0.25 ms.

`animation::multiply` and `transform_point` write their sums out in
`Iterator::sum` order, starting from its -0.0, so results are bit-identical. The
generic fold was not inlined and took a fifth of the battle step. A test checks
both against the iterator form on random matrices that include signed zeros.
The battle update fell by about 11%, the prediction by 19% and battle
presentation by 12%.

The battle step (`resonance-battle`) visits each model three times per
update: a pose-only pass for contacts, a finish pass, and presentation. Two
changes keep it exact while removing repeated work:

- `AnimatedPose::sample` finds each bone's track through a table built once per
  sample, keeping the first track per bone as `find` did. The linear search
  per bone took about a quarter of animation sampling.
- `Model::present_body` keeps the last composed palette with the local values
  it came from, compared bit for bit (`LocalPose::same_bits`), and reuses it
  when they match. Presentation repeats the finish pass's pose. Palette
  composition fell from 22% to 11% of the step.

- `Palette::set_globals` compares each bone once instead of once for itself
  and once as a parent, using the same `==`.
- Effect banks are shared with prediction snapshots through `Arc`. The weapon
  binding records which variant it installed and copies its bank only when the
  equipped weapon changes; installing the same variant again stored an
  identical copy every update. Cloning the banks was about a quarter of the
  prediction capture on the main thread.

Battle, content, game and presentation unit tests pass unchanged.

The display-list compile now collects the visible 3D and 2D draw entities once
per frame through narrow queries, in the same query order, and fetches full
draw data only for those. It used to walk every draw entity, mostly hidden
pools and HUD layers, once per camera. The same-frame audits are unchanged.

The benchmark's `smoothing_ns` now also reports the committed battle update,
battle presentation, display-list compile and surface record sync (stages 4
to 7). In battle with every switch on, frames with a committed update took
1.57 ms main-thread CPU against 0.79 ms otherwise. The update itself took
0.52 ms of the difference, measured under load. Taking it out of the frame
means running committed simulation off the main thread.

Measured on the M2 Max with Honeykrisp, offscreen 1920×1080, MSAA, profiling off,
load average about 2, 30 s per run:

| Scenario | Bevy renderer | Display list | All three switches |
| --- | ---: | ---: | ---: |
| Rheairds flight | 671 FPS, 2.65 ms | 1236 FPS, 1.75 ms | 1147–1206 FPS, 1.61–1.71 ms |
| Palmacosta Square walk | 745 FPS, 3.70 ms | 1038 FPS, 1.74 ms | 970–1031 FPS, 1.74 ms |
| Meteor battle | 637 FPS, 2.72 ms | 954 FPS, 2.45 ms | 941–970 FPS, 2.51–2.69 ms |

The figures are average FPS and P99 frame time. In battle the P99 frames are
now the frames that publish a prediction. On those, the render thread
re-prepares five to ten battle HUD meshes on layer 26 through Bevy's mesh
allocator, and Bevy's mesh extraction and GPU mesh collection process the
effect draws that changed. Both cost several tenths of a millisecond and
neither is needed while the display list draws the frame. Encoding is the
other render-thread cost: replaying the pass in wgpu-core and the driver's
per-draw state is about 0.35 ms for 416 draws, much of it material bind group
changes.

### Batched queue writes

In battle the render thread spent much of its slow frames in the kernel. An
`LD_PRELOAD` shim that records the call stack of every 128 KiB shared mapping
(`local/high-fps-investigation-20261113/tools/bo_trace.c`; a debugger must not
stop a GPU client on this machine) showed Honeykrisp creating and mapping
command-pool blocks about 630 times per second. Rheairds and Palmacosta
created almost none.

Every pipeline barrier ends Honeykrisp's compute batch, and the next copy
opens a new control stream with a 64 KiB allocation from the command pool.
wgpu-core recorded each `Queue::write_buffer` as a barrier and a copy, so a
battle frame's few hundred writes (Bevy mesh uploads, the display list's
tables, `prepare_lights`, surface records) needed more 128 KiB blocks than a
pool keeps. Raising the driver's limit on kept blocks from 32 to 128 in a
local Mesa build left about 450 creations per second, because one frame
needs more than that.

Our wgpu-core fork now records the pending buffer writes as one barrier
followed by all the copies (commit 8a78a752c on the `rowan/wgpu-core-30.0.1-patches` branch of github.com/rowanG077/wgpu).
`crates/presentation/tests/queue_write_order.rs` checks the ordering against a
CPU model on the GPU. With the stock driver, block creations fell to about 36
per second and main-thread kernel time from 10.8% to 0.9%. In a pair of
interleaved battle runs at load average 2–3:

| Meteor battle | Average | P99 | P99 after entry |
| --- | ---: | ---: | ---: |
| One barrier per write | 955 FPS | 2.61 ms | 2.54 ms |
| Batched writes | 990 FPS | 2.35 ms | 2.30 ms |

Before, 68% of the P99 frames published a prediction. With batching the slow
frames are spread evenly across ticks: their main thread takes the usual
0.65 ms and the render side about 1.85 ms. The render thread still spends
roughly a third of its time in the kernel. Honeykrisp makes one
`DRM_ASAHI_SUBMIT` per control stream, about six per battle frame, because
multi-command submissions are off by default after rare CTS flakes
(`HK_PERF=batch` turns them on). Submissions take 30 µs at the median but
0.75 ms at P99.9, and the driver's timeline waits block for up to 1.2 ms at
P99.9, about as often as slow frames occur. `HK_PERF=batch` cut the render
thread's kernel share only from 33% to 30%. Fewer render passes per frame
would mean fewer submissions and less per-pass GPU work.

Honeykrisp resolves every MSAA attachment in a separate meta render pass
after storing the whole multisampled image (`hk_CmdEndRendering`,
`is_attachment_stored`: "This could be optimized"). Nothing in the shared
Asahi code resolves in the tile. The benchmark takes `RESONANCE_BENCH_AA`
(`off`, `msaa2`, `msaa4`, ...) to override its default 2x MSAA. In two
interleaved pairs of battle runs at load average 3.6–5.4, after the entry:

| Meteor battle | Average | P50 | P99 |
| --- | ---: | ---: | ---: |
| MSAA off | 1191 / 1120 FPS | 0.76 / 0.77 ms | 1.64 / 1.94 ms |
| 2x MSAA | 959 / 831 FPS | 0.91 / 0.95 ms | 2.43 / 3.90 ms |

The second 2x run was disturbed by load. Even so, 2x MSAA accounts for most
of the remaining battle P99. The separate resolve pass is the smaller part: a
timing-only build that kept the multisampled store and skipped the resolve
measured 973 FPS and 2.13 ms P99 against 934 FPS and 2.26 ms (one pair, load
4–6). The rest is the multisampled rendering and its store, which only an
in-tile resolve in the driver would avoid.

### Scene history only when it is read

With `ASAHI_MESA_DEBUG=perf`, Honeykrisp logs every render pass and kernel
submission. A battle frame had six control streams:
- the batched copies;
- the 388-draw scene pass at 2x MSAA, and its resolve;
- a second 2x pass that copied the resolved scene and drew 16 HUD draws, and
  its resolve;
- the output resample.

Palmacosta had the same shape with a copy-only second pass. The second pass
exists so the scene can be kept without UI as the dissolve history. Each frame
swapped the scene into the history, because a dissolve, a fade-out freeze or
feedback blends against the previous frame's scene.

The history is now kept only while something can read it. Battle presentation
and the field sync set `field_dissolve::KeepHistory` when the committed state
or the predicted next update has a dissolve, feedback (including an Over Limit
or rescue burst) or a fade-out. They also set it when there is no prediction,
since another committed tick may arrive before a prediction is built. A finished
fade-in stays in the field world, so only fade-outs count. Otherwise the native
frame draws the overlay views in the scene pass: four control streams instead
of six. A frame drawn that way marks the history invalid. If a dissolve then
starts without having been predicted, both renderers start it from the
current scene, as they do for a new history, and log it at debug level. None of
the eight benchmark cases produced one.

Field frames are pixel-identical, because their second pass only copied the
scene. In battle the HUD now blends into each sample before the resolve
instead of over the resolved scene. Where a translucent HUD draw overlaps a
multisampled edge, that can round differently by one level; the same-frame
audit's native images have always been drawn this way.

Measured on the M2 Max, offscreen 1920×1080, 2x MSAA, profiling off, two runs
each at load average 1.8–2.6:

| Scenario | Average | P50 | P99 |
| --- | ---: | ---: | ---: |
| Meteor battle | 1081 / 1084 FPS | 0.81 ms | 1.86 / 1.86 ms |
| Palmacosta Square walk | 962 / 1020 FPS | 0.90 / 0.88 ms | 1.75 / 1.73 ms |
| Rheairds flight | 1150 / 1227 FPS | 0.77 / 0.72 ms | 1.60 / 1.53 ms |

Before this change, three interleaved pairs at load 1.8–2.5 put the battle at
997–1009 FPS and 2.23–2.28 ms P99.

### Spell-heavy battles are GPU-bound

The other spell cases do not all meet the target. The elemental battle ran at
890–910 FPS with a P99 of 2.6–2.9 ms at load 2–3, and the mixed battle at
1.9–2.2 ms with other processes using 1.2–2.5 cores. Both have four control
streams per frame and are 98% native frames, so neither structure nor
fallbacks explain it. Elemental draws fewer surfaces than the Meteor battle
(260–310), and its render thread is far from busy. Two pairs each, load 2–3:

| Elemental battle | Average | P99 |
| --- | ---: | ---: |
| 1920×1080, 2x MSAA | 893 / 909 FPS | 2.74 / 2.74 ms |
| 1280×720, 2x MSAA | 1413 / 1456 FPS | 1.48 / 1.45 ms |
| 1920×1080, MSAA off | 1053 / 1063 FPS | 2.00 / 1.86 ms |
| 1920×1080, 2x MSAA, multisampled image discarded (timing only) | — | 2.15 / 2.21 ms |
| 1920×1080, 2x MSAA, blended surface draws skipped (timing only) | — | 1.08 / 1.09 ms |

The GPU limits these frames, and nearly all of its time goes to the
translucent spell effects. An in-tile resolve in the driver would remove the
multisampled store and resolve pass: the discard row bounds that at about
0.45 ms, still above 2 ms. On Apple GPUs blending is part of the fragment
shader, so each blended layer pays the surface shader (material record from
storage, up to two texture samples, fog) and, with MSAA, per-sample blending.
The remaining levers are cheaper fragment work for effect surfaces, less
overdraw, and the driver's MSAA handling.

A per-pipeline draw count showed which blended draws these are. They are not
the `CLAMP_COLOR` effect particles: per frame about 44 are textured, lit,
skinned translucent models, 17 textured unskinned surfaces and 6
multiply-alpha particles; 50 more are constant-color. A timing-only build that
shaded blended surfaces with `CONSTANT_COLOR` measured 1.09–1.16 ms P99,
the same as skipping them, so their textured shading is the cost, not
blending or the geometry. A hardware sample in place of the byte-exact
four-texel filter changed nothing, because none of these draws take that
path.

The fragment shader reads its material record through a slot passed from the
vertex shader as a flat varying, so every per-material load, flag test and
uniform product runs per fragment. The native frame passes the slot as an
immediate per surface draw instead, where the device supports immediates
(`NATIVE_IMMEDIATE_SLOT`, `native_frame.wesl` `native_immediate`). The
values and arithmetic are the same, but a slot that is uniform across the
draw lets the AGX compiler move that work into its once-per-draw preamble.
The shader validation test composes the native surface shaders with and
without it. Same-frame audits with it on matched the earlier audits:
Palmacosta stays pixel-identical, Rheairds keeps its one-to-four-level
differences, and Meteor its equal-depth ties, shot for shot. One run per row,
load average 1.7–2.5 except where noted:

| Scenario | Slot as varying | Slot as immediate |
| --- | ---: | ---: |
| Elemental battle | 872 / 925 FPS, 2.80 / 2.74 ms | 977 / 1294 FPS, 1.90 / 1.63 ms |
| Meteor battle | — | 1310 FPS, 1.42 ms |
| Mixed battle | — | 1352 FPS, 1.43 ms |
| Palmacosta Square walk | — | 1078 FPS, 1.59 ms |
| Rheairds flight (load 7) | — | 1309 FPS, 1.34 ms |

Repeats with the immediate slot on by default, offscreen, counting only runs in
which other processes stayed quiet (a per-second monitor of their CPU use ran
alongside):

| Scenario | P99 per run |
| --- | --- |
| Elemental battle | 1.90, 1.63, 1.67, 1.71 ms |
| Meteor battle | 1.42, 1.41 ms |
| Mixed battle | 1.43, 1.36 ms |
| Palmacosta Square walk | 1.59, 1.56 ms |
| Rheairds flight | 1.34, 1.43, 1.38 ms |

Runs during which other processes used up to 11 cores came out at 2.3–4.7 ms;
in those, the slow frames fell in the seconds with outside load.

The figures are average FPS and P99 frame time. In a real window on the
desktop (`frame_benchmark`, 2560×1440 surface, vsync off), the Meteor battle
ran at 1268 FPS with a P99 of 1.57 ms. The native frame drew 97% of the
battle frames directly into the swap chain, and a compositor screenshot
showed the battle, spell effects and HUD correctly. The elemental battle ran
at 1294 FPS, 1.60 ms, with every battle frame native.

`RESONANCE_BENCH_WINDOWED=1` runs the scenario benchmark in a window too, so
the field cases can be measured there. One run each, every frame native and
exclusive, no GPU warnings:

| Scenario in a 2560×1440 window | Average | P99 |
| --- | ---: | ---: |
| Palmacosta Square walk | 1110 FPS | 1.22 ms |
| Rheairds flight | 1082 FPS | 1.76 ms |
| Mixed battle | 1104 FPS | 1.85 ms |

### On by default

The native frame and the configuration measured with it became the default,
with hidden-world suspension, shared skin palettes and world packets. With no
switches set, offscreen, one run each: elemental battle 1038 FPS, 1.70 ms; Meteor 1032 FPS, 1.73 ms; mixed
1114 FPS, 1.61 ms; Palmacosta 943 FPS, 1.51 ms; Rheairds 1085 FPS, 1.48 ms.
In a window, Rheairds ran at 904 FPS with a P99 of 1.74 ms, every frame
native. The same-frame audits match the earlier ones shot for shot.

Title, boot and movie text (`TitleText`) is now drawn natively as
`Material::Text`, the `text` entry of `native_flat.wesl`, with premultiplied
alpha as its Bevy pipeline. The Palmacosta run no longer falls back during
loading for title text.

Refraction (heat haze, ripples and captured field meshes) and the menu backdrop
are native too. Refraction runs Bevy's own pipelines between the scene pass and
the overlay pass, reading the resolved scene and its multisampled depth, with
the uniforms and pulses uploaded by the native frame; the menu backdrop draws
as `Material::Backdrop` and its capture copies the previous scene image before
the frame redraws it, sharing the copied generation with Bevy's pass. Same-frame
audits: six shots in the Triet ruins (`triet-ruins-0220`, heat haze active) and
four menu shots (main menu and Status page) are pixel-identical. One run each:
the Triet ruins 645 FPS, 2.43 ms P99 (Bevy's path: 549 FPS, 3.88 ms); the menu
891 FPS, 1.41 ms.

Saves from the playthrough in `~/.local/share/resonance/saves` include Ghost
Wolf (technique 260, `mods/ghost-wolf`); they load with the mod built for the
asset library and passed as `RESONANCE_BENCH_MODS`. Without it, party
validation now names the problem: "invalid saved party member 0: technique 260
is not one this character can learn".

The model preview (monster list, figurine book) is native as well. Its models
are plain `TitleSurface`s now, so they get streamed records like field draws;
its camera carries `native::Offscreen`, which selects it alongside the scene
cameras and draws it first, into the preview image, with its own scene images; the
2D quad draws as `Material::Composite`. The preview's readiness fence counts
native draws, so a model loaded while the native frame is active completes.
Audit (`monster-list-shots`: save 080, Items → key items → Monster List, Torent
then Minicoid): four shots pixel-identical. One run: 839 FPS, 1.47 ms P99, 97%
native frames.

The regular SMAA presets (low to ultra) are native: `anti_aliasing::NativeSmaa`
runs Bevy's own pipelines and lookup tables (`smaa::passes`, shared with
Bevy's `reconstruct`) on the native scene image after the dissolve, then the
overlay pass blits the result. Every `Camera3d` gets SMAA in Bevy, so the
preview's pass applies it too. Native scene and history textures allow an sRGB
view, as Bevy's view targets do. Audits with `smaa_ultra`: Palmacosta square
(12 shots), Meteor (11), Rheaird flight (12), monster list (4) and the Triet
ruins with heat haze (6) are all pixel-identical. One run each, `smaa_ultra`:

| Scene | Native, `smaa_ultra` |
|---|---|
| Palmacosta square walk | 947 FPS, 1.38 ms P99 |
| Elemental battle | 1046 FPS, 2.37 ms P99 (MSAA 2x: 1157 FPS, 1.44 ms) |
| Meteor battle | 1070 FPS, 1.68 ms P99 |
| Rheaird flight | 979 FPS, 1.85 ms P99 |

In the elemental battle's spell section SMAA raises GPU time from 0.65 to
0.84 ms (mean) and 1.05 to 1.51 ms (P99); dense effects give the blending
weight pass many edges.

SMAA S2x is native too. As Bevy's `render_s2x` replaces the main pass, the
scene part draws into S2x's readable two-sample image (`NativeSmaa::s2x_target`),
then `s2x_reconstruct` calibrates the sample order once, separates the samples
and reconstructs both into the scene image, before particles, refraction and
the dissolve; until its pipelines are ready both renderers draw plain MSAA.
Audits (capped, see below): Palmacosta square (12 shots), the Triet ruins with
heat haze (6) and the monster list (4) are pixel-identical. In the Meteor
battle shot 00 differs (0.7% of pixels around a spell's ground disc) because
Bevy queues 18 newly visible effect entities a frame after the native frame
draws them; a capped `smaa_ultra` audit shows the same, uncapped audits do
not. Two smaller differences remain, with matching draw lists:

- Capped MSAA 2x: up to about 2,000 pixels, nearly all by exactly 1 level,
  all under translucent damage numbers. An ordinary native frame draws the
  overlay views in the same two-sample pass as the scene, so overlay
  fragments blend over each scene sample; Bevy resolves the scene camera
  first and its overlay camera writes the resolved colour back into both
  samples (MSAA writeback) before drawing. Where a pixel's two scene samples
  differ slightly, the results differ by about a level. Frames that keep the
  dissolve history take the split path, which matches Bevy exactly; capping
  changes which path the shot frames take. This is the single-pass design,
  which saves a pass per frame; matching Bevy here would cost it back.
- S2x: 8 to 43 pixels per shot in small clusters on 3D model outlines
  (hair, an arm, a hand), varying between runs. Dumping both renderers'
  separated samples with the audit showed why: per frame, 1 to 4 single samples on edges differ, nearly
  always in the second sample, and the reconstruction spreads each into a
  small cluster. Most differ by a factor of two, the signature of a sample
  that a double-sided additive or equal-depth surface covers twice in one
  renderer and once in the other. Both vertex shaders compute positions
  with the same operations (joint matrices `joint.affine() * bindpose`,
  weighted skinning, world then clip), but they are separate programs over
  differently packed buffers, and WGSL does not promise bit-identical
  positions across programs, so a sample lying exactly on an edge can fall
  either way. MSAA 2x shows the same flips halved by the resolve (the few
  larger pixels above); a single sample at the pixel centre rarely hits one.

The floating damage numbers used to share depth 31 with the unison frame and
an escape image (both at `Meters + 1`), leaving their order to each renderer;
they now draw at 31.5, above those and still below the other meter layers. The audit now
also compares draw lists on screenshot frames (`Frame::screenshot`), which is
how both causes were found: at shot 00 Bevy lacks the 18 effect entities, at
the other shots the lists match (apart from the order of the four
side-by-side party portraits, which share a depth but never overlap).

`RESONANCE_OFFSCREEN_FPS=<fps>` paces offscreen runs. Audits now run at 120 FPS
so they do not occupy the GPU the desktop shares; replay steps count
simulation ticks, so pacing does not change what they show.

No cooked mesh has morph targets (none of 18,769 glTF files in either cook, nor
the Ghost Wolf mod), so the morph-target check is only a guard.

Replay waits added for these cases: `items_category`, `item_selected` (the
inventory is a two-column list: left/right step one item), `items_focus`
`categories`, and the `monsters`/`figurines` menu pages. A timed-out wait
reports the menu page, selection and inventory list.

Showing the performance panel (F3, or `RESONANCE_PERFORMANCE_OVERLAY=1` from
the start) used to hand every frame back to Bevy's renderer, because the panel
was a sprite on a camera of its own. The panel, like the developer overlay
(Shift+Tab), is now a `native::Panel`: an image and a window rectangle the
output pass draws at its end, alpha-blended and clipped by the viewport. The
panel redraws its numbers once a second. With the panel shown, the windowed
elemental battle ran at 1429 FPS with a P99 of 1.12 ms; before, it was 638 FPS
and 2.58 ms.

## Initial smoothing validation (2026-10-27)

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
