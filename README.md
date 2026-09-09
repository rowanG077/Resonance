<p align="center"><img src="resources/logo.svg" alt="Resonance" width="640"></p>

Resonance is a Rust reimplementation of the GameCube version of Tales of
Symphonia, built from the ground up using Bevy. The project aims to:

- Preserve the game's visuals, audio and gameplay with high fidelity on modern platforms.
- Make textures, meshes and other assets easy to replace.
- Support editing existing events and skits, and creating new ones.
- Enable players to edit fields and build new areas, storylines and content.

The current playable route runs from startup through New Game and the classroom,
including NPC conversations and the doorway event. Broader game coverage and
modding tools remain development goals.

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
target/debug/resonance-import cook-title-sounds
target/debug/resonance-import cook-intro
target/debug/resonance-import cook-story-intro
target/debug/resonance-import cook-classroom
target/debug/resonance-import cook-classroom-audio --coefficients /path/to/Dolphin/Sys/GC/dsp_coef.bin
```

The current import profile supports North American disc 1, GQSEAF revision 0.
Extraction is a one-time step. Cooks reuse valid outputs and default to
`local/extracted/disc1` and `local/cooked`; inspect each command's `--help` for
other paths. Keep discs, extracted files, cooked assets and recordings in the
ignored `local/` directory. All cooking is Rust, with established native codec
helpers supplied by the [development flake](flake.nix).

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
| Skip movie | Enter / Escape | Start |
| Pause movie | Space | — |
| Performance overlay / snapshot | F3 / F4 | — |

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
