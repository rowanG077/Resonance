# Overworld implementation

Core overworld travel and scene ownership are implemented in this worktree:
Sylvarant and Tethe'alla, Noishe, Rheairds and the ship, world events and
cinematics, town/dungeon entry and return, presentation, menus and persistence.
Native presentation differences and further integration checks are listed below. The separate battle implementation in `../resonance` owns combat;
world encounter lookup produces group/arena/terrain/region data for that boundary.
The shared `resonance-events::battle::Request` contract is present here; the combat
renderer/runner remains in the separate battle worktree.

## Current implementation

For a temporary flight playground, run:

```sh
cargo run -p resonance -- --assets local/overworld-skits --test-overworld
```

Use a complete current cooked asset directory instead if this local fixture is
unavailable. Town and dungeon entry requires the complete field catalogue; a
world-only cook cannot supply those destinations. The local test fixture now
includes all 501 cooked field inventories. This skips the opening and starts a fresh four-rider party on Rheairds
near Iselia with 100,000 gald. Story progress starts in early Sylvarant, after the
Sylvarant Base rescue, with Kratos in reserve. Rheairds are granted independently;
late-story events and the interworld portals retain their normal story conditions.
Battles in this temporary mode resolve immediately as victories,
including scripted encounters and enemy-symbol contacts inside fields. No combat scene or battle rewards
are generated. Hold **Space/Enter** to fly, **A/D** to turn, **W/S** to pitch,
**B** to land/take off, **C** to toggle close/distant camera views, and **M** to cycle the map.
On a gamepad, use A, the left stick, B, right-stick click, and Start respectively.
Inside a field, **Home / Start** returns directly to the overworld. Fields keep
supported scripts; an unsupported script switches to an exploration preview
with walking and terrain collision. Story-locked entrances retain their scripted
message and return to the world. A small HUD label
identifies this mode. Puzzles, the Sorcerer's Ring and moving platforms are
outside this test's scope. The preview flag survives test saves.
Closing the window deletes the temporary checkpoint and test save slots; pass
`--save-directory` explicitly if you want to retain test saves. Normal game startup
is unchanged. `--test-overworld --capture OUTPUT.png` also permits a headless smoke
capture through the same startup path.

`resonance-content::overworld` owns checked map coordinates, travel state,
collision/encounter/movement tables, full-resolution landmarks and runtime world
packages. The importer reads these from the supported English field module and
executable. Terrain preparation shares the production decoding graph and binds
one tile at a time. `worlds/world.json` inventories both worlds, all 228 terrain
sources (including story variants), the shared VM, skits and world visual assets.
World loading verifies every dependency before creating a session.

`resonance-game::overworld` supplies collision, mount control, landmark services,
world session ownership and checkpoint/field-return state. Collision retains
source face order, four-corner clearance, ship overlap rejection, alternative
headings and slope correction. The movement controller supports regional Noishe
unlocking, animation-token completion, Rheaird takeoff/inertia/altitude/landing,
and paired ship docks with timed boarding and disembarking. Invalid input or
missing collision data cannot partially commit movement. Rheaird portals switch
between the two worlds after their story unlock, with contact hysteresis to
prevent repeated prompts on cancellation or arrival.

Three resident enemy-symbol slots use the original two appearances and five
movement behaviors, spawn grace/radii, surface checks and bottle overrides.
Foot/Noishe contacts publish a battle request with the terrain/region encounter
selection. Sandworm uses its dedicated encounter and arena. A retained request
freezes the source scene until victory or escape is acknowledged exactly once;
retiring the scene cancels stale callbacks. Battle writeback retains party changes,
clears symbols and restores travel audio. Overworld bottle duration counts moving
updates rather than event ticks and pauses with menus and encounters.

