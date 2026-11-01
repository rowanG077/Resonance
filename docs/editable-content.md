# Editing named content

The named layout keeps related files together and uses `mappings.json` only where
the game requests an original numeric ID. This guide describes the implemented
authoring formats. Import diagnostics stay outside the installed library.

The cooker also includes a short `README.md` in each library, maintained as
[the standalone library guide](cooked-library.md), so the first editing steps
are available beside the resources.

Backwards compatibility is deliberately unsupported. Use the current cooked
layout and save schema. An obsolete library needs a fresh cook; an unsupported save-format version requires a new game. There are no compatibility
fallbacks, path aliases, save migration tools or test-only old cooked-format
readers in the target design. Tests use the same named layout as the application.

## Finding a resource

A library starts with `content.json`: `{"version":30,"game":"tales-of-symphonia"}`.
It identifies the cooked format and semantic game, with no source platform,
product code, revision, file inventory or checksums. A future Steam importer
targets the same layout and resource contracts; actual gameplay determines save
compatibility.

```text
content.json
fields/
  mappings.json
  iselia-1/
    field.json
    scene.json
    collision.glb
    actors.json
    audio.json
    messages.json
    scripts/scenario.ssb
audio/
  mappings.json
  music/
    mappings.json
    old-familiar-scent.wav
  effects/
  speech/
    mappings.json
movies/
  mappings.json
meshes/
  mappings.json
  characters/
  monsters/
  equipment/
animations/
  mappings.json
  lloyd-e221/
    bindings.json
    lloyd.glb
battle/
  formations.json
  arenas/
  party/
  enemies/
ui/
fonts/
scripts/
```

The names in this example are illustrative. Use your library's mappings to find
the actual resource. A mapping such as `fields/mappings.json` contains entries
like:

```json
{
  "1": "iselia-1"
}
```

Mapping targets are relative to the directory containing `mappings.json`.
Here field ID 1 resolves to `fields/iselia-1/field.json`. The domain determines
its entry filename. Direct file references inside documents use paths from the
library root, such as `audio/music/old-familiar-scent.wav`.

IDs belong to their gameplay scope. A speech ID and a model ID with the same
number are unrelated. Renaming a directory
does not change its machine ID, but its mapping and direct references must follow
the new path. To replace a file at its existing path, leave the mappings alone.

Equipment mappings use a named item directory, or `null` when an item has no
visible attachment. For example, `"135":"wooden-blade"` selects
`meshes/equipment/wooden-blade/equipment.json`. That document declares its `kind`
(`weapon` or `shield`) and contains the part models directly; GLB and KTX2 files
sit beside it. A missing mapping is an error. Numeric part and layer keys carry
attachment/rendering bindings, so changing an item's name does not change them.

## Other original resources

Content with a known game identity uses the categories above. Dynamic resource
IDs use `resources/mappings.json` and the named payloads it references. The library
contains no parallel copy of the original archive tree. Native model headers,
skeleton dumps, archive directories, camera storage and other source diagnostics
stay under `reports/decoded/` outside the library. Decoding a source file does not
publish another set of meshes, textures or animations.

Speech mappings contain WAV paths or named silence clocks (frames, rate and channels).
Silent recordings require no WAV file. Empty native speech slots and original
music arrangements without an instrument setup appear in the import diagnostics
outside the library; they are not playable recordings. Native bank-table reports
also stay there. The cooked library contains no disc metadata or import reports.

Shared script node constants use catalogue names or `resource_<source_and_member>`
module names. The maintained lookup tables stay in the importer; modders edit
the ordinary `.sym` constants used by their scripts.

## Runtime-selected resources

Fields with dynamic script selectors declare `resources/mappings.json`. Each
key is the full runtime resource ID: standalone IDs occupy the low 16 bits;
group/member IDs use `(group << 16) | member`. The group number needs no separate
mapping or directory lookup.

