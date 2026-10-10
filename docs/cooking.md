# Asset cooking

`resonance-import cook-all` builds one shared asset library from the two extracted
North American GQSEAF revision 0 discs. It recovers general models, textures,
animations, audio, movies, fields, scripts and databases, including unused content
in supported formats. Battle semantics are being added to this same pipeline as
part of Milestone 4. It now publishes all original formations and shared enemy
statistics, including repeat-battle variants, common action and projectile tables,
and common/technique effect banks. Remaining battle resources are
reported explicitly; a successful cook does not establish complete battle coverage.

```sh
resonance-import cook-all --jobs 6 \
  --extracted local/extracted/disc1 local/extracted/disc2 \
  --output local/all-assets
```

Extraction happens once. Cooking reads the extracted filesystem and processes
everything from the supplied discs. The production command has no field lists,
category selectors or partial-cook options. Output location and worker count change where
and how cooking runs, never which resources it includes. Conversion uses Rust
codecs without invoking FFmpeg, vgmstream or KTX tools. Reusable GLB, KTX2 and PCM
WAV writers live in `resonance-asset-writer`; original formats and game-specific
conversion belong in `resonance-import`.

Format conversion uses signatures, archive layouts and asset declarations.
Binary offsets and layout profiles belong to the importer. Runtime preparation
consumes named resources and settings, with independently maintained gameplay
and presentation rules. Figurine placement and conditional monster node scales
are maintained as SymphoniaScript content. Typed manifests select catalogue
records and script entries; instance transforms leave shared geometry and clips
unchanged.

## Audio interpolation

Cooking generates Resonance's interpolation filters in Rust and includes their
bytes in the cooked audio tables. No external coefficient file is required.
Diagnostic previews use the same generated filters. Their hash is part of audio
package identities, so changing the generator invalidates cached audio recipes.

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

Development publishers must use the same output lock and asset publishers as a
full cook. Stage changed payloads together with their shared and field-local
inventory hashes and byte counts, then verify the complete dependency graph.
A partial refresh does not certify full-library completeness.

Battle entry now uses a native HUD fade. Current party profiles omit
`entry.screen_break`; refresh older profiles instead of retaining that field.
Battle particles use ordinary atlas textures and masks. The `Screen` texture
variant is retired; the same targeted refresh rebuilds older effect publications.
Enemy action rows now contain named native attacks and resolved projectile damage;
refresh older enemy definitions before battle preparation. The separate enemy
projectile/hit-rule tables, numeric spell selector and `overlimit_scale` profile
field are retired. Actor voice profiles also omit unused item-give and
item-receive roles. Profiles retain `cast_ticks` and omit timed chant programs and
casting animation settings; refresh party and enemy publications after these schema changes.
Normal action publications also omit flight profiles, slot-indexed pause flags
and weapon-contact geometry. Throws use native tuning and spherical contacts;
refresh older normal tables instead of adapting them at runtime.
Menu artwork version 17 and dialogue artwork version 3 omit cursor images and
motion parameters. Menus and dialogue choices draw a native indicator using the
required font atlas. Regenerate both manifests; no cursor assets admit input.

For battle development, the tracked `refresh_battle` example rebuilds primary
battle tables, party and weapon rigs, enemy gameplay definitions and artwork,
Nurse artwork, battle UI,
shared menu/dialogue artwork and fonts, victory data, game rules and labels through the production
publishers.
It regenerates saved-content identity, updates current shared/field inventories,
and retires removed battle scripts and standalone enemy projectile tables.
Party and weapon models are republished;
unselected media and variant publications are retained; this is not a complete
library cook.

Use a disposable copy of an existing library. The helper installs staged files
by rename, so a hard-linked copy keeps the baseline intact:

```sh
cp -al local/generic-fields/assets local/battle-simplified-assets
cargo run --locked --offline -p resonance-import --example refresh_battle -- \
  local/battle-simplified-assets
RESONANCE_TEST_ASSETS="$PWD/local/battle-simplified-assets" \
  cargo test --locked --offline -p resonance-game --test battle_preparation \
  all_nine_characters_normal_attacks_hit_and_finish -- --ignored
```

The destination must be new when copying. The helper requires both the existing
library and `local/extracted/disc1`; it records its inputs and publications in
`attack-refresh.json`. It holds the regular cook lock and validates inventories
before installation. Installation replaces individual files atomically, not the
whole library at once; discard a failed disposable copy rather than treating it
as a verified publication.

