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

Movie conversion also needs `ffmpeg` on `PATH`, with the USM demuxer (tested with
FFmpeg 9.0.2). It runs only during importing; the game uses its existing Rust
FFV1/FLAC player.

**This is an incremental port, not yet a complete playable library.** The last full run
completed the first 37 passes in 227 seconds and wrote entry points for all
509 available fields. Nine field resource references are still unavailable or
unconverted. The classroom and Asgard field documents assemble through the shared
reader. The shared gameplay loader now initializes a party and saves/restores the
added collection IDs. Day/night title preparation, 120 script updates and a
headless GPU capture at tick 120 now pass the existing shared title test. The
capture still has backdrop gaps and unresolved decorative bone lookups. Later
fields and battle remain unfinished. Thirteen additional passes now cook
skits, portraits, movies, field service models, normal attacks, party speech,
martial actions, party profiles, resident projectiles, Defense System rules,
battle audio, Unison rules and field audio against that library. Native
opening-field execution reaches player control. The shared checkpoint check
passes save/reload, movement, invalid-load rejection and early village/classroom
transitions using this cooked library.

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
Field scenes also use the GC importer's shared UV controller recipes, including
the day/night title controllers required by the common title loader.
Local picture resources and texture banks write the same overlay catalogues as
GC; the installed check loads 57 banks. Steam's removed night-title menu texture
uses a transparent handle because the common startup UI owns that lettering.

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
enemy documents, remaining game/menu rules and other media also
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
when cooked; retained GC streams keep their authored GC clock. Script seek times
still need conversion with their animation owners. Packed sound IDs are lowered
to common cue IDs using ordinary bytecode expressions at import time. The
input/sound converters share branch and event relocation; no VM operations or
source-specific runtime handlers are added.

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
The enemy media owner now retains all 4,759 declared body-motion slots across the
257 enemies, with matching outline clips, secondary-body motion ranges and 44
carried layers with authored idle/copied-pose clips. It uses the same GLB writers
and `model.json` schema as the GC owner, preserving idle selection when the full
motion table is added. The installed cook checks every declared body/outline slot
and prepares eight representative models through the shared reader, including
Golem, Vidarr, a cameo rig and the added final boss. Enemy rule/artwork bindings,
extra effect models and secondary-motion/visibility details remain separate work.
No engine change or source-edition flag was needed.

The shared engine uses catalogue-derived collection counts and 16-bit monster
identities through knowledge, saves, rewards, AI and model/voice lookups. Existing
GC numeric IDs and JSON document shapes remain compatible. Counts are derived
from records rather than written as separate authoring settings.

Native battle-party costume mappings preserve all seven slots and the aliases
in the actual battle-package directory, rather than assuming GC's costume aliases.
Party battle models now use the same `party.json` documents as GC: 63 standard
selections, 3,036 motion bindings, explicit null slots, weapon attachment targets,
Colette's animated wings and Lloyd's duel clip. Body/outline geometry is shared
with fields when the native model descriptor agrees; identical rigs and source
motions share GLBs across costumes. All selections prepare through the common
battle-model reader with strict diagnostics. This pass adds no engine behavior.
Story override banks, expressions, secondary motion, victory performances,
callback-driven arte rules still need their owners.

All 63 directional normal attacks now use the GC normal-attack decoder and its
named document writer. The Steam owner only normalizes numeric storage, voice
commands and sound identities. It retains the 61 ordinary contacts, thrown
weapons, poses, movement, combo/recovery ages and weapon visibility. The native
GC party-table regression also passes through this shared decoder.

The 18 party ABNF banks write 223 ordinary WAV recordings per voice language,
using the existing ADX converter and speech mappings. Every voice referenced by
the prepared normal and martial attacks resolves in both English and Japanese.

The martial pass retains all 125 nonempty native members and prepares 146 action
tracks through the GC decoder and named document writer, including the added
ordinary sword/kick artes and Colette's hammer/theft branches. Named documents
round-trip every decoded field through the shared reader. One small shared voice
choice event preserves the weighted lines actually used by these tracks. Native
voice widths, packed sound IDs and executable launch constants stay in the importer;
there is no runtime edition flag. Grand Fall, five non-track callback members and
six added callback-driven actions remain explicitly unsupported. Battle artwork
and encounter owners still need conversion before battles play.

