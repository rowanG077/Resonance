# Battle support and validation

The current scope includes every playable party and enemy arte and all 1,000
authored enemy formations. The full catalogue audit and focused Dolphin coverage
are in progress; earlier component passes below do not establish this expanded
coverage. See `plan.md` for the current implementation and validation policy.

The expanded pass changes source and metadata schemas, but has not yet published
a new default asset snapshot or release binary. Its grouped compilation and focused
source/native comparisons remain work in progress. Current per-arte execution
receipts and unresolved bindings are tracked in
`local/battle-all-artes-20261012/coverage-current.json`; a successful recording alone
does not establish that the requested arte executed or that native visuals match.

The default library includes Hurricane Thrust and the preceding summon/Unison,
Aqua Laser, Fierce Demon Fang and Super Sonic Thrust recovery. Hurricane's hit/miss
captures pass; Fierce's blue sword ribbon is visible again with unchanged tested
mechanics. Aqua and Super Sonic retain their component passes. Eight ordinary
summons and fifteen requested Unison combinations prepare. Efreet, Sylph and
Celsius have reviewed playback; Celsius's near-caster fixture establishes visible
release and cleanup, not enemy damage.

Martel reaches both victories and its authored return with control restored.
Exbelua and Ktugach complete HP-assisted caller/return/save checks. Botta reaches
victory and the overworld after the world metadata and input-acknowledgement
repairs. A capture-only field-shadow diagnostic then asserted on the world owner;
that scene guard is now fixed. Its final screenshot/save retry remains unrun.

The last small source changes restore all measured summon announcements,
Celsius's repeated ice sounds, and Unison 302–304 afterimages using the authored
thrust event. They pass focused release compilation and are newer than the grouped
playback binaries. Timing is deliberately approximate within measured bounds;
there is no exact callback/pixel-parity claim.

Parallel work remains enabled. Artifacts use disk, workers share a 40 GiB total
budget, and available RAM has a 12 GiB reserve. There is no separate shared-memory
cutoff. The eleven-job batch finished in 28.54 seconds, with 6.85 GiB peak shared
memory and 37.21 GiB minimum available RAM. Five focused checks pass. See
`local/battle-aqua-fierce-20261012/followup/REPORT.md` for identities, retained
failures and bounded acceptance. Historical limitations below describe their
respective batches. No full recook or full validation ran.

The battle engine uses direct state and policy for supported gameplay. Implementation
structure, internal arithmetic, and scheduling are free to change. Asset formats
remain an import boundary; decoded asset identifiers do not dictate engine design.

The current implementation includes party control and companion tactics, targeting,
normal attacks and selected artes, casting, reactions, items, conditions, equipment
effects, Over Limit, battle menus, rewards, victory performances, and field return.
All nine party characters have normal-attack and victory resources. An available
character or asset does not imply that every technique or encounter is supported.

Battle cooking now enumerates the complete scene and audio catalogues instead of
following the implemented-action lists. All 89 stored magic/skill packages convert,
and the catalogue binds existing audio without decoding PCM again. Encounters
prepare once and load only their selected audio resources. Cooking also retains
all enemy projectile records and independent artwork. Seven sounds with missing
source macros and unsupported native effect/action behavior remain explicit;
successful asset conversion does not establish gameplay support. The current
cooked catalogue is installed in `local/all-assets`; publication and focused
validation evidence is in `local/battle-eager-cook-20261012/REPORT.md`.

The next scenes-only refresh is installed too. Native Spread, Gravity Well and
Heal, active-scene camera bounds, particle birth acceleration, trail size
overrides and additional contact policies are restored. Twelve distinct focused
tests and three parallel native captures pass. The catalogue retains 184 explicit
unsupported nodes; the accepted components do not establish whole-battle parity.
See `local/battle-stored-family-20261012/REPORT.md` for details and limits.

The following wind/earth batch restores Air Thrust, Cyclone, Air Blade and Ground
Dasher, including captured scene origins and corrected degree-based headings.
ForceReaction, trail radius velocity and model culling repairs reduce explicit
unsupported metadata to 127. Its 51-file delta is installed in the default library.
Seven focused tests and four parallel native component captures pass, with 24
reviewed images and no diagnostics or unprepared reads. Camera/tint parity,
additional spell families, eight ordinary summons and Unison execution remain
work. `local/battle-wind-earth-20261012/REPORT.md` records the bounded results,
original 30 resource audit and concrete next steps. No full recook or full
validation was run.

