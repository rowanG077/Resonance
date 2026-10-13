# Iselia battle repair and early-encounter audit

Baseline: `rowan/battle1` at `959b0a1e264c50390632bf5b781210c2ad953460`,
rebased onto `origin/main` at `eafd991473905818a8af5ff294273ce9f9386bbf`.
Evidence is retained in `local/battle-repair-20261011/`.

Latest batch: [early battle repair and targeted validation](battle-route-2026-10-11.md).
The default library contains the selected repairs documented there. The findings
below describe the earlier baseline; whole-route fidelity and all 30 deferred
source entries remain open.

## Reported rendering failure

The normal window reproduced the reported Iselia zombie failure: the closed
Shift+Tab menu covered almost the entire arena. Both cameras used render layer
28. Offline captures did not reproduce it because they omit that developer menu.
Battle now uses its own layer, with a regression check against both the active
and preparation cameras. The developer menu's behavior is unchanged.

The battle camera and HUD also assumed a 4:3 display. They now use the selected
resolution, including the existing scene-to-display height conversion. Bitmap
glyphs retain their proportions within the HUD's fixed text cells.

`checkpoint_replay --window` records silent screenshots through the ordinary
window loop. This makes the actual reported configuration reproducible without
adding an alternative game implementation. The original offline mode remains
available. Before-fix evidence is in `live-before-release/`.

The final release replay completed at both 1920×1080 (`live-fixed/`) and 640×480
(`live-fixed-640/`). Each run recorded entry and eight combat screenshots across
1,765 gameplay updates, with zero diagnostics and zero unprepared reads. Visual
inspection confirms that the arena and textured characters are visible, the
closed menu is absent, and HUD text retains its proportions. These runs stop
during combat. A separate 1920×1080 run (`live-victory/`) then completed the
zombie battle with ordinary attack input: one Victory commit, battle ownership
retired, and field 332 resumed. It completed 1,963 updates with zero diagnostics
and zero unprepared reads. Its field screenshot is the first frame of the
return fade, not a later free-control checkpoint.

## Encounter coverage through Triet Ruins

The inventory contains 82 distinct formation/arena pairs, derived from the
early field scripts, encounter pools, and Iselia/Triet world areas. It includes
the scripted Iselia fights, Martel Temple and Vidarr, Iselia's destruction,
Triet, the Sylvarant base and Botta, and Triet Ruins through Ktugach. Random
formations and applicable world terrain variants are included. Formation 7 in
arena 14 is included conservatively. This inventory does not enumerate every
party, equipment, story-flag, or enemy appearance variation.

Four resident Dolphin workers captured every pair using main's parallel runtime.
Each accepted segment contains 90 consecutive images and matching observations
after entry warmup: 7,380 frames in total. Every observation confirms battle mode
and the requested formation and arena. Four captures failed the strict frame
count gate initially; fresh retries passed without relaxing that gate. Failed
attempts remain available. Capture metadata records hashes and silent audio.

These are original-game entry/early-combat references using the opening party,
not completed battles or evidence that the native implementation passes.
`source-validation.json` maps every case to its accepted capture.

Random encounter selection was missing from native entry. The importer now
publishes weighted pools, and entry resolves a pool before preparation using
the existing transactional gameplay RNG. Selection uses the actual weight total;
some source pools total 110. The default cooked formation table and its inventory
and save identity have been refreshed. No full recook is needed in this checkout.

## Native failures still requiring implementation

The branch is **not playable through Triet Ruins**. Only arena 13 is currently
published by the normal cook. Other early arenas need complete import and render
support, including animated scenery, fog, and effects. A provisional arena decode
was used to expose subsequent preparation failures, then removed from production
code and the active inventory. Its investigation patch is retained locally.

With those provisional arena assets, strict CPU preparation and a 600-update
simulation probe produced the following results across the 82 cases:

| Result | Cases |
| --- | ---: |
| Prepared and simulated: formations 1, 2, and 111 | 3 |
| Missing native enemy action | 57 |
| Effect model layer bindings differ | 12 |
| Weapon layer skeleton differs from its mesh | 6 |
| Encounter appearance override is not prepared | 4 |

These are first failures per case, not a count of independent defects. This probe
used the opening party and did not validate GPU rendering, victory, or field
return. Its result is `native-cpu-complete.json`; it is not acceptance evidence
for the final arena inventory. The ordinary Vidarr script also requests an
unsupported route value that the zero-route reference fixtures do not exercise.

The next implementation work is to publish the required arenas, prepare real
actions for the early enemies, resolve model bindings and appearance overrides,
and support scripted entry routes. Then repeat the failing native cases and
exercise victory and field return along the story route. Do not substitute
generic attacks or silently drop scenery to make these checks pass.

