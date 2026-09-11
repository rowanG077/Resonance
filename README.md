<p align="center"><img src="resources/logo.svg" alt="Resonance" width="640"></p>

Resonance is a Rust reimplementation of the GameCube version of Tales of
Symphonia, built from the ground up using Bevy. The project aims to:

- Preserve the game's visuals, audio and gameplay with high fidelity on modern platforms.
- Make textures, meshes and other assets easy to replace.
- Support editing existing events and skits, and creating new ones.
- Enable players to edit fields and build new areas, storylines and content.

The current playable route runs from startup through New Game and the classroom,
including NPC conversations and the doorway event. Broader game coverage and
modding tools remain development goals. The connected school grounds and village
are under development; their visuals, interactions and save menus are unfinished.

## Build and cook

The importer extracts data from your original GameCube disc and converts
("cooks") it into assets Resonance can load directly. The game then runs from
these cooked files.

```sh
nix develop
cargo build --workspace
target/debug/resonance-import extract --disc /path/to/Disc1.rvz --output local/extracted/disc1
target/debug/resonance-import cook-title --extracted local/extracted/disc1
target/debug/resonance-import cook-boot
target/debug/resonance-import cook-title-audio --coefficients /path/to/Dolphin/Sys/GC/dsp_coef.bin
target/debug/resonance-import cook-title-sounds --coefficients /path/to/Dolphin/Sys/GC/dsp_coef.bin
target/debug/resonance-import cook-intro
target/debug/resonance-import cook-story-intro
target/debug/resonance-import cook-classroom
target/debug/resonance-import cook-classroom-audio --coefficients /path/to/Dolphin/Sys/GC/dsp_coef.bin
target/debug/resonance-import cook-field --map 332
target/debug/resonance-import cook-field-audio --map 332 --coefficients /path/to/Dolphin/Sys/GC/dsp_coef.bin
target/debug/resonance-import cook-field --map 330
target/debug/resonance-import cook-field-audio --map 330 --coefficients /path/to/Dolphin/Sys/GC/dsp_coef.bin
```

The current import profile supports North American disc 1, GQSEAF revision 0.
Extraction is a one-time step. Cooks reuse valid outputs and default to
`local/extracted/disc1` and `local/cooked`; inspect each command's `--help` for
other paths. Keep discs, extracted files, cooked assets and recordings in the
ignored `local/` directory. All cooking is Rust, with established native codec
helpers supplied by the [development flake](flake.nix).
The importer parses original databases into validated, editable JSON, including
recipes, ingredients, item statistics, EX skill definitions and menu settings in
`game/menu-data.json`.
Executable addresses and packed table layouts stay inside the importer; the
player reads the converted records.

Field cooking includes skit scripts, animated portraits and media timing. To
refresh an existing installation, run `target/debug/resonance-import cook-skits`.
The English disc's skit tracks are silent; cooking verifies that and retains their
duration without storing silent audio files. Refresh existing menu assets with
`target/debug/resonance-import cook-menu`.
Refresh older shared field effects with `target/debug/resonance-import cook-effects`;
this also updates their preload manifests.
`target/debug/resonance-import cook-monsters` prepares the enemy catalogue as
named JSON records with converted meshes, textures and idle animation clips.
`cook-menu` includes those records and prepares any missing monster assets.
For an older cook, also repeat the classroom and field audio commands above to
include item recovery and menu paging sounds.

## Play

```sh
cargo run -p resonance -- --silent
cargo run --release -p resonance -- --silent --resolution 1920x1080
```

`--silent` permanently mutes speaker output, including after device recovery.
Omit it to hear the game. Use `--assets PATH` for another cooked directory and
`--skip-intro` to enter the title directly. The classroom's opening speech is
intentionally shown over black; advance it to reveal the room.

Resolution defaults to 640×480 and stays fixed until restart. Resizing and
maximizing are disabled; compositor-imposed sizes letterbox the retained image.
Wide resolutions expand the camera horizontally and keep UI in a centered 4:3
area. Gameplay renders without an application cap; movies follow their audio
clock. Simulation remains at 60000/1001 updates per second.