```json
{
  "17": null,
  "18": {"geometry": "meshes/props/statue/scene.json"},
  "19": {"alias": 18},
  "20": {"animation": "animations/gesture.glb"},
  "65536": {"parts": {
    "body": {"geometry": "meshes/props/sign/body/scene.json"},
    "floor-collision": {"collision": "meshes/props/sign/collision.glb"}
  }}
}
```

References inside these typed entries are relative to the library root. A `null`
entry is empty; `"unavailable"` records absent source content, and `"unsupported"`
records an unsupported kind. Ordinary entries select `geometry`, `animation`,
`textures`, `collision`, or a `script` with messages. A model's nonempty `parts`
stay together in the same entry. Aliases select one other ID directly; alias
chains are rejected. A mod can replace an individual entry, including replacing
it with `null`. Editing an alias's owner changes every ID selecting that owner.

Animation entries point directly to a GLB. Standard glTF skins and animation
channels carry the current joints and curve bindings, so editing a clip requires
no sidecar JSON update. A bank with several clips uses named `parts` with direct
`animation` references. Empty parts need no file or entry.

Physical conversion does not by itself provide a playable actor: generic dynamic
model, palette and animation bindings remain incomplete. Supported static actors
retain their field/model definitions. Normal preparation captures only this
catalogue to explain unavailable bindings; it does not open unused mesh and
animation payloads. The importer explicitly checks the payloads at publication.
There are no per-resource wrapper files, empty directories, checksums or
compatibility readers.

## Replacing music

Music belongs under `audio/music/`, even when a field is its best-known use.
Fields, the world and battles select shared song IDs. Replacing one recording
therefore changes every use that selects that recording.

Songs use one `audio/music/mappings.json`, shared by fields, the world and battles.
Original disc locations do not affect their names or runtime selection.

Mappings point directly to named WAVs, for example:

```json
{"7": "old-familiar-scent.wav"}
```

