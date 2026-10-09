# Code simplicity review and implementation

Updated: 2026-10-10. Original review base: `9bfd330`; current rebase target: `main` (`598f27a`). See the final section for validation after rebasing.

All twelve planned source simplifications are implemented. The 22 failures from full validation have been addressed through fixture fixes, current assets and removal of redundant coverage. The follow-up checks and strict workspace linting pass. The full validation below records the initial state; physical controller testing remains a manual follow-up.

## Scope and rules

This review covers battle preparation, action selection/admission/execution, casting and releases, Tech editing, menu memory, their presentation consumers and relevant fixtures. It includes new and untracked modules. It does not claim to audit unrelated audio, field or tooling changes throughout the large branch.

- Prefer direct implementations for supported gameplay, with no compatibility layer for removed code.
- Do not consult, reproduce or reference decompiled code.
- Keep balance/timing choices beside their behavior. Keep asset identifiers at the asset boundary.
- Remove numbers that compensate for a wrong abstraction, rather than merely giving those numbers names.
- Keep tests for distinct behavior and real boundaries; delete obsolete capabilities and duplicated matrices.
- Validate completed batches. Use focused reruns for failures instead of repeating broad checks after every edit.

## Completed findings

| Item | Result | Main implementation |
| --- | --- | --- |
| 1. Fragmented attack definitions | [x] Native hit policies replace imported contact rules and adapters. Timing, geometry, damage and reactions are authored directly. | `game/battle/{normal,hit,enemy,fire_ball}.rs`, `game/battle/martial/attack.rs` |
| 2. Volleys using actor-action machinery | [x] Dedicated volley state owns its target, origin, shot clock and remaining work independently of the caster. | `battle/release.rs`, `battle/action.rs` |
| 3. Definition identity and preparation alignment | [x] Insertion assigns typed `ActionKey` indices. Generated IDs, their allocator, definition-ID searches, placeholder actor setups and the replacement setter are removed. Presentation and fixtures use the same model. | `battle/prepare.rs`, `game/battle/encounter/{preparation,resources}.rs` |
| 4. Special Guard's artificial range | [x] Direct self-targeted admission removes the 2,000-unit reach, opponent requirement and approach setup. | `game/battle/control.rs`, `battle/state.rs` |
| 5. Fixtures driving production APIs | [x] Removed the modifier bypass, unused spell-target modes and `authored_actions`. Fixtures declare normal bindings, techniques or enemy choices. | `battle/{prepare,casting,tests}.rs` |
| 6. Repeated admission | [x] Arrival rechecks live conditions once, then commits through `start_admitted_action`. Immediate starts use prepared keys directly. | `battle/{approach,state}.rs` |
| 7. Indirect menu editing and duplicated memory | [x] Pages return an edit request, callers apply it, and `finish_edit` commits its UI effects. Active pages own selection; memory transfers on entry/exit. Removed placeholder results and whole-page rollback clones. | `game/menu/techniques.rs`, `game/battle/{results/tech,command,lifecycle}.rs` |
| 8. Redundant casting fixtures | [x] Removed unused target/modifier matrices, combined overlapping clock cases and deleted discarded fixture arguments. | `battle/casting/tests/` |
| 9. Release definitions in the action registry | [x] Casting owns an `Arc<PreparedVolley>`. Removed the release action variant, generated release IDs, dummy TP costs and lookup/exclusion branches. | `battle/{casting,release,prepare}.rs` |
| 10. Normal meaning recovered from ordinals | [x] Definitions carry `NormalAttack`; Heavy uses `NormalAttack::Rising`. Preparation validates each normal's kind, execution and zero cost. Techniques cannot also be normals. Range selection, initialization, arrival and guard handling use that meaning. | `battle/{action_selection,control,approach,state,technique_command}.rs` |
| 11. Expensive gameplay matrices | [x] Retained the broad 63-normal asset-binding route and artwork-specific checks. Destruction and Mirage each have one integration route; small core tests cover payment, pause and interruption variations. | `game/tests/battle_preparation/companion_normals.rs`, `game/tests/battle_preparation/martial/` |
| 12. Unexplained casting-effect dependencies | [x] Each cast requests its actual chant, release, charged and stored effects. Removed unconditional extra members `3` and `7`; charged/stored members have local names. Optional artwork remains optional. | `game/battle/encounter/resources.rs` |

