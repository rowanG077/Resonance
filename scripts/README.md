# Authored gameplay scripts

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
Cooking copies these files unchanged. Physical packages link known modules through
`nodes.md`; new model layouts do not require a library entry to cook or run.
The checker uses checked-in sources by default. With `--assets`, and at runtime,
`std::` comes exclusively from cooked resources, ignoring any local replacements.
