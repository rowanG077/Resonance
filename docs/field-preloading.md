# Field preparation

Every cooked field requires a complete `fields/map-{id}.preload.json`. The schema lives
in `resonance_content::field_preload`, generation in
`resonance_import::field_preload`, and runtime ownership in `prepared`,
presentation `loading` and `field_warm`.

`cook-all` walks the original field catalogue and prepares every field through
one path. The catalogue resolves each source archive; its declarations select
geometry, animations, collision, dialogue, declared NPC resources,
the field's packed model bank and MAP-local models. The bank has its own script-ID
table; a direct actor ID need not be a separately declared shared NPC resource.
Standalone character files and grouped archive dependencies resolve through the
source resource catalog. Cooking rejects declared model/animation IDs without a
character binding. All overlay texture banks are cooked independently of script branches.
The runtime evaluates procedural location-caption reveals from their cooked image sizes;
it does not generate an animation track on first use. Authored animation tracks remain
cooked assets. Mesh filenames
include their content hash so a changed shared character clip set cannot overwrite
meshes referenced by another field.
Field inventories also include skit scenarios, message tables, portrait atlases
and media metadata. Portrait surfaces and dialogue layers warm with the field;
opening Z performs no reads or shader compilation. Silent media carries only a
clock; audible tracks join the prepared voice bank. Shared content hashes and
dependent field inventories refresh together.
Field particle recipes carry their texture, palette and motion parameters.
Their texture dependencies and render layers are prepared before activation;
live particles only update geometry and use the same missing-effect audit.
The same field preparation inventories audio across declared script branches,
message voices and shared native service cues. It resolves sound banks, music
and voices across both extracted discs. Movie dependencies follow the field's
script requests. Unresolved requests fail cooking.

The shared dialogue atlas has a fixed repertoire. Unsupported source-font
characters share its original fallback bitmap, declared explicitly in the font
metadata, so cooking one field cannot change another field's glyph coordinates.

Paths are relative to `--output`, default `local/all-assets`. Field scene and
audio metadata use `fields/map-{id}.json` and `fields/map-{id}-audio.json`.
No field-specific command or manual dependency list is required.

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
   dialogue/subtitles, location captions, effects and shadows using the live camera/target settings.
   Wait for expected render draws, successful compilation and GPU completion.
4. Release the black hold, attach the destination audio source and resume field
   updates. Opening audio commands remain queued until the scene is ready.
   Retain prepared resources for
   revisits. New raw reads, sampler bindings or field pipelines fail the
   development guard; dynamic geometry/uniform updates remain allowed.

New Game prepares setup and classroom together. Later destinations prepare on
request; checkpoint startup prepares its saved field directly. Field changes
pause gameplay, retire outgoing audio and publish verified bytes before renderer
loading begins. Live actors, effects and UI instances retire on handoff; visited
packages and artwork remain cached. Returns recreate instances from those assets
and reuse shader and sampler variants. Available destinations are discovered from
published field inventories. The validated playable route covers maps 330–340;
preparing other fields does not establish support for their native services.

Relevant tests: `cargo test -p resonance-import --lib field_preload::`,
`cargo test -p resonance-content prepared::`, and
`cargo test -p resonance-presentation sampled_tests:: --lib`.
