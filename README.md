<p align="center"><img src="resources/logo.svg" alt="Resonance" width="640"></p>

Resonance is a Rust reimplementation of the GameCube version of Tales of
Symphonia, built from the ground up using Bevy. The project aims to:

- Preserve the game's visuals, audio and gameplay with high fidelity on modern platforms.
- Make textures, meshes and other assets easy to replace.
- Support editing existing events and skits, and creating new ones.
- Enable players to edit fields and build new areas, storylines and content.

The playable route runs from startup through New Game, the classroom and Iselia's
connected grounds, village and interiors, including Halo's shop and the Genis
cooking tutorial. Milestone 3 adds field menus, ordinary saves and quicksaves,
skits and Dolphin regression coverage for this route. Broader game coverage and
modding tools remain in development.

## Build and cook

The importer extracts data from your original GameCube disc and converts
("cooks") it into assets Resonance can load directly. The game then runs from
these cooked files.

```sh
nix develop
cargo build --workspace
target/debug/resonance-import extract --disc /path/to/Disc1.rvz --output local/extracted/disc1
target/debug/resonance-import extract --disc /path/to/Disc2.rvz --output local/extracted/disc2
target/debug/resonance-import cook-title --extracted local/extracted/disc1
target/debug/resonance-import cook-boot
target/debug/resonance-import cook-title-audio --coefficients /path/to/Dolphin/Sys/GC/dsp_coef.bin
target/debug/resonance-import cook-title-sounds --coefficients /path/to/Dolphin/Sys/GC/dsp_coef.bin
target/debug/resonance-import cook-intro
target/debug/resonance-import cook-story-intro
target/debug/resonance-import cook-classroom
target/debug/resonance-import cook-classroom-audio --coefficients /path/to/Dolphin/Sys/GC/dsp_coef.bin
for map in 330 331 332 333 334 335 336 337 338 339; do
  target/debug/resonance-import cook-field --map "$map"
  target/debug/resonance-import cook-field-audio --map "$map" \
    --coefficients /path/to/Dolphin/Sys/GC/dsp_coef.bin \
    --additional-disc local/extracted/disc2
done
target/debug/resonance-import validate-shops --json local/shop-inventories.json
```

The current import profile supports North American GQSEAF revision 0.
Disc 2 supplies later-story voices referenced by Colette's house; the importer
checks the primary disc first and uses `--additional-disc` for missing archives.
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

Field cooking includes skit scripts, animated portraits and media timing. The
English disc's skit tracks are silent; cooking verifies that and retains their
duration without storing silent audio files. Shared content can be refreshed with
`cook-skits`, `cook-menu`, `cook-text` and `cook-effects`; dependent field manifests
refresh with them. After a separate `cook-monsters` run, use `cook-menu` to refresh
embedded catalogue records. Field audio includes every declared script
branch and shared menu sounds. Repeat the cooking commands after importer changes.

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
Owned books and world maps open from Items → Key Items. They show discovered
items, monsters, figurines, learned topics and visited locations/shops. Lists use
the navigation controls above; the Monster List also uses Left/Right to select,
Q/E to jump ten entries, Up/Down for repeat-battle statistics and X for its list.

Quicksaves work during free field movement. They restore progress and the player's
location by restarting the field; animations and music start afresh. Use
`--quick-slot NAME` for independent slots and `--save-directory PATH` to override
the platform's user-data directory. Completion and errors appear in the window
title and console. F9 also requires free field control.
Events, dialogue, menus, movies and transitions reject quicksaves immediately;
requests are never deferred until control returns. At a memory circle, Confirm
opens Save. The field menu's System page provides save/load with separate
filesystem slots. The title's Load option opens the same slots after restarting
the game. Ordinary saves and quicksaves share the same underlying format.
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
