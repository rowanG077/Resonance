# Asset cooking

`resonance-import cook-all` builds one shared asset library from the two extracted
North American GQSEAF revision 0 discs. It recovers general models, textures,
animations, audio, movies, fields, scripts and databases, including unused content
in supported formats. It publishes the current named layout, including supported
battle resources. Unimplemented native bindings are reported explicitly;
conversion alone does not establish that every original controller is playable.

```sh
resonance-import cook-all --jobs 6 \
  --extracted local/extracted/disc1 local/extracted/disc2 \
  --output local/all-assets
```

Extraction happens once. Cooking reads the extracted filesystem and processes
everything from the supplied discs. There are no field lists, category
selectors or partial-cook commands. Output location and worker count change where
and how cooking runs, never which resources it includes. Conversion uses Rust
codecs without invoking FFmpeg, vgmstream or KTX tools. Reusable GLB, PNG and PCM
WAV writers live in `resonance-asset-writer`; original formats and game-specific
conversion belong in `resonance-import`.

Format conversion uses signatures, archive layouts and source declarations, not
individual field or model recipes. Native table addresses and layout profiles
remain specific to the supported game revision. Runtime preparation also retains
original native presentation rules such as title composition and story-dependent
menu data. Figurine placement and conditional monster node scales are maintained
as SymphoniaScript content. A behavior table selects named catalogue records and
parameterless entries; ordinary string/integer constants pass directly to native calls.
The engine applies only generic
instance-local transforms and flag queries. These rules never change shared
geometry or animation clips. Other native preparation remains game-specific;
the entire importer is not independent of the original game's organization.

The named-layout migration is tracked in [plan.md](../plan.md). Its implemented
editing contracts are described in [Editing named content](editable-content.md).
The complete-generation publication path is implemented; the fresh full cook and
final integration measurements remain unverified. Movies, startup
assets, game-over and credits now publish directly into named directories. The
credits recording decodes directly into its shared music owner. Movie IDs come
from native executable tables; container facts come from the resulting media.
Boot/title textures come directly from native TPL data. Named textures publish
as PNGs; authored mip levels have an adjacent `.texture.json` that preserves
their order and color interpretation. Replacement art can opt into automatic
mip generation as described in [the authoring guide](editable-content.md).
World preparation passes
a typed object into named publication instead of serializing and reloading a
world package or inventory. Field jobs likewise retain typed scenes and artwork
for their final named writer; field metadata/preload staging has been removed.
Party costumes, victories, event actors and arenas now publish through typed
native preparation into named owners. Victory selection uses explicit character
and costume IDs. Their old numeric metadata and inventory readers are removed.
Shared battle rules, HUD artwork, resident effect banks and stored spell/skill
scenes also publish directly. Native palette bindings are passed in memory;
the stored-scene semantic cache and its versioned catalogue are removed.
Unreferenced skill packages have a separate scoped mapping.
Enemy rules, rigs, shared Monster Book models, attachments and effect banks now
pass directly from native preparation to named publication. Equipment uses the
same direct path. The old enemy/model records and combined equipment catalogue
are no longer publication inputs. Shared menu tables, preview books, dialogue
artwork/fonts, session data, localized text and skits also publish from prepared
values. Caller-bound field geometry carries its physical recipe and optional
palette catalogue in memory to its named owner.
Title audio publishes named WAV music and editable menu programs directly from
disc data.
The named music and effect publishers also compile their inputs directly,
without staged audio packages. Selected speech lines decode from the original
archives directly to named WAVs. Named field, battle and world audio selections
also come directly from native scripts and tables. The production coordinator
uses these native audio publishers. Dynamic resources now publish native scopes
through `resources/standalone/mappings.json` and `resources/groups/mappings.json`,
with physical dependencies, explicit absent/empty entries and binding limitations.
The generic physical loop now writes source-named owners and scoped mappings;
its audio writes current programs and WAVs. The generation installer separates
authoring files from converter scratch and external source diagnostics. Runtime
music uses only the resulting WAVs. Effects
remain editable programs with named instrument samples; the current field or
battle supplies their playback environment. Original effect declarations with
missing source dependencies appear in the external `reports/audio-effects.json` and receive
no playable mapping.

## Audio interpolation

Cooking generates Resonance's interpolation filters in Rust and includes their
bytes in the cooked audio tables. No external coefficient file is required.
Diagnostic previews use the same generated filters. Their hash is part of audio
internal render identities, so changing the generator invalidates cached audio recipes.

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
estimates are admission controls, not process RSS limits. `--memory-budget-mib`
sets that budget (2048 MiB by default), independently of `--jobs`. If a complete
library's retained dependencies exhaust the budget, rerun the full cook with a
larger budget suited to the machine, for example `--memory-budget-mib 8192`.
The budget changes scheduling admission, never which resources are cooked.

