# Field preparation

Every cooked field requires a complete `<name>.preload.json`. The schema lives
in `resonance_content::field_preload`, generation in
`resonance_import::field_preload`, and runtime ownership in `prepared`,
presentation `loading` and `field_warm`.

`cook-classroom` writes setup and classroom inventories. Separate audio/movie
cooks refresh them, including cache hits. To rebuild inventories alone:

```sh
target/debug/resonance-import cook-field-preload \
  --field fields/iselia-classroom.json --audio fields/iselia-classroom-audio.json
target/debug/resonance-import cook-field-preload \
  --field fields/new-game-setup.json --audio fields/iselia-classroom-audio.json \
  --movie story-intro.json
```

Paths are relative to `--output`, default `local/cooked`. Supply the complete
`--audio` and `--movie` lists each time. New recipes call
`field_preload::cook(root, Inputs { field, audio, movies })` after writing metadata.

## Inventory contract

The inventory contains input metadata, unique dependency paths with hashes/sizes
and roles, every scene/actor part and animation, required presentation features,
and script entries/native usage. `total_file_bytes` measures deduplicated disk
bytes, not decoded RAM/GPU usage. Missing media produces an incomplete manifest;
malformed metadata and missing/corrupt payloads fail generation.

Script analysis follows all branches, subroutines and registered events without
executing them. Recipes must declare the complete resource pool, including hidden
actors and media available on other branches. The analyzer cannot infer every
computed resource ID or discover an asset the recipe omits. It does not recursively
preload destination fields. Completeness does not prove native/effect support.

## Activation contract

1. A background worker verifies all declared paths and decodes field/audio data.
   Immutable bytes share by digest; instruments share only when sample and
   tuning/loop metadata agree. Movies are verified as streams and prebuffered.
2. The memory-backed asset reader serves declared bytes. Each model loads once;
   scenes/clips come from its retained glTF graph. Completion requires both loaded
   dependencies and finished load jobs. Texture copies share by image and sampler.
3. An offscreen draw prepares every mesh/material variant, both depth-write states,
   dialogue/subtitles, effects and shadows using the live camera/target settings.
   Wait for expected render draws, successful compilation and GPU completion.
4. Release the black hold and gameplay clocks. Retain prepared resources for the
   field lifetime. New raw reads, sampler bindings or field pipelines fail the
   development guard; dynamic geometry/uniform updates remain allowed.

The setup/classroom route prepares both fields. Future destinations need explicit
scene-owner bindings. The setup script supplies its transition fade; generic
field changes must not assume a fade. Teardown releases field leases, actors,
effects, UI and audio; Bevy retains pipeline objects for reuse.

Relevant tests: `cargo test -p resonance-import --lib field_preload::`,
`cargo test -p resonance-content prepared::`, and
`cargo test -p resonance-presentation sampled_tests:: --lib`.