Story progression updates landmark interaction and model selection. A single script
pass selects the alternate terrain tiles for both collision and presentation
preparation. Landmarks provide entry prompts, automatic events, item discoveries, party requirements
and guidepost flags. Nova's Caravan also uses the world module's separate story,
quest and key-item conditions to select active camp records. `world/caravan.sym`
owns those conditions, the route and its typed persistent `stop` variable. The
generic `world::rules::on_enter` hook advances its later roaming route on Sylvarant entry; ordinary
updates and checkpoint restoration preserve the selected camp. The original world script runs through the shared event VM;
landmark entry supplies its approach octant and takes exclusive foreground control.
Town preparation receives persistent progress and a copy of the world return pose.
The field's native world-return request carries a landmark/direction, separate
from ordinary field-position arguments. Preparing a return leaves its source VM
and pending operation intact until the owner accepts the destination. Ground
returns are pushed outside active entrance radii and resolve terrain height before
the first contact prompt. Checkpoint restoration preserves its exact saved pose.

World and field skits share preparation and playback, including script globals,
declared script state, party progress, flags, media clocks, skipping rules and completion.
Normal playback commits progress while preserving the caller's dispatcher registers
and local memory; preview playback leaves progress unchanged. Cooking includes
available event-only and disabled-notification scripts, since world events request
some directly. These resources do not become ambient notifications. Stable world
checkpoints preserve mount, position, heading, camera and play time; event/mount
transitions cannot be saved midway.

The desktop scene owner now prepares world exits asynchronously, registers map
3000 when a world package exists, and transfers the world return pose into town
entries. A dedicated renderer loads terrain, party leaders, Noishe, Rheairds,
the ship and landmarks through the shared mesh/material/animation loaders. It
wraps positions at both seams, selects story markers, connects mount animation
completion, and presents entry/discovery prompts and skit portraits. The original
camera now settles between its native pitch limits with terrain clearance,
acceleration, target lead and flight banking. The original
world music and vehicle sounds use the live scene mixer. F5/F9 quicksave/load
and startup `--load` now accept world checkpoints; field saves retain their
existing format. Loading prepares a candidate before replacing the live scene.
Failed area preparation retains its pending request and source scene, presenting
Enter/A to retry or F9 to load a quicksave instead of exiting the application.
The shared party/inventory/system menus now pause world simulation and return
party edits to the active world. World save slots serialize typed world state;
the slot list also reads existing field saves. The map opens on the current world
and marks the travel position. Catalogue previews use the shared menu viewer.
Rheairds display the first four party members with individual craft colors,
height-dependent formation spacing and animated, additive exhaust. Ship boarding
and disembarking scale the ship at the sea endpoint while the player waits or
returns to land. Sailing has a 24-instance wake pool and bow effect. Vehicle
engines restart when their original sound programs finish, vary volume with speed,
and stop while the menu is open.

Keyboard controls currently use WASD/arrows for movement, Enter/Space for confirm
and vehicle throttle, X for Noishe, B/Escape for a vehicle or cancel, Q/E for camera
rotation, C for camera perspective, M for HUD map cycling, Tab for the menu, Z for
skits and Home for skit skipping. Gamepad equivalents are the left stick, A, X, B,
L/R, right-stick click, Start, Y, Z and Start; the right stick controls direct
vehicle motion. Start skips an active skit and otherwise cycles the map.
The HUD cycles between a north-up small map, a north-up full map and hidden,
fading by 16 alpha units per simulation tick. It reuses the prepared map artwork
and marks the party and visited visible landmarks. Map mode and camera perspective
survive field transitions and saves. Camera distance eases between the authored
perspectives without changing the party position. View controls respect event,
prompt, mount, skit, menu, cinematic and battle ownership.
Optional ambient skits with uncooked availability predicates stay unavailable;
explicit event requests can still play their prepared scripts.

The follow-up overworld fixes include:

- Field-entry cameras rebind the world's actor-zero target to the field leader,
  so Triet opens on Lloyd inside town. The first House of Salvation doorway now
  completes its movement, turn, door animation and interior handoff; its interior
  exit is covered by the original-script regression test.
- Entrance, guidepost and item notices use the original wording, 24-pixel text,
  26-pixel line spacing and tight 12-pixel frame inset. They slide/fade down from
  the top; town prompts place the original A/Enter and B/Leave ribbons separately
  below the notice. Available skits
  use the shared lower-left notification. The HUD's camera-direction cone fades
  toward its outer edge.