The phase batch is installed: Double Demon Fang for Lloyd/Zelos/Kratos, Photon,
finisher-grade contacts and all 19 Unison combination records. Nine focused tests
and four parallel native captures pass their bounded checks; 35 files (12.6 MB)
were published, reducing unsupported scene nodes to 96. Photon lighting parity
and learning-driven cast replacement remain open. The Unison owner is tested
but lacks host/opener/combination integration; it is not yet playable through
normal input. See `local/battle-phase-recovery-20261012/REPORT.md`.

Physical cooking must be independent of gameplay support. All installed party
costume bodies and battle animation banks already have generic cooked outputs.
The remaining eager-cook repairs remove the arena presentation gate and publish
declared palette windows independently of effect-program support. The default
library now includes 1,070 additional images; both affected tests pass, all 89
scenes convert, and 6,702 existing palette mappings are preserved. The scenes-only
refresh took 10.31 seconds. Computed out-of-declared-storage palette references
in rejected programs remain a decoding limit. Runtime action/event bindings remain
partial and must not be described as complete merely because their assets are
present. See `local/battle-eager-remaining-20261012/REPORT.md`.

The latest metadata update is installed: 24 files (6.0 MB), reusing all 89 scene
packages in 0.55 seconds. It retains all 45 participant tracks across the 18
physical Unison scenes, callback-selectable contact rules, 253 technique
placement/native-ID records, selection artwork bindings and particle clocks.
Twelve focused tests and one two-frame ordinary rendering check pass, with zero
diagnostics or unprepared reads. Shared Unison/summon runtime components have
improved, but host/opener geometry and family registrations remain incomplete;
these features are not yet playable through normal preparation. No full recook
or full validation ran. See `local/battle-unison-summon-20261012/REPORT.md`.

Recent fixes separate Colette's victory state from general story progress, select
facial textures from each party model's appearance, and support the “To Field”
prompt at the Iselia exit. The overworld handoff uses the shared scene owner;
the current world → Iselia → world loading probe passes.

Party entry and controlled-camera tracking have been checked against registered
Dolphin observations. Three parallel native recordings through combat tick 250
complete without diagnostics. Their random streams are not aligned with Dolphin,
so whole-combat visual differences remain diagnostic; they do not establish
behavioral parity. See [the current audit](battle-validation-2026-10-11.md).

The [resource recovery report](battle-resources-2026-10-12.md) records the latest
publication: Tiger Blade, First Aid, authored normal continuations and weapon
flight, partial-body playback, signed weapon UV scrolling, 85 arenas, and all
45 ordinary victory timelines. All nine selected native checks pass. Twenty-eight
source rows and whole-route fidelity remain open. The
[early-route repair report](battle-route-2026-10-11.md) records the preceding
sixteen-worker investigation, focused checks and publication status. It adds
authored party actions, enemy follow-ups, pollen effects, three early arenas,
Eruption, entry holds, animated shadow bounds and authored HUD artwork. Successful
preparation does not establish whole-route fidelity. The
[preceding batch](battle-parallel-2026-10-11.md) remains
historical evidence for its own build.

The default error policy logs recoverable problems and continues where possible.
`--paranoid` makes those diagnostics fail the run. Partial encounters must not
silently commit invalid rewards or progress.

