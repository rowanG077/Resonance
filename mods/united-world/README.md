# United World proof of concept

This optional mod adds **United**, overworld slot 2. Sylvarant (0) and
Tethe'alla (1) retain their original terrain, locations, event rules and interiors.
Normal games and saves can still use both original maps with this mod enabled.

From the repository root, after importing both discs:

```sh
mods/united-world/play.sh
```

For the local software-rendered Linux environment, use
`WGPU_BACKEND=gl mods/united-world/play.sh` (the Vulkan software adapter cannot
start the renderer). On hardware, the launcher uses the game's normal backend
selection and respects `WGPU_BACKEND`.

The launcher defaults to 1920×1080; `--resolution WIDTHxHEIGHT` overrides it.
It rebuilds the mod when its sources or imported assets change, then starts a
disposable exploration save above Iselia on Rheairds, with the eight-member
reunited party at level 60. Normal
battles are enabled. To resolve encounters automatically while surveying:

```sh
mods/united-world/play.sh --skip-battles
```

You can also generate and launch separately:

```sh
python3 mods/united-world/build.py --assets local/all-assets
cargo run --profile release-fast --bin resonance -- \
  --mod mods/united-world/generated --test-united-world
```

Use the standard overworld controls: movement steers, Confirm accelerates in
flight, Cancel lands/takes off, Start cycles the map, and Menu opens the party
menu. Campaign entrance guards that immediately reject entry are bypassed for the
united-world sandbox so their prepared fields can be explored. In imported
interiors, the playground's Start shortcut returns to the
saved overworld position when a campaign event prevents ordinary exploration.
F5/F9 use the ordinary development save slots. Pass `--save-directory PATH`
to retain playground saves beyond the disposable launch; load them with the
same `--mod` option.

## Geography and locations