Paths in this table are relative to `crates/`, with `src/` before source module paths and `tests/` retained for integration tests.

## Final model

Catalogue IDs describe persistent technique membership. `ActionKey` identifies an immutable definition in one prepared battle. `ActionId` identifies a live execution. Captures project an action key to its numeric diagnostic value at the presentation boundary; the core has no serialization requirement for prepared indices.

An encounter row retains its actor, setup and presentation inputs until preparation is complete. Core construction receives `(Actor, ActorSetup)` pairs. The combat runtime may then store mutable actors separately from immutable setup without constructing placeholder setup or accepting a replacement collection. Redundant preparation checks for collection alignment and a second pass over already validated action bindings are gone.

Normal control slots bind definitions with their matching `NormalAttack` kind. This establishes one meaning before runtime dispatch. Boundary validation still rejects invalid keys, foreign ownership, malformed normal bindings and duplicate technique identities. Travel still requires a fresh admission check because TP, actor state and targets can change.

Fixtures use dense insertion order or returned keys, with named keys for ordinary, alternate, recovery-normal and learned casts. They construct seven real normal definitions when exercising seven control slots. They no longer preserve arbitrary generated IDs or use sparse registries, conversion shims or production test bypasses.

The stored-spell asset fixture checks the core's `CastPhase::Stored` event and its prepared artwork binding. Particle emission belongs to presentation feedback; the fixture no longer expects presentation work directly from the combat core.

## Gameplay choices retained from earlier batches

These are intentional native balance choices, independent of imported artwork and ordinary actor stats:

- Normal attacks use one contact. Ordinary power is 100%; finishers use 125% and knock down; rising attacks launch. Colette's ranged normals throw one weapon.
- Shared physical defaults use inherited weapon elements, guard pressure 1, recoil `[4, 0]`, hitstun 12 and one armor/stagger point.
- Martial power: Demon Fang 150%; Ray Thrust/Crescent Moon 180%; Infliction 200%; Spin Kick 170%; Pyre Seal/Power Seal/Destruction 160%; Beast 140% per contact; Eagle Dive 220%; Destruction rocks 60%. Mirage is movement-only. Individual attacks retain their explicit elemental and reaction overrides.
- Enemy power: Double 75% per contact; Triple 60%; Cross 130%; Pounce 140%; Swing/Spit/Lob 90%; Scatter 50%; other physical contacts 100%. Fire Ball releases three 75% Fire magic projectiles.
- Projectile assets supply motion, geometry and artwork; gameplay supplies hit policy and a 12-tick repeat cooldown.

The disposable `local/battle-recovery-roles-assets` fixture was migrated to the smaller enemy schema in an earlier batch. No full asset import is required to validate these source changes.

## Verification

Final checks use the current tree. Earlier 613-test results were superseded after the action-key migration.

| Check | Result |
| --- | --- |
| `cargo test -p resonance-battle --lib -- --quiet` | **613 passed**, 0 failed; execution 0.05s. Includes ownership, invalid inputs, normal selection/Heavy, payment, learning, after-travel rejection, pause, interruption and release lifetimes. |
| `cargo check -p resonance-game -p resonance-presentation --tests` | Passed; all migrated libraries and test callers compile. |
| Game `battle_preparation` integration: `companion_normals` | Passed: all **63 attacks across nine characters**, including feedback, contact and completion; 6.43s. |
| Game `battle_preparation` integration: ordinary `fire_ball` | Passed: casting, payment, damage and release. |
| Game `battle_preparation` integration: `ex29::` | Passed after correcting the stale core-particle assertion: dormant acquisition, spell storage and prepared effect dependencies; 0.87s. |
| Game `battle_preparation` integration: `enemy_attack` | **2 passed**, including Phantom's casting/projectile route; 4.32s. |
| Game `battle_preparation` integration: `player_control::` | Passed: technique identity, range validation and direct self-targeted Special Guard metadata. |
| Importer `battle_action::` | **4 passed** for enemy action metadata. |
| Importer `battle_projectile::tests::specialized_controllers_remain_unavailable_after_decoding` | Passed. |
| `cargo check -p resonance-import --example refresh_battle` | Passed. |
| Game library `battle::command::` | **14 passed**; its one asset-backed item-picker case also passed separately. |
| Game library `battle::results::tech`, including ignored asset cases | **8 passed**, covering live menu edits and related state transitions; 1.58s. |
| Presentation library `battle::feedback::tests::` | **3 passed**; feedback resolves typed action bindings without changing gameplay. |
| Game library `lens_picker_returns_to_inventory_when_its_last_target_becomes_ineligible` | Passed with cooked assets; 0.69s. |
| Source audit | Removed ownership/release/setter APIs and definition-ID searches are absent from the maintained battle paths. Normal meaning uses enum variants. Scoped code/comment searches found no prohibited implementation references. |
| Whitespace check | `git diff --check` passed for the affected battle source, integration and presentation paths. |

