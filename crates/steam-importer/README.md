# Steam importer

A parallel source importer based on the current [GameCube importer](../importer).
It reads an installed Steam copy and writes the same editable content contracts.
**GameCube rendering is the target; visual parity with Steam is an anti-goal.**

The coordinator has 63 direct category passes. A fresh Steam-only cook and
targeted field, combat and GPU captures exercise the shared engine. Cooking
success and executable actions are separate from visual acceptance. Original
GameCube references and Resonance renders of GameCube assets are tracked
separately; whole-game progression and complete visual fidelity remain unverified.

## Cook an installation

```sh
cargo run -p resonance-import-cli -- steam "/path/to/steamapps/common/Tales of Symphonia" \
  --output local/steam-preview --language english
```

The input is `TOS.exe` and the installed TOFHDB/TLDAT archives. A GameCube disc,
extraction or cooked library is unnecessary. Text languages are `english`
(default), `french`, `spanish`, `german` and `italian`. Localized resources override
the English base archive; executable text tables use the corresponding slot.
English and Japanese speech use ordinary recorded-audio mappings.

Movie conversion requires `ffmpeg` on `PATH` with USM demuxing support. It runs
only during importing; runtime playback uses the shared FFV1/FLAC player.
Use an output directory outside the Steam installation. Cooking overwrites owned
files, so keep authored modifications in separate mod directories. After a cooked
layout change, use a fresh output directory instead of mixing obsolete files into
the current library.

For focused development, recook one category in an existing library:

```sh
resonance-import steam "/path/to/Tales of Symphonia" --output local/steam-preview \
  --category enemy-effects --id 23 --id 245
resonance-import steam "/path/to/Tales of Symphonia" --output local/steam-preview \
  --category martial-attacks
```

Categories are `tables`, `field-scripts`, `party-models`, `martial-attacks`,
`enemy-models`, `enemy-effects`, `enemy-rules` and `resident-effects`. Dependencies
must already be cooked in the current library format. Enemy categories accept
repeated `--id`; `field-scripts` accepts one `--id`.

Scenery, actor bindings and audio require a full cook. Their final documents
combine several passes, including shared model aliases, event motions, service
actors, voice catalogues and field media selections.
Omit IDs to visit every owner in the category. Keep modifications in separate
mod directories, since selected owners are overwritten.
After updating callback presentation, recook `martial-attacks` to publish action
documents, technique bindings and the finishers' stored-skill artwork selection.

## Architecture

- `lib.rs` lists direct category writers in dependency order. Each writes its
  final named documents and media; there is no parallel publication framework.
- Archive, package, executable-address and source-encoding knowledge stays in this
  crate. Where native data differs only in storage or indices, normalize it and
  reuse the GameCube decoder and named-document writer.
- Preserve native gameplay rules and action timing. GC fidelity governs rendering
  style; intentional source timing differences remain part of that game's rules.
- Scripts normalize input, sound and native identifiers while preserving branch,
  event and message references. Both importers write UTF-8 text.
- Models and clips become standard GLBs; textures become KTX2; recordings become
  WAVs. TOANMB timing converts from 60 source units to the shared 30 Hz game clock.
  Retained GameCube motion streams reuse the existing converter.
- Equipment outlines share their primary indexed rig and pose. Owner-linked
  weapon clips include actual weapon channels, rather than unrelated body clips.
- The shared readers, VM, battle engine and renderer consume the result. Cooked
  documents carry semantic game identities, without runtime edition flags.