The hand-drawn continent polygons use the broad silhouette of
[Alex M. Lazara's Tales of Phantasia map](https://shrines.rpgclassics.com/snes/top/world.shtml):
a long western landmass, northern snowy islands, a large northeastern continent,
a southern forest belt and a southeastern desert. New straits, coastlines and
biomes represent the world immediately after fusion. Dawn of the New World was
not used. No artwork or geometry from the reference is redistributed.

`design.py` is the layout source: fifty named districts, their original local
landscapes, explicit travel corridors, forest reserves and connecting landforms.
The grand bridge keeps its complete span and both banks; Gaoracchia keeps all
four gates with Mizuho; Heimdall and Ymir share a forest district. Northern Euclid
has a continuous pilgrimage route, while the two Midgard shores meet at the
bridge. Eight native sailing landings have separate united-world dock records.

`native_regions.py` moves each district's painted scenery, collision, shore and
entrances with one rigid transform. Buildings keep their native elevation datum,
including explicit zero heights. Core terrain is never flattened, eroded or
road-smoothed after placement. The outer landscape is joined across a transition
band, and the sea uses one continuous texture phase. `landscape.py` authors these
joins and the new terrain; `build.py` assembles the mod and event registry.
Both native sea UV layers retain their original wave scale. A continuous visual
seabed covers the gaps left by moving coastal districts without changing their
shoreline or collision.
Terrain joins follow the visible native cliff edges and retain their individual
height breaks. A narrow painted apron blends their textures into the surrounding
terrain; shared texture gutters prevent lines at the 6,400-unit tile boundaries.
Only landscape surfaces supply these heights and aprons. Opaque roofs and tree
crowns remain scenery and cannot raise the transition ground or stamp their
atlas details onto hills.
Native mountain and forest passes keep their authored draw order.
Unnamed tree meshes inherit forest classification from their material. Forests
continue past district cuts using their original crowns, UVs and depth settings,
then end with trunk silhouettes. Disconnected scraps from nearby source forests
are discarded after clipping, across material and tile boundaries. Generated
reserves clip their density contours, have low rounded edges and avoid steep
cliff faces. Their foliage repeats at the native Tethe'alla scale, with crown
shading shared across tile cuts. Snow belongs to the
northern crown rather than covering the eastern grass settlements.

The united sea has one surface; coast ribbons follow the actual terrain/water
intersection. Foam uses one inset atlas frame and a derived transparency mask,
with the old blue-backed wave overlays omitted. Water finishes before secondary
tree crowns so a later sea tile cannot erase a forest edge.
Outside native foundations, the generated shore height crosses sea level
continuously; binary land/sea height steps must not pin the coast to grid corners.

There is no automatic spanning-tree road network or seeded grove scattering.
Caravan camps occupy selected clearings; small movable props receive local
footing checks. Public labels keep the original distinction between a city and
its individual gates, so bridge services and duplicate entrances do not clutter
the map. All 179 original landmark identities keep independent united-world
counterparts, and unassigned retail slots remain inactive.

The builder generates 108 terrain/collision tiles, a separate HUD/menu map image,
`relief-map.png`, `atlas.svg`, and district review images. Generated artwork stays
local and depends on the user's imported game assets.

The united locations retain `source_location` so an original interior's exits
return to the corresponding location on United. Their registry keys are separate
from the originals. Restored ranch entrances and Heimdall have explicit routes
rather than campaign gates. The unused retail Mysterious Temple marker is a
second entrance to the newly exposed Confluence Hollow.

## Confluence Hollow

The fusion exposed a small stone courtyard south of Iselia, between the two
worlds' newly joined strata. Its weathered overworld doorway sits on an existing
meadow; its footing follows that meadow's native surface. Follow the blue and
violet seams past two roaming beast symbols to the shattered obelisk. Leave
through the southern passage.
The scene, floor collision, paving, seams and rock formations are newly authored;
party/enemy models and combat use the ordinary shared game assets.

Its field ID is 2900 and its united-world landmark ID is 1400. The builder writes
the small scenario using the existing native field ABI, including enemy combat
and an exit trigger. No new dungeon-specific engine subsystem is required.

## Verification

Geometry regressions, native doorway bounds and shared texture gutters:

```sh
RESONANCE_UNITED_MOD="$PWD/mods/united-world/generated" \
  python3 -m unittest discover -s mods/united-world/tests -v
```

The full renderer survey covers all 108 tiles from above, four directions around
each of the 50 districts, three caravan sites and three wave-animation phases
(323 views). Its JSON records rendering/resource failures; the images also need
visual inspection. The HTML index links every full-size image:

```sh
python3 mods/united-world/tests/audit_world.py \
  --mod mods/united-world/generated --output local/united-world-review/full-audit
```

Contact sheets are generated alongside the full-size captures. An optional
`visual-findings.json` in the audit output records the human review separately
from renderer/resource checks and appears in the HTML report. A clean render
count alone must not be described as a clean visual audit.
The current review also records remaining native-to-generated terrain and paint
joins, including the Northern Ranch, Iselia, Flanoir and the southern continent.
Passing geometry and entrance checks does not clear those visual findings.

The grounding audit compares thousands of support surfaces and every contextual
model datum against the original maps, then checks all eight sailing piers and
sea endpoints using production collision queries:

```sh
RESONANCE_TEST_ASSETS="$PWD/local/all-assets" \
  cargo test -p resonance-presentation --lib united_world_preserves_native_foundations \
  -- --ignored --nocapture
```

`RESONANCE_UNITED_MOD=PATH` selects a staged candidate for either audit without
replacing the launcher's generated map. The entrance audit also checks both
original slots, validates ground or aerial approaches, and resolves every named
entrance independently of public map labels. It can additionally prepare and run
every interior:

```sh
RESONANCE_TEST_ASSETS="$PWD/local/all-assets" RESONANCE_UNITED_INTERIORS=1 \
  cargo test -p resonance-presentation --lib united_world_entrance_audit \
  -- --ignored --nocapture
```

Set `RESONANCE_UNITED_COMBAT=1` to additionally prepare Confluence Hollow's
encounter through the production battle worker, including party/enemy assets,
stage, effects and audio. `RESONANCE_UNITED_LOCATIONS=1400` limits that check to
the new dungeon; omit it for the complete location audit.

Generated content stays under `generated/` and is excluded from Git. Rebuild
that directory after changing the source or recooking the base game. The importer
continues to generate the unmodified original two-world library.