## Focused checks

- Formation import/selection: 5 tests passed.
- Battle presentation/ownership selection: 38 tests passed; 14 asset-dependent
  tests remained ignored by that unit-test command.
- Oracle lifecycle: 17 tests passed.
- The release game and window replay example built successfully.
- Formatting and whitespace checks passed.

Physical controller testing remains the agreed manual follow-up. No external
implementation source was used; the references are source assets and observed
Dolphin behavior.

## Overworld inventory repair

The earlier cursor restoration republished `ui/dialogue.json` and `ui/menu.json`
and updated the shared manifest, but omitted `worlds/world.json`. Its two old
dependency records caused the loader to reject valid shared artwork on world
entry. This was a mistake in the local refresh, not evidence of missing assets.

Repaired those two records in `local/all-assets` after matching the old hashes
against the preserved UI backup and the new hashes against the refresh receipt
and actual payloads. The maintained `refresh_battle` example now refreshes world
and field inventories with the same function. Runtime asset checks remain intact.

Evidence is in `local/overworld-repair-20261011/`:

- The current loader rejects the preserved old manifest with
  `inconsistent dependency ui/dialogue.json`.
- All 6,710 world dependencies match their recorded hashes and sizes; all 502
  field/world inventories agree with their shared dependencies.
- The existing silent GPU probe completed world → Iselia field 330 → world,
  including the confirmed exit trigger 1000, and saved `iselia-roundtrip/field.png`.
  This uses the playground fixture with a field override, not the full opening
  story and its battles. The first attempt selected the destroyed-village variant
  and failed the probe's requested-exit assertion after its automatic return.
- The refresh example compiles; formatting and whitespace checks pass.

No full recook was performed or is needed for this repair. This validates the
reported loading failure, not battle fidelity or every overworld visual.

## Requested full recook

Rebuilt from an empty `local/all-assets` using both extracted discs. The strict
world probe exposed a second omission: cursor texture dependencies from main
were missing in shared preparation, shared descriptor closure, and world
preparation. Restored those entries and reran the complete `cook-all` command.
The final library was generated by the importer without manual asset patches.

Final report: 69,505 conversion units, zero failures, and 30 deferred battle
resources. Verified 68,857 unique files across 503 shared/world/field inventories:
no hash conflicts, missing inputs, invalid payloads, or omitted dialogue/menu
texture dependencies. Save identity is unchanged. The strict silent title capture
and world → Iselia field 330 → world probe both passed. This does not establish
full battle support or visual fidelity. Logs, captures, and verification results
are in `local/full-recook-20261011/`; the prior library is preserved at
`local/all-assets.before-full-recook-20261011T130100Z/`.

## Battle checks against the freshly cooked library

The current-library preparation sweep attempted all 82 formation/arena pairs.
Only formations 1 and 2 in arena 13 prepared and simulated for 600 updates.
The other 80 stopped at missing stage resources. This confirms an importer
coverage gap; repeating the same cook cannot supply those arenas.
Results are in `local/battle-pairs-20261011/native-preparation.json`.

Two parallel Dolphin fixtures and two native recordings then captured formations
1 and 2 with neutral input at combat ticks 150, 250, 350, and 450. Initial party
stats and equipment matched. All eight comparisons failed: actor trajectories,
camera placement, and the HUD differ. For example, Lloyd remains at X = -300 in
Dolphin and X = -200 natively. Camera-eye differences range from 632 to 2,198
world units. Between 93.0% and 96.4% of pixels exceed the existing tolerance of 8;
the allowed changed fraction remains 1%. These are state and presentation
failures, not rendering-only discrepancies.

Both native recordings completed without diagnostics or unprepared reads.
That result establishes capture completion only. It does not establish fidelity,
arte coverage, victory, or field return. Side-by-side images and detailed
observations are in `local/battle-pairs-20261011/index.html` and
`comparisons.json`. Full battle validation remains incomplete.

## Static arena investigation

The importer now accepts additional static scenery sections and the battle view
draws them with the background. In an isolated library, arenas 13, 24, 26, 27,
28, and 29 decode with 1, 1, 6, 9, 9, and 17 layers respectively. All 95 unique
dependencies match their recorded sizes and hashes. The arena 13 descriptor is
byte-for-byte unchanged. Animated models, moving textures, fog, and recurring
effects remain explicitly unsupported. The normal cook still publishes arena 13;
these provisional imports have not been added to the default asset library.

