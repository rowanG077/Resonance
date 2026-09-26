# Asset cooking

`resonance-import cook-all` builds one shared asset library from the two extracted
North American GQSEAF revision 0 discs. It recovers general models, textures,
animations, audio, movies, fields, scripts and databases, including unused content
in supported formats. Battle semantics are being added to this same pipeline as
part of Milestone 4. It now publishes all original formations and shared enemy
statistics, including repeat-battle variants, common action and projectile tables,
and common/technique effect source banks. Remaining battle resources are
reported explicitly; a successful cook does not establish complete battle coverage.

```sh
resonance-import cook-all --jobs 6 \
  --extracted local/extracted/disc1 local/extracted/disc2 \
  --output local/all-assets \
  --coefficients /path/to/Dolphin/Sys/GC/dsp_coef.bin
```

Extraction happens once. Cooking reads the extracted filesystem and processes
everything from the supplied discs. The production command has no field lists,
category selectors or partial-cook options. Output location and worker count change where
and how cooking runs, never which resources it includes. Conversion uses Rust
codecs without invoking FFmpeg, vgmstream or KTX tools. Reusable GLB, KTX2 and PCM
WAV writers live in `resonance-asset-writer`; original formats and game-specific
conversion belong in `resonance-import`.

Format conversion uses signatures, archive layouts and source declarations, not
individual field or model recipes. Native table addresses and layout profiles
remain specific to the supported game revision. Runtime preparation also retains
original native presentation rules such as title composition and story-dependent
menu data. Figurine placement and conditional monster node scales are maintained
as SymphoniaScript content. A typed manifest selects named catalogue records and
parameterless entries; ordinary string/integer constants pass directly to native calls.
The engine applies only generic
instance-local transforms and flag queries. These rules never change shared
geometry or animation clips. Other native preparation remains game-specific;
the entire importer is not independent of the original game's organization.

## Jobs and publication

The scheduler is a typed dependency DAG. A job receives immutable `Arc<T>` inputs
through a resolver restricted to its declared dependencies. Transformations use
copy-on-write; writers publish completed assets. Decoded intermediate models,
palettes and tables are not serialized and reopened between computations.

Invalid dependencies fail before execution. A failed producer blocks its
consumers while independent jobs continue. Ready consumers take priority,
and decoded payloads retire after their last consumer. Model preparation uses
ordinary sequential functions inside each package job; only the main graph
schedules parallel conversion.
The scheduler also supports estimated scratch and retained-output budgets; these
estimates are admission controls, not process RSS limits.

An output session coordinates final publication across workers and holds an
exclusive lock against independent cooks targeting the same directory. Equal
content shares a destination; different content at the same path fails. Atomic
installation prevents partial final files. The coordinator retains identities
and status, not decoded payloads.

Every `cook-all` invocation cooks the complete input again, including media. There
are no incremental receipts, persistent caches or existing-file shortcuts. Identical
inputs and outputs share work within the current invocation. Movies run serially
to bound temporary disk use.

During battle development, the temporary ignored helper under
`local/dev-battle-cook/` calls these same production table publishers for selected
groups. Use it throughout battle implementation, including integration checkpoints.
Do not run full cooks for battle development:

```sh
cargo run --offline -j1 --manifest-path local/dev-battle-cook/Cargo.toml \
  --target-dir target -- recoil
```

