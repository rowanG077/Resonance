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
elemental variants, field mappings/foraging and EX skills. Field scenarios,
character and enemy bodies, the remaining game/menu rules and media still need
porting. Use a separate output directory while those owners are missing.

The added native techniques receive ordinary logical IDs after the common
GameCube elemental variants. Named references resolve during import. The shared
technique menu retains six forgetting alternatives, including the original four
from GameCube. Neither source requires a runtime edition flag.

Run parser tests with `cargo test -p resonance-steam-importer`. The focused installed
source test also loads the generated documents with the current content readers,
checks the two added Niflheim trios, all five text languages and overwriting an
interrupted cook:

```sh
RESONANCE_TEST_STEAM="/path/to/Tales of Symphonia" cargo test -p resonance-steam-importer installed_categories_load -- --ignored
```
