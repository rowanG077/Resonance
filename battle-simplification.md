# Battle simplification

Working plan against local `main` (`9bfd330`), including the current untracked
implementation. The earlier implementation and automated gameplay/presentation
milestone are complete. The second simplicity review below identifies further
work; the earlier checklist is not an overall readability approval. These new
findings are proposals only. Physical controller testing remains the agreed manual
follow-up.

## Working rules

- Never consult, cite or reproduce decompiled code. Historical behavior, scheduling
  and random sequences are not compatibility requirements.
- Break internal APIs and cooked formats when that removes complexity. Refresh
  content instead of adding compatibility adapters.
- Give mutable state one owner. Prefer ordinary data and explicit transitions;
  avoid frameworks, builders and configuration languages.
- Decode binaries in importers, resolve assets during preparation, run gameplay in
  simulation and visuals in the scene. Keep meaningful asset IDs at their binding
  boundary and tuning beside named fields; do not name every numeric literal.
- Preserve payment, damage, interruption, inventory/reward atomicity, save handoff
  and deterministic replay. Optional feedback cannot own gameplay.
- Make substantial coherent changes, then build once and run affected checks once.
  Keep Cargo settings consistent. Select ignored cases per binary; shared filters
  previously selected an unrelated 155-second publication sweep, since deleted.
- Rerun only failures or relevant subsequent edits. Full workspace and GPU checks
  belong at milestones. Do not repeat inventories, hashes, cooks or visual routes
  for routine refactors. Use existing recordings when they answer the question.
- Delete obsolete tests with their mechanisms. Keep transaction checks and a few
  real content scenarios. Unrelated balance/device review cannot delay a batch.
- Keep current decisions and evidence here; detailed execution logs belong in
  artifacts. Count deleted mechanisms and useful outcomes, not just passing tests.

## Review findings and proposed changes

Local `main` has no battle crate; this branch adds that subsystem. Reviewing tracked diffs alone also misses
many current untracked modules. The review inspected current owners and selected
changes against main, not every line of unrelated workspace changes.

- **Validation policy corrected.** Ordinary Linux workspace tests now use the test
  profile, and Linux no longer repeats the release smoke test subset before the
  full run. Release executable/help/render checks remain. The full test step keeps
  the smoke-output environment so audio diagnostics are still produced. Workflow
  shell syntax was checked locally; hosted CI has not run for this change.
- **Common feedback ownership simplified.** MobilityDefinition, ControlExFeedback,
  TauntDefinition and SpecialGuardDefinition are removed. The model resolves common
  jump/backstep/taunt/recovery poses; scene feedback owns EX text/art/voices,
  takeoff/backstep/knockdown feedback and the Unison-ready sound. Special Guard
  stores only its action ID; artwork uses the existing action event definition.
  Technique-command speech and Over Limit audio now resolve from scene feedback
  after semantic events. Hammer retaliation emits a native projectile; the scene
  attaches its artwork. Recovery poses and idle expressions belong to models.
  Casting now uses common chant/release poses and feedback at meaningful
  transitions; timed chant programs and repeated pulses are removed. Immutable
  action feedback stays beside authored events on the existing gameplay clock.
  Trailing cosmetic events cannot delay recovery; pending gameplay events can.
  This avoids a second timeline and mutable feedback owner.
- **HUD image admission is explicit.** Battle and game-over fonts decode before
  activation. Optional portraits/icons decode at the same boundary and are omitted
  on failure; game-over background failure uses a black surface. GPU preparation no
  longer recovers or rewrites HUD images. The duplicate image inventory and a mock
  readiness test are gone. Named depth groups own battle/menu/defeat ordering.
- **Title accounting simplified.** Results grant every newly eligible title once
  from final character state and combat statistics. Ordered attempts, title IDs in
  simulation, eligibility masks and precedence rules are gone. Combo variety lives
  with combo state; each distinct normal variant counts, including finishers and
  aerial variants. Equipment awards check the recipient's own gear. Manual-control
  and availability requirements are evaluated at results, without event snapshots.