Two parallel Dolphin fixtures and two native recordings placed formation 1 in
arenas 26 and 29 to isolate scenery. They used the same opening party and neutral
input, capturing combat ticks 150 and 250. The arena 29 reference initially failed
the strict VI/image count check; a fresh retry passed, with the failed recording
retained. Both native recordings completed with zero diagnostics and unprepared
reads, but **all four comparisons failed**. Buildings or pillars obscure combat
in the native views. Camera-eye differences are 627–703 world units, and
78.0%–94.5% of pixels exceed the unchanged tolerance. These fixtures do not prove
natural encounter entry or correct scenery visibility.

Repeating the 82-case CPU sweep against this isolated library produced 3 simulated
cases (formations 1, 2, and 111), 59 missing stages, 13 missing enemy actions,
4 effect-model binding failures, and 3 unsupported appearance overrides. These
remain first failures, not complete defect counts.

The captures also constrain the camera investigation: across 840 observed combat
updates in the two opening formations, the source focus equals the midpoint of
the controlled actor and target from the previous VI, with height 120, to within
0.00008 world units. The native camera instead frames roster bounds. Other camera
relationships still vary during combat; this observation alone is insufficient
to replace the camera behavior.

Evidence, one-off probe programs, and side-by-side captures are retained in
`local/battle-arenas-20261011/`. No default assets were modified during this
investigation. Camera and scenery visibility need resolution before publishing
these arenas as supported encounters.

## Scenery attachment repair and expanded comparisons

Rendering arenas 26 and 29 with the measured Dolphin camera did not remove the
obstructions. The additional model sections were missing their authored `kk`
attachments: repeated pillar meshes had been stacked at the arena origin.
The importer now stores each static attachment's full transform, and the battle
view composes it with the arena transform. Missing placements fail validation.
The six isolated stage imports still verify all 95 dependencies, and arena 13
remains byte-for-byte unchanged.

The corrected camera-isolation probes show the buildings and pillars in their
authored positions instead of covering the combatants. These probes are diagnostic
only: they override the rendering camera, while actor behavior and the HUD remain
native. Residual image differences remain, so they are not visual acceptance.
The temporary override was removed from production source and the ordinary
capture binary rebuilt before the runtime comparisons.

Two parallel Dolphin workers added formation 111 in arena 24 and formation 1 in
the same arena as a control. Formation 111's first recording failed frame
registration; a fresh retry passed without weakening the check. Four native
recordings then ran in parallel using the ordinary camera: these two cases and
formation 1 in arenas 26 and 29. All completed with zero diagnostics and
unprepared reads. All **eight** comparisons at combat ticks 150 and 250 still
failed, with camera-eye differences of 627–1,120 world units. Camera, actor
placement, actions, effects, and HUD fidelity remain unresolved. These opening
party fixtures do not establish all-arte, victory, or story-route coverage.

The maintained battle watcher now follows the loaded module's section table to
its battle pool, rather than retaining an absolute address across reloads.
Failed frame registration also retains its candidate image list and observations
for diagnosis. Four watcher tests and 17 lifecycle tests passed; both normal and
diagnostic renderers built successfully. The fresh source captures exercised the
updated watcher.

Evidence is in `local/battle-camera-20261011/`: `scenery.html` shows the attachment
diagnosis, `index.html` shows the ordinary runtime failures, and `result.json`
records the current runs. The default asset library remains unchanged.

## HUD layout and artwork repair

The party HUD now uses compact 64-pixel portraits, slanted HP/TP numerals and
shaped red/blue gauges. The importer publishes the meter colors and controller
marker palettes as battle UI version 34. The wide name cards, condition captions,
top-left enemy text list and text-only target labels have been removed. Enemy
icons use the observed bottom-right rows, including per-group ordinals. Target
and stun indicators use the cooked atlas artwork. Casting keeps the neutral
portrait expression rather than selecting the attack expression.

Three native recordings ran in parallel: formations 1 and 2 in arena 13 and
formation 111 in the isolated arena 24. The final runs completed with no
diagnostics or unprepared reads. They reuse the registered Dolphin captures at
matching combat ticks. All **ten full comparisons still fail**: camera-eye errors
remain 632–2,198 world units, and 83.5–91.0% of pixels exceed tolerance 8. Actor
placement, action timing and combat state still differ. Portrait flashing, enemy
icon animation, action banners and floating damage also remain unresolved.

A separate diagnostic crop of Lloyd's unchanged panel at combat tick 150 reduces
mean RGB error from about 74 to 5.5–6.1. This establishes a layout improvement,
not HUD or battle acceptance. The full-frame state and image gates are unchanged.

