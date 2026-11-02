# Steam importer

A parallel source importer based on `crates/importer`, the GameCube importer.
Each category reads the installation and writes its final editable documents.
The coordinator lists the passes in dependency order. Archive decoding and PE
address translation stay inside this crate; gameplay and rendering use the common
content format. GameCube rendering remains the target.

```sh
cargo run -p resonance-import-cli -- steam "/path/to/steamapps/common/Tales of Symphonia" --output local/steam-preview
```

Input is the installed `TOS.exe` and its TOFHDB/TLDAT archives. No disc image,
GameCube extraction or old cooked library is needed. `--language` accepts
`english` (default), `french`, `spanish`, `german` or `italian`. Localized archive
resources take precedence over the English base archive. Executable text tables
select their corresponding language slot.

**This is an incremental port, not yet a playable library.** The latest full run
completed all 33 implemented passes in 198 seconds and wrote entry points for all
509 available fields. Nine field resource references are still unavailable or
unconverted. The classroom and Asgard field documents assemble through the shared
reader. The shared gameplay loader now initializes a party and saves/restores the
added collection IDs. Startup scene and battle playability remain unverified.

Current passes write
the library descriptor, common script definitions, all 1,000 battle formations and
736 encounter pools, recoil, 531 items, 255 native techniques plus the common
elemental variants, field mappings/foraging, EX skills, and scripts/messages/collision
for all 509 available field packages. It also writes their available scenery and
panoramas as GLB/KTX2, plus camera tracks in the common camera format. Model
conversion retains skeletons, weighted joints, vertex colors and texture mips;
materials use the existing GC pass. Missing scenery referenced by shipped debug
packages is reported rather than replaced with invented geometry.

Shared field settings now include native effect atlases, localized notices,
contact shadows and a GC-style toon ramp. Scenery transforms and all nine party
characters now have standard GLB motions. Party models include the seven native
costume slots, named aliases, field/service clip bindings and an outline part.
Where Steam has no outline geometry, the importer bakes an inverted hull for the
existing GC pass. No Steam runtime renderer or animation player is added.

The following passes write global NPC/standalone model mappings, model documents,
collision, ordinary field actor selections and field entry points. Party and NPC
packages use one model writer. Script-selected TOANMB motions are baked against
the selected rigs into existing field animation bindings; the cook shares a
converted clip between fields that use the same source clip and rig. Unresolved
script selectors use the existing shared dynamic-resource boundary.

The Iselia classroom field (332), including its actors and selected motions,
assembles through the current content reader. Retained GC motion streams now
normalize their little-endian tables and reuse the GC converter, preserving the
original big-endian keys and tracks absent from the selected body. The Asgard
and Iselia ranch packages that stopped earlier cooks pass the shared reader.
Motion binding reads each source rig once without decoding geometry again;
identical model packages share their existing cooked owner across fields.

Character defaults, experience and growth, all 24 recipes and ingredient groups,
and all 177 native titles now use the same named documents as the GC importer.
Recipe ingredient merging reuses the GC converter. Title mappings keep native
IDs (including inserted Steam rows), with stable names across all five languages.
The two added costume slots are ordinary shared variants, including save/load
validation. The installed check prepares the actual character/recipe/title rules
and constructs and serializes a fresh party in every supported language.

The manual, name editor and customization options are also written in the
current GC document schemas, using the shared seven-button controls and host
options. All five languages pass the document readers. The common name editor
accepts localized names and edits character cells safely.

Embedded legacy banks, legacy model packages and dynamic animation-bank routes,
service actors, enemy documents, remaining game/menu rules and other media also
remain incomplete. Material animation, face atlas selection and animated visibility
remain pending. Party model defaults retain initial hidden geometry; scenery
node visibility still needs projection through the common playback path. Use a
separate output directory while those owners are missing.

The added native techniques receive ordinary logical IDs after the common
GameCube elemental variants. Named references resolve during import. The shared
technique menu retains six forgetting alternatives, including the original four
from GameCube. Neither source requires a runtime edition flag. Field input checks
are adapted with ordinary bytecode expressions around the existing GC input
native, including Steam's raw-press mode. Branch targets, event entries and message offsets are
preserved when code grows. Source text encodings disappear into Unicode messages.
TOANMB skeletal timestamps are converted from 60 Hz to the common 30 Hz clock
when cooked; retained GC streams keep their authored GC clock. Script seek times and sound-ID conventions still need
conversion with their media owners.

Run parser tests with `cargo test -p resonance-steam-importer`. The focused installed
source test also loads the generated documents with the current content readers,
checks the two added Niflheim trios, all five text languages and overwriting an
interrupted cook:

```sh
RESONANCE_TEST_STEAM="/path/to/Tales of Symphonia" cargo test -p resonance-steam-importer installed_categories_load -- --ignored
```

Dialogue fonts, window artwork, all 531 item icons, portraits, menu frames and
localized shop/save/action captions now use the ordinary shared UI documents.
The menu atlas and title images are normalized to the common 640x480 canvas.
Boot logos, title artwork, defeat artwork and story subtitles use the existing
readers; the title definition is shared with the GC importer. The common input
atlas is exported by the same helper as the GC cook. No runtime changes or
Steam-specific UI mode are required.

The installed-source UI check cooks the required tables, loads the bitmap font,
captures the shared UI/title/boot documents and prepares menu images. It checks
subtitles and defeat captions in all five languages. It does not prove title
scene, audio, field or battle playback:

```sh
RESONANCE_TEST_STEAM="/path/to/Tales of Symphonia" cargo test -p resonance-steam-importer installed_startup_and_dialogue -- --ignored
```

Strategy choices/presets, Grade Shop purchases, crafting vendors/recipes and all
200 journal entries now use the current named gameplay documents. Journal wrapping
and Grade Shop exclusions reuse GC conversion helpers. Shared world landmarks own
map positions, shop stock changes, rewards, party requirements and guidepost flags;
the map-menu document binds named shops and field destinations to that same data.
World travel/media and remaining controller details still need their own owners.
All 101 shared page captions are cooked. Blank translations of required labels
retain the shipped English text. Three shared-engine action captions (loading,
leader selection and targeting all allies) currently use English text.

Cooked text is UTF-8 for both source importers. Source code pages are decoded at
import time. Names, preset names and keyboard grids use character positions and
limits in the shared readers/editors. Long preset names fit within the common
menu frame; there is no platform-specific text mode. The installed menu check
prepares all five languages and a Unicode keyboard override through shared readers:

```sh
RESONANCE_TEST_STEAM="/path/to/Tales of Symphonia" cargo test -p resonance-steam-importer installed_gameplay_menu_tables -- --ignored
```

The Monster Book now contains all 257 native records and the Figure Collection
contains all 297 entries. Model previews reuse the same geometry, image, motion
and outline writers as fields. They retain authored idle clips, carried and
secondary bodies, figurine body-atlas rows and bone visibility rules. Specialized
preview behaviors still need their owners; this does not establish full visual parity.
The shared engine uses catalogue-derived collection counts and 16-bit monster
identities through knowledge, saves, rewards, AI and model/voice lookups. Existing
GC numeric IDs and JSON document shapes remain compatible. Counts are derived
from records rather than written as separate authoring settings.

Native battle-party costume mappings are also cooked, so title preparation uses
the actual battle-package directory. Party battle models/rules are still pending.
The current Steam library passes shared gameplay loading, fresh-party creation,
save/load of monster 256 and figurine 296, and native collection-completion calls.
A temporary French overlay also loads through that same path with UTF-8 names.

```sh
RESONANCE_TEST_STEAM="/path/to/Tales of Symphonia" cargo test -p resonance-steam-importer installed_catalogues_and_models -- --ignored
RESONANCE_TEST_LIBRARY="/absolute/path/to/steam-preview" cargo test -p resonance-steam-importer cooked_gameplay_definitions -- --ignored
RESONANCE_TEST_STEAM="/path/to/Tales of Symphonia" RESONANCE_TEST_LIBRARY="/absolute/path/to/steam-preview" cargo test -p resonance-steam-importer installed_localized_gameplay -- --ignored
```

The complete goal remains a Steam-only cook that loads through the same library
readers and engine as the GC output, with GameCube-style presentation. Remaining
owners include remaining game rules and battle artwork; legacy field motion, service actors
and dynamic shared banks;
party/enemy/equipment battle models and rules; overworld data and media; skits,
audio, movies and remaining startup/ending media. Completion requires exercising startup,
a field and battle from that library. The table and field-source checks alone do
not establish that end state.

For a focused field-source conversion/load check:

```sh
RESONANCE_TEST_STEAM="/path/to/Tales of Symphonia" RESONANCE_TEST_STEAM_FIELD=332 cargo test -p resonance-steam-importer installed_field_sources_convert -- --ignored
```

Omit `RESONANCE_TEST_STEAM_FIELD` to convert and read all available field scripts,
message documents and collision meshes. This check does not execute gameplay.

For a focused model conversion/load check, including a weighted party body:

```sh
RESONANCE_TEST_STEAM="/path/to/Tales of Symphonia" cargo test -p resonance-steam-importer installed_scenery_and_skinned_body -- --ignored
```

The full scenery pass currently produces 509 scene documents, 1,021 model parts
and 53 camera tracks from the installed English source. These files use the
current scene/material grouping schema. Their shared settings and image references
are also cooked. The focused classroom check additionally assembles actor and
motion documents; it does not execute gameplay or prove the full library playable.

To cook party models and sample their converted motions through the shared reader:

```sh
RESONANCE_TEST_STEAM="/path/to/Tales of Symphonia" cargo test -p resonance-steam-importer installed_party_models_and_motions -- --ignored
```

After cooking the supported categories, assemble the classroom field documents:

```sh
RESONANCE_TEST_LIBRARY="/absolute/path/to/steam-preview" cargo test -p resonance-steam-importer cooked_field_actor_documents_assemble -- --ignored
```

The global actor model conversion/load check uses installed Steam data only:

```sh
RESONANCE_TEST_STEAM="/path/to/Tales of Symphonia" cargo test -p resonance-steam-importer installed_global_actor_models -- --ignored
```

For retained native curves and localized menu tables:

```sh
RESONANCE_TEST_STEAM="/path/to/Tales of Symphonia" cargo test -p resonance-steam-importer installed_retained_actor_motion -- --ignored
RESONANCE_TEST_STEAM="/path/to/Tales of Symphonia" cargo test -p resonance-steam-importer installed_menu_settings -- --ignored
```

For character defaults, recipes, titles and party save/load validation:

```sh
RESONANCE_TEST_STEAM="/path/to/Tales of Symphonia" cargo test -p resonance-steam-importer installed_character_and_recipe -- --ignored
```
