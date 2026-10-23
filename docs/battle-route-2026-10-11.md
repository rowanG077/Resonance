# Early battle repair batch

This report describes the preceding publication. The
[resource recovery report](battle-resources-2026-10-12.md) records subsequent
fixes, all 45 ordinary victories, Giant Bee, and the current resource counts.
The earlier claim that group dialogue requires missing participant animation
was not established: our saved implementation retains staged poses, and no
matched source group performance has yet been captured.

Sixteen parallel workers investigated distinct battle resources using original
asset data and controlled Dolphin recordings. Each worker owned at most one
Dolphin instance. The parent owns the consolidated build, selected publication,
and native checks. There is no full recook or full validation run in this batch.

The selected refresh is published to the default cooked library. All nine
selected encounter/result checks now pass; successful preparation is not proof
of visual parity.

## Implemented changes

| Area | Change |
|---|---|
| Party normal attacks | Imported the seven directional action tables for each character. Removed substituted timing, fixed contact boxes, clip/voice maps, and invented finisher bonuses. Contacts follow the selected weapon's animated bones and include their authored final update. |
| Early party artes | Sonic Thrust and Sword Rain use authored action data and existing attack events. Lloyd's Demon Fang active/recovery timing is corrected. Kratos Fire Ball uses the existing verified projectile recipe. |
| Enemy decisions | Follow-ups use the existing chain window and weighted selection. Once-only actions are consumed on admitted execution. An authored condition limits the count of available enemies. |
| Enemy commands | Added body pass-through, common effect emissions, RGB flashes that return to stage tint, and animation rate changes without restarting a pose. Self-destruction enters defeat directly while preserving HP and already released projectiles. |
| Projectiles | Admitted the measured Slime flags; baked Brown Pot/Fake per-shot modifiers into native recipes. Consecutive projectile records retain their observed emission order. |
| Formation appearance | Fake selects its authored body-atlas row using existing texture data. Invalid rows remain rejected; carried weapons and facial animation retain separate selectors. |
| Particle effects | Added pollen's packed color assignment. Shared random draws preserve authored component correlations, including Fake's model choice and Stone Blast's offsets from the base model slot. |
| Arenas | Admitted the observed low layer bits after effective pre-load asset comparisons, including visible positive controls. This allows selected publication of arenas 2, 30, and 31. |
| Entry | The opening pan runs for 59 updates, followed by 46 held combat updates. Removed the duplicate simulation pause during the presentation fade. The white entry wash follows the measured alpha decay. |
| Spell scenes | Eruption loads its own models, textures, effects and contact rules; timed pulses retain their target-ground origin. Lightning and Stone Blast share the resident effect bank and scheduler. Fire Ball now uses its authored 60% hit power and cooldown. |
| Shadows | Animated visible mesh bounds determine the footprint; combat capsules no longer determine shadow size. |
| Battle UI | Authored notice frames, character portraits, action icons, combo artwork, result frames and gradients replace generic panels. Damage-number projection now matches the expanded scene. |
| Results | Celebrant-centered party arcs use measured three/four-member placement. Forty-three ordinary victory streams retain authored pose, expression, rate and loop events. |
| Audio | Selected missing route cues and voice streams are prepared separately, including Sonic Thrust and Sword Rain voices. |

## Validation and publication

The release build and focused tests pass. The selected refresh took six seconds;
subsequent scene/audio updates were also targeted. No full recook or broad test
run was performed. Native checks passed for entry/Zombie, Ghost, soldiers,
whips, Slime, pollen in arena 2, scorpions in arena 30, Ktugach in arena 31,
and victory/field return.
Changed shadow/result rendering and the completed field return were rechecked
after the follow-up build. These are selected checks, not route acceptance.

Entry observations confirm 46 held combat updates and playback resuming on the
next update. Sonic Thrust matches the retained active/completion timing and 14
sampled movement distances within 0.000024 units. Both Sonic Thrust and Sword Rain
land guarded hits in ordinary-input fixtures. Sword Rain's aligned source run
exposed later poses/contacts arriving 1–4 updates early. Its importer correction
is published; both focused timing/contact reruns pass with zero diagnostics or
unprepared reads.