A field-to-battle replay also needs the current save identity and selected field's
media. The isolated HUD review library at `local/battle-hud-review-assets` was
refreshed through the production publishers for map 332, current title/menu cues,
and maintained battle audio. Its `hud-review-refresh.json` records that narrow scope;
the identity uses all 501 existing field manifests. Current shared and map 332
inventories verified 11,889 dependencies, with checked baseline pins unchanged.
Other field media and variants were retained, so this is not a complete-library
publication.
The temporary refresh driver was removed after publication.

Nurse's asset publisher records its spell archive and table dependencies.
`battle/scenes/237.json` binds a standard source effect bank, textures,
four model slots and their verified physical dependencies. Existing importers
produce shared meshes, textures and sparse clips; identical source clips share
bytes. Nurse has no supported gameplay release. Its cinematic artwork is still published,
but battle does not execute that choreography or maintain shared cinematic clocks.

Each distinct executable is parsed into one set of catalogues. Embedded table
publication, menus and session data consume those same values. Field actors,
figurines and monster previews bind typed decoded packages through the same
model path.

## Library and field preparation

- `assets/<hash>/` holds converted source packages.
- `meshes/`, `textures/` and `clips/` share final geometry, images and motion clips.
- `audio/` holds decoded samples and playback packages.
- `fields/map-{id}.json`, `fields/map-{id}-audio.json` and
  `fields/map-{id}.preload.json` bind each field's scene, audio and local dependencies.
  `shared.preload.json` owns common dependency hashes and byte sizes once.
- `movies/{id}.json` binds the movie catalogue to shared converted streams.
- `scripts/` contains immutable authored SymphoniaScript published from embedded sources.
- `data/` and `embedded/` hold parsed records and their provenance.
- `battle/formations.json` preserves every original formation and its common-archive
  source digest. Distinct supplied common archives retain their own formation
  publications under `battle/variants/<hash>/`; equal inputs share a publication.
  The primary catalogue enters the shared verified dependency inventory.
- `battle/effects/{common,techniques}.json` holds typed particle declarations and
  schedules of particle births, sound and camera-shake events. The importer resolves
  modifier arithmetic into named birth settings and bounded ranges, and UV data
  into frames or scrolling. Position and angle variation is independent; size axes
  share one sample within a particle. There are no runtime sample identities,
  weighted expressions or property-edit instructions. Loading binds the
  selected members' textures, models and audio and validates definitions before
  activation. Programs contain direct event arrays; each model particle owns its
  playback. Independent effects emit after gameplay; particle motion and model
  animation then advance together, respecting feedback holds. Cooked banks carry
  no early/late phase selector; refresh older banks before use. Cosmetic randomness
  remains separate from gameplay. Unsupported members
  retain explicit diagnostics reported when selected. Distinct common archives use
  `battle/variants/<hash>/`; primary banks enter the shared inventory.
- `battle/projectiles.json` publishes motion, contact, lifetime and effect settings
  for the common projectile templates. Unsupported templates carry a diagnostic
  reason. Preparation resolves selected hit rules and effect resources from the
  verified snapshot and validates supported behavior before activation.
- Hair and cloth chains contain their final dynamics, rotation locks and collision
  anchors. Cooking resolves authored visual tuning against skeleton names; field
  and battle models use the same native solver without character-specific rules.
- `battle/effects/tints.json` supplies element palettes, particle colors and actor
  RGBA colors. Casting binds its tint during preparation; particle birth settings
  select RGB and palettes. Recovery binds actor colors from the same table.
- `battle/ui.json` version 33 retains the HUD font and optional party portraits.
  Commands, target indicators, scans and results use native geometry and text.
  Kill-bonus captions and palettes are no longer published; combo-based loot
  bonuses are calculated at victory. Refresh older descriptors before use.
- `battle/recoil.json` contains hit-response impulses, proximity thresholds, weight
  scales and guard settings as numeric parameters. Runtime recovery owns motion.
- `battle/{martial,spell}-actions.json` contains named resources for maintained
  actions. Cooking resolves hit rules and projectile templates without retaining
  source-slot or phase tables. Native battle definitions own timing, contact
  windows and recovery.
  Unsupported reactions, conditions and controllers remain preparation failures.
