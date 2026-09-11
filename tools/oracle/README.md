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
`--keep-duplicate-frames` also dumps repeated XFB presentations for timing
investigations. Blank intervals can still have no framebuffer to dump; this
option does not make PNG indices equivalent to VI observations.

For routes crossing loading boundaries, use `--watch-vis N --video` to record
lossless RGB FFV1 with emulated presentation timestamps. Extract explicit VI
samples with `resonance-oracle video-frames VIDEO --output FRESH_DIR --first-vi 0
--vi 0 100 300`. Register the first video frame's VI explicitly. The Rust tool
uses FFmpeg, retains timestamps and hashes in `frames.json`, and rejects missing
presentations; it never substitutes a nearby frame. This avoids guessing PNG
indices when Dolphin skips repeated or blank presentations.

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

Skit playback has a paired case in [skit-playback-pair.json](cases/skit-playback-pair.json).
It checks every subtitle of the opening Iselia skit against timestamped Dolphin
video. The case records subtitle-relative alignment because prepared playback
skips the original media startup/drain waits. Both replays record audio to files
without opening an audible device.

The [Equip case](cases/equipment-pair.json) checks slot selection, armor removal,
stat previews, re-equipping and Optimal selection. Its checkpoint uses the
Dolphin state's party statistics, including randomized level gains and luck.

Run the local script/resource and classroom reward checks without rendering:

```sh
cargo test -p resonance-game --test classroom_script -- --ignored
cargo run -p resonance-game --example field_events -- 340 2000 local/classroom-events.json
```

The sweep takes `MAP STORY OUTPUT.json [COOKED_ROOT]`, recreates the field for
each actor/trigger and discovered choice path, and fails on missing native services,
resources or dialogue glyphs. Use classroom story 1000 before the party joins,
2000 afterwards, and 2500 on return. Reports cover script behavior; direct registry
invocation does not establish navigation or image/audio equivalence. Boundaries to
uncooked fields are reported as failures rather than counted as working events.

Capture an initialized field checkpoint without a window or audio device:

```sh
target/debug/resonance --silent --load local/saves/quicksaves/slot-classroom.json \
  --capture local/native/classroom-load.png
```

The field stays at its first controllable tick while rendering prepares. The
adjacent JSON records progression, pose and content identity. Quicksaves restart
the field; pair them with a Dolphin state at the same story checkpoint and
register animation/audio origins separately.

Replay keyboard controls from a save and record named frames plus audio without
a window or an output device:

```sh
cargo build -p resonance-presentation --example checkpoint_replay
target/debug/examples/checkpoint_replay SAVE REPLAY.json local/native/checkpoint-replay local/cooked
```

Use [school-grounds-keyboard.json](cases/school-grounds-keyboard.json) for the
input schema. Input changes are one-based game updates and hold their keys until
the next change; capture zero is the initialized field. Field preparation and
GPU readbacks consume no game or audio time. `recording.json` retains pose,
progress, play time, content identity and audio commands; `audio.wav` is the
ordinary mix rendered offline. Saves are still restricted to free movement,
although the replay may open menus or trigger events after loading.

Run both engines and their comparisons with one command:

```sh
cargo build -p resonance-oracle
target/debug/resonance-oracle pair tools/oracle/cases/school-grounds-pair.json \
  --disc /path/to/Disc1.rvz --output local/oracle/school-grounds-pair
```

