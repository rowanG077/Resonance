# Battle fidelity plan

## Proposed effect storage and preparation redesign — 2026-10-14

Status: implemented and accepted for this storage/preparation scope. One shared
live render pool replaces per-effect hidden copies; emitter, visitation,
publication and procedural geometry buffers retain capacity. All 47 focused
checks pass. Resource preparation fell from 3.016 s to 0.540 s; first-use pool
growth stayed below 2.18 ms per nearby frame, and repeat use allocated no new
render slots. The overlapping-effect replay preserves effect rendering, pause,
cleanup and field return, with no diagnostics or unprepared reads.

Peak memory did not decrease (3.65 to 3.74 GiB). Entry still has a measured
61 ms warmup interval. A small enemy-number rendering instability exists in
both baseline and candidate and remains a separate follow-up. Full evidence,
limits and receipts: [effect storage report](local/effect-pool-20261014/REPORT.md).
The design below records the implemented scope.

### Evidence and objective

`crates/game/src/battle/encounter/render.rs::effect_model` assigns the global
`MAX_PARTICLES` limit (256) to **each** model. `battle_view.rs::load_with_diagnostics`
then builds that many instances, and `View::prepare` clones and binds their
scenes before entry can finish. Meshes and textures are shared, but scene
objects and binding work multiply by the number of model resources. The sprite
renderer already uses one global particle draw pool; it has a different problem.

The accepted entry measurement is 3.10 seconds, with a 19.5 ms worst frame during
the first 2.2 seconds and a late 57.8 ms frame. Data loading and initial adoption
finish about 0.25 seconds after entry begins. The existing profile identifies
scene cloning; it does not measure allocator contention or prove that malloc
dominates. See [entry evidence](local/findings-20261013-followup/entry-stall/REPORT.md).

Reduce actual scene construction and retained objects, not just the batch size.
Target preparation finishing within the existing two-second shatter, without
moving the stall to the first spell. This is an acceptance target, not a promised
timing result. Keep all required cooked assets available before combat.

### One immutable template per model

Read each loaded `WorldAsset.world` once, following the existing direct asset
inspection in `field_warm.rs`. Compile a battle-local `EffectModelTemplate`:

- Shared mesh, texture and inverse-bind-pose handles.
- Draw recipes: material, pass order, sampled bone and static transform suffix.
- Skin joint bindings expressed as template indices, not live scene entities.
- Validated material recipes and reachable blend/depth/lighting variants.

Keep only transforms required by draws and skinning. Battle model poses already
supply the animation; preserve the exact affine transform calculation, including
nonuniform scale and singular transforms. No per-particle scene hierarchy,
reflection cloning, bone-name parsing, animation controller or metadata copy.
Honor `RenderModel.suppressed_nodes` for draws while retaining any joints and
descendants needed by visible geometry; flattening must not restore attachments
that are intentionally hidden.

Warm representative draws for distinct mesh layouts and material/pipeline keys.
Validate/readiness-check unique template assets, rather than walking hidden
copies. Preserve the submitted-draw/GPU readiness fence and fail visibly on
invalid templates. Warming one copy prepares resources; it does not reserve
hundreds of live instances. Actors and scenery retain their existing preparation.
Pipeline-key coverage and resource residency are separate: two meshes can share
a pipeline key while only one is uploaded. Ensure every unique required mesh,
image and material binding is ready, even when pipeline warmup is deduplicated.

### One reusable pool for all model particles

Reserve capacity for at most `MAX_PARTICLES` small logical records, shared across
all effect models. Each active record associates a monotonic `ParticleId` with
its template and claimed draw/joint slots. Use ordinary owned `Vec` storage and
free lists, without an allocator dependency or a general pool framework.

Maintain separate global free lists for minimal draw objects and skin-joint
objects. A draw slot owns one reusable entity and mutable material handle; a
joint slot owns the transform entity required by the existing skinning path.
Reuse these objects across resources, not just across casts of the same model.
Only create additional slots when simultaneous demand exceeds the retained pool.
Return expired particles' slots before claiming new ones in the same update.
An active particle changing model must also release/rebind its slots.

On binding, reset the complete mesh, skin, material, tint, UV, lighting, order
and visibility state. Mutable appearance belongs to the live slot, never the
shared template. Remap skin joints and retain outline/multipart draws. Remove
stale skin bindings when a slot becomes unskinned. Transparent submission order
continues to follow particle birth order, independently of free-list order.

With P live model particles, draws D(i) and required joint transforms J(i),
the required active render objects scale with sum(D(i) + J(i)), plus templates.
Retained draw and joint pools each track their own peak concurrent demand.
`MAX_PARTICLES * max(template draw/joint count)` is an accounting upper bound,
**not** an upfront allocation. Do not replace the current problem with 256
fully populated slots sized for the largest model. Dormant slots stay hidden and
outside active update loops. Release their prior resource bindings when returned
so they do not pin unrelated assets.

