# Generated definitions

The source of truth for known numeric IDs is
[`symphonia-constants`](../../crates/symphonia-constants/src/lib.rs).
Cooking generates `.sym` enums from those Rust declarations. There are no checked-in
copies of the generated definitions. The development script checker generates the
same standard modules in memory; with `--assets`, it reads the cooked definitions.

For example:

```text
use std::fields::FieldId;
use std::items::ItemId;
use std::music::MusicId;

// Native services still accept numeric IDs, including mod-defined ones.
// i32(ItemId::MemoryGem) is 491.
```

Modules cover characters, fields, music, items, monsters, figurines, world
landmarks, travel, story flags, globals, quest milestones and shared sound cues.
Field IDs and world landmark IDs are separate types. Track IDs and music bank
IDs are separate types. Unidentified flags do not need thousands of placeholder
constants: scripts can still use a raw number or declare their own constant.

The initial resource name belongs to the same Rust declaration as its ID. Cooked
`mappings.json` files remain editable; enum names never override a mod's mappings
or localized display text. Source adapters for future editions must translate
any differing IDs here, without adding platform metadata to cooked assets.

Model node names are strings, not numeric game IDs. The GLB skeleton is their
source of truth. Only the five semantic aliases used by shipped preview behaviors
are maintained in `symphonia_constants::MODEL_NODES` and generated as
`std::model_nodes`. Cooking does not produce thousands of unused node catalogues.

To generate an optional reference module for an edited model:

```sh
cargo run -p resonance-script -- nodes path/to/body.glb > my_model_nodes.sym
```

Repeated labels use `#0`, `#1` occurrence selectors; a literal `#` is escaped as
`##`. Put the output in your script project and import it like any other library
module, or use a node's label directly as a string in `model::node`.

Cooked scripts can be edited directly or overridden at the same path in a mod.
Changing Rust constants changes the defaults for future cooks. Runtime loads the
cooked definitions and has no embedded fallback or ID whitelist for resources.
