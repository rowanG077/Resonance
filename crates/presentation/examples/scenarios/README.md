These scenarios use ordinary keyboard or controller input. Run `field-route.json`
from the maintained post-Frank school-grounds save at the activated memory circle
(map 332, story 2500). It walks to the village and back; the quicksave probe's
`--route` option also checks that returning reuses the prepared field resources.
The classroom transition remains covered by the field lifecycle tests.

`new-game.json` starts from the title and reaches the first playable field.
Loading is allowed to finish at native speed; waits measure gameplay updates.

`field-menus.json` uses a free-control school-grounds save with an Apple Gel in
the inventory. It captures Main, Items, the item target panel, cancellation back
to the list, and Equipment. `items_focus` requires the requested List or Target
state before `menu_settled` observes entrance slides and selection
trails before capture; input remains immediate. Ordinary taps need only one
press update and one release update, without fixed animation padding. Decorative
cursor bobs and blinking markers continue normally.

`title-menus.json` opens Continue, navigates its save banks, cancels back to the
title, opens and edits Customize, returns, and enters New Game setup. It uses
the current installed library and the normal input/render path.

The spatial AA GPU regression is independent of cooked assets:

```sh
cargo run -p resonance-presentation --example antialias_probe -- /tmp/aa-probe
```

It saves each supported AA mode and compares geometric coverage against a 64-sample spatial
reference. It also checks unchanged UI pixels, live mode
switching and identical repeated static frames. It uses a software Vulkan/DX12
adapter on Linux/Windows and Metal on macOS.

For the production field/refraction path, use a current cooked library:

```sh
RESONANCE_TEST_ASSETS="$PWD/local/all-assets" WGPU_SETTINGS_PRIO=webgpu \
  cargo test -p resonance-presentation --lib spatial_aa_renders_a_cooked_field -- --ignored
```

The WebGPU priority keeps this check within software adapters' supported features.
Set `RESONANCE_AA_CAPTURE` to retain the field screenshot at a chosen PNG path.

All SMAA quality presets have a color-space and composition regression:

```sh
cargo test -p resonance-presentation --lib regular_smaa_preserves -- --ignored --nocapture
```

It compares sRGB and encoded-unorm targets, verifies edge smoothing, untouched
UI, loading the preceding camera's scene, resizing and attachment cleanup.

The native S2x regression requires hardware with 2× MSAA support:

```sh
cargo test -p resonance-presentation --lib native_s2x_preserves -- --ignored --nocapture
```

It tests the production passes directly: loading a preceding camera's scene,
geometry rendering, matching edge colors in sRGB/encoded-unorm targets, untouched
UI, live resize, and attachment cleanup when S2x
is disabled. The image probe additionally checks S2x coverage against plain 2×
MSAA, alpha cutout smoothing, and identical output after leaving/re-entering a mode.

To test the physical GPU on Linux, override the probe's software-adapter default:

```sh
WGPU_FORCE_FALLBACK_ADAPTER=false cargo run -p resonance-presentation --example antialias_probe -- /tmp/aa-hardware
```

Select a driver with `VK_DRIVER_FILES` if needed. The field check accepts
`RESONANCE_AA_MODE=smaa_s2x` (or another serialized AA choice) and asserts that
the adapter supports the requested mode rather than silently testing a fallback.

Captured-scene mesh effects have a separate asset-independent GPU regression:

```sh
cargo test -p resonance-presentation --lib captured_meshes_obey -- --ignored --nocapture
```

It runs every supported AA choice and checks full occlusion, half coverage,
full visibility, and equal-depth rejection against synthetic per-sample depth.
This exercises the actual effect pass, including the branch absent from the
ordinary field checkpoint. It runs on both physical and software adapters.

Dialogue-frame joins have an asset-independent GPU regression:

```sh
cargo test -p resonance-presentation --lib dialogue_slices_have_no_msaa_seams -- --ignored --nocapture
```

It checks solid and textured clipping masks at fractional joins, opaque and
translucent frames, native and fractional scales, and every supported MSAA count.
It also verifies unchanged pixel-center sampling for text-like UI textures.

Live display preview and timeout recovery can be checked on a GPU with native
2× MSAA and a current cooked library:

```sh
RESONANCE_TEST_ASSETS="$PWD/local/all-assets" WGPU_SETTINGS_PRIO=webgpu \
  cargo test -p resonance-presentation --lib live_resolution_preview_recovers -- --ignored --nocapture
```

This uses the production field menu, display systems and S2x renderer with a
logical window; it does not alter the host desktop. It checks immediate target
resizing, the confirmation dialog, Keep and rollback while Customize remains open,
including a 3024×1964 native fullscreen preview, with a frozen gameplay clock.
The timeout is advanced in the test without waiting
15 seconds. Set `RESONANCE_DISPLAY_CAPTURE_DIR` to retain the screenshots.