This pool is battle-owned initially. Despawn entities and drop handles on exit,
cancel or failure; no unbounded cache across encounters. Reuse within a battle
is sufficient for the first implementation and keeps ownership simple.

New high-water marks still allocate a few component bundles/material assets and
may require renderer binding work. This design does not claim allocation-free
first use. It removes scene cloning, asset I/O and missing pipeline preparation
from that path. Measure first-use bursts before adding any predictive reservation.
Do not introduce arbitrary per-effect caps or silently drop additional particles.

Publish new slots with their already computed global poses and include those
poses in `held_pose`. Flush their commands before visibility/extraction in the
existing `PublishBattle` ordering. Do not query newly queued entities as though
they already exist, and do not add a frame of effect latency. Pause/menu capture
must retain the new slots just as it retains existing model poses.

### Allocation reuse and the arena decision

Use reusable storage where the code demonstrably discards capacity:

- `effect.rs::advance`: retain current/spare emitter vectors instead of consuming
  and discarding the allocation via `mem::take` every update. Size an emitter's
  nested buffers from its definition, not the global particle maximum.
- `particle.rs::advance_particles_filtered`: retain its particle visit queue.
  Preserve parent/child visitation, including births appended during an update.
- `particle.rs::frames` and `battle.rs`: fill the existing publication vector
  instead of replacing it with a newly allocated vector every tick.
- `battle_view/effects_geometry.rs`: fill/reuse mesh attribute vectors instead
  of constructing replacement meshes that discard their previous CPU buffers.
  Required GPU uploads still happen.

Start with these changes alongside the render pool. Retain the simulation's
particle map initially to avoid mixing a storage rewrite with rendering changes.
If a small allocation profile still identifies map churn as significant, replace
it with a birth-ordered `Vec<Particle>` reserved to `MAX_PARTICLES`, monotonic IDs,
binary-search lookup and stable removal. Do not use `swap_remove` or turn a reused
storage index into a public particle ID. This is simpler than adding generations
and a second ordering structure at the current bounded particle count.

A bump arena is unsuitable as the first change: particles can outlive emitters,
they die independently, and their `Arc`, `Vec` and asset handles need normal
destruction. Published poses may remain in use after a simulation update. Resetting
an arena at either boundary would require a broader ownership redesign. It also
cannot eliminate allocations owned internally by Bevy or GPU resource creation.
The reusable vectors and free lists provide the relevant storage reuse safely.

Leave the global allocator, field renderer and general battle snapshots alone.
Model sampling currently creates new pose arrays (`model/effect.rs::sample`);
consider retained/double-buffered pose storage only after measuring its remaining
cost and preserving snapshot ownership. No repository-wide allocator migration
is justified by the current evidence.

### Implementation and focused acceptance

1. Add template compilation and the shared model-particle pool in a small
   `battle_view/model_particles.rs` module. Route particle
   models through it; remove their per-resource capacity, duplicate scene binding,
   shared/copy-on-write material bookkeeping and dormant-pool scans. Keep actor
   instance counts and existing material/shader/draw-order contracts.
2. Reuse emitter, visit, publication and sprite geometry buffers. Keep simulation
   behavior and snapshot ownership intact. Child attachments, budget-rejected
   births, RNG consumption and authored birth indices must be unchanged.
3. Extend the relevant existing checks for simultaneous same-model particles
   with independent poses/tints, slot reuse across different skins/materials,
   first-frame publication and cleanup. Include child attachments and budget
   rejection using existing effect checks; do not build a parallel test framework.
4. Run the same isolated entry benchmark, then one short overlapping-effect replay
   covering first use, repeat use, pause/resume and return to the field. Record
   template count, active/retained draw and joint counts, new-slot allocations,
   preparation time, frame tails and memory. Confirm slot counts plateau on repeat
   use and resources are retired on exit. Compare representative captures.

Accept only if entry improves without hiding missing effects, reducing the
particle budget, delaying births or moving the pause to first use. Report the
measured memory change separately from timing. Use the existing single 56 GiB
aggregate guard for later heavy jobs; no concurrent profiling during timing runs.
No recook, full suite, catalogue sweep or new Dolphin run is required for this
storage change. Build and runtime checks remain limited to the paths above.

## Latest battle playtest findings — 2026-10-13

Scope is the 11 new battle reports in ignored `findings.md`; older field,
dungeon and music reports were removed and excluded from this pass.

Implemented stable player/target camera tracking, Colette approach corrections,
full-viewport fades and Items page, the boss animation-loop unit correction,
authored battle HUD/activation graphics, portrait gauge layering, and armor/hurt
timing corrections. Red warning outlines and the approximately ten-second
Kratos rescue gate are intentional; their exact conditions are documented.
The boss's corrected tail still circles the mace intentionally.

