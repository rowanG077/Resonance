# Gameplay scripts and generated definitions

The `.sym` files here are authored behavior. Known numeric game identities live
in the [`symphonia-constants`](../crates/symphonia-constants/src/lib.rs) crate.
Cooking generates their [standard definitions](std/README.md) and copies the
behavior sources into the asset library. Generated definitions are not checked in.

`world/rules.sym` owns overworld entry, landmark visibility, discovery conditions,
portal visibility and travel music. `world/terrain.sym` selects terrain variants.
The helpers import generated types for world locations, items, flags, milestones,
marker models and travel state, converting to `i32` at native service boundaries.

`world/caravan.sym` owns its route, story conditions and camp appearances.
Its `CaravanStopId` uses the shared landmark discriminants and a saved script
state. The engine runs its entry hook once per actual world entry and preserves
state during refreshes and checkpoint restoration.

`field/treasure.sym` owns chest choreography, sounds, rewards and acknowledgement.
`field/memory.sym` owns the save-circle tutorial and unlock dialogue.
`preview/*.sym` supplies instance-local model behavior; its binding file selects
catalogue records and entry functions. Shared meshes and animations are unchanged
by behavior execution.

```sh
cargo run -p resonance-script -- check --host world scripts world::rules
cargo run -p resonance-script -- check scripts field::treasure field::station field::memory
cargo run -p resonance-script -- check scripts preview::sword_dancer
cargo run -p resonance-script -- api model
cargo run -p resonance-script -- fmt --check scripts
```

The checker generates standard definitions from Rust by default. With `--assets`,
it uses that library's cooked standard definitions exclusively:

```sh
cargo run -p resonance-script -- check --assets local/cooked scripts preview::sword_dancer
```

Cooking writes the defaults to `<assets>/scripts`. Those files are human-editable
and can be overridden in a mod directory. Runtime prepares the cooked copies
before activation, with no embedded fallback and no source I/O during gameplay.
Development field-entry scripts can also be supplied with `--scripts ROOT`; see
[the language documentation](../docs/symphonia-script.md#running-authored-field-entries).
Scenario bytecode readability is a separate, deferred task.
