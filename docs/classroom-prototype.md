# Modern classroom prototype

The desktop player's default build enables Bevy 0.19.1 Solari in the Iselia
classroom (map 340). Opaque room and character geometry use PBR materials, direct
and indirect ray-traced lighting, a warm sun, emissive glass panes on all four
windows and restrained ceiling fill. The windows use copies of the model's two
glass groups on the side and back walls, including the window behind Lloyd. Their emission faces
into the room and sits just inside the opaque wall trim. The ceiling-height
fill is much weaker than the windows, so their light dominates and sheltered
surfaces retain their shadows. Characters both cast and receive the room's
lighting. Scripted wall hiding does not turn off the windows' light. F6 switches
between modern lighting and the authored renderer. Other maps retain their
original rendering. `--no-ray-tracing` starts with the authored renderer and does
not install the prototype.

The modern view favors a bright, painted daylight palette: golden direct sun,
cooler skylight from the windows and a modest ceiling bounce to keep faces
readable in shade. A 0.6-stop exposure lift and 15% saturation boost apply to the
3D camera before the separate dialogue compositor. Window and contact shadows
remain visible, and the matte character materials retain their diffuse shading.
F6 and leaving the classroom reset the color grade with the modern lighting.
Head emotes and status symbols are projected into the ungraded dialogue overlay,
preserving their world-space anchors, HD artwork and opacity. Their colors bypass
scene tone mapping and grading, including the HD pack's intended RGB 231/232/231
bubble fill. No white-point correction or texture edits are applied. The original 3D
bindings remain available and warmed for F6 and other maps.

```sh
cargo run -p resonance -- --silent --skip-intro --resolution 1600x1200
```

Solari needs a GPU/driver exposing its required wgpu features, including
`EXPERIMENTAL_RAY_QUERY`. The application checks the actual render device. If it
cannot run Solari, the same classroom materials use Bevy forward PBR, sunlight,
ambient fill and cascaded shadow maps. This fallback is **not ray tracing**; it
lets unsupported machines preview the HD art and lighting. The Apple M2 Max /
Honeykrisp Mesa 26.2.2 device used for development takes this fallback. Startup
logs identify which renderer is available. Build with `--no-default-features` to
omit Solari entirely.

Lavapipe (listed as `llvmpipe` by Vulkan) can render the actual Solari prototype
on this machine. Use the software launcher to capture the classroom directly,
without the movie or title menu:

```sh
tools/ray-tracing/lavapipe.sh
```

This writes `local/raytraced-classroom.png`. The launcher also accepts any command,
for example the interactive player with the local classroom checkpoint:

```sh
tools/ray-tracing/lavapipe.sh cargo run -p resonance -- \
  --silent --load local/classroom-prototype-save.json
```

The player and Solari examples apply the aarch64 Linux driver settings before
starting Bevy, including when launched directly with `cargo run --release` or
`target/release/resonance`. They restart the process once with the corrected
environment, preserving arguments. There is no need to export these settings
in the terminal. The launcher additionally selects the software Vulkan driver.

Four compatibility issues were isolated on aarch64 Mesa 26.2.2 / LLVM 21.1.8:

* Unused cooperative matrices must be disabled when creating the wgpu device.
* The default 128-bit software vector width exposes four-lane subgroups, but
  Mesa's acceleration-structure radix-sort shader requests eight lanes. GDB
  located the segfault in its `rs_scatter` global store. Startup sets
  `LP_NATIVE_VECTOR_WIDTH=256`, giving eight lanes. Repeated captures complete;
  512 bits is not a safe alternative on this driver.
* The normal user Mesa disk shader cache reproduced another segfault in
  `lp_rast_shade_quads_mask_sample` even with 256-bit vectors. The identical
  binary completed with `MESA_SHADER_CACHE_DISABLE=true`. The prototype now
  bypasses this cache without deleting it. This setting also disables Mesa's
  disk cache for the GPU fallback in aarch64 Linux Solari runs, so shader startup
  can take longer. Earlier sandbox captures did not exercise this shared cache.
* Combining temporal and spatial reservoir reuse produced almost black output
  even in an isolated sun/floor/cube scene. On Lavapipe the application resets
  Solari's temporal history each frame. Spatial sampling, direct and indirect
  ray tracing remain enabled. This is a workaround for the observed driver/Bevy
  combination, not a claim that its underlying sample-reuse bug is fixed.

A 7×7 filter smooths stochastic illumination on opaque scenery and characters,
using depth and surface normals to preserve geometry boundaries. It demodulates the HD base color before
filtering, then reapplies it to preserve texture detail. This runs between the
opaque pass and the authored transparent pass. Character cutouts and outlines,
emotes, particles and dialogue are drawn afterwards and never enter this filter.
Full-scene TAA is disabled so its jitter/history cannot smear the authored billboards.