- **Character-specific counters removed.** Item titles use per-actor item counts;
  unused Regal counters and character identities on item loans are gone. Casting,
  chain limits, item discovery and title rules use the existing Character enum.
  Keep remaining identities at the game/content boundary; do not turn legitimate
  clip/sound IDs or local tuning into hundreds of constants. Remaining contact,
  Spell Charge, equipment-history, reward and victory selectors now name characters.
  Shared named HUD depth groups own ordering.
- **Incidental test contracts reduced.** Rewards retain one replay scenario;
  no-draw/cap/resource matrices, affinity tie order and happiness notice order are
  removed. Sparse animation has one complete retention/replacement scenario.
  Learning, targeting, unavailable actors and HP-based rewards check outcomes
  without prescribing random draws. Ricochet retains contact and pause behavior;
  it no longer freezes another projectile's random sequence. The private Unison
  script/save fixture is removed: event-owner tests cover flag validation and
  persistence, while results tests cover menu gating and live edits.
- **Dormant spell paths removed.** Production preparation selects Fire Ball only.
  Nurse and Lightning release branches, their native scenarios and the two-disc
  Nurse publication test are deleted. Releases use a projectile volley with count
  and spacing selected by the game; Fire Ball retains three shots eight ticks apart.
  Shared casting fixtures use that runtime and no longer assert unrelated healing.
  The cooker also stops exporting the unused Lightning hit descriptor.
- **Audio completion simplified.** Live and offline reverb share a finite tail:
  two decay periods, allowance for queued input and a 100 ms fade. Expiry clears
  filter history; idle and dry-only effects skip filter work. Weighted network
  bounds, buffer magnitude scans and state-derived drain budgets are removed.
  Delayed-output and decay/fade checks replace the numerical proof matrices.
- **Weapon admission consolidated.** The content loader constructs admitted weapon
  definitions once; immutable layer relationships are private to the battle crate.
  Model binding checks only attachment bones and slot uniqueness. Equipment changes
  retain pose continuity and reject invalid binding without undoing gameplay commits.
  The content scenario checks displayed equipment, without inspecting private layers.

The final milestone found and fixed two presentation regressions: a model without
a chant clip now uses its cast clip, and Special Guard artwork follows the body
center. Ground-attached action effects retain their root placement. Two unused
HUD image methods were deleted. No extra animation timeline or effect scheduler
was introduced.

## Second simplicity review — proposals, 2026-10-10

Reviewed the current authored implementation, including untracked modules: action
selection/approach/start/contact/recovery, casting and released volleys, preparation,
command-page edits and representative tests. Local main has no battle subsystem;
this is an architectural review of the added subsystem, not a claim to have reviewed
every unrelated workspace change. No implementation changes, builds, test runs,
asset refreshes or captures were made for this review. No decompiled sources were
used. The following items remain open.

- [ ] **S1 — Author maintained hit policies beside their actions.** Normal timing
  and geometry are native, but contact count, damage flags and recoil still travel
  through imported pools, cooked Hit/Emission/HitRule records, and game conversion.
  See `normal.rs:286`, `action.rs:7`, `recoil.rs:56` under game battle and
  `import/src/battle_action/normal.rs:9`. Author the maintained actions' hit and
  reaction definitions directly alongside timing; retain artwork/stat imports.
  Delete the contact decoding and conversion paths made unnecessary by that choice.
  **High impact; deliberate balance/reaction changes**, not a behavior-preserving
  cleanup. Shared content used by other actions must migrate before its deletion.

- [ ] **S2 — Give released volleys a small runtime of their own.** A released volley
  uses the full action Sequence, including normal/combo/melee/recovery fields it
  does not need. `action.rs:6` and battle `state.rs:942`, `1250`, `1464` mix actor
  tasks and released residents through shared lookup and dispatch. A Volley needs
  its owner, target, slot, origin, shot count and clock. Keep it independent of
  caster interruption; remove Release from the generic actor execution path and
  simplify the mixed sequence helpers. Preserve the post-contact transition point.

