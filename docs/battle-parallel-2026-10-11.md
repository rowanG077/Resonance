# Parallel formation investigation — 2026-10-11

Sixteen independent workers explored formations through Triet Ruins, each limited
to one isolated Dolphin instance. They used binary assets, registered black-box
captures and existing evidence. The coordinator owns the combined build and
publication. No full recook, full test suite or commit was requested or performed.

Evidence lives under `local/battle-parallel-formations-20261011/`; every worker has
a `findings.json`. Successful native loading is distinct from visual fidelity.
Modified source fixtures isolate behavior and do not certify normal encounters.

## Implemented in this batch

| Investigation | Result |
| --- | --- |
| `notice_timing` | Effect controller 25 contains action-name text. Decode it into a notice event, retain its owner, and use the existing HUD. Source fade takes 8 ticks in and 16 out, with a 90-tick lifetime before fade. Removed the invented actor-name prefix. Authored panel artwork remains open. |
| `whip_contacts` | Marker 254 selects a body bone. Twenty-eight source contact samples match selected bone translations exactly, including a changed operand. Reuse native bone contacts. Marker 255 remains unresolved. |
| `beast_command9` | Action command 9 sets braking at operand / 10. Ground movement snaps residual speed at or below 0.55 to zero. Reuse existing movement and attack events. |
| `golem_holds` | Time-zero movement waits for the opening blend. Models resume one update after actor hit-stop ends. Keep the action and effect clocks unchanged. Evidence includes 5/27 and 501/1; 111/27 is not a Golem formation. |
| `ghost_spell` | Fire Ball's first shot follows release by four updates; subsequent shots remain eight updates apart. Ghost hover adds the oscillating displacement to height, reaching about 64.318 from a base of 50. |
| `appearance_overrides` | Formation byte 40 selects a carried weapon variant. Foot Soldier 100 uses the alternate blade in formations 3/26, 10/28 and 11/29. Prepare the additional package and select it for the first carried slot. Other appearance selectors remain rejected. |
| `scenery_arena2` | Distinguish scenery models from their animation bank and bind authored clips using existing stage poses. Scenery advances 0.5 clip frames per update. Arena 2 also needs unresolved layer flag 1. |
| `recurring_arenas` | Schedule recurring arena programs with existing effects. Initial births occur at age zero; later program events follow ordinary particle ordering. Use authored interval and palette bindings. Add literal vector assignments through existing scalar edits. |
| `victory_return` | Use a white victory fade and the measured held camera, zoom and orbit. Face the party toward the observed camera side. Keep result placement and artwork gaps explicit. |
| `early_bosses` | Remove the unconditional rejection of an inert action parameter when its unsupported enabling conditions are absent. Three Ktugach captures and final RAM isolate that parameter. |
| `party_artes` | Remove Lloyd Demon Fang's invented forward movement. Broader melee arte support and exact action completion remain open. |

## Evidence retained without speculative implementation

- `soldier_eligibility`: flag 0x10 suppresses a row after its first execution.
  Interrupted-approach consumption and Vidarr's separate decision policy remain
  unresolved. The independent Ktugach comparison corroborates reuse suppression.
- `followup_choices`: enemy 82 chains group 1 into group 2, skips recovery and starts
  the next action one update after selection. The 0/100 controls and two negative
  group controls establish the handoff. Intermediate probability, multiple-choice
  weighting and eligibility still need a bounded implementation decision.
- `projectile157`: controlled Slime flag changes leave complete observed shots
  unchanged, including bounce and ricochet. Global semantics of the flags remain
  unknown; the proposed mask relaxation is not applied in this batch.
- `triet_arena30`: live flag changes do not affect rendered images and initialization
  overrides failed their effective-write check. Layer flag 1 remains a real blocker.
- `zombie_fidelity`: the native shadow uses approximate combat capsules and is too
  small. A constant replacement radius is disproved; footprints vary with actor
  and pose. Source visual bounds should inform the eventual replacement.

Other open fidelity gaps include action/result artwork, damage and normal-attack
defaults, missing early party artes, result party placement, and correlated random
effect parameters. Arena 31 has an unresolved layer flag; arena 18 has unsupported
effect geometry. Ambient effects during victory are not yet source-validated.
All 30 deferred source entries remain open. This batch does not restore Stage 4
or establish whole-route parity.

## Coordinator validation

The focused importer tests passed. Complete enemy action tables now decode for
66 enemies, with supported policies for 63. These are decoder counts, not encounter
acceptance. The release build and 33 focused tests passed. One new model test
initially omitted model publication; the fixture was corrected and its rerun passed.

Six focused native runs pass with no diagnostics or unprepared reads: Ghost 2/13,
Sidewinder 523/5 through 600 combat updates, victory 1/13, and component fixtures
for arenas 14, 15 and scenery arena 20. Fire Ball release at combat 149 produces
shots at 153, 161 and 169. Arena fixtures use the zombie formation to isolate stage
rendering; they do not certify the actual encounters assigned to those arenas.
Screenshots were inspected for both recurring stages, Sidewinder and results.

Formations 3/26 and 10/28 stop at enemy 100's follow-up selection requirement.
Formation 509/5 now gets past the notice declaration and stops at effect program 5,
operation 18. These were not bypassed. Sound 110 was prepared after the first
Sidewinder run exposed it, and only that affected encounter was rerun.

Published 433 targeted files (about 14 MB) to `local/all-assets`: 75 changed enemy
definitions, 85 effect banks and their descriptors, one carried variant, six
selected arenas, sound 110 and inventories. All 30 deferred source entries remain.
The original files are retained in
`local/battle-parallel-formations-20261011/default-before`.
Full details: `local/battle-parallel-formations-20261011/result.json`.
Physical controller testing remains a manual follow-up.
