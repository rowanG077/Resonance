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
elemental variants, field mappings/foraging, EX skills, and scripts/messages/collision for all 509
available field packages. Field scenery and actors, character and enemy bodies,
the remaining game/menu rules and media still need porting. Use a separate output directory while those owners are missing.

The added native techniques receive ordinary logical IDs after the common
GameCube elemental variants. Named references resolve during import. The shared
technique menu retains six forgetting alternatives, including the original four
from GameCube. Neither source requires a runtime edition flag. Field input checks
are adapted with ordinary bytecode expressions around the existing GC input native, including
Steam's raw-press mode. Branch targets, event entries and message offsets are
preserved when code grows. Source text encodings disappear into Unicode messages.
Animation-clock and sound-ID conventions still need conversion with their media
owners.

Run parser tests with `cargo test -p resonance-steam-importer`. The focused installed
source test also loads the generated documents with the current content readers,
checks the two added Niflheim trios, all five text languages and overwriting an
interrupted cook:

```sh
RESONANCE_TEST_STEAM="/path/to/Tales of Symphonia" cargo test -p resonance-steam-importer installed_categories_load -- --ignored
```

The complete goal remains a Steam-only cook that loads through the same library
readers and engine as the GC output, with GameCube-style presentation. Remaining
owners include game/menu rules and artwork; field scenery, actors and settings;
party/enemy/equipment models and battle rules; overworld data and media; skits,
audio, movies and startup/ending assets. Completion requires exercising startup,
a field and battle from that library. The table and field-source checks alone do
not establish that end state.

For a focused field-source conversion/load check:

```sh
RESONANCE_TEST_STEAM="/path/to/Tales of Symphonia" RESONANCE_TEST_STEAM_FIELD=332 cargo test -p resonance-steam-importer installed_field_sources_convert -- --ignored
```

Omit `RESONANCE_TEST_STEAM_FIELD` to convert and read all available field scripts,
message documents and collision meshes. This check does not execute gameplay.