Loop points are stored inside the WAV's standard `smpl` sampler chunk. Use an
audio editor that preserves sampler loops when exporting. A loop is one endless
forward interval with whole sample-frame boundaries. No per-song JSON or folder
is needed. An exported WAV without a sampler loop plays once. The sampler
layout and inclusive end follow the [standard WAV loop metadata](https://github.com/microsoft/DirectXTK/blob/main/Audio/WAVFileReader.cpp).

Use mono or stereo 16-bit integer PCM WAV, at 8–192 kHz, with a positive length
of at most one hour. The player reads rate, channel count and length from the
current file. You do not update a checksum, byte count, duration or dependency
inventory after exporting a replacement.

Loop positions count sample frames, starting at zero. A stereo frame contains
both channels. For example, set the start to 48,000 and the end to 2,879,999
to play the introduction once, then repeat that interval. At 48 kHz those boundaries are 1 and 60 seconds.
The WAV sampler stores the last included frame: 2,879,999 in this example.
Remove the sampler loop for a one-shot. If an edit changes the sample
rate, introduction or repeating section, update the frame positions accordingly.

A correct loop wraps continuously without a blend. Check the musical timing,
notes crossing the boundary and the reverb tail. Matching the first and last
sample values alone is insufficient. Resonance does not add a crossfade to hide
an incorrect cut. Original tracks can contain several musical passes in one WAV
repeat when their timing or effect state requires it.

The recording includes its intended music reverb. Playback does not apply that
reverb a second time. One recording serves every use. To select different music
in a particular context, select another named recording through that context's
music mapping. There is no special battle variant inside a song.

Nine original placeholder IDs share `placeholder.wav`. The actual ending music
is `credits.wav`; the old Sound Test label on a placeholder did not identify the
recording played during credits. The title and Sound Test song 1 share
`tales-of-symphonia.wav`.

Sound banks live under `audio/effects/<bank>/`, with their instrument WAVs under
`audio/instruments/<bank>/`. Fields and battles use the same effects
mapping. `battle/audio.json` holds the shared battle playback settings.

Effect mappings point directly to a WAV, a compact JSON recipe, or a shared
silence definition (`kind: silence`, `version`, `frames`, `sample_rate`). Silent
instruments use an inline `silence` clock instead of a `path` in their recipe's
sample entry; pitch and loop settings remain beside it. This preserves timing
without empty WAV files. Replace a WAV
at the same path, or change a mapping to select your own WAV. There is no required
per-effect directory or descriptor. Recipes retain random timing, pitch changes,
loops, release envelopes and other live behavior. Their sample references use
`audio/instruments/`; shared synthesis tables occur once in `audio/synthesis.json`.
Only referenced waveforms belong in `audio/instruments/<bank>/`; source-only
program dumps, unused instruments and source sample-ID maps remain outside the
library in importer diagnostics.

For a recorded effect needing loop or routing settings, use a small definition:

```json
{
  "kind": "wave",
  "version": 1,
  "audio": "audio/effects/door-close.wav"
}
```

Map the cue to that JSON file. The same definition works for field, battle and
menu effects. Mono and stereo WAVs can use 8/16/24/32-bit integer PCM or
32-bit float audio, at 8000–192000 Hz. Rate, channel count and length come from the
current WAV; no stored hash or size needs updating. Already prepared playback
keeps its captured bytes until the resource is prepared again.

Omitting `loop` plays the recording once. An optional
`"loop": {"start_frame": 12000, "end_frame": 48000}` plays the opening once and
then repeats that exact source-frame interval; the end is exclusive. Author a
correct loop. Playback adds no crossfade or seam correction.

Optional `gain` defaults to `1`; `sends` defaults to `[0, 0]` and controls the two
shared reverb sends. Leave `reverbs` absent to inherit the selected field, battle
or menu studio. `release_ms` defaults to `5` and controls the fade when a cue is
stopped; it does not alter its loop. Cue volume, pan, mono output, slot replacement
and effect priorities still apply. A recording replaces the whole audible cue;
keep synthesized programs for effects whose evolving instrument behavior you
want to edit.

Speech uses named WAV files and `audio/speech/mappings.json`. Silence is an explicit
clock in that mapping, with a readable name and no audio file. Audible skit lines live in
`audio/speech/skits/<displayed-title>/<speaker>-NNN.wav`. The embedded overworld event
uses `audio/speech/world/01-lloyd.wav`, `02-sheena.wav`, etc., shared with its event
archive aliases. Line numbers preserve the original sequence. A track spanning
several speakers uses names such as `kratos-and-genis-004.wav`. Unreferenced
lines keep `line-NNN.wav`; audio groups without known script bindings keep
explicit numeric fallback names.

Movie mappings select `movies/opening.mkv`, etc. directly. There is no movie
sidecar; playback uses the first audio track.

## Editing a field or model

Start with the mapped field's `field.json`. It points to the scene, collision,
actors, audio, messages and scenario. Follow those direct references to the
property you want to change. Shared character bodies belong under
`meshes/characters/`; field-specific actor and animation selections stay with the
field. Battle motion banks live beside their character and costume under
`battle/party/`.

Optional `collision` paths select GLBs with ground/lighting or floor/solid roles.
Edit their mesh objects and custom properties in Blender with custom-property
export enabled. Empty collision declarations are omitted. See the
[field and actor collision guide](cooked-library.md#field-and-actor-collision)
for role, lighting, flag and coordinate conventions.

Field `actors.json` contains an ordered `models` list. Numeric entries select
IDs from `meshes/mappings.json`; objects such as
`{"resource":53,"model":"fields/my-field/alternate-npc.json"}` select an explicit
model for that field. Shared NPCs and props use the same mechanism as party
characters. `animations` selects IDs from `animations/mappings.json`, while
`local_animations` retains bindings specific to this field.
The named animation bank's `bindings.json` applies each GLB/slot to its listed
actors and model parts. A shared edit reaches all fields selecting the bank;
field-local banks retain their own scoped bindings. Model visibility defaults
such as `hidden_nodes` belong in `model.json`.

Joint names belong to the mesh GLB; model JSON does not repeat them. For model
edits, preserve each gameplay node's `resonance_bone` custom property and enable
custom-property export in your glTF editor. This small index connects scripts,
attachments and visibility bindings to the node, including rigid models without
a skin. Node order may change. Rename joints in the GLB and update clips or named
attachments that refer to them. Only the selected model's nodes are read during
preparation; browsing a catalogue does not load every mesh.

Battle rigs also come from the model GLB. Its hierarchy and rest pose control
both rendering and gameplay attachment sampling. The body definition keeps only
an optional `attachment_nodes` map from gameplay slots to joint names, such as
`{"0":"kk00","1":"kk01"}`. Component models expose `layers` directly and may
list `independent_layers` by resource slot for separately animated components.
No `rig.json`, skeleton copy or parallel rest-pose table needs updating.

Animation clips use standard glTF 2.0 animation in GLB, with unique readable
joint names and ordinary Blender import/export. Shared clips are published once;
shared bank and battle bindings reference them directly. The small JSON bindings
contain game IDs and paths, not animation curves. The
[animation authoring guide](motion-format.md) gives the tested editor settings.
It is also included as `motion-format.md` beside the cooked library's README.

Textures use KTX2. Replace the referenced KTX2 to change its pixels, color space
or mip levels. Sampling, palette selection and material bindings remain with the
owning scene or material. Authored mip pixels stay together in one file and are
preserved exactly by cooking. Export replacement artwork to KTX2, preserving
intentional mip artwork. No texture JSON, loose mip files or checksums need
updating.

Authored SymphoniaScript modules live under `scripts/`. Original scenarios use
`.ssb` VM bytecode with a lossless assembly editing workflow:

```sh
cargo run -p symphonia-script-tools -- scenario disassemble \
  cooked/fields/iselia-1/scripts/scenario.ssb work/scenario.ssasm
# Edit work/scenario.ssasm, then publish it to the matching mod path:
cargo run -p symphonia-script-tools -- scenario assemble \
  work/scenario.ssasm mods/my-mod/fields/iselia-1/scripts/scenario.ssb
```

Use the actual field name from `fields/mappings.json`. The installed `.ssb` is
authoritative; the `.ssasm` is your working source outside the library. Assembly
checks the VM program before atomically replacing the target. A failed command
leaves the previous target intact. An unchanged disassembly assembles to exactly
the original bytes, including raw tables and padding. No mapping or stored hash
needs changing. Restart to load the result; gameplay edits can invalidate saves.

This is instruction-level editing. Labels handle branch targets, but the original
header, registry overlay and auxiliary tables retain explicit layout values;
changing instruction sizes can require adjusting those values. For dialogue,
edit the field's `messages.json`. See the [scenario editing reference](symphonia-script.md#editing-original-scenarios)
for the assembly contract and the authored high-level language.

An optional `--scripts ROOT` project has its own modules and `fields.json` bindings.
It is compiled from the current source when preparing the field. Changing the
project does not invalidate existing saves. The reserved `std` modules still
come from the selected cooked library.

## Checking an edit

Finish editing, then quit and restart Resonance. Application restart is the
supported reload boundary. It recreates the asset server, image and sampler
caches, and scene preparations from the effective library and mod files.
Re-entering a field or reopening a preview does not reload edits. Preparations
retain the files they selected, and a playing recording keeps its open file
handle; finish exports before starting a new run. An atomic file replacement is
preferable to rewriting an open WAV in place.

Enable separate mod directories in the order you want them applied:

```sh
cargo run -p resonance -- --assets local/named-assets --mod mods/translation --mod mods/music
```

Each mod contains only its changed files, using the same paths as the base
library. A later directory overrides an earlier file at the same path, and
`mappings.json` overrides merge by ID. Mods need no registration manifest and
must not contain `content.json`. Remove a `--mod` argument to disable that mod.
The selected directories also apply to save loading, captures, title-audio
recording and the world playground. Keep your original cook and mods separate
so a future recook can replace the base without overwriting your edits.

Saves contain a format version and gameplay state. Current save version: 10.
Changes to dialogue, images, models, scripts, collision or combat rules do not
invalidate saves. Loading uses the current library and checks the resources
needed for the saved location. There is no content fingerprint or whole-library
compatibility scan. Unsupported save versions and missing required resources
produce direct errors. Loading repairs edited amounts with clear limits: HP/TP
are capped at the character's current maxima (including equipment and EX skills),
EXP (9,999,999), currency, gauges, stats, cooking mastery and battle counters are
capped at their gameplay limits, and item stacks respect the current item and
New Game Plus limits. Negative unsigned amounts become zero; empty stacks and expired encounter
modifiers are removed. Unknown resource IDs, malformed data and inconsistent
progression still produce errors. Recovery happens in memory; the file changes
only when you save again. There is no save version migration tool.

Enemy rules use named attacks, projectiles and contacts within the same
`rules.json`. Change a shared definition once; every timeline reference uses the
edit. The [cooked library guide](cooked-library.md#enemy-rules) describes the
format and defaults. JSON formatting keeps short arrays and event rows inline.

## Adding an asset-authored arte

An entry in `game/techniques.json` can select a complete action using
`rules.action`, a library-root path to an authored action JSON. Add a unique
nonzero catalogue ID and name to `mappings`, and that name to the character's
`learning` list. The ID does not need an engine dispatch entry. Ordinary family,
range, TP, learning, menu text, AI enablement and combo assignment come from
that technique entry. New games can know it through the character's
`initial.techniques`; existing parties acquire it through normal learning.

An authored action contains:

```json
{
  "motions": {"110":"battle/artes/my-arte/performance.glb"},
  "effects": "battle/artes/my-arte/effects.json",
  "unison": {"native_id":1000,"altitude":0,"distance":80,"duration":56},
  "attack": {
    "duration":56,"chain_at":46,"recovery":8,
    "events":[
      [0,{"pose":{"clip":"slot-110","rate":0.5,"blend":3}}],
      [0,{"voice":{"stream":3}}],
      [6,{"effect":0}]
    ]
  }
}
```

`motions` adds character-local slots for every costume selecting this arte;
choose unused slots. Conflicts fail preparation. GLB joint names bind to the
selected costume's actual rig. Motion files use 30 animation frames per second;
`rate:0.5` advances half an animation frame per gameplay update. Event ages,
contact windows, duration and recovery use gameplay updates. Hit-stop pauses
the action clock. A `pose_sequence` additionally accounts for authored blend
holds; ordinary `events` use absolute ages directly.

The `effects` bank is optional; absent selects the shared technique bank.
`attack.effect`, timed `effect` events and contact `hit.effect` select programs
in this bank. Named particle templates, models and textures use the ordinary
battle effect format. `unison` supplies the placement/timing and a unique native
identity for Unison recipes without editing the shared Unison catalogue.

The attack uses the shared martial timeline format. Native recipe fields such
as `flight`, `hammer`, `steal`, `fierce_fang`, `hurricane_hit`, and `callback_hits`
are rejected when populated in an asset-authored action; their specialized
native behavior is not part of this format. The timeline supports poses, blade
or body contacts with authored damage and shapes, shared projectiles, motion,
braking, trails, voices, sounds and effects. `traverse_target` takes a positive
`duration` and `clearance`: it crosses to the far side over that many gameplay
updates, easing into and out of movement. The endpoint is captured at the start
of the pass, so recoil does not make the caster chase a moving endpoint.
The first movement step runs on the scheduled tick, including tick zero.
Movement events scheduled at `start + duration` take effect after the pass.
Ordinary integration and hit-stop still apply.
`pass_through` controls body collision during that movement and is cleaned up
when the action ends or is interrupted. Authored actions follow ordinary
admission, TP spending, interruption, damage and recovery rules.

Encounter preparation captures the action's dependencies and audio before
combat, including capacity for artes learned mid-battle; executing it does not
read loose assets.
