# Resonance Yggdrasil

Yggdrasil and its companions, rendered by the workspace's Bevy engine. The crate
embeds its artwork and choreography. Twelve visits span **483.5 seconds**, with
**24 FPS** animation and **601 tree growth states**. The final camp keeps moving
after the story ends; the fire stays lit and Kratos keeps watch.

## Preview

```sh
cargo run -p resonance-yggdrasil --example preview
```

An optional argument selects the starting time in seconds. Space pauses;
Left/Right seek five seconds; comma/period step one frame; Tab selects a visit;
Home/End seek to its beginning/end; N changes lighting; +/- changes speed;
F11 toggles fullscreen; S saves the actual window at its physical resolution
in the process's temporary directory; Escape closes the window.

## Embed in Bevy

Add `YggdrasilPlugin` after the host's rendering plugins. `YggdrasilOutput.image` is a
1024 × 640 sRGB Bevy image, suitable for a sprite or UI widget. The render graph
updates it before the host's cameras.

```rust,no_run
use bevy::prelude::*;
use resonance_yggdrasil::{YggdrasilOutput, YggdrasilPlayback, YggdrasilPlugin};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(ImagePlugin::default_nearest()))
        .add_plugins(YggdrasilPlugin)
        .add_systems(Startup, show_yggdrasil)
        .run();
}

fn show_yggdrasil(mut commands: Commands, yggdrasil: Res<YggdrasilOutput>) {
    commands.spawn(Camera2d);
    commands.spawn(Sprite::from_image(yggdrasil.image.clone()));
}

fn importing(mut yggdrasil: ResMut<YggdrasilPlayback>) {
    yggdrasil.progress = Some(0.35); // completed work / total weighted work
}
```

`YggdrasilPlayback` supplies story `seconds`, `speed`, `paused`, `playback`, optional
external `progress`, and visual `options`. `Playback::Journey` follows the whole
story; `Event(id)` selects a visit; `Growth` shows the environment alone. With
`progress: None`, visits use their authored tree sizes. Pause and set `seconds`
to drive the clock from the host. Put controls before `YggdrasilSystems::Advance`,
or a manual clock between `Advance` and `Render`.

The plugin owns a `Renderer` resource; `Renderer::events()` lists visit IDs,
titles, times and tree sizes without loading another copy of the artwork.
Growth and story time are independent: an installer can
hold growth during a visit and advance it between visits. This crate does not
start extraction or cooking jobs. Invalid event selections report an error in
`YggdrasilOutput` while retaining the last valid image.

## Implementation

- `data.rs` loads typed sprite registrations, masks, materials and sky assets.
  Each cel stores its image and both hand anchors together. Clips supply their
  rotation pivot; actors select one clip and drawing. Character sizes and
  breathing phases, prop rectangles, and the bark image live in these assets.
  Images use named regions in the [KTX2 atlases](assets/README.md), grouped by
  character, environment, tree and props. There are no numeric image IDs.
- `scene.rs` defines actor/prop keyframes and their interpolation. Drawing
  selections change on their keyframes; positions and angles interpolate.
  Coordinates and visibility are explicit; motion defaults to zero and scale
  to one. Omitted fields do not inherit a preceding keyframe's values. Layer,
  grip and placement choices are typed, with watering and drop settings grouped
  by their action. Facing follows the authored `face` or `heading`, including
  short backsteps; heading turns interpolate while the actor is stationary.
- `timeline.rs` prepares travel distances, pose start times and growth intervals
  once, then samples the requested time. Persistent objects have explicit world
  keyframes, including the stake's fall, with no reconstruction of earlier visits.
- `actors.rs` and `props.rs` share sprite registrations for hands, cuffs and
  held objects. Complete limbs rotate about their authored pivots.
- `tree/growth.rs` samples the tree's branch curves and generates attached
  twigs, shoots and canopy leaves. `tree.rs` grows and thickens that persistent
  skeleton; `lighting.rs` and `renderer.rs` compose the environment and visits.
- `drawing.rs` records immutable drawing layers. `gpu.rs` and `yggdrasil.wgsl`
  render them with Bevy quads, prepacked atlases, and bounded texture/buffer reuse.
  Nearest sampling resolves texel-boundary ties consistently, preserving thin
  linework when character cels are reduced to their scene size.

`assets/events/<visit>.json` contains one visit's timing and choreography;
visits are ordered by their declared start time. `assets/scene.json` contains
shared sprite registrations, lighting, story timing and persistent world props.
Visit positions interpolate; world keyframes hold their state until the next
exposure. The short stake fall is authored on the same 24 FPS clock.

`assets/tree/growth.json` contains 17 authored branch curves, 12 crown tiers,
growth timings and foliage registrations. Rust samples Catmull–Rom curves,
connects crown twigs to supporting boughs, and places leaves along those branches.
Eight painted crown patterns supply canopy silhouettes and colors. Their ribs,
individual leaves and opening schedules are generated once when loading.
Branches extend along their curves and thicken with age; leaves grow from fixed
attachment points. There are no stored frame sequences or sampled paths in JSON.
See the [asset guide](assets/README.md) for editing details.
Unknown fields and invalid field types fail when loading. Masks and art
coordinates belong in this data; grid sizes, cadence and cache limits are named
in the implementation. The environment uses a 512 × 320 pixel grid, scaled 2×;
character cels use 128 × 128 pixels with 16 pixels of articulation padding.

## Checks

```sh
cargo test -p resonance-yggdrasil
```

Tests cover engine contracts: clock cadence, independent growth, keyframe
interpolation, prop attachment/release, facing and branch/leaf attachment.
The GPU test uses synthetic drawings to check transparent compositing,
thin-line sampling, replay and bounded resources, and checks playback controls.
The suite requires a GPU adapter. Tests must not freeze particular scene pixels,
character positions, event names or story timings; artwork changes are reviewed
in the native preview.

Use the native preview for visual checks: all nine characters and Noishe,
prop contacts, releases, crossings and the continuing campfire. Check thin
facial lines at fullscreen with F11; S saves the actual window. Keep comparisons
under `target/` when changing shared rendering code.

Preserve these artwork constraints:

- Move complete limbs around their pivots; keep boots and feet at their drawn size.
- Keep Noishe's body, four legs and tail separate and anchored during petting.
- Hands, cuffs and held objects share the owner's transform and drawing depth.
- Preserve Colette's complete seated costume and shoe silhouettes.
- Inspect transparent gaps before changing them; many are intentional.

GPU readback and resource accounting are private test helpers.
