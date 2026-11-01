# Ghost Wolf modding example

Ghost Wolf is a complete new arte for Lloyd, included automatically when assets
are imported. Its animation source is `source/ghost-wolf.blend`; the exported
GLB stores authored motion relative to locally imported poses. The importer
resolves those references into the animation used by the game. Blender is needed
to edit and export the animation, but importing assets and playing do not require
Blender.

After an ordinary import, run:

```sh
cargo run --release -p resonance -- --assets local/all-assets
```

Lloyd learns Ghost Wolf at **level 6**. It is not part of his starting techniques.
An existing save with Lloyd already at level 6 or higher acquires it on his next
level-up, or through ordinary base-arte learning when starting a grounded normal
attack. The combat learning check also requires level 6 and works at maximum
level. Open **Artes** to assign its combo; it is also available through the
battle Artes command and Unison loadout.

Normal importing generates `game/techniques.json` and `game/characters.json`
from the original game data, adds the definition and learning/defaults from
`example.json`, and writes the example's runtime files. After editing and
exporting, run the [ordinary asset import](../../docs/cooking.md) and restart
Resonance. The importer embeds the repository's example files at compile time;
the documented `cargo run` command rebuilds it when those files change.

No `--mod` argument is needed for this example. Its `example.json` is an
importer input; ordinary mods use the file overrides described in the
[editable content guide](../../docs/editable-content.md).

## Files to edit

| File | Purpose |
| --- | --- |
| `source/ghost-wolf.blend` | Authored animation adjustments, editing controls and studio, with links to the local character, rig and reference pose. |
| `example.json` | Catalogue ID/name, owning character, local motion references, initial learning, and the single technique entry: text, TP, family, range and learning rules. |
| `battle/artes/ghost-wolf/action.json` | Motion binding, gameplay timing, movement, contacts, sounds, effects and Unison placement. |
| `battle/artes/ghost-wolf/ghost-wolf.glb` | Exported motion deltas on named identity nodes; no native skeleton, bind matrices or idle pose. |
| `battle/artes/ghost-wolf/effects.json` | Blue-white launch streaks, passing crescents, vapor, impact flashes and shards. |

The effect document references the existing cooked texture
`battle/effects/common/textures/atlas-0-0-palette-0.ktx2`. Ordinary
importing creates it from the user's game discs. The example does not
bundle this game texture or copy it into its own asset directory.

The importer adds catalogue ID **260**, named `ghost-wolf-lloyd`, to the native
catalogue and Lloyd's learning list. `initially_known` controls whether new
parties start with it. The example supplies no full catalogue or character
replacement. All costumes bind the same animation through native joint names.
There is no Ghost Wolf-specific runtime dispatch: the game reads its authored
action through the ordinary asset system.

## Animation source

The source links Lloyd's model, swords, textures, skeleton and reference idle pose
from an ignored local library, built from the cooked game resources. After
importing the game assets, run these commands from the repository root:

```sh
nix develop .#art --command python3 tools/ghost_wolf_blender.py prepare
nix develop .#art --command blender mods/ghost-wolf/source/ghost-wolf.blend \
  --python-expr "import bpy; bpy.context.preferences.filepaths.file_preview_type = 'NONE'"
```

The helper reads `local/all-assets` and writes
`local/ghost-wolf-preview/preview.blend`. It prepares the linked dependencies;
it does not invent or alter the authored performance. Keep this library linked
when saving the source. Never pack or make the game meshes, textures, rigs or
reference action local to the distributable `.blend`. The Blender launch also
disables file thumbnails so saving does not embed a preview of game artwork.
Run the export helper after the final save and before committing: it also clears
Blender's cached native pose matrices from the source file. Ordinary Blender
Save can regenerate those caches. Blender backup files are ignored.

The accepted performance began as Python-generated poses. Those poses have been
transferred to an editable Blender rig; further animation changes belong in this
`.blend`. Blender constraints and ordinary keyframes drive the animation over a
linked reference pose. The exported GLB stores only reference-relative changes:
translation and scale differences, and rotations relative to that pose.

1. Select **01 — Ghost Wolf controls** and use **Pose Mode**. The active action
   is **Ghost Wolf · pose adjustments**, a clean additive layer for further edits.
   The accepted performance remains on the base track.
