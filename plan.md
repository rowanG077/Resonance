# Battle branch scope cleanup

Updated 2026-10-11. Compared all seven branch commits and the working tree with
`origin/main` at `6f29b0382a4d4e54d8bf86443991d34831b6b0f1`.

The rule is to keep battle work and shared changes that make its integration
simpler. Unrelated gameplay, presentation and tooling should retain main's
behavior and existing regression coverage.

## Findings and changes

| Area | Result |
| --- | --- |
| Dialogue and menu cursors | Restored main's textured cursor, movement, shadow, trail and window-specific artwork. Removed the replacement triangle. |
| Startup and title | Restored main's readiness gate, title animation, input replay and startup behavior. New Game again preloads field 340 alongside setup field 5, including its required story movie metadata. |
| Field behavior | Restored main's actor movement, animation blending, dialogue progression, effect handling, save-point rendering and skit failure behavior. Battle suspension and completion remain explicit integration points. |
| Item menu | Restored main's page transitions, closing delay, scrolling, description fades and target preview timing. Battle still borrows the shared inventory data and menu drawing. |
| Movies | Restored main's media crate, decoder preparation, buffering, PCM playback and error handling. The shared scenario input adapter remains. |
| Audio | Restored main's title startup fade and independent audio comparison tests. Retained the shared cue/voice engine used by battle. |
| Tests and tools | Restored main's field, title, script, rendering and oracle coverage and capture entry points. Removed branch-only tests for unrelated behavior that was reverted. |
| Other cleanup | Restored unrelated source-parser, documentation, workflow, script VM and world-travel default changes. |

Across the 1,012 paths originally changed by the branch, 302 now match main
exactly and 10 branch-only additions are removed. Other paths contain battle
code or mixed files with retained integration changes; the count alone does
not describe their scope.

## Retained integration

- Battle simulation, content import, preparation, input, rendering, feedback,
  results and field encounter handoff.
- Typed party ailments, stats, techniques and battle history shared by field,
  menus, saves and combat.
- Shared menu data and drawing, material registration, draw ordering, pose
  conversion and cooked secondary-motion definitions used by battle actors.
- Shared audio cues, completion tokens, mixer ownership and cooking needed for
  battle sounds and transitions between title, field and battle audio.
- Prepared resources, active-scene ownership and checkpoint admission used by
  field, world and battle transitions.
- Shared capture/scenario input APIs for routes that cross scene boundaries.

These are integration allowances, not a reason to change unrelated behavior.
Main's field secondary-motion simulation remains in use; battle shares the
cooked definitions without replacing the field simulation.

## Assets

Refreshed the UI metadata and cursor artwork in the default `local/all-assets`
directory and updated the affected inventory hashes. This published 71 UI
resources and refreshed one field inventory. No full recook is needed for
this cleanup. The old UI files and the one-off refresh utility are preserved
in the local audit directory; the utility is not added to the importer.

## Validation

- Ran workspace tests once, then fixed integration fixture mismatches and reran
  affected suites. All failures from that run have passed focused reruns.
- Python oracle suite: 58 tests passed.
- Main's cooked New Game regression passed both settings routes using
  `RESONANCE_TEST_ASSETS=$PWD/local/all-assets`.
- Headless Vulkan capture reached the title and the New Game settings prompt
  at 1920×1080. Moving from Yes to Not right now moved the restored cursor.
  The recording completed with no diagnostics or unprepared asset reads.
- Final `cargo check --workspace --all-targets`, formatting and whitespace checks
  passed. Main's cooked story-movie handoff test passed after restoring the
  movie implementation.

This is not a claim that every ignored asset or hardware test was run.
Physical controller testing remains the agreed manual follow-up.

## Recovery and evidence

Pre-cleanup branch: `backup/battle-before-scope-20261011T101039Z`.
The pre-existing working changes, source snapshots, restoration ledger, asset
backup, capture results and validation logs are retained in
`local/battle-scope-20261011T101039Z/`. The prior version of this plan is in
that directory's `original/plan.md`. No commits or history rewrites are part
of this cleanup.
