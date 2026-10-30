# Editing this asset library

This directory contains Resonance's editable game content. Browse a category and
look for the location or resource name. Use `mappings.json` when you need to look
up a machine ID or rename a resource, then edit the file for your change.
Finish editing before starting Resonance. **Restart the application to load edits**;
re-entering a field does not reload them.

## Find the owner

Menu sprite entries in `ui/menu.json` use names such as `a_pressed`, `lloyd`,
`poison_1`, item names and recipe names. Rectangles are `[x, y, width, height]`
within the shared atlas. Item and recipe names resolve through the existing
`game/items.json` and `game/cooking.json` mappings. Default party names come from
`game/characters.json`; optional `sprites.names` entries override only menu text.
The named palette also supplies matching number colors. A number-color override
can be one RGBA color or a pair of top/bottom colors.

Battle artwork in `ui/battle/battle.json` selects characters, commands, HP/TP
colors and other roles by name. Image references are direct paths; KTX2 headers
supply their dimensions. Missing optional portraits remain local to portrait
drawing. Game-over artwork uses a direct background path and named `load`/`title`
captions. `ui/story-subtitles.json` is an ordered list of frame cues: a cue without
`lines` clears the text. Nonempty lines retain explicit pixel positions.
`ui/controls/gamecube.json` remains the ordered bank of 32 descriptive image paths;
its order maps the image selectors used by existing field overlays.

Credits live in `ui/credits/credits.json`. Its `blocks` contain centered text
(`{"lines":["Heading ","Contributor "]}`), pixel gaps (`{"gap":50}`) or a
direct image path (`{"image":"ui/credits/namco.ktx2"}`). Text can use
`"align":"left"`; otherwise each line is centered independently. Optional
`line_spacing` is the distance between lines within a block. After the last line,
the cursor advances by `style.line_height`; an image does not advance it and can
set an optional `offset_x` in pixels.
Keep trailing spaces when preserving the imported centering. Reorder or add
ordinary lines to edit the credits; there are no text-control opcodes or image
indices. Image dimensions come from KTX2. `scroll` controls the authored height,
speed divisor in update ticks and stopping margin; `final_hold_ticks` controls
the pause after the music. The recording stays in shared `audio/music/`.

| Content | Where to look | What to edit |
| --- | --- | --- |
| Item rules and captions | `game/items.json` | Named items with ID mappings, use rules, stats, eligibility and text. |
| Characters and starting state | `game/characters.json` | Names, starting stats/equipment, growth and experience thresholds. |
| Titles and awards | `game/titles.json` | Named title captions, growth bonuses, costumes and ordered award rules. |
| Fields | `fields/mappings.json` | The named field's `field.json` contains audio choices and points to dialogue, scenery, collision, actors and scenario files. |
| Dialogue | The field's `messages` reference | Text and message controls. Message IDs are positions in the JSON array; preserve its order. |
| Field settings | `fields/settings.json` | Shared field effects, shadows, face cycles, particles and save-point notices; scenes contain local overrides. |
| Shared models | `meshes/mappings.json` | Named characters, costumes and props: `model.json`, meshes, textures and default motions. Shared edits affect every consumer. |
| Script joint names | `meshes/joint-mappings.json` | Original symbolic joint identifiers mapped to readable GLB node names, shared by field and battle scenes. |
| Shared animations | `animations/mappings.json` | Named GLBs, explicit shared paths and aliases. Joint/actor bindings live once in `animations/bindings.json`. |
| Standalone script models | `meshes/standalone/mappings.json` | Original standalone IDs select named models without confusing the same numbers with party or field-local models. |
| Shared camera tracks | `cameras/mappings.json` | Editable GLB position, rotation and target tracks, selected by their original resource IDs. Rotation preserves authored camera roll. |
| Texture banks | `textures/mappings.json` and `textures/banks.json` | Named ordered image banks with sampler settings; their KTX2 files remain with the artwork's owner. |
| Music | `audio/music/mappings.json` | Direct named WAVs with standard sampler loops, shared by fields, battles and the world. |
| Audio mix | `audio/mix.json` | Shared dialogue volume curve, music reverb choices and default field/menu and battle acoustics. |
| Speech | `audio/speech/` | Battle voices are grouped by speaker and purpose; events by descriptive scene and speaker; skits by displayed title and speaker. Stream IDs use `audio/speech/mappings.json`; cue IDs use `audio/mappings.json`. |
| Effects | `audio/effects/` | `audio/mappings.json` selects WAVs or compact recipes. Character voices live under speech even when they use a recipe. |
| Movies | `movies/mappings.json` | Direct scene-named MKVs, such as `mana-cannon-firing.mkv` and `worlds-reunited.mkv`; playback uses the first audio track. |
| Battles | `battle/` | Named arenas, enemies, party costumes, scenes and shared combat rules. Equipment is under `meshes/equipment/`. |
| Battle formations | `battle/formations.json` | Named enemy groups, spawn positions, encounter settings and weighted pools. Its `mappings` connects request IDs to named groups. |
| World | `world/world.json` | Inline audio choices and references to movement, collision, encounters, landmarks and continent terrain. |
| Skits | `skits/mappings.json` | Scenario and messages; availability is in `skits/catalogue.json`, and named portraits/tracks are in `skits/portraits.json`. |
| Screens | `ui/` | Title, boot, dialogue, battle, menus, credits and game-over definitions. |
| Fonts | `fonts/dialogue.json` and `fonts/battle.json` | Shared glyph definitions and spacing, with the matching KTX2 atlas beside each document. |
| Authored scripts | `scripts/` | Ordinary `.sym` source, compiled automatically when preparing the content. |

Additional original variants live under `meshes/originals/`. The party catalogue
uses character IDs for the original battle bodies. Rutee, symbols and other
models without known script IDs have direct named model paths. These are ordinary
model documents with GLB meshes and motions. Seven unassigned motions live in
`animations/unidentified/`; a `SceneClip.motion` can reference them directly.

`battle/scripts/mappings.json` preserves original event program IDs 2–9. IDs 5–9
share one program. Playable battle event definitions remain under
`battle/events/`; a preserved program alone does not supply its actors or rules.
`battle/scripts/common.ssb` retains the separate common program.

Four arrangements without known music IDs are ordinary looping WAVs under
`audio/music/unmapped/`. They use the native instrument bank with dry output;
their original reverb selection is unknown. To use one, reference its WAV from a
music mapping. Complete alternate font grids are in `fonts/styles/`, described
once by `fonts/styles.json`; `fonts/mappings.json` preserves native style indices.
The extra grids retain the original cell order without assuming an unknown
Kanji-to-Unicode mapping.

The named `disc-change` and `credits-backgrounds` banks in `textures/banks.json`
retain the remaining artwork under `ui/disc-change/` and `ui/credits/`. Materials
with `mipmapped: true` use the levels already present in their KTX texture.
Ordinary materials sample the base level. Editing a camera's rotation controls
its view; its separate target track remains available to script operations.

Movies name their first audio track `Main soundtrack`, which is the track used
for playback. `Alternate 1` retains the second soundtrack in each existing MKV.
Its language or mix has not been established by the available movie headers or
playback declarations; the label does not claim it is a dub or instrumental mix.
Track names are ordinary Matroska metadata. Keep the desired game soundtrack
first when editing a movie; changing its label alone does not change selection.

Mapping targets are relative to the directory containing that mapping. For
example, `"1": "iselia-1"` in `fields/mappings.json` would select
`fields/iselia-1/field.json`; use the actual names and IDs in this library.
References inside resource documents are relative to the library root. IDs in
different banks or categories have separate meanings.

Some mappings need an explicit value beyond a filename: a silent speech entry
owns its playback clock, a model alias can retain its own name, and a dynamic
resource can bind several media roles. A `null` equipment target means no visual
attachment. These are category values, not an extra manifest or discovery layer.

Replacing an existing file needs no mapping change. A rename requires updating
its mapping and direct references. Files owned by distinct resources remain
independently editable even when their initial bytes match. Deliberately shared
files have ordinary references from their consumers.

Battle speech uses speaker and purpose names, such as
`audio/speech/battle/colette/angel-feathers.wav` and
`audio/speech/battle/raine/first-aid.wav`. A chant reused by several spells has a
common name such as `casting-chant-01.wav`; it is one shared recording. Recipes
beside the recordings describe the cases that require programmed playback.

`audio/mix.json` owns the common audio settings once. `voice_gains` maps dialogue
volume levels 0–127 to gain. `reverbs` defines named presets, each with these
parameters:

```json
"field": {
  "coloration": 0.8,
  "mix": 0.7,
  "decay_seconds": 3.6,
  "damping": 0.6,
  "predelay_seconds": 0.08
}
```

`coloration`, `mix` and `damping` range from 0 to 1. `decay_seconds` ranges from
0.01 to 10 seconds; `predelay_seconds` ranges from 0 to 0.1 seconds. `mix` controls
the wet/dry balance. The two effect buses have explicit names:
`"field_reverbs":{"auxiliary_a":"field","auxiliary_b":"effects"}` supplies
the defaults for fields, the world and menus; `battle_reverbs` supplies the
battle defaults. Editing a shared preset changes every reference to it in this
file. To separate one use, copy the preset under a new name and change that use.

`music_reverbs` groups music IDs by the preset applied to auxiliary A when a song
starts. For example, `"battle":[85,86]` selects that preset for those IDs.
`"keep_current":[0,111]` leaves the bus as it is; it does not disable reverb.
Every ID through the last selection must occur once, including keep-current
choices. These settings affect live effects; the music WAV already contains its
recorded reverb. Named presets live only in this file, with no extra catalogue.

The Music volume setting uses the first 128 entries of `mix.volume` in
`audio/synthesis.json`, the same volume curve as the effect mixer. It applies
to recorded WAV playback in fields, battles and the title screen. Script fades
multiply that gain separately; setting Music to zero mutes it.

A place's audio definition, the battle audio definition, a programmed cue or
`audio/menu.json` can declare `reverbs` to override its default pair. Each of
`auxiliary_a` and `auxiliary_b` then contains the five named parameters directly.
Leave `reverbs` absent to follow the shared context.

Audio selections live inside each field's `field.json` and `world/world.json`:

```json
"audio": {
  "music": [7, 77, 78],
  "effects": [1, 2, 3],
  "speech": [{"ids": [917923, 917924], "sample_rate": 32028}]
}
```

The IDs resolve through `audio/music/mappings.json`, `audio/mappings.json` and
`audio/speech/mappings.json`, respectively. Empty choices can be omitted.
A speech group's optional `sample_rate` fixes its playback clock; without it,
the current WAV supplies the rate. Each selected voice belongs to one group.
These groups describe playback choices and can span any scene or speaker.
An explicit rate intentionally changes the recording's speed and duration: the
duration is its frame count divided by that rate. The imported field dialogue
uses 32,028 Hz for 32,000 Hz recordings; world cinematic dialogue uses the WAV
rate. These are caller choices, so replacing a WAV does not silently remove an
explicit clock. Omit `sample_rate` to follow a replacement WAV's own rate. Silent
lines use the same rule with their stored frame count and rate.
`battle/audio.json` lists only exceptional `speech_rates`; other battle speech
uses its WAV rate. Battle stream request IDs have direct entries in the speech
mappings, alongside scenario voice IDs. Several IDs may point to the same file;
editing that file changes their shared recording, while remapping one ID changes
only that request. No archive numbers or ID reconstruction rules are needed.

## Keep a mod separate

Copy only the files you want to change into another directory, preserving their
paths relative to this library. For example, a dialogue mod needs only
`mods/my-dialogue/fields/<actual-field-name>/messages.json`.

```sh
resonance --assets /path/to/this-library --mod /path/to/mods/my-dialogue
```

Repeat `--mod` to enable more mods. Later mods win for the same file. Mapping
overrides merge by numeric ID, so a mod's `mappings.json` needs only its changed
entries. Remove the corresponding `--mod` argument to disable a mod. Mods require
no registration manifest and must not contain `content.json`.

The cooked library has no hash-based names, checksums or fingerprint files.
It also contains no copy of the original archive tree. Native headers, skeleton
dumps and decoding reports belong to the importer's external diagnostics.
Dynamic IDs resolve through the model, animation and texture category mappings.
References use ordinary paths. Editing a file needs no byte-count update, source
index or dependency inventory. Resonance follows the current references, checks
their structure and builds its own prepared data. Keep the base library and your
mod sources so a future fresh cook does not replace your work.

## Example: change Iselia dialogue

The original village field is `fields/iselia-0332/`; its machine ID is `332`.
Other Iselia rooms have their own named directories. Open that field's
`field.json` to find the file for your change:

| Property | Follow it to edit |
| --- | --- |
| `messages` | Dialogue text and message controls. |
| `scene` | Scenery, materials, camera tracks and visual effects. |
| `collision` | Ground and character-lighting meshes in an optional GLB. |
| `actors` | Named models, shared rosters, named animation banks, local actors and field-local animation bindings. |
| `audio` | The field's choices from the shared music and effect libraries. |
| `script` | Scenario instructions, using the assembly workflow below. |

To make a dialogue mod:

1. Copy `fields/iselia-0332/messages.json` from this library to
   `my-dialogue/fields/iselia-0332/messages.json`.
2. Find the line you want in the copied file. Ordinary messages are strings,
   for example `"Welcome to Iselia!"`. A message with substitutions or playback
   controls is an array of strings and named controls. Edit its strings while
   keeping the controls you want. Keep the outer message array in the same order;
   its positions are the IDs used by the scenario.
3. Start Resonance with `--assets /path/to/this-library --mod /path/to/my-dialogue`
   and visit the corresponding conversation. After another edit, quit and restart
   the application. Remove `--mod` and restart to restore the original dialogue.

This replacement needs only the one messages file. The field definition, mapping
and shared character files are inherited from the base library. For a library
whose paths have been renamed, follow its current `field.json` references instead.

## Dialogue text and controls

A scenario with no message positions needs no `messages.json`. Fields, the world,
battle events and dynamic scripts omit their `messages` reference when they have
no dialogue. An explicit reference must point to a real, valid table.
For skits, the title directory determines the path: `messages.json` is optional.
To add skit dialogue in a mod, add that file under the skit directory; no
catalogue flag or mapping change is needed. To add a table to a field or event
that previously had none, also set `messages` in its owner file. Each preparation
retains the table, or its absence, until the next preparation.

Each `messages.json` is an ordered list. Keep empty strings and existing positions
so scenario IDs continue selecting the same messages. A message is either a string
or an ordered array of strings and controls:

```json
[
  "Welcome to Iselia!",
  [{"actor_name":"colette"}, ", wait!", {"color":"red"}, "Come back!"],
  ""
]
```

Strings use normal JSON escapes: `\n` starts a new line and `\f` starts a new
dialogue page. No markup parser interprets punctuation inside ordinary text.
Character substitutions use the current saved names, including player renames.

| Control | Meaning |
| --- | --- |
| `{"actor_name":"lloyd"}` | Insert a character's current name. Supports the nine party characters, `noishe`, `controlled`, or a numeric actor ID. |
| `{"item_name":3}`, `{"title_name":1}`, `{"technique_name":57}` | Insert the corresponding localized label using its gameplay ID. |
| `{"number":12}` | Insert a number; a calculated expression can replace the literal. |
| `{"color":"red"}` | Change the following text's color. Names are `white`, `blue`, `red`, `purple`, `green`, `cyan`, `yellow` and `flashing`. |
| `{"delay":0}` | Reveal following characters immediately. Positive values give ticks per character; `-1` restores the player's normal delay. |
| `{"voice":655360}` | Start the stream selected by that ID in `audio/speech/mappings.json`. `{"voice":"stop"}` stops it. |
| `{"number_input":3}` | Reserve three editable digits; supported widths are 1–10. |

Calculated values are named operations with ordered operands. For example,
`{"number":{"add":[{"state_i32":2064},1]}}` reads a signed 32-bit scenario
state value and displays one more than that value. `state_i8` and `state_i16`
read signed 8-bit and 16-bit values. Their numbers are byte offsets in scenario
state, not file offsets. `indexed_reference` takes a state reference and an
element index. Arithmetic, comparisons, bitwise operations and assignments use
the scenario interpreter's existing integer rules. Unary operations take a
one-element operand array. `evaluate` runs an expression without displaying it.
No bytecode bytes, numeric control opcodes or expression terminators belong in
the message document.

Some original debug messages contain game-service calls, written as
`{"call":{"function":"test_event_bit","arguments":[150]}}`. These retain their
function names and arguments; the dialogue evaluator reports unsupported calls
instead of supplying a guessed value. Recovering descriptive scenario-variable
names and implementing those services remain separate work. The same UTF-8
strings and named controls can be supplied by another source importer.

## Menu text and synopsis

Menu descriptions and training-manual paragraphs are ordinary strings. Use `\n`
for an authored line break. When a passage needs color changes or button icons,
use an ordered list of strings and controls:

```json
["Press ", {"button": "a_pressed"}, " to confirm.\n",
 {"color": "gold"}, "Remaining:", {"color": "white"}, "%d"]
```