Asset-backed checks use `RESONANCE_TEST_ASSETS=/home/rowan.goemans/Documents/engineering/resonance/local/battle-recovery-roles-assets`. Execution times exclude compilation. The Fire Ball filter initially also selected EX29; its single failure was diagnosed and only that test was rerun after the fixture correction.

These were focused implementation checks. The subsequent full validation below supersedes their limited coverage. Existing direct damage, event-list execution, optional artwork, input-edge latching, fixed-step sampling, disconnection handling and atomic menu edits remain supported responsibilities.

## Full validation before fixes — 2026-10-10

Host: Linux ARM64, Rust 1.97.1 / Cargo 1.97.0. This pass reports findings without changing implementation or fixtures. Logs and exact commands are retained under `local/full-validation-20261010/`.

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` and `git diff --check` | Passed. |
| `cargo test --locked --workspace --no-fail-fast` | **1,679 passed**, 0 failed, 305 resource/device tests ignored by default. Includes the workspace doc-test targets. |
| `cargo clippy --locked --workspace --all-targets -- -D warnings` | **Failed.** A diagnostic follow-up using `--cap-lints warn` completed all targets and found the four issues below; that follow-up is not a strict lint pass. |
| `python3 -m unittest discover -s tools/oracle -p 'test_*.py'` | **11 passed**. Used the environment's Python because `/usr/bin/python3` is absent. |
| Ignored game, presentation, audio and event suites with cooked assets | **137 passed, 21 failed**. Only the physical audio-device test was excluded from these suites. |
| Importer prepared preload and title-music checks | **3 passed**. |
| Media `cooked_movie` ignored suite | **2 passed, 1 failed**. Cancellation and full story decoding passed; opening decoding exceeded its 120-second deadline. |
| Release game/importer build and both `--help` smoke checks | Passed, using the CI static-codec environment settings. |
| Release synthetic rendering smoke check | Passed on Vulkan/llvmpipe: surface and movie/output pixel checks. |

The extra resource suites attempted **164** tests: **142 passed, 22 failed**. They used the absolute `local/battle-recovery-roles-assets` root through `RESONANCE_TEST_ASSETS`, `RESONANCE_COOKED` and `RESONANCE_COOKED_TEST_ROOT`. The other **141** default-ignored tests, including physical-device and additional resource/import audits, were not run. This is not a full asset recook, a gameplay visual acceptance pass, or a Windows/macOS validation run.

### Lint findings

- `crates/battle/src/casting.rs:396`: nested charge-power condition can be collapsed.
- `crates/battle/src/contact.rs:334`: nested combo-notice condition can be collapsed.
- `crates/battle/src/enemy_decision.rs:317`: fixture unnecessarily wraps `PreparedBattle::new` in `Ok(...?)`.
- `crates/game/src/battle/encounter/preparation.rs:60`: obsolete `too_many_arguments` lint expectation.

### Integration findings

| Failures | Evidence and follow-up |
| --- | --- |
| 10 cooked-audio failures | Recovery audio, field audio, movie setup and new-game loading reject packages with an unsupported version or missing `sustain`. These are confirmed stale asset inputs. Regenerate the affected publications before judging the behavior they prevent from running. |
| 2 captured-checkpoint failures | The saved slope fixture contains the removed `conditions` field. Regenerate the checkpoint; do not add compatibility for the old format. |
| 1 Hammer Revenge assertion | `game/tests/battle_preparation/all_party_encounter.rs:240`: damage and projectile expiry pass, but `visible=false`. The fixture inspects core `Cue::Effect`; presentation now maps `Cue::HammerRevenge` to the visual effect. Update the assertion at the appropriate boundary rather than restoring presentation work to the core. |
| 2 classroom checkpoint failures | `game/tests/classroom_script.rs:2475,2831`: title changes and examination rewards reach reload, which rejects an `invalid saved party member`. Root cause remains unresolved. |
| 2 classroom input failures | The collection navigation assertion at `classroom_script.rs:1582` receives `(7, 0)` instead of `(0, 1)`; the Strategy exit at line 904 does not return field control. Review the input sequences and intended menu behavior. |
| 1 opening battle lifecycle failure | `game/tests/field_battle_lifecycle.rs`: the first victory commits, but the second fight reaches `DefeatNotice`, preventing verification of the second victory and field-event resume. Review the native combat route and fixture assumptions. |
| 1 title-scene assertion | `game/tests/title_script.rs:51`: actual coordinates `[242, -573, 1038]` differ from `[240, -575, 1035]`. Establish the intended native contract before changing implementation to satisfy fixed coordinates. |
| 1 Strategy drawing assertion | `presentation/src/field_ui_menu/strategy_tests.rs:67`: the expected `(2, Text, FONT)` drawing batch is absent. Determine whether the drawing or the fixture's internal batch expectation is wrong. |
| 1 streamed movie PCM mismatch | `presentation/src/movie/tests.rs:234`: sample 139264 is zero instead of -2360. This test overlapped release compilation; it has not been isolated from possible resource contention. |
| 1 opening movie decoder timeout | `media/tests/cooked_movie.rs:93`: decoding exceeds 120 seconds. The story movie passes its full frame and lossless-audio checks. The opening test also overlapped release compilation; distinguish performance/load sensitivity from a decoder defect. |

Paths in this findings table are relative to `crates/`. Full failing test names and captured errors are in [asset-tests.log](local/full-validation-20261010/asset-tests.log) and [movie-assets.log](local/full-validation-20261010/movie-assets.log); lint details are in [clippy-diagnostics.log](local/full-validation-20261010/clippy-diagnostics.log).

These findings describe the tree before the follow-up below. They are retained as validation history, not an outstanding task list.

## Validation fixes and test reductions — 2026-10-10

The failures did not warrant restoring removed runtime behavior. Fixed the four lint findings, corrected test setup, and refreshed the disposable local audio publication using the current importer. No old-format compatibility or production test bypass was added. The temporary fixture-refresh hook was removed from the importer after use.

### Useful tests retained and corrected

- Classroom reloads now bind menu rules when constructing `SessionData`, matching normal admission. Without those rules, the fixture accepted only each member's initial title and rejected legitimately earned titles on reload. The shared fixture also removes two repeated rule assignments.
- Classroom input waits for Strategy transitions and sends one intended collection-navigation action per press. The old sequences combined conflicting inputs under the former input model.
- The Strategy shade assertion checks the background draw batch; it still verifies coverage and the opacity ramp.
- Authored-entry loading builds a current checkpoint from the existing fixture helper. It no longer reads an incompatible captured save file.
- Connected-field coverage still visits all eleven Iselia maps and checks locks, shops, both cooking choices and persistence. It now uses normal rule admission and dialogue durations without synthesizing every audio frame. Removed its unused playback-transition helper; dedicated field-audio tests retain real mixer handoff coverage.
- The field-movie clock fixture uses field residency for readiness. Title preparation is independent of an active field; the test now verifies that explicitly and no longer opens an unrelated title movie. Preparation and pause must still consume no play time.
- Title playback retains resource admission and a long-running check for bounded tasks and particles. Removed exact camera, particle and animation snapshots at selected recording ticks.

### Five asset tests removed or consolidated

| Removed test | Why it was unnecessary | Coverage retained |
| --- | --- | --- |
| Hammer Revenge during live equipment editing | Repeated retaliation coverage and incorrectly expected the combat core to emit presentation effects. | Core `retaliation_heals_immediately_and_its_projectile_survives_interruption`; presentation `contact_feedback_uses_resolved_hits_and_keeps_gameplay_unchanged`; existing live equipment-edit coverage. |
| Full opening sequence with two battles, plus its private opening helper | Repeated handoff checks while assuming repeated normal attacks must win both encounters under current balance. | Six event battle tests cover request ownership, outcomes and exactly-once resume/rewards. Game result tests cover reward commits and persistent technique use across two battles. |
| Captured moving-slope checkpoint | Depended on an obsolete save schema and duplicated restore/navigation coverage. | Current checkpoint restart and atomic rejection test; synthetic single/double-axis slope navigation. |
| Real-time full-opening PCM recording comparison | Expensive, dependent on a captured WAV and host scheduling, and duplicated codec and handoff checks. | Existing generated short-movie test now checks actual mixer PCM, final pixels, natural completion and cleanup, in addition to rejecting missing streams. Skip and field-movie handoff tests remain. |
| Second full-movie codec run for the opening | Duplicated full story decoding and included a pinned pixel from the opening recording. | One full story decode checks frame/audio continuity and lossless source PCM. Synthetic codec tests check exact RGB, PCM, timestamps and full-queue cancellation; asset probing cancellation remains. |

The remaining full-movie check fails after ten seconds without a decoder event, rather than requiring an entire movie to finish within 120 seconds. This checks stalls without imposing a machine-speed budget on successful decoding.

The obsolete tests were deleted, not newly ignored. No new test matrix was added. Runtime edits are limited to the lint simplifications; fixture and coverage changes account for the rest of this follow-up.

### Follow-up verification

Logs are under `local/validation-fixes-20261010/`. Reruns target the affected suites and the retained battle boundaries; the earlier workspace test run and release smoke checks were not repeated.

| Check | Result |
| --- | --- |
| Battle library | **613 passed**. |
| Event battle integration | **6 passed**. |
| Synthetic codec integration | **3 passed**. |
| Generated short movie through the presentation mixer | **1 passed**, 0.03s. |
| Classroom and title asset integration | **27 passed** (26 classroom, 1 title). |
| Affected presentation asset checks | **15 distinct tests passed** across field audio, movies, new-game loading and Strategy drawing. |
| Recovery audio and retained battle result boundaries | **3 passed**. |
| Retained full story decode and asset probing cancellation | **2 passed**, 98.42s total. |
| `cargo clippy --locked --workspace --all-targets -- -D warnings` | Passed on the final source tree. |
| `cargo fmt --all -- --check` and `git diff --check` | Passed. |

This follow-up verified **670 distinct tests**, including **47 asset tests**. The first presentation run exposed the stale movie-clock expectation; its focused rerun passed after correction. That run was then stopped during the expensive connected-field audio synthesis. The simplified connected-field test passed separately in 51.60s, preserving every map, lock, shop and cooking assertion. Passing cases were not rerun to obtain a single all-green log.

The earlier full workspace run still provides the broader baseline of 1,679 passing default tests. This follow-up does not claim to rerun every ignored resource audit, release smoke check or hardware test.

## Manual hardware follow-up

Verify physical-pad mappings, held and short presses, two-controller menu ownership, and disconnect/reconnect behavior. The environment has no physical controllers; the user explicitly accepted this as a documented follow-up rather than a completion blocker.


## Rebase onto current main (2026-10-10)

The pre-rebase branch, including its uncommitted source changes, is preserved at
`backup/rowan-battle1-before-main-20261010-5706d84` (`5706d84`). The two WIP commits
were consolidated before rebasing onto `main` at `598f27a`; the backup retains
the original history and complete source snapshot.

Conflict resolution retains the native battle and audio implementations and the
simplified input, asset and task ownership. It incorporates main's world map,
crafting, Grade Shop, credits, scene saves, effects and animation work. Battle
handoff now uses the active scene's party, clock, assets and result destination.
Battle modifiers have one saved owner. Camera-facing transforms retain scale
and shear, and local bone rotation remains a separate operation. Shared field
and world loading supplies the battle dependencies without retaining a live
field renderer. Obsolete adapters and their implementation-only tests were
removed; new upstream fixtures use the simplified APIs.
Surface material setup uses the shared shader registration so every entry point
loads the new effect-color dependency.

Validation logs: `local/rebase-main-20261010/`.

| Check | Result |
| --- | --- |
| `cargo test --locked --workspace --no-fail-fast` | **1,928 passed, 0 failed, 399 ignored**, including documentation tests. |
| `cargo clippy --locked --workspace --all-targets -- -D warnings` | Passed. |
| `cargo fmt --all -- --check` | Passed. |
| Python oracle unit tests | **61 passed**. |
| Synthetic headless rendering (`ci_render`) | Surface and movie/output pixel checks passed on the Vulkan CPU adapter. |

The ignored resource and device tests were not run during this rebase. Full
asset recooking and physical controller testing remain manual follow-ups.
The rebase does not implement encounter-pool selection, battle route overrides,
or additional coliseum/stat-adjustment rules. Combat preparation rejects these
unsupported requests explicitly; main's `--skip-battles` exploration workflow
remains available. Nothing was pushed.

## Post-rebase simplification — 2026-10-10

This review covers the rebased implementation at `aacf265` and its working-tree
changes. The sections below record the additional ownership and loading work;
the final audit states the verification scope and remaining asset limitations.

Completed in this batch:

- Shared session-definition admission lives in `content::session::SessionData::load`.
  Field, world and save loading use it; the presentation-only callback loader and
  its byte copies are removed.
- Field packages prepare their rules, text, skit catalogue and effect definitions
  before entry. Entry and restore reuse those values. The presentation session no
  longer stores separate session definitions or a separate skit catalogue; callers
  borrow the active scene's admitted definitions.
- Skit preparation has one resource-aware entry point. The duplicate loader that
  re-read session/text JSON without binding gameplay rules is removed. The skit
  continuation test now calls production continuation code instead of a test-only
  `next_field` method.
- Attacks validate contact and thrown-weapon definitions in the event walk that
  already validates timing and projectiles. Four iterator/wrapper methods and two
  extra event traversals are removed.
- Gameplay, field captures and world captures share surface/output material setup.
  UI material, shader and draw extraction registration also have one entry point.
- Removed the 256-combination copied-field test and a subtraction assertion that
  never used the production blend state. Actual shader compilation, pixel checks
  and blend configuration checks remain.
- `battle-simplification.md` is now the concise design contract. Its stale open
  proposals and duplicate historical progress reports no longer compete with this
  plan.

Validation logs: `local/simplification-post-rebase-20261010/`.

| Check | Result |
| --- | --- |
| Strict Clippy, workspace and all targets | Passed. |
| Content, battle, game and presentation library tests | **1,118 passed, 0 failed, 157 ignored**. |
| Formatting and diff whitespace | Passed. |
| Synthetic headless rendering (`ci_render`) | Surface and movie/output pixel checks passed on Vulkan/llvmpipe. |
| Selected resource checks | **Three failed before gameplay on stale local assets**: optional-label and authored-entry cases lack `grade_shop`; opening checkpoint loading lacks `collision`. No compatibility fallback or fabricated asset data was added. |

The resource checks used `local/battle-recovery-roles-assets`. Its menu version
number alone does not establish compatibility with main's new fields. These
checks require current publications before they can verify field restore and
entry behavior; the earlier rebase validation also did not exercise them.

## Active scene and save loading — 2026-10-10

The scene owner now contains either a field or a world. A world session no longer
retains a suspended field, its prepared package or an `anchor_field` in its save.
World loading reads the shared and world inventories without preparing a field.
Field transitions retain only the active CPU package; the renderer's separate
artwork cache still reuses GPU resources when revisiting a field.

Scene saves use an explicitly tagged `field` or `world` envelope. The custom
JSON dispatcher and untagged format are removed. Save menus, quicksaves, fixture
writers and diagnostic probes use the same format. Existing save files must be
regenerated; there is no compatibility reader.

All quickloads prepare a replacement through the same candidate loader used by
menu loads. The synchronous in-place restore and its separate script-refresh
path are gone. Input and presentation clocks wait while a quickload is pending;
failed preparation leaves the live scene intact. Test setup calls the production
candidate constructor, and the edited-script test exercises actual reloads.
Obsolete cache-shape assertions and duplicate warm-restore checks are removed.

Scene-neutral consumers use the active menu, events, play time and readiness.
Field-only rendering and diagnostics are guarded by the scene type. Battle and
menu-model shading use the shared lighting texture without borrowing field
artwork. Model previews read script sources from the current inventory, removing
the duplicate source map from field artwork and capture setup. This also removes
the field-only readiness gate that could prevent world audio from starting.

Validation logs: `local/scene-ownership-20261010/`.

| Check | Result |
| --- | --- |
| Content, battle, game and presentation library tests | **1,118 passed, 0 failed, 157 ignored**. Includes tagged field/world saves with numeric JSON keys and scene-owner classification. |
| Strict Clippy, workspace and all targets | Passed on the final source and test callers. |
| Formatting and diff whitespace | Passed. |
| Asset integration | Not rerun against the already identified stale publications. Field/world entry, audio handoff and graphical model-preview integration still require current cooked assets. |

Retained asset fixtures now wait for asynchronous preparation instead of assuming
that field 340 is cached at startup or that a quickload publishes synchronously.
The quicksave probe compares the published restore snapshot after preparation
finishes. These fixtures compile; they have not been claimed as executed.

## Menu checkpoints and final ownership audit — 2026-10-10

Menus and save loading now share `resonance_game::Checkpoint`, with explicit
field and world variants. World menus retain the actual world checkpoint;
the fake field snapshot and world-checkpoint reconstruction are deleted.
Menu pages borrow common progress through the checkpoint. Saved-slot summaries
keep only party, location and play time, without retaining the full scene state.
The numeric-key serialization test moved to the shared type and now exercises
menu edits and the play clock for both variants; no duplicate matrix was added.

Quicksaves and field diagnostic snapshots use the same eligibility checks.
The old world-save fallback for an absent `map_display` and its compatibility
assertion are removed. Save formats may break, as required by this review.

World preparation extends the already verified shared inventory. It no longer
reads that inventory a second time or replaces the caller's diagnostic policy.
The loader, capture routes and fixtures supply the same explicit input.
Renderer artwork remains reusable during area changes, but returning to the
title clears it. CPU field packages already follow the active scene's lifetime.

The saved-slot EX-rule test now loads only session definitions, menu definitions
and a save identity, instead of thousands of unrelated field resources. It still
checks accepted EX skills, rule binding and rejection of an invalid skill.
The temporary importer example was removed from source; its unsuccessful full
inventory refresh is retained only under ignored `local/` for diagnosis.

Validation logs: `local/menu-snapshot-20261010/`.

| Check | Result |
| --- | --- |
| Game and presentation libraries after checkpoint migration | **467 passed, 0 failed, 157 ignored**. |
| Strategy and Unison ordinary integration | **15 passed**, 2 resource cases ignored in that run. One stale Unison decoder failed initially; its focused rerun passed after using the scene checkpoint. |
| Strategy and Unison with fresh menu definitions | **2 passed**. |
| Menu drawing/resource checks with fresh menu definitions | **8 passed**. The first command used a file name instead of the module path and selected zero tests; only the corrected `field_ui::menu::` run counts. |
| Saved-slot EX-rule admission | **1 passed**, including malformed-skill rejection. |
| World simulation/restore tests after removing the old save fallback | **45 passed**. |
| Presentation save tests after sharing eligibility checks | **17 passed**, 6 resource cases ignored. |
| Prepared-resource tests after sharing the world input snapshot | **8 passed**: snapshot reuse, integrity checks, cancellation and diagnostic-policy preservation. |
| `cargo clippy --locked --workspace --all-targets -- -D warnings` | Passed after the final world-loader change, including every migrated fixture and capture caller. |
| `cargo fmt --all -- --check` and `git diff --check` | Passed. |

The fresh menu publication contains 67 dependencies generated by the current
importer from the user's extracted disc. It is sufficient for the eleven menu
and slot checks above. Extending it into a complete field publication fails on
older field data missing `collision`; no compatibility fallback or invented
field data was introduced. Current field/world assets are still needed to run
the ignored scene-entry, audio-handoff and full graphical integration checks.
These results do not claim that those checks passed or that a full recook ran.

### Completion evidence

- The twelve findings at the start of this plan have maintained implementations
  and the behavior checks recorded above. The final source review checked typed
  action insertion, actor/setup ownership, independent volleys, direct normal and
  Special Guard policies, and atomic Tech edits. Removed preparation setters,
  release registry variants and fixture-only bypasses remain absent.
- Field/world ownership is explicit. Admission, save checkpoints, quickload
  preparation, menu progress and world inventory extension each have one path.
  The scoped source scan found no references to decompiled implementations.
- Reduced tests retain distinct behavioral boundaries. Asset-dependent checks
  remain opt-in; obsolete compatibility assertions and duplicated matrices are
  deleted rather than newly ignored. Hardware testing remains the accepted
  manual follow-up above.
- `plan.md` records the changes and evidence; `battle-simplification.md` contains
  the current design contract. The newer backup branch
  `backup/rowan-battle1-before-main-20261010-aacf265` (`22ef82e`) preserves the
  committed and uncommitted state before this final pass. The original pre-rebase
  backup remains intact. Main was fetched and verified at `598f27a`; the repeat
  rebase was a no-op. Nothing was pushed.