- [ ] **S3 — Resolve action identity once during preparation.** Generated u16 action
  IDs and catalogue u16 IDs are repeatedly converted through maps and searched
  vectors (`encounter/preparation.rs:35`, battle `technique_command.rs:10`,
  `state.rs:1670`). Use a checked definition index or direct prepared reference
  internally; retain catalogue IDs for persistent identity and ActionId for a live
  execution. Pair actor values with their setup during construction instead of
  rebuilding parallel roster associations. Remove redundant identity validation
  made impossible by construction; retain external-data checks and live admission.

- [ ] **S4 — Make Special Guard a direct self action.** Game `control.rs:54` assigns
  it a range of 2000 and `uses_weapon_reach: true` despite SelfTarget. Battle
  `state.rs:1971` then requires an opponent and constructs approach parameters.
  Bypass opponent approach for self actions using the existing target semantics.
  This deletes an artificial range and movement setup. Also replace Unison's raw
  `enabled & 0x02` check with the existing named command mask.

- [ ] **S5 — Remove test-only and dormant spell capabilities.** Production always
  enables `CastingDefinition.apply_modifiers`; disabling it exists in fixtures.
  SelfOnly is never prepared by the game, and both maintained party/enemy casts
  use the offensive Fire Ball definition, not Support. Remove the unused spell
  release modes and modifier bypass, then prune their synthetic matrices.
  `ActorSetup.authored_actions` is populated only by test code in the current tree;
  replace that extra production admission channel with fixtures for real supported
  action ownership. Do not remove ally targeting needed by items or menus.

- [ ] **S6 — Have one arrival admission/commit path.** Approach completion checks
  `action_rejection`, then reaches `start_actor_command`, which checks it again
  (`approach.rs:325`, `state.rs:1716`). Normal dispatch detours through two helpers
  to recover an action ID it already had. Consolidate admission and start for the
  resolved action. Keep the essential recheck after travelling: TP, availability
  and target state may change during approach. Preserve paralysis and payment order.

- [ ] **S7 — Return menu edits directly and give selection memory one owner.**
  `results/tech.rs:189` calls an edit callback that only captures the edit and
  returns a placeholder result, then applies it and patches the visit or restores
  a cloned page. Return the edit as ordinary data and apply it before confirming
  the page transition. Keep validation before mutation. Separately, page/command
  selections are mirrored through lifecycle forwarding methods and presentation
  events (`command.rs:256`, `lifecycle.rs:131`, presentation `battle.rs:1046`).
  Transfer remembered selection on menu entry/exit rather than synchronizing it
  through several representations throughout the visit.

- [ ] **S8 — Remove tests together with those mechanisms.** Start with the unused
  target modes in `casting/tests/targets.rs:22`, the apply-modifiers bypass and
  duplicate once-at-entry clock coverage in `casting/tests/clocks.rs:6,110`.
  Remove the discarded `_id` argument and misleading overlimit-specific fixture
  wrappers in `casting/tests.rs:180`. Keep payment/interruption/landing/hold tests,
  the all-normal-attacks content scenario, and once-only persistence checks.
  No test-count quota and no new generic fixture framework.

The damage arithmetic is already a direct pipeline; its large file also contains
substantial test code. Rewriting it into a modifier framework would make it harder
to follow. Likewise, the existing attack event list and the distinction between
simulation and optional artwork are useful. The next changes should remove state,
lookups, adapters and unsupported capabilities rather than merely redistribute files.

## Work list

1. [x] **Separate optional feedback from gameplay.** Native capsules, contacts,
   spacing, targets, shadows and camera bounds are independent of artwork. Tolerant
   diagnostics omit optional models/effects/audio; paranoid mode rejects malformed
   resources. Native HUD entry fade and cursor geometry replace capture/fracture and
   cursor asset machinery. Required fonts decode and match their atlases before HUD
   activation. Optional portraits/icons decode before GPU warmup; failures omit
   them. Game-over text also requires a decoded font, with a black fallback for its
   optional backdrop. Menu-page image errors return to the readable command strip.
   The late HUD replacement path is removed. Shared decode and real card-rendering
   checks verify required fonts and optional-image omission before GPU warmup.

