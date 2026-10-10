# Gameplay capture tools

Run from the repository root inside `nix develop`. Reusable input recipes belong
in `cases/`; discs, savestates, captures, and reports stay in ignored `local/`.
Each capture needs a fresh output directory and an isolated emulator profile.

These tools capture ordinary gameplay, screenshots, audio, and selected data-memory
observations. Engine development is guided by supported behavior. Comparisons at
matching gameplay events are useful; matching an internal algorithm or update
sequence is not an implementation requirement. Existing captures remain historical
records until a current run establishes a new behavior baseline.

Unattended captures are silent: Dolphin uses `No Audio Output`, `Muted=True`, and
`DumpAudio=True`; native recorders write audio to files. Interactive unattended
recording also requires `--silent`.

## Dolphin capture

```sh
target/debug/resonance-oracle dtm tools/oracle/cases/title-navigation.json \
  --output local/oracle/title.dtm
python3 tools/oracle/capture.py --disc /path/to/Disc1.rvz \
  --movie local/oracle/title.dtm --output local/oracle/title-reference \
  --frame 2700 --xvfb --watch-state
```

The capture profile uses GQSEAF, progressive 640×480 output, 4:3 aspect and
disabled copy filtering. Game patches are disabled. The regular Dolphin launcher
is required because `dolphin-emu-nogui` does not replay movie inputs. Captures from
the retired blur-removal profile need a fresh visual baseline.

`capture.json` records input identities, effective configuration, completion,
images, and audio files. `--watch-vis N` records a data-memory timeline without
PNG dumping; add `--video` for timestamped lossless video. `--watch-locations`
accepts a JSON map of data-memory pointer paths to observation names. Default
watches cover current paired gates and pose diagnostics; retired menu clocks and
internal state are not collected. Actor, particle, synopsis, and volume-group
options provide explicitly requested bounded data observations.
Explicit location names override defaults; conflicting explicit requests are rejected.
These observers do not pause gameplay or install executable breakpoints.

Use `--save-state --xvfb` to retain a nearby paused checkpoint and its companion
DTM. `checkpoint-reference.png` is the last complete image near that pause and
may precede the saved state. Inspect the checkpoint without running it:

```sh
python3 tools/oracle/state.py STATE.s01 --output local/oracle/state.json
```

Checkpoint inspection reads replay identity and available field/story origins.
Use `--field-origin` to require those origins, or `--actors`, `--particles`, and
`--battle` to discover the storage needed for those observations. Unrequested
audio, animation, script, and random-generator internals are not decoded.
`--party` exports the observed formation and member statistics used by
`checkpoint_fixture --party-stats`, retaining the source checkpoint hash.

For state-relative input, take `N` from `movie.input_count`:

```sh
target/debug/resonance-oracle dtm CASE.json --prefix STATE.dtm \
  --start-poll N --output local/oracle/replay.dtm
```

This retains the consumed input prefix and RTC and replaces future inputs. Pass
the resulting DTM with `capture.py --initial-state STATE.s01`. Stick coordinates
are byte pairs, with neutral at `[128,128]`.

## Native capture and comparison

```sh
target/debug/examples/checkpoint_replay SAVE REPLAY.json \
  local/native/replay local/all-assets 640x480 --paranoid
```

Native replay version 2 uses ordered `hold`, `wait`, and `capture` steps. A wait
names a field, menu, dialogue, battle phase, or title event and has a bounded
update budget. Captures have semantic names rather than prescribed update numbers.
`cases/school-menu-native.json` opens and captures the main menu from a playable
school checkpoint. `recording.json` records state, completed steps, and audio.
Preparation and readback do not advance gameplay time.

A version 2 paired manifest binds those capture names to ordinary Dolphin observations.
Supported gates compare images, audio, field/story identity, persistent party
choices, cooking results, and Items selection. Position, camera, and heading
errors or unavailable readings are reported under `diagnostics`; they do not determine
acceptance. The initial origin requires only `map_id` and `story`, not an exact pose.
Explicit image regions define pixel acceptance; the full-image comparison remains
diagnostic in that case. Without regions, the full image determines acceptance.
Reusing native recordings requires a complete report with the renderer identity
and matching hashes for every recorded artifact. Every named native capture must
occur exactly once, and Dolphin audio must match its recorded capture hash before
comparison. Dolphin images come only from a hashed video recording with a required
`video_first_vi` registration; extracted or reused PNGs are hash checked. The legacy
`dolphin` frame-dump index is rejected. Private animation clocks and
retired UI diagnostics are not part of this format:

```sh
target/debug/resonance-oracle pair PAIR.json \
  --disc /path/to/Disc1.rvz --output local/oracle/comparison
```