All 13 HUD tests pass, including the asset-backed cases. The current artwork
publishes successfully from both discs. Only `battle/ui.json`
and `shared.preload.json` changed in the default cooked library; the arena
experiments remain isolated. All 68,857 files across 503 inventories still verify,
and save identity is unchanged. Default-library title and battle captures pass
their strict loading checks. The dedicated world → Iselia 330 → world probe also
passes. The generic `--test-overworld --capture` command instead waits for a
field-only event and exhausts its audio evidence limit; that separate capture
limitation is retained in the logs and was not changed as part of the HUD work.

Evidence is in `local/battle-hud-20261011/`: `index.html` shows the remaining
Dolphin comparison failures; `runtime-final.json`, `comparisons.json`,
`hud-regions-final.json`, and `verification.json` record the measured results.
The initial misconfigured asset tests and incomplete isolated-arena attempts are
retained alongside their corrected runs.

## Party entry and controlled-camera repair

Four party arrangements were measured at combat entry using parallel Dolphin
fixtures: the opening party, four front-row members, two split rows, and three
members outside the leader's row. Party placement now preserves the measured
row depths and within-row order. Enemy automatic placement remains incorrect;
its previous symmetry with party placement was an invented rule, so the two
layouts are now separate.

Controlled combat now focuses on the leader and target. Auto-zoom follows their
horizontal separation with a minimum distance and a delayed radius update;
fixed zoom follows the leader with a short look-ahead. Yaw approaches the battle
line at the measured rate, and eye height uses the arena pitch independently of
target height. The camera samples actors before the gameplay update. Automatic
control, selection, entry transitions and victory still require separate source
validation; their existing framing is not certified by this repair.

The camera investigation uses live observations, extracted profile data and
registered frame captures. The investigated profile offsets are zero for the six
currently supported enemy types; no speculative camera fields were added to the
cooked schema. Two additional enemy-count fixtures distinguish roster count from
enemy type. A diagnostic prediction from the recorded actor positions and prior
camera pose matches 2,022 updates across nine recordings with maximum eye error
below 0.001 world units. This is camera-model evidence, not native battle parity.
A compact checked-in recording covers delayed zoom, limited turning, fixed zoom
and the minimum distance.

Evidence is retained in `local/battle-framing-20261011/` and the sibling
`battle-framing-extra`, `battle-framing-retry`, and `battle-framing-counts`
directories. Three early automatic-placement recordings failed strict frame
registration. Two retries passed and one failed again; the rejected recordings
are not used for image comparisons. No registration tolerance was relaxed.

The native recordings exposed another implementation defect: effect admission
rejected particles that follow an emitter's origin unless the emitter was attached
to an actor or projectile. A fixed world origin is also supplied by effect
requests, including impacts and projectile emission. These particles now retain
that fixed origin; moving attachments still track and retire with their parent.
A regression test moves the owner and checks both immediate and delayed particles.

A limitation in the earlier whole-combat comparisons also needs correction:
the native fixture starts with battle seed `2844106719406811793`, while the source
recording reports entry seed `29564`; their generators are not aligned. Matching
combat ticks and party statistics therefore does not establish matching AI
choices or damage rolls. Those recordings remain useful visual diagnostics, but
their different actor trajectories are not controlled evidence of behavioral
regressions. Random inputs must be aligned or isolated before attributing those
differences to AI or movement. The party-entry observations and camera calculation
checks above do not rely on equal random outcomes.

After the effect repair, all three parallel native recordings complete through
combat tick 250 with zero diagnostics and unprepared reads. They capture 12 frames
across formations 1 and 2 in the default arena and formation 111 in the isolated
arena 24. The initial replays for formations 2 and 111 incorrectly waited for
formation 1; those failed attempts are retained, and the corrected replay files
name each actual formation. The two opening formations' camera-eye differences
at ticks 150/250 fall from 632–2,198 to 173–406 world units, but residual movement,
rendering and timing differences remain. The uncontrolled whole-frame comparisons
establish no parity pass. `before-after.json`, `comparisons.json`, and `index.html`
show the diagnostic results, including the isolated arena's remaining large error.

All 613 battle unit tests pass after the effect repair. The preceding camera and
placement batch also passed all 237 active game unit tests; 41 game asset fixtures
were not requested by that run. The final release game and capture binaries build,
and formatting passes. No cooked files or save identities changed in this batch.
The full encounter, arte, victory and progression scope through Triet Ruins remains
incomplete. `local/battle-framing-20261011/result.json` records this batch's bounds.

## Stage 4 fidelity recovery: damage and recovery display

Compared the current branch with the preserved pre-rebase snapshot `5706d84`
and the earlier repository implementation `000bf24`. The simplified camera and
HUD were already present before the rebase. The simplification also deliberately
substituted hit power and reaction policies, documented in its plan; restoring
fidelity must address those changes rather than attributing every discrepancy
to merge conflicts. No external implementation sources were consulted.