Battle lifecycle and AI are ordinary Rust state and policy. Normal attacks and
maintained martial techniques use prepared native events. Normal attacks own
chaining and completion timing; control bindings no longer carry separate clocks.
Combo buffering begins at action admission, including immediate input after a
chain. Ground thrust/low followups and aerial followups share native direction
rules. Normal contacts now use authored weapon attachments, power, reaction and
timing. Colette's thrown weapon uses its three authored flight profiles; complete
trajectory/visual parity remains unverified. Mirage dashes through
actors, Destruction emits three independent rocks, and Eagle Dive strikes on
landing. Aerial admission uses the shared ground boundary, with family and equipment
rules. Special Guard uses native events. Enemy attacks now come from decoded asset timelines:
clip changes, contact windows, recovery, movement, sounds, projectiles and hit rules
replace the six-enemy hand-written list. Melee contacts bind body/weapon bones and
sample the current animation pose before damage. Collision and drawing share one
model player. Unrecognized commands and action features still reject preparation.
Catalogue coverage does not certify complete encounters; nested projectile and
decision-policy limitations remain explicit. Action-owned
effects and looping attack poses are supported. Cooking includes animation-blend
holds in elapsed attack times; the runtime retains one action clock. Both Golem
attacks now match recorded uninterrupted clip/frame sequences and recovery timing.
Action emitters pause during their owner's hit-stop while existing particles
continue. Completed frames record actor clock holds for model and effect playback.
Enemy attack voices and poison contacts use the shared audio and condition systems.
Hemisphere effects share the sphere mesh's upper half, including camera-facing
orientation and rim-to-tip gradients. The scorpion encounter now prepares and
renders its attack effect; whole-encounter visual parity remains unverified.
Enemy guard
windows and several decision features remain incomplete. Spell releases use
prepared timed projectiles with explicit origins and scene lifetimes. Fire Ball sends three homing
shots eight ticks apart from the captured caster center; each projectile owns its
flight and survives caster interruption. Velocity and offset come from the prepared
projectile definition. Eruption uses its own scene, target-ground pulses and
transition hold. Its caster prelude, stage dimming and retirement have focused
coverage. Active-scene camera bounds now hold and restore through the common
camera channel; complete visual parity remains unmeasured.
Lightning and Stone Blast now use the resident effect bank and authored damage
rules. Their ground position is sampled one update after release and retained
for the remaining pulses. First Aid restores a selected living ally using the
saved native recipe; its source pulse timing/amount remain unconfirmed. Nurse
healing and scene dimming are implemented; full visual parity remains unmeasured.
Game preparation builds native action definitions directly.
Casting quotes and payment share the action's TP cost.
The live cast owns held-spell state; activity and HUD read that observation without
a separate casting phase or HUD latch. Chant motion, charge growth, and pulses
share the action clock, with the first pulse at entry. Casting memory and stored spells use the
prepared action ID. Action dispatch matches native variants directly. Automatic
retargeting preserves an owned action's target through recovery.
Actors carry no cached activity: live queries derive it from task, guard, and
availability, and completed frames publish that observation for presentation.
Casting cancellation and spell storage share the action's completion transition.
Each action owns one timeline; released spells and projectiles own their remaining
lifetimes. Conditions own their magnitude and expiry, while equipment grants
remain separate. Changing Strategy changes tactics without rebuilding equipment.
Weapon swaps replace trail samples while keeping the active attack's trail timing.
Contact feedback and particle palettes prepare the fixed element set, including
Quartz enchantments applied during battle. Contacts use one common impact policy;
enemy action rows with unresolved impact art or hit sounds reject preparation.
Item feedback follows a committed release event, with one user voice and an
optional scan-discovery override. Recovery flashes belong to the scene. Hourglass
keeps its opponent freeze and ordinary item feedback without a fullscreen flash.
Breakfall publishes semantic recovery poses; the model owns their clips and the
scene owns its sound and artwork. Gameplay retains only eligibility and native
turning, without a separate recovery resource bundle. Shared locomotion, casting,
entry and item clip identities resolve through named roles during preparation.
Taunting uses its own optional pose without borrowing the item animation.
Entry speech is selected during preparation and played once by the scene. Victory
choices belong to the results owner and use one random sample per selection.
Simulation keeps only gameplay randomness; cosmetic choices cannot advance it.
Party/enemy capacities and projectile actor storage share the battle limits.
Jump, backstep and taunt use the model's common pose set. Scene feedback resolves
EX outcomes, takeoff, backstep, knockdown and Unison readiness; actor setup no
longer carries their cosmetic definitions. Special Guard retains only an action
ID and uses the existing action event for its artwork.
Technique-command speech resolves in scene feedback after an accepted queue event;
it is independent of gameplay setup and assist-command validation.

Results grant every newly eligible title once using final character state and
combat statistics. Simulation stores no title IDs or ordered award attempts.
Normal-attack variety belongs to the active combo, and every distinct variant
counts toward the recorded maximum when chaining into a technique. Item titles
use per-actor usage counts; item loans carry inventory and persistence references
without character identities. Equipment titles check the recipient's own gear.

Live and offline reverb share finite tail completion: two configured decay periods,
100 ms for queued input, then a 100 ms fade. Expiry clears the effect history and
idle effects skip filter work. Field-audio completion uses this same policy without
filter-network scans or a separate drain-budget calculation.


Combo feedback uses the authored battle artwork and gradients. Completed damage
travels for 20 updates, holds for 60 and fades for 30. Menu ownership pauses its
lifetime; results dismiss both panels. Target and stun markers use the cooked
atlas artwork, projected above current actor geometry. Party portraits and vitals
use the compact observed layout; enemy icons occupy the bottom-right rows.
Battle HUD content requires schema 39, including authored result gradients.
Damage numbers use imported sizes and
gradients, four independent hit anchors, contact expansion and a drifting fade.
Recovery numbers retain their horizontal arc and italic overlap. These repairs
do not establish fidelity: action banners, combo panels and several HUD animations
still differ from the registered Dolphin captures.

Weapon definitions admit their immutable layer relationships once during content
preparation. Binding checks the selected body attachment and slot uniqueness,
without rescanning weapon chains, fallback clips or rope links on equipment changes.