- Discovery skits render the shared dialogue windows and response choices.
  Up/down selects a response and Enter/A confirms it. Their original colored
  circle model scrolls both texture layers and disappears during vehicle travel.
- Terrain submits complete tiles in camera order to preserve transparency at
  forest edges. Portals use their native visibility range and animation; this
  prevents unused, distant reference spheres from appearing over the ocean.

Follow-up verification includes rendered Triet, wrapped entrance/guidepost
notices, the Lloyd/Raine choice window, discovery circles, and the ocean beside
the Tower of Salvation. The original-asset tests cover the House of Salvation
doorway/return and all 33 discovery skits, including restoring world control
after the coastal Raine contact. All 501 field inventories and the available
world-to-field routes were also checked. Exact native image parity is still
subject to the limitations below.

The next field-exploration fixes cover the Salvation door's absolute bone angles,
its continuous stair ramp, and both exterior world exits. Native world-exit
action 3 is accepted, and valid scripted returns no longer trigger exploration
fallback. When fallback is necessary, caption and save-point actors are retired
with their controllers so they cannot request missing models or unwarmed material
pipelines. Presentation assertions remain enabled.

Field animation visibility now follows the actual display aspect ratio, including
save points outside the original 4:3 framing. Scripted grounded placements snap
back to the walking floor before control returns; this fixes Lloyd being left
above the ground after Nova's Linkite Tree conversation. Actor reactions resolve
the current-leader alias, including the reaction when examining Colette's hole
in Triet at the temporary test's story state.

Original-asset regressions exercise walking upstairs into field 96, both Salvation
exits to the world, all four Linkite conversations followed by movement, examining
Triet's hole through the normal interaction prompt, and Iselia's denial/return.
GPU probes also complete Iselia field 346's denial, Triet's exit and Salvation's
exterior exit back to the overworld with the presentation and pipeline assertions
enabled. Rendered captures cover all three overworld notice types.
Run them with:

```sh
RESONANCE_WORLD_ASSETS=/absolute/path/to/world-output cargo test -p resonance-presentation new_game::tests::exploration -- --ignored
```

Bed triggers expose the original Rest action. Inn scripts query saved skit history,
and music requests use a typed command enum so the rest jingle preserves the room
score for restoration. Scene props initialize animation before the next script
instruction, allowing Thoda's falling rocks to remain paused until their event.
Field audio preparation inventories ambient actor cues as well as explicit sound
calls, including Thoda's sound 286. Chests remain visible across the ground landmark
horizon, with a gradual fade at its outer edge.

The cooked field audio audit compares every script branch and actor-bound ambient
cue with each field's sound and preload inventories. Dynamic sound references
require the full catalogue. Run it with `RESONANCE_WORLD_ASSETS` set:

```sh
cargo test -p resonance-import original_prepared_fields_include_every_scripted_sound -- --ignored
```

The exploration fixture refresh added missing references in 75 field inventories;
the subsequent audit passed all 501 maps. A device-free mixer regression renders
Thoda's ambient cue and checks room-music restoration after the inn jingle.

The GPU transition probe accepts optional approach direction, confirmed exit
trigger (`none` to omit), and a field override for reproducing a specific crash:

```sh
cargo run -p resonance-presentation --example overworld_field_probe -- ASSETS 3 NEW_OUTPUT 2 1000
cargo run -p resonance-presentation --example overworld_field_probe -- ASSETS 2 NEW_OUTPUT 2 none 346
```

## Known limitations and integration checks

- This test targets overworld travel, field entry and walking. Exploration
  previews intentionally stop incomplete scripts; they do not implement every
  dungeon mechanism, NPC event or dynamic scenery collision. Nova's Caravan has
  additional original NPC-dialogue and world-return coverage. Battle requests
  return victory automatically when `--test-overworld` is enabled.