Available groups are `recoil`, `normals`, `profiles`, `enemy-models`, `monster-data`,
`party-models`, `weapons`, `scenes`, `stages`, `ui`, `victory`, `audio`, `game-over`,
`actions`, `projectiles`, `formations`, `effects`, `tints`, `techniques`, `voices`,
`items` and `scripts` (existing battle `.sym` sources);
multiple groups may be selected.
After building the helper, source-only edits can use `target/debug/dev-battle-cook scripts`
directly. This copies the maintained files from the workspace, with no embedded
content fallback, and refreshes the same inventories. Rebuild the helper when its
publisher code or list of script paths changes.
The helper requires an existing cooked library, stages selected publications and
refreshes their hashes in field declarations and dependency inventories, including
the changed descriptors' own hashes and byte counts. It shares
the full cook's output lock. Only explicitly selected model groups prepare model resources; table/script refreshes
do not reconvert models, movies, audio or field assets.
The explicitly supported `normals`, `profiles` and `tints` groups add their REL tables to
source aliases and inventories containing the paired recoil table, matching the
production shared dependency set. `enemy-models` adds all 251 enemy rigs and profiles,
including hashes of their existing shared model dependencies, through the same
production publisher. After a Monster schema change, refresh `monster-data items`
before `enemy-models`, which reads those installed records. Standalone Monster
records remain cooker inputs; runtime menus use the embedded catalogue.
It does not reconvert meshes, textures or clips. Other new paths or memberships require explicit
preparation. Its `dev-battle-cook.json` report describes a partial refresh and
records whether the previous `coverage.json` is present. If an interrupted cook
removed that report, the helper retains its prior identity as provenance without
claiming full-library completeness. It still verifies selected publications and
refreshes their dependency inventories. This local helper is temporary, not a second
production cooking pipeline or an incremental cache.

`scenes` currently publishes Nurse's original stored package. Full cooking uses
the same publisher and records both the spell archive and REL table dependencies.
`battle/scenes/237.json` binds a standard source effect bank, action records, textures,
four model slots and their verified physical dependencies. Existing importers
produce shared meshes, textures and sparse clips; identical source clips share
bytes while each model slot retains independent playback. Authored Nurse control
flow remains in `scripts/battle/nurse.sym` and compiles on load.

Each distinct executable is parsed into one set of catalogues. Embedded table
publication, menus and session data consume those same values. Field actors,
figurines and monster previews bind typed decoded packages through the same
model path.

## Library and field preparation

- `assets/<hash>/` holds converted source packages.
- `meshes/`, `textures/` and `clips/` share final geometry, images and motion clips.
- `audio/` holds decoded samples and playback packages.
- `fields/map-{id}.json`, `fields/map-{id}-audio.json` and
  `fields/map-{id}.preload.json` bind each field's scene, audio and dependencies.
- `movies/{id}.json` binds the movie catalogue to shared converted streams.
- `scripts/` contains immutable authored SymphoniaScript published from embedded sources.
- `data/` and `embedded/` hold parsed records and their provenance.
- `battle/formations.json` preserves every original formation and its common-archive
  source digest. Distinct supplied common archives retain their own formation
  publications under `battle/variants/<hash>/`; equal inputs share a publication.
  The primary catalogue enters each field's verified dependency inventory.
- `battle/effects/{common,techniques}.json` holds every original effect timeline,
  controller declaration, referenced modifier stream and UV row/table from the
  common archive. These are source inputs, without compiled VM instructions.
  Cooking retains controllers independently of runtime support. Loading compiles
  the requested members and rejects unimplemented operations before activation.
  Texture/model/audio preparation is a separate requirement for those members.
  Distinct common archives use the same `battle/variants/<hash>/` prefix, and
  the primary banks enter the shared verified field inventory.
- `battle/projectiles.json` preserves all 26 common projectile templates, including
  inactive operands, unknown selectors and instance storage. The importer retains
  original float bits when an inactive operand is non-finite. This publication uses
  the same common-archive variant prefix and verified field inventory as the other
  battle tables. Battle preparation reads selected rows from the verified snapshot,
  resolves caller-selected hit records from verified action tables, binds effects,
  and checks runtime support before
  activation. Cooking does not generate arte behavior or executable code.
- `battle/effects/tints.json` retains the battle REL's ten element palettes and
  colors, plus twelve actor RGBA tint requests. It shares REL source identity/variants and the verified
  field inventory. Casting selects its tint while loading; scripts own emission
  timing, and particles apply the selected RGB/palette after original modifiers.
  Maintained recovery source selects actor tints through a load-time binding to
  this same verified table.