Each passage starts in white. Colors are `black`, `blue`, `red`, `purple`, `green`,
`cyan`, `yellow`, `gray`, `gold`, `white` and `pale_green`; they select the editable
menu palette. A color change lasts through subsequent text and line breaks.
Keep formatting placeholders such as `%d` where a menu supplies a value.

Button names describe the pictured glyph, independently of input bindings:
`a`, `b`, `x`, `y`, `l`, `r`, `z`, `start`, and their `_pressed` variants;
`stick`, `c_stick`, `d_pad`, and the available directional variants. The stick
and D-pad have `_up`, `_down`, `_left` and `_right`; the C-stick has `_up` and
`_down`. The three decorative glyphs are `panel_left`, `panel_middle` and
`panel_right`. Their images and rectangles belong to `ui/menu.json`.

`game/menu/synopsis.json` maps all 200 saved event IDs to named entries.
Identical entries can share one owner; real story variants keep separate entries.
The twelve month labels and `sylvarant`/`tethe_alla` world choices are named. Each entry's
`text` is an array of displayed lines; `""` preserves a blank line. A line can
also use the color controls above. Optional `updates` replace the text for
event states 2 or 3, falling back to the default when absent:

```json
{
  "heading": "Iselia",
  "title": "A new journey",
  "text": ["The journey begins.", "", "There is much to discover."],
  "updates": {"3": ["The journey is complete."]}
}
```

An explicit empty update displays no lines. Removing an update restores the
default for that state. Titles, locations and text remain local to their entry.

## Remaining menu owners

`game/menu/manual.json` is an ordered list of named chapters and topics. A topic's
`learned_flag` is its saved unlock flag. Text and button controls are inline.

`game/menu/rename.json` names all nine character reset presets and the `confirm`,
`restore` and `cancel` commands. Its keyboard has eight rows of thirteen cells;
spaces are real cells. Reset presets are deliberately independent of ordinary
character names: the original game uses different spellings for seven of them.

`game/menu-data.json` keeps crafting and map/shop declarations beside their
captions. `crafting.mappings` maps script vendor IDs to named vendors, whose
ordered `recipes` select named recipe definitions. Results and ingredient counts
use the existing item names from `game/items.json`. Real recipes remain editable
when no current vendor offers them; the unused empty sentinel is omitted.
Recipe keys are internal to this document, so renaming one only requires updating
its vendor references.

`world_map.mappings` maps saved shop IDs to named shops. Each shop owns its `name`
and ordered item list. `shop_visit_groups` uses those shop names; opening any
member records all IDs in the group as visited. Different named stores retain
independent inventories even when their current stock happens to match.
`field_locations` uses existing field names and points to the shared landmark IDs.
Only fields available in the installed source are published. The two world
captions live in `world_map.names`. Landmark selections still use their stable
IDs in `world/landmarks.json`.

Grade-shop purchases remain in visible menu order with named benefits, prices,
labels and exclusions. Omitted `excludes` means no conflicting benefit.

## Menu and dialogue images

Menu portraits, world maps, icons, panels and arrows live directly in
`ui/menu/`, with names such as `lloyd-portrait.ktx2`, `sylvarant-map.ktx2` and
`scroll-up.ktx2`. Dialogue-specific symbols, shading and lined fill are in
`ui/dialogue/`. Their references remain in `ui/menu.json` and `ui/dialogue.json`.

Shared frame pieces, background patterns and fills belong to `ui/windows/`;
shared cursor artwork belongs to `ui/cursors/`. Editing one shared image affects
its menu and dialogue uses. The gold blade cursor and pale hand cursor are
different images. Ornate and gold frame pieces are distinct; their common fill
is shared. The white fill is an actual opaque fill bitmap.

The five shared patterns use opaque alpha. Menu patterns already take opacity
from the window color (`opaque: true`); their unused source-alpha variations
are not separate library images. RGB edits affect both uses. Other images
retain their meaningful alpha channels. Texture sampling and opacity remain
editable in the existing metadata.

### Dialogue drawing roles

`ui/dialogue.json` directly names `symbols`, `shading`, `lined_fill`,
`plain_fill` and `cursor` images. Its five `patterns` entries are the background
choices in preference order. `lined_fill` is the sixth choice for decorated
windows; `plain_fill` is the sixth choice for plain windows. Image width and
height come from KTX2 headers, so replacing an image needs no size edit in JSON.

`default_style` names the default dialogue window style: `plain`, `ornate` or
`gold`. The player's window preferences take precedence. `selection.color`
is RGBA bytes; `row_offsets` shapes the nine one-pixel highlight rows.
`bob_amplitude` is pixels and `bob_radians_per_tick` is the phase increment per
presentation update in the existing 24-update cursor cycle. These coefficients
retain their original precision before integer pixel rounding. `font` names
the shared bitmap-font document. Dialogue images are required resources;
font-only consumers can read this reference without loading the images.

### Menu drawing roles

`ui/menu.json` binds images directly to drawing roles. For example,
`images.lloyd_portrait` is `"ui/menu/lloyd-portrait.ktx2"`. `images` also names
other portraits, the two maps, scroll arrows, panel pieces and the icon atlas.
A normal image is a path string. An image with special sampling can instead be
`{"image":"ui/windows/frame-fill.ktx2","repeat":true,"opaque":true}`.
`repeat` tiles the image; `opaque` uses window-color opacity instead of image
alpha. Both default to false. Width and height come from the KTX2 header.

The five `patterns` entries are the background choices in preference order.
`windows.plain`, `windows.ornate` and `windows.gold` each define their sixth,
plain background as `fill`. A style can override the first five choices with
its own `patterns` list. A decorated style's `frame` names its top, bottom,
sides, corners and flourishes. `heading` and `cursor` are optional direct image
references; omitting `cursor` uses the dialogue cursor. A style without `frame`
uses the plain beveled drawing. Frame fills use the style's `fill` image.

Window geometry settings remain editable. `cursor_motion` is amplitude in
pixels followed by radians per presentation update; `outset` and the flourish,
foot and left-edge settings control the placement of the frame pieces.
Colors use RGBA bytes. `sprites.rects` retains actual icon-atlas rectangles;
these are regions of one bitmap, not additional image files. The library's
root version is the menu contract's only version.

## Startup logos

`ui/boot/sequence.json` is an ordered list, for example:

```json
[
  {"image":"ui/boot/licensed-by-nintendo.ktx2","fade_out_at":180},
  {"image":"ui/boot/namco.ktx2","fade_out_at":240,"skip_after":120}
]
```

Reorder, add or remove entries directly. `[]` skips the logo sequence. Each
logo is centered using its image dimensions. `fade_out_at` is the update when
fade-out starts, measured from that logo's entry. Timing uses presentation
updates (60000/1001 per second, approximately 60). `fade_ticks` defaults to 64
for each fade and must be positive; `fade_out_at` must follow fade-in. The
entry's natural duration is `fade_out_at + fade_ticks` updates.

An optional `skip_after` lets a confirmation press begin fade-out sooner; it
must follow fade-in and be no later than `fade_out_at`. Without it, the entry
cannot be skipped. Optional `background` is an RGB byte triplet, defaulting to
black; the background fades with the image. Captures and startup recordings
use the edited sequence's duration. There is no fixed logo count, source-bank
order, per-image metadata file or separate version number.

## Title artwork

`ui/title/title.json` chooses the day/night fields, music and menu sounds. Its
`artwork` list describes the images actually drawn, in drawing order:

```json
{
  "image": "ui/title/new-game.ktx2",
  "selected_image": "ui/title/new-game-highlighted.ktx2",
  "choice": "new_game",
  "top": 295.0
}
```

Images are centered horizontally. `top` is their top edge in the 640 × 480 UI
canvas; dimensions come from the image itself. Optional `choice` values are
`new_game`, `continue` and `customize`. The selected image is optional and
requires a choice. Images without a choice, such as the logo and copyright,
use only `image` and `top`. Reorder, add or remove artwork entries directly;
menu behavior follows the choice name, independently of the entry's position.

Artwork sits beside this document with readable filenames. Extra Game Record,
Sound Test, alternate Continue and Lloyd-in-a-field images are retained for
mods; the default title does not load them. The tree background has one owner
at `ui/game-over/background.ktx2`. Original disc labels have no role in the
unified library. No native bank, image indices, per-image wrappers or additional
mapping file is required. Startup logos are configured separately.

## Bitmap fonts

Each font has a JSON definition and an atlas beside it: `fonts/dialogue.json`
uses `fonts/dialogue.ktx2`, and `fonts/battle.json` uses `fonts/battle.ktx2`.
The definition specifies the atlas dimensions, `line_height`, common
`glyph_size` and common horizontal `advance`, all in pixels.

Each entry in `glyphs` declares its characters and atlas `position`. Characters
that use the same image and spacing share an entry. For example, these two
punctuation characters share one bitmap and override the common advance:

```json
{"characters": "、､", "position": [1, 157], "advance": 9}
```

An entry can override `size` and `advance`. Long character lists can be split
across an array of strings; the strings concatenate without adding spaces or
line breaks. The large fallback entry covers only its listed characters, so
adding text does not silently invent font coverage.

To give one character different artwork or spacing, remove it from its shared
entry and add a separate entry. A character may occur only once. Changing a
shared entry affects all of its characters; changing a common metric affects
entries that inherit it. If you resize or rearrange the atlas, update its
dimensions and glyph positions to match. The font documents need no individual
version fields or additional mappings.

## Media and motion

`actors.json` selects shared models by name in its ordered `models` list and
shared animation banks by name in `animations`. For example:

```json
{
  "models": [{"group": "party"}, "characters/militia-iselia"],
  "animations": ["lloyd-e073"]
}
```

Use bank names from `animations/mappings.json`. `meshes/groups.json` holds the
ordered `party` roster once.
A group expands at its position in the list and cannot contain another group.
To change a roster everywhere, edit its group. To change one member in one field,
replace that field's group entry with its members and edit the relevant entry.
No model or animation payload needs copying for that selection change.

An animation mapping string selects `animations/<name>.glb`. To share a motion
owned elsewhere, use `{"name":"characters/lloyd/shared-walk","motion":"meshes/characters/lloyd/animations/walk.glb"}`.
An alias such as `{"name":"characters/lloyd/alternate","resource":65536}`
selects another animation ID without copying its file. Keep names unique and
avoid alias cycles.

`animations/bindings.json` retains explicit actor selections and joint maps.
Its `rigs` table describes each physical model GLB's editable joint names and
binding identifiers once; `tracks` describes each motion bank once. These
tables let a dynamically requested motion bind to a model without a repeated
definition for every possible pairing. When renaming a model joint, update its
rig entry's `name`; keep its binding identifier and optional `source` label when
the existing motions should continue to select it.

`meshes/standalone/mappings.json` uses the same model targets as the main mesh
mapping. A field's optional `local_resources` lists the low IDs whose field-owned
models take precedence over global standalone selections. It does not include
the automatically available party roster. Keep those contextual selections when
editing a field; the same number can otherwise select different resources.

`textures/mappings.json` selects names in `textures/banks.json`. A bank is an
ordered array of image paths, with `null` for an invisible slot. Nondefault
samplers use image objects or an `entries` array with a shared `sampler`.
Preserve order when replacing artwork: scripts and materials can select an
image by index. A transparent slot needs no empty KTX file.

`meshes/mappings.json` connects script IDs to those names. An ordinary string
names its model directory. An alias explicitly separates its name and model:

```json
{
  "1": "characters/lloyd",
  "587203329": {"name": "characters/lloyd/story", "model": "characters/lloyd"}
}
```

Both entries use `meshes/characters/lloyd/model.json`, but retain distinct script
identities and animation targets. Change an alias's `model` to give it different
art without changing its name. Each name must be unique. The `/field` aliases
identify the field-bound NPC/prop handles; their unsuffixed counterparts identify
the separately requested shared actors. `/story`, `/equipped` and `/interactive`
identify story costumes, Colette's equipped wings and the interactive chest.
Two alternate prop handles use `/alternate`; their precise variant roles remain unknown.
These labels carry no disc or platform ownership.

Compiled scenarios keep their original symbolic joint requests. The shared
`meshes/joint-mappings.json` translates those identifiers to editable model names:

```json
{
  "Bone_sebone03": "spine-03",
  "Bone_te_R": "hand-right"
}
```

Names absent from this table are used directly. The mapping applies when a
scenario looks up an actor's joint, including its current costume. Changing a
mapped GLB joint name also requires changing its target here; scenarios do not
need patching. This is an ordinary JSON document: a mod replaces the complete
table. A failed lookup logs the requested name, mapped name, actor, model and
field once per distinct failure in that scene, rather than silently detaching
an effect such as wings.

A field-specific model is an explicit entry in that same list, for example
`{"resource":53,"model":"fields/my-field/alternate-npc.json"}`. This also lets a
field override a shared model without changing other fields. The optional
`hidden_geometry` names joints in the primary model GLB and overrides that
model's default. Omit it to inherit, supply a list such as `["hat", "coat"]` to
replace the default, or use `[]` to show all geometry. These must be actual,
unambiguous joint names in your GLB. Hiding a joint's geometry keeps its transform
and skeletal children active. The same named setting lives in `model.json` for
shared defaults; runtime script node indices are resolved during preparation.
Local model definitions and geometry sit directly in the field's `models/`
folder, for example `models/dark-dragon.json` and `models/dark-dragon.glb`.
Their images sit beside them; clips use the field's `animations/` folder with
that model's name as a prefix. Unidentified local models use `unidentified-N`,
with the request ID kept in `actors.json`. Do not infer identity from that number.

Identical complete models used by several fields have one owner under `meshes/`.
Each local actor still keeps its own field-scoped request ID and direct model
path. Editing that shared model affects its users; point one local declaration
at another JSON file to make an independent variant. Matching appearance alone
does not merge models with different clips, visibility or collision.

`local_animations` retains the field's bank IDs and named actor/GLB targets.
The remaining 214 banks have no established single action name; inventing a verb
or adding a naming sidecar would misrepresent their contents. Known service
roles such as opening doors remain named. Model defaults such as visibility and
built-in motions live in the model definition once.

Embedded model/scenery clips use one row per shared performance and request set:

```json
"clips": [
  {"motion": "meshes/characters/lloyd/animations/walk.glb", "slots": [36]},
  {"motion": "fields/my-field/animations/greeting.glb", "slots": [12], "banks": [65537, 65538]}
]
```

`slots` lists the requested motion slots. Omit `banks` to bind those slots to the
model itself; otherwise the row binds every listed slot in every listed bank.
Keep different slot sets in separate rows to avoid selecting extra combinations.
`pose_chains` remains an optional list of hair/cloth chain names. Curves and
actual duration come from the GLB; no keyframes or sampled duration live here.
Editing one row changes all of its listed requests. Split the row and point one
selection at another GLB when only that request should change.

Rows preserve the first clip used by automatic playback and the first choice
for consumers that select only by slot. Do not reorder conflicting slot choices
casually. A request may occur only once per part. Shared party body/outline groups
use this same `clips` format. This is separate from actor-targeted animation-bank
definition files, whose rows select named actors and parts.

Embedded animation files sit directly inside their owner's `animations/` folder;
field-local models share their field's animation folder.
Known field request roles use names such as `idle`, `walk`, `run` and
`field-service-open-door`. Other files use their animation-bank names or neutral
`motion-N`/`field-motion-N` labels. Non-primary parts add `part-N-`; genuinely
different files with the same role use `-variant-N`. There are no intermediate
`part-N/local/slot-N` or hexadecimal resource directories. Some catalogue bank
names and local model identities remain unidentified; filenames do not invent
an action or speaker for them.

