# Arte visual catalogue

Generate pending visual work directly from the current cooked libraries:

```sh
python tools/arte-visuals/catalogue.py \
  --gamecube local/gc-arte-reference104 --steam local/steam-importer-cooked \
  --legacy-manifest tools/oracle/cases/battle-artes.json \
  --output local/arte-visuals/catalogue104.json
python -m unittest discover -s tools/arte-visuals -p 'test_*.py'
```

Both libraries must have `content.json` matching the current layout version from
`crates/content/src/library.rs` (104). Older runtime-format Unison dumps are
rejected; this generator reads the current named documents only.

The JSON has `schema_version`, `cooked_layout_version`, `libraries`, `summary`, and `cases`. Each case has
an edition-prefixed `id`, `category`, `display_name`, and a renderer request
`binding`: `{character, arte}`, `{combination}`, or `{enemy, row}`. Unison cases
use their ordered position plus one as the runtime combination ID, matching
`battle_rules::unison::Unison::prepare`. They retain the named scene and every
`recipe_variants` entry with its recipe index, named `arte_keys`, and matching
group IDs in `artes` resolved from the Unison document's own `mappings`.
Participant counts come from the first recipe or the explicit dormant count,
including combinations with no ordinary recipe. Variants have their own IDs,
`{combination, recipe}` binding, and semantic recipe counterpart candidates, so
added recipes within an existing combination stay visible. Party case capabilities retain
the authored family; `finisher` is not a reliable count of playable mystics.

Every learned party binding, Unison combination, and authored enemy row remains
listed, including blank names, placeholders, and previously excluded cases.
`legacy_gc_manifest` records old numeric entries and pending setups as metadata.
It never removes work or changes capture/review status. Exclusions never apply
automatically to either current library. Even a GC binding may have changed since
the old manifest; listed names and name matches remain explicit metadata.
Numeric IDs and cooked keys can retain different meanings between
editions: Steam Holy Binds, for example, reuses GC's unused Eternal Chaos key.

The inventory is broader than the focused enemy review. Compare distinct enemy
arte/effect families; exhaustive row checks and repeated basic attacks are
deferred. Row totals describe retained context, rather than the remaining number
of required family comparisons. Keep unrun rows pending instead of turning a
representative family review into passes for every row.

`counterpart` contains candidates, not an approved baseline. Party matching uses
the same character and nonplaceholder name. A blank-name fallback requires the
same character, cooked key, and unchanged rules. Unison matching uses its name and
participant count. Enemy candidates require the same named owner and matching
visual action structure: resolved attacks, referenced projectiles/contacts,
physical-action definitions, pose labels and timing, and effect-bank programs
and templates. Audio commands and hit power are omitted from this comparison;
projectile motion, effect identities, contact geometry, reactions and support
commands are retained. Effect artwork pixels are not compared by the generator.
Row ordinals and `attack-1` alone cannot establish identity. Enemy spell and
physical technique numbers use the native matching namespace. Resolve them
through the Unison placements, including explicit `matches` aliases, using the
same ordered lookup as production battle preparation. A runtime technique with
the same number is not a substitute. Each resolved row records its
`native_technique_binding`; a missing placement stays unresolved and retains its
native number. Multiple candidates stay ambiguous. An unmatched Steam case carries
`steam_only_candidate`; this flags investigation rather than proving no original
counterpart exists. Different names at the same party/Unison binding are flagged.

`capture.status` and `review.status` start independently pending, also for Unison
recipe variants. A matching name, successful execution, old mechanical receipt,
or exclusion cannot establish visual fidelity. `original_gc_reference` starts
missing: a Resonance render using GC assets is not an original-game reference.
Compare Steam-only effects against the shared GC rendering style with an explicit
reviewed reference choice; the generator never assigns an arbitrary baseline.

