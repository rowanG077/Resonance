# Strategy combat fidelity

Combat strategies use named rules cooked during asset import. Self-contained
battle tests check strategy behavior; game lifecycle and persistence checks cover
orders, refresh and learned enemy information. Original-game capture tools remain
available for local investigation.

## Editable rules and runtime ownership

`battle/party/rules.json` owns the shared policy data:

| Setting | Consumer |
| --- | --- |
| `tp_reserve_percent` | Offensive technique admission |
| `healing_threshold_percent` | Which injured allies need healing |
| `buff_max_level_advantage` | Whether support buffs are considered |
| `idle_tp_reserve_percent` | Whether ranged companions wait instead of using normals |
| `target_strategies` | Handler for each saved action-strategy choice |
| `companion_positioning` | Idle arrival, formation radius, occupancy and approach timeout |
| `companion_normal_selection` | Airborne target height and Colette's close melee gap |

The two TP tables have separate consumers and remain independently editable even
though their imported values currently agree. Shared choice enums and numeric
decoding live in `resonance_content::strategy`; resolving Default retains whether
the saved position was explicit. Menu choices and combat handler
names are separate: changing a handler changes combat behavior without renumbering
saved instructions. Byte addresses and executable references stay out of cooked
data. [The cooked-library guide](cooked-library.md#party-rules) describes mod edits.

Technique capabilities supply `ai_target_scope` and `summon`. Admission scope is
independent of effect area: an `either` technique can participate in both single
and group visits. Normal tables supply primary and secondary combo checkpoints.
Encounter preparation resolves saved choices and imported definitions into a
validated runtime policy. Strategy refresh prepares every replacement before
installing any of them. `companion_choices` exposes the raw saved array for saves
and diagnostics;
combat reads the cached target handler, opening pool, thresholds and combo rules.
Default versus explicit positioning is resolved once at these boundaries.

The companion selector borrows Battle immutably and receives the cached policy
and a mutable random stream. It uses the existing read-only combat admission
methods. Separate opening and combo entry points return decisions without changing
actors, targets, controller state or movement. Only an opening constructs the
target roster;
approach reselection reads actor height directly and evaluates posed geometry
only when Colette's close-range rule needs it. Opening decisions include
the ordered target visits and distinguish the opponent used for movement from the
action's recipient. Battle applies these decisions through the ordinary approach
and execution path. Support visits stay ordered: revival, healing, physical cures,
magical cures, buffs, offense, then a normal or formation wait. Character exceptions
and probability gates remain small named functions.

Shared Battle admission methods own learned techniques, affordability, aerial
restrictions and combo-family legality. Autonomous selection adds enabled choices,
TP reserves, position preferences and character rules. Explicit commands keep their own path
through that shared admission and execution. Controller timing, contact, payment,
effects and persistence remain outside selection.

These fields require cooked layout 102. Targeted `battle-party-rules`,
`techniques`, `battle-enemy-rules` for enemy 180, and `descriptor` cooking is
sufficient to update the earlier installation; artwork and audio need no conversion.
Installation receipts and rollback copies remain under `local/strategy-fidelity`.

## Behavior that must survive refactoring

- **Targets:** all eight handlers are imported. Attack Same retains the original
  formation leader's target after that leader falls. Protect Friend includes
  fallen allies and chooses later roster entries on HP and distance ties. Block
  Magic uses the acting character's chant-history latch. Scatter samples the full
  opposing roster up to sixteen times before nearest fallback. A normal fallback
  makes a second target-selection visit.
- **Position:** Hold suppresses new autonomous actions and healing; explicit
  commands and committed combos remain available. Skills/Magic can fall back to
  normals. An idle companion keeps its current spot when its requested formation
  point is inside the allowed radius. Otherwise, clamping and occupancy determine
  idle movement. Post-action formation returns retain their destination even
  when occupied. Approaches time out after the imported limit.
- **Opening pools:** explicit Long-Range excludes spells, and explicit Long-Range
  Magic requires them. Raw Default retains a broader ranged pool. Don't Pursue
  can sample a melee arte and fall back to a normal. Opening restrictions do not
  filter committed combo candidates.
- **Support:** revival precedes healing, then physical cures, magical cures and
  buffs. Revival prioritizes the preferred player-controlled ally (or formation
  leader with everyone on Auto) when fallen, otherwise sampling the fallen pool.
  Healing ties choose later allies. Group buffs have priority even when
  only one living ally lacks the buff; single admission chooses an ally before
  checking need. Healing and cures are independent of offensive TP reserves.
  Buff admission compares integer roster-average levels fixed at entry, with
  the difference clamped to -8..8; equality is admitted.
- **Combos:** autonomous selection occurs at authored checkpoints. Recovery
  completion does not revisit a checkpoint at the action's final age. Explicit
  commands can chain later in the open window, subject to the same contact,
  target, TP and chain eligibility checks. Do Not Use suppresses
  idle techniques, but Skills/Magic overrides the reserve at a committed combo
  boundary. Earlier normal stages below level 30 have a
  `1 / (floor(level / 2) + 1)` opportunity. Final-stage reduced use has a 25%
  opportunity at or below reserve. Colette's throws have separate 50%
  opportunities around the ordinary gate. Preserve short-circuit draw order and
  bounded spell rejection, rather than replacing them with a different random
  sampling scheme.
- **Characters:** Colette's normal selection depends on position, flight,
  target height, body gap and the 75% HP boundary. Her throws can chain early.
  Sheena autonomously selects only summons during Over Limit; explicit commands
  bypass that restriction. Kratos and Zelos use normal openings at 30% HP and
  above, and can open with melee artes below that boundary.

First neutral, thrust and low normals retain faster approach momentum; later
links assign their authored speed. Losing that momentum can make a throw miss
and let a later idle arte resemble a successful combo.

## Learned enemy information

Weakness preference requires known weakness information and an entry-level
advantage at most 2. Advantage 3 leaves ordinary spell candidates available.
Committed combos ignore that preference. Absorption and immunity are excluded
only after resistance information is known; an uninformed companion can cast
into immunity.

Knowledge starts from authored profile flags and the Monster Book. Contact and
Magic Lens update live actor knowledge. The result ledger separately records
contact discoveries so inherited information and complete scans do not invent
contact flags in the saved book. Live knowledge belongs to an actor; saved
knowledge belongs to the monster type and initializes the next encounter.
Legacy saves default the two new fields to unknown, while existing complete scans
still supply both kinds of information.

All 251 authored enemy profiles were checked. Ktugach starts with weakness
information; the others start without these flags. Named profile traits preserve
that authored information when applying saved knowledge or refreshing a profile.
The observed book transitions are `0x01` → `0x41` for a weakness and `0x01` →
`0x81` for immunity. The save/next-encounter integration check verifies both.

## Local observations and checks

The tools in `tools/oracle/strategy_*.py` can capture original-game behavior for
local investigation. They accept `--disc`, `--state`, `--output` and, where needed,
`--assets`; use `--help` for suite and character selection. Keep reports under an
ignored local directory. Repository tests do not load capture reports.

Run the self-contained battle checks and game test builds from the workspace root:

```sh
CARGO_INCREMENTAL=0 cargo test -p resonance-battle --lib
CARGO_INCREMENTAL=0 cargo test -p resonance-game --lib --no-run
CARGO_INCREMENTAL=0 cargo test -p resonance-game --features test-support --lib --no-run
```

The optional cooked-library test
`cooked_elemental_contact_knowledge_survives_save_and_next_encounter` checks contact
knowledge through result persistence, saves and the next encounter without capture
reports. Strategy lifecycle checks cover suspension, atomic refresh and cancellation.
