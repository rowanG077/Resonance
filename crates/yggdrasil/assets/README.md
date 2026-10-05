# Yggdrasil artwork

Edit the atlases directly. Each KTX2 has an adjacent JSON mapping local sprite
names to `[x, y, width, height]` rectangles, measured from the top-left corner.
The scene and event files reference these regions by path, for example
`characters/colette/pet/01` or `props/objects/book`.

```text
characters/   colette, genis, kratos, lloyd, noishe, presea, raine, regal, sheena, zelos
environment/  ground, sky
tree/         bark, foliage, painted crown patterns, growth.json
props/        objects
events/       one JSON per visit: watering, study, camp, ...
scene.json    shared sprite registrations, articulation, lighting and world props
```

Character atlases contain front-view strips, named action cels and whole walking
parts. Cel numbers are local to their action. The foliage atlas groups painted
leaf variants as `palette/<shape>/<RGB hex color>` and small shoots by shape.

## Tree growth

Edit `tree/growth.json` to change the tree's shape and growth schedule:

- `branches` supplies named Catmull–Rom control points, mature widths and
  `birth`/`end` progress. Points are relative to the parent attachment, or
  `origin` for the trunk. `attach` is distance along the parent's mature curve;
  `at_tip` follows its endpoint. Parents precede children. `initial` is the
  branch length already present at zero progress. Roots and bark ridges have
  their own `kind`.
- `crown` supplies rows of canopy centers. The model varies their placement
  deterministically and connects each to a supporting bough with a twig.
- `timing` controls twig duration, the delay before the lush crown opens,
  and leaf opening. Branch and shoot times use overall growth progress from
  0 to 1. Crown delay consumes a fraction of the progress remaining after its
  twig arrives; `full_crown_base + ready * full_crown_tip_weight` sets the
  earliest finish. Blade opening uses its canopy's local opening progress.
- `canopies` registers eight painted masks in `tree/crowns.ktx2`. Their opaque
  pixels specify leaf coverage and palette color. `shoots` supplies the four
  leaf shapes, and `palette` selects their painted color variants. Keep mask
  colors paired with matching palette regions when editing.

Rust derives all sampled curve points, connecting twigs and leaf placements
from these inputs. Geometry is built once and growth is evaluated at 601
checkpoints. Leaves stay attached as their parent branches extend; changing
progress does not generate a new tree.

## Texture format

Textures use the same profile as Resonance's cooked textures: **RGBA8 UNORM KTX2,
Zstandard compression, straight alpha**, with one mip level. Save without lossy
block compression, resampling or color conversion. Preserve each region's size
and one-pixel extruded gutter; update its JSON rectangle if moving it. The renderer
composites the artwork's encoded colors before its final sRGB presentation.

The build embeds these files directly. Bevy decodes them once; sprites share
atlas pixels and GPU textures. No runtime packing or build-time conversion is
needed. PNG is used only for preview screenshots, not shipped artwork.