The output must be fresh. The manifest pins the disc, save, Dolphin state and
consumed DTM prefix, input fixture and Dolphin build/profile. It also declares
the common field/story/player pose, input timing and named image/audio windows.
Each named frame declares `dolphin_vi` for its memory observation. Player
position/heading and camera position/target have separate 0.01-unit/degree gates.
Map and story must match at every frame; changed source storage pointers reject
the observation rather than compare stale session words.
Set `dolphin.video_first_vi` to opt into timestamped video in a paired case, and
omit the per-frame `dolphin` PNG index. The runner captures, indexes and extracts
the declared VIs before comparing them. Multiple recordings require an explicit
zero-based `dolphin.video_segment` and a documented first-VI registration for that
segment; the runner never guesses which file to use. All files and hashes remain
in the capture report. A negative first VI represents presentation queued before
the memory-observation origin; establish this once from observed transitions and
retain the same offset throughout the recording. For existing PNG cases, the
`dolphin` image index is separate: loading can advance VI observations
without producing another image. Register it from the source presentation
sequence and verify consecutive moving frames across each loading boundary.
Frames with `action_prompt: true`, `save_prompt: true` or `skit_prompt: true` also check visibility,
both opacities and the button's blink phase against observed game state. Skit
and generic action checks include the selected ID; image gates verify the text.
`ambient_animations` maps native actor IDs to observed controller names and
checks both sample and speed, using two cooked ticks per authored frame.
`party_state` checks formation, field leader, controlled member and the script's
leader lock. `statistics_state` checks the active party/Status display page.
The [statistics case](cases/statistics-pair.json) covers Start's alternate party
display, both Status pages, character changes and title selection.
`tech_state` checks Tech focus, character, row, scroll, displayed techniques and
target and preview counter, forget-dialog choices, controller settings and shortcut-editor availability,
plus every member's HP/TP, conditions, learned/AI flags, equipment, equipped EX
skills and both kinds of shortcut. It also checks the save-point context directly.
The [field-spell case](cases/tech-spells-pair.json) covers healing, revival,
target bounds and retention, rejected casts and automatic return when TP runs low.
The [assist case](cases/tech-assists-pair.json) checks both shortcuts, character
selection, assignment, removal and cancellation with the same four-member fixture.
Its controlled fixture uses `--formation 1 2 3 4 --learn-technique 3:121
--member-vitals 0:1:36:0 1:0:32:0x80000000`; vitals use
`CHARACTER:HP:TP:CONDITIONS` with zero-based characters. Starting values, maximums
and knockout consistency are checked before writing either copy. Learned grants
preserve matching AI flags. This does not simulate damage or natural learning.
The [party case](cases/tech-party-pair.json) extends that fixture to formation
`1 2 3 4 5`. It checks knockout rejection at entry, page buttons that include
knocked-out members, header navigation that skips them, and reserve-member access.
The [inline U. Attack case](cases/tech-inline-unison-pair.json) uses the same fixture
to check control cycling, the four shortcut slots, assignment/removal and nested
cancellation. Controller status is observed directly: only player one is connected.
The [long-list case](cases/tech-lists-pair.json) grants Lloyd techniques 2–12 and
Genis 67–80 in copies of `ex-skill-fixture-v1`, retaining story 2500 and matching
AI flags. It exercises both list layouts, partial pages, assigned-technique
selection, last-row bounds and confirmed/cancelled forgetting.
The [Tech transition case](cases/tech-transitions-pair.json) reuses that recording
for 239 opening, help-fade, paging and confirmation samples. Its older source
recording observes the effect clock but lacks effect RNG and Main transition words;
those two state gates are not claimed. Its Tech gate checks navigation and
transitions; full loadout/save-point observations come from the Equip/Tech case. Its source route
never closes Tech or starts row scrolling. The
[Equip/Tech transition case](cases/equipment-transitions-pair.json) adds 250
samples of both menus' entry, exit and reopening, forward scrolling, ignored
input during motion, equipment paging bounds and retained sort order. It grants
Lloyd's weapons with the Rust fixture tool; acquisition remains outside that
controlled fixture. Neither recording needs a loading-time adjustment. Equip
state gates compare page/help motion, selection and all equipment/inventory;
Tech gates also compare its independent banner and confirmation opacity.
The [EX](cases/ex-transitions-pair.json), [U. Attack](cases/unison-transitions-pair.json)
and [Cooking](cases/cooking-transitions-pair.json) transition cases reuse existing
silent recordings with complete ambient origins and observed input timing.
They retain the earlier settled samples and add consecutive transition images.
Their manifests list missing observations and uncovered interactions; older
recordings cannot assert every transition counter. Unison's description word
checks the retained outgoing technique, separately from the current selection.
The [group-spell case](cases/tech-groups-pair.json) extends the five-member fixture
with Raine's Nurse, Recover and Purify (`--learn-technique 3:99 3:101 3:102`) and
`--member-vitals 0:1:36:0x20 2:10:58:0x40 4:1:42:0`. It checks poison and deadly
poison portraits, single/group cures, group healing that includes the reserve and
skips knockout, rejected casts, reserve targeting and the retained preview cycle.
Consecutive captures straddle two preview changes. The Rust cooker extracts the
status icons into menu artwork version 12; recook older assets with `cook-menu`.
The [Personal case](cases/tech-personal-pair.json) grants a Faerie Ring and sets
Raine to 4 TP in copies of the group-spell fixture. It equips the ring and
Personal through the actual Equip/EX menus, then verifies ordinary discounted
costs, Personal's 1 TP override, healing, revival, group healing and rejected
casts. The source save-point flag and equipment/EX selections are observed, so
matching TP cannot conceal an incorrect context or loadout. Menu data version 18
stores the save-point override as a named skill rule; no executable bytes are
needed by the runtime.
The [condition case](cases/tech-conditions-pair.json) changes the group fixture's
vitals to `0:100:36:0x240 1:0:32:0x800003e0 2:100:58:0xa0 4:100:42:0x3e0`.
These artificial combinations check portrait priority across menu pages,
curse/paralysis animation and actual cure/rejection actions. Artwork version 13
adds Rust-decoded grayscale portraits for petrification. Its observations cover
settled pages. The [Status transition case](cases/status-transitions-pair.json)
adds page entrance, character cross-fades, title popup opening/closing and overlapping
animations, including ignored inputs during fades. `status_state` observes character,
previous portrait, title selection, display page and the three animation counters.
The [Status handoff case](cases/status-handoffs-pair.json) adds both Main-menu
transitions, party wraparound and both Rename handoffs. `main_menu_state` checks
the fade, selected character and party viewport. Status returns its inspected
character to Main; Main retains its viewport until that return.
The [Rename Gem case](cases/rename-pair.json) covers editing through Status,
deletion, defaults, restore, cancellation and commit. The
[Items entry case](cases/rename-items-pair.json) covers the target panel, editor
handoff and return to the list. Both use `inventory-fixture --grant-item 499`;
`rename_state` checks saved names, the reusable gem and the active editor's state.
The native confirmation starts twelve updates before the observed first Status
pose, allowing Main's departure slide. The handoff case omits the observed disc
waits and registers their UI ticks separately with `presentation_advances`;
runtime does not reproduce loading waits. Cases omit VIs without recorded video
presentations. Field return remains a separate case.
`inventory_state` checks Items category, row, scroll, focus, single/group targets,
preview counter, popup opacity/closing state, description cross-fade and the
presentation clock, plus all item counts and all nine members' HP/TP, conditions
and equipment. The [Items handoff case](cases/items-handoffs-pair.json) and
[reopening case](cases/items-reopening-pair.json) cover Main/Items and
Items/Collector's Book in both directions, including the transparent handoff
poses, description crossfades, scrolling and retained selection. These source
handoffs have no loading stalls; native update 64 is source VI zero throughout.
`collection_state` checks category, row, viewport, scroll pose, page fade,
description state and UI clock. Its scroll adapter accounts for the source
counter advancing after drawing. The [tab-input case](cases/collection-tabs-pair.json)
checks ignored shoulder buttons on tabs, directional wraparound, list resets,
reopening and combined shoulder/directional input. The
[Items scroll case](cases/items-scroll-pair.json) checks row motion in both
directions, ignored input during motion, full-page jumps and list bounds. Focused
arrow regions accompany whole-frame checks. The
[category case](cases/items-categories-pair.json) checks row/viewport resets,
empty lists, ignored shoulder input on tabs and returning to Main.
`inventory_state` compares the scroll counter after the source drawing step,
alongside the other inventory checks. The
[Rune Bottle case](cases/items-rune-pair.json) covers mixed categories, cancelling
the picker, acknowledging results with either button, and bottle exhaustion.
The [full-stack case](cases/items-rune-full-pair.json) checks rejected conversions
and reopening from recent items; the [empty-picker case](cases/items-rune-empty-pair.json)
checks automatic closure after the last eligible item and rejection from both
ordinary and recent items. State gates check deferred consumption, notices and
the retained return cursor as well as every inventory count. The
[target case](cases/items-targets-pair.json) grants one Panacea Bottle to copies
of the condition fixture. It exercises the five-member picker, reserve navigation,
healing, invalid targets, item exhaustion, revival and a reserve's status cure.
The [group-item case](cases/items-groups-pair.json) grants an Energy Tablet and
Spirit Bottle, with Colette knocked out at zero HP/TP. It covers all five preview
boundaries, ignored navigation, opening/closing fades, cancellation and exhaustion.
It also checks the unusual case where only a knocked-out member lacks TP: the
Spirit Bottle is consumed although that member receives no recovery.
The [eligibility case](cases/items-groups-eligibility-pair.json) starts with Lloyd
knocked out and everyone else healthy. It checks automatic field-leader replacement,
rejected group recovery before revival, successful entry afterwards, and full-party
HP/TP rejection without consumption. These are controlled menu fixtures, not tests
of combat or natural damage. Their later native inputs use source poll/2 + 1, the
observed input-processing VI; capture updates and source VIs remain equal.
Menu data version 19 contains a structured group-use prompt and item attention
rules parsed by Rust. The [Items equipment case](cases/items-equipment-pair.json)
grants four weapons, two armors and three accessories to the five-member fixture.
It checks equipped/better/worse/equal/unavailable portrait markers across their
animation phases, stat previews, missing list cells, actual swaps, both accessory
slots, reserve equipping and closing into an empty category. Artwork version 14
adds the comparison markers and equipped glyph to the existing atlas. `cook-menu` also refreshes
item permissions and dependent field hashes. The four Zelos-only exceptions are
parsed from checked instructions; the current image case covers the first five
characters, with a local gameplay regression for Kratos/Zelos permissions.
Existing checkpoint identity is retained for this exact permission correction;
other data or script changes remain subject to the normal compatibility check.
Run `cook-menu` on older cooks. Other item families and later-story restrictions
still need separate coverage.
`collection_state` checks the Collector's Book category, row, scroll and focus.
The [collection case](cases/collection-pair.json) includes all eight categories
and an empty one; the [full catalogue case](cases/collection-full-pair.json)
covers sorting, page jumps, last-page bounds and completion totals. The
[populated figurine collection](cases/collection-figurines-pair.json) checks that
the Figurine Book moves into the usable key-item group once a figurine is owned;
the full catalogue's empty collection keeps it in alphabetical order. The
[empty book case](cases/figurine-empty-pair.json) checks the rejected opening and
unchanged inventory.
`world_map_state` checks the world, focused list, selected location/shop/item,
viewport and scroll motion, page/popup fades, description state and UI clock.
Scroll counters account for the source drawing step. The
[map handoff case](cases/world-map-handoffs-pair.json) checks nested panel fades,
description changes, item scrolling, paging and reopening. The
[location case](cases/world-map-locations-pair.json) adds location scrolling and
page bounds, then Tethe'alla's opening, multiple shops and return to Items.
Both use native update = source VI + 64 and matched copies with both maps,
all listed locations and visited shops. The [map case](cases/world-map-pair.json) covers both maps,
an empty location list and an unvisited shop; the
[full directory case](cases/world-map-full-pair.json) covers location and stock
paging, multiple shops and nested cancellation. Both register the presentation
clock from the source checkpoint to compare the crosshair pulses.
`manual_state` checks Training Manual chapter/topic focus, reading paragraph,
page fade/closing state and the presentation clock against the source UI clock.
The [handoff case](cases/manual-handoffs-pair.json) covers both slide directions,
reopening and ignored transition input, with native update = source VI + 64.
The [manual case](cases/manual-pair.json) covers navigation, the
[full text case](cases/manual-all-pair.json) visits every paragraph, and the
[filtered case](cases/manual-filtered-pair.json) checks partly learned chapters.
Create their controlled checkpoint copies with `inventory-fixture --grant-item 73
--learn-manual-topic ...`; accepted flags come from the cooked manual's topics.
`monster_state` and `figurine_state` check page/model fades, list scrolling,
preview pose, animation and the UI clock. The [Monster List handoffs](cases/monster-handoffs-pair.json)
and [Figurine Book handoffs](cases/figurine-handoffs-pair.json) cover intermediate
opening/closing poses, returning to Items and reopening. The figurine route also
checks retained models through selection changes. Both use the source's existing
fixture copies, register field origins once and omit observed disc waits from
the native timeline, recording those waits as UI-clock advances in the replay.
Model opacity and animation advance naturally without preview sample overrides.
Asynchronous model waits advance the effect clock too (`effect_advances`);
blocking menu-entry/exit waits only advance the UI clock. The source routines
and captured counters distinguish the two.
The [figurine case](cases/figurine-pair.json) covers
navigation; [variants](cases/figurine-variants-pair.json) covers shared textures,
costumes and Katz. Create their fixture with `inventory-fixture --grant-item 72
--collect-figurine ...` using the IDs recorded in each case's provenance.
`ex_state` checks gem/skill navigation, inventory consumption, every character's
equipped gems, learned compounds and recent highlights, the active compound list,
the selected character's stats, and the UI clock.
The [EX Skills case](cases/ex-skills-pair.json) covers insertion and skill
selection; [variants](cases/ex-skills-variants-pair.json) cover replacement,
cancellation, Max gems, duplicates, HP/TP bonuses and character switching.
The [compound case](cases/ex-compounds-pair.json) checks previews, requirement
highlighting, bounds, unequipping and reopening. Its fixture uses
`--learn-ex-compound CHARACTER:RECIPE` and `--new-ex-compound CHARACTER:RECIPE`
with zero-based character and cooked recipe IDs; these grant knowledge in test
copies and do not simulate battle discovery.
`unison_state` compares the learned list, selection, paging, presentation clock
and all nine characters' saved technique shortcuts. The
[U. Attack case](cases/unison-pair.json) covers assignment, removal and party
navigation; [variants](cases/unison-variants-pair.json) add a twelve-technique
list, page bounds, retained scroll and duplicate shortcuts.
The [four-member case](cases/unison-four-party-pair.json) checks Raine's grey
healing techniques alongside Photon, assignment/removal and the fourth player's
navigation bounds. Its controlled fixture uses `--formation 1 2 3 4` and
`--learn-technique 3:113`; recruitment and battle execution are outside this case.
The [missing-ingredients case](cases/cooking-missing-pair.json) declines the
default No discard choice, explicitly discards three pieces of bread, and checks
cooking rejection from both header selections. Inventory changes commit when the
discard notice is acknowledged. `inventory_state` includes both discard choices
and the held notice; `cooking_state` checks recipe knowledge, selected recipe/chef,
fullness, inventory, HP/TP, conditions and all nine characters' cooking training.
The existing [meal](cases/cooking-pair.json) and
[browsing](cases/cooking-browse-pair.json) cases also enable these cooking checks.
The [injured-party case](cases/cooking-injured-pair.json) checks positive recovery
and independence from field effects. `gameplay_random_state` compares the gameplay
MT19937 cursor from an independently verified `ambient_origin.gameplay_random`;
the field LCG remains covered by `effect_state`.
`inventory-fixture --learn-recipe ID` grants cooked recipe knowledge in matched
checkpoint copies. The [Fruit Cocktail](cases/cooking-fruit-pair.json) and
[Cream Stew](cases/cooking-stew-pair.json) cases record their ingredient and vitals
changes explicitly; they test cooking, not story acquisition.
Cream Stew also checks poison puffs before opening the menu and after curing the
party. `poison_particles` names observed source slots; the comparison checks count,
age, position, size and rise speed. `ambient_origin.poison_puffs` registers the
source-observed initial particles once, including an explicitly empty origin.
The native runtime keeps the drawn puff pose; the replay adapter converts it to
Dolphin's post-draw position and age for memory comparison. The
[ordinary-poison case](cases/cooking-mild-poison-pair.json) checks smaller puffs
before cooking cures the party. The
[paralysis case](cases/field-paralysis-pair.json) checks both sides of successive
32-update pose changes, with tight image regions and `paralysis_state` atlas-frame
checks. Cream Stew also verifies that this phase freezes while the menu is open.
Run `resonance-import cook-effects` for field-effects version 3 before this case.

