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

**This is an incremental port, not yet a playable library.** Current passes write
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

Field actors, enemy documents, remaining game/menu rules and other media still
need porting. Material animation, face atlas selection and animated visibility
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
Native skeletal motion timestamps are converted from 60 Hz to the common 30 Hz
clock when cooked. Script seek times and sound-ID conventions still need
conversion with their media owners.

Run parser tests with `cargo test -p resonance-steam-importer`. The focused installed
source test also loads the generated documents with the current content readers,
checks the two added Niflheim trios, all five text languages and overwriting an
interrupted cook:

```sh
RESONANCE_TEST_STEAM="/path/to/Tales of Symphonia" cargo test -p resonance-steam-importer installed_categories_load -- --ignored
```

The complete goal remains a Steam-only cook that loads through the same library
readers and engine as the GC output, with GameCube-style presentation. Remaining
owners include game/menu rules and artwork; field actors and shared banks;
enemy/equipment models and battle rules; overworld data and media; skits,
audio, movies and startup/ending assets. Completion requires exercising startup,
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
current scene/material grouping schema. Their shared settings and image references are also cooked. Field actor and
shared animation bank owners are still pending; this is not yet a field startup/playability
check.

To cook party models and sample their converted motions through the shared reader:

```sh
RESONANCE_TEST_STEAM="/path/to/Tales of Symphonia" cargo test -p resonance-steam-importer installed_party_models_and_motions -- --ignored
```
