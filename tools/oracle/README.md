# Dolphin oracle

Run from the repository root inside `nix develop`. Keep reusable replay/probe
inputs in `cases/`; generated captures, state files and reports belong in ignored
`local/`. Each capture/recording output directory must be fresh.

## Capture and compare

```sh
cargo build --workspace
target/debug/resonance-oracle dtm tools/oracle/cases/title-navigation.json --output local/oracle/title.dtm
python3 tools/oracle/capture.py --disc /path/to/Disc1.rvz \
  --movie local/oracle/title.dtm --output local/oracle/title-reference \
  --frame 2700 --xvfb --watch-state
target/debug/resonance --silent --presentation-start 2365 --tick 964 \
  --replay tools/oracle/cases/native-navigation.json --capture local/native/title.png
target/debug/resonance-oracle compare local/oracle/title-reference/reference.png \
  local/native/title.png --output local/oracle/comparison \
  --tolerance 8 --max-changed-fraction 0.01
```

The sample counters must be registered to the same scene state before accepting
an image comparison. Dolphin dump frames, controller polls and native ticks are
different counters. `--presentation-start` restores an observed title-entry
counter for static probes; it is fixture data, not a live default.

The pinned `no-blur-v1` profile uses Dolphin 2606, GQSEAF, native resolution,
progressive 640×480 output, 4:3 aspect, Remove Blur Gecko patches and
`DisableCopyFilter`. `capture.py` launches regular Dolphin in an isolated user
directory; `dolphin-emu-nogui --movie` does not replay input. `--xvfb` isolates
Linux display output. `--watch-state` verifies both patched instructions.

**Capture never plays audio.** The launcher enforces `Backend=No Audio Output`,
`Muted=True` and `DumpAudio=True`. WAVs remain available under `user/Dump/Audio`;
`capture.json` records configuration, hashes and completion. Desktop Dolphin
settings and system volume are untouched. `--keep-frames` retains intermediate
PNGs; otherwise the requested image is preserved as `reference.png`.

[no-blur.json](cases/no-blur.json) retains 21 reusable image/region checks: reference
paths/hashes, native command arguments and optional crop rectangles. Build
`classroom_probe` with `cargo build -p resonance-presentation --example classroom_probe`
for its field cases. Run each distinct `native_args` command, then compare its
output with the named reference using the manifest's tolerances. Add
`--region X Y WIDTH HEIGHT` for cropped checks. The comparator writes a difference
image and metrics, exiting unsuccessfully when the gate fails.

Reference PNGs remain local fixtures; their hashes identify the intended inputs.
Colette probes register observer positions and animation samples for rendering
comparisons; they do not prove natural navigation or turn timing. Full replay and
consecutive-frame checks cover those separately. Refresh references when changing
the presentation profile, and investigate failures without loosening tolerances.

## Replay and state tools

`cases/title*.json`, `intro.json`, `new-game-entry.json` and
`classroom-dialogue.json` are Dolphin controller-poll fixtures. `native-*.json`
are player title inputs. The other small JSON files are field probes, movement
sequences, particle seeds or audio comparison inputs; they are not DTM files.

Use `--save-state` with `--xvfb` to retain a paused checkpoint and its DTM.
`--initial-state PATH` restores one with a matching state-based DTM. A saved state
may be later than the requested image; `checkpoint-reference.png` records the
last completed dump near the pause. Check update/draw phase for timing work.

```sh
python3 tools/oracle/state.py local/oracle/title-reference/user/StateSaves/GQSEAF.s01 \
  --output local/oracle/title-reference/state.json
```

`state.py` observes the pinned Dolphin layout without executing the game.
`--watch-state` captures frame-end memory observations; use `--watch-vis N`
instead of `--frame` for a timeline without PNG dumps. `--watch-actor ID` and
`--watch-volume-group ID` extend observations. `--trace-startup` is a bounded GDB
function-entry diagnostic: it pauses execution, so validate timing against an
ordinary replay. Both modes retain mandatory silent recording.

## Native recording and audio

```sh
# Live recording with permanently muted output.
target/debug/resonance --silent --record-playthrough local/native/playthrough
# Device-free complete supported story route.
cargo run -p resonance-presentation --example new_game_capture -- local/native/new-game
# Consecutive frames for walking and emote regressions.
cargo run -p resonance-presentation --example field_sequence -- tools/oracle/cases/classroom-walking.json local/native/walking
cargo run -p resonance-presentation --example field_sequence -- tools/oracle/cases/classroom-emotes.json local/native/emotes
# File-only title synthesis, including the first loop.
target/debug/resonance --silent --skip-intro --record-music local/native/title.wav --audio-frames 1280000
# Golden source-PCM windows: startup fade, overlapping cues and first loop.
cargo test -p resonance-presentation startup_fade_overlapping_cues_and_first_loop_match_dolphin -- --ignored
```

Field sequences write per-frame images and state. `renders_per_update` repeats
a simulation tick; `movement` holds directions at update indices and
`accept_updates` supplies button edges. With no `start_tick`, recording begins
at player control. The examples create no output device.

`resonance-oracle compare-audio` compares explicit WAV frame windows using
`--reference-start-frame`, `--actual-start-frame` and `--frames`; tolerance defaults
to zero. The three audio fixtures retain reference hashes, windows and cue events
used by the golden test. Keep source-PCM equivalence separate from device timing.

For long background-music comparisons, `music_compare.py --help` describes fixed
registration and drift diagnostics without gain correction or resampling.
`voice_content.py --help` compares cooked speech against recordings. Both read
files without playing them. Run the analysis unit tests with
`python3 -m unittest discover -s tools/oracle -p 'test_*.py'`.
See [performance probes](../../docs/performance.md) for movie/device stress tests.