`inventory-fixture --learn-technique CHARACTER:TECHNIQUE` grants knowledge in
matched copies after checking the original table order against cooked IDs.
For later menu unlocks, `--story` changes the live Dolphin state and records a
`story_origin` for the native replay. Add its `from`/`to` values and the update
at which the main menu opens to that replay. Native field preparation uses the
original saved story; the change is applied only with the menu open. The runner
verifies both origins and subsequent progress. These fixtures do not verify
later-story field initialization, skit conditions or acquisition events.
The runner checks those before comparing every named frame at 640×480 with
tolerance 8 and at most 1% changed pixels; regions add gates. It retains a report,
both recordings, memory observations and differences, and exits unsuccessfully
on a failed gate. No image registration is inferred or audio silently resampled.

[Customize panels](cases/customize-panels-pair.json) exercises color channels,
volume levels, stereo, controller remapping and screen position. Its optional
`customize_state` gate compares the active panel, cursor and every draft
preference against read-only source memory, including the settings sent to the
field audio adapter. All 20 draft checks and 20/22 images pass. `audio_state` checks
committed mixer preferences on the initial and returned main-menu frames. Nonzero screen-position images remain
diagnostics: native scanout applies the requested offset, while the source frame
dump precedes the original VI-origin adjustment. Both raw images and their failing
gates are retained; no fitted translation is applied. The separate
[windows case](cases/customize-windows-pair.json) now passes all 25 images and state
gates, including return to the field.