All nine party profiles now reuse the GC actor-profile decoder and named party
rules writer. Source-owned strategies, item offsets, contact cues, death pairs,
chant poses, 113 casting voice overrides and Over Limit voices enter the existing
schema. The maintained standing capsules, companion positioning and battle-entry
presentation follow GC. Rescue captions reuse the localized item and EX documents.
Steam's retained resident voice-duration table still indexes GC clips, so casting
clocks come from the converted recordings at the common 30 Hz rate. The shared
encounter reader loads the result, all nine default bodies bind their profile
anchors, and all 502 referenced voice recordings resolve in both English and Japanese. Extra character voices are ordinary optional profile
fields, with original fallbacks for older libraries. Elemental contact cues use the
same u16 IDs as other sounds; neither change introduces an edition mode.
The resident projectile owner normalizes all 26 templates into the GC decoder,
then writes the same projectile table, six resident spell rules, eight support
clocks and Fire Ball launch parameters. The named Defense System definition
includes its native repair/deployment timings and localized messages. Battle audio
uses ordinary mappings for 20 common cues, preserving existing field/menu service
bindings, recording sample rates and the shared spatial controls. The installed
check loads these documents through the common readers and prepares/plays their
recordings through the existing effect player. No engine change was needed.

Unison uses the shared GC recipe and opening-strike decoders and the existing
named document writer. It retains Steam's 29 ordered recipes (26 action owners),
all nine openings, 254 native technique placements and authored voices; added
techniques use their existing normalized logical IDs. The mirrored prelude is
cooked into the GC layout. All 100 physical stored-scene identities are checked
against installed package names and written to the common mappings, independently
of their pending artwork owner. The shared battle-rule capture now loads every
rule owner together; both voice languages and opening contact cues resolve. The
real GC Unison regression also passes. No runtime edition flag or engine behavior
was added. Scene tracks/artwork, enemies, equipment and battle environments still
need their owners before encounters are playable.

The current Steam library passes shared gameplay loading, fresh-party creation,
save/load of monster 256 and figurine 296, and native collection-completion calls.
A temporary French overlay also loads through that same path with UTF-8 names.

The audio passes decode the installed AWB music and both voice languages through
the GC importer's existing ADX decoder. They write ordinary PCM16 WAVs, standard
RIFF loop metadata and the same music/speech mappings as GC. English is the
default speech mapping; the Japanese mapping references its own ordinary files.
Battle speech preserves the main bank's low-ID aliases and the legacy bank's
additional tail. The unbacked `tos_ending.adx` music declaration remains for the
ending-media owner; it is not replaced with another recording.

All 67 PHD/PBD effect banks and the streamed-effect bank use shared recorded-effect
documents. Native packed effect IDs are assigned ordinary positive catalogue IDs;
menu and world service cues retain the shared IDs. Menu actions point directly to
the authored SYS recordings. The common mix keeps recording buses dry; native
acoustics, effect chaining/chance/pitch and enemy voice banks still need their
owners. No source audio codec is added to the engine.
The base audio check loads 111 mapped music tracks (104 with authored loops),
27,652 speech bindings and 1,563 effects, and renders the menu cues through the
shared mixer. Field and party passes add their cue and voice bindings afterward.
Library format 103 moves recorded music's volume curve into
`audio/mix.json` for both importers, so WAV playback no longer needs synthesis
tables. Older libraries require recooking.

