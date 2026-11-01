# Asset cooking

`resonance-import DISC_1 DISC_2` converts the supported North American
GameCube GQSEAF revision 0 disc images into one editable asset library. The
command uses `crates/importer`; it has no legacy fallback.

```sh
cargo run --release -p resonance-import-cli -- /path/to/Disc1.rvz /path/to/Disc2.rvz
```

Output defaults to `local/all-assets`, matching the engine's default asset path.
Use `--output DIRECTORY` for another destination. Cooking writes directly into
the target directory and overwrites existing output. There is no prerequisite
`local/extracted` directory.

Disc 1 is primary. Only five additional event-speech archives and four story
movies are read from disc 2. Shared assets cook once and the
disc-1 opening wins. Disc order and the supported edition are checked.

The command temporarily extracts native files because the current decoders use
ordinary file paths, with seekable archives for speech and movies. This is source
input, not an intermediate cooked format. A temporary directory owns these
files and removes them on success or failure; nothing is retained under
`local/extracted` and no extraction cache or freshness check is introduced.

For focused development work, explicit extraction and `cook-all` remain available:

```sh
target/release/resonance-import extract --disc /path/to/Disc1.rvz --output local/extracted/disc1
target/release/resonance-import extract --disc /path/to/Disc2.rvz --output local/extracted/disc2
target/release/resonance-import cook-all
```

`cook-all` uses `--extracted local/extracted` and `--output local/all-assets` by
default. Category commands in `resonance-importer` can reuse that extraction
without reading disc images again.

Importing also includes the [Ghost Wolf modding example](../mods/ghost-wolf/README.md).
Its one technique entry and Lloyd's learning/defaults are added to the
newly generated game documents, and its action, exported animation and effects are
written under `battle/artes/ghost-wolf`. Other imported techniques and characters
keep their generated values. The example guide covers editing its Blender source
and exporting the runtime animation.

Progress is always written to stderr, with a numbered asset-type heading,
indented `i/N jobs` lines and further-indented counters within each job:

```text
[asset type 48/64] Cooking Movies
  Using 10 worker threads for 10 jobs
  [1/10 jobs] Movie: opening
    [300/1500 frames] opening: decoding
```

Each job retains its name and number when running concurrently; log lines can
arrive out of order. Speech counts lines within an archive, field rigs count
models within a field, and movies report decode/encode frames at roughly
ten-second intervals while frames are processed. No verbosity flag is needed.
Each completed pass prints a wall-clock duration with the same asset-type
number and name, for example:

```text
[asset type 39/64] Field scenery and actors took 42.318 seconds
```

A failed pass also prints its elapsed time before the error. Timings include
waiting for that pass's workers. A successful full cook additionally prints its
total elapsed time. To capture your run and extract the timing lines:

```sh
cargo run --release -p resonance-import-cli -- cook-all 2>&1 | tee /tmp/resonance-cook.log
rg 'took .* seconds' /tmp/resonance-cook.log
```

Independent work within a pass uses `std::thread::available_parallelism()` to
detect the CPU allowance and start at most that many workers. Each pass finishes
before the next starts. Parallel passes include field scripts/collision, battle
party characters (each character's costumes remain ordered), arenas, scenes,
skills, enemy rules, field definitions, music tracks and movies. Field source and
scenery conversion, field-local rigs, enemy media and enemy effects also run in
CPU-sized batches. Each batch converts concurrently and applies shared-owner
updates in the original order. Field actor finalization stays ordered, and field
141's shared scenery motion is published before its consumers are prepared.
Opened field archives are reused for actor finalization. Only one batch is held
in memory; there is no temporary cooked library.

Music/movie aliases are resolved before dispatch. Enemy motion and effect media
retain the same first owner as the sequential converter. Native animation
binding caches and other shared-owner passes keep their required ordering.
On failure, new work stops and active workers finish before the error returns.

## Conversion and publication

Each ordinary category function reads its native input, constructs the final
document or standard media and writes its named output. The coordinator calls
owners in explicit dependency order. Structured documents use their final editable
types; media use the existing Rust readers and encoders. No job graph, publication
registry, hashes, receipts or intermediate asset library is involved.

Cooking writes directly to final paths and overwrites existing output. There
is no staged publication or interrupted-cook resume mode; rerun the cook after
a failure. The command reports the failing owner, and files may be incomplete
until the cook succeeds.

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

## Input icons

Game UI cooking exports the full input atlas and its layout to
`ui/controls/input-prompts.ktx2` and `ui/controls/input-prompts.json`, with the
CC0 license beside them. The defaults use 256-pixel tiles rendered from SVG.
The stored KTX2, layout and license are bundled only with the importer.
The viewer requires the cooked files and does not contain an embedded fallback.
The curated atlas contains 117 distinct controls, including L1–L5/R1–R5 and
readable START/BACK labels. It excludes mouse art, grouped keyboard-arrow islands,
unused generic shapes, duplicate aliases and LT/RT artwork. Physical key aliases
share the canonical icon at runtime. Keys and L/R use blue/gold, while A/B/X/Y
use green/red/blue/yellow. Each has a matching pressed frame. Source geometry and
lettering come from the Kenney pack.

All runtime button prompts share this atlas, including the post-battle Cook
prompt, inline manual/EX/item help, and script-created controller-help sprites.
The historical `ui/controls/gamecube.json` name preserves the script's 32 image
indices; newly cooked banks contain only the three window ornaments. Button
slots resolve the current keymap and active device at draw time, including their
pressed states. Original controller images are neither loaded nor required.
Menu and battle atlas cooking clears obsolete controller pixels to prevent
accidental reuse.

To add just these files to an existing library:

```sh
cargo run -p resonance-importer -- input-prompts local/extracted local/all-assets
```

Like other cooking steps, this overwrites existing output. Edit the KTX2
in place after cooking, or override it from a mod directory. The JSON's `size`
must match the image. `icons` and `pressed` map each name to its normal and pressed
`[x, y, width, height]` pixel rectangles. A mod can
change those rectangles or add icons for additional keys. Keep the `button`
fallback. Runtime preparation reads both files through the normal layered
library; missing or invalid files report an error. Add these files to older
libraries using the cooking command above.

The final composed SVG is stored at `resources/input-prompts/atlas.svg` alongside
the ready-to-cook KTX2 and layout. It is below 10 MB and uses normal Git storage.
Ordinary builds and cooking do not run SVG tooling. Composing just the SVG
requires only `python3 tools/build_input_prompts.py --svg-only` and leaves the
runtime texture/layout pair unchanged. Full generation rasterizes the composed
SVG once and publishes the SVG, KTX2 and layout after conversion succeeds; it
requires ImageMagick with librsvg and Rust, without Pillow. The viewer's normal
image loader owns texture decoding and errors; atlas preparation does not
decompress or specially validate the KTX2.