- Exact native visual parity remains unverified, including animated water,
  particles, HUD map layout, some camera constraints and the cloud cinematic. Cosmetic shake and
  wake jitter use a separate deterministic sequence rather than native shared RNG.
- Desktop ownership tests exercise entry/exit, menus, saves and failed preparation
  with original assets. Interactive controller/keyboard routes and Dolphin image
  comparisons remain additional verification work.
- The separate combat runner must use the active scene's `events()` /
  `events_mut()` accessors when the worktrees are integrated. This worktree supplies
  the tested encounter request, frozen source scene and result-writeback boundary;
  it does not contain the sibling worktree's combat renderer.
- Optional ambient skits with unsupported availability predicates remain hidden;
  explicitly requested, prepared skits still run.

## Asset contract

Landmark data retains full positions, radii, interaction classes, model resources,
item rewards and party requirements. Menu map points are downscaled and cannot
substitute for these travel records. World terrain packages have local collision
geometry in member 4; the shared event program is `field_sil.so`.

Collision-table columns 1 and 3 have no established mount meanings. Ground
movement uses column 0 and ship movement uses column 2. Rheaird flight bypasses
horizontal ground collision; landing requires clear terrain and resolves
landmark contacts first.

## Validation so far

Content, event and game unit suites and event integration checks cover the
runtime behavior below. Original-disc checks reconstruct collision, encounter and
movement tables and full landmark positions from both discs. The terrain package
check binds all 228 sources and verifies their runtime inventory and motion files.
The existing collision sweep covers base tile centers/seams and alternate meshes.
The original world VM sweep reaches field/skit boundaries for all 160 handlers at
seven story milestones and four approach directions (4,480 cases).

The skit cooker prepares 475 available resources on disc 1, including all 94 direct
skits; source comparisons verify scripts/messages. This does not prove that every
skit scene renders correctly. Scene tests cover discovery/reward persistence,
guideposts, full-inventory behavior, skit suspension/completion, and successful or
failed field-return preparation without consuming the source operation.

```sh
cargo test -p resonance-content -p resonance-game -p resonance-events --lib --test events
cargo test -p resonance-import --lib original_overworld -- --ignored
cargo test -p resonance-import --lib original_world_templates -- --ignored
RESONANCE_ASSETS=/absolute/path/to/all-assets RESONANCE_COOKED=/absolute/path/to/current-cooked cargo test -p resonance-game --test overworld_assets original_world_script -- --ignored
RESONANCE_COOKED=/absolute/path/to/media-library RESONANCE_WORLD_SKITS=/absolute/path/to/disposable-world-output cargo test -p resonance-import --lib original_world_skit_resources -- --ignored
RESONANCE_COEFFICIENTS=/absolute/path/to/dsp_coef.bin RESONANCE_WORLD_ASSETS=/absolute/path/to/world-output cargo test -p resonance-import --lib original_world_terrain_packages -- --ignored
RESONANCE_WORLD_ASSETS=/absolute/path/to/world-output cargo test -p resonance-presentation --lib original_world_ -- --ignored
RESONANCE_WORLD_ASSETS=/absolute/path/to/world-output cargo test -p resonance-game --test overworld_assets original_world_enemy -- --ignored
cargo clippy -p resonance-content -p resonance-game -p resonance-events -p resonance-import -p resonance-presentation --all-targets -- -D warnings
cargo fmt --all -- --check
```