`scope_status` defaults to `original_game_required`. Only individually proven
entries in [source_slots.json](source_slots.json) receive a different status.
The bounded audit of current party learning bindings proves 27 GC and 13 Steam
bindings have `source_unimplemented_slot`. Original executable/REL hashes, exact native row
and dispatch offsets, consumer instructions and valid neighboring controls show
null native phase-handler pointers. This evidence goes beyond target flags or
old exclusions. Each direct native identity was also checked against its
character's original native learning list. Nonnull native handlers remain
required, including Steam Sword Rain: Tempest and Grand Cross, and GC Grand Fall
despite its old exclusion. Eight derived bindings per edition remain required
with their native identity deferred, rather than guessing a native row from
their logical IDs. Repository-added Ghost Wolf has `repository_authored`, verified by its
authored definition and importer emitter. Matching uses semantic owner/key and,
for Ghost Wolf, its authored action path, so runtime ID changes do not misclassify
another slot. Every case stays listed, carries its evidence link, and preserves
independent capture/review state; none becomes a visual pass. Summary counts
report both all-case and party-only scope statuses.

Regeneration replaces inventory output. Keep capture results and visual review
decisions separately, keyed by case ID and the library/content revision; this
tool does not manage receipts or publish assets. Generated catalogues belong in
local work directories rather than source control.

## Unison capture planning

```sh
python tools/arte-visuals/unison_plan.py \
  --catalogue local/arte-visuals/catalogue104.json \
  --output local/arte-visuals/unison-plan104.json
```

The planner retains all combinations and actual recipe variants. Each planned
`requests` row has the existing recipe ID, ordered `{combination, recipe}` binding,
and one `assignment: [[character, arte], ...]` for the GPU harness. It enumerates
the same placement groups, learning owners, and offensive/menu eligibility as
`visual_artes/assignments.rs`, then chooses the first valid distinct-owner team
in semantic character/action-key order. All alternatives remain in metadata;
there is no requirement to capture every equivalent party permutation. Capture
admission must still validate the proposed team against current prepared rules.

Source-proven null ingredients retain their evidence and block that assignment,
while a valid alternative from the same native matching group stays selectable.
No distinct-owner team and an empty native recipe list remain explicit pending
work. The bounded primary-source proof in `unison_source_exceptions.json` marks
exactly two retained inputs `source_unselectable`: GC Innocent Edge has six zero
native recipe slots, which its native selector skips; Steam Rock Pillar's
Stalagmite + Ground Dasher recipe requires Genis twice, which its native selector
rejects. Explicit Grave matching aliases include Kratos and Zelos, so Rock
Pillar's other three recipes remain required and selectable. Semantic scene/name
and ordered ingredient/native-ID fingerprints guard these annotations. Their
capture/review status stays pending; only `requires_visual_comparison` is false.
The source retains these unusable inputs, so neither importer invents a recipe,
owner, visual pass or omission from the inventory.

Signatures record actual named action tracks, contacts/projectiles, placement,
scene data and ordered body owners. `primary_scene_groups` associates only the
shared finisher scene/body choreography; it cannot stand in for different
ingredient actions or their placement. Complete-recipe signatures retain those
differences. Associations merely identify reusable evidence candidates: every
recipe's capture and review remain pending until its execution/evidence and
visual review are explicitly recorded. No unrun recipe is automatically passed.

## Original GameCube references

`fresh_reference.py` boots the unmodified extracted US disc in one silent Dolphin
process and saves a new classroom checkpoint with its recorded ordinary input.
Dolphin mounts `disc1/sys/main.dol` and the adjacent `sys/` and `files/` directly;
there is no ISO rebuild or old savestate requirement. The patched 2606 binary
works with the development shell's `LD_LIBRARY_PATH` unset. The current state
reader requires that version.

```sh
python tools/arte-visuals/fresh_reference.py \
  --disc /path/to/extracted/disc1/sys/main.dol \
  --dolphin /path/to/patched-dolphin-2606/bin/dolphin-emu \
  --output local/battle-capture-work/fresh-gc-reference
python tools/arte-visuals/source_reference.py \
  --catalogue local/arte-visuals/catalogue104.json \
  --reference local/battle-capture-work/fresh-gc-reference \
  --character 1 --character 3 \
  --output local/battle-capture-work/gc-lloyd-genis
```

Every output directory must be new. `--case gamecube:party:1:1` and `--limit N`
bound a pilot. The capture driver reuses one resident Dolphin worker. It selects
current catalogue bindings, uses historical input recipes only for setup, and
retains unsupported bindings as explicit pending work. It does not inherit old
exclusions or run the full mechanical equivalence oracle.