The [Customize transition case](cases/customize-transitions-pair.json) adds 215
comparisons of the System handoff, page slides, color carousel, scrolling,
retained text preview and panel changes. Its two screen-offset diagnostics stay
in the original case. The [window transition case](cases/customize-window-transitions-pair.json)
adds 220 comparisons, including window changes, closing and field return.
Both reuse existing muted recordings. Customize state checks include page phase,
post-draw scroll/carousel counters and retained text reveal/wait counters;
System transitions use images because these recordings lack popup observations.

[Dialogue appearance](cases/customize-dialogue.json) checks all three frame styles
and six backgrounds on Genis's fully revealed Mithos line. The complete panel,
speaker tab and continue marker use a fixed region and the usual tolerance-8/1%
gate. Source recordings change only saved appearance in checkpoint copies; the
source memory observation confirms each setting at VI 150. Run from the repo:

```sh
cargo build --offline -p resonance-presentation --example dialogue_capture
cargo build --offline -p resonance-oracle
target/debug/resonance-oracle dialogue tools/oracle/cases/customize-dialogue.json --output local/dialogue-check
```

The command rerenders every variant through the real scripts, verifies applied
preferences, checks pinned source images and fails if any comparison fails.
It opens no audio device. Cases can name multiple regions, each of which must
pass, and select either a held dialogue line or the New Game setup prompt.