Importer disc checks use `local/extracted`. World package checks need shared game
JSON and skits already prepared in the output root. Audio output must be an isolated
writable directory because this check prepares music and vehicle sounds. The
regular `cook-all` graph supplies these inputs.
Environment paths should be absolute because Cargo runs tests from crate folders.
The original package session check passes eight combinations of world/story,
including checkpoint round trips and town-entry ownership. Desktop-owner tests
restore all four mounts from serialized saves and execute the original Iselia
world-to-town-to-world scripts. Injected preparation failure preserves the source
and pending request without automatically retrying. Two additional camera tests
cover pitch settling, obstruction clearance and failed query ownership.
Menu checks cover all four mounts: simulation pauses, closing restores control,
and serialized menu saves preserve the exact world pose. The rendered main menu
and retry notice both use resident assets with zero late reads.
Original-asset enemy checks populate all three slots in both worlds, select native
encounters, freeze clocks during battle and resume after completion. A wide-camera
render shows both enemy appearances with no late reads. Audio tests synthesize
all four world music tracks and both vehicle sounds, checking audibility after
eight seconds, finite samples and sound-slot retirement without a device. Static render probes cover
foot, Noishe, Rheairds, ship and Tethe'alla with zero asset reads after activation.
They exposed camera framing, initial floor height and empty UI mesh issues; the
corrected captures now show terrain clearance and the ship's horizon view without
renderer errors. Further captures show a four-person Rheaird formation with
speed-dependent exhaust and a moving ship's wake, again with zero late reads.
Water edge artwork and full mount transition sequences still need verification.
`overworld_capture` is a silent observer using
the production package and renderer:

```sh
cargo run -p resonance-presentation --example overworld_capture -- /path/to/world-assets /path/to/probe.json local/native/overworld.png
```

A probe contains `story` and a serialized `TravelState` under `state`; optional
`camera_ticks` defaults to 300 to settle the stationary view without stepping the
scripts or player. `simulation_ticks` runs stationary gameplay (including symbols),
`formation` selects party order, and `throttle_ticks` drives vehicles through the
production scene, including their effect clocks. It does not
represent a gameplay or Dolphin acceptance route. No complete Dolphin or desktop
overworld route has been verified yet.

On this workspace's Mesa software renderer, use `WGPU_SETTINGS_PRIO=webgpu` and
`VK_DRIVER_FILES=/run/opengl-driver/share/vulkan/icd.d/lvp_icd.aarch64.json` for
captures. The default maximum-feature device request fails on its advertised
cooperative-matrix capability; no renderer source changes are needed for that
environment issue.

## Numbered event ownership

Field scripts enter numbered presentations through `PlayWorldCinematic` (opcode
`0x4C`). Its six arguments are the
presentation ID, following field ID, x/y/z and heading. For a following field ID
at or above 3000, x is the world landmark and heading is its approach direction.
`PlayWorldCinematic` preserves that continuation while its source VM remains
suspended. Preparing the cinematic succeeds before the source VM is cancelled.
The cinematic owns input, its own animation clock and the subsequent destination
request. Failure leaves that request and scene intact for retry; completion never
resumes the retired caller. Cinematics cannot be saved, moved or interrupted by
the menu. Field continuations retain the saved world return pose.

Numbered presentations load `FIELD/e00.d` through `e13.d` for IDs 513..526. Each archive has
matching CAMM eye/target times and original actor packages. The camera starts at
0, advances by 0.25 in each evaluator call, and receives two calls per update.
The actor/camera field origin is `(35200,-25600,0)`, corresponding to map position
`(38400,28800,0)`. Scene 516 chains to 517 before the following destination;
returning from 518 to a world landmark restores Rheaird flight. This was verified
against the native loader/update/main paths and the original field call sites,
including FAC_D00 (523 -> field 39), GRA_D00 (521 -> field 238), ELA_D05
(526 -> field 128), and OZA_T00 (519 -> field 425).

World package version 2 requires all fourteen cinematics and the nine dialogue
records in scene 517. The importer binds their original camera tracks, actor clips,
embedded spoken lines and native sound programs. Runtime playback applies the
native UV controller modes, actor fades, scene fades and timed subtitles. Scene
521/526's extra actor is used as the scene background. Cinematic shaking uses a
separate deterministic cosmetic sequence and does not consume encounter RNG.
The cinematic vertical field of view is 18.9 degrees; ordinary travel uses
31.668 degrees.

`overworld_capture` also accepts `cinematic: 513..526`; `simulation_ticks` selects
the observed timeline frame. It uses the same prepared scene and renderer as the
desktop owner, including subtitles and fades.

