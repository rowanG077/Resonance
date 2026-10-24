# Human-readable cooked content

Design updated: 2026-10-24. This document defines the target content format and
its implementation. The [implementation record](#implementation-record) preserves
progress, verification and remaining work separately. Partial implementation
exists; the design is not a claim that the complete format is ready to use.

Start with the [directory structure](#proposed-directory-structure) and
[mapping rules](#id-mappings-and-references) for the public format. The
[editing examples](#what-an-edit-requires), [mod rules](#mod-directories) and
[acceptance criteria](#acceptance-criteria) describe what it must let a person do.

## Decision and priorities

Adopt the proposed layout: recognizable content directories, readable resource
names, and a small `mappings.json` wherever Resonance translates an original
machine ID into a resource. Use lowercase directory names consistently:
`fields/`, `audio/music/`, `audio/speech/`, `audio/effects/`, `movies/` and
`meshes/`, with additional domains as required.

**Disc identity is an importer detail, never cooked resource metadata.** This
applies to every resource category. The installed library has no disc directories,
disc suffixes, disc ownership fields or disc-based runtime selection. Import both
discs into one game library, with one named resource for each logical game resource
and ordinary ID mappings. Duplicate declarations of that resource share its owner.
Resolve conflicting source definitions in the importer; do not invent a second
runtime catalogue or silently select an arbitrary conflicting definition. If game
behavior actually distinguishes two resources, name and bind them by that behavior.
Source locations and import accounting stay in working diagnostics outside the
installed asset library. Earlier implementation notes about disc scopes and
installed provenance reports are superseded by this rule.

Make design choices in this order:

1. A person can locate content and understand what editing it will affect.
2. Supported edits work directly, without repairing hashes or regenerating
   dependency inventories.
3. Original game identities, behavior and conversion coverage are preserved.
4. Runtime preparation and caching make that editable representation efficient.

**No backwards compatibility is a design goal. Simple implementation wins.**
Support one current cooked layout and one current save schema. Remove obsolete
runtime readers, path aliases, format autodetection, compatibility fallbacks and
save repair or migration tools. Old cooked libraries require a fresh cook; old
or gameplay-incompatible saves are rejected with a clear error. A format or
identity change may deliberately invalidate existing saves. Do not build a bridge
to preserve them or let compatibility concerns complicate the new design.

The cooker still needs parsers for the original disc data. Old cooked-format
readers must be deleted, including test-only readers: keeping a second loader
merely to compare fixtures is not a design requirement. Update behavioral tests
to prepare the current named layout. Retire checks that test only obsolete
manifest machinery; retain meaningful game behavior and source-fidelity coverage.
Remove temporary old-layout publication/translation as direct cooking is wired.
Presentation edits and resource renames within the current format still preserve
gameplay identity.

Human-readable organization does not justify losing game behavior. Where native
data needs a specialized representation, keep its ownership clear and provide an
explicit editing path. Runtime optimizations belong behind the public layout.

`mappings.json` is a translation table, not a replacement manifest. A folder needs
one only when a runtime lookup addresses its contents by ID. A field's textures,
scripts and other files already referenced by name do not each need another map.

For a modder, the normal workflow is:

1. Open the category and named resource, such as `fields/iselia-1/`.
2. Edit the relevant dialogue, scene, script or media file directly, or place a
   replacement at the same path in a mod directory.
3. Change `mappings.json` only when redirecting a machine ID to a different
   resource. Replacing a resource at its existing path needs no mapping change.
4. Reload at the supported boundary. Resonance discovers and validates the
   current dependencies and rebuilds its own derived data.

The public contract should stay understandable in three steps: choose a category,
open a named resource, edit the file for the intended change. The engine performs
one ID-to-resource translation at the category's lookup boundary and then follows
ordinary resource references. Neither the modder nor the engine should traverse
another chain of generated registries to find the actual content.

### What metadata remains, and why

Some metadata describes the content itself: a scene needs actor positions, a
model needs material bindings, and a music recording needs its loop boundaries. Keep that information beside its named owner. The problem is requiring
people to maintain a second description of files that already exist.

| Document | Contains | Must not contain |
| --- | --- | --- |
| `content.json` | Layout version and supported game identity. | An installation-wide file list or dependency graph. |
| `mappings.json` | Original IDs and their named targets within one lookup domain. | Hashes, asset settings, transitive dependencies or another generated registry key. |
| A resource document such as `field.json` or `model.json` | Editable properties and direct references needed to describe that resource. | A mirrored inventory of every file reachable through those references. |
| Internal preparation/cache data | Discovered dependencies, computed digests and generated runtime representations. | Additional authoring obligations; deleting it must trigger automatic rebuilding. |

Do not add a descriptor when a direct mapping to an ordinary file expresses the
whole resource. Speech can map directly to a WAV; a field needs a document because
it has several meaningful components. Use the smallest representation that retains
the actual semantics, rather than forcing every resource into the same wrapper.

The practical test is to replace an existing file at the same logical path. If
the edit requires changing unrelated metadata merely to persuade the loader that
the new bytes are legitimate, the new layout has retained the old problem.

## Identity model

Keep three concepts distinct:

| Concept | Example | Responsibility |
| --- | --- | --- |
| Game identity | Field ID `1` | Stable identity used by original scenario instructions and saves, within its lookup domain. |
| Logical resource path | `fields/iselia-1/field.json` | Human-readable location used by descriptors and mod overrides. A mapping translates the game identity to this location. |
| Derived content identity | An internal digest of captured inputs | Cache reuse and invalidation. Computed by Resonance; never a filename or checksum a modder must maintain. |

The example ID is illustrative. Renaming a field changes its location and affected
path references; it does not change its game identity. Replacing artwork at the
same path changes the captured bytes and any dependent cache entries; it does not
require a new mapping. Editing gameplay may affect save compatibility even when
both the ID and path stay the same.

This separation makes the proposed layout feasible without teaching the original
game about directory names. The cooker produces readable defaults, the resolver
translates IDs, and the runtime prepares the effective files people have edited.
No second registry assigns GUIDs or content hashes to those files.

The intended workflow is: find a field, edit its dialogue or artwork, reload it,
and see the change without updating hashes or regenerating dependency manifests.
Application restart is the initial supported reload boundary. It recreates the
asset server, decoded image/sampler caches and scene preparations. Finish editing
before starting the next run. Re-entering a field or reopening a preview does
not reload edits; those boundaries may be supported later only when every
affected runtime cache is invalidated. Live hot reload is a later capability.

## Current problems

The current library is organized substantially around conversion and deduplication:

- Source packages live under `assets/<hash>/`, retaining archive member paths.
- Shared meshes, textures and motion clips use content-addressed names.
- Fields are separate `map-{id}.json`, `map-{id}-audio.json` and
  `map-{id}.preload.json` files.
- Preload inventories record paths, hashes, byte counts and roles. Runtime rejects
  content that differs from those recorded bytes, including authored scripts.
- Source indexes and prepared descriptors expose the importer's organization.

The inspected local library contains approximately 4,300 mesh files and 49,000
motion clips. One field descriptor is 2.8 MB, with a separate preload inventory
listing 798 dependencies. These are observations of the current local output,
not format limits or performance targets.

A directory rename would leave the editing problem unresolved. The publication
format, resource references and loading contract must change together.

## Proposed directory structure

Use lowercase names consistently. The following names are illustrative, not
verified assignments of original game IDs or a complete category catalogue.

```text
cooked/
  content.json

  fields/
    mappings.json
    iselia-1/
      field.json
      scene.json
      actors.json
      messages.json
      collision.json
      audio.json
      scripts/
      meshes/
      textures/
      animations/
    iselia-2/
      ...

  audio/
    music/
      mappings.json
      ...
    speech/
      mappings.json
      iselia-1/
        ...
    effects/
      mappings.json
      ...
    instruments/
      ...

  movies/
    mappings.json
    opening/
      movie.json
      video.mkv

  meshes/
    characters/
      lloyd/
        model.json
        meshes/
        textures/
        animations/
    props/
      treasure-chest/
        ...

  battle/
  world/
  ui/
  fonts/
  skits/
    mappings.json
    catalogue.json
    skit-0001/
      skit.json
      scenario.ssb
      messages.json
  game/
  scripts/
```

`content.json` declares the cooked layout version and game identity. Keep it
small: it is not a global resource inventory. Resource schemas retain explicit
versions where needed.

Organize content by meaningful ownership. A field owns its scenery, local
textures, dialogue, collision and local scripts. A shared character or prop owns
its model, materials, textures and animations. Audio is browsable by purpose,
with subdirectories for locations, characters or banks where appropriate.

Avoid splitting every model's components across global mesh, texture and
animation pools. A shared texture directory is justified for genuinely shared
artwork, not as the default destination for every image.

Retain conversion coverage and source provenance as importer reports, separate
from the editable resource contract. Runtime must not require those reports.
Unsupported or unidentified resources remain accounted for with honest names and
explicit status; the layout change must not reduce conversion coverage.

## ID mappings and references

For the illustrative IDs used in the proposal, `fields/mappings.json` is simply:

```json
{
  "1": "iselia-1",
  "2": "iselia-2"
}
```

The lookup is:

```text
field ID 1 -> fields/mappings.json -> fields/iselia-1/field.json
```

Read and validate mappings once per content snapshot. Runtime lookup is an
ordinary in-memory table operation. Replace helpers that construct paths from
IDs with a resolver that uses these mappings.

Rules:

- Mapping keys preserve original machine IDs. They are never assigned from
  directory enumeration, alphabetical order or a content hash.
- Numeric keys use canonical unsigned decimal strings, without leading zeros.
  Human-readable names belong in the values, not in a replacement ID scheme.
- Mapping values are paths relative to the directory containing `mappings.json`.
- Each lookup domain defines its target kind. Fields map to directories with
  `field.json`; movies map to directories with `movie.json`. Other domains choose
  a documented file or directory entry point consistently.
- Explicit resource references inside descriptors are relative to the cooked
  root, for example `meshes/characters/lloyd/model.json`. Do not guess whether a
  path is local or global, search multiple directories, or silently substitute a
  different resource.
- Mapping entries contain bindings only. Playback settings and scene properties
  belong in the corresponding resource descriptors. Hashes, byte counts and
  importer provenance do not belong in mappings.
- Use mappings where the engine or original script requests an ID. Files already
  referenced by a readable path do not need another numeric namespace.
- The mapping is authoritative for the external ID. Avoid duplicating that ID in
  every component as an independently editable value. Where a schema retains an
  ID for validation, it must agree with the mapping.
- Reject duplicate JSON keys, malformed IDs, missing targets and paths escaping
  the content root. Detect case-only path collisions during publication so the
  library behaves consistently across supported filesystems.
- An unmapped requested ID is an error. An invalid explicitly mapped replacement
  must not fall back to an old numeric filename, a hash index or embedded content.
  Report the mapping path, requested ID and target so the user can fix the binding.

A mapping can intentionally bind multiple IDs to one resource when their semantics
allow it. Equal bytes alone are not evidence that those IDs should share editable
content.

### Where mappings belong

The directory tree is for people; mapping boundaries follow actual engine
lookups. Do not require a mapping in every folder merely for consistency.

| Request | Resolution |
| --- | --- |
| A scenario requests field ID `1`. | Resolve through `fields/mappings.json`, then open the selected directory's `field.json`. |
| Playback requests a movie ID. | Resolve through `movies/mappings.json`, then read `movie.json`. |
| Audio requests a song or a member of a speech bank. | Resolve through the corresponding music or bank mapping, preserving the original ID scope. |
| A scene already names `meshes/characters/lloyd/model.json`. | Read that path directly; no additional mapping is needed. |
| A model names its texture or animation file. | Read that path directly; no per-file mapping or manifest is needed. |
| The runtime loads a fixed shared document such as menu settings. | Use its documented logical path; do not invent a numeric ID. |

Consequently, `audio/` need not have its own mapping if runtime requests already
distinguish music, speech and effects. The same applies to `meshes/`: add a mapping
only for an actual model-ID lookup, in the scope where that ID is meaningful.
Avoid chains of registries that merely translate one generated name into another.

### Scoped identities

Not all IDs are global. Preserve the existing lookup domains for field-local
models, animation banks, speech archives and sound banks. Two banks may both
contain resource `12` without identifying the same resource.

Use nested mapping entries or a mapping within each named bank to retain that
scope. Select the representation after auditing each actual runtime lookup;
do not flatten unrelated IDs or invent a universal composite-ID syntax merely
for uniformity. A directory's purpose and mapping schema must make the scope clear.

Replacing content for an existing ID is part of this design. Arbitrary new IDs
also require any relevant runtime bounds, catalogues and gameplay rules to accept
them. Adding an entry to a mapping must not imply support that the runtime lacks.

## Names, ownership and sharing

Prefer stable semantic names such as `iselia-classroom` when the identity is known.
`iselia-1` is acceptable when a finer description is unavailable.

Choose names in this order:

1. Maintained semantic names with a known association to the original identity.
2. Useful original resource names.
3. Stable fallbacks such as `field-0042`, `sound-0127` or `texture-03` within a
   meaningful owner directory.

Keep the naming catalogue with the importer so repeated cooks produce the same
paths. Do not derive paths from localized display text or discovery order.
Normalize names consistently and resolve collisions explicitly. Distinct logical
resources need distinct names or gameplay scopes; source duplication does not
create another resource. Resolve conflicting definitions during import without
publishing source-specific variants. User edits to the cooked mapping are authoritative
for that installation and need not alter the importer's default naming catalogue.

Ownership determines the expected effect of an edit:

- Editing a field's wall texture changes that field.
- Editing Lloyd's shared model changes every consumer of that shared model.
- A consumer that needs a variant references a separate named resource.

Accept some disk duplication to preserve independent ownership. Do not merge
editable resources merely because their original bytes match. Avoid filesystem
links that make editing one apparent file silently modify another resource.
Identical data may still share decoded buffers or compiled cache entries internally.

Renaming a resource requires updating its mappings and explicit incoming path
references. Numeric game IDs remain unchanged. Automatic rename tooling can be
added later; the initial format must keep the affected references understandable.

### What an edit requires

| Intended change | Files a modder changes |
| --- | --- |
| Change a field's dialogue. | Its `messages.json`; no mapping changes. |
| Replace a field's texture at the same path. | The supported image file; no mapping or checksum changes. Material settings change only if the intended rendering changes. |
| Change a shared character's appearance everywhere. | The files owned by that shared model. Its consumers keep their existing references. |
| Give one field its own character variant. | Copy the model and the components that need independent ownership into a named variant, then change that field's actor reference. |
| Redirect an existing field ID to a replacement field. | That ID's mapping entry and the replacement field's descriptors/resources. The original scenario and saved field ID stay unchanged. |
| Rename a field directory. | Its mapping entry and every explicit path affected by the move, including paths inside the moved descriptors. Changing the mapping alone does not rewrite those references. |

Each operation takes effect at the next reload/preparation boundary. Structural
validation still applies, but none of these operations requires editing a
generated dependency list or restoring a cook-time content hash. This is the
practical authoring contract against which the migration should be judged.

### Tradeoffs

| Choice | Cost | Design response |
| --- | --- | --- |
| Names and explicit paths | Moving a resource can break incoming references. | Validate references with useful errors; keep names stable across cooks and document rename requirements. |
| Independent editable ownership | Some identical bytes occupy more disk space. | Accept publication duplication; deduplicate decoded buffers and generated caches internally. |
| Dependencies derived from current files | A cold load performs discovery and validation work. | Prepare before gameplay and cache derived work with automatic invalidation. |
| Whole-file mod overrides | Two mods editing different parts of the same file still conflict. | Split files by meaningful editing tasks and report deterministic override order. General patch merging can follow later. |
| Stable original IDs | Adding a directory does not automatically add a new game location or resource ID. | Use explicit mapping entries and validate the relevant gameplay limits. |

These costs are preferable to requiring modders to understand importer archives,
hash identities and generated dependency graphs. Measure the runtime and disk
costs during migration rather than treating readability as a reason to ignore them.

## Resource contents and editable formats

Keep entry descriptors small enough to explain the resource. Split substantial
content along meaningful editing boundaries, rather than reproducing one large
prepared object graph in JSON.

For a field, keep the resource entry point in `field.json`, dialogue
in `messages.json`, and collision in `collision.json` or an explicitly selected
editable geometry representation. Use `scene.json` for substantial scenery and
environment settings, `actors.json` for actor bindings, and `audio.json` for the
field's audio choices. Keep local resources under the field directory. Do not add
a separate index or manifest for every individual file.

For example, `fields/iselia-1/field.json` can be a short entry point:

```json
{
  "version": 1,
  "script": "fields/iselia-1/scripts/scenario.ssb",
  "messages": "fields/iselia-1/messages.json",
  "scene": "fields/iselia-1/scene.json",
  "collision": "fields/iselia-1/collision.json",
  "actors": "fields/iselia-1/actors.json",
  "audio": "fields/iselia-1/audio.json",
  "movies": []
}
```

These references correspond to recognizable editing tasks. They do not repeat a
complete dependency inventory. Scene and model files point directly at the meshes,
textures and animation clips they use. A reader following an actor binding reaches
`meshes/characters/lloyd/model.json`, which describes that model's own resources.
Movie choices retain catalogue IDs and resolve through `movies/mappings.json`;
the runtime must not recover their identity by parsing filenames.

The `.ssb` above is the original scenario bytecode, whose editing limitation is
described below. A readable directory name must not imply readable script source.

Pretty-print editable JSON with stable ordering. Use named properties and units
where their meaning would otherwise be ambiguous. Semantic rendering and gameplay
settings remain explicit; data that can be derived safely from the current files
should not become another value that modders must synchronize.

Ship a short authoring guide with the cooked library: where to find each kind of
content, which files are authoritative, which formats can be edited, and how to
reload changes. Document schemas by resource kind, including meaningful units and
shared ownership. Documentation must not become a per-resource inventory that
needs updating whenever someone adds a file.

### Geometry, textures and animation

Retain GLB for geometry. Support accessible texture authoring such as PNG, with
color interpretation, sampling, palette behavior and material settings represented
explicitly where required. Preserve original mip levels and other authored data
that cannot be regenerated equivalently. Publish ordinary PNGs for single linear
levels. A `.texture.json` adds only explicit color interpretation (`linear` or
`srgb`) and a mip policy: `none`, an ordered list of `authored` PNG levels, or
opt-in `generate` for replacement art. Paths are library-root-relative, dimensions
come from the images, and original authored mip pixels never pass through a
filter. Generated replacement mips use alpha-weighted linear-color filtering.
Runtime-oriented texture encodings may be generated automatically in the cache.

Motion clips currently preserve native curves, controls, matrix channels and
other behavior that a sampled animation export may lose. Provide a documented,
lossless editing path using `.motion.json` files with explicit native curve data.
Document the channel meanings, units and skeleton bindings. Use one JSON motion
schema in importer output and runtime; remove the old binary codec. Any future
compiled representation belongs only in a disposable derived cache. Derive duration and other
redundant preparation metadata from the edited clip only where equivalence has
been demonstrated; numeric JSON round-tripping alone does not prove identical
animation behavior.

Some physical geometry requires caller-supplied textures and cannot yet become an
ordinary rendered actor. Preserve those recipes under the owning field's
`models/unbound-<resource-id>/layer-<index>/` directory, with `scene.json`,
`mesh.glb`, and any locally owned texture bank and palette images. Preparation
follows those current references just as it does other scene dependencies. An
authored texture-table label remains a symbol, not a filename to discover.
Keeping such geometry readable and accounted for does not imply that its missing
renderer binding is implemented. Its original diagnostic remains explicit.

### Audio and movies

Background music is rendered to ordinary stereo PCM WAV during cooking. This is
an explicit design change requested on 2026-10-24: normal music playback should
read a recording, rather than sequence notes and synthesize instrument programs.
The synthesizer remains useful inside the importer to render the original tracks
and for effects whose behavior depends on live controls. Speech remains decoded
WAV. Do not require mod authors to understand scores, instruments or sample banks
to replace background music.

Music is a shared top-level domain, separate from fields. Publish each named song
once under `audio/music/`, including tracks used by several fields, the world and
battles. `audio/music/mappings.json` owns the song-ID translation. A field's
`audio.json` selects song IDs through that mapping; it never owns copies of the
recording, its playback document or its instrument sources. Replacing a shared
song changes every consumer of that song. Field names do not determine music
ownership, and publishing another field must not render or copy its songs again.

The target audio lookup boundaries are:

| Mapping | Original key | Target |
| --- | --- | --- |
| `audio/music/mappings.json` | Song ID | Named song directory containing `music.json` and its ordinary WAV. |
| `audio/effects/mappings.json` | Field cue ID | Bank/cue directory containing `sound.json`, for example `s-se/door`. |
| `audio/speech/mappings.json` | Speech archive base ID | Named archive directory. |
| `audio/speech/<archive>/mappings.json` | Member ID within that archive | Named WAV file. |

For example, an illustrative music resource is:

```text
audio/music/
  mappings.json
  iselia/
    music.json
    music.wav
```

```json
{
  "version": 1,
  "audio": "audio/music/iselia/music.wav",
  "loop": {
    "start_frame": 320000,
    "end_frame": 3840000
  }
}
```

Loop boundaries are zero-based sample frames, with an exclusive end. Play from
frame zero for the introduction, then repeat `[start_frame, end_frame)`. A null
or omitted loop means a one-shot recording; retain its audible release tail.
Infer rate, channels and length from the current WAV. Validate loop bounds against
that file and identify the song in errors. The example name and frame positions
are illustrative. Replacing the WAV needs no mapping, checksum or inventory edit;
a replacement with a different introduction or loop length needs corresponding
loop settings because those are playback semantics, not redundant file facts.

Keep original song IDs and the existing play/stop, jingle, suspend/resume,
scene-handoff, movie muting and fade behavior. In particular, do not reinterpret
resume as seek-resume if the original command restarts the selected background
track. A one-shot jingle must not acquire a loop. These behaviors belong to the
recording player, independently of how its PCM was produced.

The importer must derive the musical boundaries from the actual sequencer clock,
including tempo changes, and render enough repeated material to preserve notes
and tails that cross a loop. Verify the introduction-to-repeat transition and
repeated seams; simply cutting the first pass at a score tick is insufficient.
Do not crossfade or smooth a join to conceal an incorrect loop. A discontinuity
must be investigated in the boundary, musical clock, carried voices and effect
state; matching the PCM before the cut alone does not establish a correct loop.
A WAV repeat can span several musical passes when that is necessary for the
rendered state to recur. Native track 95 demonstrates this: successive musical
passes differ in event phase, while boundaries three passes apart have identical
dry buses, effect buses and final PCM in the inspected window. Use measured
recurrence, without retiming, fading or rewriting samples. Keep lookahead and
candidate storage bounded, and reject an unresolved loop explicitly.
When the source buses form a valid repeat but their live reverb does not, bake
the effects against that chosen repeating performance. Preserve the original
introduction, replay the unmodified dry/auxiliary passage through one continuous
studio, and accept a cut only after the resulting PCM also recurs on both sides.
This lets the effects settle against the input the WAV will actually repeat.
It is not a crossfade. Use bounded scratch storage for the source buses rather
than retaining an entire song in memory.
Search up to 20 minutes of native synthesis. Once a dry passage is verified,
budget its effect bake separately: allow two repetitions plus the bounded effect
tail, with a one-hour ceiling on the whole recording. Otherwise a valid passage
found late in the search could be rejected before it can repeat even once.
Recurrence checks admit the first native wrap; an extra mandatory warm-up pass
would only increase the introduction and file size. Measure long multi-pass
recordings explicitly before accepting the catalogue's storage cost.
Use bounded rendering with explicit diagnostics for tracks that cannot terminate
or form a usable repeat. Compile the original synthesis input directly from the
disc when cooking or diagnosing a track. Do not retain a second cooked music
format or require a serialized synthesis package between compilation and WAV
rendering. Optional source exports are independent authoring tools, not runtime
dependencies or a prerequisite for cooking.

Bake the track's intended reverb into the normal music WAV and avoid applying the
same music reverb again at playback. Audit inherited environments and tails
before the switch: native song IDs 0 and 111 retain the current reverb, while
other songs select a fixed preset. If these two need distinct audible renders,
keep clearly named field/battle recordings under their song owner with a small,
explicit context choice. Do not multiply every track into context variants.
The implemented descriptor has an optional `battle` object containing `audio`
and optional `loop`, using the same meanings as the main recording. The importer
emits it only for inherited-reverb songs whose rendered WAV or loop differs. A
replacement can omit it and use its main recording in both contexts. The named
mixer selects this variant from the
current music preset; entering a battle alone does not replace an inherited preset.

A fixed recording also fixes any randomized instrument variation and no longer
participates in the original shared synthesizer's voice allocation. Baked music
reverb cannot reproduce every interaction with an evolving shared effects studio
or every original pre-fader send during a volume fade. This is a deliberate
simplification to evaluate and document with audio comparisons, not a claim of
sample-identical live synthesis. Preserve track choice, timing, loops, cues and
transitions, and report any remaining audible fidelity differences explicitly.

Stream long music recordings through bounded buffers, preserving the effective
mod-selected file and its playback lifetime. Do not load the entire soundtrack
into memory or reopen through the base directory when looping. At 32 kHz stereo
PCM16, storage is 7.68 MB per minute before filesystem overhead; measure actual
catalogue size and cook time. WAV is the initial editable format; compression is
a separate later decision.

The central publisher and named title, field, world and battle paths now publish
and read WAV music. The full battle worker now captures its encounter and selected
recordings from one named snapshot. Delete the remaining old audio fixture
loader and update its tests to this same path. Native encounter activation is
still being verified. The earlier
verified score/sample comparisons provide the source baseline; they do not
establish recording loop fidelity. Complete publication and all consumers before
accepting this transition.
There should be no permanent synthesized-versus-recorded music authoring choice
merely to hide an incomplete switch.

The original field effect catalogue explicitly rejects duplicate cue IDs across
its selected banks, so that domain has one direct mapping. Bank-scoped domains
outside that catalogue must retain their own mappings. Source-bank names still
determine ownership of cue directories and instrument samples.

Field `audio.json` declares selected music/effect IDs and speech archive members,
along with effect reverb choices and the dialogue-volume curve. An optional speech
playback rate preserves callers that intentionally use a different clock from the
source WAV. Otherwise playback follows the current WAV rate. Frame counts,
channel counts, byte counts and integrity hashes are inferred or computed.

Shared effect programs inherit their field/battle playback environment, keeping
one editable program/sample owner when contexts differ only in reverb. A
standalone arrangement such as menu cues may own its reverb. A program must not
simultaneously specify its own studio and inherit a conflicting context. Prepared
effect caches include the context and current program/sample bytes. Preserve any
music-triggered studio changes that still affect effects, even after music itself
comes from a recording.

`battle/audio.json` names the music and effect mappings and discovers their IDs
from those mappings. It discovers ordinary speech through the archive/member
mappings rather than repeating a complete voice inventory. Archive-wide playback
clock overrides and exceptional member overrides are explicit. Its `streams`
value identifies the native speech archive whose member IDs also serve as battle
stream aliases; both spellings resolve to the same named WAV. Only selected
recordings, effects and voices are prepared for an encounter. Importer reports
outside the library account for unavailable cues and authored empty speech members,
and are never runtime prerequisites.

Synthesized effect `sound.json` files use `"kind": "synthesized"` and retain editable programs, synthesis tables
and sample references. Instrument WAVs retain the original first pass followed by
the repeated-loop waveform, which can differ because of decoder history.
`loop_length` identifies the appended waveform; first-pass length and sample rate
come from the current WAV. Preserve live pitch, pan, volume, release and cue
lifetime behavior. Suitable effects can instead use `"kind": "wave"`, an `audio`
path and optional source-frame loop points. The current schema explicitly selects
the playback kind; there is no fallback to an earlier sound document. The
background-music decision does not flatten original synthesized effects.

A movie directory contains its stream and small playback descriptor. Preserve
track selection and timing semantics. Infer format facts from the current media
where possible, so replacing a file does not require manually repairing stale
frame counts, sizes or hashes. Errors in a replacement should identify the actual
media requirement.

The implemented movie entry point is `movies/mappings.json` -> named directory
-> `movie.json`, with this descriptor:

```json
{
  "version": 1,
  "path": "movies/opening/video.mkv",
  "audio_track": 0
}
```

The track index is zero-based and defaults to zero. The current playback profile
is Matroska with lossless RGB8 FFV1 video and stereo FLAC audio at 8–96 kHz.
Video dimensions remain bounded at 1920×1080, default frame duration at 10–100 ms,
and playback at 100,000 video frames and one hour. These are codec/runtime limits,
not values to repeat in editable JSON. Audio length may be unspecified in the
media header; preparation then uses EOF. Decoding validates timestamps, supported
formats, any header-declared audio length and actual audio/video duration agreement.

Each playback retains the selected file handle. Independent buffered cursors use
positional reads, so separate players cannot move one another's file position. An
atomic pathname replacement affects the next playback. In-place changes to an
already open file are not transactional: finish editing before reloading. Offline
capture planning scans timestamps from that same handle without decoding frames
or creating a persistent inventory. Normal playback remains streamed.

### Scripts

Original field scenarios currently use bytecode, while authored SymphoniaScript
uses readable source. Moving original bytecode into a named field directory does
not make it readable source.

Make existing authored source editable through the new loading contract. Original
scenarios have an explicit instruction-level workflow: `symphonia-script scenario
disassemble INPUT.ssb OUTPUT.ssasm`, edit the assembly, then `scenario assemble
INPUT.ssasm OUTPUT.ssb` into the selected base or mod path. Disassembly proves
byte-for-byte reassembly before publication; assembly admits the result with the
VM's current program rules before atomically replacing the target. Working
assembly stays outside the library, and the selected `.ssb` remains authoritative.
The original header, registry overlays and auxiliary offsets retain explicit
layout values; instruction-size changes may require updating those values. Do not
promise automatic translation to high-level source without demonstrating that it
preserves behavior. Where a source and compiled form coexist, define one as
authoritative and generate the other; never require users to keep both synchronized.

The directory and loader migration can land before every specialized editing
format is implemented. Document these remaining limitations explicitly rather
than declaring all content readily moddable after files have merely moved.

### Shared field content and skits

Shared session/menu definitions, labels, fonts and UI artwork have fixed logical
entry points. Follow their typed references during preparation. Discover authored
modules under `scripts/` in the effective base-plus-mod view, so adding a module
does not require updating a separate source inventory. Compile against those
captured sources and capture every asset declared by the compiled entry points.

`skits/mappings.json` binds native skit IDs to directories containing `skit.json`,
scenario bytecode and messages. The descriptor retains authored skip permission
and event-only titles. `skits/catalogue.json` retains availability conditions,
replay order, portrait recipes and media choices. Portrait images belong under
`skits/portraits/`.

Audible skit media resolves through `audio/speech/skits/mappings.json`. Its frames,
source rate, channel count and cache identity come from the current WAV; an
explicit playback-rate override preserves intentional native clocks. Silent
tracks retain their authored duration and rate without requiring a silent WAV.
Skit voices join the field's prepared voice bank. Gameplay and UI consume the
same resolved catalogue rather than reopening or independently interpreting it.

### Title and startup

`ui/title/title.json` owns title artwork and the original day/night background
field IDs. Those IDs resolve through the ordinary field mappings. Music and cue
bank paths are direct references. `audio/music/title/music.json` selects the
rendered title WAV and its loop boundaries. `audio/effects/menu/cues.json` binds
the controller's `navigate`, `confirm`, `back` and `error` names to editable
programs, with samples under `audio/instruments/menu/`. Title music retains its
intentional owner; matching original score or sample bytes do not silently couple
its edits to field music. Title playback now uses the recording contract;
original synthesis remains only in its explicit fidelity-test path.

`ui/boot/boot.json` retains startup-logo dimensions and background colors with
ordinary image references. Capture requested logos, title artwork, the selected
field, shared menu UI and audio from one content snapshot before startup. Returning
to the title retains those same prepared files; a restart selects subsequent
edits. No title preload inventory, source digest or nested audio digest is part
of this authoring interface. Actual GPU/rendering fidelity remains a separate
verification requirement from successful resource capture.

### Credits and game-over screens

`ui/credits/credits.json` owns the editable credits program, picture references,
music reference and final hold duration. Pictures belong under
`ui/credits/pictures/`; the recording belongs under `audio/music/credits/`.
Keep only an intentional playback-rate override in the descriptor. Derive sample
rate, channel count and length from the current WAV, so replacing the recording
also changes playback timing without manual frame counts or checksums. Credits
use the font selected by the shared dialogue definition.

`ui/game-over/game-over.json` owns its background, font, caption and two menu
labels. Both scripted and battle game-over presentation must consume those
values. A hardcoded duplicate of the labels or a hidden title-image index would
make the apparent editing contract misleading. These screens have fixed entry
points; neither needs an invented numeric mapping.

Discover credits requests from the current scenario scripts during preparation,
including unselected branches and dynamic session-screen choices. Do not require
an author-maintained list of screens to preload. Prepare game-over alongside
shared field UI because a fatal battle need not have an explicit screen request
in the field script. Keep cancellation and the existing optional-background
diagnostic policy; required descriptors, fonts and media must still validate.

### World content

`world/world.json` names the world scenario, messages, audio selection, movement,
collision rules, encounters, landmarks and guideposts. These stay in separate
ordinary files so editing movement does not require opening a terrain/model
inventory. `world/mappings.json` binds native continent IDs 0 and 1 to `sylvarant`
and `tethe-alla`; each directory's `world.json` declares its terrain positions and
marker mapping.

A tile owns `terrain.json`, `collision.json`, `model.json` and the latter's meshes,
textures and animations under a coordinate such as
`world/sylvarant/terrain/column-01-row-02/base/`. An authored alternative has its
own adjacent `alternate/` owner. Coordinates retain native tile identity; both
continents and all story-dependent alternatives are prepared before world entry.
The same model structure applies to owned skies, world actors, markers and
cinematic actors. Party world descriptors reference their declared shared
character meshes/textures under `meshes/characters/`; their world travel and mount
clips remain under their world actor's animation bank. A field/world texture edit
therefore has one body owner, while equal slot numbers in different animation
banks keep separate meanings. Known actor roles receive names such as `lloyd`, `noishe`,
`ship`, `rheaird` and `rheaird-rider-lloyd`; unidentified models retain scoped
numeric fallbacks.

Actor, marker and cinematic IDs each resolve through their own `mappings.json`.
A cinematic directory separates its presentation rules, camera, dialogue and
actor models. No authored world document contains the old aggregate file list,
expected digests or source-archive dependency graph. Captured inputs assemble the
existing runtime world types after validating current geometry, motions, native
ID ranges and complete terrain coverage. World audio uses the ordinary named
music/effect/speech selections, including its cinematic sounds and voices.

World saves and field/world transitions use the same effective gameplay identity.
Movement rules, collision, encounters, landmark behavior and scenario changes
participate; renamed paths, display labels and model/media bytes do not. A field
entry retires its previous world package, so a return takes a new preparation.
This does not by itself establish in-process GPU cache invalidation.

### Battle content

Apply the same organization to battle resources rather than leaving battle as an
exception that still requires the old manifests. Give arenas, enemies, party
variants and choreography recognizable owners under `battle/`. For example, an
arena belongs under `battle/arenas/<name>/`, with `arena.json` and its local
scenery, textures and animation files. Its native arena ID resolves through
`battle/arenas/mappings.json`. Use scoped numeric fallback names where a reliable
semantic name is unavailable.

Keep lookup domains distinct: an enemy ID, an equipment item ID, a native
technique ID and a skill catalogue ID do not become interchangeable because they
have the same numeric value. Each ID-addressed category gets its own mapping;
fixed shared gameplay tables use documented paths. Separate gameplay definitions
from substantial model and effect data so changing enemy behavior does not mean
editing a mesh inventory.

Shared battle rules also need an authoring contract. Formation choices, party
combat profiles, combination recipes, projectile behavior and recoil settings
remain ordinary named documents. Preserve their gameplay constraints, native
indices and ordering, while removing source-archive digests from the public
schema. A valid edited recipe must pass because its structure and references are
valid, without supplying a fictitious original-source hash. Battle UI owns its
font, portraits and controls under readable artwork paths.

`battle/party/rules.json` contains party profiles and policies, with direct
references to `normal-attacks.json` and `techniques.json` in that directory.
This keeps the large action timelines separate from ordinary combat settings.
Formations, Defense System rules, Unison recipes, projectiles and recoil use
their existing descriptive paths under `battle/`, now with versioned authoring
schemas. `game/techniques.json` remains the shared technique/learning catalogue.
`ui/battle/battle.json` references `fonts/battle/font.json`; portraits, target
markers, notices, status icons, scan icons, Unison controls and escape artwork
have named owners under `ui/battle/`. Native table order and scoped fallback icon
indices remain explicit where they carry meaning.

Reference shared character and equipment models directly by their named owners.
Keep battle-specific motion banks and costume choices explicit. In particular,
victory performances select a character and costume, not a body-file hash.
Replacing a model's bytes must not accidentally detach its victory animation or
change which costume the engine believes it is using. Skeleton and attachment
compatibility still require structural validation.

Party IDs resolve through `battle/party/mappings.json`, then the selected
character's costume `mappings.json`. A costume's `party.json` names its rig,
visual model, optional wings and victory definitions. Shared body meshes/textures
remain under `meshes/characters/<name>/`; battle motions remain under the costume's
battle directory. Victory selectors belong to that character/costume owner, with
their motion and event timeline alongside it. Group victory rules have their own
`battle/victory/groups.json` document.

Equipment item IDs resolve through `meshes/equipment/mappings.json` to named
`equipment.json` documents. Each explicitly declares a weapon, shield or
nonvisual attachment. Visible parts reference their model documents, retaining
native part slots, independent layer skeletons, blade trails and texture animation
settings. Item names come from a maintained naming catalogue, independently of
edited menu captions. Unidentified battle-only props retain scoped numeric names.

Enemy IDs resolve through `battle/enemies/mappings.json` to a named `enemy.json`.
That entry names combat `rules.json`, the battle `model.json` and its effect bank.
The battle model references a separate `rig.json` and the shared monster preview
under `meshes/monsters/<name>/model.json`. Carried models and secondary bodies
reference that same owner's geometry and textures; battle-specific motion banks
remain explicit. Additional weapon variants have separate owners. A secondary
body used as an effect shares its geometry while retaining the effect's own
native model slot and motion bindings.

Capture the current monster preview with the battle rig. A preview retained when
the field first loaded must not silently override later model edits. Combat rules
remain required; failed optional artwork or effects retain explicit diagnostics.
Neither a source hash nor a file inventory belongs in these authoring documents.

Stored technique scenes resolve through `battle/scenes/mappings.json`; skill
catalogue scenes resolve through `battle/skills/mappings.json`. Each named
`scene.json` refers to `rules.json` and `effects.json`, with optional cut-in and
temporary weapon definitions. Native scene model slots and texture bank 6 are
views of the effect bank, so their editable references exist only once. Rules
retain contact recipes, participant timelines, camera/lighting settings and
explicit unsupported-action diagnostics. Names come from pinned original labels,
joined through the native technique IDs rather than current menu captions.

`battle/events/mappings.json` selects named event directories containing
`event.json`, scenario bytecode, messages and actor model documents. Actor bindings
preserve native script resource IDs. External animation resources keep their own
clip-slot scope alongside each actor's ordinary motion bank. Party and Vidarr
geometry remain shared with their declared character/monster owners; event motions
retain their own bindings. The entry explicitly names `battle/events/blink.json`
instead of requiring hidden embedded data. Audio remains a declared battle selection
and joins the encounter's named audio preparation.

Before entering a battle, derive its prepared resources from the current
definitions and effective mod view. Include declared enemy reserves, eligible
equipment changes, party techniques, possible combined attacks, arena effects,
event scenes, victory choices and their audio. Preparing only the initially
visible participants would leave valid battle choices dependent on late file
reads. Preserve existing unsupported-resource diagnostics rather than treating a
readable filename as evidence that a previously unsupported behavior now works.

This is a coordinated publication and loading change. A named field must be able
to start a battle without carrying the old battle file inventory, and a battle
must not recover missing named resources through legacy numeric or hash paths.

## Loading, preparation and generated data

The named content tree is the source of truth. Runtime discovers dependencies
from the current descriptors, mappings, bank declarations and script interfaces.
It prepares the required resources before they are needed by gameplay.

Preserve preparation coverage across story branches, dynamic selections and
runtime services. Dependency discovery must include valid declared possibilities,
not only references exercised by the current scene. For choices that cannot be
enumerated statically, use the relevant declared bank or resource scope as the
preparation boundary. Do not replace complete preparation with late disk reads
inside gameplay updates.

Replace persisted, authoritative preload inventories with derived preparation
results. Keep useful internal concepts such as dependency roles, cancellation,
shared decoded bytes and readiness checks. The changes concern how the inventory
is obtained and which bytes are considered valid.

- Validate schemas, reference targets, supported formats and required relationships.
- Read a consistent content snapshot for each preparation operation. Compile and
  prepare from those bytes; do not validate one version and later reopen another.
- A snapshot pins each resource when it is first read; it does not make an arbitrary
  set of filesystem edits transactional. Finish related edits before reloading.
  Capture mapping overrides for that same preparation and reject inconsistent
  references with their originating descriptor identified.
- Treat edits made after preparation as changes for the next explicit reload.
- Compute hashes internally where useful for cache identity or buffer reuse.
  Do not compare legitimate edits against mandatory cooker-recorded hashes.
- Cache derived dependency information, compiled scripts and converted media
  outside the editable tree. Invalidate it from the effective current inputs,
  relevant settings and tool/schema versions. A deleted or stale cache must rebuild
  automatically without a full extraction or cook.
- Keep movie payloads streamable and preserve a consistent file lifetime for an
  admitted playback operation.
- Optional verification against an original installation may report differences;
  it must not be the normal gate for running edited content.

Moving existing manifests into a hidden directory is insufficient if users still
need to regenerate them. Invalid content should produce a useful error naming the
resource, referencing descriptor and failed requirement, rather than a generic
instruction to recook because bytes changed.

### Cooking and publication boundary

The importer can retain its dependency graph, source identities and temporary
deduplicated output internally. Its public output must be the complete named tree,
with every runtime reference resolving inside that tree or an enabled mod. Loading
must not depend on an intermediate hash directory left by the cooker.

Publish a complete generation only after its mappings and references validate.
Use a separate output directory during migration and keep failed staging output
separate from a usable library. Repeated cooks of the same inputs and naming
catalogue must produce the same names and bindings, independent of worker order.

A recook regenerates the base library; it is not a merge operation for direct
edits. Make replacement of existing output explicit, and never silently treat a
user-edited tree as disposable staging. Separate mod directories are the durable
workflow for retaining changes across recooks. Direct editing remains supported
for experimentation, with that persistence limitation documented.

## Mod directories

Direct editing of a cooked tree must work. Also support separate mod directories
using the same paths so users can recook the base library without overwriting mods.

Resolve resources through an ordered view of the base tree and enabled mods:

- Ordinary files replace the same logical path as a whole.
- Mapping overrides merge by machine ID within their lookup scope. A mod replacing
  one field must not copy the complete field catalogue.
- Use explicit mod order. A later enabled mod wins a conflicting path or mapping
  entry, and diagnostics must identify both contributors and the selected result.
- Resolve references against the same effective content view, allowing a replacement
  descriptor to use unchanged base resources.
- Derive dependencies and cache identities after applying the overrides.

Start with file replacement and mapping-entry overrides. General JSON patching,
automatic conflict resolution and resource deletion semantics are separate work.
Disabling a mod should restore the base resources without rewriting them.
Enable directories with repeated `--mod DIRECTORY` arguments, in ascending
priority. The selected library must follow title/boot/movie loading, field/world
workers, battle entry, preview preparation, save browsing and restoration.
Recording and test-save entry points must use the same selection too.

For example, a mod can contain only
`fields/iselia-1/messages.json` to replace dialogue. It needs no mapping override
because the existing field already refers to that logical path. A mod that moves
field ID `1` to `fields/iselia-remade/` supplies
`fields/mappings.json` containing `{"1": "iselia-remade"}` and the replacement
entry point and resources it changes. References to unchanged base files continue
to resolve through the effective content view.

## Save identity

Preserve original machine IDs. Directory names, physical file locations, formatting
changes to equivalent data and presentation-only replacements must not themselves
invalidate saved gameplay state.

Separate content snapshot identity used for caching from gameplay compatibility
used for saves. The gameplay projection in `content/src/save_identity/projection.rs`
is the starting point; stored cook-time fingerprints are insufficient
once scripts and gameplay definitions are editable.

Compute compatibility information from the effective content view, including mods.
Reject saves with a different schema or gameplay identity. Do not provide an
upgrade, repair, alias or migration path. Do not use the whole content
tree's byte hash as a substitute, and do not silently admit a save against changed
gameplay merely because the original cooked identity file is still present.

Physical path changes alone should preserve compatibility. Script format changes
may need separate treatment, especially where saved state depends on execution
positions or persistent variable layout. This design does not promise arbitrary
script edits will preserve existing saves.

| Change | Required save behavior |
| --- | --- |
| Move a field directory and update its mapping and affected references. | Preserve compatibility when its original ID and gameplay meaning are unchanged. |
| Reformat JSON or change source comments and whitespace. | Preserve compatibility through normalization of the relevant definitions. |
| Edit dialogue, artwork, audio or movie payloads. | Preserve gameplay identity; validate the replacement separately for runtime use. |
| Change native scenario bytecode, authored gameplay instructions or persistent-state definitions. | Change gameplay identity and reject existing saves that no longer match. |
| Load a save from an obsolete schema or change the identity algorithm. | Reject the old save; start a new game. No migration bridge. |
| Leave an old `save-identity.json` beside edited gameplay files. | Compute from current effective inputs; the stale file cannot authorize a save. |
| Enable a mod that changes gameplay. | Apply the same checks to the merged content view used by preparation. |

Normalization must distinguish a physical asset path from a logical script module
or symbol identity. Hashing every source token indiscriminately would still reject
a harmless texture-path rename inside a resource declaration. Conversely, removing
every string literal would hide meaningful gameplay edits. Define that distinction
from the script language's declarations and types, and test it explicitly. Source
normalization is not a promise that arbitrary refactoring is save-compatible.

Changing the fingerprint algorithm can bump the save schema and invalidate old
saves. Accept only identity derived from the currently prepared content. A stored
authoring fingerprint, an old schema or a supplied alias cannot authorize loading.
The current identity schema is 7. There is no bridge from any older schema.

## Implementation sequence

### 1. Establish the content contract

Audit runtime ID namespaces, ownership, path construction and hash checks. Define
entry points, mapping schemas, naming rules and the layout version. Build the
initial stable name catalogue, retaining deterministic fallbacks for unidentified
resources. Record scoped IDs and known variants before flattening any catalogues.

### 2. Introduce the resolver and migrate fields

Add a shared resolver for IDs and logical paths. Route field metadata, audio,
preparation and save-identity reads through it. Publish named field directories
with readable references and separated editing boundaries. Keep the actual
geometry, audio and script decoding behavior unchanged during this step.

Use representative fields to validate the format, including shared assets,
script-only fields, local resource banks and story-dependent choices. Shared
resources required by these fields must have a coherent named publication path;
a pretty field directory pointing into opaque hash trees is not acceptance.

### 3. Replace immutable publication admission

Derive preparation from current content and remove mandatory stored hash/size
checks throughout consumers, including nested audio, movie and script loaders.
Preserve structural checks, consistent snapshots and preparation readiness.
Implement automatic cache invalidation and an explicit reload boundary.

Steps 2 and 3 form one usable migration milestone: a named field must actually be
editable without regenerating preload manifests.

### 4. Migrate the remaining content domains

Apply the same ownership and mapping rules to music, speech, sound banks, movies,
shared models, animations, battle content, world content, UI and gameplay data.
Retain shared decoding and the existing cook scheduler where they remain useful.
Move source indexes and conversion provenance out of the runtime contract.

### 5. Add mod overrides and update save compatibility

Implement ordered path overrides and mapping-entry merges. Prepare the effective
content view and identify conflicting contributions. Derive save compatibility
from that same view, preserving presentation-only edits and detecting meaningful
gameplay differences.

### 6. Complete authoring support and transition

Add the explicit media replacement and editing paths described above. Track
original-scenario editing and motion authoring as distinct deliverables with their
own fidelity requirements.

Update cooking and authoring documentation with examples that use the final
layout. Produce a fresh library in a separate output directory, validate it, and
then switch the runtime's expected layout version. Remove obsolete production
readers and migration commands; unsupported layouts request a recook and
unsupported saves require a new game. Preserve
existing user-edited content during the transition; a recook is not a mod merger.

## Acceptance criteria

The redesign is accepted when the relevant migration milestones demonstrate:

1. A person unfamiliar with the importer can find a known field and identify its
   dialogue, scenery, collision and shared character references from the files.
2. Changing dialogue, replacing a supported texture or editing an authored script
   takes effect on reload without editing a hash, byte count, source index or
   preload inventory, and without recooking the original discs.
3. A local texture edit affects its owner; an intentional shared-model edit affects
   all consumers. Identical original bytes do not create hidden edit coupling.
4. Field transitions, scoped bank lookups, movies, audio and saved machine IDs
   resolve correctly through mappings. Missing or ambiguous targets fail clearly.
5. A mod can replace one file or mapping entry while inheriting the rest of the
   base library. Conflict order is deterministic; disabling it restores the base.
6. Deleting generated caches changes preparation cost, not content behavior.
   Editing a dependency invalidates the relevant derived work automatically.
7. Renaming paths with their references updated and making presentation-only edits
   preserve save compatibility. Meaningful gameplay changes cannot reuse a stale
   cook-time fingerprint unnoticed.
8. Conversion coverage is retained. Representative field, battle, world, menu,
   script, audio and movie routes preserve their behavior and preparation readiness.
9. Cold and warm preparation time, runtime stalls, memory and disk use are measured
   against the current baseline. Disk duplication is an explicit tradeoff for
   understandable ownership; loading regressions must not be hidden in gameplay.
10. Production loading accepts only the current layout and save schema. Missing
    named resources fail directly. No old-path fallback, save repair command or
    compatibility bridge remains.
11. Every resource category uses the same disc-independent library. Installed
    paths, mappings and resource documents contain no disc identity or ownership;
    runtime lookup never selects a disc. Import diagnostics remain outside the
    library. Genuine gameplay variants use their gameplay meaning as their scope.

Use focused resolver, override and invalidation checks, plus representative
runtime comparisons and a complete cook for final format integration. Keep
specialized format fidelity checks tied to the formats actually changed. A
successful file rename or conversion run alone does not establish acceptance.

## Initial implementation touchpoints

- `crates/content/src/field.rs`: current field path construction and field schema.
- `crates/content/src/field_preload.rs` and `prepared.rs`: dependency inventories,
  admission, verified snapshots and resource reuse.
- `crates/import/src/all_assets.rs`, `field.rs`, `shared.rs`, `scene/` and
  `media/`: publication paths, source-package references and resource preparation.
- `crates/content/src/field_audio.rs`, `field_audio/archive.rs`, `lib.rs` and
  `crates/audio/src/package.rs`: media references and embedded integrity checks.
- `crates/presentation/src/`: field/title entry, resource preparation and consumers
  that construct current paths directly.
- `crates/import/src/save_identity.rs` and save admission: effective gameplay
  compatibility instead of an unchanged stored fingerprint.
- `docs/cooking.md` and `docs/symphonia-script.md`: replace the current immutable
  cooked-resource contract with the implemented editing and reload workflow.

## Implementation record

### Commit checkpoint — 2026-10-24

This checkpoint preserves the implementation so far; the overall plan remains
incomplete. Named resource directories and scoped mappings, ordered mod overrides,
editable PNG/motion/script documents, shared offline WAV music, current-content
save identity and removal of obsolete cooked readers are implemented in the
worktree. The no-disc-metadata rule applies to every installed resource category.
Source diagnostics remain outside the library. Application restart is the
supported edit reload boundary; backwards compatibility is deliberately absent.

Completed verification is recorded below: workspace/all-target compilation,
Python syntax checks, formatting and the affected-library rerun passed. The first
full workspace run had failures subsequently fixed and covered by that rerun;
it must not be described as an uninterrupted green workspace run. The rebuilt
development importer also completed successfully after the audio JSON fix.

The environment interrupted the later integration processes. Their process
handles are gone, and their logs contain no completion result:

- The full cook reached general-asset job 13,900 of 14,037 before interruption.
  No completed library was installed, no final coverage report was admitted, and
  no cook timing result exists. This run used the importer from before the audio
  JSON fix; a fresh cook must use the rebuilt importer.
- The native audio run completed the identical-recording sharing check and began
  the full soundtrack and representative loop comparisons. The complete audio
  suite and the separately started five native battle checks did not finish.
- The isolated pre-change release benchmark build completed. The current release
  build first encountered a stale baseline compiler artifact in the shared target
  directory. Project release artifacts were cleared; the clean rebuild and queued
  player build were interrupted before completion. No current release success or
  runtime performance comparison is claimed.

Logs and incomplete scratch remain under
`local/layout-migration/final-integration-20261024/`, outside committed source and
the installed library. The preserved baseline binary is
`baseline-frame-benchmark` in that directory. Do not treat incomplete scratch as
a usable library or overwrite existing cooked content and user edits.

Remaining integration work, in order:

1. Finish the current release builds and a fresh complete cook; resolve actual
   conversion/admission failures, then confirm external source coverage and the
   absence of disc metadata in every installed domain.
2. Complete the native media/geometry/gameplay fidelity checks and representative
   boot, title, New Game, field transition, battle, world, menu and ending routes.
3. Complete mod replacement/rename/disable, gameplay save identity and save/restore
   acceptance, including renderer behavior after application restart.
4. Measure cold/warm preparation, runtime stalls, memory and disk against the
   preserved baseline without concurrent builds or captures; record limitations
   and address unacceptable regressions.
5. Reconcile the authoring documentation and audit every acceptance criterion
   against completed evidence before declaring the plan implemented.

### Final integration pass (in progress)

The implementation batches are now in integration. Workspace/all-target
compilation passed after correcting stale fixture readers and importer wiring,
including the shared-effect cache type, a deleted source-read helper and a
publication admission name collision. Native field,
world, skit and menu fixtures now use current named readers. Profile edits in the
equipment fixture use an actual mod directory, so encounter capture observes the
same override the fixture intends to exercise.

All 42 oracle Python scripts passed an in-memory syntax check. Rust formatting
completed. Removed the unused oracle disk reader identified by compilation; the
following test build also covers that cleanup. The complete workspace test run
finished with 2,345 passed, 24 failed and 470 ignored. Collected the failures
before fixing them: the synthesized-effect tag exposed a numeric JSON map-key
deserialization bug; the remaining failures were outdated error expectations and
incomplete synthetic fixtures. The rerun of the four affected library targets
passed all 826 enabled tests (312 ignored), including the new tagged-audio
regression coverage. No fresh complete cook, runtime acceptance or final
performance result is claimed yet. The earlier unrun notes below describe the
state before this integration pass.

### Final legacy field and inventory cleanup (implementation in progress)

Deleted the obsolete field metadata/audio/preload path helpers and deserialization
of the assembled field aggregate. Native field behavior fixtures and development
probes now resolve field IDs through the current mappings and assemble the named
documents. The classroom speech fixture and voice-content oracle follow current
speech mappings and WAV headers. Removed the unused field-audio manifest reader,
including its tolerant entry decoder.
Prepared voice facts and credits aggregates also no longer deserialize; their
inputs are the current WAV files and named credits definition.

Deleted the old preload module, including its field/shared manifests, stored file
inventories and roles. Prepared battle scenes, models, effects, arenas, events and
victory data no longer carry empty copies of those inventories. The optional
subtitle fixture uses an in-memory capture with the selected diagnostic policy.
Source world-tile catalogues now live only in the importer. World terrain,
encounter and script fixtures use current named world preparation; skit fixtures
use the current catalogue and mappings. Removed unused source hashes from prepared
world tiles and skit clocks.

Removed source-evidence strings from public pattern-spell, title-award and world
cinematic definitions. Maintained importer recipes still retain and check that
evidence, without copying it into the installed library. This applies the same
boundary to every domain: source/disc accounting is an importer concern; cooked
documents contain resource semantics and direct references.

These edits and fixtures have not been built, tested, formatted or cooked. Finish
the implementation before final validation. The complete fresh cook, full runtime
routes, save/mod acceptance and fidelity/performance measurements remain open.

### Remaining aggregate readers and attachment identity (implementation in progress)

Removed deserialization from prepared battle scenes, effect artwork, equipment,
party-rule tables, projectiles, recoil, formations, Defense System, Unison and
victory performances. Their current named documents remain the input schemas;
the prepared aggregates are memory values, not an alternative cooked format.
Ported the remaining native weapon fixture from the deleted publisher and old
combined JSON reader to direct preparation, named publication and the current
equipment loader. It retains the original item/slot, shield/prop alias, trail,
independent-layer and native motion checks. Scene recipe and Defense System
fixtures now exercise current document admission.

The attachment identity corrections are described below. Cooking documentation
now describes editable scripts and mod overrides, PNG publication and external
diagnostics instead of immutable preload inventories and recipe hashes.
No build, test, formatter, cook or validation pass has run for this batch. The
remaining full source binding/accounting, optional gameplay resources, snapshot
costs and end-to-end acceptance still require completion and final verification.

### Current battle and field gameplay identity (implementation in progress)

Expanded the derived identity to shared battle rules, formations and encounter
pools, party profiles and action timelines, technique learning, Unison recipes,
Defense System behavior, recoil, enemy actions and projectile definitions. Named
party costumes, equipment, enemy bodies/attachments, scene weapons and effect
models contribute their contact rigs and decoded motion semantics, keyed by game
IDs and slots rather than file paths. Native battle event scripts and their actor
bindings participate too. Effect emission, model animation and particle motion
parameters are included because they can drive projectile contact recipes; image
payloads, colors, UV settings, sound/voice events and camera feedback are excluded
from that projection. Enemy entrance movement is retained separately from its
camera, tint and voice choices.

Field identity uses current assembly for ground/region collision, door
interaction parameters, actor-local collision and selected motion completion
times. Actor motion overrides use the same merge as runtime preparation. Field
attachment queries also use indexed GLB skeletons, rigid bounds for block
interaction and live motion curves. Those semantic inputs now pass through one
reader shared by field preparation and save identity. Materials, texture data
and vertex payload bytes do not enter the identity; a rig or rigid-bound change
does because it changes gameplay. Reading a GLB to extract these inputs is
necessary under the current field attachment contract. Scenery translation alone
does not change the separate collision documents. All reads use the same
effective snapshot, including mappings and mod replacements.

Battle identity includes secondary bone motion and independent component rigs,
which participate in the poses used by contacts. Body and event projections
select the same primary model part as runtime; weapon and effect projections
retain their selected layers and playback. Raster options, field-independent
display translations, head feedback anchors and Over Limit burst artwork are
excluded. Added unrun coverage for secondary motion, field attachment bounds,
material-only changes and particle color/raster edits versus movement and random
draw changes. Malformed field attachment child references and invalid bounds now
produce parser errors rather than unchecked indexing.

The review also found gameplay outside the old shared projection. Monster List
records hold the enemy statistics, drops, stealing and grade/reward values used
by combat; these now participate independently of their captions and previews.
Title-award thresholds, crafting recipes/vendor choices, Grade Shop benefits and
prices, and shop visit groups now participate. The save identity schema is 7;
older identities are rejected, with no migration or stored-fingerprint override.

Added unrun coverage for contact-rig edits, motion timing, path renames, omitted
visual payloads, mod enable/disable and retained snapshots. Extended the native
identity fixture with field collision and monster-stat edits versus presentation
changes. No build, test, formatter, cook or validation pass has run. The full
native projection, optional/unsupported gameplay bindings, costs of retaining
the expanded snapshot, complete save/restore routes and the other acceptance
criteria still need final integration and verification. These changes do not
establish complete save compatibility or complete the overall implementation.

### One library independent of source discs (implementation in progress)

Applied the no-disc-metadata rule to every resource category. Removed the new
disc-specific music mappings, sound-bank directories, battle catalogue overrides
and field-to-battle catalogue routing. Music, effects and their instruments now
have shared named owners. The importer reads both source catalogues and renders
each song once; conflicting rendering inputs fail import instead of creating
another runtime namespace. Physical song conversion retains its per-setup source
coverage in external diagnostics and leaves WAV rendering to the central writer.

Battle rules, effects, arenas, scenes, victory, UI/fonts and session data now
publish to their ordinary shared roots. General converted resources use
`resources/physical/<original-path>` without a disc directory. Movies use names
such as `movies/opening/`; repeated sources share a conversion based on the native
movie bytes and color-conversion inputs. Conflicting movie ID bindings are import
errors. Original disc numbers remain internal source locators, never mapping keys
or cooked ownership fields. Actual game variants still retain their ordinary
semantic scopes, such as costumes and story alternatives.

The complete-generation installer now excludes all import reports from the
library. Source accounting, native tables and coverage remain in the separate
working diagnostics directory printed by the importer. On success, converter
scratch is removed and those diagnostics remain outside the installed assets.
Original-data fixtures now take that external directory through
`RESONANCE_TEST_IMPORT_REPORTS`, separately from `RESONANCE_TEST_ASSETS`.

Speech publication now merges the supplied native lookup tables and checks shared
archive definitions, rather than selecting the first available source. Generic
dynamic resources also reject conflicting physical copies. Repeated field IDs
must agree on their source data, dependencies and audio environment; resource
catalogues must agree on their game ID bindings, ignoring unused native storage.
Unavailable runtime resources carry only their availability state, without native
filenames or byte/header diagnostics. Original locations remain in the external
import tables. Added an unrun duplicate-source/order-independence fixture.

These changes and the adjusted fixtures are unrun. No build, test, formatter,
cook or validation pass has run. Finish implementation before final validation,
as requested. Original-data conflicts, complete source coverage, full gameplay
routes, save identity and fidelity/performance acceptance still require the
remaining implementation and final runs. This does not claim the overall task
is complete. Earlier notes that prescribe public disc scopes or installed import
reports are superseded by this section and the decision at the top of the plan.

### Work in progress and verification policy

Finish the implementation before running the validation suite. Do not repeat
checks after each incremental edit; use targeted debugging only for an actual
implementation blocker. Results recorded below apply to the revisions checked
at that time, not to later unverified changes.

The latest implementation batch removes old cooked audio selection inputs from
the named publisher. Field selections come from original map scripts; battle
catalogues, speech playback clocks, music environments, gains and spatial
parameters come from native resource tables. World selection comes from its
native script and cinematic declarations. Embedded cinematic speech shares the
event archive's WAV after source-PCM alias verification during cooking, with its
native clock retained in the world selection.

Field scene publication accepts an explicit current audio definition, allowing
synthetic fixtures to supply current documents without another reader. Complete
field-audio fidelity fixtures now discover original scripted fields and compare
against native compilation and decoding. Removed the obsolete staged complete
battle-audio comparison; current native effect, music and speech checks provide
the relevant source fidelity coverage.

These latest changes have not been checked. The pending native speech run was
stopped when the validation policy changed. Its earlier fixture assertion that
the configured archives must contain authored-empty lines has been removed;
those native archives contain no such entries. Production coordinator
conversion and deletion of the remaining temporary old publication machinery
are still required, along with the other acceptance work in this plan.

### Direct movie and startup publication (implementation in progress)

The production movie loop now converts each native physical source directly to
`movies/<owner>/video.mkv` and a current `movie.json`. Native executable tables
supply `movies/mappings.json`; repeated declarations of a movie share its named
owner. Disc locations do not create distinct public resources.
All source audio tracks stay in the container. Conversion keeps a typed result
for mux integrity checks and uses temporary raw YUV/WAV scratch, removed on exit.
It no longer writes/reloads an old movie manifest or searches `sources.json`.

Removed `MovieAsset`, numeric movie metadata paths, the old movie binder and its
cooked-format reader. Movie fidelity/cancellation fixtures and the playback probe
now use the current library and native H4M headers/PCM. The classroom fixture now
captures its field and opening movie through current named resources.

Startup publication now reads original TPLs and title declarations directly,
writing named title images (including authored mip levels), boot logos and the
current startup definitions. The old `boot.json`/`title.json` producers and their
translation reads are removed. The production finish path calls this publisher.

Game-over and credits now follow the same direct native path, including the
credits recording in `audio/music/credits/music.wav`. The shared production job
publishes current screen definitions, native TPL images and ADX PCM directly.
Removed the old credits binding reader, game-over inventory and their old root
paths. The native screen fidelity fixture compares current publication against
source text, pixels and every ADX PCM sample. Presentation fixtures now load the
current ending/title routes; the title fixture retains day/night scripts, WAV
playback, mod selection, cancellation and optional GPU capture coverage.

Removed the unused source digests from startup data; title presentation facts
are assembled from the current startup definition and content descriptor.

The world producer now returns its typed `overworld::Package` to the DAG
collector, and the final publisher consumes that object directly. Removed the
old world serializer/reader, `Package` deserialization and file inventory, and
old `FieldAudioCooker::world`. World media uses the native selection publisher;
its mappings are finalized by the same publisher as startup. Raw geometry,
images and decoded script payloads still use the converter's working paths.
The later implementation entry below records the field/party/arena switches.
Other battle/shared publication and final working-file cleanup remain pending;
this is not a completed clean output tree yet.

`cook-all` now requires an absent or empty destination. Named writes reject a
conflicting existing resource, including across separate publisher instances.
A publisher can consume raw converted payloads from the same fresh working tree;
this does not permit overwriting a previous cooked library or user edits.

World cinematic fidelity fixtures now resolve current named content and compare
embedded native speech PCM, with no old path/hash assertions. Presentation's
mutable native opening/world fixtures copy an existing complete current library
into independent temporary files. They no longer translate an old staging tree.
These full-library copies have a disk cost that remains part of the pending
measurement work.

This batch has not been validated; implementation continues under the policy
above. Other domains still need their production publication switch, and the
remaining temporary semantic staging machinery must be removed with that switch.
The subsequent field implementation entry records removal of field preloads.

### Direct field, party and arena publication (implementation in progress)

Field DAG jobs now return typed scenes, shared effect artwork, overlays and native
movie requirements. The final named publisher consumes those values directly and
writes field names, scoped mappings and audio selections. Field effects and
overlay preparation no longer write intermediate semantic JSON. The old field
metadata/preload writer, preload reader and per-field audio cooker are removed.
The original representative-field fixture now uses native preparation and the
current named writer; synthetic fixtures pass typed artwork explicitly.

The production finish path now publishes the native complete battle audio
catalogue. Removed the old battle-audio publisher, inventory reader and refresh
examples. The captured CRI speech regression now exercises direct native speech
decoding and current named mappings. Existing source-fidelity checks have been
ported rather than treating old cooked files as a permanent reference format.

Party jobs now retain typed costume records instead of numeric JSON files.
The terminal writer creates named character/costume bodies, rigs, animations,
wings and mappings. Victory preparation retains the native character and costume
IDs, and writes their named performance documents without joining on a body hash.
The old victory metadata serialization/reload and cached party refresh APIs are
removed. Victory resources use their character and costume scopes.

Battle event preparation now receives those typed party records and reads native
scripts, event motions, Vidarr artwork and blink timing. It returns typed scenes
to the named event publisher. Removed numeric event files, their inventory
builder, the old party/enemy/monster staging reads and embedded-blink JSON read.
Raw geometry, images, curves and decoded script payloads remain converter scratch.
The current event documents contain explicit actor owners and scoped mappings.

Arena preparation now returns typed scenery and its effect bank. The production
loop publishes current arena owners, layers, textures, effects and mappings
directly, including physically present unused arenas. Unsupported presentation
remains explicit. Removed numeric stage/effect JSON publication, stage inventories
and their named-writer reloads. Arenas use their shared named owners.
Runtime assembled party, event, victory and arena objects no longer derive
old-format deserialization; their current document readers remain the sole input.

The affected native fixtures now use original-disc preparation or current named
content. The persistent full battle fixture invokes the production full cooker;
it no longer translates an old staging directory. Costume encounter discovery
uses scoped mappings. Required-resource failure fixtures apply actual mod files
before a fresh snapshot, so their edits reach the production preparation path.

Source-output existence accounting now follows terminal publication, because
party jobs return typed values before their named files exist. Final output
cleanup and complete accounting still need the remaining domains converted.

This implementation batch has not been compiled, tested or formatted. Validation
remains deferred until implementation is finished. Subsequent production switches
for shared UI/session/skits, battle equipment/enemies and dynamic resource owners
are recorded below. Script module names and physical-audio legacy output still
need their production switches and old readers removed. The complete acceptance criteria, authoring
workflows, GPU reload boundaries, end-to-end routes and measurements remain open.

### Direct shared battle rules, UI, effects and stored scenes (implementation in progress)

The production cooker now prepares resident effect banks and their artwork as
typed values, then writes named effect programs, particles, models and textures.
Native palette bindings stay in memory and are shared with stored scene and
monster preparation. Removed the resident-bank metadata reader, hashed variant
lookup and cached-effect refresh APIs. The common effect artwork no longer builds
an old file inventory. Shared effects use one ordinary named owner.

Battle UI preparation returns typed artwork; its terminal writer creates the
named HUD resources and separate font. Removed `battle/ui.json` publication and
its readers, including the inline-font decoder. HUD fixtures now load the current
UI document and font. Native UI fixtures still compare the published portrait,
command-atlas pixels and font against original texture data.

Shared battle rules now have a native aggregate reader and a terminal named
writer. Formations, party profiles, normal/martial attacks, Defense System,
Unison, projectiles, recoil and techniques no longer serialize an old schema and
reload it for named publication. The coordinator's reuse key includes all three
native inputs: executable, resident archive and battle module. Shared preparation
no longer reads battle UI/effect inventories or republishes old Unison data.
The rule and UI fixture now compares native prepared values with current captured
and loaded documents, while retaining mod and snapshot checks.

Stored spell/skill preparation now returns typed scenes and effect banks.
Removed its versioned conversion catalogue, numeric JSON writers, old cache
admission/reload and semantic refresh path. Full cooking publishes named scene
rules, effect artwork, cut-ins and temporary weapons directly. Technique IDs,
skill catalogue IDs and unreferenced physical skill-package slots have separate
mapping scopes. All physical packages remain represented; failures stay explicit.
The native choreography fixture derives coverage from the original catalogue and
compares the resulting named scenes and current dependencies.

Removed the cached enemy refresh command and the unused metadata refresh APIs
that supported it. Enemy model/equipment production was converted in the batch
described below. Native palette-window tests retain their direct-source behavior
coverage; checks that existed solely for the deleted cache refresh path are gone.

This batch is unvalidated: no build, test, formatting or validation pass has run.
Enemy/equipment ownership and shared UI/session/menu/skit publication were
converted in the batch below. Dynamic resources, readable script module names,
physical-audio staging removal and clean final output publication remain open.
Some source-oracle comparison helpers still read old pinned scene/effect
metadata. Those evidence consumers must be ported to current resources with
independently pinned proof inputs; they are pending work, not an exception to the
reader-removal goal. Full acceptance, authoring workflows, GPU reload behavior,
complete gameplay routes and performance/fidelity measurements remain open.

### Direct enemies, equipment, menus and physical field recipes (implementation in progress)

Native enemy preparation now returns the Monster Book preview, battle rig and
components, gameplay rules and effect bank as one typed value. The production
menu job sends these directly to named enemy owners and publishes their scoped
mapping. Removed numeric enemy/model/effect JSON writes, the metadata-only
monster refresh path, the selected-enemy refresh publisher, hashed dependency
inventories and all corresponding named-writer reloads. Equipment preparation
likewise returns typed attachments; its writer publishes named item owners and
`meshes/equipment/mappings.json` directly. Source accounting now points to those
final entries instead of numeric metadata paths.

The shared menu publisher now writes named monster and figurine model references
from prepared catalogue values. Figurine preparation no longer emits a separate
numeric JSON file per record. Menu artwork, dialogue windows/cursor/font,
subtitles, fresh-game data, localized text and skits pass directly into their
terminal writers. Removed the generic shared staging translator and its source
JSON reloads, the old font/layout publication from executable scanning, and the
old combined skit catalogue write. Shared session data lives under `game/`.
Raw image, motion, geometry, script and speech conversion still uses scratch
payloads; it does not serialize and reload those domain objects.

Caller-bound field geometry now retains typed physical recipes and optional
local palette catalogues alongside the field artwork. Its final writer creates
the named scene and palette documents. Removed the hashed physical-scene staging
write and the last generic source-JSON reader in the named publisher. Current
content can still be captured and republished through its explicit document
readers. This change does not yet remove the generic physical walker's separate
inspection/publication outputs.

Enemy catalogue execution observations now enumerate scoped mappings and capture
current rule/artwork owners together. Removed the old numeric path helpers and
assembled enemy/model deserializers. The affected gameplay fault fixture edits a
current enemy descriptor in a real mod directory. Native enemy/equipment fixtures
retain their complete resource comparisons; monster/figurine/menu/font/session
fixtures now use original input or current named content. These fixture changes
have not been executed.

No build, test, formatting or validation pass was run for this batch. Per the
user's instruction, validation remains deferred until implementation is finished.
Dynamic resource ownership is addressed in the batch below. Readable script
module names, generic physical/audio output removal and final content
descriptor/clean-tree publication remain open. Source-oracle proof consumers for
stored scenes and effects still require current-resource migration with their
independent proof pins preserved. Full acceptance routes, source coverage,
ordinary-WAV effect replacement, authoring workflows, GPU reload boundaries and
performance/fidelity measurements remain open; this is not completion of the
plan.

### Scoped dynamic resource publication (implementation in progress)

Replaced the old `game/resource-catalogue.json` availability rows and hash archive
paths with `resources/catalogue.json`, separate standalone/group mappings, and
named resource owners. Names come from native declarations with their scoped ID,
so equal bytes do not merge unrelated entries. Native groups are keyed from one;
member mappings preserve the original empty-member fallback to member zero.
Declared missing sources and the empty group remain explicit documents.

The writer reads native sources directly. It publishes physical geometry and
palette recipes, complete texture pages, skeleton-independent editable animation
curves, collision groups and recognized scripts. Compound model packages keep
named body/outline/motion/collision parts and their native slot mappings.
Unclassified resources have an explicit unsupported description; recognized
formats with invalid data fail preparation rather than being silently omitted.
Decoded model caches are bounded to a single resource package during this pass.
The generic physical walker still retains its broader source-recovery outputs;
removing that older output path and reconciling full source accounting are still
required work.

Field capture now follows the dynamic catalogue's current scoped references,
including nested palettes and meshes, through the same snapshot. Capture rejects
cycles and honors cancellation. The script resource service can report the named
owner and the specific missing-source/binding limitation when a request has no
prepared binding. Existing prepared bindings continue to take precedence. This
is physical availability, not a claim that arbitrary dynamic model/palette/motion
combinations are executable; that pre-existing generic binding limitation remains
explicit. Fields 544/545 retain their dynamic catalogue declaration.

Source inspection confirmed the original standalone/group scopes used by the
fixtures: 53 standalone slots, including 19 absent sources; 14 groups, including
one empty slot. Added fixtures for ID scopes, native aliases, independent owners,
partial mapping overrides, nested dependencies, retained snapshots, cancellation
and cycles, plus an original-disc catalogue coverage fixture. These have not run.
No build, test, formatting or validation pass was run for this batch.

The next batch below addresses readable script module ownership and generic
physical/audio output. Final clean-tree/content-descriptor publication remains open. The independent source-proof
migration, complete gameplay routes, save identity coverage, authoring workflows,
GPU cache behavior and performance/fidelity acceptance remain open.

### Generic physical resources and native audio publication (implementation in progress)

Validation remains deferred at the user's request. This batch changed publication
and ported the affected fixtures; no build, test, formatter or validation pass ran.
The complete implementation and acceptance work below remain open.

- Renamed 1,764 remaining hash-named node modules to maintained names derived from
  their original source/member owners. Modules with no known owner use their node
  selectors and count. Each records the naming evidence in a `Source owner`
  comment; the internal skeleton signature still drives importer lookup. Existing
  catalogue names remain in place. No old module aliases were added.
- Generic physical files publish under
  `resources/physical/<original-path>` instead of `assets/<hash>`. Native
  directory slots use `mappings.json` and `member-NNNNN` owners, preserving explicit
  empty slots and native range aliases. A field's embedded model table maps its
  actual script resource IDs to `model-NNNNN` owners. Cabinet filenames remain
  ordinary directories and no cabinet manifest is written.
- Generic meshes/materials and complete texture palettes now pass typed native
  data directly to the same named physical writers used by fields and dynamic
  resources. Animation exports use the current editable curve document. Raw mesh
  and image payloads still need final scratch cleanup; native skeleton/camera and
  storage-audit output still needs the final authoring/report boundary review.
- Generic AFS speech and standalone/embedded streams publish ordinary named WAVs.
  Archive member mappings point directly to nonempty WAVs; authored empty members
  are recorded under `reports/`. Skit preparation now decodes original archive
  members directly. Deleted the cooked voice-index/voice-descriptor reader, the
  old field-voice binding adapter and the content `VoiceArchive` schema.
- Generic songs now use the existing exact-loop WAV recorder, including inherited
  field/battle studio variants. No synthesizer score package is published for
  runtime music. Unbound arrangements are retained in source reports without
  guessing instruments. The later producer-claim change below consolidates repeated
  renders for the same named owner and native inputs.
- Generic sound banks now emit current editable programs and named instrument
  WAVs. Program and sample IDs have scoped mappings; native tables and provably
  absent dependencies go into reports. Native bank groups determine the source
  owner, including separately stored sample payloads. Removed the old standalone
  compiled-resource export structure.
- Removed the test-only character-layer, cabinet and physical-member readers.
  The native character fixture now publishes and opens the current model schema,
  preserving geometry, material, texture, motion and collision comparisons after
  deleting intermediate physical metadata. Voice fixtures use real native ADX,
  and the original-disc voice checks now cover current publication and native
  archive order, empty members, clocks, PCM dimensions and both-disc fallback.
  All these fixtures are unrun.

The final clean tree and content descriptor, full source accounting, remaining
old cooked readers (including independent oracle proofs), embedded/table outputs,
complete gameplay routes, save identity, editing workflows, GPU cache behavior
and measured fidelity/performance acceptance remain unfinished. Do not treat this
batch or its rewritten fixtures as proof of a complete production cook.

### Complete generation boundary (implementation in progress)

The production entry point now builds in a sibling staging directory. Terminal
named writers mark their authoring files through the existing in-memory
publication coordinator. That cook-local export set is not installed, consumed by
runtime, or maintained by authors. Source-only physical resources, maintained
scripts and native audit reports are explicit terminal trees; temporary meshes,
images, clips and semantic intermediates are omitted from the final copy.

The generation step copies exported files into an independent candidate, with
case/path checks and ordinary copies so equal source payloads cannot couple edits
between distinct owners. Original source accounting is projected onto the actual
exported files. Its reports now live at `reports/import-sources.json` and
`reports/coverage.json`; native table/audit exports live under
`reports/native-data/`. No old root `sources.json`, `coverage.json`, preload alias
or exported cooker inventory is added.

Added candidate admission through current schemas: shared gameplay and menus,
fonts/UI/startup, credits/game-over, skits, dynamic resources, fields and their
audio/movie selections, world, battle rules/UI/effects/arenas/enemies/equipment,
party/victory, stored scenes/events, catalogue model payloads, audio programs,
music loop headers, speech files, physical scene references and mapping targets.
Snapshots are released between resource owners. The descriptor exists only in the
private candidate until admission succeeds; an admission failure removes it.
The complete directory is installed with a same-filesystem rename. The destination
is checked again immediately before installation, and existing files or edits are
never removed. Failed scratch stays separate and the error identifies its path.

This is implemented code, not verification evidence. No build, test, formatter,
cook or validation command has run. Candidate admission itself still needs to be
exercised against a complete fresh generation, including unused/variant resources,
all declared scopes, memory limits and source-accounting completeness. The
publication boundary does not replace the gameplay/fidelity acceptance tests.

The native scenery fixtures no longer copy or read an old `sources.json` staging
index. The complete MAP comparison opens the current library and checks native
phase IDs, scenario bytes, layer ordering, geometry and autoplay motions against
original archives on both discs. Added unrun boundary fixtures for independent
file ownership, scratch omission, preserved direct edits and failed-cook isolation.
The source-exclusion fixture now reads the current diagnostic reports.

Remaining work includes old cooked-reader/type removal (notably original proof
consumers and older importer fixtures), complete variants/source accounting,
shared music publication, ordinary WAV effect replacement, script/image/mip
editing workflows, complete save identity and gameplay routes, GPU cache behavior,
and final fidelity/performance measurements. Keep the full acceptance criteria.

### Single-format reader removal and music work sharing (implementation in progress)

Removed the importer `cooked::Source` module and its old `sources.json`, cabinet
manifest and texture-binding reader. The affected original-disc fixtures now use
current physical `scene.json`/texture catalogues and `reports/import-sources.json`
for diagnostic provenance only. Native mesh/material, all-mip pixel and complete
camera word comparisons remain. The portrait declaration fixture compares the
renamed original payload directly. The current texture catalogue fixture compares
schema values instead of requiring the old compact JSON whitespace.

Deleted the unused serialized audio `Package`/`SampleAsset` staging schema, its
music and sound package writers, the remaining source-index helper and the old
hash-named sample writer. Native preparation returns typed synthesis inputs.
Named instrument publication owns WAV encoding and retains sample validation;
its fixture now checks current PCM output and rejection of existing edited files.

Removed the binary `RMOT` motion codec and automatic format detection entirely.
There is one versioned `.motion.json` format for raw converter payloads, named
publications, fixtures and runtime reads. Removed the second encoder API, updated
producers and references, and ported codec coverage to malformed current documents
and explicit float-bit comparisons across all curve modes, controls and matrices.
No binary motion compatibility or speculative compiled-cache codec is retained.
Animation behavior and performance acceptance still require the final runs.

Music publication now shares completed renders across generic source jobs and
later domain publishers in the same cook. The existing publication coordinator
keeps a separate in-memory producer claim, keyed by owner and all typed render
inputs (programs, score, synthesis tables, sample metadata/PCM and both reverb
variants). It retains no package, decoded PCM cache or installed marker. Matching
work reuses completed files without reopening a semantic descriptor; different
inputs for the same owner are a conflict. This removes duplicate synthesis for
the same owner. Distinct owners remain separate editable files. Generic source
accounting also includes a retained battle-studio WAV when one is produced.
The later no-disc-metadata decision supersedes disc-specific variant ownership.
Resolve conflicting source definitions during import; ordinary mappings and named
resources remain independent of the original storage media.

All changes and the ported fixtures are unrun. No build, test, formatter, cook or
validation pass has run in this batch, per the instruction to finish implementation
first. Original oracle proof consumers, complete variant/source accounting,
editing workflows, ordinary WAV effect replacement, gameplay/save identity,
GPU cache behavior and full fidelity/performance acceptance remain unfinished.

### Ordinary WAV effect replacement (implementation in progress)

Added an explicit current `sound.json` union: `kind: synthesized` retains the
editable native program and instrument WAVs; `kind: wave` references an ordinary
recording. The importer now emits the synthesized tag. Runtime readers and native
fidelity fixtures select the current union; untagged old sound documents have no
compatibility path. The WAV form derives sample rate, channel count and length,
supports mono/stereo integer PCM and float32 within bounded reads, and accepts
optional exact source-frame loops, gain, reverb sends and stop-release duration.
No loop blending or seam rewriting is performed.

Field, battle and menu cue preparation now produce one prepared-effect type.
Original cues keep their existing synthesizer. Recordings use their WAV clock,
retain stereo channels, and receive the same cue pan, source/group levels,
velocity scaling, mono setting and release commands. They reserve slots in the
existing bounded sound-effect allocator and participate in effect priorities.
They enter at the next unrendered block; dropped/stolen sources use the shared
release tail. Native macros and random draws are not fabricated for recordings.

Field/battle cache dependencies now include the current recorded WAV bytes as
well as its definition and environment. New preparations see changed WAVs or
mapping overrides; retained preparations keep their old captured bytes. Menu WAV
cues inherit the menu studio when no inline studio is specified. Documented the
replacement workflow and optional fields in `docs/editable-content.md`.

Added unrun coverage for stereo preservation, header-clock resampling, exact loop
wrapping, live controls and release, queue boundaries, shared effect priority
limits, malformed replacements, field mapping mods/snapshot retention/cache
invalidation and menu cue playback/completion. Existing native program fixtures
were ported to the tagged schema without weakening their score/sample comparisons.
Also added unrun battle-entry/return ownership and mixed native-program/recording
priority fixtures. No build, test, formatter, cook or validation pass has run.
All coverage still needs to be exercised with the final validation after
implementation. This is not acceptance evidence.

The remaining scope still includes original oracle proof consumers, source conflicts and full source accounting, image/mip and script editing workflows,
complete save identity/gameplay routes, GPU cache behavior and measured native
fidelity and performance. Preserve every full acceptance criterion.

### PNG texture authoring and explicit mip policy (implementation in progress)

Named texture publication now writes PNGs. Single linear levels are direct PNG
references; authored chains have adjacent `.texture.json` documents listing the
original levels. The publisher also copies current descriptors while preserving
their color and generation policy. Title, boot, defeat and credits artwork now
use the same terminal pixel writer; their direct native preparation does not
encode and reread an intermediate image document. Generic converter KTX2 bytes
remain scratch input to terminal PNG publication, with profile, orientation and
channel checks before conversion.

Added bounded RGBA8 PNG decoding/encoding and opt-in mip generation. Authored
levels retain every decoded pixel and are checked against their base dimensions.
Generated mips include odd-sized edge pixels and filter premultiplied linear
color, using the descriptor's explicit sRGB transfer when requested. PNGs above
eight bits per channel are rejected rather than silently losing precision.

The current snapshot captures the descriptor's exact PNG dependencies. A Bevy
image loader follows the same selected asset source for every level, including
preview sources; the synchronous UI path uses the same preparation rules.
Prepared images retain caller sampler settings and the descriptor's color
interpretation. Publication admission now decodes public PNGs and mip chains.
The ordinary authoring guide documents PNG replacement, preserved authored mips
and automatic replacement mip generation.

Existing publication and native-fidelity fixtures now compare decoded pixels and
every mip across the encoding change. Added unrun fixtures for exact PNG/native
mip preservation, odd-dimension and alpha-aware filtering, explicit sRGB,
malformed replacements, mod selection and retained snapshot dependencies,
republishing authored/generated policies, UI/scene agreement and nested PNG
loading from a selected asset source. The image loader fixture also covers warm
handle reuse and a new application reading changed PNG dimensions, pixels and
generated mips through an unchanged descriptor. It remains unrun and makes no
GPU-rendering claim. The skit portrait
fixture now captures current named documents instead of reading `game/skits.json`.
No build, test, formatter, cook or validation command has run for this work.
The supported boundary is application restart; in-process renderer caches are
not advertised as edit reload boundaries. GPU rendering after restart and full
end-to-end acceptance remain open;
this implementation record is not verification evidence. The complete remaining
scope and acceptance criteria still apply.

### Original scenario assembly editing (implementation in progress)

Exposed the existing lossless scenario assembler/disassembler through the
`symphonia-script-tools` CLI. The commands have explicit `.ssb`/`.ssasm` inputs
and outputs, bounded reads, path-specific diagnostics and atomic destination
replacement. Disassembly checks byte-for-byte reassembly; assembly uses actual
VM program admission before writing a runnable file. No original-disc extraction,
recook, inventory update or stored checksum repair is involved.

Documented the authority rule: the library's selected `.ssb` is authoritative;
working assembly is kept outside the installed library. Documented raw header,
registry and auxiliary-data layout constraints instead of promising high-level
decompilation. Removed the stale claim that edited scenarios are checked against
cooker-recorded hashes. Added an unrun command fixture covering unchanged-byte
round-trip, an instruction edit, preserved raw trailing data, independent mod
publication and failed assembly leaving the previous output intact.

No build, test, formatter, cook or validation command has run for these changes.
Original corpus round-trips, edited field execution, save identity behavior and
the complete remaining acceptance work are still unverified. Existing assembly
unit coverage remains in place.

### Independent battle evidence uses current documents (implementation in progress)

Battle comparison evidence now has an explicit current entry path, pins for the
current documents it reads, and a separate pin for independently captured original
bytes. These proof files and original copies live outside the cooked library.
They are comparison inputs, never author-maintained manifests or runtime
admission requirements. Production readers prepare the pinned named documents;
there is no fallback to a serialized old scene, effect bank or enemy definition.

Converted stored-scene registration, attached model joints, Acid Rain, stationary
area targets, ballistic particles, enemy reaction recipients and recoil readers
to this evidence. Current enemy mappings select rules, artwork, rig and preview
documents. Original package/module identities and parent/child byte ranges remain
independent proof obligations. Runtime effect ownership carries its current
logical entry path so an observed source-bank identity can be bound explicitly
without storing original hashes in editable resources.

The Python capture helpers now follow current scene, enemy, arena and effect
references and retain the selected meshes, motions and images in proof snapshots.
Projectile launch, trail, ground-effect and following-model descriptors select
typed current evidence; their original table bytes, observed RAM, action ownership
and launch recurrence comparisons remain in place. Relative-rotation and effect
birth-input readers use current effect documents and require agreement with the
capture's registered effect evidence. Existing synthetic projectile fixtures
were updated to the new proof schema.

Enemy case selection and prerequisite preparation now use mapped enemy rules.
Initialized body geometry resolves enemy artwork or the party/costume mappings
to its rig, retaining exact ordered original-RAM bone comparisons. Formation
selection follows the world encounter reference and arena mappings. Enemy
catalogue generation reads the current menu monster records and pins the current
metadata it selected outside the library; it no longer scans numeric cooked
directories or reads standalone old monster documents. Its capture fixture now
records the actual generated catalogue rather than using a rules file as its
catalogue fingerprint. Recoil binds projectile action evidence back to the same
typed current enemy rules used by the action catalogue.

Converted the remaining shared battle-preparation effect, projectile and recoil
fixtures to current documents. The projectile model-follow fixture now stores
current effect entry/program/actor documents; its particle and original compiler
checks retain the same declarations and behavior. Removed deserialization of the
prepared effect-bank aggregate, so it cannot serve as an old single-file cooked
reader. Dormant Fire Ball coverage identifies the prepared common bank by its
current document owner rather than comparing empty runtime source hashes.

This migration is unfinished. Complete source binding and the remaining old
semantic types still need attention before final validation. Added evidence
fixtures and updated behavior fixtures remain unrun. No build, test, formatter,
cook or validation command has run for this implementation batch. All acceptance
criteria below and the broader source accounting, variant ownership and
end-to-end work remain open.

### Current reader-removal status (2026-10-24)

There is no exception for old cooked-format readers in tests. Original-disc
parsers belong to the cooker; a test fixture is not a reason to ship or retain a
second cooked-content loader.

Removed the manifest/inventory admission API from `prepared::Files`, including
`load`, `load_with_diagnostics`, `from_inventory`, `with_dependencies`, stored
manifest state and rejection bookkeeping. `Files` now holds captured current
resources. Field access resolves the captured mappings and assembles the named
field document. CPU behavior fixtures capture gameplay and selected fields by ID.

Removed the old encounter, model-dependency, arena, stored-skill, victory and
battle-audio loaders. Encounter preparation uses one current snapshot and the
named audio catalogue. Victory selection has no body-hash argument. Original
oracle action fixtures use that same current capture, with an explicit option to
observe an isolated action without requiring its surrounding story route.

Removed the old title-music, title-playback and field-audio manifest/package
readers. Title test fixtures write current program, cue, music and startup
documents directly; field-audio fixtures use current WAV and program selections.
Battle coverage checks open the current library; they no longer prepare an old cooked library as
a parallel runtime. The earlier exact old/new comparisons below are historical
results, not a requirement to preserve those readers. Source fidelity checks may
still compare against original-disc parsers and independently recorded behavior.

The workspace/all-target check passed after the inventory, battle and title
reader removals. Seven title playback checks and the native opening encounter
atomicity/replay test passed. The converted suites passed 70 content tests and
294 game tests (47 native cases remain opt-in). All 18 native encounter cases
passed with recorded audio, advancing 10,800 simulation frames in 20.80 seconds
without an old cooked reader or baseline runtime. Formation tests retain
selected-row validation: an unused malformed row cannot block a valid encounter.
The complete presentation audio filter passed 69 tests (13 native cases ignored),
including the converted field/battle selection and playback checks. The final
workspace/all-target check passed after field-audio reader removal. This does not
complete direct cooking, the remaining old fixture cleanup or the full plan.

Removed the low-level old audio package loaders and their conversion helper.
Current program preparation reads its JSON and ordinary sample WAVs directly;
it derives format facts and cache identity from captured bytes and editable
tuning/loop settings. Edited sample files need no authored digest. Native audio
fidelity fixtures prepare original-disc compilation directly or capture current
named programs. Tests of obsolete hash admission were replaced with checks of
current WAV content, loop bounds, cache invalidation and shared sample lifetime.
The old cooked-score example and an obsolete loop-inspection diagnostic were
removed.

Field, battle and title music now have only recorded-WAV playback, including
synthetic fixtures. Removed the dormant synthesized-music variants and fallback
maps. The synthesizer remains for live sound effects and offline music cooking.
The recorded fault-isolation fixture exercises a real WAV page-read failure.

The current audio library passed all 129 tests, including native cue completion
and recovery-volume cases captured from the named library. Four native importer
checks passed in 17.02 seconds: all 2,434 playable effects and 5,146 sample
bindings in both environments, the four menu cues, native instrument-closure/PCM
checks, and layered randomized cue completion. These checks do not load an old
cooked audio package. All 69 presentation audio tests passed (13 native cases
remain opt-in), including the recorded-WAV failure-isolation test, field/battle
handoffs, fades, queued PCM, pause/resume and shared-effect behavior.

Remaining reader cleanup includes the old production coordinator's selection
code and its fixtures, which still deserialize obsolete cooked documents.
Private old audio record structs remain
only inside the importer's temporary staging path, with no runtime loader; they
must disappear with that path. Production cooking still needs direct named
publication. The temporary old-output staging translator is unfinished work,
not a retained compatibility layer or an accepted final design.

### Direct music, effects and title-audio cooking (2026-10-24)

The named music publisher now compiles original arrangements and their instrument
closures in memory and passes them directly to the WAV renderer. It no longer
reads the old battle audio catalogue, music packages, sample hashes or staged
instrument WAVs. Original reverb commands still select the main recording and
the two inherited-environment cases; equal recordings still share one song
owner. Music remains independent of fields and no loop samples are blended.

Title audio follows the same direct path. Menu cues compile from the resident
source bank into current editable program JSON and named instrument WAVs. The
production cook calls this publication directly. Removed the old title audio
writers, title-audio descriptors, menu-cue manifest reader and cooked-score
preview command. Native diagnostic preview commands still read original disc
data. The remaining cue bank type contains prepared playback data only.

Named field/battle effects now compile directly from their original banks too,
including battle banks whose samples live in a separate native archive. Shared
programs inherit the selected field/battle studio, and named instrument files
remain independent of music WAVs. The complete effect publication records
declarations with dependencies proven absent from the original banks in
`reports/audio-effects.json`; it does not invent silence, admit malformed
programs or create runtime mappings for unavailable cues. Explicitly requesting
one of those cues still fails. The report is not a runtime dependency.

Native music checks now start with an empty converter directory and compare
current WAV playback against original-disc compilation. The new menu check
compares program data, tuning, loop samples and rendered PCM through current
content capture. Verification passed:

- All 224 declared music setups across both source discs preserve their score
  boundaries, reverb selection, instrument closure and sample/loop PCM.
- Six representative songs (0, 3, 5, 78, 95 and 111), including both inherited
  environments, match 72,967,584 source-performance frames and unchanged repeat
  boundaries. This check completed in 273.24 seconds without staged inputs.
- All four directly published menu cues preserve program data and rendered PCM.
- All 2,434 playable native effects preserve their programs and 5,146 sample
  bindings across field/battle environments. The audit accounts for every
  declared effect as playable or proven unavailable, and verifies that explicitly
  requesting an unavailable cue fails. This final native batch also rechecked
  menu cues and shared inherited recordings; all three checks passed in 16.60
  seconds.
- The audio library passed 127 tests (three opt-in native tests remain ignored),
  and presentation audio passed 69 tests (13 native cases ignored). The final
  workspace/all-target check passed without warnings; formatting and diff checks
  passed.

These checks do not replace the still-pending complete production cook and
cross-domain acceptance work.

The named speech publisher now decodes selected native archive members directly
to `audio/speech/<archive>/<line>.wav`. Its source catalogue retains original
archive ownership and member order; missing or authored-empty members fail when
explicitly selected. No staged PCM, old voice metadata, hash-named filename or
invented fallback filename is needed to publish a line. Runtime facts come from
the ordinary WAV header. Playback-clock overrides remain in current selection
documents. The native decoder is shared with the remaining temporary cooker
path, without passing through a cooked descriptor.

This removes the title-audio bridge and the named music/effect/speech publishers'
old payload inputs. Named field, battle and world selection publication now also
uses native inputs directly. The production coordinator and its temporary old
publication paths still require conversion; the full cooking transition and
plan remain unfinished.

### Existing foundations

- Added `crates/content/src/library.rs`: explicit layout descriptor, validated
  numeric mappings, category-relative resolution, ordered mod layers and a
  preparation snapshot that retains the bytes it reads.
- Ordinary resource paths use whole-file override semantics. `mappings.json`
  merges by ID, including bank-local mapping files. Override diagnostics retain
  the logical path, optional ID, previous contributor and selected contributor.
- Resolver reads, JSON decoding and internally computed digests use the same
  captured bytes. A fresh snapshot observes edited resources. Stream selection
  returns an open file handle from the effective mod layer.
- Added a maintained field-name catalogue in `crates/import/src/naming/fields.json`.
  Its initial 546 entries were pinned from both original phase tables and the
  maintained location constants. Examples: `iselia-0330`, `iselia-0332`,
  `title-day` and `title-night`. Suffixes are original field IDs, so adding a room
  cannot renumber existing names. Unknown locations keep explicit fallbacks.
- Added importer naming selection with duplicate-ID and path-collision checks.
  Names are selected independently of discovery order and localized game text.
- Added split field documents and assembly into the existing in-memory field
  representation. Named publication separates field-owned resources, shared
  character models and field-specific animation bindings, and copies independently
  owned files without hardlinks.
- Added editable native motion JSON and decoding support. The named publisher
  writes those clips; field assembly derives their duration. Cloth attraction
  bindings (`secondary_pose_nodes`) stay explicit: native scenery deliberately
  disables them even when the motion contains animated tracks. Reconstructing
  those bindings from track counts caused a verified regression and was removed.
- Named visual publication now includes shared field effects, contact shadows,
  toon ramps, field-owned particles and overlay palettes. Shared effect atlases
  retain deliberate ownership within the effect collection.
- Added typed scene dependency capture through the effective snapshot, including
  nested effect and overlay artwork. `Files::from_snapshot` transfers captured
  bytes into the existing runtime store, preserves override diagnostics, and
  reuses resident buffers by hashes of the actual bytes. It does not read a
  persisted preload inventory or reopen captured files.
- Audio sample, package and voice caches now identify actual input bytes. Package
  reuse also checks the current sample bytes, so changing a sample alone cannot
  return stale decoded audio. Named field, world, title and battle audio use these
  caches with current snapshot bytes and no stored integrity gate. Old cooked
  readers are being deleted, including test-only entry points; see the current
  reader-removal status above.
- Added named field audio publication from original song, bank and archive
  identities. Known service cues use names such as `door` and `treasure-open`;
  unknown cues retain numeric fallbacks within named banks. Original archive
  member names and IDs supply stable speech filenames.
- Added editable audio program loading, typed mapping resolution and snapshot
  capture for every selected program, sample and spoken line. The standalone
  field-audio recorder now uses this path and requires `content.json`; it has no
  runtime fallback to the obsolete field-audio filenames. Gameplay's named field
  package also uses this prepared audio path.
- Added named movie publication by game resource identity. Known opening
  and introduction resources have semantic names; remaining resources retain
  source names. Repeated source declarations share a single movie owner.
  The no-disc-metadata decision removes the former disc-specific filenames.
  Large streams copy atomically with bounded memory and no hardlinks.
- Movie descriptors contain only layout version, stream path and track selection.
  The decoder no longer accepts a cook-time `MovieAsset` or its frame/dimension
  facts. It probes the selected file, keeps it open for both workers, and validates
  actual stream semantics. Opening and script-movie preparation use named mappings.
  Presentation now handles short movies and unknown lengths, and derives the final
  frame from decoded EOF. Offline capture planning reads current timestamps.
- Added named credits/game-over publication and snapshot preparation. Credits
  recording clocks come from the current WAV; game-over labels and font come from
  its editable document. Field preparation discovers credits from current field
  and skit scenarios and captures game-over as shared UI. Native-call argument
  analysis now lives in `symphonia-script` so import and runtime preparation use
  the same analysis. End-to-end route verification remains distinct from these
  publication and capture changes.
- Added named publication for field-owned unbound geometry, including physical
  mesh recipes and complete local texture/palette metadata. The physical scene
  and texture contracts now live in `resonance-content`; importer decoding keeps
  its existing algorithms and projects supported recipes as before. Named field
  preparation follows current nested references instead of retaining a hash-path
  descriptor without its mesh. Caller-dependent geometry retains its existing
  unsupported-binding diagnostic.

- Added world authoring roots under `world/`: continent, actor, marker and
  cinematic mappings; separate movement, collision, encounters, landmarks and
  guideposts; owned terrain/model/motion/image files; scenario/messages/audio;
  and separate cinematic camera/dialogue documents. The original source inventory
  and provenance hashes do not gate edited content. World preparation captures
  current references, shared UI/gameplay, skits and audio from one snapshot.
- World transitions, saved-world admission, test-save creation and the capture
  command now use named preparation. The old world loader and production save
  migration tooling have been deleted. Leaving the world for a field
  drops its retained world package so a later return prepares current files.
  World gameplay tables, collision, continent/cinematic IDs and native code now
  participate in derived save identity, excluding paths, display labels and
  artwork/media bytes. Optional-root discovery pins presence or absence without
  recursively enumerating terrain/model files.
- Added arena publication under `battle/arenas/`, with ID mappings, small
  `arena.json` entry points and owned layer/model/motion/image files. Native layer
  slots, scenery attachments, animation rates, palette indices, fog and recurring
  effects remain explicit. Unidentified arena names currently use deterministic
  `arena-NNN` fallbacks; mappings can rename them without changing arena IDs.
- Added named common, technique and arena effect banks. Programs, particle
  declarations and artwork have separate editable documents, and model references
  reach their owned rigs/layers/assets. No authoring document carries an expected
  digest or file inventory. Stage effect images and native effect-bank slot 9
  intentionally share the same named images, based on their original archive
  relationship. Shared-pose effect layers retain one motion owner; independent
  layer skeletons retain separate motion banks.
- Arena capture follows current references and derives clip duration from edited
  motion data. `game::battle::stage::prepare_named` admits the captured arena into
  the existing runtime rig/pose loader. The effect-program reader consumes named
  documents directly when given named prepared files. Missing optional effect
  models are isolated, optional image errors retain diagnostics, and cancellation
  remains fatal. Complete encounter preparation and production cooking are still
  pending; these APIs do not yet make a named field's battles playable.
- Added named party/costume publication and capture, reusing shared character
  body geometry while retaining battle motion banks, rigs, empty motion slots
  and optional wings. Character and costume mappings can both be overridden;
  entry points and nested references use the effective captured view.
- Added costume-owned victory documents and separate group definitions. Named
  runtime victory preparation selects explicit character/costume IDs and reads
  captured motion bytes. Runtime and test preparation no longer join victories
  through model hashes. The temporary staging translator still uses that source
  relation and must disappear with direct named cooking. No hash identity or file
  inventory is emitted in the named victory authoring format.
- Added named equipment publication/capture for weapons, shields and explicitly
  nonvisual choices. A pinned 179-entry item/prop naming catalogue provides
  readable owners. Equipment and effect models now share the typed model reader
  and publication helper, preserving separate motion owners for independent
  skeletons. Full encounter integration and battle
  audio remain pending alongside production cooking.
- Added named enemy publication/capture with separate rules, rigs, models and
  effects. It follows current shared monster previews and preserves carried
  components, secondary bodies, weapon variants and native effect model slots.
  Declared preview/battle geometry ownership is shared without content-hash joins;
  secondary effect geometry uses the same owner with separate effect motions.
  Original-data and edit/failure checks pass as recorded below. Full encounter
  preparation has not yet switched to these typed inputs.
- Added named stored-scene publication/capture with separate technique and skill
  mappings, pinned readable names, editable rules, shared effect/model/texture
  references, cut-ins and temporary weapons. Scene consumers read the named
  representation from captured bytes; no synthetic legacy descriptor is inserted.
- Added named event publication/capture, including script/messages, scoped actor
  and external-motion bindings, shared party/monster geometry and an explicit
  blink-cycle resource. Runtime event binding accepts that captured cycle. Event
  audio selection is preserved; complete battle audio preparation remains pending.
  Original-data publication and runtime binding checks pass, including sampled
  attachment poses. Full encounter activation and production cooking remain pending.
- Added named shared battle rules and UI publication/capture. The party's large
  normal-attack and martial-action tables have separate direct references;
  other rule tables retain their native order and gameplay constraints. Named
  schemas reject source hashes and inventories. Formation and Unison validation
  now checks gameplay without requiring a provenance string.
- Battle UI has separate named font and artwork ownership, with the existing
  optional-portrait diagnostics preserved. The formation, profile, defense,
  Unison, projectile, recoil and UI consumers read the captured named documents.
  Full encounter preparation still needs to capture these roots alongside its
  selected models, scenes and audio. Shared battle gameplay is not yet part of
  derived save compatibility.
- Added complete named battle-audio publication, mapping-derived catalogue
  discovery and selection, including native stream aliases and archive/member
  playback clocks. Source-bank group identity preserves deliberate aliases while
  giving field, party and enemy cues readable owners. Shared programs inherit
  field/battle environments instead of conflicting at the same `sound.json`.
  Empty speech classification is shared with the native converter and recorded
  separately from playable resources. Complete publication and original-data
  comparison now pass. This program-based music publication is an intermediate
  implementation: the requested rendered-WAV music switch supersedes its public
  score/sample music format.
- Added a named battle mixer preparation API using the same snapshot and caches
  as field audio. Its context-aware cache keeps reverb changes separate while
  retaining sample reuse. Production encounter preparation now calls it using
  the same snapshot as formation selection and CPU/art preparation. The old
  presentation audio fixture loader still needs deletion. Native encounter
  activation and host checks must use the current named preparation path.
- Added a central WAV publisher, strict `music.json` capture and bounded PCM
  playback over retained file handles. Repeated selections reuse the song's one
  publication. The player preserves introductions, exclusive loop bounds,
  resampling, independent cursors and pause position. Title publication/playback
  uses this contract, including existing startup fades and separate live menu
  cues. The existing mixer worker supplies output buffering; WAV pages are read
  there, never in the device callback. Storage, whole-catalogue coverage and
  runtime throughput still need measurement.
- Offline loop selection compares windows on both sides of native sequencer
  boundaries, including every dry/auxiliary bus and final studio output. It
  preserves multi-pass timing and never blends or rewrites a seam. When the dry
  passage recurs first, the importer replays its unchanged buses from bounded
  temporary storage through the continuing studio until the final PCM also
  recurs. Native checks now verify 72,967,584 frames for songs 0, 3, 5, 78, 95 and
  111 against independently generated source buses and studio output, including
  the repeated seam and inherited-environment variants (252.55 seconds).
  Track 95 retains three musical passes; song 5's verified dry passage spans
  eight. Its completed WAV contains 46,114,976 frames, so storage cost is a real
  remaining acceptance item. Searching and effect baking have separate bounds,
  with a one-hour recording ceiling. The earlier full-catalogue audit finished
  in 1,285.67 seconds: 109 songs plus title produced 5,726,897,144 WAV bytes
  (5.73 GB), including inherited-environment variants. Its old shared search/bake
  budget rejected songs 5, 43 and 90 after finding valid dry passages. Song 5 now
  passes with the separate budget. The current complete audio publication now
  passes for all 112 songs, including 43 and 90: 114 WAVs contain 1,383,488,142
  frames and occupy 5,533,957,584 bytes, excluding the separate title recording.
  Publication took 1,244.99 seconds; the complete check took 1,282.35 seconds.
  It verifies every song's native prefix and loop declaration, 2,434 effect
  programs, 2,573 sample bindings, 14,429 voice bindings and 501 field selections.
  Full PCM/seam comparisons remain the representative-track checks above.
  Both inherited-environment songs (0 and 111) have byte-identical field/battle
  WAVs and equal loop settings. Publication now discards an alternate only after
  that equality is established. The targeted check passes in 0.33 seconds and
  compares every frame of both shared recordings against both native environments.
  It removes two redundant 257,324-byte files from subsequent publications.
- Named field, world and battle audio now resolve `music.json` and publish through
  the central WAV renderer. Named mixer preparation captures the current WAVs;
  effects still use shared editable instrument programs. Recorded music is
  prepared on the existing mixer block clock and added after the live studio.
  Its queued block survives source replacement, field/battle handoffs and stops;
  fades and levels use the existing control quanta. Inherited variants follow
  the current music preset, including when that preset was selected outside a
  battle. The full battle worker now uses named capture and has no synthesized
  music fallback. Original-format audio loading remains in fidelity tests.
  All 67 regular field/battle audio checks pass, including three recorded
  transport tests for reverb bypass, fades, movie muting, jingle/resume behavior,
  atomic WAV replacements beyond a cached page, queued handoffs, inherited
  presets and pause position. Content's 74 checks and the workspace check pass
  after this switch; strict audio/content/import Clippy also passes.
- Production battle entry now carries its effective library snapshot through
  formation-pool selection, named encounter capture, action-selected audio and
  final preparation. The pool regression verifies that an edit between selection
  and worker admission cannot change the selected table, and a later snapshot
  sees that edit. The regression and workspace all-target check pass. A reusable
  original-data battle fixture is being published for full encounter comparisons.
  The expanded native CPU check passes: 18 encounters cover formations 1 and 2,
  all 44 party costumes across all nine characters, Defense System and Vidarr.
  Each matches 600 complete battle frames from the original loader (10,800 frames
  total, 34.28 seconds), including render models and effect banks. This check uses
  source audio declarations while WAV publication continues. Full named audio
  preparation, GPU activation and fatal routes remain pending.
- Added `docs/editable-content.md` with mapping/path rules, shared music ownership,
  WAV replacement and exact loop settings, field/model editing entry points and
  current limitations. It explicitly requires a correct unblended loop. The
  cooking guide links to it and identifies its legacy output until the production
  publisher switch is complete. Final CLI/mod and cooking instructions still need
  to follow the completed implementation.
- Authored-source identity now uses declaration types to normalize explicit asset
  path literals, scalar constant `Message` text and message-template text. Ordinary
  string keys, declaration names/types, template parameter lists and state
  initializers remain significant. This avoids treating arbitrary strings as
  filenames or captions. All 32 compiler checks, strict compiler Clippy and the
  named save-identity integration check pass (the native check took 0.37 seconds).
  Referenced asset gameplay semantics still belong in their domain projection.
- External `--scripts ROOT` projects now capture `fields.json` and non-standard
  modules once. Field compilation and identity derive from those same captured
  inputs; field/world preparation and title slot browsing include the project.
  Module names, bindings, task arguments and arrival/entry policy participate,
  while absolute project paths, JSON formatting, source comments and declared
  display text do not. The project identity regression and all five authored
  preparation checks pass, including edits between capture and compilation.
  The workspace all-target check and native slot-context identity comparison pass
  (the latter in 0.65 seconds). Full field/world save admission with an external
  project still needs validation against the complete named fixture.
- Added ordered `--mod DIRECTORY` selection. The startup library now flows through
  resident field/world workers, title slot browsing, new-game/scripted movies and
  existing battle/preview capture. Title-audio recording and world-test save
  creation also use the selected layers; the latter includes an external script
  project's identity. A new native check exercises a mapping rename and dialogue
  overrides across fresh preparation and restoration. All three CLI checks, the
  layer-order regression and the workspace all-target check pass. The layer check
  covers equal folder basenames, reverse priority and duplicate directories.
  Native field mapping/mod restoration passed in 14.03 seconds; project-aware
  field restoration passed in 18.86 seconds. World save creation, browsing and
  restoration with a mod and external project passed in 296.75 seconds. Full
  boot/movie/GPU mod integration still needs verification. The battle-worker check includes an edited WAV that must fail fresh
  preparation instead of reusing the base or an earlier recording; it passed
  in 9.66 seconds. Fatal host
  fixtures now prepare named fields and retain named title resources; a separate
  title-field mod fixture supplies their return path without recooking music.

These are foundations, not an accepted end-to-end migration. The named publisher
is not wired into production cooking. Command-line mod selection is connected,
with native field/world/battle-worker integration verified. Dynamic resource catalogues still point
into the old layout. Field-owned
unbound physical geometry now has named publication and nested capture. Fields
prepare shared UI and authored sources through named
capture; other game-wide dependencies still use the old preparation entry points.
Preload files and nested media hash gates remain in those consumers. No production
cooked library has been rewritten or replaced.

The partial consumer switch means the old production cooker output alone is no
longer sufficient for the migrated title, field, audio recorder and movie entry points. Complete
the publisher and runtime transition together before calling this a usable release;
do not introduce a permanent old-format fallback to hide the unfinished migration.

### Verification so far

- User clarification: retaining old cooked-format readers for tests is also
  rejected. The prior test-only isolation was an intermediate step, not an
  accepted solution. Delete these readers and move their meaningful behavior
  checks to current named preparation. Original-disc parsers belong only to
  cooking and remain necessary for importing the game.

- The no-backwards-compatibility decision removes the schema-5 repair command,
  public repair API, migration implementation/tests and importer fingerprint
  publisher. Save encode/decode/admission now accept only schema 6 and preparation
  requires a derived identity. Tests reject both older and future schemas even
  when the supplied identities match; rejected writes preserve the existing slot.
  The initial content/persistence check passed all 76 content and two slot tests.
  Original field-session loading, world migration startup and the old world
  reader have been deleted. World behavior checks now use named preparation.
  Content loaders no longer switch formats based on the presence of a library.
  Remaining old manifest, battle and audio fixture readers must be deleted,
  with their meaningful tests moved to the current layout; test-only retention
  is explicitly not accepted.
- The complete named battle fixture now passes 18 encounters with recorded audio:
  all 44 costumes across nine characters, formations 1/2, Defense System and
  Vidarr. All 10,800 complete simulation frames matched the original preparation
  in 38.03 seconds, with each encounter's selected songs, cues and voices prepared.
- The six fatal battle host checks exposed missing shared UI capture in named
  encounter preparation before scene construction. Capture now includes those
  dependencies, and HUD cut-ins use captured scene mappings. The next run
  reached scene construction and exposed an obsolete source-hash/inventory
  requirement in arena validation. Arena validation now checks runtime structure
  while named capture checks its dependencies. The next host run is pending;
  this is not a GPU fidelity claim.

- Recording-reader and renderer checks pass for retained replacements, bounded
  pages, fractional-rate playback, pause, native PCM, tempo changes, cancellation
  and duration limits. All three regular recorded-title checks pass, covering
  startup gains, looping, one-shot replacement completion, live cue isolation
  and current mod-selected media. A fractional-clock regression also passes: a
  two-beat synthetic score at 135 BPM requires three passes, rejects the adjacent
  boundary and retains every original PCM sample. The workspace check passes
  after the title recording switch.
  Content passes 74 tests and both named movie-stream checks pass after file
  capture was changed to retain one selected stream per snapshot.
  The complete audio crate now passes 129 tests (three original-data/diagnostic
  checks ignored); strict audio/content/import Clippy passes. Presentation
  Clippy reports existing style warnings in battle/oracle/water code; the new
  test-only title player has been boxed to remove its large-enum warning.
- The native recorded-title preparation test passes (271.37 seconds), including
  both backgrounds, 18 artwork textures, startup logos, live menu cues and 120
  script updates. The two-second startup comparison against live synthesis
  measures a maximum difference of 1,944 PCM units and RMS 346.59 because the
  fade now applies to baked reverb; menu cue PCM remains unchanged. This is a
  measured behavioral difference, not sample-identical title music.
- The complete audio publication/comparison passes (238.57 seconds): 2,546
  programs, 4,038 instrument sample bindings, 14,429 voice bindings, all 501 field
  audio selections and 16,625 named publications. Scores, programs, tables,
  inherited reverb, tuning, loop metadata, first-pass/loop PCM and voice bindings
  match the original. Empty speech members and unavailable cues remain accounted
  for in reports. These are synthesis-source checks, not rendered-WAV acceptance.
- The named battle mixer check passes, including shared samples, independent
  field/battle reverb, stream/archive aliases, warm-cache edits, retained bytes,
  cancellation and missing dependencies. All 12 regular named presentation
  checks pass; audio passes 123 tests (two original-data checks ignored), and
  content passes 72 tests. Strict audio/content/import Clippy passes after the
  reverb-ownership check. The workspace check also passes after the initial
  full-encounter capture API described below; that API still needs runtime tests.
- Both shared battle publication checks pass (0.73 seconds): 1,000 formations,
  736 pools, 11 party profiles, nine normal-attack banks, 114 martial members,
  19 Unison combinations, 26 projectile templates, 253 techniques, 58 UI image
  bindings and 70 named publications. All retained rule values and original UI
  payloads agree with the source cook. Current nested action edits, inherited mod
  roots, font/label edits, retained snapshots, invalid pool/actor/version/hash
  metadata, invalid Unison camera bindings, missing required resources, optional
  portrait policy and cancellation are covered.
- The shared-rule runtime comparison passes (10.97 seconds): 999 formations,
  1,614 declared enemy resources and 2,134 actor spawns, plus combined encounters,
  recoil and strategy controls, 260 resolved techniques and 58 UI image bindings.
  Formation 90 preserves its original error: enemy 191 has three statistics
  variants, but that formation requests variant index 3. An independent source
  table audit confirms it is the only unavailable variant reference. The initial
  sweep exposed that existing limitation; the final check verifies the matching
  legacy/named error and all other formations. This does not prove complete battle
  activation, audio preparation, save compatibility or HUD rendering.
- After the shared battle changes, the 72 content tests, 294 game tests and 12
  regular battle-UI tests pass (47 game and nine UI original-data cases remain
  ignored by those ordinary commands). The workspace check, strict content/import
  all-target Clippy, formatting and diff whitespace checks pass.
- All four choreography publication checks pass (33.05 seconds): 89 stored scenes,
  270 models, 63 clips, four temporary weapons, three cut-ins and 3,676 scene
  publications; plus both event scripts, 10 actor bindings, 421 primary clips and
  625 publications including shared party/Vidarr owners. Comparisons retain all
  original rules and asset/motion data and cover the complete 89-entry conversion
  catalogue. Tests also cover ID-scope isolation, mapping redirects, inherited mod
  resources, current rule/blink edits, retained snapshots, required-resource errors,
  inventory rejection, external-motion slot collisions and cancellation.
- The corresponding original-data runtime check passes (31.46 seconds): all 89
  stored-scene readers, 266 native effect models and both event scripts. Named
  event resource bindings, local/external clip durations, blink frames and 3,368
  sampled attachment matrices agree with legacy preparation. This establishes
  model admission and event resource preparation, not complete event playback,
  battle audio, encounter activation or GPU rendering.
- After the choreography changes, all 72 content tests and 294 game tests pass
  (47 original-data game cases remain ignored by that ordinary command). The
  workspace check, strict content/import all-target Clippy, formatting and diff
  whitespace checks pass. The final runtime check includes the expanded pose
  assertions and the latest scene-consumer changes.
- Both enemy checks pass, including the complete original-data comparison:
  251 enemies, three secondary bodies, 196 carried models, eight weapon variants,
  144 effect models, 9,327 preview clips and 24,005 named publications. All retained
  rules/model/effect properties, asset bytes and decoded motions match the staging
  cook. Preview mesh bone indices and component motions validate against their
  skeletons; shared secondary/attachment paths retain their declared ownership.
  The check also covers mapping-only renames, inherited base resources, current
  preview/clip edits, pinned snapshots, missing optional rigs, mandatory rule
  failures and cancellation. It finished in 269.92 seconds under concurrent
  validation. This verifies publication and capture, not full encounter activation
  or GPU rendering. No existing cooked tree was changed.
- After the enemy changes, all 12 regular named-publication tests pass (nine
  original-data cases remain ignored by that ordinary command), the 72 content
  tests pass, and the workspace check and strict content/import all-target Clippy
  pass. Formatting and diff whitespace checks also pass. The focused enemy test
  covers current motion timing, retained snapshots, contradictory empty motion
  slots, optional malformed clip diagnostics, strict failures and cancellation.
  An independent inventory audit of all 251 original enemies found no source
  inventory entry outside the preview, battle model and effect references that
  named capture follows. The original party regression also passes after extracting
  the shared body capture helper: all 44 costumes, 4,132 body clips, 220 victory
  performances and 54 groups, in 125.02 seconds under concurrent validation.
- The complete party publication comparison passes for 44 costumes, 4,132 body
  clips, 220 costume-owned victory performances and 54 group definitions. It
  publishes 7,309 files including field 332, verifying the shared field/battle
  character mesh and texture owners as well as unchanged typed properties,
  asset bytes and decoded motions. Character/costume mapping renames, inherited
  mod resources, edited motion timing, retained snapshots, missing nested rigs and
  cancellation are covered. The check finished in 86.06 seconds.
- Named runtime victory preparation passes for all 44 costumes and 220
  performances, matching original selected timelines and prepared motion bytes.
  Party and wing mesh bone indices and every selected motion validate against
  their rigs. The check finishes in 66.76 seconds. A regular fixture also verifies
  character/costume isolation despite equal legacy hash fields, fresh clip edits,
  retained previous bytes, duplicate owner rejection and cancellation. These are
  model/victory checks; they do not establish full named battle entry or rendering.
- Both equipment checks pass: all 179 original entries (143 weapons, 11 shields
  and 25 explicit nonvisual choices), 210 clips and 1,521 named publications.
  They compare retained definitions and asset/motion bytes, validate layer/mesh
  bone indices and clips, check unique portable item names, and exercise renamed
  mapping targets, inherited resources, retained snapshots, missing nested models
  and cancellation. The group finished in 8.31 seconds. The arena/shared-effect
  regression also passes after extracting their common model reader/publisher
  (three checks, 18.04 seconds).
- The three named-battle publication checks pass, including an original-data
  comparison of all 86 arenas, 415 layers, 30 arena clips and both shared effect
  banks (26 effect models), through 3,820 named publications. They compare all
  retained properties, resource bytes and decoded motions against the complete
  staging cook. The arena-only comparison passed first with 3,215 publications;
  the expanded run finished in 19.05 seconds. Tests cover mapping-only renames,
  inherited mod resources, changed animation paths/durations, pinned snapshots,
  invalid structural edits, rejected inventory fields, missing nested references,
  optional-art isolation and cancellation. The original fixture is selected by
  `RESONANCE_NAMED_STAGE`; no existing cooked tree is rewritten.
- The final named-arena runtime check passes for all 86 arenas and 2,075 layer
  poses (five clocks per layer), compared exactly against legacy prepared rigs.
  It also compares arena effect programs/declarations and prepares recurring
  emissions. The expanded run also admits both named shared effect banks and all
  26 effect models through the existing battle model loader, including indexed
  rig and shared-pose layer checks; it finished in 17.79 seconds. This is runtime
  geometry/motion equivalence, not a GPU screenshot comparison or proof of
  complete named encounter entry.
- After the arena/effect changes, the content suite passes (72 tests), the game
  library suite passes (294 tests, 47 original-data cases ignored), and the
  workspace check passes. Strict all-target Clippy passes for content/import.
  The broader game/dependency lint run still fails on existing warnings in
  battle unison, event companions/EX skills, and game battle code; this is not a
  clean whole-workspace lint result. Formatting and diff whitespace checks pass.
- After party/equipment integration, content and game library suites again pass
  (72 and 294 tests respectively, with 47 game original-data cases ignored), as
  does the workspace check.
  The named victory fixture also passes with malformed independent performance
  and group rows: healthy entries remain usable in tolerant mode, while duplicate
  owners and cancellation remain fatal. Diagnostics identify the owning document
  and row/selector.
- Both named-world publication checks pass after the final shared-body owner
  changes (75.70 seconds for the group under concurrent validation). They cover
  all 216 base tiles, 12 alternatives, 14 cinematics, 51 music/effect programs and
  nine voices, comparing original resource bytes, decoded animation values,
  synthesis programs and first-pass/loop PCM. The final fixture includes field
  332 and 7,332 named publications, verifying that field/world party bodies share
  their meshes/textures while world travel clips keep their own bank. The earlier
  world-only fixture had 4,754 publications. Scoped mapping overrides, inherited
  resources, renamed ownership, retained snapshots, collision/native-script
  identity changes, missing nested files and cancellation also pass.
- Both named-world runtime tests pass (635.75 seconds for the group while other
  integration checks ran). They cover save admission, restoration of all four
  mounts, menu saves, native Iselia entry/exit, fresh world preparation after the
  field visit, and all 14 cinematics including the 516/517 chain, completion
  boundaries and return to travel. This elapsed test time includes isolated
  publication and debug preparation; it is not a release startup benchmark.
  The native Rheaird test-save check also passes (363.39 seconds), including
  named creation/admission/restoration, early-party state, flight access and
  1,250 updates of the original skit/landmark behavior. The generated header uses
  the prepared world's identity rather than rereading mutable definitions.
  The headless rendering check passes after fixing a remaining `game/skits.json`
  lookup in world UI construction. The UI now receives the prepared typed skit
  catalogue. The capture `local/layout-migration/named-world-20261024.png` and its
  JSON sidecar show 340 instances and zero unprepared asset reads. It used the
  documented llvmpipe/WebGPU-limits environment and completed in 256.06 seconds
  including private publication/preparation. This establishes preparation and
  rendering, not an original-console pixel comparison. The final shared
  field/world party-body capture also passes (192.24 seconds) at
  `local/layout-migration/named-world-shared-body-20261024.png`. Its PNG bytes and
  observer metadata are identical to the first successful named capture, with
  340 instances and zero unprepared reads. All eight non-ignored named-publication
  unit tests pass after the shared body/clip publisher refactor.
- All 72 content tests and 304 non-ignored presentation tests pass after the world
  changes; 133 presentation integration tests were explicitly ignored in the
  broad run. The game suite passes 326 tests with 124 original-data cases ignored.
  Workspace compilation and strict all-target content/import Clippy pass. A missing
  production-only `BTreeSet` import was found by the workspace check and fixed;
  the test configuration alone had masked it.
- `original_unbound_geometry_has_named_owners_and_complete_capture` passes for
  both affected baseline fields, 125 and 499, capturing 2,331 and 4,522 files.
  Physical mesh bytes, skeleton names, draw recipes and resource IDs agree with
  the old output. Captured paths contain neither old `assets/` nor `geometry/`
  roots or preload inventories. The two synthetic geometry checks also pass for
  independent ownership, complete palette pages, an edited descriptor introducing
  new nested dependencies, base inheritance, retained snapshots, disabling the
  mod, missing dependencies and cancellation. These are publication/preparation
  checks; they do not add rendering support for caller-dependent geometry.
- All 35 non-ignored importer geometry checks pass after sharing the physical
  metadata types; 13 original-data cases remain ignored in that broad run. The
  named original-data check above ran separately. The workspace check and strict
  all-target Clippy for content/import pass after the physical scene changes.
  All ten TPL decoder checks and eight texture checks also pass; two texture
  original-data checks remain ignored. After the final screen/geometry changes,
  the content and presentation suites again pass 70 and 304 non-ignored tests.
- All five credits tests, including both original-data cases, pass. The full
  recording completion test runs a named opening session, consumes the complete
  soundtrack through the offline mixer, waits the final hold, resumes the native
  script, and checks cancellation. All 70 content and 304 non-ignored presentation
  tests also passed after credits/game-over integration, with 132 integration
  cases explicitly ignored in the broad presentation run.
- `game_over_load_and_quit_restore_the_title_and_release_the_field` passes in
  165.59 seconds against a privately published named opening/shared library.
  It edits the caption and both menu labels, constructs the actual scripted
  screen, checks field retirement, exercises both menu choices, and verifies
  restoration of the exact retained title resource snapshot. It uses no graphics
  or audio device; native screen pixel fidelity and the separate battle-fatal
  route remain unverified. The shared named fixture is also used by the credits
  lifecycle check.
- The field-332 restoration test passed again after shared-screen discovery:
  7,488 captured files, 475 skits and 2,768 media clocks in 215.96 seconds. It still
  verifies save admission/restoration, one-file dialogue overrides, stale-identity
  rejection, invalid service edits and cancellation. This run predates the
  unrelated unbound-geometry publication change; field 332 has no unbound geometry.
- The credits/game-over synthetic checks pass for edited text and pictures,
  replacement WAV rates/lengths and actual playback duration, inherited base
  resources, retained snapshots, disabling a mod, branch/dynamic screen discovery,
  malformed media, cancellation and optional-background diagnostics.
- `original_named_screens_preserve_credits_and_game_over_content` passes against
  the completed staging library. It preserves 2,212 credits operations, five
  pictures and 11,712,311 recording frames, comparing original bytes, clocks and
  text layout as well as editable game-over labels. It checks screen discovery
  from the actual ending script; it does not exercise the full ending field,
  its movie transition or GPU screen rendering.
- All seven importer `field_resources::tests` pass after moving native-call
  argument analysis into the script crate, including original party declarations
  on both discs and 501 distinct field archives. The workspace check also passes
  after the credits/game-over consumer changes.
- The complete old-layout staging cook at
  `local/layout-migration/staging-20261023` finished successfully: 69,505 conversion
  units, 1,745 duplicates, 22 explicitly deferred battle resources, 63 exclusions
  and zero failures across 3,388 source files. The first attempt exceeded the
  explicit 2 GiB scheduling estimate; the completed retry used four workers and
  4 GiB. This is a current-schema comparison baseline, not the final named output.
- `original_named_field_prepares_and_restores_without_inventory` passed against
  that baseline in 203.99 seconds. It published a private named field 332 and shared
  roots, captured 7,486 files, restored a checkpoint and exercised pending save
  loading, 475 skits, 2,768 media clocks, a one-file dialogue mod, changed gameplay
  identity, stale-fingerprint rejection, invalid service-source edits and
  cancellation. Early failed attempts used an unfinished baseline or omitted the
  source audio naming catalogue from the fixture; both prerequisites are now
  explicit. This establishes the tested field route, not full New Game or world
  restoration. Use an absolute `RESONANCE_NAMED_STAGE` path with Cargo tests.
- Shared model publication now uses pinned names for 251 monsters and 288 figurines.
  Menu records reference `meshes/<domain>/<name>/model.json`; geometry, artwork and
  motion files belong to each model. The focused publication test verifies stable
  names despite caption edits, independent ownership and current motion timing.
  All four presentation `model_preview::` tests pass, including changed model
  settings, mod asset inheritance, retained snapshots, cancellation and
  invalid motion diagnostics. The naming suite passes three non-original-data
  tests. This does not establish GPU cache invalidation or every model's render
  equivalence.
- The content suite passes 70 tests, and the workspace check passed after the
  named model-preview changes. `original_legacy_identity_preserves_the_schema5_projection`
  also passed: the extracted projection reproduces the baseline schema-5 identity
  across 501 field scripts. This validates preservation of the old algorithm;
  this is historical fidelity evidence only. A schema-5 to schema-6 save
  migration bridge is explicitly excluded from the design.
- Title/startup publication and loading now use the named entry points described
  above. The audio playback suite passes all six tests, including native PCM
  equivalence for a synthetic score/cue bank, valid score edits without updated
  hashes, one-file instrument overrides, retained playback, disabling the mod,
  malformed/missing cues under both diagnostic policies, and cancellation.
- `original_named_title_prepares_both_backgrounds_without_inventories` passes
  against the completed original-data baseline. It publishes both title fields,
  all 18 title images, four startup logos, shared UI, the title score and four menu
  cues. It compares original image bytes, advances both scripts for 120 updates,
  verifies a title-definition mod inherits the base resources, and compares two
  seconds of mixed title music/menu-cue PCM exactly with the original loader.
  Its optional GPU check also passed: the production startup route rendered tick
  120 with zero unprepared reads and no content diagnostics. Evidence:
  [image](local/layout-migration/named-title-20261023.png) and
  [capture report](local/layout-migration/named-title-20261023.json).
  This is a rendered startup check, not a pixel comparison against a Dolphin
  reference or a complete boot/movie/New Game/title-return sequence.
- The native title check's first GPU attempt failed during device creation using
  Mesa's default maximum-feature request. The repository's documented configuration
  (`WGPU_SETTINGS_PRIO=webgpu` and
  `VK_DRIVER_FILES=/run/opengl-driver/share/vulkan/icd.d/lvp_icd.aarch64.json`)
  passed without renderer source changes. Set `RESONANCE_NAMED_TITLE_CAPTURE` to
  an output PNG to run this optional check alongside the native preparation test.
- After title integration, all 70 content tests and 301 non-ignored presentation
  tests pass; 131 original-data/render cases remain explicitly ignored in that
  suite. `cargo check --workspace --offline`, formatting and diff whitespace
  checks pass. The native title check above ran separately with original inputs.
- `named_save_identity_tracks_gameplay_and_preserves_presentation_edits` passes
  against the fresh cook's current session/menu definitions and native field/skit
  scripts. Its minimal named fixture deliberately has no audio or scenery: save
  identity and slot browsing do not require those assets. It verifies comments,
  JSON formatting, menu-label changes, field/script path moves, pinned snapshots,
  changed inventory rules, field-ID removal, native skit edits, authored source
  edits, gameplay mod overrides, disabling a mod, cancellation, and ignoring an
  old authoring fingerprint. This is compatibility evidence for those inputs,
  not proof of playable field restoration. Old saves are intentionally unsupported.
- The current content suite passes all 70 tests after extracting the existing
  gameplay projection for reuse by named save identity. The new mutation test
  checks that editing, removing, clearing or extending prepared inputs discards
  their derived identity while leaving retained snapshots unchanged. The focused compiler
  identity check passes for comments, whitespace and equivalent numeric literal
  spelling, and rejects changed literal values and invalid syntax. These checks
  do not yet establish complete current-format gameplay identity coverage.
- Shared preparation work: all 68 `resonance-content` tests pass, including new
  coverage for mod-added script modules, pinned namespace discovery, case
  collisions, directory cycles, skit mapping overrides, current WAV clocks and
  invalid/cancelled skit preparation. The first run exposed a directory lookup
  incorrectly using the file-only resolver; that was fixed before the passing run.
- All three `field_audio::named_tests` pass after skit integration. The new test
  exercises audible skit PCM in the same field voice bank, changed source rates
  and frame counts, silent timing tracks, cache reuse and cancellation without
  swallowing it as an optional audio error.
- `cargo check --workspace --offline` passes after named field capture, typed
  skit UI consumption, field-event checker changes and initial/saved field loading
  through prepared packages. Both presentation asset-reader tests pass; the
  activated reader still serves captured bytes and rejects unprepared disk reads.
  Formatting and diff
  whitespace checks pass. Strict all-target Clippy reached existing warnings in
  battle/event dependencies and, with `--no-deps`, unrelated battle/rendering
  files in presentation; it is not a passing whole-workspace lint result.
- `cargo test -p resonance-content --offline`: all 63 checks passed, including
  9 resolver/snapshot checks covering mapping and file overrides, base
  inheritance, scoped IDs, captured bytes, fresh reads after editing, invalid
  bindings, missing targets, selected streams, layout rejection, containment,
  runtime buffer reuse and invalidation after edits.
- Naming checks: all 3 passed, including complete declared-field coverage against
  both original executables, stable selection order and unique portable names.
- Motion codec checks: both passed, including preservation of native float bits
  through editable JSON, applying edits, and rejecting invalid clip definitions.
- Latest named-publication run:
  `cargo test -p resonance-import named:: --offline -- --include-ignored --nocapture`.
  All 7 checks passed, including original fields 330, 332, 340 and 372 through
  5,844 named publications and 52 distinct audio programs. Comparison follows
  rewritten JSON references and compares decoded motion content, every field
  property, synthesis programs, scores, tables, first-pass/loop PCM and speech
  bindings. This proves the tested representations agree; it does not prove
  complete library coverage or gameplay entry into the new library.
  The focused checks also verify overlay palette capture, a mod introducing a new
  nested effect texture, and a missing mod dependency reporting its descriptor.
  The expanded audio audit validates all 501 scripted-field selections, 415
  distinct programs and 8,228 named publications. Its expected scope is derived
  independently from original archives containing VM programs. The 546 named
  phase declarations also include native scenes and absent resources; treating
  every declaration as a scripted audio package was an incorrect initial test
  assumption. This does not establish coverage of unused programs, additional
  voice archives or native battle/world/title audio.
- `cargo test -p resonance-audio --offline`: 122 passed, 2 original-data cases
  left ignored. The broader run exposed an unrelated stale cue test expecting
  the entire 64-voice pool; its assertion now uses the allocator's existing
  22-voice effects budget. No allocator or mixer behavior changed.
- `cargo test -p resonance-presentation field_audio:: --lib --offline`:
  61 passed, 10 original-data integration cases left ignored. Named audio tests
  cover WAV rate/length edits, scoped archive IDs, mapping-entry mod overrides,
  disabling mods, cache invalidation, cancellation, and audible output from the
  public field-audio recorder. Original stored-hash baseline fixtures use the
  test-only disk loader where needed.
- `cargo clippy -p resonance-content -p resonance-audio --lib --tests --offline -- -D warnings`:
  passed after the field, snapshot, motion and audio schema changes.
- `cargo check --workspace --offline`: passed, including all three applications.
- `cargo test -p resonance-media --offline`: 11 checks passed, 3 original-data
  checks initially ignored. Includes replacing movies with different dimensions,
  sample rates and lengths, mapping/file mod overrides, inherited files, aliases,
  disabling a mod, exact RGB/PCM output, and replacing a pathname between stream
  admission and worker startup. Existing queue-backpressure and cancellation
  checks still pass. A mismatched movie with unspecified header lengths is
  rejected using its actual decoded audio and video durations.
- `cargo test -p resonance-media --test cooked_movie --offline -- --include-ignored --nocapture`:
  all 3 passed. The original opening and story movie retain their complete frame
  counts and exact PCM relative to the source H4M audio; cancellation also passes.
- Named movie publication checks passed, including the expanded physical-source
  coverage check: 13 catalogue IDs and 25 named files, including the separate
  second-disc opening. Original stream bytes, format facts and selected tracks
  agree with the staging library. Independently owned files remain independent
  after edits; conflicting publication fails without replacing the valid file.
- `cargo test -p resonance-presentation movie:: --lib --offline`: 2 passed,
  5 integration cases ignored. The named opening fixture needs no old metadata,
  hash or length declaration; short-stream preparation and cancellation pass.
- `cargo clippy -p resonance-media -p resonance-import --all-targets --offline -- -D warnings`:
  passed after the physical-source coverage addition. The workspace check and
  focused presentation movie checks also passed after the final consumer changes.

### Lookup audit and next work

| Domain | Existing identity and required preservation |
| --- | --- |
| Fields | Original `u32` phase ID; replace filename construction and directory scans with mappings. |
| Movies | Original movie catalogue ID; track choice remains playback metadata. Field movie IDs are currently recovered from filenames and must become explicit. |
| Models | Standalone IDs, packed group/member IDs and reserved field-local ranges; preserve each lookup scope and intentional shared ownership. |
| Animations | Clip/member identity belongs to its model or motion bank and skeleton binding. |
| Speech | Physical archive membership supplies voice IDs; selected archive and playback metadata must remain explicit. |
| Music and effects | Song/cue IDs select packages in an audio environment; instrument, bank and reverb relationships must survive the layout change. |

Field preparation now connects named scene capture with shared gameplay/UI roots,
skits, audio and source compilation. Initial, saved-field and dungeon entry points
now consume the resulting package directly. Current source ownership evidence:
music is keyed by song ID within an executable's
audio environment; effects are keyed by `(source bank, cue ID)` before field
composition; field speech uses the upper 16 ID bits to select an archive and the
lower 16 bits for its member. Preserve the field's playback-rate override apart
from a WAV's source rate. Do not infer these ownership relationships from equal
package hashes during named publication.

`FieldPackage::prepare_with_diagnostics` now opens the named library and its
`capture` method follows current field, shared UI, skit, audio and authored-script
dependencies before transferring bytes into resident `Files`. It carries explicit
movie IDs, checks their named descriptors/stream selection, and leaves payloads
streamable. Field transition tasks capture again on revisits instead of refreshing
only scripts over old field bytes. Skit artwork consumes the prepared catalogue.

Named shared publication now emits ordinary session/menu/text documents, semantic
font/UI owners, per-skit scenarios/messages, portrait images, media mappings and
source modules. These are authoring roots and references, not renamed preload
entries. Source hashes are no longer serialized or required in font, dialogue-art
or session-data authoring documents.

Field discovery now enumerates IDs from `fields/mappings.json`, and the field-event
checker uses named preparation and reports named field/audio entry points.
`loading::Pending` and `Session::load` now pass a prepared named field package;
session construction uses its captured field catalogue. Slot browsing computes
identity and session definitions from the current named snapshot. Explicit legacy
field construction and world migration startup have been deleted. Tests of
field restoration and world behavior now use named preparation. Production save migration tooling has been removed.
World-save loading now uses the named world package and its derived identity;
native admission/restoration, field travel and cinematic checks pass. The first transition to field 340 is now prepared by
the pending field task before activation, instead of preloading its old inventory
alongside field 5. Full New Game testing must verify readiness and measure that
change in startup/transition cost. Existing graphics caches also retain
artwork by field ID and asset path: those need invalidation from current inputs
before in-process visual reload can be claimed. Credits/game-over now have named
publication, captured screen data and runtime consumers. Full ending/movie and
battle-fatal integration still require verification; the synthetic screen checks
and original-data comparisons above do not establish those routes. Dynamic model
catalogues and complete encounter preparation remain to be migrated. Shared
battle rules/UI now have named publishers and readers, with the integration and
save-compatibility work described below still pending. World shared roots now
have named publication and preparation with verified save/travel/cinematic
routes, test-save creation and headless rendering. Field-owned
unbound geometry now has named publication and nested capture, with the existing
caller-texture rendering limitation retained.
Title/boot artwork and title audio now have named publication and captured runtime
loading. The title menu reads captured UI; returning to the title restores its
retained resource snapshot. Full boot/movie/New Game and title-return integration
still needs a complete named library, and direct importer title-audio diagnostic
commands still use their explicit old-layout staging path.
Monster/figurine previews now resolve named model documents and capture current
descriptors, assets and source modules through the selected library. Descriptor
capture at menu admission is separate from payload preparation when opening a
preview. Existing GPU caches still need the reload treatment described above.
Generated hash-named node modules now have maintained source/member names, as
recorded in the generic physical publication batch. Their internal skeleton
signatures are importer lookup details, and no old import aliases are retained.
Preserve complete declared dynamic resource preparation and readiness; copying
old preload entries into a new wrapper would retain the editing problem.

The field-reference audit found `resource_catalogue` only in baseline fields 544
and 545. The former document contained source availability and hash archive paths;
the scoped dynamic-resource batch above replaces that representation with named
physical owners and current dependency capture. Generic runtime model bindings
remain incomplete, with explicit availability/support status. Do not remove those declarations merely because the
current test-field runtime cannot consume all of them.
The current baseline declares 53 standalone slots (34 cooked and 19 absent
sources) and 14 group slots (13 cooked and one empty). Preserve the empty group
and native packed `(group, member)` scope; the executable uses `group - 1` to
select its group table. Initial names can come from the source catalogue rather
than current archive hashes. A row marked cooked is conversion availability,
not proof that the runtime has a complete dynamic model binding.

World publication and its ordinary transition, checkpoint and capture entry
points now use the named contract. The inspected staging world document is
25,638,938 bytes and combines
216 base terrain tiles, 12 alternates, actors, skies, markers, 14 cinematics,
movement/collision/encounter/landmark tables and a 6,719-file inventory. Its audio
selects four songs, 47 effects and nine voices. Those editing domains now have owned
documents and derive dependencies from current references, retaining both
continents and every story-dependent alternative. `game::overworld::Prepared`,
presentation `WorldPending`, world-save admission and the world capture command
have all been changed together. World gameplay and native code participate in
save compatibility. Native save/travel, cinematic, test-save and headless
rendering checks all pass. Full game-wide save compatibility still needs the
remaining gameplay domains. There is no legacy migration bridge.

A named save-identity path now captures the shared gameplay projection,
mapped field and skit bytecode, and normalized authored source from one snapshot.
It is retained as derived preparation state rather than an authoring file. This
expands the old fingerprint's coverage to field services and skits. A field
transition rejects changed gameplay before carrying live state into its destination.
The tested field-332 and world save-admission/restoration routes now pass. Full
New Game and battle restore routes still need verification. Explicit asset-path
and scalar/template message declarations now have type-aware source normalization;
ordinary strings and state initializers remain conservative inputs. Separately
supplied `--scripts ROOT` projects now share captured sources between compilation
and derived identity; native field/world admission and restoration checks pass.
Old save identities are intentionally unsupported. The production importer still
publishes the old staging layout; switch it to named publication. Its obsolete
stored identity publisher has been removed. Runtime admission now requires a
derived current-schema identity without consulting an authoring fingerprint file.

The named audio publisher now covers the complete baseline battle catalogue,
including songs and cues unused by fields, ordinary voice archives and separate
empty/unavailable-member reports. World and title selections also have verified
named program publication/preparation. The central publisher and named runtime
consumers now use WAV music with bounded streaming and explicit inherited-reverb
variants. Selected original recordings have complete PCM and loop-seam comparisons
without blending; complete catalogue publication, native-prefix comparisons and
the storage audit now pass as recorded above. Recording replacements for suitable
effects remain a separate deliverable.

The encounter preparation boundary now captures named rules, transferable
equipment, party/costume models, every declared enemy resource and reserve,
refreshed previews, arenas, victory, UI, scenes and events from one snapshot.
Immutable runtime views share captured bytes while the same snapshot discovers
action-selected audio. Production battle entry carries this snapshot from pool
selection through the worker and WAV preparation. The old `Inputs::load`, model
inventory extension, arena and victory loaders have been deleted. Named events
require their captured blink cycle and do not recover embedded fallback data.
Native fatal-battle host tests passed all six loading/completion/cancellation
cases after removing obsolete arena provenance validation; complete GPU routes
remain separate work.

Battle save compatibility still needs current rules and contact/motion semantics:
a rig or animation that changes hit positions is gameplay data even though it is
stored beside visual resources. Full native conversion coverage must also be
wired into direct named cooking. Do not synthesize old JSON, inventories or hashes
as a bridge between these boundaries.
The 87 native-technique scenes, two skill-catalogue scenes (51 and 228), and two
event scripts (2 and 3) now have named publication/capture and runtime readers.
The complete staging conversion catalogue contains those 89 stored scenes and no
unreferenced physical skill packages. Preserve that coverage when wiring the
production cooker; do not infer general coverage merely from runtime-used scene
IDs. Full encounter capture must include every eligible combination and skill as
well as both event scripts and their audio. The named event path no longer needs
`data/embedded/field-effects.json`; its explicit captured blink cycle is required.

The fixed shared battle roots now have named publication, capture and runtime
readers: formations, party profiles, Defense System rules, technique definitions,
Unison recipes, projectiles, recoil and battle UI. Source/recipe hashes are absent
from these authoring schemas; formation and Unison structural checks no longer
depend on provenance strings. Complete encounter activation must capture these
roots from its effective snapshot. Formation pools are selected before the
preparation worker starts, so that entry boundary also needs current named
formation bytes. Add the gameplay portions of these roots to derived save
compatibility, excluding artwork, text, voice choices and other presentation-only
settings; the new readers alone do not establish save compatibility.

Battle audio has a complete named catalogue and runtime selection of music,
cues, streams and event voices. Its old cooked reader and inventory-based
preparation have been deleted; current selection follows mappings and snapshot
bytes without stored integrity requirements.
The battle-audio audit identifies an additional binding rule: streamed battle
voice IDs below `0x8000` are member aliases for the voice-directory entry whose
native resource handle is `0x78`. Its `logical_base` is a separate field in that
directory; do not compute it from the handle or treat these stream IDs as field
speech archive zero. The complete old catalogue publishes both the stream alias
and the archive-based voice ID for those same physical lines. Retain that
intentional shared ownership in named mappings. `AudioNames` now covers the
complete native bank catalogue, including battle party/enemy banks. Source group
identity governs aliases. The original-data check verifies shared program content
and inherited field/battle environments. That remains useful for effect programs
and as a baseline for the requested offline music renderer.
The complete baseline battle catalogue contains 112 songs, 2,434 playable cues
and 14,429 voice bindings. Its coverage report accounts for 2,441 authored cues,
including seven unavailable source dependencies, and no missing voice archives.
Comparison against all field-audio selections finds
86 shared song IDs and 329 shared cue IDs. All 329 cue packages differ only in
their reverb parameters; song ID 0 has that same difference, while the other
85 shared songs agree completely. Preserve the original environment inheritance
and explicit song reverb choices when separating shared program ownership from
playback context. Simply publishing both prepared packages at the same named
`sound.json` would either conflict or change the audio. The full source catalogue
also records unavailable sounds, missing archives and empty voice members;
retain that coverage accounting separately from playable authoring resources.

A split descriptor that assembles in a test is not yet a playable, editable field.
The implementation sequence and all acceptance criteria above remain in scope.