See [editable content](../../docs/editable-content.md) and the
[cooked library guide](../../docs/cooked-library.md) for the authoring contracts.
Actual gameplay differences still determine cross-library mod/save compatibility.
The [GC-to-PS2 Changes Guide](https://gamefaqs.gamespot.com/ps2/920235-tales-of-symphonia/faqs/32765)
is a useful overview of user-visible edition differences. Installed executable
and archive data remain authoritative for Steam identities and behavior.

## Implemented owners

These categories are wired into the production cook. Coverage within an owner
can still contain explicitly unavailable actions or source resources.

| Area | Published content |
| --- | --- |
| Core rules | Descriptor, common scripts, formations and encounter pools, recoil, items, techniques, EX skills, progression, recipes and titles |
| Fields | Directory, scripts/messages, collision, scenery, cameras, actors, shared and retained animation resources, overlays, service models and audio selections |
| Party media | Field and battle models, seven native costume slots, named selections, skeletal clips, attachment targets, outlines and wing bindings |
| Menus | Dialogue font/windows, item icons, menu artwork/captions, manual, name editor, options, strategy, Grade Shop, crafting, journal and world-map menu |
| Collections | Monster Book and figurine records with shared model previews |
| Startup and ending | Boot/title/defeat artwork, subtitles, movies, ending artwork, credits and ending audio |
| Skits and audio | Scenarios, availability rules, portraits, timed media, music, speech, effects, mix settings and battle/field cue mappings |
| Battle rules | Normal and martial attacks, profiles, projectiles, resident spells, Defense System, Unison, victory tracks and battle event scripts/actors |
| Battle media | Enemy rules/models/speech/effects, resident effects, stored scenes and skill artwork, equipment/trails/UV controls, arenas and local effects |
| Battle UI | GC layout/colors/motion, normalized HUD pages, bitmap font, nine portraits and localized command/status captions |
| World | Terrain, travel models/motions, movement and encounter rules, landmarks, guideposts and cinematics |

Callback actions, including Final Fury and Sword Rain: Tempest, publish ordinary
authored actions. Fairy Circle uses sampled scene paths and shared recovery
commands; Inspect Magic uses the existing scan result flow. Party secondary
motion reuses the GameCube chain binder, and costume-owned weapon props use
optional visual metadata without changing equipped items or combat attributes.

Source-specific color ranges, material layers, particle rates and native IDs are
normalized during import. The runtime consumes the same scene, effect, motion
and audio contracts as the GameCube library.

## Remaining work and limits

- Some actions and stored-scene participant commands remain explicitly
  unsupported. Diagnostics identify the owner and reason; successful publication
  does not establish correct choreography or effects.
- Some learned prototype slots have null phase handlers in the original games.
  The visual catalogue retains those bindings with primary-source evidence;
  they are neither missing executable effects nor visual passes.
- Some field resources are absent from the shipped installation or still lack a
  conversion. The cook reports those references instead of inventing replacements.
- Material/visibility controllers and specialized presentation still need
  fidelity work. The arte-title backing remains a source-media gap. Steam
  fonts/menu artwork cannot reproduce every GameCube asset exactly.
- These checks do not establish whole-game progression or full GameCube visual
  fidelity. The current visual pass covers executable party bindings, canonical
  Unison families and distinct enemy arte/effect families; minor texture
  differences are allowed. Exhaustive enemy-row checks, repeated basic attacks,
  full-game walkthroughs and mechanical equivalence sweeps are deferred.

Visual capture/review tooling is maintained separately from the importer.
Generated inventories, captures and review decisions live under `local/`,
outside the cooked library.

## Targeted validation

Run parser/conversion unit tests without installed assets:

```sh
cargo test -p resonance-steam-importer --offline
```

The optional integration tests below cover source conversion in all five text
languages, then gameplay/save admission, representative fields, battle models,
stored scenes, world content and audio through the shared readers. Source
conversion writes only to temporary output; cooked-library checks are read-only.

```sh
export RESONANCE_TEST_STEAM="/path/to/Tales of Symphonia"
export RESONANCE_TEST_LIBRARY="/absolute/path/to/steam-preview"

# Private source conversion, including authored martial actions.
cargo test -p resonance-steam-importer --offline installed_gameplay_menu_tables -- --ignored

# Validate the completed library without recooking it.
cargo test -p resonance-steam-importer --offline cooked_ -- --ignored
```

Runtime action regressions remain in `resonance-battle` and `resonance-game`.
In the repository's Nix environment, prefix Cargo commands with
`nix develop --offline -c`. These CPU checks do not establish visual fidelity or
whole-game progression.
