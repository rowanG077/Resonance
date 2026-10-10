# Battle support and validation

The battle engine uses direct state and policy for supported gameplay. Implementation
structure, internal arithmetic, and scheduling are free to change. Asset formats
remain an import boundary; decoded asset identifiers do not dictate engine design.

The current implementation includes party control and companion tactics, targeting,
normal attacks and selected artes, casting, reactions, items, conditions, equipment
effects, Over Limit, battle menus, rewards, victory performances, and field return.
All nine party characters have normal-attack and victory resources. An available
character or asset does not imply that every technique or encounter is supported.

Recent fixes separate Colette's victory state from general story progress, select
facial textures from each party model's appearance, and support the “To Field”
prompt at the Iselia exit. The overworld destination remains unimplemented and
returns to the title menu. Ordinary supported field exits remain playable.

The default error policy logs recoverable problems and continues where possible.
`--paranoid` makes those diagnostics fail the run. Partial encounters must not
silently commit invalid rewards or progress.

Battle lifecycle and AI are ordinary Rust state and policy. Normal attacks and
maintained martial techniques use prepared native events. Normal attacks own
chaining and completion timing; control bindings no longer carry separate clocks.
Combo buffering begins at action admission, including immediate input after a
chain. Ground thrust/low followups and aerial followups share native direction
rules. The normal contact table carries hit resources only; preparation resolves
them directly into the action. Mirage dashes through
actors, Destruction emits three independent rocks, and Eagle Dive strikes on
landing. Aerial admission uses the shared ground boundary, with family and equipment
rules. Special Guard and all 17 attack rows for the six supported enemy types also
use native events. Enemy attacks share swipe/combo timing; pounce recovery waits
for physical landing, and native entry initializes gravity. Spell releases use a
projectile volley with explicit shot count and spacing. Fire Ball sends three homing
shots eight ticks apart from the captured caster center; each projectile owns its
flight and survives caster interruption. Velocity and offset come from the prepared
projectile definition. Nurse and Lightning are unsupported; their dormant runtime
branches and dedicated fixtures have been removed.
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
per-action impact art and unsupported optional hit sounds do not gate gameplay.
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


Combo feedback uses one fixed panel per attacking side, showing hits and damage
for two seconds after its latest combo contact. Menu ownership pauses its lifetime;
results dismiss both panels. The display uses the dialogue font and needs no
camera, tracked actor pose, or imported combo artwork. The unused "Cancel orders"
banner and its artwork schema are removed. Target-owner labels (P1–P4) and STUN
labels project current actor geometry through existing HUD layers. They have no
simulation clock, trail, special anchor, or imported marker artwork. Battle HUD
content requires schema 26.

Weapon definitions admit their immutable layer relationships once during content
preparation. Binding checks the selected body attachment and slot uniqueness,
without rescanning weapon chains, fallback clips or rope links on equipment changes.

Party and enemy animation clips share independent admission. Tolerant loading
omits malformed optional clips; paranoid loading rejects them. The initial pose
remains required. Each actor has one native capsule for hit reception, spacing,
target bounds, and label height. Those bounds do not depend on the animation;
native action volumes supply melee contacts independently of models and equipment.
Shadows use the capsule footprint. Melee windows count active gameplay updates,
and recovery waits for their completion. Simultaneous attacks use their own hit
history. Projectiles and thrown weapons retain action power at launch.
Petrification persists only as an ailment. Entry selects a frozen idle pose; result
commits and saves do not depend on a model, animation clip, or frame. A knocked-out
member still enters in its death pose.

The command menu owns one active page. Cooking commits in the game layer, then
presents a completed result. Battle PCM starts from prepared clips when the scene
is ready, and confirmation waits for actual playback completion where required.

Cooked assets and saves use the current schemas only; saves require schema 5.
Checkpoints require
explicit technique and battle history; it does not infer missing history from
older saves. Regenerate development fixtures when changing these contracts.

## Known limits

- Additional artes, enemy variants, and complete Unison execution remain work.
- Some asset-backed integration fixtures depend on unsupported techniques or
  experimental choreography. An ignored fixture is not evidence that it passes.
- Visual differences remain in some menu, camera, and effect scenarios. Audio
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
The library must include the current save identity and field-media publications;
`refresh_battle` alone does not establish that readiness.

```sh
target/debug/examples/checkpoint_replay SAVE REPLAY.json OUTPUT "$RESONANCE_TEST_ASSETS" 640x480 --paranoid
```

Keep captures focused on changed visible behavior and compare matching gameplay
states. The simplification plan records outstanding visual and device checks;
historical recordings do not add per-batch obligations. See the
[oracle guide](../tools/oracle/README.md) for capture tools.