| Action | Keyboard | Controller |
|---|---|---|
| Menu selection / movement | Arrows; WASD also moves in fields | Left stick / D-pad |
| Confirm / advance / talk | Enter; Space also works in fields/setup | South button |
| Run | Shift | East button |
| Field menu / back | Tab / Escape | North / East button |
| Inventory category / menu character / recipe page | Q / E | Left / right shoulder |
| Discard item / remove armor / cook dish | X | West button |
| Equip sorting / Optimal (character selected) | Tab | North button |
| Choose field leader / exchange party members | Enter / Tab, with a party member selected | South / North button |
| Toggle party statistics | Home | Start |
| Cycle control type in the Tech header | Home | Start |
| Edit an AI member's U. Attack shortcuts in the Tech header | Tab | North button |
| Status page | Q / E | Left / right shoulder |
| Items / book / map list / Status / recipe page | Page Up / Page Down | Right stick up / down |
| Rotate / zoom Monster List preview | [ and ] / Page Up and Page Down | Right stick |
| Reset Monster List preview | Home | Start |
| Open announced skit | Z | Z trigger (when its playback resource is cooked) |
| Skip movie | Enter / Escape | Start |
| Pause movie | Space | — |
| Performance overlay / snapshot | F3 / F4 | — |
| Development quicksave / quickload | F5 / F9 | — |

From the menu's bottom row, press Down to select the party. Field leader and
formation order are saved independently; Escape cancels a pending exchange.
In Items, Q/E changes category while the item list has focus; Left/Right selects
category tabs. Enter or Down enters a category at its first item, and Page Up/Down
jumps a full page.
Open the Collector's Book from Items → Key Items. It records discovered items
even after they leave inventory, with completion percentages for each category.
Q/E changes its category; Page Up/Down scrolls a full page.
Owned world maps also open from Key Items. Select a visited location to browse its
shops; stock is shown for shops you have visited. Enter opens each list and Escape
returns to the previous one.
The owned Monster List opens from Key Items. Left/Right changes the monster,
Q/E jumps ten entries, Up/Down changes discovered repeat-battle statistics,
and X opens the selection list. Unscanned statistics remain hidden.
The owned Training Manual opens from Key Items and shows learned topics.
Up/Down selects a chapter or topic, Enter opens a chapter, and Page Up/Down
changes the reading paragraph. Escape returns to chapters, then Items.
The owned Figurine Book opens from Key Items once you have collected a figurine.
Up/Down selects a collected figurine, Page Up/Down jumps a page, and Escape returns
to Items.

Quicksaves work during free field movement. They restore progress and the player's
location by restarting the field; animations and music start afresh. Use
`--quick-slot NAME` for independent slots and `--save-directory PATH` to override
the platform's user-data directory. Completion and errors appear in the window
title and console. F9 also requires free field control.
Events, dialogue, menus, movies and transitions reject quicksaves immediately;
requests are never deferred until control returns. At a memory circle, Confirm
opens Save. The field menu's System page provides save/load with separate
filesystem slots. The title's Load option opens the same slots after restarting
the game. Menu artwork is still being matched to Dolphin.
System → Customize edits saved preferences. Escape applies the draft and returns
to the main menu; its Cancel action discards edits, and Default restores defaults.
Music volume previews while editing; effect levels and stereo apply when you leave Customize.
In Adjust Screen, Home (or controller Start) resets the offset. Resolution remains
a startup option.
Development checkpoints are tied to the session data and scripts of the supported
fields; changing that content invalidates older checkpoints.

```sh
cargo run -p resonance -- --silent --save-directory local/saves --quick-slot classroom
cargo run -p resonance -- --silent --load local/saves/quicksaves/slot-classroom.json
```

## Development and validation

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

Development builds optimize every crate at level 2 with debug assertions and
overflow checks. Use release builds for performance measurements. Tests marked
`ignored` require locally cooked assets or independent Dolphin recordings.
The supported route has been exercised on ARM Linux; Windows, Steam Deck and
Apple Silicon macOS hardware validation remains pending. Intel macOS is out
of scope.

- [Plan and milestone scope](PLAN.md)
- [SymphoniaScript and native registration](docs/symphonia-script.md)
- [Audio/video ownership and clocks](docs/audio-video-architecture.md)
- [Field preparation](docs/field-preloading.md)
- [Performance measurements and silent probes](docs/performance.md)
- [Dolphin capture, replay and comparison](tools/oracle/README.md)
- [Offline media tools](tools/media/README.md)

Keep reusable test inputs in version control. Generated reports, logs, hashes
of individual runs and screenshots belong under `local/`; update the relevant
current guide when behavior changes.

## License

Resonance is licensed under the [MIT License](LICENSE).

Original game assets and third-party dependencies retain their respective
rights and licenses.
