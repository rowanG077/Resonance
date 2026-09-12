# Dolphin oracle

Run from the repository root inside `nix develop`. Reusable inputs belong in
[cases](cases/); discs, savestates, captures and reports stay in ignored `local/`.
Capture output directories must be fresh.

Read the relevant original routines and recovered asset recipes before a fidelity
batch. Group related fixes, build once, then run independent cases with up to 12
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

The Rust tool uses FFmpeg and retains timestamps/hashes in `frames.json`.
It rejects missing presentations instead of choosing a nearby frame. A pair's
`dolphin.video_first_vi` registers the first frame; its frames use `dolphin_vi`
without a PNG index. Multiple video segments require explicit `video_segment`.
A negative first VI must be justified by observed queued presentation and remain
fixed throughout the recording.

Ordinary play continues as soon as resources are ready. Oracle-only
`resource_waits` can register an observed DVD delay using a resource ID, post-call
PC and request/resume updates. The pair must supply a hash-pinned
`resource_wait_source` capture and VM slot proving both boundaries. Only that
script pauses; other scripts, actors and audio continue. Unexpected or unused
waits fail. `--watch-locations LOCATIONS.json` adds read-only observations.

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
`--trace-startup` and bounded `--trace-random N` use a read-only debugger that
pauses execution; confirm timing against an ordinary replay afterward.

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
  local/native/replay local/cooked
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

```sh
python3 -m unittest discover -s tools/oracle -p 'test_*.py'
```

Source PCM agreement does not establish physical device latency or underrun
behavior. See [performance probes](../../docs/performance.md) for those checks.