[Choice appearance](cases/customize-choices.json) checks the three saved window
styles with separate question, choice-panel and cursor gates. Build the
`setup_capture` example and run the same command with this case. Its retained
Dolphin sequence registers memory observations against the buffered image dump;
the selected frames have no cursor trail. This checks cursor artwork, draw order,
highlight and position. Attached pointers, opening transitions and cursor trails
across input history still need separate acceptance cases.

[classroom-return-pair.json](cases/classroom-return-pair.json) exercises timestamped
video across a school visit and return outside. It is still a diagnostic: the
same-VI registration exposes the deliberately skipped black loading interval
and notification clock differences. Those failures remain visible in its report.

The school-grounds replay waits 1397 native updates before its registered initial
frame so the timed skit notification appears and the shared 128-update spark
rotation/64-update button cycle aligns. This is fixture timing; loading a quicksave
needs no such wait.
Its optional `ambient_origin` registers existing scenery and memory-circle loops
once at the first capture, using samples observed in the Dolphin checkpoint;
the school fixture also records the fifteen source-observed spark births through
the ordinary high-level billboard recipe.
It cannot seek again during the replay. `npc-wandering-pair.json` additionally
registers actor positions, headings, ambient decisions and clip samples once.
The runner checks each against the source savestate; the native adapter checks
the actor's authored movement settings and cooked clip. Player position cannot
change. These transient origins are only for oracle replay, never save data.
`ambient_actors` checks positions, headings, idle/walk states, decision timers and
floor probes. The wandering case passes all 44 images and state gates, including
the camera anchor. It also registers existing `flutters` and `background_waits`;
waits must already be at the observed script instruction and control gate.
These registrations never seek the VM or become ordinary save data.
`effect_tick` registers the running effect clock separately from paused field
animation and UI time. With spark births, `random_state` can also restore the
source seed after ordinary checkpoint initialization; the runner verifies both
against the savestate. `effect_state` checks the clock and RNG at named frames.
The party fixture needs only 64 warm-up updates. All 11 images and state gates
pass, including the returned Genis field, its scenery, effect clock and RNG.
It uses the same source-observed actor and leaf origins as the wandering case.
The [reserve-party case](cases/party-reserves-pair.json) covers eight members,
cross-page swaps, cancellation and returning to the field with a reserve leader.
The [reserve Strategy case](cases/strategy-reserves-pair.json) checks current and
preset edits, page bounds and consecutive row-scroll poses. `strategy_state`
compares navigation, description fades, all nine current instruction triples and
all three presets. It also checks page/preset/rename opacity, rename text and
cursor position, and the UI clock. The
[Strategy transition case](cases/strategy-transitions-pair.json) covers every
entry/exit pose, preset slides and rename fades. Preset slides block input;
rename accepts input while fading. It verifies that Main and Strategy retain
independent party selections, including Main's reserve viewport, and covers
back-to-back cancellation while rename and preset fades overlap.
The [Zelos case](cases/strategy-zelos-pair.json) adds the
alternate roster, formation lanes and rapid changes during help-text fades.
Paging uses the source C-stick vertically, mapped to native previous/next-page.
The [later Synopsis case](cases/synopsis-later-pair.json) covers saved dates and
levels, progress variants, both maps, long-text paging and navigation during
scrolling/fades. `synopsis_state` checks navigation and all recorded history;
it enables the optional `--watch-synopsis` observations for that recording.
When an omitted source wait still advances UI time, `presentation_advances`
registers those extra UI ticks at a native update. It leaves gameplay, scripts
and random streams alone. The Synopsis case checks both clocks independently;
this preserves cursor trails without adding the original computation delay.
`eyes` registers existing blink phases, checked against the sequence and eye
channel timers in the source state. It cannot enable or change an eye mode.
`eye-blink-pair.json` checks three actors through two cycles using `eye_actors`.
All 51 full images and 153 eye-state checks pass. Eye state is transient and
never added to player saves.
Subsequent samples and rates are checked against Dolphin at every named frame.
The Save prompt's eight region gates cover both button images and the departure
fade; ten skit regions check its title and both button frames. These pass, but
the complete case still fails its active-circle crop gate; music needs an origin
and acceptance window. The final full-frame, departed circle,
sign and sign-shadow checks pass. Walking/camera checks
cover the wall-slide and follow-camera regressions.
Local checkpoints are not distributed. To recreate them, finish Frank's
conversation and the memory-circle tutorial on both sides, save with free field
control, and retain Dolphin's state/DTM using the tools below. Observe the source
pose and input count, record equivalent native state, and update the manifest's
paths/hashes and explicit registration. Do not add transient VM, animation or
audio snapshots to the player's save format to align a comparison fixture.