- `battle/recoil.json` publishes the battle REL's 19 forward/vertical impulse
  pairs, proximity threshold, weight scales and guarded speed through the normal
  battle table cooker. It retains source identity and float bits, is included in
  field dependency inventories, and is loaded from the verified encounter snapshot.
  No reaction controller or other native control flow is generated by cooking.
- `battle/{martial,spell}-actions.json` retains the two common action tables:
  phase descriptors, hit rules/windows, animation records and original command
  records. Both tables preserve all 147 indexed slots, including null entries,
  compact records and unused storage. These are original source-format records,
  not compiled authored programs. The same variant publication and shared
  integrity inventory apply. Unsupported reactions, conditions and controllers
  remain explicit preparation failures; they do not prevent source cooking.
- `battle/normal-actions.json` publishes the nine character groups from the
  battle REL: seven selectors and action bindings per group, independent descriptor
  tables, hit rules, animation/hit rows and original command records. Descriptor
  aliases and unused entries remain distinct, since reach and action selection
  have different consumers. It is published before field inventories, beside the
  shared recoil data. Maintained normal-attack programs remain `.sym` source.
- `battle/party-profiles.json` retains all eleven original party templates, with
  source identity, typed shared traits, casting/attachment operands, effect scale,
  Genis's original chant rows (including their terminal record), and remaining
  uninterpreted bytes. The verified loader supplies existing guard, recoil,
  stagger, stun and movement traits to a session-derived actor candidate. Party
  guard pressure uses prepared maximum HP, as in the original setup. Model,
  equipment, condition and casting preparation still have separate consumers;
  publishing their operands does not establish runtime support.
- `game/techniques.json` publishes the existing DOL technique/learning/Unison
  catalogue for battle loading, shared with the menu importer. TP costs, casting
  additions, recovery and route flags remain original data. The ordinary casting
  loader binds them with party profiles and prepared body clips; maintained
  `.sym` control flow is compiled only on loading.
- `battle/enemies/NNN.json` contains each original enemy profile and primary-body
  skeleton, contact radii, attack groups and attachment slots. Bind-channel presence
  and original bone indices are retained. It also inventories the shared model,
  texture and sparse-motion files; battle preparation verifies those resources for
  the selected enemies without adding their payloads to every field snapshot.
  The Monster Book remains the shared source of statistics and scene bindings.
- The `overworld-tiles` catalogue binds every terrain coordinate and available
  alternate to its converted package using the original path templates and axis
  labels. Runtime story state selects between those alternatives.
- `sources.json` maps source identities to completed publications. Disc labels
  appear in provenance, not in separate asset trees.
- `coverage.json` accounts for processed, failed and intentionally excluded
  resources. Unknown formats remain errors.

The previous source index and coverage report are invalidated before discovery.
The source index is published only after success. Field finalization uses the
current run's successful field jobs, never a scan of old output directories.

The same `cook-all` run prepares startup, menus, skits and the complete field
catalogue. Field IDs select source records, not different cooking implementations.
The source declarations determine models, animation banks, voices, sound banks
and movies to bind and preload; they never limit which assets are converted.
Script-only fields use the same path with empty scenery; setup does
not borrow the classroom's assets. Final media descriptors may be read for
validation and timing. Menu icons and symbols share decoded banks; Monster List
and figurine previews use the same model preparation as field actors. Executable
data is parsed into named records, not retained as executable slices for the
player to interpret.

Every AFS member, physical movie audio track, discoverable sound bank and song
arrangement is converted independently of field references. Missing dependencies
in the original sound data remain explicit unavailable records; they are not
replaced with silence. Enemy metadata, base/variant combat statistics and model
resources share the Monster Book preparation path. Their remaining behavior,
effects and embedded sound banks still appear in the battle backlog.
Native executable code, build metadata and disc headers are
accounted for separately from assets; known embedded tables and artwork are
recovered without publishing executable slices.