Replaced the floating-number redesign with a compact presentation-owned animation:
imported dimensions and gradients, four hit positions retained independently of
actor movement, contact expansion, and a drifting fade. Recovery numbers use the
party-row arc and overlapping italic digits. Timing derives from age; no obsolete
controller or memory representation is retained. HUD schema is now 35.

Validation: 13 focused HUD tests and the both-disc HUD publication test pass. Two
parallel strict opening-battle recordings through tick 300 and a player-attack
recording through tick 260 complete without diagnostics or unprepared reads.
Previously captured gameplay state at tick 250 remains unchanged in both neutral
recordings. The default-library recording matches all eight candidate images.
Only `battle/ui.json` and `shared.preload.json` changed in the default library;
all 17 published dependencies match their hashes and save identity is unchanged.
These are rendering and publication checks, not a complete Dolphin parity result.

Evidence: `local/battle-fidelity-recovery-20261011/result.json`. The earlier probe
that sent interaction input before fixture admission did not enter battle; its
failure is retained separately. The successful attack probe waits for combat
entry before sending attacks.

The 30 deferred entries represent 15 unique files repeated on both discs: eight
mixed battle archives and seven native modules. Their actual published paths
are recorded in `resource-coverage.json` alongside this batch. No deferral was
removed or relabeled. Arena publication still selects only 13, stored-scene
publication selects only 237, most early enemies lack actions, and some model
bindings, appearances and scripted routes remain unsupported. Full stage 4
recovery, early-route/arte coverage and visual fidelity remain required work.

One concrete binding defect for the next batch: enemy 134 effect model 0 has
identical indexed bone transforms, hierarchy and clip bytes across body and
outline, but bone 6 has different names. The current name-equality check rejects
it. Any repair must establish structural compatibility at the import boundary.


## Arena publication and indexed effect binding recovery

The cooker now scans the complete stage directory instead of selecting arena 13.
It publishes 42 arenas, including main-layer animation in arenas 0, 32, 36, 62,
and 89. Both discs produce identical descriptors; all 481 declared dependencies
verify. Scenery attachments use primary-model bone indices and sampled poses,
so they retain placement when their parent animates. Animation uses the existing
sparse, affine-capable bone evaluator and rates read from the stage metadata.

The remaining 44 arenas are still explicit in coverage. Their first unsupported
feature is recorded per arena; these categories do not claim an exhaustive
feature inventory. Fog, texture animation, separate scenery animation banks,
recurring effects, and additional rendering flags remain required work.
No deferred source entry was removed or relabeled: all 30 remain open.

Enemy 134's effect outline now binds despite differing bone labels. Preparation
reads the already verified cooked GLB skeleton and checks every indexed parent
and rest transform against the effect rig, as well as the clip identities.
The regression test admits the real model and rejects a changed rest transform.
This proves compatibility at the cooked-resource boundary without renaming bones
or weakening the structural checks. Seven model tests and the stage integrity
test pass.

Five strict native cases pass with no diagnostics or unprepared reads: formation
1 in arenas 0, 13 and 29; formation 2 in arena 13; formation 111 in arena 24.
The two opening battles at ticks 150 and 250 are byte-identical to the preceding
HUD-recovery captures. Two parallel Dolphin workers recorded 240 registered
frames each for formation 1 in arenas 0 and 13. Arena 0 scenery renders, but
whole-combat comparison still shows the known actor, action/HUD, shadow and
random-stream differences. Animation timing has not been independently measured
against the original controller clock. These checks are not fidelity acceptance.

The expanded default-library sweep still passes only three of 82 early-route
pairs through 600 updates: (1,13), (2,13), (111,27). It reports 23 missing enemy
action cases, 56 missing arena cases and three appearance overrides. The earlier
effect-binding rejection is gone from this sweep; those cases now reach the next
preparation blocker. Stored-scene coverage and the Vidarr route remain incomplete.

Published the 42 descriptors and their dependencies into `local/all-assets`,
updated the shared inventory, source index and arena coverage descriptions, and
retained backups. Only descriptors enter the shared preload; meshes and clips
remain encounter dependencies. The default arena-0 recording matches all three
candidate images with no diagnostics or late reads. No full recook is needed for
this publication. Evidence: `local/battle-stage-recovery-20261011/result.json`.

## Arena texture animation and fog

The next batch adds interval-based UV scrolling, vertical atlas animation and
camera-relative exponential-squared fog using the existing material shader.
Twenty-four controlled Dolphin sequences changed only stage asset metadata in
copied checkpoints. They establish atlas offsets, scrolling intervals, unbounded
scroll offsets and unfogged actors. Fog starts and ends beyond the camera's
horizontal distance to its target. Live edits to layer flags did not establish
their initialization behavior; those flags remain unsupported.