All focused checks and reviewed captures are recorded under
[the current investigation](local/findings-20261013/README.md). UI and Over Limit
metadata are installed in default assets with rollback copies. No full recook,
full suite, commits or history rewrite was needed. Final representative
comparisons all pass against those installed assets. The scoped
[acceptance receipt](local/findings-20261013/acceptance.json) pins the results.
Historical work follows below.

## Earlier completed fidelity work

Earlier scope: correct Ray Thrust effects, the battle command menu and the falling
screen-fragment transition. Add a compact Rust catalogue test that runs real
actions against a recipient and compares normalized Dolphin observations, using
shared fixture policies instead of manually authored cases. Offensive actions must
make attributed contact; support actions must produce their actual benefit.
Preparation, missing observations and unsupported setups remain explicit failures.
Source execution alone is not equivalence. Static inspection supplies effect
details; GPU draw-command capture remains cancelled.

Current findings (2026-10-13):

- Ray Thrust now retains its authored birth effect and projectile shadow. Removing
  inherited normal-attack braking restores the measured 35.25-unit movement.
  The focused gameplay test passes; registered captures show the ring, burst,
  contact and cleanup. Exact impact-frame pixel parity is not established.
- The entry transition retains the outgoing field and reveals battle through
  separating, tumbling, fading fragments. All five transition captures pass the
  scoped review, with no diagnostics or unprepared reads.
- The six command emblems, selected ribbon and cursor are restored. Missing
  submenu text was traced to its first publication having no initialized world
  depth. The minimal transform fix passes the existing menu submission check
  and all three rebuilt captures: opening strip, learned Tech page, and return
  after cancel. Text, descriptions and TP cost render with no diagnostics or
  unprepared reads. Other full-screen submenus were not separately replayed.
- Ray Thrust passes the complete 111-frame Rust/Dolphin comparison, including
  actor movement, contact, selected poses, deterministic effects and recovery.
  The shared projectile recoil, damage arithmetic, body separation, readiness
  and hurt feedback corrections pass their focused checks. The reviewed profile
  metadata is installed: 37 recoil fields and the named hit-shake trait, with
  no artwork recook. Installation hashes and rollback copies are retained in
  [profile publication](local/battle-capture-work/arte-equivalence/profile-shake01/installed.json).
  The enemy contact representative also passes after replacing approximate
  reception bounds with authored body markers and restoring the common impact
  overlay on elemental hits. The latest five passing comparisons are retained in
  [attempt14](local/battle-capture-work/arte-equivalence/native/attempt14/report.json).
  No catalogue-wide fidelity acceptance is claimed.

The comparison now uses one generated case list and one canonical event stream.
Internal interpreter ages and empty scheduler allocations are diagnostics;
observable contact, effects, damage bounds, movement and cleanup remain required.
Five representative source jobs ran together. Four focused retries corrected
camera readiness, movie rebinding and saved OverLimit in the fixtures. Ray, Sonic
Thrust and Fire Ball produce actual contacts; First Aid produces healing. The
enemy fixture also produces attributed damage. In attempt14, all five complete
comparisons pass: Ray Thrust, Sonic Thrust, Fire Ball, First Aid and the enemy
representative. Fire Ball's three trajectories match after preserving the target
center visible during the caster's update. Its second contact now matches by
combining the current skeletal pose with the existing placement snapshot used
for reception. Captured actors retain their current attachment placement.
The native fixture preserves observed headings,
learned spells and animation loop bounds. Casting effects now receive their
initial same-update visit. Critical-hit feedback is validated against each run's
independently established damage branch before random outcomes may differ.
Genis's chant stages, cast-pose recovery and Fire Ball's launch data are restored.
The two affected metadata files and one preload hash are installed in the default
assets, with [rollback copies](local/battle-capture-work/arte-equivalence/chant-fire-ball01/installed.json).
The early pose sample fixes Sonic separation while secondary/carried animation
remains at final placement. All targeted batch177 regressions pass, including
body separation, attached arms, secondary chains, feedback and comparator checks.
The final batch180 also passes the three focused reception/capture checks and
the five paired comparisons using the default assets. Each selected check ran;
none passed through an empty filter. Runner success alone is not equivalence.

The final [acceptance receipt](local/battle-capture-work/arte-equivalence/acceptance-20261013.json)
pins the representative report, focused checks, visual reviews and asset
publication records. This closes the scoped fidelity fixes and test integration;
it does not establish catalogue-wide or universal visual equivalence.

The source runner stops when the observed action and effects finish, instead of
recording the entire one-minute safety budget. Incomplete effect cleanup fails.
The 18 special-context bindings and 32 nonhealing-support bindings have explicit
adapter limitations; they are not accepted equivalence results.