Eruption's model UV scroll is established by a controlled source probe and two
focused tests. Its missing palette window is now imported from the authored
selection; natural encounter preparation passes. An isolated release completes
14 selected captures through combat update 480 with no diagnostics or unprepared
reads. It covers the transition freeze, three visible bursts and damage; exact
projectile birth instants remain supported by source observations and focused
tests rather than these spaced screenshots. The result
recording also selects group 15, whose animation binding remains unimplemented;
its idle poses do not validate ordinary or group victory playback.

Published 583 changed files (23,349,775 bytes) into `local/all-assets`, including
the new battle UI, selected arenas, action metadata and missing voice/effect
cues. Changed-file identities were checked and previous files retained in
`default-before`. A second publication updates six files (3,566,773 bytes) for
the scene palette and Sword Rain profile, retaining `default-before-2` and
`publication-2.json`. All 30 deferred source entries remain open.

A third publication refreshes 13 files (4,837,533 bytes), with originals in
`default-before-3`. It includes resident spell rules and models, Lightning ribbon
drawing metadata, Fake's body-atlas selector, Raybit's direct defeat event, result
gradients and selected audio. The playable release runtime is rebuilt. Three
additional focused checks pass for ribbon geometry, body-atlas isolation and
formation decoding. No full recook or full validation suite was run.

Six affected captures pass with no diagnostics or unprepared reads: entry,
victory/field return, natural Fake formation 271, Stone Blast, Lightning and
Raybit self-destruction. The spell fixtures verify TP costs of 7/9, Stone Blast's
three births at release+32/+37/+42, and Lightning's birth at release+22. The
self-destruction fixture preserves HP through defeat, retains its released
projectile and explosion, and reaches victory results. These controlled spell
and defeat fixtures do not establish matching source damage or whole-battle
fidelity. The initial entry replay waited until the wash cleared. A separate
replay-only capture now verifies rendered alpha 255/207/159/47/0 at updates
0/5/8/15/18. Source observations support the intermediate samples; source
clearance specifically at update 18 remains inferred. Screen fragments and exact
Lightning noise-pattern fidelity remain unverified.

The updated metadata audit clears explicit blockers for Fake and Raybit: nine
of the route's 28 enemy identities still have explicit blockers, affecting 48
of 82 fixture cases. This count describes inspected metadata, not visual parity.

Machine-readable records are retained under
`local/battle-route-fidelity-20261011/`:

- `test-build*.log`: integration compilation and any corrections.
- `cook-report.json`: selected metadata/art publication, including failures.
- `native-results.json`: affected encounter recordings, with diagnostics.
- `publication.json`: default-library publication, backup and explicit failed case.
- `publication-2.json`: palette/profile follow-up and all nine passing checks.
- `publication-3.json`: selected follow-up publication and six affected captures.
- `followup-render-checks.json`: three focused checks and selected metadata refresh.
- `party_artes/resident-magic/README.md`: spell component observations and limits.

No commits are made during this repair batch.

## Remaining gaps

The route and the 30 deferred source rows remain incomplete. Those rows represent
15 distinct resources duplicated across the two discs. The complete inventory and
criteria are in
`local/battle-route-fidelity-20261011/resource_coverage/checklist.md`.

- Vidarr's story controller enforces a half-health floor; the event that releases
  it is not implemented. Admitting a generic controller would be incorrect.
- Botta's controller and one target-selection mode remain unresolved.
- Giant Bee needs independent wing/body playback. Some airborne enemy commands,
  projectile controllers and delayed particle changes remain unsupported.
- Wind Blade, Aqua Edge, First Aid and other applicable early spells
  still need native recipes. Tiger Blade and Tempest need aerial-control work.
- Colette's thrown-weapon trajectory and Demon Fang's projectile/effect recipe
  retain approximations.
- Entry shatter, early camera easing, Eruption caster framing and
  stage darkening remain different. Result camera heading selection and some UI
  motion/tint details are not established.
- Two Colette victory control streams and the shared group-performance template
  remain incomplete. Group performances currently emit their voice without
  participant animations; the idle result is not caused by missing party members.
- BTLskill's two packages are Falcon's Crest and Grand Fall, with missing native
  consumers. They are neither unused data nor aliases. The bounded extraction
  design is retained in `skill_archive/design.md`; it does not close the archive.
- Shadow opacity now uses authored alpha without the unsupported 0.4 multiplier; its exact animated
  footprint and gradient remain unproved.
- Exact random-stream alignment is unproved. Physical controller testing remains
  the previously agreed manual follow-up.
