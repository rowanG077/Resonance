# Authored gameplay scripts

`world/rules.sym` owns the generic overworld entry hook, landmark visibility, discovery
conditions, portal visibility and travel music. `world/terrain.sym` selects the
alternate terrain tiles in one pass; portal interaction uses the scripted visibility.
The data modules use separate ADTs for locations, items, flags, story stages, quest steps,
marker models and travel state. Unidentified values are named `UnknownId…`;
their names do not speculate about a cutscene or quest meaning. Each enum declares
its numeric IDs once, for example `Caravan = 11`. Explicit
`i32(value)` conversions are confined to data access; rule functions use the ADTs.

`world/caravan.sym` owns its route, story conditions and camp appearances. Its
`state stop: Stop = Stop::Triet;` declaration supplies a typed default and a saved
value. `on_enter` advances it on eligible Sylvarant entries; `refresh` reads it to
show the active camp. Rust calls `world::rules::on_enter(world)` once per genuine
world entry, and preserves state during refreshes and checkpoint restoration.
There is no caravan service or route field in the gameplay engine. Story and quest
variables are read directly from persistent globals; Rust has no duplicate list of
quest-specific fields.

`field/treasure.sym` owns opening/closing choreography, sounds, item/gald reward
branches, receipt wording and acknowledgement. Native `Reward` and `Kind` ADTs keep
item IDs, currency and chest styles separate. Rust supplies target selection,
validated handles, inventory operations, animation sampling and dialogue services.
The existing event scheduler supplies suspension, cancellation and input ownership.
The Rust ring controller owns casting, targeting, puzzle callbacks and recovery.
It emits trails, impacts and pulses through the shared particle runtime.

```sh
cargo run -p resonance-script -- check --host world scripts world::rules
cargo run -p resonance-script -- check scripts field::treasure field::station
```

These modules follow the same immutable cooking and preparation path as model
scripts: cook-all publishes them, inventories hash them, and scene preparation
compiles and validates them before activation. There is no embedded runtime
fallback or source I/O while playing. Field receipts validate all literal and
substitution glyphs before entry. World rules run synchronously with a bounded
instruction budget; a failed refresh does not publish partial landmark changes.

`.sym` files are maintained source. Each starts with `script field;`,
`script model;`, or `script library;`. The checker selects the native API from
that declaration; static assets and message glyphs are validated before events start.

```sh
cargo run -p resonance-script -- check scripts preview::sword_dancer
cargo run -p resonance-script -- api model
cargo run -p resonance-script -- fmt --check scripts
```

`preview/*.sym` supplies instance-local model behavior. Its checked-in
binding manifest selects named catalogue records and parameterless entry functions.
Scripts import ordinary string and integer constants from the checked-in
[standard library](std/README.md). For example, `sword_dancer_191::LeftWing` is
`"Bone_hane01_L"` and `std::story::SwordDancerTailVisible` is `147`. Scripts can
define new constants using the same primitive types. The script-content crate
embeds the sources with `include_str!`; `cook-all` writes
them to `<assets>/scripts`. Runtime uses the verified immutable cooked copies,
with no embedded fallback. Change the checked-in sources, rebuild and recook to
update them; do not edit cooked resources. Mod overlays are future work.

```sh
cargo run -p resonance-script -- check --assets local/cooked scripts preview::sword_dancer
```

Model sources load through the field's normal integrity checks before any preview
opens. Development field-entry scripts live in a separate source project passed
with `--scripts ROOT`; see the
[language documentation](../docs/symphonia-script.md#running-authored-field-entries).
They do not override cooked model scripts. Shared meshes and motion clips remain
unchanged by model behavior.

The entire standard library lives in `std/`, including readable model node
constants and an index linking monster and figurine records to shared node modules.
Cooking copies these files unchanged. New model layouts do not require a library
entry to cook or run.
The checker uses checked-in sources by default. With `--assets`, and at runtime,
`std::` comes exclusively from cooked resources, ignoring any local replacements.