An output session coordinates final publication across workers and holds an
exclusive lock against independent cooks targeting the same directory. Equal
content shares a destination; different content at the same path fails. Atomic
installation prevents partial final files. The coordinator retains identities
and status, not decoded payloads.

Every invocation requires a fresh, absent or empty output directory and cooks
the complete input again, including media. Existing content and edits are never
silently replaced. There are no
incremental receipts, persistent caches or existing-file shortcuts. Identical
inputs and outputs share work within the current invocation. Movies run serially
to bound temporary disk use.

Each distinct executable is parsed into one set of catalogues. Embedded table
publication, menus and session data consume those same values. Field actors,
figurines and monster previews bind typed decoded packages through the same
model path.

## Library and field preparation

The production entry point now assembles the named generation described in
[editable-content.md](editable-content.md). This publication change has not yet
been exercised by a fresh complete cook; see [plan.md](../plan.md) for the remaining
implementation and acceptance work.

- `content.json` identifies the current layout and supported game revision.
- `fields/mappings.json` selects named field directories containing scene,
  collision, actor, dialogue, script and audio documents.
- `audio/music/` contains recorded WAVs and exact loop definitions;
  `audio/speech/` contains named WAVs; `audio/effects/` contains editable programs
  using WAVs under `audio/instruments/`.
  Songs and sound banks have shared names and mappings, independent of their
  original disc locations. The physical-source pass compiles song setups for
  coverage; the central named publisher owns WAV rendering.
- `movies/mappings.json` selects named movie documents and containers.
- `meshes/characters/` and `meshes/equipment/` own deliberately shared models.
- `ui/title/`, `ui/boot/`, `ui/credits/` and `ui/game-over/` own screen content.
  Title backgrounds remain ordinary fields 474 and 531, selected by local time.
- `world/` retains both continents, every terrain coordinate and story alternate,
  along with movement, encounters, actors, skies, dialogue and cinematics.
- `scripts/` contains ordinary editable SymphoniaScript and named node constants.
- `resources/physical/<original-path>` preserves other converted source
  resources with native member mappings and explicit empty slots.

Disc identity is confined to import inputs and diagnostics. Resource names,
documents and mappings in the installed library contain no disc ownership or
disc selection. Repeated declarations of the same game resource share one owner;
conflicting converted values must be resolved in the importer.

Import diagnostics stay in the sibling working directory printed at completion.
Its `reports/import-sources.json` accounts for source files, and
`reports/coverage.json` records failures, exclusions and support limitations.
Parsed source tables live under `reports/native-data/`. None of this directory
is installed in the asset library. Source-accounting paths beginning with
`reports/` refer to these diagnostics; other paths refer to the asset library.
Original-data fixtures take the library in `RESONANCE_TEST_ASSETS` and this
external reports directory in `RESONANCE_TEST_IMPORT_REPORTS`.

Cooking requires a fresh destination. The importer builds in a separate sibling
staging directory, selects terminal authoring files through an internal export
list, and copies them into a clean candidate with independent files for distinct
owners. It follows the current documents from that candidate before installing
it with a directory rename. Converter scratch and the export list are not part
of the library. Successful cooking retains only the external diagnostics after
discarding conversion scratch. A failed cook retains its incomplete staging directory separately
and reports its location; it never overwrites an existing library or direct edits.
No old source-index or preload-manifest alias is published.

The same `cook-all` run prepares startup, menus, skits and the complete field
catalogue. Field IDs select source records, not different cooking implementations.
The source declarations determine models, animation banks, voices, sound banks
and movies to bind; they never limit which assets are converted.
Script-only fields use the same path with empty scenery; setup does
not borrow the classroom's assets. Final media descriptors may be read for
validation and timing. Menu icons and symbols share decoded banks; Monster List
and figurine previews use the same model preparation as field actors. Executable
data is parsed into named records, not retained as executable slices for the
player to interpret.

Every AFS member, physical movie audio track, discoverable sound bank and song
arrangement is converted independently of field references. Missing dependencies
in the original sound data remain explicit unavailable records; they are not
replaced with silence. Unsupported enemy bindings and sound resources are reported
individually. Native executable code, build metadata and disc headers are
accounted for separately from assets; known embedded tables and artwork are
recovered without publishing executable slices.

Table readers produce semantic records directly, including inactive entries and
meaningful unresolved fields. Fixed-size text slots use the same text references
as pointer tables, with their source bounds checked during parsing. Source
alignment padding and list terminators serve parsing rather than becoming cooked
fields. Validation compares semantic catalogues and prepared menu data with the
original sources and existing baseline.

After a format change, rerun `cook-all`. There is no
compatibility layer for obsolete cooked formats: version mismatches request a
recook.

### Saves after format changes