2. [x] **Establish an ordinary test baseline.** An earlier full workspace run passed.
   It is a baseline, not certification of later changes. Use affected checks while
   editing; ignored asset cases are not ordinary coverage. Broad milestone checks
   remain necessary before claiming the complete goal.

3. [x] **Give actor state coherent ownership.** Actor-local tasks own admission,
   payment, interruption and recovery. Released attacks own their lifetime. Combo
   state, hit history and controller assignment have one owner. One published clock
   governs gameplay and scene updates, including menu holds and the rescue exception.

4. [x] **Use direct native action definitions.** Normal attacks share one event
   runner, three windup speeds and native contact timing. Opening/follow-through
   poses resolve together; throws use native timing and spherical contacts. Enemy
   choices name native actions and carry resolved damage. Shared animation roles
   resolve at preparation, and breakfall turning has native tuning. Released spells
   use a direct projectile volley; unused per-spell release branches are gone.
   Current content scenarios exercise all 63 normal attacks, Genis/Phantom casting,
   Regal landing/recovery and the maintained martial actions. Native checks cover
   directional chains, launchers, contacts and movement. Recordings review impacts,
   thrown weapons, Presea, casting, menu holds and Special Guard placement. Missing
   chant art has a simple cast-pose fallback; shield effects use the existing body
   attachment. Human feel and balance judgments remain outside automated evidence.

5. [x] **Use one ordinary-attack implementation.** The maintained content scenario
   requires all 63 attacks to hit and finish. Native scenarios retain payment,
   interruption, recovery, buffered continuation and landing behavior.

6. [x] **Remove unexplained identities and duplicated limits.** Sound types, shared
   animation roles and world draw layers are named. Projectile references resolve
   before runtime. Particle budgets and party/enemy/total actor capacities each have
   one owner; throw hit history replaces per-target cooldowns. Title rules now own
   their content IDs at results; item accounting needs no character IDs. Contact
   audio, Spell Charge, equipment history, rewards and victory name their character
   selectors. Named HUD depth groups own ordering,
   including shared menu planes and the game-over overlay. Meaningful asset IDs
   and local tuning remain acceptable.

7. [x] **Keep presentation out of authoritative state.** Scene owners now handle
   animation/effects, voices, flashes, camera shake, rumble, notices and result pages.
   Contact, admission, recovery, rescue, landing, death, item and breakfall feedback
   use resolved events. Extra impact branches, reflection artwork, Over Limit
   particles and Hourglass fullscreen-flash state are removed. Entry speech is
   prepared once; victory cosmetic choices belong to results. Jump/backstep/taunt
   poses now use model-owned common clips. EX labels/voices/effects, knockdown and
   Unison-ready audio belong to the scene, without cosmetic bundles in actor setup.
   Technique-command and Over Limit speech belong to scene feedback. Over Limit
   has plain gain/duration settings, with no resource admission or missing-bundle
   failure. Hammer Revenge artwork follows a native projectile from its semantic
   event. Projectiles resolve launch data and birth feedback at emission, without
   an unpublished initialization state. Recovery return carries speed/turning policy
   only; model playback owns return/stop clips and restores idle expressions.
   Casting carries gameplay policy only; scene feedback owns its audio, effects and
   notices. Common chant/release poses replace per-character timed choreography.
   Ordinary, stored, aerial and revenge spells share one dispatch path.
   HUD images are admitted before GPU warmup; required text cannot enter the
   optional replacement path. Immutable optional feedback remains beside action
   events, using the existing cursor. Recovery waits for gameplay events and active
   contacts, never trailing cosmetic cues. The landing scenario checks this while
   retaining pause and complete contact windows. Special Guard now follows
   the body center, verified in the completed visual milestone.

8. [x] **Publish one completed snapshot per update.** Lifecycle commits first;
   audio and presentation consume the completed batch once. No partial-dispatch
   cursor, prefix replay or duplicate snapshot owner remains.