The cooker publishes 59 arenas from each disc with identical descriptors and
648 verified dependencies. Seventeen arenas are newly available. Twenty-seven
remain blocked: 18 by rendering flags, five by recurring effects and four by
scenery animation banks. These are first blockers, not exhaustive inventories.
All 30 deferred source entries remain open.

Five strict recordings pass at combat ticks 40, 150 and 250: formation 1 in
arenas 1, 5, 21, 76 and 13. They have no diagnostics or unprepared reads. Arena
13 images are byte-identical to the preceding batch; the published default
arena-5 images match the candidate. The source and native images still differ
in combat state, shadows, feedback and camera framing; this is not whole-battle
fidelity acceptance.

The source entry recording advances stage animation through gameplay tick 59,
holds it during ticks 60–105, then resumes. The earlier apparent constant phase
offset came from this hold; no arbitrary offset was added. Native entry timing
remains an explicit follow-up. The new texture-clock unit test covers measured
scroll steps and atlas wraparound.

After publication, the 82-pair early-route sweep still has three passing pairs.
Fifty cases now reach missing enemy action definitions, 26 lack an arena, and
three require appearance overrides. Restoring the discarded authored action
tables is the largest next blocker. Default publications and previous files are
recorded under `local/battle-stage-surface-20261011/`. The full cook requested
earlier is complete under `local/full-recook-20261011T172103Z/`; this batch uses
targeted publication and does not require another full recook.


## Enemy action recovery

The six-enemy attack-name switch has been removed. Cooking resolves source action
rows into native event timelines. The zombie's first attack now keeps its 71-tick
duration, tick-23 contact, 25-tick contact window and 10-tick recovery; its combo
hits no longer apply invented 75%/60% power multipliers. All supported enemy melee
contacts use source shapes and dimensions at sampled body/weapon bones. Shared
recoil impulses and per-hit/projectile cooldowns are recovered from asset data.
A single model player advances before collision and supplies drawing afterward.
Party attack data and actor reception capsules remain approximations.

Both discs produced identical definitions for 251 enemies and 19 recoil impulses.
Every enemy package hash matches its independently extracted input. Twenty-six
enemy definitions decode all rows; unknown semantics remain explicit preparation
errors. The 82-pair, 600-update sweep still passes only (1,13), (2,13), and (111,27).
Eleven previously blocked cases now reach unsupported projectile controllers.
The remaining cases require action effects, commands, decision features, appearance
variants or missing arenas. All 30 deferred source entries remain open.

Battle unit tests: 614 passed. Action/recoil import tests: five passed. The game
suite passed 237 tests and exposed an older effect-model fixture that omitted its
mesh. The fixture now provides a valid one-bone GLB so it tests isolation of a bad
resource; its focused rerun is recorded separately. Forty-two asset-dependent game
tests remain ignored in the ordinary suite.

Three parallel native recordings at combat ticks 40, 150 and 250 completed without
diagnostics or unprepared reads. Images were inspected. A fresh Vulkan Dolphin
zombie capture contains 180 registered frames. The second reference failed strict
frame registration (186 images / 185 VI samples), so it is not accepted as new
comparison evidence. Earlier failed attempts used an OpenGL recording header or
exhausted the short controller movie; those workers exited and are retained as
failed evidence. The accepted run uses the long neutral movie and the requested GPU.
The source/native random streams remain unaligned. Action banners, party behavior,
shadows and other visible differences remain; this is not fidelity acceptance.

Evidence: `local/battle-actions-recovery-20261011/`. Default asset publication and
its backup are recorded in `publication.json`; no additional full recook was used.

## Action effects, looping poses and arena inventory

Enemy action rows now select their authored effect programs. Emission starts on
the next scene update, retains the previous actor root, and cancels pending births
when its action disappears. Existing particles keep their own lifetime. Attack
animation flags now preserve looping poses. Source captures and controlled
asset-delay experiments support these changes; no external code was used.

Two 720-frame Dolphin captures establish emission timing and placement for the
Golem and the enemy-17/enemy-111 formation. Two 360-frame paired captures change
only a selected effect's delay: actor traces remain identical, and an interrupted
attack does not produce the delayed birth. The trace observes the newest five
particles per draw group, so absence evidence is bounded by that observation.
A separate animation trace demonstrates clip wraparound. Its Golem capture has
520 registered images; the second worker's image/VI counts differ and its images
are excluded from comparison. Only its memory trace informs the looping result.
The original disc was independently rehashed; a copied metadata typo is corrected
and documented in `metadata-correction.json` without changing observations.