The modern classroom view enables Bevy SMAA Ultra with its real area/search
lookup textures. It smooths 3D edges after tonemapping and color encoding, before
refraction and the separate dialogue compositor. It uses no camera jitter or
previous-frame history, so dialogue stays sharp and moving silhouettes have no
temporal trails. This applies to interactive play, the PBR fallback and all
capture quality presets. F6 removes SMAA with the rest of the modern view.

Interactive rendering uses spatial filtering without accumulating stale poses.
CPU rendering remains slow and can retain noise. For a quality showcase, the
offline capture tool freezes each game tick and averages 256 Solari lighting
evaluations in linear HDR before applying the filter and drawing the overlays/UI.
History resets for every saved tick; invalid radiance samples are rejected before
they can contaminate the average or neighboring pixels. This accumulation is
separate from Solari's driver-sensitive temporal reservoirs. It trades render
time for cleaner images without advancing the dialogue or animation during sampling.
The offline sampling loop runs without a frame-rate cap; 60 fps describes the
encoded video and simulation cadence, not the rate of lighting evaluations.
At the high preset, each saved frame requires 256 rendering evaluations, so
capture speed is not an interactive frame-rate benchmark.

```sh
# First second, high quality: 1920×1080, 60 frames, 256 lighting samples/frame.
tools/ray-tracing/capture-classroom.py --max-duration 1 --output local/classroom-test-1s
# Capture one second starting 15 seconds into the event.
tools/ray-tracing/capture-classroom.py --from 15 --max-duration 1 --output local/classroom-middle-1s
# Full classroom event: stop after the frame that gives Lloyd player control.
tools/ray-tracing/capture-classroom.py --output local/classroom-event
# Quick preview of a later section; this intentionally retains ray-tracing noise.
tools/ray-tracing/capture-classroom.py --quality low --from 15 --max-duration 1 --output local/classroom-smoke-1s
# Validate the complete event and handoff without rendering.
tools/ray-tracing/capture-classroom.py --plan-only --output local/classroom-plan
# A single visible classroom frame for checking lighting and HD dialogue.
tools/ray-tracing/capture-classroom.py --start-tick 818 --max-duration 0.016666666666666666 --no-video --output local/classroom-still
```

The duration limit measures **video time**, not wall time. The event starts
immediately after the story movie completes, retaining the scripted initial
black/fade and dialogue; no movie decoder or audio device is opened.
The room becomes visible about eight seconds into the event; a one-second test
therefore captures the opening dialogue against black. One game update becomes
one frame in the 60 fps output (the original game's wall-clock
cadence is 60000/1001 Hz, so this showcase plays 0.1% faster). Dialogue
advances after its text and voice timing finish, plus a two-second reading hold.
No movement is injected. Rendering stops on Lloyd's control handoff or the
requested duration, whichever comes first.

`--from SECONDS` skips ahead from the start of the event without rendering the
earlier frames. It uses the same dialogue/voice timing and reading holds as a
full capture, including holds already in progress at the selected start.
Fractional seconds are supported and round up to the next 60 fps frame.
`--max-duration` limits the clip length after that offset. Omit it to capture
from the offset through Lloyd's handoff. A start beyond the handoff fails before
starting the renderer. `--from` also works with `--plan-only`; it cannot be
combined with the diagnostic `--start-tick` option.

`--quality` selects render resolution and lighting samples. All presets use
actual ray tracing, retain the same 16:9 framing, and output 60 fps with identical
event timing. Low quality is intended for quick timing and camera checks.

| Quality | Resolution | Lighting samples/frame |
| --- | --- | --- |
| `low` | 640×360 | 1 |
| `medium` | 1280×720 | 32 |
| `high` (default) | 1920×1080 | 256 |
| `ultra` | 3840×2160 | 256 |

`--samples` accepts 1 through 65,536 and overrides the preset's sample count
while keeping its resolution; for example, `--quality ultra --samples 16384`. Full
path-traced captures are unfiltered: increasing samples reduces Monte Carlo
noise while retaining the original texture detail. Four times as many samples
roughly halves the noise and takes four times the ray-tracing work. The selected
dimensions, sample count, and per-frame render time are recorded in the metadata. `--plan-only` validates a preset
and the event timing without rendering.

The capture defaults to `--lighting pathtraced`: Bevy 0.19.1's reference path
integrator adapted to the classroom compositor, with jittered camera rays,
multiple importance sampling, and successive diffuse/specular bounces terminated
by Russian roulette. It uses Bevy's ray scene and material sampling, without
Solari's realtime ReSTIR approximation. Light proxies stay invisible to camera
rays; back faces of the room shell are culled only for camera rays. Transparent
authored effects, dialogue and emotes remain on their existing passes. The HD
texture pack and UI colors are unchanged. The interactive game still uses Solari;
`--lighting solari` selects that renderer for comparison captures.