Only the current save schema and matching gameplay identity are supported.
The current identity schema is 7 and includes current battle and field gameplay
inputs, including field attachment bounds, motion-dependent contacts and enemy rewards.
Obsolete or gameplay-incompatible saves are rejected; start a new game after a
breaking change. There is no save repair or migration tool. Within the current
format, presentation-only edits and resource renames preserve gameplay identity.

Offline checkpoint replay preserves saved preferences; interactive loads apply
the user's `config.toml` preferences.

Stored battle scenes now require scene format 1. The Ray fixed-pattern recipe in
`crates/import/src/battle_scene/spell_recipes.json` translates its verified native
schedule and extracts heading, strike points, camera and color from the original
REL tables. Cooking includes the resulting typed recipe in the scene descriptor
and its normal preparation identity. Preparation resolves the declared contacts
and effects into `PreparedSpell`; transition clocks, TP payment, interruption and
released-effect ownership remain native. Changing this representation requires a
complete cook, including scenes that retain their existing native controllers.

## Authored events

The SymphoniaScript DSL and compiler ship with the cooking system. Authored code
is checked in as readable source and embedded with `include_str!`. Cooking writes
the maintained model-behavior sources and the entire checked-in `scripts/std`
library verbatim. It checks entry types without executing behavior. String node
constants and integer catalogue constants carry native values directly.
`scripts/std/README.md` links catalogues and shared model node libraries.
Unknown model interfaces remain cookable without a standard-library entry.
These sources are ordinary editable content. Edit them directly in the library or
replace the same paths in an enabled mod, then restart the application.

Runtime compiles the effective captured sources during preparation and caches
the resulting programs. Current contents determine cache and gameplay identity;
no stored hash or preload inventory must be repaired. Missing or invalid sources
fail with a source diagnostic; runtime never substitutes embedded defaults. See
the [language guide](symphonia-script.md) for the CLI, project layout and field
entry bindings. Battle-specific programs and preparation remain separate.

## Models and animations

Geometry remains GLB. Published textures use PNGs, with authored mip levels in
adjacent `.texture.json` definitions. Bound animation clips use named `.motion.json`
files with native curves, control points and skeleton indices. Both importer
scratch and published clips use this one JSON schema; the runtime has no binary
motion reader.

Motion clips preserve source times, controls, easing, rotation modes and matrix
channels. Playback evaluates these curves rather than storing sampled animation
frames in GLB. The affine evaluator preserves shear through model hierarchies
and attachments. Secondary motion retains distinct drawing and attachment poses.
Clip paths are captured with their owning resource, so missing clips fail preparation.

Fields use named, disjoint draw stages for scenery and actor body/outline passes.
Missing scenery sections retain their stage instead of shifting later materials
into actor ranges. Field metadata versions reject obsolete ordering layouts.

## Recovery and validation

Use checked parsers and named fields for asset layouts. Keep binary decoding in
the importer and define gameplay behavior in the runtime. Validate input data
without reproducing implementation quirks from another engine.

Finish the coordinated implementation before running final validation. Exercise
the complete cook and the supported field, battle, world, menu,
skit, save, movie and audio routes. Compare fresh native output with verified
Dolphin captures at native resolution, preserving existing thresholds. All
unattended playback stays silent. See the [oracle guide](../tools/oracle/README.md).

Successful conversion establishes coverage only for the declared supported scope.
It does not prove every native controller is implemented or every scene renders
correctly; native late-actor phases remain unsupported by the runtime. Continue
recovery when a concrete failure or original consumer proves
a gap. Preserve the independent battle and field fidelity evidence.

Current authoring documents carry explicit schema versions; old aggregate
packages and inventories are not supported inputs. Shop visit groups retain saved
shop IDs; skit resources carry authored skip permission; world landmarks carry
validated entry overrides. Skit exceptions are extracted from checked original
instructions. Shop groups and world entry selection are translated recipes with
external source evidence; entry angles and heights come from the original REL constants.
Runtime callers still constrain skit skipping, and native placement, save admission
and battle lifecycle rules remain authoritative. Recook the complete library when
updating to these formats.

Party profiles contain validated standing-capsule dimensions.
The collider recipe preserves Resonance's maintained body envelopes; these are not
extracted original values, and authored MO/DM reception geometry remains separate.
The required `battle/defense-system.json` declares composition, PA slots,
placement, deployment, repair/revival cycles, messages, sound and camera parameters
for the existing native controller. Repair cycles retain independent orbit counters,
increasing self-revival delays and the core defense ratio; native recovery owns
revival, protection, health caps and feedback.
Its placement/rate constants are extracted from the original REL, while the camera
restriction is an explicitly maintained Resonance setting. Source evidence and
recipe hashes belong to importer diagnostics. Runtime reads the current named
rules and discovers their dependencies without a stored preload inventory.