- `battle/normal-actions.json` contains each character's contact rules and
  detached-weapon parameters. Native definitions own normal timing, reach, input
  routing, combo admission and recovery. Preparation resolves contacts directly
  into those actions without intermediate maps. Special
  Guard and supported enemy attacks also use prepared native events. Fire Ball uses
  a native projectile volley; Lightning and Nurse releases are unsupported. The production refresh
  removes their retired scripts and refreshes inventory hashes and save identity.
- `battle/party-profiles.json` contains actor traits, attachment and facial channels,
  casting duration, voice selections and entry geometry. Models own common chant
  and release poses; the scene handles casting feedback at gameplay transitions.
  Loading combines profiles with session statistics and equipment.
  Party and enemy traits omit the unused body-bounce and reaction root overrides.
  Per-actor camera yaw and distance overrides are omitted; native body bounds and
  a shared viewport margin determine framing, including the initial view. Entry
  profiles retain only artwork. After the scene reveal, native gameplay owns a
  fixed introduction, with no imported camera coordinates or slide timing.
  Native placement centers and diagonally staggers party/enemy lanes. Profiles
  contain no coordinate tables; strategies select lanes, and encounters can
  still declare explicit enemy positions.
  Refresh older profiles before use; strict decoding rejects obsolete fields.
  Prepared models use one reaction pose set and a shared transition blend; native
  activity selects guard, stun, launch, down and get-up feedback.
- `game/techniques.json` supplies technique rules, learning choices, TP costs and
  action ranges. Casting combines these settings with party profiles and prepared
  body clips. Native definitions select executable actions. Numeric action
  identities and imported casting-implementation flags are omitted; menu TP
  percentage costs are resolved during decoding. Strict admission requires
  refreshed technique publications. Battle preparation and execution use native
  definitions, with no battle `.sym` scripts or compiled battle test fixtures.
- `battle/enemies/NNN/definition.json` contains the required enemy gameplay
  profile, AI/action rules, projectile templates, entry placement and guard traits.
  It is shared data and can be prepared without loading a skeleton or effect bank.
  `battle/enemies/NNN.json` contains optional body and carried-model artwork, plus
  an inventory of meshes, textures, sparse motions and effects. Preparation loads
  those payloads only for selected enemies. Missing artwork can be omitted under
  tolerant diagnostics; required gameplay data must still be valid. Obsolete
  standalone enemy `projectiles.json` files are no longer published.
  Bind channels and bone indices serve animation. Runtime actors own native
  colliders and actions own melee volumes; rigs contain no combat geometry,
  attack groups or target-bone classifications. The Monster Book supplies names,
  statistics and scene bindings.
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
entry bindings. Battle actions use native definitions with prepared content;
they do not compile event programs.

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
inventories contain enemy gameplay definitions and party/enemy model descriptors;
each model descriptor lists
the meshes, textures and clips verified when that model is selected for battle.
These payloads are not loaded for every field. Both original party body and motion
sources list the resulting descriptor in `sources.json`.

The shared weapon bank similarly preserves original resource slots and holes,
primary/outline/extra model layers, local sparse clips and contact rigs. Its bank
descriptor is shared; equipment selection verifies only the chosen model payloads.
Battle preparation resolves optional trail endpoints against the selected weapon
rig. Presentation samples those endpoints and owns ribbon history and fading;
weapon clips derive their phase from the owner's current playback.

Fields use named, disjoint draw stages for scenery and actor body/outline passes.
Missing scenery sections retain their stage instead of shifting later materials
into actor ranges. Field metadata versions reject obsolete ordering layouts.

## Recovery and validation

Use representative asset bytes and documented format layouts to validate readers.
Prefer checked shared parsers and named fields. Report unsupported meaningful
fields explicitly; do not hide failed decoding with asset-specific fallbacks. `read::record!`
keeps binary field layouts beside decoding expressions. Runtime behavior is designed
independently of the input format.

Validate changed readers on representative original content before a full cook.
At integration checkpoints, exercise both discs and the supported field, menu,
skit, save, movie and audio routes. Compare fresh native output with verified
Dolphin captures at native resolution, preserving existing thresholds. All
unattended playback stays silent. See the [oracle guide](../tools/oracle/README.md).

Successful conversion establishes coverage only for the declared general scope.
It does not prove every native controller is implemented or every scene renders
correctly; native late-actor phases remain unsupported by the runtime. Continue
format support when a concrete decoding or gameplay failure exposes a gap.
Battle preparation has its own implementation and validation boundary.