`--smaa strong` (default) extends the stock Ultra preset with a 0.025 edge
threshold, 64 horizontal/vertical search steps and 20 diagonal steps. Use
`--smaa ultra` for unmodified Bevy SMAA. Path tracing additionally averages
subpixel camera samples. Accumulation resets at every video frame; batches of up
to eight paths per pixel avoid repeating CPU scene work for every sample.

```sh
# Full 4K event with sound; first validate a visible frame on the remote GPU.
tools/ray-tracing/capture-classroom.py --quality ultra --from 13.566666 --max-duration 0.016 --no-video --output local/pathtraced-validation
tools/ray-tracing/capture-classroom.py --quality ultra --output local/pathtraced-classroom-4k
```

Each video frame freezes the game, camera and poses while lighting accumulates.
The script writes `frames/frame-000000.png` onwards, per-frame JSON metadata,
`settings.json`, `recording.json` with the actual stop reason, and a high-quality
H.264 `classroom.mp4`. It checks that no frames are missing before encoding at
exactly 60 fps. PNGs remain lossless. The default soundtrack is recorded through
the actual field mixer (music, sounds and voices), replaying pre-roll before
`--from` so existing sounds remain continuous. `audio.wav` contains stereo PCM
and the MP4 includes 320 kbps AAC; `audio-events.json` records command timing.
No audio device is needed and render time does not affect synchronization.
Use `--no-audio` to omit sound or `--audio-only` to validate the complete soundtrack
without rendering. `--plan-only` continues to report timing without audio. FFmpeg must be installed unless
`--no-video` is used. The output directory must be new.

The default renderer is a ray-query-capable GPU. Use `--renderer lavapipe`
for software rendering with eight workers; `--threads` changes this. Unsupported devices are rejected
instead of silently capturing the raster fallback. Higher sample counts reduce
noise further. CPU rendering can require hours for even a short clip at high
sample counts. `--no-build` reuses an already built capture example; normally the
script builds it first. The optional `--start-tick` is only for diagnosing later shots; omit it to capture
from the start of the event.

For a remote NVIDIA machine, place the cooked assets at `local/cooked`, including
`local/cooked/overrides` for the HD classroom, character and UI replacements.
These generated assets and the texture archive are ignored by Git; copy them
separately or cook/import them on that machine. With a working NVIDIA Vulkan
driver, Rust, Python 3 and FFmpeg installed, capture a short GPU test with:

```sh
tools/ray-tracing/capture-classroom.py --renderer gpu --quality high \
  --from 15 --max-duration 1 --output local/classroom-gpu-test
```

The script builds the capture example for the remote machine's architecture.
Omit `--from` and `--max-duration` to record the full event. GPU rendering is
the default. Use `--lighting solari --quality ultra --samples 256` for the
standard ray-traced 4K showcase; `--lighting pathtraced` selects the reference
path integrator. Keep the chosen lighting mode explicit for long captures.

The prototype converts copies of rigid, opaque scenery to Solari's required
position/normal/UV/tangent and 32-bit-index layout. Glass, translucent shafts,
particles, outlines and character cutouts keep the authored shaders. Visible
opaque actor geometry is copied into Solari after applying the current joint
pose, and those posed surfaces also populate the deferred lighting pass. They
retain the HD textures and animated expression/costume UVs, receive shadows and
bounced light, and cast shadows onto the room and other characters. Character
materials use maximum roughness and zero base reflectance to
keep the painted skin, hair and clothing from acquiring a uniform glossy sheen.
The prototype's Solari BRDF treats zero-reflectance nonmetals as diffuse-only,
including grazing angles and indirect hits; ordinary material settings retain
Bevy's original response. Diffuse light and traced shadows still shape the
characters; rigid props retain their own material response. Separate
raster meshes preserve authored vertex colors, which Solari's acceleration
layout cannot contain. Animated normals follow the joints; rigid props use
geometric normals to avoid self-shadowing from inward-facing toon normals.
Unchanged poses reuse their acceleration geometry. Vertex lighting and secondary
material textures are not reproduced in the PBR room
materials. This is a lighting experiment, not a fidelity renderer. Solari's
stochastic lighting can be noisy; DLSS ray reconstruction is not included.

Authored actor and effect materials keep their existing projected shadows and
do not enter Bevy's stock shadow/depth passes. The room's converted PBR materials
cast the fallback's shadow maps; Solari uses ray queries for shadows. This avoids
compiling invalid stock shadow variants when a scripted actor appears after
the movie.

Converted room materials retain their authored face culling. Cutscene cameras
can sit above the ceiling or outside a wall; rendering both faces would cover
the room with those surfaces. The ray scene still contains the solid shell.