For menu pages awarded later in the story, `inventory-fixture` prepares copies of
both checkpoints with matching inventory or map-history changes:

```sh
target/debug/resonance-oracle inventory-fixture source.s01 native.json \
  --grant-item 70 --output local/oracle/collectors-book
```

It validates the starting inventories, discoveries and recent-item lists, grants
one of each requested item, and records original/output hashes in `fixture.json`.
`--item-count ID:COUNT` sets a specific quantity up to the cooked stack limit;
zero removes the held item while preserving its discovery and acquisition history.
`--discover-all` catalogues all classified items without adding them to inventory.
`--formation` sets matching party order while retaining the field leader.
`--view-skit` marks named skits as already viewed in both copies after checking
their initial history agrees. It leaves notification settings unchanged; record
this fixture change explicitly and do not count it as skit-playback evidence.
`--record-synopsis ID:VALUE:LEVEL:UNIX_SECONDS` changes a scenario record after
verifying its original value and metadata. For example, `4:2:26:1700000000`
selects Iselia Forest's second text variant with its recorded level and date.
Live story progress stays unchanged; these history fixtures verify menu behavior.
`--visit-location` and `--visit-shop` accept lists of cooked IDs to exercise map
directories. Travel history is checked too; older native fixtures acquire the
source-observed history, recorded in the report. The Rust tool uses the development
shell's LZ4 library to update the copied inventory and requested knowledge records.
`--encounter-monster` records encounters; `--catalogue-monster` also reveals scans,
drops, steals and locations. `--monster-variant ID:VARIANT` sets the highest known
repeat battle, validating it against `cook-monsters` output. These changes update
both fixtures and preserve the other monster's packed variant value.
Original files and game code remain untouched. These controlled menu fixtures are explicitly separate
from story-progression evidence; keep that distinction in the paired manifest.
Monster cases also compare the preview animation, yaw and distance. A replay's
`preview_origins` registers a source-observed idle sample once per selected model
to account for the omitted disc wait; later samples advance normally. The replay
rejects repeated registration for the same selection and samples outside the clip.

Read the relevant game routines and recovered asset recipes before starting a
fidelity batch. Group related fixes, then validate them together. During native
iterations, `pair --reference local/previous-pair/dolphin` reuses that completed
source capture. Input DTM, disc, savestate, emulator/profile and observed-memory
hashes must match. Cached PNGs are reused only when their video, VI registration
and individual hashes match; additional frames are extracted from the same video.
The new report records reference provenance. Re-record Dolphin when source inputs,
state, profile or required observations change; native code edits alone do not
require another source recording. Required Equip/Tech, Main and effect observations
are checked before native rendering. To change comparison gates without replaying
either engine, also pass `--native-reference local/previous-pair`. Its saved
case must pin the same native save and replay. The report records the reused
recording hash and original renderer hash; the latter remains explicitly unknown
for older incomplete reports that did not retain it. This option compares existing
output and does not validate subsequent runtime edits.

The Figurine handoff case marks `disc_loading_overlay` only where observed source
state confirms a visible DVD transfer notice. Its text and transfer bar are
excluded from acceptance; native loading text reflects actual asset preparation
without emulating DVD throughput. The report names the excluded framebuffer
rectangles and reason, and retains the unmodified full-image comparison and
difference alongside `without-disc-loading/`. All state gates and the 8/1% image
tolerance remain in force; the changed fraction uses only compared pixels.

[iselia-boundaries-pair.json](cases/iselia-boundaries-pair.json) starts after
returning from the village, at school-grounds position `[1791, -622, 0]`, heading
185 and story 2500. It walks through the village boundary and back, checking both
arrival poses. Its manifest documents the different loading durations and the
registered walking frames. All seven map/story/player/camera checks, both arrival
images and three consecutive village walking images pass. Its initial scenery
and skit origins are registered in the replay; all seven images and skit visibility
checks pass. Field changes dismiss the title while preserving its refresh clock.
Music now continues across both boundaries; the native recording matches an
uninterrupted music render sample for sample. A fixed fifteen-second comparison
against Dolphin has no lag drift, but raw PCM differences remain. The paired
case still needs a music origin and acceptance window.
Recreate its local fixtures by making that return on both sides and recording
the free-control checkpoint, pose, consumed input count and fixture hashes.

[classroom-door-pair.json](cases/classroom-door-pair.json) checks the school
entrance slope, door approach, turn, opening and fade. Its checkpoint starts at
`[1016.4123, 1356.6626, 11.681712]`, heading 185, story 2500, camera distance 1890.
Only scenery origins are registered; player poses evolve through ordinary input.
All 80 whole-frame/tight-region images, 30 state checks and 90 scenery samples
pass after correcting sheath-bone lookup and the hinge's first contact pose.

