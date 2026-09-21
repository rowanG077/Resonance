# Asset cooking

`resonance-import cook-all` builds one shared asset library from the two extracted
North American GQSEAF revision 0 discs. It recovers general models, textures,
animations, audio, movies, fields, scripts and databases, including unused content
in supported formats. Battle-specific preparation is a separate change. Deferred
battle resources are reported explicitly; a successful general cook is not a
claim of complete battle coverage.

```sh
resonance-import cook-all --jobs 6 \
  --extracted local/extracted/disc1 local/extracted/disc2 \
  --output local/all-assets \
  --coefficients /path/to/Dolphin/Sys/GC/dsp_coef.bin
```

Extraction happens once. Cooking reads the extracted filesystem and processes
everything from the supplied discs. There are no field lists, category
selectors or partial-cook commands. Output location and worker count change where
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

Cycles and invalid dependencies fail before execution. A failed producer blocks
its consumers while independent jobs continue. Ready consumers take priority,
and decoded payloads retire after their last consumer. Nested model graphs run
one worker each, keeping `--jobs` a bound on asset conversion concurrency.
The scheduler also supports estimated scratch and retained-output budgets; these
estimates are admission controls, not process RSS limits.

An output session coordinates final publication across workers and holds an
exclusive lock against independent cooks targeting the same directory. Equal
content shares a destination; different content at the same path fails. Atomic
installation prevents partial final files. The coordinator retains identities
and status, not decoded payloads.

File and voice jobs retain final-output receipts in `.cook-receipts/`. Reuse
verifies the published files, including shared resources outside a package. Keys
include original inputs, relevant dependencies, options and the reader executable.
Changed or missing outputs force conversion. This cache contains final-file
receipts, not decoder intermediates. Embedded database passes still run; movies
run serially to bound temporary disk use and verify their own final publications.

## Library and field preparation

- `assets/<hash>/` holds converted source packages.
- `meshes/`, `textures/` and `clips/` share final geometry, images and motion clips.
- `audio/` holds decoded samples and playback packages.
- `fields/map-{id}.json`, `fields/map-{id}-audio.json` and
  `fields/map-{id}.preload.json` bind each field's scene, audio and dependencies.
- `movies/{id}.json` binds the movie catalogue to shared converted streams.
- `scripts/` contains immutable authored SymphoniaScript published from embedded sources.
- `data/` and `embedded/` hold parsed records and their provenance.
- The `overworld-tiles` catalogue binds every terrain coordinate and available
  alternate to its converted package using the original path templates and axis
  labels. Runtime story state selects between those alternatives.
- `sources.json` maps source identities to completed publications. Disc labels
  appear in provenance, not in separate asset trees.
- `summary.json`, `failures.json`, `excluded.json` and `deferred.json` account for processed,
  failed and intentionally excluded resources. Unknown formats remain errors.

The source index is removed when publication starts and replaced only after the
run succeeds. A failed cook cannot leave an old index claiming success.

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
replaced with silence. Enemy archives and their embedded sound banks remain under
the battle exclusion. Native executable code, build metadata and disc headers are
accounted for separately from assets; known embedded tables and artwork are
recovered without publishing executable slices.

After a format change, rerun `cook-all`. There is no
compatibility layer for obsolete cooked formats: version mismatches request a
recook. Editing original assets invalidates their publications and dependent
prepared manifests.

## Authored events

The SymphoniaScript DSL and compiler ship with the cooking system. Authored code
is checked in as readable source and embedded with `include_str!`. Cooking writes
the maintained model-behavior sources and the entire checked-in `scripts/std`
library verbatim. It checks entry types without executing behavior. String node
constants and integer catalogue constants carry native values directly.
`scripts/std/README.md` links catalogues and shared model node libraries; physical
packages link known interfaces through `nodes.md`. Unknown model interfaces remain
cookable without a standard-library entry. Sources are final publications recorded
by reuse receipts, so incremental cooks retain the same symbol coverage.
These are immutable cooked resources;
their hashes and bytes enter the normal field preload inventory.

Runtime compiles the verified source snapshot during preview preparation and
caches immutable programs. Missing or modified cooked sources fail the normal
integrity checks; runtime never substitutes embedded defaults. Change the
checked-in sources, rebuild and recook to publish updates. Mod overlays are
out of scope for now. See
the [language guide](symphonia-script.md) for the CLI, project layout and field
entry bindings. Battle-specific programs and preparation remain separate.

## Models and animations

Geometry remains GLB. Texture conversion writes KTX2 directly from decoded pixels,
preserving original mip levels. Bound animation clips use content-addressed
`.motion` files and the generic `resonance_content::animation` schema.

Motion clips preserve source times, controls, easing, rotation modes and matrix
channels. Playback evaluates these curves rather than storing sampled animation
frames in GLB. The affine evaluator preserves shear through model hierarchies
and attachments. Secondary motion retains distinct drawing and attachment poses.
Clip paths participate in preload dependencies, so missing clips fail admission.

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
