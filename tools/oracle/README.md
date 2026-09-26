# Dolphin oracle

Run from the repository root inside `nix develop`. Reusable inputs belong in
[cases](cases/); discs, savestates, captures and reports stay in ignored `local/`.
Capture output directories must be fresh.

Read the relevant original routines and recovered asset recipes before a fidelity
batch. Group related fixes, build once, then run independent cases with up to 20
Dolphin sessions. Each session needs its own profile/configuration directory,
virtual display, watcher/debugger socket, and capture/audio/log/report directories. Retain exit codes,
timings and failed comparisons. Reuse verified source captures when only native
code changes. Keep heavy builds and cooks separate from capture batches; use one
Cargo build job and at most 24 workers for non-Dolphin tooling. The 20-session
Dolphin cap is independent of emulator-internal thread counts. Run performance
benchmarks separately from concurrent captures.

**All validation is silent.** Dolphin capture enforces `Backend=No Audio Output`,
`Muted=True` and `DumpAudio=True`, without changing desktop settings. Native
recorders use no output device; interactive recording requires `--silent`.
Audio is still recorded and compared.

Battle checkpoints are inspected through the loaded REL and its BSS. The watcher
records battle clocks, RNG, actor/vital pointers and camera state; suspended field
banks are not interpreted as active models. `cases/battle-opening-source.json`
retains the historical opening-route input. It does not establish native battle
acceptance. See [current battle evidence](../../docs/battle-status.md).

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
`--trace-startup`, bounded `--trace-random N`, `--trace-distance N`,
`--trace-geometry N`, `--trace-damage N`, `--trace-melee N`,
`--trace-motion N`, `--trace-movement N`, `--trace-residents N`,
`--trace-casting N`, `--trace-effects N`, `--trace-reactions N`, `--trace-hurt N`,
`--trace-guard N`, `--trace-particles N`, `--trace-particle-uv N`, `--trace-voices N`, `--trace-weapon-flights N`, `--trace-stun-rolls N` and
`--trace-stun N` use a
read-only debugger that pauses execution; confirm timing against an ordinary
replay afterward. Distance tracing records original SDK vector-length inputs,
results and FPSCR, with 1–4096 calls from a recorded checkpoint. It changes no
game memory or instructions and cannot be combined with another trace mode.
Common particle tracing records complete input/output declarations, object ages and
RNG around each update, including initialization. Casting traces retain ordered
effect, sound and spell-release requests; effect traces retain post-modifier particle
inputs and their separate update/draw groups.
Voice tracing records relative/absolute actor requests, the common dispatcher,
logical priority/pending slots and ordered audio idle/stop/play calls. Supply
observed audio completion at the replay boundary; do not prescribe a guessed
voice duration. Before/after priority and pending state are assertions, not replay
inputs. Volume and pan are recorded separately from acceptance of the voice
arbitration state.
Weapon-flight tracing observes active equipped-weapon updates at `21F48`,
the catch-distance call at `2213C` and its flag update at `2215C`. It retains
the actual stack catch point before and after movement, source hit row, slot,
outbound counter, position, Euler angles, direction, speed and cooldowns.
The outbound branch skips initialization of this catch point; a return-only
assumption must not be inferred from partial C or a successful distant throw.
Adding `--trace-weapon-stack-writes` instead starts each observation immediately
before an already active owner's state callback. A Dolphin memory write
watchpoint attributes changes to the twelve catch-point bytes through the common
tail, then records that visit's flight update. It ends at the requested visit
bound or the first catch/retirement. Each visit admits at most256 write stops
and the capture at most8192 actor dispatches. It changes no game
memory; confirm the observed trajectory against the identical ordinary replay.
Melee tracing distinguishes `melee` hit-stream visits, `commands` visits,
`animation_start` admission bindings and later `animation_row` visits.
Animation observations retain the separate action, command, hit and animation
counters, row index, clip and model blend flag before and after binding. Their
source order matters: a later motion row can hold its animation counter after
commands and contacts already advanced on that visit.
Command rows retain the command clock and ordered calls to the sound and voice
request helpers, with RNG before/after. These establish requests, not audio playback.
Melee rows also record Lloyd's held body and carried-weapon matrices at contact
submission. Gameplay matrices and drawing matrices remain distinct; contact-only
bones can have no drawing object. These samples can validate ordinary bone and
weapon attachment composition. Hair/cloth dynamics require the secondary solver's
history, and sampled poses alone do not establish complete action timing.
Motion tracing distinguishes `motion` clock visits from `chains` solver visits;
nested solver calls appear in a motion row's `chain_visits`. Chain snapshots retain
positions, previous positions, targets, velocity, parameters, callback identity,
model rotation/scale, acceleration, wind, the floor flag and primary bone matrices
before/after the solver. A clock visit with
flag 4 set skips matrix composition and cannot establish solver behavior.
Particle UV tracing records only visits with a bound UV stream, from `403F4`'s
UV branch through its clock increment. It records the original keys, rectangles,
palette selectors, row/scroll state and owner's clock-hold flag. Matching this
component does not establish an observed particle's attachment or rendering.
Stun-roll tracing records the contact's chance, attacker bonus, target resistance,
EX query, unsigned random roll and resulting entry state. Zero chance still
consumes a roll. Failed rolls do not establish successful entry or the stunned
controller's later motion, particle attachment, sound and recovery behavior.
Stun-controller tracing observes `2E848` entry/return, retained particle writes,
body motion bindings, timer and recovery state, and original `9E38` sound calls.
Supplied head samples can validate attachment writes without establishing native
skeletal sampling; sound-call comparisons do not establish audio playback fidelity.
Geometry tracing requires a loaded battle checkpoint. It follows the original
REL's five hurt-shape branches, recording shape dimensions, scales, world points
and the overlap decision before hit rules run. The 1–4096 observations establish
only the shapes/branches actually encountered; they do not establish damage,
guard or full collision behavior. Damage tracing also requires a loaded battle
checkpoint. It records the resolver's actor/rule inputs, selected action row,
live attack-element overrides and the original element-selector return,
position/facing and incoming vector, computed amount, result flags, HP, shared RNG
state and target controller/guard fields before/after each call. Native comparisons must
identify which observed branches are implemented; a formula match alone does
not establish guard selection, reactions or the contact dispatcher.
Reaction tracing follows `63470` through its return to `3BDF8` and then through
the complete contact return. It records pending/live recoil, delay, hitstun,
actor state, HP and shared RNG at each boundary. Comparisons must distinguish
wrapper writes from controller transitions and later effect/audio RNG draws.
Hurt tracing records `2FB24` entry/return, including signed hitstun, local hit-stop,
pending impulse, movement, combos, controller activity and RNG. It also records
the explicit guard preference and auto-guard chance, distinguishing recovery's
random draw from contact resolution. Its profile/owner/root fields identify the
branches observed; ordinary hurt visits do not establish captured, Unison, EX or
knockdown behavior, model transitions or complete contact entry.
Guard tracing uses the same callback observations for `2F284`, with guard mode,
active state, body clip and next-action selection inputs. Classify ordinary
countdown/recovery separately from held inputs, counters and automatic follow-ups.
Contact tracing also retains the chosen recoil direction and model track bindings;
matching these does not establish full pose or contact-tail fidelity.
Melee tracing observes `2D564` entry/return: the current source row, hit-stream
clock/cursor, actor contact caches and newly submitted origins/descriptors. It
requires a loaded battle checkpoint. Comparing supplied sampled origins verifies
stream timing and submission, not the native pose sampler or full actor timing.
Motion tracing records controller lists before/after `8006D2E0`, including pending
cross-fade replacements, playback intervals, rates and completion flags. It
requires a loaded battle checkpoint and records 1–4096 model visits. This verifies
controller arithmetic; it does not capture bone matrices or establish full action
or drawing timing.
Movement tracing records `24314`, `244D0` and `24040` inputs/outputs, including
velocity, acceleration, origin snapshots, braking, profile flags and model root
state. It requires a loaded battle checkpoint. Arithmetic comparisons must retain
the observed scope; a helper call does not establish arena corrections, controller
transitions or root-motion routes absent from the capture.
Resident tracing reads both original slots around `3A978`, preserving roster order,
initialization/active phases, age, duration, retention and owner HP. Ordinary-slot
observations do not establish stored-scene or summon cleanup.
Loaded-battle snapshots and VI watches also retain the stored transition's owner,
countdown and slot flags, both stored resources/owners/completion callbacks, and
actor casting, body-clip and resident phase/age words. Decode the named word fields
using their original widths: the stored countdown is the upper signed halfword;
the lower halfword is a separate presentation value. These read-only observations
distinguish resource activation, resident initialization, active expiry and cleanup.
Casting tracing reads `39974`, `3898C` and `385A0` entry/return state: selected
actor/technique records, countdown, elapsed time, model replacement/playback, TP,
primary occupancy and RNG. It requires a loaded battle checkpoint. Compare each
observed branch with the native script using the original parameters; motion-clock
agreement does not establish curves, images, voices or all caster variants.
Effect tracing records `420C4` visits and their ordered `418B4` command dispatches,
including immediate construction and later object-group calls. It preserves the
source record, cursor, age, repeat-list counters, bank slot/root pointers and battle RNG
before/after each visit. Nested particle-allocation observations retain the original
352-byte recipe, allocated address and the copied record after command modifiers,
plus origin, heading and RNG. Allocation failure produces no particle record.
It requires a loaded battle checkpoint. Compare the prepared particle fields and
RNG separately from timeline dispatch; these snapshots precede initialization and
do not establish continuous particle motion, attachments, rendering or audio.
All trace modes retain the ordinary replay's
inputs and use isolated debugger sockets.

`resonance-oracle inventory-fixture --help` describes matching changes to copied
Dolphin states and native saves: items, formation, learned records, discoveries,
settings and history. `--learn-technique CHARACTER:TECHNIQUE` grants a technique;
`--disable-technique CHARACTER:TECHNIQUE` disables AI use of a learned technique
in both copies, using the ordinary saved menu setting. Characters are zero-based
and technique IDs come from the cooked catalogue. This supports controlled spell
observations without changing the battle code. The tool validates starting values
and records input/output hashes and requested changes.
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

Reaction traces also record the attacker at the damage-wrapper and contact return,
plus each nested `A9B4` TP recovery request. This distinguishes suppressed recovery
from an eligible request clamped by maximum TP. The observations remain read-only;
compare identical VI indices with an ordinary replay before accepting timing.

Effect-command observations include nested `9DC4` spatial sound requests with
sound index, priority and world-position bits. An empty request list establishes
only that the observed command emitted no sound; it does not establish sound
controller or playback acceptance.
