# Editing named content

The named layout keeps related files together and uses `mappings.json` only where
the game requests an original numeric ID. This guide describes the implemented
authoring formats. Production now assembles a separate named candidate and exports
only authoring files, but a complete fresh cook and acceptance checks remain
unrun. Source accounting and remaining reader removals are still being completed. See [plan.md](../plan.md)
for implementation status and verification.

Backwards compatibility is deliberately unsupported. Use the current cooked
layout and save schema. An obsolete library needs a fresh cook; an obsolete or
gameplay-incompatible save requires a new game. There are no compatibility
fallbacks, path aliases, save migration tools or test-only old cooked-format
readers in the target design. Tests use the same named layout as the application.

## Finding a resource

A library starts with `content.json`, which identifies its layout version, game
and revision. It contains no file inventory or checksums.

```text
content.json
fields/
  mappings.json
  iselia-1/
    field.json
    scene.json
    collision.json
    actors.json
    audio.json
    messages.json
    scripts/scenario.ssb
audio/
  music/
    mappings.json
    iselia/
      music.json
      music.wav
  effects/
    mappings.json
  speech/
    mappings.json
movies/
  mappings.json
meshes/
  characters/
  monsters/
  equipment/
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
library root, such as `audio/music/iselia/music.wav`.

IDs belong to their original scope. A speech archive has its own member mapping;
member 1 in one archive is unrelated to member 1 in another. Renaming a directory
does not change its machine ID, but its mapping and direct references must follow
the new path. To replace a file at its existing path, leave the mappings alone.

## Other original resources

Content with a known game identity uses the categories above. Otherwise, converted
physical resources keep their original source paths beneath
`resources/physical/`. Archive mappings preserve native member IDs, empty
slots and explicit aliases. These physical conversions do not establish that the
engine has a gameplay binding for every resource.

Speech mappings point directly to WAVs. Empty native speech slots and original
music arrangements without an instrument setup appear in the import diagnostics
outside the library; they are not playable recordings. Native bank-table reports
also stay there. The cooked library contains no disc metadata or import reports.

Shared script node constants use catalogue names or `resource_<source_and_member>`
module names. A module's `Source owner` comment explains its name. The internal
`Skeleton` signature is an importer lookup, not a checksum modders maintain.

## Runtime-selected resources

Fields with dynamic script selectors declare `resources/catalogue.json`. It
selects two separate mapping domains:

| Mapping | Machine ID | Target |
| --- | --- | --- |
| `resources/standalone/mappings.json` | Standalone slot | A directory containing `resource.json`. |
| `resources/groups/mappings.json` | Native group number, starting at 1 | A group directory containing `resource.json` and its member mapping. |
| A group's member mapping | Member number within that group | A directory containing `resource.json`. |

A packed ID such as `0x0002000e` selects group 2, member 14. Group IDs are already
native IDs; do not subtract one when editing the mapping. Native empty-member
fallbacks are ordinary mapping entries targeting member zero's directory. Other
slots keep independent owners even when their initial bytes match.

Resource documents describe physical geometry, textures, animation curves,
collision, scripts or a collection of named parts. Missing original sources,
empty slots and unsupported resources remain visible. Physical conversion does
not by itself provide a playable actor: generic dynamic model, palette and
animation bindings remain incomplete. Supported static field actors retain their
usual field/model definitions. Preparation captures current nested references;
editing these files requires no checksum or dependency inventory update.

## Replacing music

Music belongs under `audio/music/`, even when a field is its best-known use.
Fields, the world and battles select shared song IDs. Replacing one recording
therefore changes every use that selects that recording.

Songs use one `audio/music/mappings.json`, shared by fields, the world and battles.
Original disc locations do not affect their names or runtime selection.

Each song's `music.json` selects its WAV and optional loop:

```json
{
  "version": 1,
  "audio": "audio/music/iselia/music.wav",
  "loop": {
    "start_frame": 48000,
    "end_frame": 2880000
  }
}
```

Use mono or stereo 16-bit integer PCM WAV, at 8–192 kHz, with a positive length
of at most one hour. The player reads rate, channel count and length from the
current file. You do not update a checksum, byte count, duration or dependency
inventory after exporting a replacement.

Loop positions count sample frames, starting at zero. A stereo frame contains
both channels. The example plays the introduction once, then repeats frames
48,000 through 2,879,999. At 48 kHz those boundaries are 1 and 60 seconds.
Remove `loop` or set it to `null` for a one-shot. If an edit changes the sample
rate, introduction or repeating section, update the frame positions accordingly.

A correct loop wraps continuously without a blend. Check the musical timing,
notes crossing the boundary and the reverb tail. Matching the first and last
sample values alone is insufficient. Resonance does not add a crossfade to hide
an incorrect cut. Original tracks can contain several musical passes in one WAV
repeat when their timing or effect state requires it.

The recording includes its intended music reverb. Playback does not apply that
reverb a second time. A few original tracks inherit an environment; their
descriptor can contain a `battle` object with its own `audio` and optional `loop`.
Omitting that object makes the main recording serve both environments. Keep
music in its shared song directory in either case.

Sound banks live under `audio/effects/<bank>/`, with their instrument WAVs under
`audio/instruments/effects/<bank>/`. Fields and battles use the same effects
mapping. `battle/audio.json` holds the shared battle playback settings.

An effect's `sound.json` selects its playback kind. Original effects use
`"kind": "synthesized"` with editable programs and shared instrument WAVs under
`audio/instruments/`. To replace a cue with a recording, replace its `sound.json`:

```json
{
  "kind": "wave",
  "version": 1,
  "audio": "audio/effects/door/close.wav"
}
```

Keep the cue's existing mapping and put your WAV at the referenced path, either
in the library or an enabled mod. The same definition works for field, battle
and menu effects. Mono and stereo WAVs can use 8/16/24/32-bit integer PCM or
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

Speech uses named WAV files and scoped archive mappings.

## Editing a field or model

Start with the mapped field's `field.json`. It points to the scene, collision,
actors, audio, messages and scenario. Follow those direct references to the
property you want to change. Shared character bodies belong under
`meshes/characters/`; field-specific actor and animation selections stay with the
field. Battle motion banks live beside their character and costume under
`battle/party/`.

Editable animation documents use `.motion.json`. Their timing comes from the
current motion data. Meshes use GLB; changing a rig or indexed bone relationship
also requires compatible motions and attachment bindings.

Textures use ordinary PNGs. Replace an image at its existing path to change its
pixels. Sampling, palette selection and material bindings stay with the owning
scene or texture catalogue. Plain PNGs use linear channels, matching imported
textures. Embedded display profiles do not change that interpretation.

A texture with authored mip levels has one `.texture.json` beside its PNGs:

```json
{
  "version": 1,
  "image": "meshes/props/sign/textures/wood.png",
  "color_space": "linear",
  "mips": {
    "kind": "authored",
    "levels": [
      "meshes/props/sign/textures/wood-mip-1.png",
      "meshes/props/sign/textures/wood-mip-2.png"
    ]
  }
}
```

Paths start at the library root. The scene references this descriptor, which
declares color interpretation and the mip policy. Dimensions come from the PNGs;
each successive mip halves the previous dimensions, rounding down with a minimum
of one pixel. Imported mip pixels are preserved exactly. Edit those PNGs when you
want to keep control over each level.

For replacement artwork, set `"mips": {"kind": "generate"}` to generate the full
chain automatically from the base image. Set `"color_space": "srgb"` for artwork
whose RGB channels use sRGB encoding; generation filters in linear color with
alpha weighting. Omit `mips`, or use `{"kind":"none"}`, for one level. PNGs must
use at most eight bits per channel. No KTX2 export, stored dimensions, checksum or
dependency inventory needs updating.

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
Its gameplay participates in save compatibility, including task arguments and
whether an entry runs after restoration. Moving the project or changing comments
preserves its identity. The reserved `std` modules still come from the selected
cooked library.

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

Save compatibility uses current gameplay data after applying mods. This includes
field collision and interaction data, model attachment bones and rigid bounds,
script and animation completion timing, enemy statistics and rewards, battle
rules, contact rigs, motion curves and secondary bone motion.
Moving files with their references updated, reformatting JSON, or replacing
textures, meshes, recordings and dialogue preserves identity when gameplay stays
the same. A mesh's indexed bones and rigid bounds can affect field attachment
queries and pushable blocks; changing those is a gameplay edit. A change to reach,
collision, attack timing or rewards changes identity.
The current save schema is 7; older schemas require a new game.

This expanded projection is implemented but has not yet completed the final
native-data, runtime and performance validation described in `plan.md`.