9. [x] **Reduce test machinery and coupling.** Core tests retain payment,
   interruption, contacts, recovery, pause, learning and transactions. Real content
   cases cover normal attacks, maintained techniques, enemy preparation, equipment
   and field handoff. Duplicate catalogue, voice/pose, missing-art and unsupported
   spell matrices have been removed. Host tests use direct checkpoints/completed
   results. Workspace test compilation is unoptimized with optimized dependencies.
   The duplicate direct Unison gauge matrix and cosmetic mobility fixtures are gone;
   existing scene/model tests cover the new common requests. CI uses the test profile
   and avoids repeating the Linux smoke subset. Title-order matrices and three
   private-asset title-policy scenarios are removed; small results tests cover the
   simpler award policy. Audio completion uses two focused delayed-output/decay
   scenarios in place of numerical proof matrices. A duplicate weapon-admission
   test is removed; the content case checks displayed equipment. Dormant spell and
   Nurse publication scenarios are removed; shared casting fixtures exercise the
   playable volley path. Reward draw-count matrices, result-order tests, repeated
   sparse-blend lengths and cross-projectile seed coupling are removed. The private
   Unison script/save fixture duplicated event-owner coverage and is removed.
   Over Limit wrapper, audio-forwarding, missing-bundle and delayed-jitter tests
   are removed; existing scene/model scenarios cover the simpler feedback paths.
   The native chant-voice matrix and duplicate voiced item-cancellation fixture are
   removed; existing transaction/model/scene scenarios cover casting transitions.
   Over Limit coverage now combines automatic guard and reaction suppression,
   retains one strongest-reduction matrix and one large absorption scenario, and
   removes repeated affinity/control/bounds matrices. The retired chant decoder
   and its test are deleted. The 13-case victory fault model and duplicate display
   adapter are gone. One missing-motion admission/fallback scenario and one full
   reward/item/learning commit scenario replace the presentation matrices. Native
   movement, conditions and party projection retain speed/regeneration coverage;
   their duplicate content modules are removed. The weapon-swap content scenario
   retains live-contact and committed-equipment integration. Startup covers weapon
   history once, alongside formation projection. Content scenarios now focus on
   preparation and cross-owner commits instead of repeating native policy matrices.

10. [x] **Remove identified implementation citations.** Current authored-code scans
    found no direct decomp function citations. This does not prove architectural
    independence; the native ownership, identity and randomness reviews are complete. Comments describe native behavior;
    binary-format facts stay at the import boundary.

11. [x] **Keep selected actions stable when learning.** Learning changes membership
    and shortcuts without replacing admitted actions, changing their cost/voice
    or bypassing affordability.

12. [x] **Use native randomness.** Gameplay and cosmetic streams are separate.
    Successful handoff commits rewards and gameplay randomness; failed preparation
    or cancellation leaves the field unchanged. The native Battle no longer owns
    a cosmetic RNG: entry preparation, scene hurt/death feedback and results own
    their choices. Victory selection takes one sample. Learning, targeting,
    rewards and ricochet no longer prescribe incidental draw counts. Reward replay,
    transaction rollback, pause and cosmetic isolation checks remain. Projectile
    launch variation resolves once at emission, with no delayed-initialization
    contract. Lucky charge checks the result and matching feedback without mirroring
    the generator's first sample. Breakfall, missed contacts, contact benefits,
    proficiency and paralysis check outcomes without prescribing draw eligibility.
    Impossible ailments and cooking outcomes also avoid incidental draw counts.
    Rejected transactions still preserve randomness. The shared native SplitMix64
    stream retains a save/resume check; pause, replay and cosmetic isolation remain
    explicit guarantees.

13. [x] **Keep damage and contact policy small.** Bounded variance, native criticals
    and percentage combination replace historical arithmetic. Released attacks
    capture power; actual damage and confirmed kills supply benefits. Each throw
    hits a target once. The damage-policy review retains direct additive attacker
    bonuses and defender mitigation, then affinity, protection, HP and conditions.
    No configurable modifier framework is needed. Current native checks preserve
    absorption, defense, conditions, reactions and interruption. Maintained content
    actions hit and finish; the Presea recording confirms damage, one payment,
    pause and recovery. These checks establish behavior, not a claim that every
    matchup has received human balance testing.