A model can declare common `textures`, `clips` and `secondary_motion` once in
`shared`, beside its ordered `parts` (or an attachment's `layers`):

```json
{
  "shared": {"textures": ["meshes/characters/lloyd/textures/body-texture-0.ktx2"]},
  "parts": [
    {"mesh": "meshes/characters/lloyd/meshes/body.glb"},
    {"mesh": "meshes/characters/lloyd/meshes/outline.glb", "resource": 1}
  ]
}
```

Each part inherits only missing settings. A setting on the part replaces the
whole shared value; `[]` explicitly clears it. There is no inheritance between
files. Geometry, materials, placement and layer-specific behavior remain on the
part. Keep part order: animation and gameplay selectors index this array. Main
body and outline geometry use those names; additional parts whose role is unknown
retain `part-N`. Local field geometry keeps its model's descriptive filename.

Established joints use names such as `head`, `neck`, `shoulder-left` and
`finger-00-01-right`. Unknown labels remain explicit limitations; numbered helper
prefixes can still distinguish separate joints. A name is unique within its rig.

Display meshes and their animation GLBs retain Z-up local coordinates. Keep the
same local basis when editing geometry, rest poses, attachments and motion curves.
This is an authoring convention, not a new source-platform identifier. Collision
and camera GLBs already use their own Y-up convention. A full display-model basis
conversion is not implemented; moving only a model's root changes its relation to
animation helpers and gameplay attachments. Future importers must produce the
same declared conventions.

Joint names live in the mesh GLB, with no duplicate list in model JSON. When
editing a model, preserve each gameplay node's `resonance_bone` custom property
and enable custom-property export. These indices connect scripts, attachments
and visibility to nodes, including rigid parts without skins. Node order may
change. Update clips and named attachments if you rename a joint. Ordinary
animation-only GLBs do not need these custom properties.

Combat uses the same GLB hierarchy and rest pose as rendering. There are no
separate `rig.json` files or JSON skeleton copies to update. An optional
`attachment_nodes` map gives gameplay slots readable joint targets, for example
`{"0":"kk00","1":"kk01"}` on Lloyd's body. Component model documents contain
their `layers` directly. The uncommon `independent_layers` list selects resource
slots with their own rig and animation clock; ordinary outline layers share the
primary layer's clock. Edit the corresponding GLB to change either rig.

Equipment lives under readable item names such as `meshes/equipment/wooden-blade/`.
Its `equipment.json` declares `kind: "weapon"` or `kind: "shield"` and contains
the part models directly in `parts`. Meshes, textures and animations sit beside
that document; there are no part directories or separate part JSON files.
Part numbers are attachment slots, and layer numbers distinguish rendered layers.
Each part uses the same `layers`, optional `attachment_nodes` and
`independent_layers` described above. Blade trails and texture animation settings
retain their matching part keys. Temporary scene weapons use the same inline parts.
An explicit `null` in `meshes/equipment/mappings.json` means the item has no visual
attachment. A missing ID is an error. A mod can change either kind of mapping;
nonvisual items do not need empty directories or placeholder documents.

Each `battle/enemies/<name>/enemy.json` contains its `rules` and optional `artwork`
sections directly. There are no separate enemy rules or artwork wrapper files.
The artwork's `model` references the shared monster preview for the main body.
Its `attachments` contain part models directly. A `weapon_variants` entry holds
its inline `model` plus any trail and animation settings; `secondary_body.model`
is inline too. Local variant media and attachment animations sit beside the
enemy document. Parts that share the monster preview refer directly to its
existing files. There are no one-use component JSON files or component folders.
Empty choices are omitted. Gameplay rules and the containing JSON document must
be valid. Malformed optional artwork or missing visual resources produce a
diagnostic and are skipped in normal mode; paranoid mode stops on those errors.
Omitting `artwork` deliberately creates an enemy without body artwork.
Edit an inline layer to change only that component;
edit a referenced shared GLB to change every use of that GLB.

Model layers omit the ordinary defaults `animation:null`, `attached_to:null` and
`additive:false`. An absent animation uses the bind pose, an absent attachment
shares the root transform, and additive blending is off unless enabled.
Equipment and enemy artwork rely on the root layout version instead of repeating
local version fields.

`meshes/equipment/blue-shield-prop/equipment.json` shares the Blue Shield's GLBs
and KTX2 files directly. Its `kind` remains `weapon` for the carried-prop use;
`blue-shield/equipment.json` retains `shield`. Editing their shared files changes
both appearances. A local override can point either declaration at a separate file.

Weapon `uv_animation` entries contain only active controls. `primary` applies
to layers using the main rig; `independent` applies to independently animated
layers. A scrolling texture uses `{"kind":"scroll","step_milli_uv":[0,-12]}`:
the two values move U and V by thousandths of a texture repeat per simulation
tick. An optional `offset_milli_uv` sets its starting position. An atlas uses
`{"kind":"atlas","rows":4,"ticks_per_frame":5}` to cycle through four rows,
holding each for five ticks. Add `frames:[0,1,2,3,2,1]` for a custom row sequence.
`texture` selects an image in the layer's texture list and defaults to zero.
The optional `effect_every_ticks` controls periodic attachment effects. Disabled
channels and inactive effect clocks are absent. These are small procedural
appearance controls; skeletal animation remains in ordinary GLB files.
Offsets must lie strictly within one repeat; steps can range from -1000 to 1000.

Each `trails` entry names two or three joints in its part's primary GLB, in order
across the ribbon, and selects a material from `battle/trails.json`:

```json
"trails": {
  "0": {"joints": ["blade-base", "blade-tip"], "material": "blue-tapered"}
}
```

Use the actual joint names in your model; there is no required naming convention.
Repeated joints are allowed and order matters. Removing the entry disables that
part's trail. The shared material palette lets one edit change all users of a
style. For a local appearance, replace the name with an inline object, such as
`"material":{"color":[0,1,0],"additive":false}`. New styles and custom imports
do not require an original-game preset.

Materials use linear RGB `color` multipliers, where 1.0 is full intensity, and a
pixel `rectangle:[left,top,width,height]`. Additive blending and palette zero are
defaults. Omitting `texture` produces a solid-color trail. `texture.source` selects
the common effect artwork or the current enemy's artwork, with optional `slot`
(default zero). Enemy styles named after a representative character still use
the current enemy's textures. Missing joints, styles or images omit the optional
trail with a diagnostic; paranoid mode reports an error.

Figurines that display an existing character reference that character's GLBs
and textures directly. For example, Noishe's figurine uses the files under
`meshes/characters/noishe/`. Editing those files changes both the character and
the figurine. The figurine's `model.json` retains its preview pose, scale,
visibility and appearance settings. For a figurine-only mesh or texture edit,
copy the file and change that figurine's reference to the copy.

Model layers put `mesh`, `textures`, `materials`, motion choices and attachment
settings together; there is no nested `scene` object. A preview model needs only
its `parts` list. Scale defaults to 1, elevation to 0, hidden geometry to an empty
list and scripted behavior to absent. Model documents use the library's format
version. Empty texture/material lists and resource slot zero are omitted.

Geometry visibility follows the GLB hierarchy. A mesh belongs to its nearest
parent joint with a `resonance_bone` custom property, or to itself if the mesh
node has that property. Ordinary editor grouping nodes are allowed between the
mesh and joint. Moving the mesh to another joint changes which geometry a script
toggle hides; the joint's skeletal children keep animating. Unparented meshes
have no joint visibility binding. Several instances of one mesh combine their
joint bindings. No parallel `material_nodes` list is authored in JSON.

The `hidden_geometry` list uses those GLB joint names. Rename a joint and its
explicit named references together. Keep its `resonance_bone` property when
reordering nodes: existing scripts use that gameplay index. The reader prepares
the names and visibility bindings from the requested mesh once, with no library
scan or media identity check.

Each model part's `materials` list assigns a surface recipe to GLB mesh indices.
For example, `{"meshes":[0,2,3],"color":{"texture":1}}` uses the same surface
on three meshes. Edit that recipe once to change all three. Every mesh must have
exactly one assignment; move a mesh index to a new entry to give it a different
material. Texture indices select the part's `textures` list.

Materials default to vertex color enabled, opaque blending, depth writes and
back-face culling. Texture samplers default to linear filtering and repeating UVs.
Set `blend`, `vertex_color`, `depth_write`, `cull`, `wrap_u`, `wrap_v`,
`nearest_min` or `nearest_mag` only when different. An optional `draw_order` array
gives one priority per listed mesh; lower numbers draw first. When omitted, each
mesh index is its priority. Preserve an existing priority array when repainting.
The less common `multiply` and `image:"capture"` settings select a second texture
and the captured scene respectively.

Body and outline layers share identical palette images within their model's
texture directory. Editing one file changes every layer referring to it. To
change one layer independently, copy the texture and update that layer's path.
Empty appearance channels and node bindings, zero translations, disabled autoplay
and empty animation lists are omitted. Their defaults require no JSON entries.
Dynamic geometry uses a `draws` list to assign each material once:

```json
{
  "mesh": "meshes/characters/example/meshes/part-0.glb",
  "textures": ["meshes/characters/example/textures/part-0-texture-0.ktx2"],
  "draws": [
    {
      "meshes": [0, 2, 3],
      "recipe": [{"multiply_texture": 0}],
      "textures": [{"image": 0}]
    },
    {"meshes": [1]}
  ]
}
```

The first group multiplies vertex colors by texture slot 0. The second uses
vertex colors alone. Edit the group once to change all its meshes, or move a
mesh to its own group. `draw_order` defaults to the corresponding GLB mesh index;
an explicit array preserves exceptional priorities. A `node` names a GLB joint
controlling the draw's visibility. Names and index counts belong to the GLB and
are not repeated in the JSON. An empty draw uses `null` in `meshes` and an explicit
order; it has no geometry to render.

A `recipe` is an ordered list of color operations: `multiply_texture`,
`replace_texture`, `decal_texture`, `blend_texture`, `add_texture`,
`subtract_texture`, `default_view_texture`, or the string `multiply_vertex_color`.
Texture operations take a stage number. Omitted or empty recipes use vertex
colors; `"recipe":"inherit"` explicitly requires prior caller material state.
Inherited and complex operations retain their meaning even when the current
renderer cannot play them. There is no numeric GPU stage count to keep in sync.
A draw's texture bindings select `image` slots, with `stage` defaulting to zero.
Optional wrapping and filtering override the usual repeating UVs and linear
filters. Caller palette names, when needed, are symbols in `table`, not paths.

Geometry with its own palette lists it directly in `textures`. A simple slot is
a KTX2 path; a slot with variants or sampler overrides uses
`{"images":["first.ktx2","second.ktx2"],"sampler":{...}}`.
A `null` entry preserves an unused slot. Omit `textures` for an empty local
palette. Geometry receiving images from its caller uses `"textures":"caller"`.

An image bank selected by its own resource ID uses the same array notation.
A single clamp/linear image can be referenced directly as a KTX2 path. Image-bank
sampling defaults to clamped UVs, linear filtering and zero LOD settings. Share
other sampling settings once with a bank object:

```json
{"sampler":{"wrap":["repeat","repeat"]},"entries":["effects/ring.ktx2","effects/glow.ktx2"]}
```

An entry's `sampler` replaces the whole shared sampler; `{}` explicitly selects
the clamp/linear defaults. Model draw samplers retain their own repeat default.
KTX2 owns compression, pixels, dimensions and mip levels. Source texture-table
names and importer alpha estimates are absent from the authoring data.

Location captions live under `ui/location-titles/`. For example, `iselia.json`
contains four image paths: the shared frame line, shared ornaments, the subtitle,
and the location name. `null` means no image in that slot. Luin's Rebirth,
Devastation and Hope captions share the location name while keeping separate
subtitles. Field `overlays` maps native resource IDs directly to these banks.

Ending artwork lives in `ui/ending/`, emotion bubbles in `ui/emotions/`, and
controller images in `ui/controls/gamecube/`, selected by `ui/controls/gamecube.json`.
The GameCube name describes the pictured controller. Future importers can publish
other controller art using the same bank format. The separately animated letters
in `ui/ending/fin.json` retain their individual slots. Shared light and ring images
live under `effects/overlays/`. Global resource mappings and field bindings refer
to the same artwork. Edit the image to change its pixels or size; edit a bank path
to substitute a different image. Overlay banks use only the base palette.

Party artwork lives directly in `battle/party/<character>/<costume>/party.json`.
Outfits use their title names, such as `lloyd/nobleman`, `colette/fair-lady` and
`kratos/judgment`. The character's `mappings.json` maps machine costume IDs to
these names. IDs selecting identical appearances share one entry; editing that
entry changes every ID mapped to it. A mod can give an ID a separate copy by
changing its mapping. Colette's `angel` entry keeps its distinct motions;
Presea's `initial` entry keeps her initial story appearance. `alternate` remains
a neutral label for Sheena's and Kratos's unidentified extra appearances.

A costume's `parts` contain mesh and material bindings. Its `animations` section
contains groups such as `{"parts":[0,1],"clips":[...]}`: the body and outline
share one clip list, and an independent part can use another group. A part can
belong to at most one group. Change a clip's `motion` or cloth-pose joint list
once to affect every targeted part; there is no inheritance or external binding
file. The resulting motion curves are still ordinary editable GLBs. Known battle
roles use filenames such as `idle.glb`, `walk.glb` and `guard.glb`; unconfirmed
roles retain neutral numbered names. The `slots` list binds the game's requests.

Shared meshes remain under `meshes/characters/`, with the same outfit names.
Colette's reusable wings keep one shared model document. Clips shared by several
costumes live under `battle/party/<character>/animations/`; clips specific to one
costume remain with that costume. Party animation folders have flat filenames.
Already shared field and bank clips retain their existing owners. There is no
separate single-use party model wrapper, source ownership data or identity scan.

Each character has one `victory.json`. A party entry selects it explicitly, for
example `"victory":{"file":"battle/party/lloyd/victory.json","costume":"default"}`.
The selected `costumes` entry maps the five gameplay selectors to `motion` paths
and named local `timeline` references. The `timelines` section defines each
choreography once, keeping `events` in tick order. Editing a shared timeline
changes every costume that references it; to make a costume-specific change,
add a named timeline and change that costume's reference. Distinct motion rigs
retain their separate GLBs. Pose `start`, `blend` and `repeat` default to zero,
zero and false; absent `end` uses the clip's end. `rate` stays explicit, including
zero. An unsupported timeline retains its diagnostic reason.

Hair and cloth dynamics live in a model part's `secondary_motion` array:

```json
"secondary_motion": [
  {
    "name": "hair",
    "joints": ["hair-root", "hair-middle", "hair-tip"],
    "gravity": 0.4,
    "damping": 0.7,
    "attraction": 0.03
  }
]
```

The example joint names must be replaced with names from the model's GLB.
The last joint is the terminal guide. A single gravity or damping value applies
to every joint; an array supplies one value per joint in the same order. Damping
is signed velocity feedback, so a negative value reverses the previous motion.
`attraction` defaults to zero. In fields and model previews, a clip selecting this
chain uses `pose_attraction` instead, which defaults to 0.2. It can be lower or
higher than normal attraction. Battle dynamics use `attraction` throughout.
`preserve_rotation`, `lock_rotation_x` and `lock_rotation_y` default to false.
An optional `collision_plane` names its GLB `anchor` and `normal`, with `offset`
defaulting to zero and `strength` to one. Chain order is explicit and preserved.

A chain label is independent of its GLB joint names. Renaming a joint requires
updating its references in the model and animation files; renaming a chain
requires updating its `pose_chains` selections. Unresolved imported chain roles
retain explicit `unidentified` labels. They remain editable like other chains.

Shared field motions live under `animations/characters/<name>/` when their
character is known. `animations/mappings.json` translates the requested bank ID
to that readable path. A bank's `.json` definition sits beside the `.glb`; there
is no directory for each motion and no copy for each field, costume or model
part. A genuinely field-local motion stays in that field's `animations/` folder.

Each definition entry applies one `motion` to named `actors` and model `parts`.
Actor names are the unique names in `meshes/mappings.json`, including aliases.
Part indices select the model's ordered `parts` list. The usual `resource_slot`
is 12 and is omitted. Optional `pose_chains` selects named hair/cloth chains
whose pose-attraction setting applies to this performance.

Models bind the shared GLB's curves by joint name, independently of node order.
Missing channels retain each model's own rest values. A `joints` object lists
only exceptions: `"hips": "mo6-hips"` routes that clip joint to the model's hips;
`"cape": null` leaves that model joint unanimated. An absent entry uses the same
name in both files. These are ordinary model bindings; curves, timing and affine
transform helpers remain in the standard GLB. No original-format data is needed.

Editing one GLB updates all its users. To change one actor independently, copy
the file under a new readable name and change that actor's `motion` reference.
Split a definition row if it selects several actors. Preserve its part and
secondary-motion selections. A field's `local_animations` can override its own
selection without copying all the animation data.

For example, bank 65558 maps to `characters/lloyd/motion-e221`, with the motion at
`animations/characters/lloyd/motion-e221.glb` and its targets in the adjacent
`.json`. The action behind `e221` is not established, so its numbered label stays
explicit. Known actions such as `idle`, `walk`, `run` and `open-door` keep those
names. `unassigned/` holds files whose owner is still unknown.

- **Textures:** replace the referenced KTX2 file. It contains its pixels, color
  space and mip levels together. Preserve authored mip levels when editing;
  material sampling settings remain with the material. No per-image JSON or loose
  PNG mip files are required.
- **Geometry:** meshes use GLB. Preserve the material and skeleton bindings in
  the owning model when replacing geometry.
- **Animation:** clips use standard glTF 2.0 animation in `.glb`, editable with
  Blender. Each file contains one action and named joints; established reference rigs
  make character clips convenient to edit. Meshes and textures remain separate. Shared clips have direct references from their consumers. See
  [animation authoring](motion-format.md) for the tested import/export settings.
- **Music:** mappings select a named mono or stereo PCM16 WAV at 8–192 kHz,
  up to one hour. The WAV's optional standard `smpl` chunk carries one endless
  forward loop. Start and end count sample frames from zero; the stored end is
  inclusive. A stereo frame includes both channels. Playback begins at frame
  zero, then repeats the loop; a WAV without a loop plays once. Use an editor
  that preserves sampler loops, and update them when changing rate or timing.
  Resonance adds no crossfade. Songs have no JSON sidecars or per-song folders.
  `placeholder.wav` is shared by nine original IDs; `credits.wav` contains the
  actual ending recording; title music shares `tales-of-symphonia.wav` with song 1.
- **Effects:** mappings point directly to a WAV or a compact JSON recipe. Use
  WAVs for ordinary sounds. Recipes preserve programmed pitch, timing, release,
  loops and auxiliary sends. Character voice recipes and recordings live together
  in `audio/speech/battle/<speaker>/`; other recipes reference `audio/instruments/`.
  `audio/instruments/` contains shared waveforms used by playable recipes, such
  as `menu-confirm.wav` and `ring-fire.wav`. Original storage banks do not create
  directories. Unidentified sounds retain numbered `cue-NNNN` or `sample-NNNNN`
  names until their meaning is established.
  Editing a shared waveform changes every recipe that uses it. Source-only
  program dumps and unused instruments stay outside this library in importer
  diagnostics. Shared synthesis tables appear once in `audio/synthesis.json`.
  A WAV needing
  explicit loop points can use `{"kind":"wave",
  "audio":"audio/effects/my-cue.wav","loop":{"start_frame":0,"end_frame":32000}}`.
- **Movies:** use Matroska with RGB8 FFV1 version 3 video and stereo FLAC audio at 8–96 kHz.
  The current limits are 1920×1080, frame durations of 10–100 ms, 100,000 video
  frames and one hour. Mappings point directly to the movie; playback uses its
  first audio track.

For example, this FFmpeg command exports a replacement movie using the supported
video profile and preserves its audio tracks. Keep the input dimensions, frame
rate and audio rates within the limits above:

```sh
ffmpeg -i replacement.mp4 -map 0:v:0 -map 0:a -c:v ffv1 -level:v 3 \
  -coder 1 -g 3 -slices 4 -slicecrc 1 -pix_fmt bgr0 \
  -c:a flac -sample_fmt s16 -ac 2 replacement.mkv
```

## Texture-coordinate motion

Scene and preview `texture_animations` select a texture and a `sequence`. A
sequence is one motion or an ordered list of motions and individual `[u, v]`
keyframes. Rates and durations use that entry's selected animation clock.
One UV unit is the full width or height of the texture. For example:

```json
{"texture":8,"sequence":{"kind":"scroll","ticks":1000,"step":[0.001,0.001]}}
```

This advances both texture coordinates by 0.001 per tick, then loops after 1,000
ticks. `origin` optionally sets the first offset. A `cycle` instead describes
whole texture turns, wrapping the phase within each texture:
`{"kind":"cycle","ticks":500,"turns":[2,-5]}` moves two turns horizontally and
five in the reverse vertical direction during 500 ticks.

An atlas uses row-major grid cells:

```json
{"texture":9,"sequence":{"kind":"atlas","grid":[1,8],"interval":20}}
```

This displays eight vertically arranged cells for twenty ticks each. `interval`
defaults to one tick, `first` to cell zero, and `frames` to the whole grid. Set
`frames` or `first` for a shorter sequence or another starting cell. Row offsets
can extend beyond the grid when the material's texture wrapping calls for it.

Use `{"kind":"hold","ticks":20,"offset":[0,0.5]}` to hold an offset; an omitted
offset means `[0,0]`. An individual `[u,v]` pair lasts one tick. `delay_ticks`
holds the first value before playback starts. `loop_from` selects the tick at
which the repeating section starts; both settings default to zero. Selecting
the last tick holds the final value. For an intro followed by a repeating scroll:

```json
{"texture":0,"loop_from":20,"sequence":[
  {"kind":"hold","ticks":20},
  {"kind":"scroll","ticks":100,"step":[0.01,0]}
]}
```

Field entries select their actor, texture and clock outside the motion. Their
timeline is `"motion":{"kind":"timeline","sequence":...}` with optional
delay and loop settings beside `sequence`. Cinematic UV entries put `clock`,
`texture` and `sequence` together. A sequence may contain at most 36,000 ticks.

## Battle visual effects

Each effect bank has one `effects.json`. Its `programs` contain timed operations,
`templates` holds named particle definitions, and `artwork` contains
texture bindings and inline model declarations. Empty sections may be omitted.
Shared battle effects live under `battle/effects/`; a scene, enemy or arena can
also own an effect bank. Follow that owner's `effects` reference.

Edit the schedule, template and model settings in this one file. For example,
change an event's `at` value to change its emission time, or a particle template's
`lifetime` to change how long it lasts. These clocks use battle ticks.
Templates are named entries, for example `"templates":{"additive-shell":{...}}`.
Each birth refers directly to that name. Rename a template and its references
together; adding or reordering definitions does not change other births. Unused
mod prototypes remain available. The common bank's `stun-bird` definition is
also requested directly by gameplay, so keep that name.

Program IDs still select positions in the `programs` array. An empty program is
an intentional no-op and retains its ID. Shakes, notices and stage tints contain
their settings directly in the timeline; they need no separate templates.

One instruction can repeat without copying its settings:

```json
{"at":10,"repeat":20,"every":2,"kind":"spawn","particle":"additive-shell"}
```

This emits `additive-shell` twenty times, starting at tick 10 and waiting two
ticks between emissions. Omitted `at` means zero, omitted `repeat` means once,
and omitted `every` means the same tick (a burst). A repeated sequence uses
`events`, whose times are relative to the start of each repetition:

```json
{"repeat":5,"every":10,"events":[
  {"kind":"spawn","particle":"additive-shell"},
  {"at":3,"kind":"spawn","particle":"alpha-billboard-quad"}
]}
```

Instructions execute by time. At equal times, their expanded document order is
preserved. Group members can themselves repeat an operation. Changing the count
or interval changes that one sequence; it does not require editing a table of
individual emission times. Each program may expand to at most 65,536 events.

Ordinary particle settings are implicit: motion vectors, fade rates and counts
start at zero; optional controls are absent; flags are false except that depth
testing is enabled. Colors, geometry, texture selection, orientation and blend
mode remain explicit. `blend` uses `"alpha"`, `"additive"` or `"subtractive"`;
timeline and birth overrides use the same names. Add a setting to change it, for example
`"velocity": [0, 2, 0]`, `"fade": [0, 0, 0, 8]` or `"depth_test": false`.
An absent lifetime means the particle has no lifetime limit. These defaults are
independent of the game's original storage format.

Birth overrides distinguish an omitted setting from an explicit zero. For
example, `"birth":{"velocity":[null,0,null],"lifetime":30}` sets vertical
velocity to zero and lifetime to 30 ticks while keeping the other template
values. A fixed value is just a number. A random choice uses a range such as
`{"min":1,"max":5,"step":2}`, selecting 1, 3 or 5. `random_draws` takes that
many fresh shared samples for each birth. A range's `draw` selects one of those
samples starting at 0; using the same number in two settings correlates them.
Negative references reuse earlier samples: `-1` is the last sample before this
birth, `-2` is the one before that. These local references stay unchanged when a
sequence repeats. Keep `reverse` to reverse a range while retaining its shared
sample. A range without `draw` varies independently. A missing model-animation
rate means normal speed; an explicit zero holds the selected frame.

Delayed motion changes and model-parent bindings use `previous_spawn`: 1 selects
the most recent particle birth, 2 selects the preceding birth, and so on. These
references follow birth order, including repeated emissions; sounds and other
feedback do not count as births.

`artwork.textures` names established scopes: `actor`, `actor-secondary`, `scene`
and `arena`. An atlas binding uses the same name, for example
`"texture": {"kind":"atlas", "slot":"scene"}`. The shared banks retain
`bank-0`, `bank-1`, `bank-10`, `bank-11` and `bank-12`: existing consumers do not
establish a unique descriptive purpose for each. Model slots and palette choices
remain local selectors; equal numbers across owners do not identify equal art.

`artwork.models` maps model slots to their local declarations. Meshes and clips
are ordinary GLBs in `models/`, and atlas images are KTX2 files in `textures/`.
There is no per-model JSON file or per-model media directory. Shared monster
geometry can still be referenced directly from its named monster owner.
Atlas pages with identical contents share one image within their effect owner;
the resident common and technique banks also share their atlas files. Each
texture lists distinct image paths in `images`; dimensions come from the KTX2
headers, so replacing an atlas with a different size needs no JSON size edit.
Without `palette_sets`, selectors follow image
order: selector 0 uses the first image, selector 1 the second, and so on.
Exceptional sets are short arrays, for example `"palette_sets": [[0, null, 1]]`.
Here selector 1 is unavailable; selectors past the end are also unavailable.
A number means every selector uses that image: `"palette_sets": [0]` describes
one set that always uses image 0. An empty set `[]` has no available choices.
Keep set positions because particle bindings select them by index. To separate
two choices that share an image, add another image and change the desired entry.
Repeated model images share one file; copy an image and change a reference if
one use should become independent. Clip aliases share their original file while
different variants keep distinct filenames.

A mod can replace only `effects.json`, one GLB or one KTX2. Normal mode reports a
broken optional model and keeps healthy models; paranoid mode rejects that
error. Invalid JSON in the containing document remains an error.

A stored battle scene has one `scene.json` containing its duration, action tracks,
projectiles, camera and optional spell settings, alongside its `effects` reference.
Empty collections and absent options are omitted. There is no separate rules file
or local version. Scene cut-in `images` also list KTX2 paths and take their sizes
from those images. Scene and skill ID mappings remain independent.

## Programmed effects

An ordinary effect is directly under `audio/effects/`, for example `navigate.json`
or `door.json`. `audio/menu.json` selects the same navigate, confirm, cancel and
error resources used elsewhere and supplies the menu's reverb environment.
Replacing one of those shared recipes changes all its uses. To make a menu-only
variant, copy it under a new name and change just that menu reference.

A recipe contains `programs`, shared `samples`, and initial `voices`. For example,
`"voices": [{"group":1,"notes":[{"program":88}]}]` starts program 88. The group
shares voice-allocation limits across uses; it is editable behavior. A note's
ordinary defaults are key 60, velocity 127, pan 64, priority 10 and maximum voices
255. Set an explicit value to change it. Channel defaults to zero.

A program value can be its instruction array or a direct reference, for example
`"programs":{"660":"audio/programs/ring-prepare.json"}`. A shared program file
contains `instructions` and their `samples`. Programs used only by one cue remain
inline; repeated programs have one named owner. There is no catalogue to maintain
and shared files cannot include other files. The cue keeps its own voice groups,
notes, pan, pitch and priority.

Edit a shared program to change every referencing cue. For a cue-specific edit,
copy that program under a new filename and change just the cue's reference.
Sample IDs used by several selected programs must have matching bindings; a
conflicting path, root pitch or loop setting is an error. Use a distinct sample
ID in the copied program when its binding should differ from another selected
program. `unassigned-program-NNNN` retains an unidentified program; its number
is a program ID, not a cue ID. Known uses have names such as `ring-prepare`,
`recovery` and `silent-placeholder`. Cue names ending in `variant-NNNN` share
that established program role but keep their own note and allocation settings.
`silent-placeholder-NNNN` cues also keep their programmed lifetime and allocation
behavior. A remaining `cue-NNNN` or `sample-NNNNN.wav` has no established sound
identity. Instrument WAVs are the audio fragments used by these programs; pitch,
loop and silence settings belong beside each program's sample binding, with no
per-instrument JSON sidecar.

Instructions retain their order, random choices, loop targets and release rules.
Ordinary false flags, absent curves/wait durations and zero cents/offsets are
omitted. For example, `{"operation":"wait","sample_end":true}` waits for the
sample to end; `{"operation":"wait","milliseconds":100}` waits 100 ms. A
missing duration does not mean zero. Effect definitions use the root layout
version instead of repeating a version in every recipe.

The compact instructions retain their existing integer arithmetic. Their units
are:

| Setting | Meaning |
| --- | --- |
| `milliseconds`, `wait_ms`, `period_ms`, envelope `attack_ms` / `decay_ms` / `release_ms` | Milliseconds. |
| `beat_wait.ticks`, recipe `duration_ticks` | 384 ticks per quarter note. Effects start at 120 BPM; a beat wait samples the tempo when issued. |
| `set_volume.factor` | Divide by 127; an offset uses volume levels 0–127. |
| `fade_volume.factor` | Divide by 128. |
| `scale_volume.factor` | Divide by 4096, retaining integer rounding: `(volume >> 5) * factor >> 7`. These factors are not percentages. |
| `select_control.scale` | Signed 16.16 gain: multiply the signed source control, shift right 16, clamp to −8192…8191, then add 8192. |
| `vibrato.depth_8`, `pitch_envelope.depth_8` | Divide by 256 for semitones. Other pitch fields explicitly use semitones, cents or Hz. |
| `pitch_sweep.period` | Number of 16 ms steps. Duration is `period * 16` ms; total change is `step_hz * period` Hz. Zero disables the sweep. |
| Ordinary envelope `sustain` | Linear level 0–32767. |
| DLS envelope `sustain` | Logarithmic level 0 (silent) through 193 (full), resolved through the attenuation table; not linear gain. |
| DLS `attack_timecents`, `decay_timecents`, velocity/key scales | Signed 16.16 timecents. Apply note-dependent scaling, divide by 65536, then duration is `2^(timecents / 1200)` seconds. A time of −2147483648 means immediate; that scale value means no adjustment. |
| `set_age.value`, `add_age.value` | Voice-allocation preference 0–65535 and a signed change to it, not elapsed time. `age_period.milliseconds` is its ramp to zero; a new voice starts at 60000 over 7500 ms. |
| `random_branch.minimum` | Branch when a random byte is at least this value: probability `(256 - minimum) / 256`. |

Program, instruction and loop targets select actual code entries. Keep their
references consistent when editing control flow; numeric targets are not media
filenames.

## Battle arenas

An arena lives in a directory such as `battle/arenas/iselia-classroom/`.
`arena.json` contains an ordered `layers` list, and the GLBs and model textures
sit beside it. Each layer has a readable `name` and an inline `model` using the
same material and GLB conventions as other models. There is no layer JSON file.
`battle/arenas/mappings.json` connects arena IDs to these directories.

The first layer supplies the attachment skeleton. Other layers can set `joint`
to a joint name from that GLB, for example `"joint": "tree-anchor"`. Rename the
joint in its model and animation GLBs and update this reference together; node
indices are not authoring data.
Layer names are labels and may be changed freely. An optional `foreground: true`
draws a layer after the combatants. Foreground layers follow their list order;
adding scenery does not silently turn it into a foreground layer.

Animated layers default to half a clip frame per battle update; `animation_rate`
overrides that speed. Per-layer `texture_animations` holds scrolling or atlas
clocks. Set `minimum_alpha` (0–255) to fade a layer when it obscures the battle's
camera focus. The arena's `fade_step` defaults to 24 opacity units per update.
Omit `minimum_alpha` for a layer that should not fade.

`light_position`, optional fog, scene placement and actor lighting belong to the
arena. Neutral RGB tint is 64 and opaque alpha is 255; neutral colors, zero
translation/yaw and other ordinary settings can be omitted. Arena effect programs
and their image definitions live once in `effects.json`. Identical scenery meshes
and model images share one editable file; placements and material choices stay
with each layer. Point a layer at a new mesh or image to make that edit independent.
Seven unidentified arena locations retain explicit
fallback names; numbered location variants are distinct settings, not duplicates.

## Battle voices

Known speakers use their names, never native archive labels such as `btl-01`.
The importer translates source IDs into these names; the cooked mappings connect
runtime IDs directly to the resulting paths.

Single-character recordings use the speaker's folder, for example
`audio/speech/battle/lloyd/unison-call.wav` and
`audio/speech/battle/colette/photon-blast.wav`. Combined victory recordings live
under `audio/speech/battle/victory/`, with their participants and situation in the
filename. Each recording stays in one file. Unidentified event lines retain
explicit `events/voice-NNNN.wav` names until their speaker is established.

A character's battle recordings have names such as
`audio/speech/battle/lloyd/hurt-01.wav`, `lloyd/guard.wav`,
`lloyd/demon-fang.wav`, and `colette/victory-01.wav`.
A recipe beside a WAV retains required pitch, envelope, loop or routing behavior.
The same recording can be referenced by several recipes.

`audio/mappings.json` translates sound cue IDs to paths relative to `audio/`.
It includes both ordinary effects and character voice recipes. Stream voice IDs
remain in `audio/speech/mappings.json`, relative to `audio/speech/`.
These are two existing ID namespaces, not two copies of the recordings.

An `-alternate.wav` suffix identifies a different source encoding of a line;
its decoded samples differ, so editing it does not change the other version.
Confirmed identical stream recordings share one file. Numbered `voice-NNNN`
entries retain an unidentified purpose; `unassigned/` means the speaker has not
been established. These are explicit gaps in the importer naming data.

## Event dialogue

Event recordings live under `audio/speech/events/<scene>/<speaker>-NNN.wav`.
For example, the opening classroom dialogue is in `iselia-classroom/`, starting
with `raine-001.wav`. Colette's letter is in `colettes-farewell-letter/`.
These are descriptive scene names, independent of fields and source archives.
A scene reused in several places still has one set of recordings.

The numbers order the scene's lines and fragments. They do not identify alternate
takes. Unidentified speakers have an explicit `unidentified-NNN.wav` filename.
A recording reused by different speakers can have a name such as
`colette-or-genis-001.wav`; editing it changes both uses. The nine overworld
lines remain shared in `audio/speech/world/`.

Replace a WAV to change its audio. To rename or move it, update its paths in
`audio/speech/mappings.json`, relative to `audio/speech/`. Multiple IDs can point
straight to the same WAV. There are no per-scene or per-recording JSON wrappers.
Edit the associated field's `messages.json` when changing its displayed dialogue.
Silent timing entries have `silent` in their descriptive mapping names and no
WAV file. Their frame counts and sample rates control dialogue timing. Unused
silent placeholders have `events/unassigned/` names without an empty directory.

## Original scenarios and saves

Original `.ssb` scenarios remain bytecode. Edit dialogue through `messages.json`.
For instruction changes, keep a working assembly file outside the library:

```sh
symphonia-script scenario disassemble /path/to/scenario.ssb work/scenario.ssasm
symphonia-script scenario assemble work/scenario.ssasm /path/to/mod/scenario.ssb
```

Use the original relative scenario path in your mod. The selected `.ssb` is
authoritative; assembly validates it before replacing the target. This is
instruction-level editing: changing instruction sizes can require updating the
explicit registry and auxiliary table layout. Authored `.sym` modules are already
readable source and need no manual compilation.

Saves contain a format version and gameplay state. Changing assets, scripts or
rules does not invalidate saves. Loading resolves the saved location and required
resources from the current library, without hashing or scanning unrelated assets.
Unsupported save versions require a new game; no repair or migration is provided.
An obsolete cooked layout requires a fresh cook. `content.json` contains the
cooked format `version` and `game: "tales-of-symphonia"`. It describes the game,
independently of the original release used by the importer; source platform,
product code and revision are not cooked metadata. Changing the version does
not convert files. A future Steam importer will target these same resource
formats; this does not promise compatible mods or saves across different gameplay.

## Dynamic resource IDs

`resources/mappings.json` maps full script resource IDs directly to content.
For example, `{"65536":{"geometry":"meshes/props/statue/scene.json"}}`
selects a geometry document. An entry can instead select a GLB animation,
textures, collision or a script. Model parts stay together under `parts`, with
names such as `body`, `outline` and `floor-collision`; empty parts are omitted.
References inside these typed entries are relative to the library root.

Resource files live in their ordinary categories. Remaining party clips use
`animations/characters/lloyd/unassigned-motion-7.glb`, for example. The actor is
known; an unassigned motion number does not pretend to identify its action.
Unknown owners use `animations/unassigned/resource-ID.glb`,
`meshes/unassigned/resource-ID/` or a flat `textures/unassigned/resource-ID.json`
bank beside its images. These are honest unknown identities, not archive or disc
names. The machine ID mapping supplies the link; directory structure adds no
loading rules.

Dynamic skit image banks use names such as
`skits/portraits/lloyd/angry-images.json` and point to the existing named portrait
artwork. Editing an image changes both uses. Bank-specific sampler choices stay
in the bank: editing those changes its dynamic use, while ordinary skit portrait
playback keeps its own sampling. The order still defines image selectors.

Dynamic NPC models live with their named character or prop. For example,
`meshes/characters/yggdrasill/body.json` and `outline.json` describe dynamic draw
bindings, while `model.json` holds the field model's behavior and materials.
Both use the same files in `meshes/` and `textures/`: editing the shared GLB or
KTX2 changes both uses. These are direct paths, without a second media registry.
Unknown owners are explicitly named `unidentified-model-<id>` until identified.
Small actor-service IDs are a different namespace from dynamic resource IDs;
a matching number alone does not establish a shared character.

Model packages keep `body.json`, `outline.json`, optional collision GLBs and
an `animations/` folder directly under their owner. They need no `parts/body/geometry/`
chain. Motion numbers are retained where their meaning is still unknown.

For example, `{"65537":{"animation":"animations/gesture.glb"}}` selects the
GLB directly. Its joints and animation channels are authoritative; there is no
separate joint list, clip descriptor or per-clip directory. Several resource
entries can refer to one shared clip. A resource with multiple clips uses named
`parts`, each containing its own `animation` reference.

A `null` entry is empty. `{"alias":65536}` selects another ID directly, so edits
to its owner affect all aliases. Alias chains are rejected. `"unavailable"` and
`"unsupported"` retain explicit limitations. Mods replace individual ID entries.

These declarations do not make unsupported dynamic model/animation bindings
playable. Normal field loading reads the catalogue, without decoding unused
physical assets. The importer checks payloads explicitly during publication.

## Party rules

`battle/party/rules.json` keeps character settings together under `characters`.
The ordinary `battle/party/mappings.json` binds gameplay IDs to these names and
their artwork folders. There is no second character-name catalogue. For example,
edit `characters.lloyd.profile.run_speed` for Lloyd's speed, or
`characters.colette.body_collider` for Colette's standing capsule. Offsets and
collider dimensions use world units. Each character also owns their default
strategy, chanting pose sequence, casting voices, neutral contact sound,
Over Limit voice and item-release offset. The profile's facial, casting and
combat settings remain alongside that character.

Strategies use named choices instead of three unexplained numbers. Lloyd's
default is `{"target":"spread","techniques":"at-once","position":"frontlines"}`.
Target choices are `nearest`, `farthest`, `leader`, `spread`, `flying`, `casting`,
`lowest_hp` and `protect`. Technique choices are `at-once`, `keep-reserve`,
`retain`, `save`, `do-not-use`, `moderation`, `heal` and `support`. Position choices
are `frontlines`, `dont-pursue`, `hold-position`, `long-range-skills`,
`long-range-magic` and `skills-and-magic`. The shared `technique_strategies` section
sets TP-reserve and healing-threshold percentages for those technique choices;
it is not a list of character attributes. These names select the existing
gameplay behavior; percentage edits change its thresholds.

Each `casting_voices` section declares its common `fallback` once as a sound and
duration in ticks. Its `default` and `techniques` select chants and releases.
A sequence can override `fallback` with another sound/duration;
`{"sound":null,"ticks":0}` explicitly disables it. An omitted override uses the
caster's shared fallback. Changing that shared fallback affects all its uses,
while explicit overrides stay independent. Technique keys currently use the
battle-scene IDs, which are distinct from menu-technique IDs. Cues and recordings
resolve through the audio mappings; their files have speaker and role names.

`death_voice_pairs` lists `[victim, responding-character]` names. The existing
special Lloyd-affinity selection still applies. `elemental_contact_sounds`
names all eight elements; neutral cues belong to the individual characters.
`lethal_rescue_names` names the five rescue captions by their role. Shared entry
settings and references to the normal/martial attack tables remain in this same
file. Only the nine playable profiles are published; unused source rows and
unused policy columns are importer details.

### Battle profile categories and casting

Profiles use named `species`: `unknown`, `plant`, `undead`, `beast`, `magical`,
`machine`, `human`, `bird`, `insect`, `aquatic`, `immaterial`, `dragon`, or `angel`.
Equipment `species_bonus` uses the same names; omission means no species bonus.

`size` is `small`, `medium` (default), `large`, or `huge`. These reserve respectively
0, 1, 2, or 3 cells around the actor in the recovery-position grid. Each cell spans
100 world units. Large and huge enemies resist Cyclone; huge participants request
wide encounter framing. This does not scale the artwork: use `model_scale` for that.

`recoil_weight` describes hit response rather than physical mass: `normal`
(default) keeps the vertical impulse, `light` and `heavy` use the corresponding
scales in `battle/recoil.json`, and `grounded` prevents launches. Heavy and grounded
actors use a 12-tick falling blend; the other classes use 16 ticks.

Casting settings live in one optional `casting` object. For example:

```json
"casting": { "base_ticks": 80, "release_start": 0, "chant_loop_start": 10 }
```

Only differences from these defaults need to be written:

| Setting | Default and meaning |
| --- | --- |
| `base_ticks` | 120 chanting ticks before technique adjustments or an explicit enemy-action override |
| `chant_once`, `chant_loop_start` | False, frame 0 |
| `recovery_motion` | Absent; no closing gesture |
| `opening_clip`, `release_clip` | `cast-opening` and `cast` |
| `opening_start`, `release_start` | Frames 0 and 1; an explicit release start of **0** is distinct |
| `opening_end`, `release_end` | Absent; use the animation's endpoint |
| `opening_rate`, `release_rate` | 0.5 animation frames per tick; the release rate also controls ordinary chanting |
| `opening_blend`, `release_blend` | 4 ticks |
| `opening_repeat`, `release_repeat` | False |
| `release_loop_start` | Frame 0 |

The existing animation and spell schedules still determine when gestures play;
these fields control their selected clips and playback. Resident spells keep their
four-tick anticipation offset; `release_start` controls stored-scene playback.
Original frame-zero,
repeating and custom-loop gestures retain their own small overrides. There is no
preset file or inheritance between actors.

Other omitted profile defaults are `idle_variation: 30`, `stagger_threshold: 100`,
`stagger_ticks: 60`, `guard_reduction: 75`, `guard_pressure_limit: 10`, `armor: 0`
and no custom defeat motion when `death_motion` is omitted.
Party guard pressure is still calculated from current maximum HP.

### Battle expressions and shadows

Each entry in `expressions` keeps one texture channel's settings together:

```json
"expressions": [
  { "texture": "eyes", "frames": 16, "active": 4, "rescue": 10, "blink": [1, 2] },
  { "texture": "mouth", "frames": 8, "active": 6, "rescue": 5 }
]
```

`eyes` and `mouth` select the primary body's named `appearance` roles, so the same
profile works with each costume's texture palette. An explicit number, such as
`"texture": 2`, selects that zero-based palette slot on each body part. This is
also respected for party characters; there is no hidden replacement of edited
texture selections. Enemy channels retain explicit slots because different
parts may use different images at the same slot, and two separately animated
slots can share one image. Living Doll's slot 2 belongs to a later body part;
Pumpkin Tree's slots 0 and 1 share an image but remain independent channels.

`frames` counts vertically stacked atlas frames. `idle`, `active` and `rescue`
select the resting, engaged and revival frames; each defaults to zero.
`blink` supplies the half-closed and closed frames. Omission disables blinking;
`blink_exclude` defaults to `[2, 3, 10]`, and an explicit empty list allows blinking
on every expression. Each channel can have its own blink artwork and exclusions.
Keep the array order when editing existing characters: action and event expression
commands address these channels in order, with at most four channels. Story-forced
party eyes still address the first channel. Profiles with no expression channels
omit the list.

Omitting `shadow` uses a footprint with scale 1 and RGBA `[16, 16, 16, 176]`.
Use `"shadow": null` to disable it, or an object containing only overrides,
such as `"shadow": { "scale": 0.75, "color": [24, 24, 24, 160] }`.
The setting applies equally to party and enemy actors. The footprint follows
the animated body and shrinks with altitude. Each joint in the model GLB
participates unless its custom property `shadow_bounds` is false. In glTF JSON
this is `"extras":{"shadow_bounds":false}` alongside its other properties.
Rename joints freely without changing this choice. To exclude a swinging hand
or decoration, set that joint's property to false; to include it, set true or
remove the property. Preserve custom properties when exporting from an editor.
The animation clip does not own this setting.

### Battle profile anchors

Enemy `enemy.json` profiles and `battle/party/rules.json` use GLB joint names for
`head_anchor.bone` and `overlimit_bone`. Edit these names when changing a model's
joint labels. The selected costume resolves its own joint names; there are no
hidden party overrides.

The head anchor positions stun and poison effects. Its optional `offset` is in
actor-local coordinates, scaled with the body and rotated with its heading,
independently of the joint's rotation. Targeting uses that same anchor when
`target_anchor` is omitted (or set to `"head"`). Editing the head then moves both
placements. To position the targeting indicator independently, use:

```json
"target_anchor": { "bone": "Bone_sebone01", "offset": [0, 20, 0] }
```

`"target_anchor": "center"` selects the actor's body center instead of a joint.
A named root joint still means the joint, even if it is the first node in the GLB.
The `overlimit_bone` name positions the activation burst; `overlimit_scale`
continues to control its size. These settings do not change animation bindings.

## Normal attacks

Body poses use named `clip` selections such as `idle`, `guard`, `hurt`, `cast`,
`cast-opening`, `knocked-down` and `get-up`. For example,
`{"clip":"cast","start":1}` starts that body's cast animation at frame 1.
The model's clip bindings still connect request IDs to editable GLBs; these
names express the established body roles. An unidentified action uses `slot-30`,
for example. Initial, defeat and casting-recovery motion overrides use the same
names. Omit a defeat override for ordinary defeat behavior. Carried models and
particle animation banks have independent local slots; their numbers do not
claim the body's motion roles.

Poses default to rate 0.5 animation frames per tick, a four-tick blend, start
frame zero, the clip's endpoint and no repeat. Explicit zero rates/blends and
other variants remain valid. Hit definitions require their damage `kind` and
`power`; omitted ordinary settings are:

| Hit setting | Default |
| --- | --- |
| `element` | `inherited` |
| `guard_pressure` | 1 |
| `hitstun` | 20 ticks |
| `cooldown` | 30 ticks |
| `stun_chance` | 1 percent |
| `stagger` | 1 |
| `recoil` | Pattern 1 in `battle/recoil.json` |

Explicit zero values override these defaults. Enemy attacks omit zero `armor`.
Changing one contact or pose keeps other authored variants independent; there is
no shared preset file or inheritance between actors.

`battle/party/normal-attacks.json` contains named characters and their seven
attack roles: `neutral`, `rising`, `thrust`, `low`, `finisher`, `aerial-slash`
and `aerial-thrust`. Its small `mappings` section connects character IDs to these
editable names. All definitions stay in this one file.

Select `characters.lloyd.attacks.neutral`, for example, to change Lloyd's neutral
attack. `duration`, `chain_at`, `recovery` and event timestamps are gameplay
ticks. Each event is `[tick, command]`; order at the same tick is significant.
`reach` is `[maximum, minimum]` in world units. Poses retain clip frame units and
their explicit playback rate; a rate of zero holds the frame.

Combo choices use names, for example:

```json
"continuation": {
  "directions": ["rising", "thrust", "low"],
  "fallback": "finisher"
}
```

Grounded up selects `rising`, down selects `thrust`, and side input selects
`low`. In the air, down selects `aerial-thrust`; other inputs select
`aerial-slash`. `directions` lists allowed successors; `fallback` is the choice
for other input. An omitted continuation ends the normal combo. An omitted
`chain_at` means no chaining timestamp, rather than tick zero.

A contact's `hit` can hold its settings directly or a name in the same
character's `hits` section. Editing `characters.lloyd.hits.neutral` changes all four
contacts that use that recipe (neutral, aerial slash and both finisher hits). Copy the settings into one contact to make it
independent. These references also work for thrown weapons and projectile hit
overrides. An absent projectile hit keeps its existing recipe; a fixed power of
zero remains an explicit override. Ordinary false flags and absent controls
are omitted.

An identical entire track can refer directly to another role, such as Raine's
`"aerial-thrust": "aerial-slash"`. Replace that name with a copied attack object
to edit it independently. References cannot chain or form cycles. The file has
no external include graph or per-character wrappers.

## Shared field settings

`fields/settings.json` owns common field effects, sprite colors, mouth and blink
cycles, contact shadows, the toon-lighting ramp, fluttering particles and save-point
messages. A field's `scene.json` selects it with `"settings": "fields/settings.json"`.
Edit this shared document once to change all fields that use the setting.

`effects.sprites` uses named drawing roles, such as `smoke`, `station-halo`,
`electric-spark` and `lightning-bolt`. Edit each role's texture, UV rectangle and
playback settings directly. These role names are fixed; no numeric sprite lookup
or separate recipe files are needed.

`blink` and `effects.mouth_cycle` use timed atlas frames, for example:

```json
"blink": [
  {"frame": 0, "ticks": 4},
  {"frame": 1, "ticks": 4},
  {"frame": 2, "ticks": 4},
  {"frame": 0, "ticks": 121}
]
```

Timing uses 60 ticks per second and repeats after the last entry. Battle events
inherit this same blink sequence; an event's inline `blink` array overrides it.
Event and actor documents use the library version in `content.json`.

A scene can override `blink`, `contact_shadow`, `toon_ramp`, `particles` or
`save_point` directly. Omission uses the shared value; an override replaces that
complete setting. An explicit `"particles": {}` disables those particles in that
field. A particle recipe may override its `palette`; omission uses the shared
`effects.palette`, so changing that palette updates sprites and fluttering particles together.
Empty `movies` lists are omitted. Door interaction settings live on their model hinges.

Save-point text uses ordinary strings with named color controls, for example:

```json
"no_gem": [
  "You do not have a ", {"color":"cyan"}, "Memory Gem",
  {"color":"white"}, "."
]
```

The shared `save_point` object contains `tutorial`, `unlock` and `no_gem`.
Available colors are white, blue, red, purple, green, cyan and yellow. Line and page
breaks remain `\n` and `\f`. Fields with a save-point model use these shared
messages automatically; a scene's `save_point` object provides a local replacement.
These literal notices do not interpret script expressions.

`effects/field/textures/field-effects.ktx2` is the common atlas used by contact
shadows, station effects and fluttering particles. Their UV settings select the
appropriate regions. There are no per-field copies of that atlas and no separate
version tag inside the settings document. Asset-library versioning remains in
`content.json`.
Other shared atlases have descriptive names: `particles.ktx2`,
`sparks-and-rays.ktx2` and `lightning-and-flames.ktx2`. Field atmosphere also shares
`particles.ktx2`, `dust.ktx2` and `heat-haze.ktx2`; local artwork can use its own path.

## Skit availability and names

`skits/mappings.json` connects game IDs to named skit folders. Most names follow
the displayed title. Untitled overworld conversations use descriptive names such
as `colette-making-friends` and `preseas-flying-charms`; these names do not add an
on-screen title. Identical dialogue-free stubs share `empty` or `empty-padded`,
and the two identical Lloyd portrait demos share `lloyd-portrait-demo`. The two
empty owners preserve their distinct existing binary files, including padding.
Every ID remains independently addressable. To customize one shared ID, copy its
folder and change that ID's mapping in the mod.

`skits/catalogue.json` describes prompts, replay order and resource options. A
prompt can look like this:

```json
{
  "id": 1,
  "title": "What's the Temple Like?",
  "trigger": "entry",
  "story": [1000, 101000],
  "party": ["lloyd", "colette", "genis"],
  "location": "sylvarant"
}
```

`entry` checks when entering a field; `exploration` checks every twenty seconds
of free exploration. The trigger is independent of the ID. If several entries
qualify, the last one in the list wins. `preview_order` is a separate ordered list
for replay. Party requirements mean every listed character must be in the active
formation, not merely recruited. An omitted party list imposes no restriction.
Character references inside conditions use those same names, for example
`{"member":"sheena"}` or `{"range":{"value":{"affinity_rank":"raine"},"max":1}}`.
Affinity rank compares companions, including those outside the formation; Lloyd
is excluded and ties favor the existing character order.

Omitting `story`, `location` or `condition` removes that restriction. Story ranges
are inclusive. Locations can be `any_field`, `overworld`, `sylvarant`,
`tethe_alla`, or a named selector such as `{"field":"iselia-0331"}`. Names resolve
through `fields/mappings.json`. An inclusive field range is written as
`{"fields":{"from":"palmacosta-0054","through":"palmacosta-0055"}}`.
The range follows game field IDs between those endpoints, not alphabetical order.
An absent boundary resource can be written as `{"id":94}`; keeping this boundary
preserves its condition even though no installed field has that ID.

Conditions can read the known saved values `"story_progress"` and `"world_side"`
(zero for Sylvarant, one for Tethe'alla). For example,
`{"range":{"value":"story_progress","min":1000,"max":101000}}`.
The other sixteen state-word indices remain explicit `{"global_word":3}` selectors:
the available evidence does not establish reliable semantic names for them.
An absent resource title displays no title unless the prompt supplies one. Skip
permissions remain independent per ID, including IDs sharing a folder.

Thirteen unbound speech groups contain only silence clocks. Their labels use
`skits/unassigned/silence-0068-001`; they have no skit title, speaker or WAV file.
The 31 unreferenced event clocks similarly use `events/unassigned/silence-001`.
These remain addressable by their existing IDs. Event names use attributable
speakers or explicit descriptive roles, such as `virginia-diary-keeper`; the four
`virginia-diary` entries describe their content without claiming a voice identity.
The bounded speech review leaves 257 recordings explicitly `unidentified`: no
unambiguous speaker is established by their dialogue and actor evidence.

## Skit portraits

`skits/portraits.json` owns portrait images and their eye, mouth and extra-overlay
tracks. `skits/catalogue.json` owns availability, preview order, resource options
and speech selection. The only library version is in `content.json`.

Portrait artwork is grouped by character, for example
`skits/portraits/lloyd/neutral.ktx2`, `neutral-eyes-1.ktx2` and
`neutral-mouth-1.ktx2`. Pose names describe the reviewed artwork; Colette's
`plain-collar-*` variants distinguish their visible clothing without assigning
a story state. Each named entry in `images` lists its base image first, then its
patch images in order. `mappings` connects machine image IDs to those names.
Image and canvas dimensions come from KTX2, so resizing a base image also resizes
the portrait canvas. Keep dimensions and patch positions on the eight-pixel grid.

The `animations` object contains named, shared drawing recipes. Each recipe's
`ids` list states exactly which selectors use it. A recipe name is representative
artwork, not a restriction on which character may use it: images and animation
selectors are independent. Rename a recipe freely; its selector list preserves
its meaning. To customize one selector, move that ID into a copied recipe.
The `still` definition contains selectors with no automatic drawing tracks.

```json
"lloyd/neutral": {
  "ids": [0],
  "eyes": {
    "repeat": true,
    "frames": [
      {"duration_ticks":181,"image":1,"position":[64,128]},
      {"duration_ticks":9,"image":2,"position":[64,128]}
    ]
  },
  "mouth": {
    "repeat": true,
    "frames": [
      {"duration_ticks":9,"image":3,"position":[88,176]},
      {"duration_ticks":9,"image":4,"position":[88,176]}
    ]
  }
}
```

Use `eyes`, `mouth` and `extra` only when a track exists. Each contains 1–255 `frames`
and an optional `repeat` boolean, which defaults to false. `duration_ticks` is
an actual frame duration at 60 ticks per second (1–32767). Without repetition,
the last frame holds. Original eye/mouth phase offsets, talking state and explicit
expression controls still apply; the mouth returns to its resting frame when
not talking.

`image` selects a slot in that portrait's image list. Omit it to leave the current
canvas unchanged for a frame. `image:0` copies from the current canvas, which may
already contain earlier patches. `position` is a pixel offset and defaults to
`[0,0]`. Patch results persist; extra overlays draw first, then eyes, then mouth.
The image files remain editable KTX2. These compact recipes describe controlled
patch drawing; scenario programs remain `.ssb` and are outside this review.

## Field atmosphere and rendering inputs

`fields/settings.json` holds shared `environment` properties for `snow`,
`drifting`, `fire`, `wind`, `heat_haze` and `lights`. A scene enables only the
components it lists. For example, all four Flanoir scenes use:

```json
"environment": {"particles": [{"kind":"snow"}]}
```

Change the shared snow properties to edit all four fields. Add `"size":18` beside
`"kind":"snow"` to change just that emitter. Each local property replaces the
whole shared property; nested objects and arrays are not recursively merged.
`"wind":{}` and `"heat_haze":{}` enable their shared recipes. Omitting a component
disables it, even when shared defaults exist. There is no preset catalogue or
additional file to follow.

Conditions use `when`. Omit it for unconditional effects, or use
`{"setting_nonzero":7}`, `{"actor_exists":{"setting":7}}`, or
`"rendering_enabled"`. An explicit `"always"` overrides a shared condition.
Settings 0–7 are existing live field-rendering inputs. Fixed rendering values
are plain integers; `{"setting":7}` reads a live value, and
`{"setting":7,"offset":1}` adds an integer offset. `"scenery"` names the primary
scenery actor. These forms also apply to texture animation bindings.

Particle coordinates retain field Z-up space. Distances use field world units;
updates run at 60 ticks per second. Sizes are full billboard dimensions.

| Effect | Editable properties |
| --- | --- |
| `drifting` | `interval_ticks`, actual `lifetime_ticks`, `spawn_position`, `spawn_step`, `spawn_counts`, `velocity_per_tick`, two-component `size`, `color` and `sprite`. Each axis independently samples `spawn_position + spawn_step * integer(0..spawn_counts)`. |
| `snow` | `count`, inclusive integer `horizontal_offsets`, `spawn_height` above the player, inclusive integer `fall_speed_per_tick`, `size`, `color`, `fog_color`, `fog_range` and `sprite`. Existing snow offsets are `[-1023,1024]`. |
| `fire` | `position`, `size` and `sprite`. This uses the existing fire effect: two layers every four ticks, lasting 61 and 31 ticks. |
| `wind` | `first`, `step`, `count`. Each axis independently samples `first + step * integer(0..count)`. When its condition becomes false, wind retains its last force. |
| `heat_haze` | `sprite`, `scroll_per_tick`, `rotation_degrees`, `scale` and `alpha`. |
| `lights` | `target` (normally `"scenery"`), `ambient`, `color`, `attenuation` and a `lights` list. Each light specifies `actor`, vertical `height` and `intensity`; it follows that actor's current position. Missing actors clear their lights. |

Colors and alpha use 0–255 components. Sprite texture paths refer directly to
KTX2 images, with UV rectangles selecting the desired artwork.

## Field and actor collision

A field's optional `collision` path names one `collision.glb`. It contains ordinary
mesh objects with `role` custom properties: `ground` for movement and `lighting`
for character-light selection. Actor models use the same optional path with
`floor` and `solid` roles. Models without collision omit the property entirely.
Dynamic resources also point directly to floor/solid GLBs; there is no JSON
geometry wrapper or per-file version.

Edit these files in a glTF-capable mesh editor. Enable custom-property export
in Blender. Object names are yours to change; the `role` determines the meaning.
The optional integer `order` preserves query priority when an editor sorts or
renames objects. Newly added objects without an order follow the existing ones.
Geometry uses standard glTF Y-up coordinates; ordinary object transforms are
applied when reading. Actor geometry remains local to its model. These collision
GLBs convert coordinates at loading; existing display-model conventions are
unchanged.

Ground, floor and solid objects expose these optional boolean properties:

| Property | Meaning when true |
| --- | --- |
| `ignore_characters` | Player and enemy queries skip this geometry. |
| `ignore_enemies` | Enemy queries skip this geometry. |
| `ignore_blocks` | Block collision queries skip this geometry. |
| `no_block_support` | Pushable blocks cannot use this geometry for support. |
| `sliding` | The ground applies the existing sliding behavior. |

Lighting objects use `light` (0–31, default 0) to select the character light.
The remaining `attributes` integer preserves unresolved script-visible bits;
it defaults to zero. Use the named properties for the bits they represent.
Lighting meshes do not accept collision flags. Triangle winding and query order
matter, especially for solid volumes and overlapping floors. Empty field/model
collision declarations and unused empty groups are omitted.


## Scenery doors

Door interaction settings are custom properties on the hinge node in the primary
scenery GLB (`meshes/part-0.glb`). There is no separate door list in `scene.json`.
In Blender, select the hinge and edit these **Custom Properties**:

```json
"door_action": "push",
"door_approach": [-122.5, -55.287, 0],
"door_angle_degrees": -30
```

`door_action` is `push` or `pull`, selecting the character's opening animation.
`door_angle_degrees` is the signed rotation around the hinge's local Z axis.
`door_approach` is the character's standing position relative to the hinge, in
model units. Moving, rotating or scaling the hinge or its parents moves the
interaction with it; translating the scenery also moves the interaction.
Facing follows the hinge's scaled axes, rounded down to whole degrees as before.

Keep the hinge's existing `resonance_bone` property so scripted selections and
opening animation still reach it. Equal node names do not merge hinges. Joint
names may also be used by animation tracks, so keep those bindings consistent
when renaming a model node. Remove all three `door_` properties to remove that
interaction. Export GLB with **Custom Properties** enabled.

Scenery retains its model coordinate convention. These local offsets follow the
model's axes; Blender's glTF axis conversion applies when editing the scene.

## World terrain

Each grid cell has a `terrain.json` beside its meshes, textures and
`collision.glb`, for example
`world/sylvarant/terrain/column-00-row-00/`. A cell with a story-dependent variant
also has `alternate.json` and files prefixed `alternate-`. The continent's
`world.json` selects these paths; neither variant needs a separate model wrapper
or a base/alternate directory tree. Each terrain document owns its `parts`
directly and refers to its collision GLB.

Open `collision.glb` in Blender to edit its geometry. Each mesh object's custom
properties contain `surface` and `order`. Set `surface` to one of `barrier`,
`grassland`, `road`, `wasteland`, `snowfield`, `desert`, `forest`, `deep_forest`,
`beach`, `water`, `bridge` or `pier`. Ordinary land is walkable; `water` supports
ship travel; `barrier` blocks both. A `pier` also permits ship boarding. Barriers
still contribute to flight and camera clearance. The `order` property
sets collision-query priority and survives object renaming or editor sorting.
New mesh objects need a valid `surface`; an omitted `order` places them after the
existing objects. Group transforms and ordinary indexed or unindexed triangles
are supported. Collision is static; export as GLB with **Custom Properties**
enabled and animations disabled.

The collision file follows standard glTF Y-up coordinates. Blender displays
height on its Z axis; moving an object up 10 units raises that collision surface
10 gameplay units. Existing visual meshes retain their current model-coordinate
convention. Preserve face winding: world queries use upward-facing triangles,
and overlapping water/land faces have specific ship behavior. The engine reads
current geometry and properties directly, with no JSON geometry fallback.

Editing the base collision changes only the base variant; copy and edit the
alternate file separately when both should change. Root `content.json` owns the
format version. World model, terrain, continent and cinematic documents do not
repeat it. Field and actor collision use the roles described above.

## Character names, starting state and growth

`game/characters.json` contains named entries for the party and Noishe. The
`mappings` object connects gameplay IDs to those names. Edit `name` to change the
default display name in menus and dialogue. A saved custom name still takes
precedence. `full_name` is an optional format such as `"{name} Irving"`; omit it
to show only the current name. Original display spellings such as `LIoyd` are
preserved; `I` and `l` have separate dialogue-font glyph entries.

Playable entries have `initial` and `growth`. For example:

```json
"lloyd": {
  "name": "LIoyd",
  "full_name": "{name} Irving",
  "initial": {
    "stats": {"hp": 200, "tp": 26, "strength": 44, "defense": 16,
              "intelligence": 40, "evasion": 64, "accuracy": 76},
    "equipment": {"weapon": "wooden-blade", "body": "soft-leather"}
  },
  "growth": {"hp": [58, 65], "tp": [4, 6], "strength": [3.8, 5],
             "defense": [2, 2.4], "intelligence": [1.2, 2],
             "evasion": [0.7, 1.3], "accuracy": [0.7, 1.3]}
}
```

Stat values are points: HP/TP are whole numbers; strength, defense, intelligence,
evasion and accuracy support tenths. Growth ranges include both endpoints. A
single number means a fixed gain. Omitted stats or gains are zero. Title growth
is added separately from `game/titles.json`, including the initial-title bonus
when no active title is supplied. Every stat keeps its usual random draw even
when the gain is fixed.

Zelos and Kratos use `"growth": "magic-swordsman"`, referring to the shared
`growth_profiles` entry in this same file. Edit the profile to affect both, or
replace either reference with a local stat/gain object to make it independent.
Their starting stats and equipment remain separate.

In `initial`, omitted `level` means 1 and omitted `luck` means 50. Experience,
affinity, Over Limit and technique balance default to zero. These are starting
values; changes do not overwrite state already stored in a save. Equipment uses
`weapon`, `body`, `head`, `accessory1`, `accessory2` and `arm`, with names from
`game/items.json`. Missing slots start empty. Starting `techniques` is a list of
names from `game/techniques.json`; `shortcuts` maps `neutral`, `up`, `down` and
`side` to those names. Eligibility and level learning stay in the technique
catalogue. Initial recipe mastery (`cooking`) uses names from `game/cooking.json`,
for example `"cooking": {"sandwich": 3}`; `ex_gems` uses physical slots 1–4, each with `level` and an optional named `skill`. Level 5 means a Max gem.
Compound-skill lists use names from the character’s EX compound mappings in
`game/ex-skills.json`. Omit unused starting state.

The top-level `experience` object maps each level to its total required EXP.
Preserve contiguous level keys. The final original thresholds are retained,
including the terminal entry. Noishe needs only a name; it has no playable state.

## Cooking recipes and ingredients

`game/cooking.json` owns cooking rules and captions. The recipe `mappings` keep
saved recipe IDs stable; edit named entries such as `sandwich`, `curry` and
`paella`. Each recipe has `text`, one `effects` list, ordered `required`
ingredients and nine named `cooks`. Effects use names such as `hp_recovery`,
`revive` or `defense_boost`; their displayed captions live in `effect_names`.

An ingredient is an item name, such as `"beef"`, or a named group selection,
such as `{"any": "greens"}`. Groups contain a display `name`, an `icon` and an
ordered list of item names. Group order affects which ingredient a random draw
selects. Ingredient list order and repeated ingredients are meaningful: each
entry consumes one item. There are no persistent group IDs.

A cook has `stars`, three `recovery` values for mastery 0–2, 3–5 and 6–8,
and an optional ordered `extras` list. Recovery is authored independently of
stars. Extra ingredients can have inclusive `from_mastery` and `through_mastery`
limits, omitted for 0 and 8 respectively. Bounds cover complete mastery grades.
For example, Zelos's sandwich extras are:

```json
[
  {"any": "meat-and-fish", "through_mastery": 2},
  {"any": "seafood", "from_mastery": 3},
  {"any": "greens"},
  {"item": "tomato", "from_mastery": 3},
  {"item": "cucumber", "from_mastery": 6}
]
```

This changes the first ingredient at mastery 3 without repeating the rest of the
list. There are no inherited recipe templates or patch operations. Missing
extras means no extra ingredients. Empty ingredients are omitted.

`preferences` uses character names with item-name `likes` and `dislikes`; an
omitted character has neither. `genis_bonus_ex_skill` names a skill from `game/ex-skills.json`, normally
`genis-personal`. Changing starting mastery in
`game/characters.json` affects new games. Existing saves retain their recipe IDs,
mastery, chef selection and fullness, and use the current recipe rules.

## Options and preferences

`game/options.json` owns all 18 option names/descriptions, editor labels, themes,
reset defaults and arte-forgetting confirmations. Both field and title menus read
this document through the active library, including enabled mods. Option keys are
engine meanings, such as `message_speed`, `combined_battles` and
`learn_both_arte_paths`; their menu order is fixed by the engine.

Defaults use names: `difficulty` is `normal`, `hard` or `mania`, and `window` is
`plain`, `ornate` or `gold`, matching `ui/menu.json`. `background` is
`cracked-stone`, `rough-stone`, `fine-grain`, `speckled`, `small-stones` or
`window-fill`. The last uses the selected window's fill. The existing options
menu cycles Normal/Hard; the Mania caption and saved value are retained.

Themes contain seven named RGBA colors. Omit `defaults.colors` to use the selected
window's theme. Include an explicit full color object for different reset colors.
Changing window style in the menu resets colors to that style's theme and selects
`window-fill`. Editing a theme therefore changes both style selection and the
reset palette when that theme is the default; no copied default palette is needed.

`defaults.controls` maps buttons to actions, for example:

```json
{
  "a": "attack", "b": "techniques", "x": "defend", "y": "menu",
  "l": "delay_spell", "r": "target_change", "z": "unison_attack"
}
```

Each action occurs exactly once. The `actions` object owns their displayed
captions. Button glyphs come from the named menu artwork; the options document
contains no sprite numbers. `color_groups` and `volume_channels` use meaningful
keys. Volumes retain the 0–127 scale, `message_speed` is updates per character
(0 means immediate), `screen_position` is `[x, y]`, and colors are `[r, g, b, a]`.
Empty option descriptions are omitted. Other descriptions and confirmation
messages preserve explicit newline characters.

The `confirmations` keys are `forget_elemental_artes`, `forget_arte_paths` and
`forget_artes` (both). Their text is editable; the engine still decides when each
confirmation is necessary and applies the same learning/forgetting rules.

`defaults` defines the menu's Reset action. Existing config and saved gameplay
preferences keep their current values and representation. Opening options does
not apply a mod's reset defaults automatically. When no config exists, the engine
still supplies its initial user-preference defaults. The library's options file
is admitted when the page is opened, so a broken optional options document does
not prevent unrelated gameplay.

## Strategy

`game/strategy.json` owns choices, captions, formation lanes, presets and the
preset-name keyboard. Each named group (`action`, `skill_magic`, `position`) has
its own `mappings` from saved choice ID to option name. These IDs also select
engine behavior and menu order. Rename an option by updating its mapping and
preset references; keep its ID stable for saved games.

An option's `text` contains its display `name` and one or two `description` lines.
Line breaks and spaces are intentional. `characters` lists eligible party names;
omit it to allow every character. Restricted choices no longer use bitmasks.
Fixed position choices have a `lane`: `front`, `middle` or `rear`. Position ID 0
(`move-freely`) uses the nine named character `default_positions` instead.

`preset_mappings` keeps the three saved preset IDs. A preset has a displayed
`name` (up to seven ASCII characters), one set of named default orders, and
optional character overrides containing only the changed choices. For example:

```json
"reserve": {
  "name": "Reserve",
  "default": {
    "action": "attack-close",
    "skill_magic": "save",
    "position": "move-freely"
  },
  "characters": {
    "raine": {"skill_magic": "keep-reserve"},
    "zelos": {"skill_magic": "keep-reserve"},
    "regal": {"skill_magic": "keep-reserve"},
    "kratos": {"skill_magic": "keep-reserve"}
  }
}
```

The defaults apply to characters without an override; an override changes only
its named choices. Existing saves keep personal strategies, edited presets and
preset names. Resetting a preset uses the current library definition.

`keyboard.rows` contains nine strings of ten ASCII cells. Keep spaces, including
trailing spaces: they are selectable blank cells. `keyboard.actions` names the
six action captions. The unused three mode captions are omitted when blank.
`labels` owns the page title and Orders, Rename and Default captions. Invalid
optional text is diagnosed independently from gameplay rules.

## EX skills and foraging

`game/ex-skills.json` owns EX skill rules, captions, character choices and gem
items. `mappings` connects machine/saved skill IDs to names. Each skill has its
`text` beside its rules. A skill’s `tendency` is `technical` or `strike`; omitted
means a compound skill. Omitted `activation` means `constant`. Empty stat bonuses,
absent save-point costs and absent combinations are omitted.

`stat_bonuses` is a list of `{ "stat": "strength", "percent": 5 }` entries.
Each bonus is rounded independently, so repeated entries are meaningful. Other
stat names are `defense`, `accuracy`, `evasion`, `max_hp`, `max_tp`, `luck` and
`intelligence`. `save_point_tp_cost` sets the special field-healing cost. Engine
behaviors use the mapped skill identity; renaming its key requires updating its
references and mapping. The nine Personal skills have character-specific keys,
such as `genis-personal` and `presea-personal`, while retaining their game captions.

Each character’s `levels` lists four named choices at gem levels 1–4, preserving
menu order. `gem_items` names the inventory items for levels `1` through `4` and
`max`. Each character’s `compounds` maps saved discovery IDs, starting at zero,
to named compound skills. Keep these IDs stable when editing an existing game.

A compound skill owns its `combinations`. Shared requirements appear once:

```json
{
  "characters": ["lloyd", "sheena", "regal"],
  "requires": ["strong", "tough"]
}
```

Each mapped character needs exactly one combination for that skill. To give one
character different requirements, move that character to a separate combination.
Requirement order is preserved in the menu. Editing a shared combination changes
all its listed characters. Initial gems, equipped skills and known/recent compound
skills live in `game/characters.json`, using these same names. Existing saves
retain their equipped IDs and compound discovery indices.

`fields/foraging.json` owns Presea’s available food. `pools` lists item names;
`areas` groups named fields with the ordered pool choices they share. Field names
come from `fields/mappings.json`. There are no copied field IDs. Each field belongs
to at most one area. The game selects a pool, then an item within it; a single
pool skips the first random draw. Pool/item order and repeated choices affect
selection and weighting. Keep the two stages when editing probabilities. Omitted
fields have no foraging items. Source declarations without a field resource are
not published.

## Titles, growth and awards

`game/titles.json` owns title captions and rules. Its `mappings` groups title IDs
under character names. For example, `mappings.lloyd["22"]`, `mappings.zelos["12"]`
and `mappings.kratos["10"]` select `tetra-slash`. Editing that definition affects
all three. To make an independent version, copy the definition under a new name
and change that character's mapping. Keep the numeric IDs for existing saves.

```json
"tetra-slash": {
  "growth": {"hp": 2, "strength": 0.2},
  "text": {"name": "Tetra Slash", "description": "Your title description."}
}
```

Growth is added on every level. `hp` and `tp` use whole points; `strength`,
`defense`, `intelligence`, `evasion` and `accuracy` use tenths of a point. Omitted
stats add zero. Omitted `growth` means no bonus. Different definitions can have
the same display name; `zelos-magic-swordsman` and `kratos-magic-swordsman` have
different bonuses and remain independent.

A title's optional `costume`, such as `"nobleman"`, selects a name from that
character's `battle/party/<character>/mappings.json`. Omit it to let story rules
choose the costume. A named alias selects the first matching costume ID.

`awards.levels`, `awards.combos` and `awards.equipment` are ordered lists. Keep
list order to preserve award notices. A level rule can be
`{"character":"lloyd", "title":"grand-swordsman", "minimum":20}`.
Equipment requirements use item names from `game/items.json` and slots `weapon`,
`body`, `head`, `accessory1`, `accessory2` and `arm`. Each requirement matches any
listed item in any listed slot; all requirements must match. Other event and
battle achievements retain their existing behavior.

## Item rules, captions and equipment eligibility

`game/items.json` owns every item's rules and text. Its `mappings` object resolves
machine IDs to the named entries in `items`, such as `apple-gel` or `wooden-blade`.
Edit the named entry; menus, inventory and dialogue substitutions use that same
entry. Editing a displayed name does not rename its stable key. Artwork remains
under `meshes/equipment/`, with its own item-ID mappings.

An entry can look like this:

```json
{
  "category": "gel",
  "field_usable": true,
  "battle_usable": true,
  "price": 50,
  "transforms_to": "lemon-gel",
  "field_use": {"kind": "recover", "hp": 30},
  "attention": "low_hp",
  "text": {
    "name": "Apple Gel",
    "description": "A gelatin-based medication.",
    "details": "Restores 30% of Max HP."
  }
}
```

`category` selects the inventory category and equipment slot. Names include
`gel`, `bottle`, `herb`, `dual_blades`, `staff`, `armor`, `helmet`, `shield`,
`ring` and `key_item`. `characters` is a list such as `["lloyd", "kratos"]`,
or `"all"`; omitted means nobody can equip it. Eligibility alone does not make
an unclassified item equipment. A key item has a stack limit of one; other items
have a limit of twenty. Set `stack_limit` to override it.

`stats` contains only the item's bonuses, using `slash`, `thrust`, `defense`,
`intelligence`, `accuracy`, `evasion` and `luck`. For example,
`"stats": {"slash": 100, "accuracy": -5}`. `properties` contains additional
non-default effects, such as `"attack_element": "fire"`, elemental `resistance`,
`immunities`, `hp_regeneration` or `tp_discount`. Omitted properties have no
effect. `properties.descriptions` selects named status-page descriptions from
`equipment_descriptions` in this same file:

```json
"equipment_descriptions": {
  "third-tp-cost": "Consume 1/3 less TP",
  "half-tp-cost": {
    "text": "Consume 1/2 less TP",
    "suppresses": ["third-tp-cost"]
  }
}
```

For example, an item can use `"descriptions": ["half-tp-cost"]`. Descriptions
control only the Status page. Gameplay still comes from properties such as
`tp_discount`. A shared text edit affects every item that selects it. Add a new
name directly to this dictionary; descriptions have no machine IDs or mapping
file. Use a plain string unless the description suppresses another. Suppression
hides the other caption when both are equipped and does not change its gameplay
effect. Description lists keep their order, including intentional repeats.
Omit a description reference when an item needs no status-page text.

Field and battle usability are independent. `field_use` describes field actions:
`recover` uses HP/TP percentages and optional `party: true`; `revive`, `cure` and
`transform` select their named operations. A herb such as
`{"kind":"herb", "stat":"strength", "amount":10}` adds ten stat points.
Use `percent: true` for a percentage increase. Stat points support tenths;
percentages are whole numbers. `encounter_rate` uses `aura: "holy"` or
`aura: "dark"`. `transforms_to` names another mapped item; omitted means there
is no transformation. Optional `view` selects an item-opened book or map, and
`attention` selects its battle-use prompt.

`price` is the base sale value; shops apply their purchase multipliers. Missing
flags are false, missing stats/prices are zero, and missing text description or
details is empty. Preserve the mappings when changing captions. Adding an item
requires the next consecutive ID and a unique named entry. The format contains
no source-platform, archive, ownership or content-identity metadata.

## Technique rules, captions and learning

`game/techniques.json` owns each technique once. Its `mappings` connects menu and
battle technique IDs to names such as `demon-fang-lloyd` and
`demon-fang-zelos-and-kratos`. Identical captions do not imply identical rules;
the owner suffix distinguishes these genuine variants. Change the displayed
caption without changing the stable key.

Each entry under `techniques` contains `rules`, optional `menu` settings and
`text` (name and description). For example, edit
`techniques.demon-fang-lloyd.rules.tp_cost` to change its TP cost everywhere.
Elements and learning references use names. Zero/false/absent ordinary settings
are omitted. An omitted element means no element; the default target is `enemy`
and the default casting effects are `standard`.

`menu` retains only separate choices: `tp_percent`, category marker `rank`,
`event_only`, the ordered named `forget_with` list and a `field_use` action.
Cost, element, level, parent and Unison eligibility come from the battle rules.
Menu captions and script technique names use the same `text` entry. An invalid
optional caption does not disable its rules or another technique's caption.

`characters` maps the nine playable IDs to learning-list names. Each `learning`
list contains ordered technique names. Automatic level learning selects entries
with levels 1 through 250, no T/S or event-only route, and no parent or grouped
prerequisites. This is separate from the battle `learn_on_level_up` flag.
The session and menu documents no longer repeat these technique tables.

The seven elemental variants have explicit editable entries. Runtime loading
does not recreate them from their base technique. Their battle activation logic
still uses the established elemental-variant rules. The unused zero slot is not
stored. `unassigned-144` and the two numbered Colette Listra entries retain
unresolved identities rather than invented names. Casting-scene IDs in party
voice policies belong to a different namespace from this mapping.

## Martial techniques

`battle/party/techniques.json` keeps technique tracks in one document. Start at
`techniques.demon-fang.variants.lloyd`, for example. Character variants are
named; Rising Falcon distinguishes grounded, airborne and Rising Phoenix
tracks. Each technique's `mappings` connects runtime variant IDs to these names;
the top-level mapping names the technique owners. `bindings` retains the
technique/character selections used by gameplay.

Empty variant slots are absent. Real conversion failures retain an explicit
`unsupported_reason`; successful tracks contain their settings directly. Event
times, chaining, recovery and pose frames use the same units as normal attacks.
The alternate hammer and successful theft events belong to the same technique.

Repeated contacts and projectile releases use names in that technique's
`contacts` or `projectiles` section, for example `[16, {"projectile":"projectile"}]`.
Single-use definitions stay inline. Editing Demon Fang's shared projectile
changes all three character versions. To make one release independent, replace
its name with a copy of the definition. Shared definitions cannot reference
other definitions or files. Omitting a projectile's `hit` inherits its existing
hit recipe; explicit zero values remain meaningful.

`unassigned-action-51` and `unassigned-action-141` through `unassigned-action-145`
preserve real tracks
whose purpose has not yet been confirmed. Their numbered variants are an open
naming gap, not guessed character or technique identities.

## Enemy rules

The `rules` section of `battle/enemies/<enemy>/enemy.json` keeps action choices
and their named `attacks`, `projectiles` and `contacts` together. An action selects an attack
such as `{"attack":"attack-1"}`. Its timeline uses events such as
`[74,{"projectile":"projectile-1"}]`. Edit that projectile once to change every
shot using it. Names are local to the file and can be renamed with their uses.
These are ordinary names; there are no hashes, inheritance chains or extra files.

Absent flags are false, optional values are absent, and lists are empty.
Returning to formation defaults to true; body opacity defaults to 255 and model,
shadow, effect and overlimit scales default to 1. Timing, damage and other required
values remain explicit. Short numeric arrays and timeline entries stay on one
line; longer numeric arrays wrap across lines.

## Empty assets

A speech mapping may contain a named silence clock, for example:

```json
{"name":"skits/whats-the-temple-like/colette-003","silence":{"frames":64000,"sample_rate":32000,"channels":1}}
```

This preserves timing without an empty WAV or a PCM allocation. To add speech,
replace the mapping value with the WAV path relative to `audio/speech/`.
Skits use this same mapping. Their catalogue selects voice IDs and a shared
playback speed:

```json
"media": {"ids": [131072, 131073], "speed": 1.000875}
```

Omit `speed` for normal playback. The multiplier applies to the current WAV header
or silence clock, rounding down to a whole playback sample rate. Imported skits
use `1.000875` to preserve their original timing. To voice a silent skit, replace
its speech mapping with a WAV path; no catalogue change or separate skit speech
mapping is needed. Changing that WAV's length or rate updates playback and subtitle
timing on the next preparation. The explicit ID list selects available lines;
scenario scripts are not scanned at runtime.
Silent effects share `audio/effects/silence-320-frames.json`, a small `kind: silence`
definition with `frames` and `sample_rate`. Change a cue's mapping to give it an
audible replacement. Programmed effects can contain silent instrument entries
such as `"silence": {"frames": 16, "sample_rate": 22050}` instead of `path`.
The sample's pitch and loop settings still apply; frame counts include any
separate repeated-loop frames. No empty effect or instrument WAV is needed.

Physical source directories use `null` for absent members. They do not create
`empty.json` payload files or empty texture catalogues. Texture banks with real
images, including intentional transparent artwork, remain editable KTX2 assets.

## Unison attacks

`battle/unison.json` keeps the team-attack rules together. `characters` uses the
names from `battle/party/mappings.json`; each character chooses a named `opener`
and an optional `voice`. Omit the voice for silence. `openers` contains the shared
opening tracks, and `hits` contains their shared contact recipes. A shared edit
affects every character using it. The team's `contact_ticks` interval defaults
to the track's `duration`; Genis and Raine retain their distinct longer interval.
Event times and durations are combat ticks.

`placements` groups techniques that share their distance, altitude and duration.
Names come from `game/techniques.json`. Altitude defaults to zero; a zero duration
is an authored value and does not decide whether the technique is usable.

```json
{
  "techniques": [
    "demon-fang-lloyd",
    { "technique": "demon-fang-zelos-and-kratos", "matches": "demon-fang-lloyd" }
  ],
  "distance": 350,
  "duration": 65
}
```

An ordinary name also names its matching group. `matches` makes another
character's technique count as the same ingredient in combination recipes.
The document's `mappings` bridges machine matching IDs to those group names.
To change one technique's placement independently, move its selection to a new
placement block. Keep its matching group when its recipe identity should stay
the same. Distances and altitude use battlefield coordinates.

`combinations` is an ordered list; earlier entries have selection priority.
Each entry names its existing `battle/scenes/` scene and lists alternative
recipes using matching-group names:

```json
{
  "name": "Pow Blade",
  "scene": "pow-blade-0301",
  "recipes": [["tiger-blade", "pow-hammer"], ["tiger-rage", "pow-pow-hammer"]],
  "duration": 100
}
```

Ingredient order and repeated ingredients are meaningful. All alternatives need
the same two-to-four participant count, inferred from their lengths. An empty
recipe list disables selection and requires an explicit `participants` count.
`camera_pitch` defaults to zero. The host settings retain the four relative
party placements, spacing, distance limits, colors and prelude camera; each
character's voice is edited beside its opener.

## Battle formations

`battle/formations.json` contains named groups under `formations`. The `mappings`
object binds each machine request ID to one group. Several IDs can share the same
formation; an edit to that named group affects all of them.

```json
{
  "actors": [
    { "enemy": "wolf-0017", "count": 2 },
    { "enemy": "boar-0030", "position": [300, 100] }
  ],
  "escape_restricted": true
}
```

Enemy names come from `battle/enemies/mappings.json`. Actors appear in the listed
order; `count` repeats an identical actor consecutively. Omit `position` for
automatic placement, or supply `[x, z]` in battlefield coordinates. `variant`
selects the enemy's statistics variant; `texture_variant` and `weapon_variant`
select appearance alternatives. All three default to zero. `hidden_name` hides
that enemy's battle label and defaults to false. Optional `reserves` declare
additional enemy kinds needed during the encounter, using the same `enemy` and
`hidden_name` fields. They do not spawn actors.

Music, victory celebration and entry voices are enabled by default. Set
`play_music`, `celebrate` or `entry_voice` to false when needed. Escape is allowed
unless `escape_restricted` is true. Explicit unsupported appearance instructions
keep their `unsupported_reason`; these remain preparation errors when selected.

Each entry in `pools` lists the machine pool `ids` sharing its ordered `choices`.
A choice supplies a formation request ID and a positive relative `weight`:

```json
{
  "ids": [0, 400],
  "choices": [
    { "formation": 102, "weight": 30 },
    { "formation": 103, "weight": 70 }
  ]
}
```

Use `mappings` in this same document to look up each choice's named formation.
Choices retain request IDs even when two IDs share a group, and their order
controls which random draws choose each encounter. To edit one shared pool
independently, move its ID to a new pool entry. Keep every existing formation
and pool ID represented; adding a new formation uses the next request ID and
its own mapping. No new files or media conversion are needed.

## World encounters and docks

`world/encounters.json` names the two worlds and each encounter region. A region's
`id` is the number used by its world's 24-column, 18-row `grid` and the existing
Noishe unlock flags. Grid columns increase east; rows increase south; each cell
covers 3,200 map units. Renaming a region does not change its ID.

`groups` uses terrain names such as `grassland`, `forest` and `beach`. A number
selects the same battle encounter group for both enemy symbols; `[first, second]`
selects their distinct groups. These are group IDs in `battle/formations.json`,
not formation IDs. Omitted terrains use the document's explicit `default_group`.
For example:

```json
"iselia": {
  "id": 0,
  "groups": {"grassland": [501, 502], "road": [503, 504], "forest": [505, 506], "bridge": [503, 504]}
}
```

A string in `groups` shares another named region's inline groups in the same
world. References are direct: there are no chains. Tethe'alla regions may supply
`later` groups, selected when the story counter reaches `later_story`. Omitting
`later` keeps that region's early groups. A string in `later` shares the target's
later groups, or its early groups if it has no later override. Editing a shared
owner updates its references; replace a reference with an object for a local edit.
Each world's `arenas` maps all ten terrain names to battle arena IDs. Later
encounters use the same arena selection.

The shared `surface_terrain` map selects battle terrain by the named collision
surface. For example, `"pier":"bridge"` uses bridge battles while retaining pier
boarding behavior. An absent or null entry suppresses encounters and prevents
Rheaird landing. A world's optional map overrides just the listed surfaces.

`docks` is an ordered list of `land` and `sea` endpoints. Each endpoint has an
integer `[east, south]` map `position` and optional `height` (default zero).
Order matters: the last nearby endpoint wins, and embarking falls back to the
first dock when no land endpoint is nearby. Docks have no fixed count; an empty
list disables embarking/disembarking. Boarding requires a `pier` surface;
land endpoints use a 2,000-unit search box and sea endpoints use a 200-unit box.
Unused native endpoint words, source diagnostic labels, and unused original
streaming-priority tables are not published.

## World landmarks and guideposts

`world/landmarks.json` contains ordered `sylvarant` and `tethe_alla` location
lists. A location owns its optional `reward_item`, `required_character` and
`guidepost`; there are no separate reward/party tables or guidepost file.
`required_character` uses a party name (`lloyd`, `colette`, `genis`, `raine`,
`sheena`, `zelos`, `presea`, `regal` or `kratos`). Rewards are one item, using the
item IDs in the `mappings` object of `game/items.json`.

Hidden developer labels use their established purpose: character affinity events,
item discoveries, tutorials or guidepost regions. Unassigned disabled locations
retain explicit identifiers. Two original development captions remain because
field menus refer to them (locations 282 and 304); the source caption text is
preserved. Event IDs, contact order and displayed location/region captions do not
change when cooking these descriptive hidden labels.

```json
{
  "id": 45,
  "name": "Stone Marker",
  "position": [10000, 20000],
  "radius": 80,
  "marker": "unmodeled",
  "guidepost": {"region": "Triet Desert", "discovery_flag": 901}
}
```

The map menu uses these same entries through its direct
`world_map.landmarks` reference in `game/menu-data.json`. Rename a location or
move its `position` here to change both world travel and the map display. Map
points are derived at 200 world units per image pixel. `map_listed: true` includes
the location in the menu once visited; omission hides it. The menu lists visited
places in numeric ID order, independently of contact priority in this document.

Locations can own `shops: [0, 1]`, referring through `world_map.mappings` to the named shops in
`game/menu-data.json`. Omission means no shops. Optional `shop_variants` select a
replacement list when a saved script global reaches a threshold, for example
`{"global": 62, "at_least": 500000, "shops": [16, 50]}`. The last matching variant
wins. `global` is the word index in saved `script_globals`; these numeric state
selectors remain the stable references used by this landmark document; shop
captions and stock are editable under their named menu owners. An optional
`visit_alias` records another location's ID as visited, as the two ruined/rebuilt
Luin entries do with location 7. Omission records this location's own ID.

The position above is an editing example. Positions use `[east, south]` map
coordinates. Omitted `height` follows terrain; explicit `height: 0` means sea
level. Interaction defaults to `active`; `disabled` suppresses contact and
`blocked` keeps the existing blocked-entry behavior. `automatic` defaults to
false, and absent `entry_overrides` means ordinary return placement.

A marker is `field_point`, `unmodeled`, or an object such as `{"model": 11}`.
Model IDs use `world/markers/mappings.json`. Sylvarant uses the local ID;
Tethe'alla adds 256, so local marker 16 uses mapping 272. This is the same world
namespace convention used by locations. An omitted marker means
`none`; marker visibility and interaction are independent. Field points retain
the existing foot-only contact behavior; unmodeled locations retain their
location behavior without drawing a marker model.

Marker artwork lives in named folders such as `world/markers/novas-caravan/`,
`luin-ruins/`, `luin-rebuilding/`, and `ozette-destroyed/`. Each has a `model.json`,
its GLB and textures directly beside it. Animated portals also have a
`portal-loop.glb`. The two worlds share `stone-guidepost/` and `discovery-point/`;
their portal models remain distinct. Common `luin-cliffs.ktx2`,
`ozette-ground.ktx2` and `portal-glow.ktx2` live directly in `world/markers/`.
Editing one of these files changes all references to it. To customize just one
use, copy the resource and change that model's path or its single mapping entry.
For example, a mod containing `{"272":"my-guidepost"}` in the marker mappings
changes Tethe'alla's guideposts while retaining every other base binding.

The first matching location wins overlapping contact checks, so list order is
intentional. IDs remain independent of position in the list: reordering entries
does not retarget scripts, rewards or requirements. The current script/save
namespace distinguishes Sylvarant and Tethe'alla. The current game services and
save state accept IDs 1–98 and 257–337 respectively; adding locations beyond
those ranges requires further runtime work. Keep existing IDs with their
locations unless the referring scripts are edited too.

A guidepost's `region` is its displayed caption. `discovery_flag` records that
it has been read and is set along with optional `unlock_flags`. There are no
null flag slots or source string-pointer IDs. The guidepost sequence follows
location order across Sylvarant and then Tethe'alla; the world script iterates
that sequence to update discovered markers.

Entry overrides preserve their authored order. `direction`, when present, is
0 east, 1 southeast, 2 south, 3 southwest, 4 west, 5 northwest, 6 north, or 7
northeast. `heading` and `camera_yaw` are radians; an omitted value retains the
ordinary return behavior. A mount override names `foot`, `noishe`, `ship` or
`rheairds`. `placement` explicitly selects `fixed`, `landmark_center` or
`perimeter`. Unknown/native placeholder location captions and marker-model
names still need further review.

## Camera animation and cinematic settings

Camera motion is a standard GLB animation with two named animated points:
`Camera position` and `Camera target`. The first carries the viewpoint and full
camera rotation, including roll; the second carries the authored target point.
These are animated scene nodes. Keep their names, but their node order and parent groups
can change. Translating or rotating a parent group moves the corresponding path.
The reader supports static transforms and linear, step and cubic interpolation.

GLB coordinates use Y-up and timestamps use seconds. Gameplay coordinates
`[x, y, height]` correspond to GLB `[x, height, -y]`. Each GLB contains one
animation. In Blender, use 30 fps to match the resource-frame clock and export
both points together as one animation (the **Active actions merged** export
mode). Keep animations enabled. Retain the two points when adding an editor
camera or a tracking constraint for preview. Export the position node's rotation
as well as its translation. Runtime playback preserves that rotation, including
roll. Actor-relative aiming overrides use the authored up direction.

A field's `scene.json` maps its existing camera resource IDs directly to GLBs.
A single trajectory normally uses `camera.glb`; multiple trajectories use
`camera-1.glb`, `camera-2.glb`, etc. Rename the file and update that path to give
an identified scene a more descriptive name. Script IDs stay unchanged. The two
title backgrounds share their paths under `ui/title/`.

`world/cinematics/mappings.json` selects named scene folders, such as
`izoold-to-palmacosta`, `thoda-geyser-outbound` and `rheairds-run-out-of-mana`.
Each actor's model, GLB and clips sit directly beside `cinematic.json`, with names
such as `lloyd-rheaird.json` and `lloyd-rheaird-motion-0.glb`. The `actors` object
maps local controller IDs to these model paths. Its separate presentation settings
keep those same IDs; renaming a file does not change actor dispatch.

Common textures live in `world/cinematics/artwork/`, named after their first
scene/actor owner. A shared image edit affects every model that references it.
Copy the image to a new path and update one model's texture reference to make
an independent edit. Meshes, material choices and animation bindings remain
independent: no complete cinematic body matched another byte for byte.
The `unassigned-ranch-explosion` scene retains its unknown ranch identity;
its explosion and smoke actors still have descriptive roles. The two ordinary
world encounter symbols retain numbered variants because their narrower identities
are unsupported by the existing evidence.

World cinematics have a direct `camera` path and can contain timed `dialogue`
lines in `cinematic.json`. Omitted dialogue means no lines. `presentation.actors`
contains only overrides; omission keeps a model's ordinary depth/blend/opacity
and UV behavior. Empty audio lists, false `before_dialogue`/`background`/`white`,
absent intro fades and empty flash lists can be omitted. Audio events preserve
list order, including deliberate repeats. Frame/tick numbers in these gameplay
settings retain their documented clocks; the camera GLB itself uses seconds.

## World movement and flight clearance

`world/movement.json` owns shared movement and camera coefficients. Camera
ranges name their view and mount category:

```json
"camera_distances": {
  "close": {"foot": 1200, "mounted": 2800},
  "distant": {"foot": 1800, "mounted": 3400}
}
```

`mounted` covers Noishe, Rheairds and the ship. A view change or mount transition
uses the existing `zoom_transition_ticks` easing. Distances use game world units;
controller tick settings use the 60 Hz update clock. `heading_quantization` and
`camera_yaw_quantization` are scene-transfer steps per radian.

A location in `world/landmarks.json` can own an optional `flight_floor`:

```json
"flight_floor": {"height": 1000, "margin": 50}
```

It uses the location's position and radius, extended by `margin`. The example
sets a minimum obstacle height of 1000; the movement document's shared
`flight_clearance` is applied above that height. `active_only` defaults to true:
the floor disappears when the location's interaction is disabled. Set
`"active_only": false` for a permanent obstacle. Removing `flight_floor` removes
this additional floor; terrain clearance still applies. Height plus the shared
clearance must fit below `maximum_altitude`.

Independent circular obstacles stay in `movement.json` under
`flight_zones.sylvarant` or `flight_zones.tethe_alla`. Each contains `position`
(`[east, south]`), `radius` and `height`; an omitted world list means no extra
circles. Overlapping circles and location floors use the greatest height, and
footprints wrap across world seams. Zone list order does not change the result.

The `collision` object in `world/movement.json` contains the neighborhood
`radius`, `ground_probe_half_extent` and `ship_probe_half_extent`, all in world
units. Ground and ship queries require clearance at all four probe corners.
There is no separate collision-rule file or numeric response matrix. Ship travel
rejects overlapping land even when a water face is encountered first.

## Victory selection, defense and recoil

A character's victory costumes select `healthy-party`, `injured-leader`, `ordinary`,
`rare` and `flawless` performances. These are gameplay selection roles, not names
for the gestures. Colette's story state and Presea's recovery have existing special
selection rules; Colette can also use the flawless slot regardless of party damage.
The rare slot is the one-in-ten alternate to the ordinary slot. Independent costume
clips remain separate; identical choreography still shares a named timeline.

`battle/victory/groups.json` is an ordered list. Character, participant and optional
leader names resolve through `battle/party/mappings.json`. Group IDs, conditions,
priorities and encounter request IDs remain explicit. An ordinary group's gate
specifies random sample bits: `{"set":[0]}` requires bit 0 to be one;
`{"clear":[0,1,2]}` requires those three bits to be zero. These preserve the exact
sample tests and correlations, not just the nominal probability. Voice bindings
are unchanged.

`battle/defense-system.json` names its four component enemies through the existing
enemy mapping. `arm_joints` lists the core GLB's four exact joint names in formation
arm order. A model edit can rename these joints by updating the bindings too;
there is no runtime dependency on a PA name prefix. Deployment uses a named body
motion selector. Repair timings, percentages, placement and messages stay together.

`battle/recoil.json` maps hit recoil request IDs to `horizontal` and `vertical`
impulses. The IDs remain explicit so reordering JSON cannot redirect a hit.
Guard recovery bonuses use character names from the party mapping. Only the nine
playable characters have default bonuses; original unused catalogue rows are not
published. Formation `statistics_variant` selects that enemy's independent stats
and reward row. `texture_variant` selects an atlas row and `weapon_variant` a carried
weapon variant. These three choices are independent; unsupported identities remain
numbered rather than receiving invented names.

## Defaults and shared geometry

Identical mesh parts within one model, equipment item or effect bank can reference
one GLB. Their ordered part declarations, material settings, attachment roles and
animation choices remain separate. Editing that GLB changes all its references;
copy it and change one part's `mesh` path for an independent edit. Distinct model
owners and the base/alternate world terrain remain independently editable.

Enemy actions omit an empty `policy`; its default has no special restrictions.
Monster List entries omit empty `weaknesses` and `resistances`, and all-neutral
`affinities`. Preview parts omit all-zero `uv_offsets`. Nonzero settings stay
explicit. An absent UV list supplies zero for every material, while a provided
list preserves material order, including any zero rows among nonzero rows.

Other empty or zero values can be meaningful: an empty effect program keeps its
request slot, an empty behavior object enables that behavior with defaults, and
zero colors or palette selections are actual drawing values. Empty overrides may
also intentionally clear inherited settings. Do not remove them indiscriminately.