For a scripted arrival, `field_sequence` accepts
`"scene": {"arrival": {"checkpoint": ...}}` and
`"at": {"kind": "tick", "update": 0}`. This starts normal arrival scripts
without requiring free control before recording. Declare the copied progress as
a scene fixture; this does not validate save restoration or natural progression.
Arrival positions may be above the floor, as in authored teleporter transitions;
ordinary checkpoint restoration still requires a grounded saved position.
Other scene kinds are `classroom`, `new_game`, and `restore`. Capture moments
are `control`, `tick`, and `dialogue` (with `prefix` and `hold_updates`).
`inputs` is a timeline of complete held controls, for example
`[{"update":50,"buttons":["ring"]},{"update":55,"buttons":[]}]`.
Each entry may also set `direction` and `run`; button edges follow held transitions.
For post-battle scenes, `battle_victories` lists the expected formation IDs in
order. The capture grants each victory through the field handoff and records the
consumed count; unexpected battles or unused grants fail. Declare these grants
in the case: they do not validate combat or its rewards.
Sequences can cover up to five minutes at 60 Hz, including seal dialogue.
An optional `resolution: {"width":1920,"height":1080}` uses the live display
layout for aspect checks. `capture_frames` selects sorted, unique zero-based
render frames while still running the entire sequence; omit it to save every
frame. Paired Dolphin comparisons continue to require 640×480.

Write a fresh paired manifest from observed matching game events and rebaseline
its image and audio checks. Native loading remains fast; report disc-loading and
related audio differences separately. The old injected-state keyboard recipes
and their paired manifests have been retired. No source clock, RNG cursor, pose,
or ambient-state injection is accepted by the native recorder.

The field capture examples (`classroom_capture`, `classroom_probe`,
`particle_probe`, `setup_capture`, `dialogue_capture`, and `field_sequence`) all
accept `CAPTURE.json OUTPUT [ASSETS]`. A capture spec contains an optional
prepared save path and an ordinary version 2 scenario:

```json
{"checkpoint":"local/native/save.json","scenario":{"version":2,"steps":[
  {"do":"wait","until":{"kind":"field_ready"},"max_updates":600},
  {"do":"capture","name":"field"}
]}}
```

The first five examples write one PNG and its held-state sidecar; `field_sequence`
writes the named PNGs, native audio, and `recording.json`. They use the production
application, input, movie/audio completion and GPU readback. There is no separate
field simulation, render-settling counter, pose sampling override, or particle seed.
For ordinary New Game products:

```sh
target/debug/examples/setup_capture \
  crates/presentation/examples/scenarios/setup-capture.json local/native/setup.png local/all-assets
target/debug/examples/classroom_capture \
  crates/presentation/examples/scenarios/classroom-capture.json local/native/classroom.png local/all-assets
target/debug/examples/dialogue_capture \
  crates/presentation/examples/scenarios/classroom-dialogue-capture.json local/native/dialogue.png local/all-assets
target/debug/examples/particle_probe \
  tools/oracle/cases/eraser-particles.json local/native/particles.png local/all-assets
target/debug/examples/field_sequence \
  tools/oracle/cases/classroom-walking.json local/native/walking local/all-assets
```

The classroom product retains geometry/tint inspection. The old Colette phase
fixtures and their pixel gates were retired because an injected phase does not
identify an ordinary gameplay event. Register fresh matching observations before
using those images for pixel acceptance. `no-blur.json` retains its title gates.
The setup product captures the normal default prompt; the former style matrix
required changing preferences before the player could reach Customize.

The dialogue appearance matrix takes a current prepared save positioned at the
matching conversation and an ordinary replay in its `capture` object. Produce
that save from an ordinary New Game/input capture first; the repository does not
ship a historical classroom save. Each variant runs `checkpoint_fixture
--preferences FILE` before loading, so settings belong to the prepared save.
Build that example alongside `dialogue_capture`. `RESONANCE_TEST_ASSETS` selects
the prepared asset root for this matrix. Its exact preference assertion remains
in addition to the registered image-region comparisons.

`field_events`, `quicksave_probe`, `menu_probe`, `title_load_probe`, and
`new_game_capture` cover event enumeration, persistence, and ordinary travel.


## Effect comparisons

The effect suite sends the same scenario commands and controller inputs to
Dolphin and Resonance. Each engine selects its own artwork, animation, motion,
and cleanup. Static checkerboard tiles expose refraction and subtractive blends;
they are stage scenery, never expected effect output.

```sh
cargo build -p resonance-oracle
cargo build --release -p resonance-presentation --example effect_sequence
python3 tools/oracle/effect_lifecycle.py --disc /path/to/Disc1.rvz \
  --native target/release/examples/effect_sequence \
  --output local/effect-comparison --workers 4 --random-cases 500 --seed 17
```

The default run covers all eight stage profiles: field particles, emitters,
emotes, ring casts, stations and model effects. `--random-cases 500` allocates 500 generated
cases across those stages, in addition to each stage's base catalogue. The root
`results.json` reports per-stage coverage and completion; each stage retains its
own detailed report. `--case tools/oracle/cases/effect-tower.json` selects model effects;
`effect-rings.json` selects ring casts. Bomb, bubble, sunlight and shrink have
separate `effect-ring-*.json` stage profiles because their scenery or controller
input differs. Profiles pin source checkpoints, their movie prefix and native
scene inputs; existing cooked assets are used without cooking.