The body matrix captured at a VI can belong to an earlier display publication.
Its publication time is unobserved, so it remains diagnostic rather than forcing
a rendering delay into the runtime. The comparison keeps body clip/frame, actual
actor movement, projectiles and deterministic effect-model geometry; body-bone
geometry, exact pixels/audio and cosmetic random sequences are outside its scope.

Receipts: [Ray capture](local/battle-capture-work/ray-thrust-fidelity/integration25/attempt02/semantic-review.json),
[entry fragments](local/battle-capture-work/static-gaps-20261013/entry-fragments/native01/semantic-review.json),
[menu review](local/battle-capture-work/battle-menu-fidelity/semantic-review.json),
and [focused test](local/battle-capture-work/all-artes-batch152/ray-thrust-game-focused-runner.log).
The four reviewed UI asset changes are installed in the default `local/all-assets`
directory, keeping the new descriptor and runtime schema consistent. Installed
hashes and rollback copies are recorded in
[installed.json](local/battle-capture-work/battle-menu-fidelity/ui-publication/installed.json).
The submenu fix has visual acceptance; no full recook ran.

The installed recovery below is a historical baseline, not fidelity acceptance.

Integration24 is installed in the default `local/all-assets` directory (2026-10-13).
Its 203 changed metadata files total 10.4 MB. Installed hashes and rollback copies
match the tested candidate. Integration23 failed preparation and was never installed.
No full recook or full test suite ran.

The Stage4 recovery and original deferred-resource implementation work is complete
within the documented scope. The original 30 deferred rows represent 15 resources
on two discs. Native result handling and unused alternate modules are intentional
replacements/exclusions; the old “22 deferred rows” status is superseded.

Current evidence: [acceptance](local/battle-all-artes-20261012/integration24/acceptance.json)
and [catalogue census](local/battle-all-artes-20261012/integration24/census.json).
The full previous plan is preserved in
[plan-before.md](local/battle-all-artes-20261012/integration24/plan-before.md).
Historical reports retain their original failures and limitations.

## Implemented

- Story-flag action eligibility, health/time-based retreat, nominal HP drain,
  effective-hit waits and recovery flashes at full HP/TP.
- Timed collision bypass using the existing actor timer; independent projectile
  origins/directions, conditional rebound and periodic effects.
- Inert-effect handling and compatible bone-group/ring attachments using existing
  runtime mechanisms. The unnecessary projectile whitelist is removed.
- Scene-sampling rings with authored depth settings; persistent Colette wings and
  their missing palette-cycled sparkles.
- Previously accepted battle entry, actions, spells, summons, Unison, results and
  field-return behavior remain covered by retained receipts.

## Validation and remaining limits

All 29 distinct focused checks pass, including six follow-up checks; unchanged
checks and captures were reused. The latest native checks show the wing sparkles
through recovered idle, visible scene distortion with intact UI and effect cleanup,
and the story-flag action's three releases and recovery. Failed/mistimed fixtures
are preserved separately and receive no visual acceptance.

All 1,210 enemy action rows have execution metadata with no active row or selected
effect gates. The bounded census finds no metadata blocker for 999 formations.
All 221 required party artes / 249 bindings retain source execution evidence.
These inventories and selected captures do not establish universal visual parity.

- Formation90 requests absent enemy191 variant3; no fallback was invented.
- Enemy source execution is 1,202/1,210: two remaining rows have contradictory
  eligibility; six were preempted by retreat in tested contexts. They are not
  counted as executed or universally unreachable.
- Unselected declarations remain explicit. Exact camera/damage parity in every
  context, cosmetic random sequences, wing-layer phase and direct depth-occlusion
  comparisons beyond the captured ring layout remain unmeasured.
- Physical controller testing remains the agreed manual follow-up.

## Working constraints

Keep changes within battle or necessary integration with main. Use static inspection
when it resolves behavior; keep decomp references out of production and tests.
GPU draw-command capture is cancelled. Do not repeat catalogue sweeps, full recooks
or the full suite without a concrete new reason. No commits or history rewrites
are part of this recovery.

Use up to 16 parallel agents/Dolphin slots for independent work, under one 56 GiB
hard limit, 52 GiB high limit and no swap. Admit below 50 GiB owned memory with at
least 8 GiB available; stop below 6 GiB available or 10 GiB disk reserve. Keep outputs
on disk and shared memory for live IPC. The final batch180 peaked at 4.51 GiB
owned memory, kept at least 44.05 GiB available and recorded no memory-limit/OOM
events. Its global shared-memory peak was 2.70 GiB; no owned shared-memory use
was recorded during this retained-trace comparison.
Keep importer and runtime build commands separate to reuse their dependency caches.