2. Select a control, move or rotate it, and choose **K → Location & Rotation**
   to insert a keyframe. Use the Dope Sheet or Graph Editor to adjust poses and
   timing. The 25 visible controls include **Hips**, **Chest**, **Upper arm.L/R**,
   **Elbow.L/R**, **Forearm roll.L/R**, **Palm.L/R**, and **Thigh/Shin/Foot/Toe.L/R**.
   These are
   forward-kinematics controls: rotating an upper arm carries its forearm, palm
   and sword with it. Pose the legs and feet together when changing a planted
   stance.
3. The **Advanced · skin rings, fingers and attachments** bone collection exposes
   the finer deformation and grip controls. The visible character is bound
   directly to the control rig; a separate native rig follows it for export.
   Large changes to a shoulder arc may also need adjustments to these skin-ring
   keys; this preserves the native model's deformation controls rather than
   supplying a general-purpose autorig.
4. If editing the base NLA strip, press **Tab** in the NLA Editor to leave strip
   editing first. Exporting from strip editing omits the additive adjustments.
   Save the `.blend`, then run this from the repository root:

   ```sh
   nix develop .#art --command python3 tools/ghost_wolf_blender.py export
   ```

   This samples the saved animation, removes the locally referenced skeleton
   and pose, and writes `battle/artes/ghost-wolf/ghost-wolf.glb`. The intermediate
   absolute animation stays under ignored `local/`; exporting the native rig
   directly into the repository would reintroduce game data.
5. Run the ordinary asset import and restart the game to review the result.

The detailed accepted performance remains on the NLA track
**BASE · authored performance (Tab to edit)**, above the linked
**REFERENCE · local native idle** track. For normal pose refinements,
key the active adjustment layer; its keys add to the base motion. To change the
underlying performance or its timing directly, select its
**Ghost Wolf · authored performance** strip in the NLA Editor and press **Tab**
to edit its FK action. Press **Tab** again to leave
strip editing. The base contains detailed converted curves, not a small set of
newly hand-authored poses.

The scene runs at **120 FPS**, frames **0–160**, for a 1⅓-second clip. Two Blender
frames represent one gameplay update; the three-update entry blend is applied
by the game. Preserve the scene rate and range when changing poses. If you
change the clip's duration, update the gameplay timeline too. The export helper
samples the constrained rig, retaining the editing controls in the `.blend`.

The preview's travel and half-turn illustrate the gameplay action at a nominal
target distance. Actual traversal and facing come from `action.json`, so keep
changes to body timing coordinated with that timeline. Do not add the studio's
travel to the exported motion: the game already applies it.

## Gameplay timing

The arte costs **12 TP** and deals two physical, non-elemental hits at **90%**
and **150%** power, under normal defense, accuracy, guard and interruption rules.
Lloyd begins the right-hand cut before accelerating through the target. The
right blade follows through past his hip while the left prepares across his
chest; his counterclockwise turn carries the second, horizontal cut.

Traversal starts at gameplay update **16**, lasts **nine updates**, and aims
**70 units** beyond the target's root. The pass is shorter from a close entry
and remains limited by the arena boundary. Both cuts are checked at starting
distances from 40 to 120 units. Lloyd retains the completed half-turn even when
he cannot get behind the target; a follow-up arte may therefore face away from
an enemy trapped at the boundary. This is intentional.

The action lasts **83 updates**, including the three-update entry blend, with
**10 updates** of recovery. A higher-level arte can chain at update **48**. With
Ability Plus equipped, three forward normal attacks can lead into Ghost Wolf,
Lightning Tiger, and Sword Rain: Alpha. A miss still completes the movement and
recovery; interruption stops the pass and clears its collision override.

The voice event uses Lloyd's native “Take this!” line from the imported speech
library, stream 104. Native sword ribbons follow the blades. The effect bank
adds blue-white arte light and a stronger second impact while the animation
continues through contact. The design adapts Yuri's Ghost Wolf from Vesperia to
Lloyd's twin blades; it does not need Vesperia assets at runtime.

## Verification

After importing assets:

```sh
cargo test --offline -p resonance-content ghost_wolf_assets
cargo test --offline -p resonance-importer modding_examples
cargo test --offline -p resonance-game --features test-support \
  ghost_wolf_ -- --ignored --nocapture
```

The content check validates the action's animation/effect dependencies and checks
that playback finishes with its gameplay timeline. Importer checks cover the
example's generated definitions and catalogue collisions. The gameplay checks use
`local/all-assets` by default, or the absolute path in `RESONANCE_TEST_ASSETS`.
They exercise hits, misses, interruption, TP, voice, facing through recovery,
both arena edges, and the 17-hit Ability Plus chain at several close distances.