Table readers produce semantic records directly, including inactive entries and
meaningful unresolved fields. Fixed-size text slots use the same text references
as pointer tables, with their source bounds checked during parsing. Source
alignment padding and list terminators serve parsing rather than becoming cooked
fields. Validation compares semantic catalogues and prepared menu data with the
original sources and existing baseline.

After a format change, republish the affected assets. During battle development,
use the targeted helper described above; full cooks are forbidden. Ordinary
complete-library publication uses `cook-all`. There is no compatibility layer
for obsolete cooked formats: version mismatches request a recook.

## Authored events

The SymphoniaScript DSL and compiler ship with the cooking system. Authored code
is checked in as readable source and embedded with `include_str!`. Cooking writes
the maintained model-behavior sources and the entire checked-in `scripts/std`
library verbatim. It checks entry types without executing behavior. String node
constants and integer catalogue constants carry native values directly.
`scripts/std/README.md` links catalogues and shared model node libraries.
Unknown model interfaces remain cookable without a standard-library entry.
These are immutable cooked resources;
their hashes and bytes enter the normal field preload inventory.

Runtime compiles the verified source snapshot during preview preparation and
caches immutable programs. Missing or modified cooked sources fail the normal
integrity checks; runtime never substitutes embedded defaults. Change the
checked-in sources, rebuild and recook to publish updates. Mod overlays are
out of scope for now. See
the [language guide](symphonia-script.md) for the CLI, project layout and field
entry bindings. Authored battle programs follow the same source publication and
load-time compilation contract; their encounter preparation is tracked in
[battle status](battle-status.md).

## Models and animations

Geometry remains GLB. Texture conversion writes KTX2 directly from decoded pixels,
preserving original mip levels. Bound animation clips use content-addressed
`.motion` files and the generic `resonance_content::animation` schema.

Motion clips preserve source times, controls, easing, rotation modes and matrix
channels. Playback evaluates these curves rather than storing sampled animation
frames in GLB. The affine evaluator preserves shear through model hierarchies
and attachments. Secondary motion retains distinct drawing and attachment poses.
Clip paths participate in preload dependencies, so missing clips fail admission.

Battle party preparation retains the nine standard bodies and their separate
battle motion banks in the same cooking graph. It reuses decoded geometry and
sparse curves, preserving original motion slots, including gaps. Shared field
inventories contain the party and enemy model descriptors; each descriptor lists
the meshes, textures and clips verified when that model is selected for battle.
These payloads are not loaded for every field. Both original party body and motion
sources list the resulting descriptor in `sources.json`.

The shared weapon bank similarly preserves original resource slots and holes,
primary/outline/extra model layers, local sparse clips and contact rigs. Its bank
descriptor is shared; equipment selection verifies only the chosen model payloads.
Weapon trails and owner-linked playback remain battle implementation work.

Fields use named, disjoint draw stages for scenery and actor body/outline passes.
Missing scenery sections retain their stage instead of shifting later materials
into actor ranges. Field metadata versions reject obsolete ordering layouts.

## Recovery and validation

Use the original loaders and consumers in
`/home/rowan.goemans/Documents/engineering/Tales-of-Symphonia-decomp` to establish
layouts and behavior. Prefer checked shared parsers and named fields. Preserve
meaningful unresolved storage; do not hide failed decoding with asset-specific
fallbacks. `read::record!` keeps original field layouts beside decoding expressions.

Validate changed readers on representative original content before a full cook.
At integration checkpoints, exercise both discs and the supported field, menu,
skit, save, movie and audio routes. Compare fresh native output with verified
Dolphin captures at native resolution, preserving existing thresholds. All
unattended playback stays silent. See the [oracle guide](../tools/oracle/README.md).

Successful conversion establishes coverage only for the declared general scope.
It does not prove every native controller is implemented or every scene renders
correctly; native late-actor phases remain unsupported by the runtime. Continue
recovery when a concrete failure or original consumer proves
a gap; battle preparation has its own implementation and validation boundary.