Party and enemy animation clips share independent admission. Tolerant loading
omits malformed optional clips; paranoid loading rejects them. The initial pose
remains required. Each actor has one native capsule for hit reception,
target bounds, and label height. Those reception bounds do not depend on animation.
Approach and Unison placement instead measure separation between posed body spheres. Enemy attack volumes now follow
animated attachment bones. Party normals, Sonic Thrust, Sword Rain and Tiger
Blade use their authored contact definitions; remaining artes still need recovery.
Shadows use animated mesh bounds. Melee windows count active gameplay updates,
and recovery waits for their completion. Simultaneous attacks use their own hit
history. Projectiles and thrown weapons retain action power at launch.
Petrification persists only as an ailment. Entry selects a frozen idle pose; result
commits and saves do not depend on a model, animation clip, or frame. A knocked-out
member still enters in its death pose.

The command menu owns one active page. Cooking commits in the game layer, then
presents a completed result. Battle PCM starts from prepared clips when the scene
is ready, and confirmation waits for actual playback completion where required.

Cooked assets and saves use the current schemas only; saves require format version 10. Asset edits do not invalidate saves.
Checkpoints require
explicit technique and battle history; it does not infer missing history from
older saves. Regenerate development fixtures when changing these contracts.

## Known limits

- The branch is not yet playable with faithful battles through Triet Ruins.
  All 15 arenas used by the 82 retained route fixtures are now prepared in the
  current candidate. Enemy policies, action commands, secondary body animation,
  projectile behavior and selected effect programs still block route encounters.
  See the [current repair report](battle-route-2026-10-11.md) for publication and
  runtime results. Historical sweep counts in the
  [earlier audit](battle-validation-2026-10-11.md) describe their captured builds;
  they are not current acceptance results.
- Additional artes, enemy variants, and complete Unison execution remain work.
- Some asset-backed integration fixtures depend on unsupported techniques or
  experimental choreography. An ignored fixture is not evidence that it passes.
- Matched Dolphin captures show substantial battle HUD, camera, actor-placement,
  and scenery-visibility differences; successful loading does not establish
  battle fidelity. Arena descriptors and their verified dependencies are now in
  the default library; their publication does not certify encounter behavior. Audio
  tuning and physical multiplayer/device behavior remain manual review work.
  The user accepted physical controller testing as a follow-up to simplification;
  check separate actor control, menu holds, reconnect/reassignment and rumble.
- Fast prepared-asset loading is intentional. Loading-related timing differences
  are recorded separately from gameplay behavior.

Historical reports under ignored `local/` describe the builds they captured.
Their test counts and acceptance claims do not certify the current working tree.
Validate each changed build, including the victory, texture, and exit fixes.

## Validation

The current work list, validation evidence and batch policy live in
[battle-simplification.md](../battle-simplification.md). Run affected checks once
after a coherent change, rerunning only failures or subsequent relevant edits.
The following broad commands are milestone checks, not a per-batch checklist.

```sh
cargo fmt --all -- --check
cargo test --offline --locked -j1 -p resonance-battle --lib
cargo test --offline --locked -j1 -p resonance-game --lib
cargo clippy --offline --locked -j1 -p resonance-battle -p resonance-game --all-targets -- -D warnings
```

Use representative prepared assets for importer and loading changes. Run selected
asset-backed tests explicitly; report unsupported or failing cases individually.
Do not treat a larger raw test count as stronger coverage.

With a current cooked library, run affected host ownership and field handoff checks
explicitly. Host tests use direct current checkpoints and a completed-result handoff;
the result and event suites cover reward commits and suspended script execution.
Missing assets fail the tests.

```sh
export RESONANCE_TEST_ASSETS="$PWD/local/battle-recovery-roles-assets"
cargo test --offline -p resonance-presentation --lib battle::fatal_tests:: -- --ignored --test-threads=1
cargo test --offline -p resonance-events --test battle
cargo test --offline -p resonance-game --lib battle::results::setup_tests:: -- --ignored
```

The host scenarios cover fatal quit, load success/failure, failed-music preservation,
and battle page/selector failure recovery. The fixture loads selected images normally
and substitutes GPU completion; it establishes ownership and input behavior, not
rendering fidelity. Current results belong in the simplification plan.

For presentation changes, capture ordinary inputs through the supported route.
The library must include the required named field and media resources;
`refresh_battle` alone does not establish that readiness.

```sh
target/debug/examples/checkpoint_replay SAVE REPLAY.json OUTPUT "$RESONANCE_TEST_ASSETS" 640x480 --paranoid
```

Keep captures focused on changed visible behavior and compare matching gameplay
states. The simplification plan records outstanding visual and device checks;
historical recordings do not add per-batch obligations. See the
[oracle guide](../tools/oracle/README.md) for capture tools.