[save-confirm-pair.json](cases/save-confirm-pair.json),
[overwrite-confirm-pair.json](cases/overwrite-confirm-pair.json) and
[load-confirm-pair.json](cases/load-confirm-pair.json) each register a settled
confirmation once and compare 25 consecutive frames, covering a full cursor cycle.
All 75 state and 150 whole-frame/popup checks pass at tolerance 8 and a 1% outlier
limit. State checks include bank, slot, mode and Yes/No; images cover text, frame,
dimming, highlight and cursor trail. The latter two source states start with a
populated slot; [save-load-keyboard.json](cases/save-load-keyboard.json) recreates
that slot natively through normal input from the free-control checkpoint. Their
manifests document the different navigation prefixes and recorded play times/stats.
The `save-popup-pair.json`, `overwrite-popup-pair.json` and `load-popup-pair.json`
cases extend this to opening and closing frames; Load also exercises an empty
slot's error feedback. `slot_popup: true` compares both logical confirmation
state and the displayed message/opacity, including retained content during fade
out. The source message comes from its observed menu history. No transient menu
or cursor state is added to the checkpoint.
All 258 images and 152 state checks pass across these three transition cases.
The populated-menu source state produces an empty video header and two ticks of
queued presentation; its manifests select segment 1 and register first VI -2,
verified at both opening and dismissal. Save uses segment 0 and first VI 0.

[iselia-exploration-keyboard.json](cases/iselia-exploration-keyboard.json) uses the
same native checkpoint for the longer `332 -> 330 -> 332 -> 340 -> 332` route,
including doorway confirmation. Run it with `checkpoint_replay` to exercise
ordinary keyboard input and retain each arrival; it does not invoke triggers
directly. Its native completion is functional coverage, not image/audio acceptance.

For 100 live save/load cycles, including actor preparation and resource guards:

```sh
cargo build -p resonance-presentation --example quicksave_probe
target/debug/examples/quicksave_probe local/saves/quicksaves/slot-classroom.json \
  local/native/quicksave-probe
```

This device-free probe writes timings to its output directory and exercises an
isolated slot. It checks changed player state is restored and rejected saves are
not queued. Timings exclude display scanout and separate capture work from the
background file write.

[quicksave-slope-keyboard.json](cases/quicksave-slope-keyboard.json) saves during
the second walking update on the school entrance ramp, then loads and captures
the first restored frame. Start from `local/milestone-3/classroom-return-quicksave.json`
with the normal `checkpoint_replay` command. Its captured F5 save is pinned as
`local/milestone-3/slope-quicksave.json` by this device-free cold/warm regression:

```sh
cargo test -p resonance-presentation moving_slope_checkpoint_survives_cold_and_warm_loads -- --ignored
```

Setup must preserve the saved pose even when grounding would adjust it on the
next update. The regression checks progress and play time as well as location.

To exercise memory-circle save, field-menu load and loading in a new process:

```sh
cargo build -p resonance-presentation --example menu_probe --example title_load_probe
target/debug/examples/menu_probe CHECKPOINT local/native/menu-probe
target/debug/examples/title_load_probe local/native/menu-probe/slots local/native/title-load-probe
```

Use a controllable school-grounds checkpoint. Both probes run without a window
or audio device, use an isolated save directory, and retain captures and result
JSON. Title loading uses keyboard input and checks cancel/reopen before loading
the previous process's save. These functional checks require separate registered
Dolphin image/audio comparisons.

For the continuous New Game route, including Frank, both field revisits, the
memory-circle tutorial, menu Save and System Load:

```sh
cargo build -p resonance-presentation --example new_game_capture --example title_load_probe
target/debug/examples/new_game_capture local/native/iselia-save local/cooked exploration \
  tools/oracle/cases/iselia-new-game-save.json
target/debug/examples/title_load_probe local/native/iselia-save/slots local/native/iselia-reload
```

The first recorder continues the same application and mixer after the classroom;
it does not load an intermediate checkpoint. The replay asserts map/story/control
at named captures and checks the actual saved file against the live checkpoint.
The second process loads that menu save, then walks from the school grounds into
the village and verifies preserved party/progress. Both use isolated files and
no audio device. Recordings establish route and persistence behavior; the paired
oracle cases remain responsible for visual and audio acceptance.

Starting from a post-Frank school-grounds save, append `--route` to also exercise
332 -> 330 -> 332 -> 340 -> 332 through registered triggers. It records transition
times and verifies returns reuse prepared assets without additional reads. This
checks the live loader; walking, visual comparisons and dialogue timing still
need their paired cases.

`field_sequence` also accepts an optional raw `checkpoint` in its JSON input.
It initializes that field before applying explicit pose/animation registration
and relative updates. These registrations belong to the comparison case, not
to the player's quicksave format.

`cases/title*.json`, `intro.json`, `new-game-entry.json` and
`classroom-dialogue.json` are Dolphin controller-poll fixtures. `native-*.json`
are player title inputs. The other small JSON files are field probes, movement
sequences, particle seeds or audio comparison inputs; they are not DTM files.

Use `--save-state` with `--xvfb` to retain a paused checkpoint and its DTM.
`--initial-state PATH` restores one with a matching state-based DTM. A saved state
may be later than the requested image; `checkpoint-reference.png` records the
last completed dump near the pause. Check update/draw phase for timing work.

Author a short input case relative to a checkpoint with `resonance-oracle dtm CASE
--prefix STATE.dtm --start-poll N --output REPLAY.dtm`. Read `N` from the state
inspector's `movie.input_count`. The generator preserves the consumed recording
prefix and RTC, replaces future inputs with the relative case, and marks the
result as a state-based replay. Pass that replay and `STATE` to the capture tool.

Poll inputs may specify `stick` and `c_stick` as byte pairs, neutral at `[128,128]`.
Cooking pages with C-stick down/up (`[128,0]` / `[128,255]`); its keyboard replay
uses E/Q. Shoulder-button clicks do not page that menu.

