# Dolphin oracle

Run from the repository root inside `nix develop`. Reusable inputs belong in
[cases](cases/); discs, savestates, captures and reports stay in ignored `local/`.
Capture output directories must be fresh.

Group related fixes, build once, then run independent cases with up to 12
workers. Give each worker a separate output directory; fresh Dolphin recordings
also require isolated profiles, displays and watcher sockets. Retain exit codes,
timings and failed comparisons. Reuse verified source captures when only native
code changes.

**All validation is silent.** Dolphin capture enforces `Backend=No Audio Output`,
`Muted=True` and `DumpAudio=True`, without changing desktop settings. Native
recorders use no output device; interactive recording requires `--silent`.
Audio is still recorded and compared.

## Paired replay

```sh
cargo build --workspace --bins --examples
target/debug/resonance-oracle pair tools/oracle/cases/school-grounds-pair.json \
  --disc /path/to/Disc1.rvz --output local/oracle/school-grounds-pair
```

Each `*-pair.json` pins the disc, native save/replay, Dolphin savestate and consumed
DTM prefix, emulator profile, common starting state and named comparisons. Its
matching `*-keyboard.json` supplies ordinary native input. Case descriptions
record fixture grants, source observations, timing origins and coverage limits.
The suite directory covers field travel, dialogue, skits, shops, menus and saves;
use the manifests as the case catalogue.

Pair checks use native 640×480 framebuffer output, before display-position
adjustment, matching Dolphin XFB dumps. Native metadata must declare
`output_stage: "framebuffer"`. Map/story must match at every registered frame;
player and camera coordinates/headings have separate 0.01-unit/degree gates.
Optional manifest fields enable menu, party, prompt, animation and other state
checks. Changed source storage pointers invalidate observations instead of
allowing stale session data to pass.

For native iterations, add `--reference local/previous-pair/dolphin`. Reuse requires
complete captures with matching disc, DTM, savestate, emulator/profile and required
memory observations. Cached images must match the video, VI registration and
individual hashes; additional samples come from the same pinned recording.
The report retains reference provenance. Re-record when source inputs, profile
or required observations change.

`--native-reference local/previous-pair` also reuses native output for comparison
analysis. It requires the same pinned save/replay and records the original
renderer hash when available. **It does not validate subsequent runtime edits.**
Do not loosen tolerances or retime inputs to hide a regression. An intentional
native convenience can differ from the source; keep the failing comparison and
explain it in the run report.

## Source capture and registration

```sh
target/debug/resonance-oracle dtm tools/oracle/cases/title-navigation.json \
  --output local/oracle/title.dtm
python3 tools/oracle/capture.py --disc /path/to/Disc1.rvz \
  --movie local/oracle/title.dtm --output local/oracle/title-reference \
  --frame 2700 --xvfb --watch-state
target/debug/resonance --silent --presentation-start 2365 --tick 964 \
  --replay tools/oracle/cases/native-navigation.json --capture local/native/title.png
target/debug/resonance-oracle compare local/oracle/title-reference/reference.png \
  local/native/title.png --output local/oracle/comparison \
  --tolerance 8 --max-changed-fraction 0.01
```

The pinned `no-blur-v1` profile uses Dolphin 2606, GQSEAF, progressive 640×480,
4:3 aspect, Remove Blur Gecko patches and `DisableCopyFilter`. The launcher uses
regular Dolphin in an isolated user directory: `dolphin-emu-nogui --movie` does
not replay input. `--xvfb` isolates Linux display output; `--watch-state` verifies
the patched instructions. `capture.json` records hashes and completion, with WAVs
under `user/Dump/Audio`. Python tools observe Dolphin; all asset cooking is Rust.

Dolphin PNG indices, VI observations, controller polls and native updates are
different clocks. Register them from observed state and moving frames before
accepting image comparisons. `--presentation-start` is an observed title-entry
counter for probes, not a gameplay default. `--keep-frames` retains intermediate
PNGs; `--keep-duplicate-frames` includes repeated XFB presentations, but blank
intervals may still produce no image.

For loading boundaries, prefer timestamped lossless RGB FFV1 capture with
`--watch-vis N --video`. Extract exact samples with:

```sh
target/debug/resonance-oracle video-frames VIDEO --output local/oracle/frames \
  --first-vi 0 --vi 0 100 300
```

The Rust tool streams Matroska through the Rust FFV1 decoder and retains
container timestamps and image hashes in `frames.json`. No external media
extraction command is required.
It rejects missing presentations instead of choosing a nearby frame. A pair's
`dolphin.video_first_vi` registers the first frame; its frames use `dolphin_vi`
without a PNG index. Multiple video segments require explicit `video_segment`.
A negative first VI must be justified by observed queued presentation and remain
fixed throughout the recording.

Other presentation/animation origins also belong to fixtures, not saves or live
loading policy. Preview origins register a selected model once and reject repeats
or out-of-range samples. A `disc_loading_overlay` exclusion requires observed DVD
transfer state; reports retain the untouched comparison, excluded rectangles and
reason, with all remaining image/state gates active.

## Savestates and controlled fixtures

Use `capture.py --save-state --xvfb` to retain a paused state and companion DTM.
`checkpoint-reference.png` is the last completed dump near the pause; verify its
update/draw phase. Inspect without running the game:

```sh
python3 tools/oracle/state.py STATE.s01 --output local/oracle/state.json
```

For a replay relative to that state, take `N` from `movie.input_count`:

```sh
target/debug/resonance-oracle dtm CASE.json --prefix STATE.dtm \
  --start-poll N --output local/oracle/replay.dtm
```

This preserves the consumed prefix and RTC, replaces future inputs and marks the
DTM as state-based. Supply it with `capture.py --initial-state STATE.s01`.
Poll `stick`/`c_stick` values are byte pairs, neutral at `[128,128]`.

`--watch-state` records frame-end memory; `--watch-vis N` can record a timeline
without PNGs. Actor, particle and volume-group watchers extend observations.
Battle checkpoints retain movie/result metadata for replay; field actor and
particle watches require a field checkpoint because combat replaces that storage.

`resonance-oracle inventory-fixture --help` describes matching changes to copied
Dolphin states and native saves: items, formation, learned records, discoveries,
settings and history. It validates starting values and records input/output hashes.
Original files and game code remain unchanged. Declare these grants in the paired
manifest; controlled menu fixtures do not prove natural story progression.

## Native probes

Replay ordinary keyboard input from a free-control save, recording frames, state
and file-only audio:

```sh
target/debug/examples/checkpoint_replay SAVE REPLAY.json \
  local/native/replay local/all-assets
```

Input changes use one-based game updates and hold keys until the next change;
capture zero is the initialized field. Preparation/readbacks consume no game or
audio time. `recording.json` retains pose, progression, identity and audio commands.
An optional final `WIDTHxHEIGHT` tests other display layouts; paired Dolphin
acceptance remains at native resolution. Quicksaves restart field animations and
music, so register source animation/audio origins separately.

Other device-free tools:

| Tool | Purpose |
|---|---|
| `field_events MAP STORY OUTPUT.json [COOKED_ROOT]` | Sweep entry choices and each registered actor/trigger branch through real field services and offline audio |
| `field_sequence CASE.json OUTPUT [COOKED_ROOT]` | Consecutive animation, movement and effect frames; optional checkpoint and explicit pose origins |
| `classroom_probe CASE.json OUTPUT.png [COOKED_ROOT]` | Registered static scene/pose capture |
| `quicksave_probe SAVE OUTPUT` | Repeated live save/load cycles with timings and rejected-save checks |
| `menu_probe SAVE OUTPUT` | Memory-circle Save and field-menu Load in isolated slots |
| `title_load_probe SLOTS OUTPUT` | Load in a new process, including cancel/reopen and subsequent exploration |
| `new_game_capture OUTPUT COOKED_ROOT exploration REPLAY.json` | Continuous New Game through the supported exploration/save route |

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

These are presentation examples under `target/debug/examples`. The event sweep
fails on missing services, resources, audio or glyphs, and uses ordinary Cancel
input to close shops/menus. It synthesizes every audio request and waits for real
voice completion. Uncooked destinations are failures. Direct event/pose probes
establish service behavior, not walking, collision or audiovisual equivalence.
The paired replay cases establish those separately.

`resonance-game`'s `presentation_trace` example diagnoses script services without
graphics/audio output; `--trigger KEY --follow` invokes a registered event directly.
`--output PATH` exports a lightweight checkpoint when control returns. Use the
player for save identity validation.

## Effect comparisons

The effect suite sends the same scenario commands and controller inputs to
Dolphin and Resonance. Each engine selects its own artwork, animation, motion,
and cleanup. Static checkerboard tiles expose refraction and subtractive blends;
they are stage scenery, never expected effect output.

```sh
cargo build -p resonance-oracle
cargo build --release -p resonance-presentation --example field_sequence
python3 tools/oracle/effect_lifecycle.py --disc /path/to/Disc1.rvz \
  --native target/release/examples/field_sequence \
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
target/debug/resonance --silent --skip-intro --record-music local/native/title.wav \
  --audio-frames 1280000
cargo test -p resonance-presentation startup_fade_overlapping_cues_and_first_loop_match_dolphin -- --ignored
cargo test -p resonance-presentation program_cues_match_dolphin_and_respect_live_group_volume -- --ignored
```

`resonance-oracle compare-audio` compares explicit WAV windows using
`--reference-start-frame`, `--actual-start-frame` and `--frames`; tolerance defaults
to zero. Audio manifests pin source hashes, replay inputs, PCM offsets and any
matched baseline to subtract with `--reference-baseline`. Preserve raw errors;
do not gain-fit or resample recordings to pass comparisons.

`music_compare.py --help` documents fixed registration, drift and per-channel
mean-removed metrics for Dolphin's DSP DC offset. Supplying `--actual-start-frame`
disables alignment search. `voice_content.py --help` compares cooked speech with
recordings. Both tools read files without playback. `record_field_music` and
`record_field_voice` render the ordinary field mixer without a device; Customize
music/mono/voice manifests define the corresponding volume comparisons.
The music recorder accepts a final map ID after volume, fade ticks and stereo
mode to load another field's bank (default: 340), for example:
`record_field_music local/all-assets local/native/ranch.wav 34 120 127 6 stereo 196`.

```sh
python3 -m unittest discover -s tools/oracle -p 'test_*.py'
```

Source PCM agreement does not establish physical device latency or underrun
behavior. See [performance probes](../../docs/performance.md) for those checks.