The same traces expose a remaining timing defect: action age holds during model
blending in the original game, whereas native action events currently continue.
One enemy-111 effect also has unresolved scheduling differences. This batch does
not establish overall effect or combat parity.

Forty-one enemy definitions now decode every action row (15 more than the prior
batch); 38 also have supported decision policies. Only changed definitions are
republished. The existing early-route core sweep passes five pairs through 600
updates: (1,13), (2,13), (5,27), (111,27), and (120,27). Remaining first blockers
are 37 action cases, 11 projectile controllers, 26 missing arenas and three
appearance overrides. The core sweep omits audio binding and GPU presentation.
All 30 deferred source entries remain open.

A focused Golem recording found that the preceding full cook wrote all 59 arena
descriptors but listed only arena 13 in the shared preload. The cooker now adds
its actual arena publications to that inventory. The current library is repaired
from the full cook's source index; no geometry was recooked. The next focused run
found missing authored attack sound 93. Only that cue was cooked, and its entry
was added to the maintained battle audio inventory.

The zombie recording and the corrected Golem recording reach combat ticks 40,
150, 250, 400 and 550 without diagnostics or unprepared reads. The Golem capture
shows the arena and actors, but visual differences remain. The two failed Golem
attempts are retained as evidence of the inventory and sound failures.

Before the request to limit validation, the action-effect change passed 615 battle
unit tests and four focused action-import tests. The inventory fix passed the
importer release check/build. After that request, work used one-cue cooking and
focused Golem reruns only; no full validation or full recook was performed.
Published 164 changed files into `local/all-assets` with backups and identity
checks for changed publications. Temporary audit/publication helpers are archived
outside the source tree. Evidence: `local/battle-action-effects-20261011/result.json`.

## Blend timing and clip positions

Cooking now translates enemy action times into elapsed gameplay ticks, including
animation blend holds. The opening blend contributes its full duration; a later
blend begins after the pose event's tick has already advanced. Events, completion
and vulnerability bounds use the converted clock. This keeps runtime attacks on
one clock. Pose start/end bytes are converted from 60 Hz positions into the
30 Hz frame units used by decoded clips.

Playback presents a new clip's start frame, blends into that pose, then advances
its frame. The shared model player also remembers the last completed update, so
repeated drawing cannot advance the body or attachments again. Main's field
playback is unchanged.

Existing registered Golem recordings contain two uninterrupted attacks lasting
86 and 70 updates. A focused native trace disables party AI to isolate those
attack poses. All 640 clip/frame observations across eight native executions match
the two recorded reference sequences, including the second attack's pose change.
Every execution enters recovery at the matching boundary. This compares animation
and action duration; it does not establish contact geometry, damage, random-state
alignment, camera framing or whole-battle visual parity.

Twenty-seven model/attachment/player tests and five focused action-import tests
pass. Existing fixtures were updated to represent actual update IDs and the
new clip's initial frame; no tests were removed. The release build completed.
Short zombie and Golem recordings through combat tick 550 are valid, with no
diagnostics or unprepared reads. No new Dolphin capture, full recook, full-route
sweep or full validation was run for this batch.

Only 155 changed enemy definitions and the shared inventory were published into
`local/all-assets`, with backups and changed-file identity checks. The import
coverage remains 41 enemies with every action row decoded, of which 38 also have
supported decision policies. No new unsupported rows were introduced. All 30
deferred source entries remain open. Temporary publication/trace helpers are
archived outside the source tree. Evidence:
`local/battle-blend-timing-20261011/result.json`.

## Actor holds, poison contacts and enemy voices

Enemy poison contacts now use the existing condition system, sharing its type
instead of maintaining a second condition enum. Enemy action voice commands bind
through the existing cue/stream lookup. All action rows decode for 55 enemies;
52 also have supported policies. No previously decoded row became unsupported.
These are import coverage counts, not complete encounter support.

Action emitters now respect actor clock holds while already-born particles
continue. Frames publish the hold sampled during the actor update, before later
contacts can change its timer. Model playback consumes the same observation.
The existing enemy-111 recording explains the previously unresolved effect delay:
two two-update hit-stops move four births to relative updates 31, 38, 43 and 48.
A focused regression reproduces those births. This corrects the earlier assumption
that action emission always follows the scene clock. Complete animation behavior
around hit-stop remains unverified; the recorded animation cursor also shows a
resume-frame delay that this emitter comparison does not resolve.