```sh
python3 tools/oracle/state.py local/oracle/title-reference/user/StateSaves/GQSEAF.s01 \
  --output local/oracle/title-reference/state.json
```

`state.py` observes the pinned Dolphin layout without executing the game.
It includes field model-ID/address mappings and trigger shapes/metadata for
checking cooked bindings against the running game.
`--watch-state` captures frame-end memory observations; use `--watch-vis N`
instead of `--frame` for a timeline without PNG dumps. `--watch-actor ID`,
`--watch-particle SLOT` and `--watch-volume-group ID` extend observations.
Actor observations follow the live animation controller through clip changes,
including its sample, speed and blend progress.
Particle slots are discovered from the initial state and retain their recipe
address so reuse can be detected. `--trace-startup` is a bounded GDB
function-entry diagnostic: it pauses execution, so validate timing against an
ordinary replay. Both modes retain mandatory silent recording.
`--trace-random N` records up to 4096 RNG callers, seeds and clock values through
the same read-only debugger. Use it to identify a divergence, then check its
observations against a replay with debugging disabled.

## Native recording and audio

`cargo run -p resonance-game --example presentation_trace -- 0 --load CHECKPOINT
--trigger KEY --follow` runs field services without graphics or an audio device.
The optional trigger invokes a confirmed event directly for service diagnostics;
it does not test walking or collision. `--output PATH` exports a raw lightweight
checkpoint when control returns. Unsupported services stop the trace explicitly.
This diagnostic accepts raw checkpoints or save envelopes; use the player for
content-identity validation and visual/audio comparisons.

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
# Isolated Back/Error programs against independent Dolphin recordings.
cargo test -p resonance-presentation program_cues_match_dolphin_and_respect_live_group_volume -- --ignored
```

Field sequences write per-frame images and state. `renders_per_update` repeats
a simulation tick; `movement` holds directions at update indices and
`accept_updates` supplies button edges. With no `start_tick`, recording begins
at player control. The examples create no output device.

`resonance-oracle compare-audio` compares explicit WAV frame windows using
`--reference-start-frame`, `--actual-start-frame` and `--frames`; tolerance defaults
to zero. The three audio fixtures retain reference hashes, windows and cue events
used by the golden test. Keep source-PCM equivalence separate from device timing.

[menu-program-audio.json](cases/menu-program-audio.json) pins Back/Error recordings,
matched baselines, full cue windows and source-state replay inputs. Both pass with
at most one PCM unit of error per sample, including their decay and silent tails.
Original Customize sets music to zero through controller input, leaving effects
at 127. The baseline uses the same state and navigation with the target cue omitted;
subtract it with `--reference-baseline` to remove the original mixer's residual DC.
No centering, gain fitting or resampling is applied. The live cue test also verifies
category mute/resume. Two additional Back windows verify SE=64 before and after
commit: the draft cue still uses saved SE=127, and the commit cue uses SE=64.
All four windows pass within one PCM unit per sample. This acceptance covers
isolated cues; outdoor music and the shared field mix have separate comparisons.

To recreate a recording, extract its `dolphin.replay` into an input file, generate
a DTM with the pinned state companion and `start_poll`, then use `capture.py`
with that state and `capture_frame`. For its baseline, remove the input polls
listed in `baseline_omit_polls` and repeat from the same state. All fixture paths
are relative to the repository root; captures remain muted and local.

For long background-music comparisons, `music_compare.py --help` describes fixed
registration and drift diagnostics without gain correction or resampling. Its
acceptance result uses per-channel mean-removed waveform metrics because the
Dolphin DSP dump has a measurable channel DC component; raw PCM errors remain
in the report. The current outdoor recording passes the fixed thresholds of
0.99 minimum one-second correlation, 15 dB minimum mean-removed signal-to-error,
and 0.15 dB maximum level error.
[Customize BGM](cases/customize-music-audio.json) pins a six-second preview at level
64 and its native/source PCM offsets. Use `--actual-start-frame` for a registered
position; it excludes an alignment search. The command retains diagnostics and
exits unsuccessfully if the fixed waveform/level gates fail. A full-volume
negative control confirms that a matching melody cannot hide the wrong gain.
[Customize mono](cases/customize-mono-audio.json) reuses the stereo fixture's
PCM offset after committing mono. It passes all six waveform/level windows. The
previous final-channel average fails at up to 1.25 dB; the corrected renderer
centers voices first. `record_field_music` accepts an optional `stereo`/`mono`
argument after volume and fade ticks and always records without a device.
[Customize voice](cases/customize-voice-audio.json) compares level 64 with full
volume over a complete spoken line, subtracting a zero-volume Dolphin baseline.
Ten fixed half-second windows match attenuation within 0.000485 dB; the previous
linear gain fails by more than 7 dB. This checks the volume curve, not decoder
bit equivalence. Run `cargo test --offline -p resonance-presentation
saved_dialogue_volume_matches_dolphin_attenuation -- --ignored --nocapture`.
`record_field_voice ASSETS OUTPUT VOICE FRAMES VOLUME` records the actual field
mixer without a device. `inventory-fixture --voice-volume 0..127` changes only
the saved voice setting in matched checkpoint copies, with hashes in its report.
`voice_content.py --help` compares cooked speech against recordings. Both read
files without playing them. Run the analysis unit tests with
`python3 -m unittest discover -s tools/oracle -p 'test_*.py'`.
See [performance probes](../../docs/performance.md) for movie/device stress tests.
