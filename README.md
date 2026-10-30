<p align="center"><img src="resources/logo.svg" alt="Resonance" width="640"></p>

Resonance is a Rust reimplementation of the GameCube version of Tales of
Symphonia, built with Bevy. Its goals are:

- Faithful gameplay, visuals and audio on modern platforms.
- Human-readable, editable assets for modding.
- Support for creating new fields, events, skits and other content.

## Clone and build

With Git and Nix installed:

```sh
git clone https://github.com/rowanG077/Resonance.git resonance
cd resonance
nix develop
cargo build --release -p resonance -p resonance-import-cli
```

## Import assets and play

You need both North American GameCube disc images (GQSEAF revision 0):

```sh
target/release/resonance-import /path/to/Disc1.rvz /path/to/Disc2.rvz
target/release/resonance
```

The importer writes to `local/all-assets`, which must be empty before importing.
The game loads that directory by default.

Move with WASD or the arrow keys, confirm with Enter, and open the field menu
with Tab. Gamepads are also supported.

See the [cooking guide](docs/cooking.md) for import options and
[editing guide](docs/editable-content.md) for modding.