HDR color is converted back into the existing encoded scene format before UI
composition. Scripted visibility and field fades remain connected to the room.
Leaving the room or pressing F6 restores original mesh/material bindings and
removes the prototype lights and camera prepasses.

The standalone driver probe compares the same sunlit cube using Solari's
real-time lighting and Bevy's reference path tracer. `--keep-history` reproduces
the sample-reuse issue; `--no-occluder` removes only the cube from the ray scene
to verify that the floor shadow comes from ray queries (shadow maps are off).

```sh
tools/ray-tracing/lavapipe.sh cargo run -p resonance-presentation --features solari \
  --example solari_probe -- local/solari-cube.png
tools/ray-tracing/lavapipe.sh cargo run -p resonance-presentation --features solari \
  --example solari_probe -- local/solari-cube-reference.png --pathtracer
```

## HD texture pack

Extract the supplied archive into the ignored local directory, then build the
classroom override inventory. The conversion utility needs Python, Pillow with
DDS support, 7-Zip for the emote archive, and libxxhash (`--xxhash-library PATH`
can select the library).

```sh
7z x hd-textures.7z -olocal/hd-textures
python3 tools/hd-textures/prepare_classroom.py \
  --cooked local/cooked --pack local/hd-textures/GQS \
  --extracted /path/to/extracted/disc1
```

The utility reads the original TPL intermediates and matches Dolphin `tex1`
names using the original tiled texture bytes and the used palette range. It
rejects ambiguous matches and changed atlas aspect ratios. It converts only the
matched textures to lossless RGBA PNGs and writes
`local/cooked/overrides/classroom.json`, including source names, checksums,
dimensions and missing matches. The supplied pack matches all 76 model/scenery
bindings with 41 unique replacements: 39 at 4× resolution and two at 2×.

`--extracted` also enables dialogue replacements using the extracted disc's
`system.tpl`, font data and executable, plus the emote atlas in `effect.cab`.
The UI/font source checksums must match the cooked
revision. The default dialogue frame and background pattern have 4× textures;
the color mask has a replacement at its original size. The utility assembles
the supplied US font glyphs into the existing atlas layout, preserving glyph
advances and normalized UVs. It matches 197 characters, primarily 2× English
glyphs, with some 4× symbols. Missing glyphs and six unused alternative window
patterns retain their original art. The common emote atlas has a matching 4×
replacement (1024×1024), preserving its animation UVs. The complete inventory has
81 bindings, 45 replacement textures and one assembled font atlas. Omitting `--extracted`
rebuilds a model/scenery-only inventory.

Original cooked assets and their content hashes are unchanged. Overrides load
automatically for map 340, including its NPC and party models and dialogue UI.
Eye, mouth, costume and expression atlases retain their UV layout. The player loads override
bytes before field preparation and warms them through the ordinary material
path. Rename `overrides/classroom.json` to disable the HD overrides.

## Captures and checks

```sh
# 1280×960 prototype capture; uses the supported lighting path.
cargo run -p resonance-presentation --features solari --example modern_classroom -- \
  local/modern-classroom.png
# Same HD art with authored lighting.
cargo run -p resonance-presentation --features solari --example modern_classroom -- \
  local/hd-classroom.png --raster
# Unmodified, native-resolution oracle capture (no overrides or modern lighting).
cargo run -p resonance-presentation --example classroom_capture -- local/original-classroom.png

cargo test -p resonance-presentation --features solari --lib
cargo clippy -p resonance-presentation -p resonance --all-targets -- -D warnings

# Real New Game movie-to-classroom handoff with HD and modern lighting.
# Output directory must not already exist. Omit player-control for the full route.
cargo run -p resonance-presentation --features solari --example new_game_capture -- \
  local/modern-new-game local/cooked modern player-control
# Skip the story movie through the real input handler for rapid regression checks.
cargo run -p resonance-presentation --features solari --example new_game_capture -- \
  local/modern-new-game-fast local/cooked modern-fast classroom-dialogue
```

Classroom preparation renders every authored material and effect in both the
original and HDR camera formats, including currently hidden actors. Activation
waits for both views; a completed original-format draw cannot satisfy HDR warmup.
This also prepares the F6 comparison before gameplay starts.

Solari integration follows Bevy's
[Solari example](https://docs.rs/crate/bevy/0.19.1/source/examples/3d/solari.rs) and
[mesh requirements](https://docs.rs/bevy/0.19.1/bevy/solari/scene/struct.RaytracingMesh3d.html).
Texture naming follows Dolphin's
[TextureInfo](https://github.com/dolphin-emu/dolphin/blob/master/Source/Core/VideoCommon/TextureInfo.cpp).

This workspace also has `local/classroom-prototype-save.json`, a copied
development checkpoint positioned at the rear of the classroom. Open it without
replaying the introduction:

```sh
cargo run -p resonance -- --silent --load local/classroom-prototype-save.json --resolution 1600x1200
```