The catalogue includes all 70 implemented emitters, including the new trails,
star orbits, cylinders, light sheets and bursts. They use the same named input
domains for individual comparisons, random compositions and shrinking. Sprite
coverage also includes flames, sparkles, lightning and cooking clouds.
Cloud cleanup runs in each emission phase with both cleanup transitions, in
addition to randomized controls and compositions. Rust tests cover script results,
resource requirements and particle ownership; this suite covers visual appearance.

Required private effect inputs live in `local/oracle-fixtures/effects/`: one
savestate and companion `.s01.dtm` per starting field, plus the native scene
templates. Preserve this directory when deleting capture reports. To prepare a
replacement stage, capture a paused field and its movie using the savestate
workflow above, prepare the matching native scene, and register both file hashes
in its profile. Moving an unchanged checkpoint does not invalidate captures.

Generated cases cover every eligible input before repeating, then combine
finite particles with randomized placement, size, palette, opacity, duration,
casting direction and random seed. Sprites, models, emitters, emotes and actors
use named inputs and timed changes; numeric commands are compiled at the simulator
boundary. Sprite cases cover fractional and integer velocity, speed, direction mode
and all six rotation orders with combined axis rotations, including leaves and refraction.
Generated changes also include moving targets, early quake removal and
emitter-handle reuse. Every complete scenario is saved in
`case.json`. Use `--replay-case PATH` to reproduce it independently of generator
changes. `--only 'sprite-*'` filters case names. `--random-cases 0` runs the
base catalogue only. Visible scenarios must actually appear in the source recording;
intentional no-op emotes are declared explicitly. Whole-story replays remain separate checks of
scene transitions, attachments, callbacks and story progress.

Visual generation and shrinking exclude zero random ranges that can spread
particles beyond the stage. They retain zero-valued controls such as emission
intervals that deliberately disable an effect. The source visibility, containment
and expiry checks validate each generated scenario before judging Resonance.

Ring cases vary casting direction and controller timing. Model cases vary their
heading, scale, opacity and removal time; the tower stage also exercises all eight
bound sprite texture slots. Cases with equal controller timelines share a resident
Dolphin process. These inputs vary independently of added particle layers.

Workers retain silent Dolphin processes between cases. A watched state marker
rejects stale loads, and a white presentation pattern registers the clock
before effects begin. Both renderers use Vulkan: OpenGL's raster edge rules
can produce false geometry failures during camera shake. Generated DTMs select
Vulkan explicitly because their saved backend overrides Dolphin's command line.
On Mesa, headless swapchains allow GPU rendering inside Xvfb; captures record
the renderer and its environment, and older OpenGL captures require replacement.
Every consecutive frame is checked for visual equivalence, including empty
birth/expiry frames. Small rounding patches and scattered raster fringes are
acceptable; the cluster check allows the same one-pixel edge movement as the
geometry check. Concentrated substantial errors must fail even when most of a
larger effect matches. Geometry, overall brightness and local error checks enforce this
without requiring identical pixels. Refraction
uses the last background frame before birth. Reports retain commands, provenance,
per-frame measurements and failed image pairs; failures exit nonzero. No image
alignment, brightness fitting or automatic baseline acceptance is performed.
Successful native frame dumps are removed after comparison; their measurements
and renderer fingerprint remain. Dolphin captures are retained for reference reuse.

`--reference DIRECTORY` reuses verified Dolphin captures and rerenders Resonance.
After an interrupted run, `--resume` reuses complete captures with matching inputs.
The root `plan.json` preserves seeds, selection and the pending work across resumes.
Omit `--random-cases` when resuming or reusing a reference; the recorded plan supplies it.
`--minimize` removes unrelated layers and changes, reduces parameters and timing,
and writes `minimal.json`. Reduced controller inputs receive their own recordings.
Invalid scenarios and capture errors never count as a preserved visual failure;
only the current smallest failure keeps its capture under `minimize/retained`. Keep a minimized
reproducer when a new regression is found.

## Audio and performance

```sh
target/debug/resonance --silent --record-playthrough local/native/playthrough
python3 -m unittest discover -s tools/oracle -p 'test_*.py'
```

`resonance-oracle compare-audio` compares explicit WAV windows using
`--reference-start-frame`, `--actual-start-frame`, and `--frames`. Use
`music_compare.py --help` for drift and channel metrics and
`voice_content.py --help` for speech-content comparisons. These tools read files
without opening an audio device. Report the registration and limits of each check;
waveform identity is not a substitute for correct playback and audible completion.

Builds, cooks, captures, and performance measurements should run separately when
resource contention could affect the result. See [battle support and
validation](../../docs/battle-status.md) for the current rebaselining scope.
