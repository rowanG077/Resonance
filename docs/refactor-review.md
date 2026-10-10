# Native simplification review and acceptance

This is an archived review. The active work list and validation policy are in
[battle-simplification.md](../battle-simplification.md). The unchecked findings
below describe the review snapshot, not current completion status.

Review against local main `9bfd330`, including untracked implementation. The
read-only review measured 151,312 Rust lines on main and 333,942 in the working
tree. Count implementation, tests and deleted capture data separately. The new
implementation baseline is archived in local/refactor/2026-10-06-pass36.

Do not consult or reference decompiled source. Binary conversion belongs at
import boundaries; native simulation and presentation need no old-code or
old-schema compatibility. Default diagnostics warn and continue coherently;
`--paranoid` stops on errors. Do not build a new general-purpose framework to
replace a small mechanism.

## Findings to fix

- [ ] **1. Complete interrupted migrations and restore compilation.** Update
  the lockfile for the removed Oracle dependency. Complete native death API,
  deleted-script test and caption consumer migrations without restoring old
  APIs. Finish native fall/rest/revival behavior and discard controller fixtures.
- [ ] **2. Cosmetics cannot gate gameplay or rewards.** Apply condition clocks,
  poison, regeneration, Self Cure, idle recovery, kill recovery and every combo
  contribution independently of optional tint/notice feedback. Do not require
  unused Self Cure captions on encounter entry. Cosmetic diagnostics must not
  invalidate result commitment. Replace frozen-state expectations with actual
  battle-step assertions; preserve paranoid errors and real transaction guards.
- [ ] **3. Keep prepared asset reads verified.** Once a Files snapshot is
  installed, absent/rejected data must use the same diagnosed failure path;
  optional consumers omit/substitute. Raw disk fallback is only for startup
  before snapshot installation. Admit late dependencies through verification.
  Cover whole-inventory failure, not only a rejected individual file. Avoid
  carrying cached fields from a different publication into a replacement session.
- [ ] **4. Prepare selected equipment and available swap capacity.** Require
  equipped resources, omit unavailable spare/reserve options with diagnostics,
  and reject unavailable swaps atomically. Local weapon playback needs only its
  selected clip. Remove speculative spare weapon/shield compatibility searches;
  validate actual equipped and selected replacement combinations.
- [ ] **5. Preserve owner-linked animation on equipment replacement.** Bind
  changed owner-linked weapons to current body playback. Keep unchanged layers
  and independent local loops. Extend the existing replacement behavior test.
- [ ] **6. Declare actual cooking dependencies.** Shared presentation depends
  only on consumed package outputs (including selected skit voices), not every
  independent package. Preserve aggregate failures and successful unrelated work.
- [ ] **7. Retain selected cursor ownership.** Keep a strong handle while its
  image is pending and share it between readiness and rendering. Exercise actual
  input advancement through pending, available and tolerant failed states;
  preserve dialogue acknowledgment.
- [ ] **8. Validate selected menu controls.** Frame-only drawing requires only
  drawn pieces; heading/cursor/decorations are independent. Decode selected
  caption entries/control groups independently, including malformed nested
  data. Complete publication remains strict. Remove packed-index coupling if a
  smaller native frame recipe replaces it. Preserve draw order and sparse IDs.
- [ ] **9. Use actual selected cooking-popup admission.** One drawable-result
  preparation controls candidate commitment, including glyph availability.
  Remove weaker duplicate preflight. Preserve inventory/RNG atomicity and
  once-only commitment without unrelated page resources gating the transaction.
- [ ] **10. Remove duplicate pose representation and passes.** Sample and blend
  field poses locally, publish once and mark only successfully sampled rigs.
  Remove unused Skeleton::blend/Pose.local and identity placeholders for affine
  poses; compose globals from local matrices without redundant allocations.
- [ ] **11. Preserve affine adjustments.** Rotation preserves translation and
  shear; scale preserves unaffected pose components under native scale semantics.
  Replace destructive-conversion expectations with preservation assertions.
- [ ] **12. Give runtime constants semantic owners.** Prepare a small validated
  blink descriptor for channel/frames/timing, retain one baseline expression and
  compose blinking into output. Centralize audio block/control dimensions and
  derive pass count. Do not rename unrelated numeric tables into one constant.
- [ ] **13. Keep native actions free of redundant VM state.** One runtime
  execution enum owns mutually exclusive states; script state/resources stay
  in its variant. Derive native scheduling and remove unused Effect phase and
  invalid-combination checks. Keep authored spell/task behavior unchanged.
- [ ] **14. Shrink test setup at behavior boundaries.** Reuse existing native
  candidate fixtures for lifecycle and reward arithmetic; remove private-asset
  roundtrips for synthetic assertions. Keep representative nine-character asset
  integration, lifecycle outcomes and equipment/reward transaction invariants.
- [ ] **15. Inspect only requested capture observations.** Ordinary captures
  require origins and requested state, not unrelated inactive savestate internals.
  Delete unconsumed inspection, preserve supported requested observations and
  explicit watcher names. Add only focused meaningful Python regressions.

## Previously implemented work to retain and verify

Single bounded typed checkpoint parsing and identity admission; native semantic
conditions and equipment properties; sparse affine retention; exact non-silent
PCM acceptance with silence/phase rejection; native zero/midpoint/LFO tremolo
inputs and current audio schema; removal of the obsolete state fixture writer;
ordinary rescue snapshot/replacement tests. Prior review and pass34 requirement
archives retain behavior obligations, not obsolete implementation or selectors.

Keep combat, save/load and once-only rewards; atomic equipment changes;
field/menu/title transitions; Colette side grip and stable Genis victory
textures; independent audio failures and voice tails; verified capture inputs;
actual movie rendering and owner handoff; published asset integrity. Native
loading stays fast: compare matching gameplay events and report disc-loading
related audio differences separately. Unimplemented Iselia world exit may return
to title. Hardware GPU checks have standing approval.

## Validation policy

Follow the active simplification plan: one affected check run after a coherent
batch; rerun only changed or failed scopes. Full workspace, cooking and capture
checks are milestone work. This historical review does not require repeating them
for each refactor. An ignored test or historical artifact does not establish that
the current implementation passes.