```sh
RESONANCE_TEST_STEAM="/path/to/Tales of Symphonia" cargo test -p resonance-steam-importer installed_catalogues_and_models -- --ignored
RESONANCE_TEST_LIBRARY="/absolute/path/to/steam-preview" cargo test -p resonance-steam-importer cooked_gameplay_definitions -- --ignored
RESONANCE_TEST_STEAM="/path/to/Tales of Symphonia" RESONANCE_TEST_LIBRARY="/absolute/path/to/steam-preview" cargo test -p resonance-steam-importer installed_localized_gameplay -- --ignored
RESONANCE_TEST_LIBRARY="/absolute/path/to/steam-preview" cargo test -p resonance-steam-importer cooked_party_battle_models -- --ignored
RESONANCE_TEST_STEAM="/path/to/Tales of Symphonia" RESONANCE_TEST_AUDIO_OUTPUT="/absolute/path/to/steam-preview" cargo test -p resonance-steam-importer installed_audio_recordings -- --ignored
RESONANCE_TEST_LIBRARY="/absolute/path/to/steam-preview" cargo test -p resonance-steam-importer cooked_audio_recordings -- --ignored
RESONANCE_TEST_STEAM="/path/to/Tales of Symphonia" RESONANCE_TEST_LIBRARY="/absolute/path/to/steam-preview" cargo test -p resonance-steam-importer installed_field_overlays -- --ignored
RESONANCE_TEST_ASSETS="/absolute/path/to/steam-preview" RESONANCE_NAMED_TITLE_CAPTURE="/absolute/path/to/title.png" cargo test -p resonance-presentation original_named_title_prepares_both_backgrounds -- --ignored
```

The complete goal remains a Steam-only cook that loads through the same library
readers and engine as the GC output, with GameCube-style presentation. Remaining
owners include remaining game rules and battle artwork; legacy field motion, automatic accessories
and dynamic shared banks;
callback-driven arte rules and victory performances; enemy/equipment battle models and rules;
overworld data and media;
battle/world audio bindings and remaining startup/ending media. Completion requires exercising startup,
a field and battle from that library. The table and field-source checks alone do
not establish that end state.

The skit owner writes 471 scenarios, 343 availability rules, 106 portraits and
2,768 timed media entries into the shared skit documents. Portrait images are
normalized to GC dimensions. English skits retain the native muted voice clocks;
Japanese recordings remain ordinary selectable speech files. Cooking-result,
chef and mild-poison predicates use the shared party state.

The movie owner converts all 13 shipped script movies (25 native IDs) into the
same named Matroska format as GC, keeping both audio tracks and putting English
first for voiced scenes. The separate ending movie still needs its owner.
Field entry points bind movie IDs from their scripts. Save circles and the three
treasure types use the common actor model writer and native service selections;
their fields bind the ordinary service resource IDs.

Field entry points now select native music, speech and effects through the shared
audio documents. Dynamic selectors bind the corresponding published catalogue.
The installed check resolves audio in all 509 fields: 492 music owners, 147
speech owners and 21,558 effect selections.
All 32 shared service cues select their source recordings, including ring modes,
doors, treasure, recovery and menu actions. Native out-of-range PHD selectors
retain the executable's no-audio behavior through the existing explicit silence
recipe. Missing valid recordings remain errors. The binding pass reuses cooked
audio and does not decode its recordings again.
The shared field-mixer check also plays the Iselia recordings across classroom,
village and opening-scene handoffs, preserving music PCM and retiring old voices.

```sh
RESONANCE_TEST_STEAM="/path/to/Tales of Symphonia" RESONANCE_TEST_LIBRARY="/absolute/path/to/steam-preview" cargo test -p resonance-steam-importer installed_skits -- --ignored
RESONANCE_TEST_STEAM="/path/to/Tales of Symphonia" RESONANCE_TEST_LIBRARY="/absolute/path/to/steam-preview" cargo test -p resonance-steam-importer installed_movies -- --ignored
RESONANCE_TEST_STEAM="/path/to/Tales of Symphonia" RESONANCE_TEST_LIBRARY="/absolute/path/to/steam-preview" cargo test -p resonance-steam-importer installed_field_services -- --ignored
RESONANCE_TEST_STEAM="/path/to/Tales of Symphonia" RESONANCE_TEST_LIBRARY="/absolute/path/to/steam-preview" cargo test -p resonance-steam-importer installed_field_audio -- --ignored
RESONANCE_TEST_STEAM="/path/to/Tales of Symphonia" RESONANCE_TEST_LIBRARY="/absolute/path/to/steam-preview" cargo test -p resonance-steam-importer installed_normal_attacks -- --ignored
RESONANCE_TEST_STEAM="/path/to/Tales of Symphonia" RESONANCE_TEST_LIBRARY="/absolute/path/to/steam-preview" cargo test -p resonance-steam-importer installed_party_speech -- --ignored
```

For a binding-only rerun after the current importer has cooked the scenarios,
set `RESONANCE_TEST_REUSE_SCENARIOS=1`; this retains their scripts and geometry.

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