14. [x] **Remove secondary complexity.** Duplicate preparation/resource/equipment
    wrappers and repeated immutable animation-curve validation are removed. Actor
    visits share profiles, action tables, voice binding, controls and feedback;
    results/learning share the roster. Enemy preparation calculates statistics once.
    Title event history and precedence, unused character counters and item-loan
    identity payloads are removed. Reverb tails now use a finite countdown and fade;
    weighted network bounds and drain budgets are gone. Immutable weapon-layer
    admission happens once at construction, with attachment/slot checks at binding.
    Over Limit and contact-art setup bundles, the game Over Limit wrapper, and
    resource-dependent Over Limit admission are removed. Casting motion/voice
    wrappers and resource/release builders are removed; one native dispatch path
    serves ordinary, stored, aerial and revenge releases. The imported chant
    program, decoder, casting-presentation fields and Casting wrapper are removed.
    Profiles carry only a scalar casting duration; party/enemy publications are
    refreshed through the production publishers, without a compatibility adapter.

## Current evidence and milestones

The final milestone passed **1,337 ordinary checks** across battle, game,
presentation, audio and import, plus **10 selected content checks**. Content
coverage includes all 63 normal attacks, Genis and Phantom casting, four martial
scenarios and three escape scenarios. Escape covers command handoff, cancellation,
departure and once-only field commit without rewards. One armor test initially
failed because it prescribed notification order within an update. It now checks
the same third-hit interruption after reading the completed update. Only that
failure was rerun; the remaining suites then ran once. No new tests were added.

Five current strict recordings cover opening combat/results/field return,
Fire Ball, Colette's throw, Presea's Destruction and Guardian. All completed without
diagnostics or unprepared asset reads. Opening results commit gald and battle
history once. Throws and Destruction hold with the menu and recover; Destruction
also clears its particles. Fire Ball and Guardian alone were recaptured after
their fixes. Genis visibly chants, pays once, releases and returns to idle with
no remaining particles; Guardian's shield surrounds Lloyd's body. HUD labels,
target markers, the command strip, impacts and results were visually inspected.
Artifacts: `/tmp/battle-milestone` and `/tmp/battle-visual-finish-*`.

The preceding test-consolidation batch removed **727 net Rust lines, 6 tests and
2 fixture modules**; its 12 selected checks passed. The preceding implementation
cleanup removed another **167 net Rust lines, two data types, the chant decoder
and four tests**; its 51 selected checks passed. Their artifacts remain under
`/tmp/battle-test-consolidation-*` and `/tmp/battle-final-cleanup-*`.

Use **`local/battle-recovery-roles-assets`** for current private integration work.
The production targeted refresh wrote 878 publications and one inventory;
receipt: `attack-refresh.json`. This final code-only batch required no asset
refresh. Earlier libraries and `local/all-assets` were left untouched.

## Manual follow-up and review limits

- **Physical controllers:** the user chose a documented manual follow-up rather
  than keeping simplification open for unavailable hardware. Check two controllers
  controlling separate actors, menu pause/resume, reconnect/reassignment and rumble.
  No physical controller behavior is certified by the software recordings.
- **Feel, balance and audio:** scripted routes establish observable behavior;
  subjective input feel, all-character matchup balance and listening judgments
  still need human play. No universal balance or audio-quality claim is made.
- **Visual scope:** Phantom, Regal and ordinary escape have current content checks;
  their complete GPU sequences were not visually reviewed in this milestone.
- Fire Ball is the supported spell release. Nurse, Lightning, additional artes,
  enemy variants and complete Unison execution remain separate feature work.
- Hosted CI has not run for the earlier workflow changes. The current broad checks
  cover the five affected libraries; the earlier full workspace run is only a
  historical baseline. This is not certification of unrelated workspace changes.