Original-asset cinematic preparation checks all fourteen camera endpoints and
actor counts, decodes every bound motion against its skeleton, and verifies the
nine embedded voices plus every native cinematic sound cue. Desktop ownership
checks play every numbered scene through its prepared continuation, including
516 -> 517 and 518's Rheaird return, while retaining and cancelling each source
operation exactly once. The ordinary mount/menu/save and Iselia round-trip checks
also pass with world package version 2.

Rendered observations `local/native/overworld-cinematic-513.png` (tick 400) and
`overworld-cinematic-516.png` (tick 80) show the original boat departure and
Rheaird departure effects. Both observations completed with zero late asset
reads. These are production-renderer observations, not Dolphin image matches.

The initial implementation passed the non-ignored importer, presentation,
content, event and game suites, script metadata tests and all-target Clippy.
Original-disc checks are run separately where noted above. Regression checks
for the temporary playground are recorded below.

Additional rendered observations cover scene 517's voiced subtitle (tick 600),
scene 523's explosion (tick 100), and scene 521's cloud background (tick 120).
They have zero late asset reads. Scene 526's cloud-only frames at ticks 40, 600
and 980 need an original-game image comparison before claiming visual parity.

The final HUD checks exercise all three display modes and both camera perspectives.
Travel tests cover smooth zoom, map crossfades, all four mounts, legacy save defaults
and restoration; ownership tests prevent view input during prompts, events, menus,
cinematics and battles. Rendered observations are under
`local/native/overworld-hud-{small,full,hidden}.png`, with zero late asset reads.
These checks also caught and corrected the distinction between the ordinary
31.668-degree travel projection and the 18.9-degree cinematic projection.

## Temporary playground regressions

Overworld story decisions now run from the prepared `world::rules` module:
landmark appearances and access, caravan availability/route, discovery conditions,
portal availability, visit recording, music selection and terrain variants.
`scripts/world/` supplies typed IDs and explicit `UnknownId…` variants for values
whose identities are not established. Rust retains geometry, storage, validated
script bindings and presentation ownership. The existing battle handoff is unchanged.

Field treasure interaction starts `field::treasure::open` in the existing event
scheduler. The script controls animation waits, reward branches, sounds, receipt
messages, acknowledgement and full-inventory closing. `TreasureKind` and
`TreasureReward` also replace raw style/reward encodings in the Rust field state;
the original native ABI converts at its boundary. Script preparation uses the
verified cooked sources and validates all receipt glyphs before activation.

Repeated Salvation rests now finish the original optional-skit scan: the field
ID query and saved battle-participation query are supported, including when no
skit qualifies. Enemy-symbol command `0x0f` shares the pending battle/victory
handoff with scripted battles. Requests distinguish weighted encounter pools
from fixed formations. Skipped encounters record the native battle counters once;
the resumed field script removes a defeated symbol and restores control.
Original-asset tests cover repeated early- and late-story rests, unchanged camera
settings, healing, NPC collision, downstairs exit, and all three enemies in
Triet Ruins' main room with walking after each victory.

The nine reported issues are covered by the field-entry fallback and preparation
checks, north-up HUD maps, an explicit C camera-view hint, closed/one-shot treasure
poses, terrain frustum culling and native fog, corrected water UV controllers,
local-axis Rheaird pitch/banking, stopped movement during reward receipts, and
hidden optional weapon geometry on both Lloyd mesh layers.

The exploration recovery check covers script errors during initialization and
later updates, then verifies walking. Battle bypass checks cover world encounters
and scripted victory return registers. These modes are enabled by the temporary
launcher; ordinary script execution still reports unsupported services.

Current regression results: all 62 distinct destinations reached by the overworld
entrance sweep grant walking control. Nova's Caravan also completes its original
NPC conversation and scripted exit. Direct Home/Start return checks cover normal,
story-closed and unsupported-script fields. The catalogue prepares all 501
inventories and exercises walking across all 500 maps containing room geometry;
map 393 is an empty unused archive. The two catalogue edge cases were rerun after
fixing preview collision with incomplete NPC/prop setup. The Ymir Forest GPU
capture verifies visible scenery, Lloyd and the preview/return hint.