Validation was limited to 40 focused tests, the release build, 11 affected core
encounter cases, and two short silent native recordings. Formation 109 / arena 76
now passes 600 core updates. The other ten cases reach separate attachment,
eligibility, follow-up, impact-art, projectile or effect-controller blockers.
Golem and formation 109 recordings reach combat tick 550 without diagnostics or
unprepared reads. Their arena/actor rendering was inspected; they do not establish
whole-battle parity or end-to-end enemy voice fidelity.

Published 100 changed files into the default library, including 90 definitions,
the audio descriptor and preload inventory. Only eight missing cues and one voice
stream were prepared. Backups and changed-file identities are recorded. Temporary
publication/audit helpers were removed from the source tree. No full recook, full
validation or new Dolphin capture ran. All 30 deferred entries remain open.
Evidence: `local/battle-enemy-bindings-20261011/result.json`.

## Hemisphere effects and the scorpion encounter

The rejected scorpion effect is a hemisphere. Its renderer now shares the upper
half of the sphere mesh, with five latitude bands and sixteen columns. It honors
camera orientation and interpolates color from rim to tip. Controlled Dolphin
probes show that its middle size component and anchor flag leave the image
unchanged. Twenty-five recorded size samples match the existing particle motion.
The projected hemisphere covers the recorded solid silhouette apart from one
pixel and 45 extra edge pixels; this is bounded shape evidence, not exact GPU
parity. The textured captures support the repeated panel mapping.

Only three production files changed. Existing artwork and palette tables were
reused while refreshing 21 effect banks: 33 particle declarations and 11 previously
rejected programs become available. Published 43 metadata files, including the
containing enemy descriptors and shared inventory, with backups and changed-file
identity checks. No texture or model was recooked.

The release build passes. Formation 525 / arena 5 reaches combat tick 550 without
diagnostics or unprepared reads. A second run disables party AI so attacks can
finish; its tick-490 capture contains and visibly renders the repaired enemy
particle. Neither run establishes complete combat fidelity. Special melee
attachment markers remain unresolved and were not assigned speculative bindings.
No full recook, broad route sweep or full validation ran; no tests were added.
All 30 deferred source entries remain open. Evidence:
`local/battle-tail-effects-20261011/result.json`.

## Authored contact effects

Evidence: `local/battle-hit-feedback-20261011/`.

The hit-rule effect field selects a program from the attacking enemy's effect
bank. It supplements the ordinary hit feedback, independently of the damage
element. The native hit rule now carries the prepared appearance; melee and
projectile contacts emit it through the existing effect scheduler. No separate
contact-effect clock or presentation subsystem was added. Nonzero special effect
modes and contact sound overrides remain unsupported.

Four controlled Dolphin sequences compare effect program 2 against no additional
program, both with neutral input and held guard. Each sequence has 140 registered
frames. A hit at combat tick 603 starts the extra program at the same point as the
ordinary spark; its hemisphere appears at tick 618, matching the authored delay
of 15. Damage remains equal between the paired runs. The guarded runs have the
same damage and particles, showing that guard suppresses the extra effect.
`source-evidence.json` records the samples. Initial probes treated the field as
an element selector and selected nonexistent programs; they are retained as
failed hypotheses and are excluded from acceptance evidence.

Six focused importer tests and one contact/guard test pass. A temporary metadata
publication check also passes and was removed from the source tree. A release
build of the application and battle capture tool passes. Existing test fixtures
only gained the empty value required by the shared hit rule.

The refresh resolves 13 rejected rows and retains 19 decoded contact-effect
bindings. Enemies 4, 5 and 6 now have complete decoded action tables, raising the
count to 58 (55 with supported policies). This is importer coverage, not proof
that these enemies can complete a battle. The mixed formation 509 / arena 5 fails
preparation on enemy effect bank 14, program 4, controller 25. Its failure remains
in `native-results.json` and has not been downgraded or bypassed.

The ordinary scorpion encounter 525 / arena 5 passes through combat tick 600 with
no diagnostics or unprepared reads. A separate copied scorpion definition adds
program 2 to contacts solely to exercise the new path. That recording passes and
shows both effect requests at tick 480, with the extra particle born at tick 495
at the contact point. `native-effect-samples.json` distinguishes that particle
from the scorpion's existing action effect. The copied definition is confined to
`contact-assets`; it was not published and does not establish encounter fidelity.

Published 20 files: 17 changed enemy definitions, the battle audio descriptor,
one prepared sound package for cue 112, and the shared inventory. The cue's sample
already existed and was reused. The maintained audio selection includes cue 112.
Changed identities were verified during publication, with rollback copies in
`default-before`. No models or textures were recooked; no full validation or
route sweep ran. The 30 deferred source entries remain unchanged. Guard breaks,
Over Limit, absorption and avoidance still need source comparisons for the extra
contact effect; full battle, route, arte, HUD and camera fidelity remain open.