`nativecapture-report.json` uses the same catalogue case IDs, `source_kind:
"dolphin"`, `admitted`, and relative image paths with observed action-relative
ages. Its source provenance pins the fresh state/movie, original executable,
current technique catalogue and every extracted source file. The `disc_sha256`
inside inherited worker metadata explicitly identifies the extracted tree
manifest, not an original ISO hash. Fixture grants, actor arrangement, and the
Remove Blur profile remain recorded; the original source files are unchanged.

The driver keeps eight martial, nine spell, or eleven summon phase PNGs, a small action timeline, admission
observations and cleanup. All-frame dumping is disabled; the observer reads only
identity, TP, phase clocks and owned projectile markers. F9 screenshots are
registered to their first frame-advance VI; a second VI flushes the image. The
Demon Fang pilot produced a pixel-exact match to its earlier continuous capture. Requested identity and a successful execution establish a usable source
capture, not visual parity. Image review stays pending until the actual source
and Resonance images are compared. These are original GameCube executable
renders inside Dolphin; a Resonance render of cooked GameCube assets is a
separate reference kind.

Source visual fixtures declare neutral manual opponents before their first AI
choice, then let the original entry and physics finish. This keeps the reference
opponent grounded without injecting a pose, action, timer, or physics lane.
`neutral-opponents.json` records those data changes. Entry and ready state/movie
pairs remain reproducible; accepted-case intermediate snapshots are removed to
avoid retaining several extra compressed RAM copies per arte.

Summon input fixtures already grant active Over Limit. The neutral settling
period consumes much of its duration, so the source worker refreshes that
already active duration to 1000 guest ticks before the declared summon input.
`active-overlimit.json` records the native address and before/after values; the
active flag is unchanged. Summon references retain later phase images at 128
and 256 ticks to include the spirit presentation.

Regal aerial input uses the observed mode0 and a ready-relative up-stick jump
(polls20–80), then B(polls88–92). `aerial-input.json` records the actual movie.
The bounded diagnostic admitted native121 with mode0; the mode1 attempt stayed
grounded. No airborne pose or velocity is injected.

`source_enemy.py` captures one original enemy family with the same sparse
Dolphin worker. For example:

```sh
python tools/arte-visuals/source_enemy.py \
  --catalogue local/arte-visuals/catalogue104.json \
  --reference local/battle-capture-work/fresh-gc-reference \
  --enemy 0 --case gamecube:enemy:0:0 \
  --output local/battle-capture-work/gc-enemy-pilot
```

For a bounded multi-owner batch, pass `--plan PLAN.json` containing the current
case IDs under `cases[].id`. The batch reuses one Dolphin process across owners;
each owner retains its baseline and provenance. Source fixture failures remain
pending and the next row continues. Worker/infrastructure failures stop the
batch with all unvisited cases still pending. Final reports record actual
elapsed time and worker closure.

It expands only the encounter's actual source packages, verifies row hashes,
and identifies the owner through its native action-table pointer. Controlled
row weights and disabled followups are recorded fixture assistance. Images
require actual action5 and the requested row/animation/command or spell identity.
The authored native distance range determines the declared initial separation.
HP and difficulty setup read the actual native row flags, rather than the old
manifest's incomplete eligibility metadata. Difficulty is selected before
encounter initialization so native stat scaling remains intact; its address and
before/after values are recorded in `difficulty-fixture.json`.
Enemy spell images wait for release; the script does not run a damage oracle or
claim natural admission. A weight change suppresses weighted reselection after
the first observed admission; ordinary native fallback remains unchanged. Later
context/cleanup images can include that fallback and require visual review.

Each owner retains one baseline and its first ready checkpoint. Other accepted
rows retain typed changes and input metadata in `ready-replay.json`; recreate
their byte-checked state/movie with `python tools/arte-visuals/source_enemy.py
replay CASE_DIRECTORY`. Intermediate admission checkpoints are removed after
their images and receipts are complete. All images, source provenance, native
identity proof and per-case `visual-fixture.json` contexts remain.
