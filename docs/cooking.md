# Asset cooking

`resonance-import cook-all` converts the supported North American GameCube
GQSEAF revision 0 data into one editable asset library. The command uses
`crates/importer`; it has no legacy fallback.

```sh
cargo build --release -p resonance-import-cli
target/release/resonance-import cook-all \
  --extracted local/extracted \
  --output local/all-assets
```

The extraction root contains `disc1/sys/main.dol` and `disc1/files/`. Disc 1 is
primary. The optional `disc2/files/` supplement needs only five additional event
speech archives and four story movies, listed in the plan. Shared assets cook
once; a complete second extraction is unnecessary. The disc-1 opening wins.
Missing required supplemental files fail with their specific path.

## Conversion and publication

Each ordinary category function reads its native input, constructs the final
document or standard media and writes its named output. The coordinator calls
owners in explicit dependency order. Structured documents use their final editable
types; media use the existing Rust readers and encoders. No job graph, publication
registry, hashes, receipts or intermediate asset library is involved.

Use a fresh destination. Files are created without overwriting existing files.
On failure, completed files remain available for inspection and focused recovery;
the command reports the failing owner. A partial output is not a playable library.
Keep an existing working library and mod edits separately. The command does not
stage and automatically install another generation.

Focused development conversions use the replacement owner's CLI:

```sh
cargo run -p resonance-importer -- field-core local/extracted local/field-review 330
```

That writes one field's script, messages and collision. It needs the appropriate
shared dependencies for other selected owners. Do not rerun completed music,
speech or movie conversion to check an unrelated fix.

Temporary disk data is allowed only when a converter needs it. Movie decoding
retains reordered video frames and audio locally for the Rust MKV encoder, then
removes the scratch directory. It does not publish an intermediate movie format.
Cooking uses in-process codecs; FFmpeg, vgmstream and KTX command-line tools are
not required. Audio interpolation filters are generated in Rust.

## Library and field preparation

The [asset guide](cooked-library.md) describes the editable contract:

- `content.json` carries the format version and semantic game name.
- Category `mappings.json` files connect machine IDs to named resources.
- `fields/` contains named field definitions, scenery, actors, collision,
  messages and scenario bytecode. Shared meshes and animation GLBs live once
  under their natural owners; fields select and bind them.
- `audio/music/` contains shared WAVs with exact loop boundaries. Speech uses
  named WAVs; effects use WAVs or compact programmable definitions.
- `movies/mappings.json` points directly to named MKVs.
- Models and animations use GLB; textures use KTX2, including authored mip levels.
- `battle/`, `game/`, `ui/` and `world/` hold their own editable definitions.
- `scripts/` contains maintained behaviors and generated known-ID definitions.

References inside documents are relative to the library root. Neither paths nor
documents carry source/disc ownership, content fingerprints or source inventories.
Diagnostics and independent comparison evidence stay outside the library.

Native bytecode keeps its machine IDs. Resource requests resolve through mappings.
The explicitly maintained gameplay corrections (Shadow following, Gnomette fade,
wood cleanup, boulder retries and Niflheim bookkeeping/visibility) are preserved as
small authored corrections after native extraction. These corrections do not
rewrite resource IDs or modify modded cooked scripts at runtime. The two obsolete
checked-VM workarounds are replaced by source-neutral VM semantics.

The runtime reads current cooked formats directly. Original-format asset readers
and compatibility adapters are not runtime fallbacks. Later numbered mod layers
and Steam input support are recorded requirements, not a new cooker framework.

### Saves after format changes

The current save format is version 10. Asset and gameplay edits do not invalidate
saves. Loading checks the save version and prepares the resources needed by the
saved state from the current library. No asset fingerprint, global compatibility
scan, save repair or migration tool exists.

Offline checkpoint replay preserves saved preferences; interactive loads apply
the user's `config.toml` preferences.

Stored battle scenes now require scene format 1. The Ray fixed-pattern recipe in
`crates/importer/src/battle_scene_package.rs::ray_recipe` translates its verified native
schedule and extracts heading, strike points, camera and color from the original
REL tables. Cooking includes the resulting typed recipe in the scene descriptor
and its normal resource preparation. Preparation resolves the declared contacts
and effects into `PreparedSpell`; transition clocks, TP payment, interruption and
released-effect ownership remain native. Changing this representation requires a
complete cook, including scenes that retain their existing native controllers.

## Authored events

The SymphoniaScript DSL and compiler ship with the cooking system. Authored code
is checked in as readable source and embedded with `include_str!`. Cooking copies
those behaviors and generates their standard definitions from the explicit enums
in `symphonia-constants`. The same declarations supply initial resource names;
there are no checked-in generated `.sym` files or parallel ID/name catalogues.
Model node names live in the GLBs; `resonance-script nodes MODEL.glb` generates
optional reference modules. Cooking generates only the few reviewed behavior
aliases stored alongside the ID declarations, with no skeleton hash registry. See [the standard definitions](../scripts/std/README.md).
Cooking checks entry types without executing behavior. Unknown resource IDs
remain usable through raw numeric service boundaries.
These sources are ordinary editable content. Edit them directly in the library or
replace the same paths in an enabled mod, then restart the application.

Runtime compiles the effective captured sources during preparation and caches
the resulting programs. Current sources are compiled for use; they do not determine save compatibility.
No stored hash or preload inventory must be repaired. Missing or invalid sources
fail with a source diagnostic; runtime never substitutes embedded defaults. See
the [language guide](symphonia-script.md) for the CLI, project layout and field
entry bindings. Battle-specific programs and preparation remain separate.

## Models and animations

Geometry remains GLB. Published textures use KTX2, including their authored mip levels. Animation clips use standard glTF 2.0
animation in GLB, with named rigs and one action per file. The publisher shares
converted clips through direct readable paths; fields keep their own selections.
Generic animation banks contain target bindings and GLB paths, not native curves.

Source-specific curve decoding, easing and time conversion stay in the importer.
Adaptive LINEAR/STEP keys preserve source curves through Blender's ordinary
importer; runtime also accepts standard CUBICSPLINE channels. Affine source keys
use a hierarchy of two standard TRS nodes to preserve shear. Rendering and
attachment queries use the same standard animation reader. See the
[animation authoring guide](motion-format.md) for the verified editor workflow.
Old motion JSON and its runtime reader are removed; the current version is declared by `resonance_content::library::VERSION`.

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
source comparison evidence belongs outside the cooked library. Runtime reads the current named
rules and discovers their dependencies without a stored preload inventory.
