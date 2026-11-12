# Ghost Wolf modding example

Ghost Wolf is an ordinary additive arte mod for Lloyd. Its animation source is
`source/ghost-wolf.blend`; the exported GLB stores authored motion relative to
locally imported poses. Blender is needed to edit/export the animation. Building
the mod resolves that reference pose against your existing cooked library; it
does not need the discs or a fresh base-game import.

After an ordinary base import, build the mod into a separate generated directory:

```sh
cargo run --release -p resonance-import-cli -- prepare-arte-mod \
  --source mods/ghost-wolf --assets local/all-assets \
  --output local/mods/000100-ghost-wolf
cargo run --release -p resonance -- --assets local/all-assets
```

The numbered directory is discovered automatically. To use another output path,
pass it with `--mod PATH`. Disabling the mod leaves the base library unchanged.
After another Blender export or a recipe edit, rerun `prepare-arte-mod` and restart.
The output directory is generated; keep custom source edits in this project or a
separate higher-priority mod.

Lloyd learns Ghost Wolf at **level 6**. It is not part of his starting techniques.
An existing save with Lloyd already at level 6 or higher acquires it on his next
level-up, or through ordinary base-arte learning when starting a grounded normal
attack. The combat learning check also requires level 6 and works at maximum
level. Open **Artes** to assign its combo; it is also available through the
battle Artes command and Unison loadout.

`example.json` is the offline arte build recipe. The build writes only this
arte's rules/text and its learning registration as granular source files. It
registers no number: the game gives the arte a free ID at startup, and saves
record it by name. It writes no `collection.json` markers for those shared
catalogues; the base library owns them. Another mod can add a different arte or
translate this one's `text.json` without copying any global catalogue. The base importer has no Ghost
Wolf registration or embedded runtime payloads.

## Files to edit

| File | Purpose |
| --- | --- |
| `source/ghost-wolf.blend` | Authored animation adjustments, editing controls and studio, with links to the local character, rig and reference pose. |
| `example.json` | Catalogue name, owning character, local motion references, initial learning, and the single technique entry: text, TP, family, range and learning rules. |
| `battle/artes/ghost-wolf/action.json` | Motion binding, gameplay timing, movement, contacts, sounds, effects and Unison placement. |
| `battle/artes/ghost-wolf/ghost-wolf.glb` | Exported motion deltas on named identity nodes; no native skeleton, bind matrices or idle pose. |
| `battle/artes/ghost-wolf/effects.json` | Blue-white launch streaks, passing crescents, vapor, impact flashes and shards. |

The effect document references the existing cooked texture
`battle/effects/common/textures/atlas-0-0-palette-0.ktx2`. Ordinary
importing creates it from the user's game discs. The example does not
bundle this game texture or copy it into its own asset directory.

The mod adds the arte named `ghost-wolf-lloyd` to the native catalogue and
Lloyd's learning list. With no other arte mod enabled it receives ID 260, the
first number after the original artes. `initially_known` controls whether new
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
5. Rebuild the generated mod with `prepare-arte-mod` and restart the game.

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
**70 units** beyond the target's root, with a **200-unit maximum total travel**.
The destination is captured when the dash starts; target motion during the
windup or pass cannot increase that limit. Within 130 units, Lloyd makes the
full pass behind the target. Farther away, he performs a short lunge that can
miss rather than crossing the arena. The distance changes continuously at the
limit, without a range threshold that abruptly cancels the dash. The 200-unit
budget preserves the verified 40–120-unit entries plus their 70-unit clearance,
with ten units of margin; it is an authored mod choice. Older generated Ghost
Wolf action documents also receive this bounded default when loaded. The pass
is shorter from a close entry and remains limited by the arena boundary. Both
cuts are checked at starting distances from 40 to 120 units. Lloyd retains the
completed half-turn even when he cannot get behind the target; a follow-up arte may therefore face away from
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

After importing the base assets and building the mod above (or setting
`RESONANCE_TEST_MOD_ASSETS` to your generated mod directory):

```sh
cargo test --offline -p resonance-content ghost_wolf_assets
cargo test --offline -p resonance-importer modding_examples
cargo test --offline -p resonance-game --features test-support \
  ghost_wolf_ -- --ignored --nocapture
```

The content check validates the action's animation/effect dependencies and checks
that playback finishes with its gameplay timeline. Importer checks cover reference-pose resolution, catalogue collisions, and a
build against a small synthetic library: the generated files, composition with
the base, and the output-directory guards. The gameplay checks use
`local/all-assets` by default, or the absolute path in `RESONANCE_TEST_ASSETS`.
They exercise hits, misses, interruption, TP, voice, facing through recovery,
both arena edges, distant targets at 500 and 2000 units, and the 17-hit Ability
Plus chain at several close distances. Core traversal tests also cover mirrored
and diagonal headings, the distance limit, target motion during windup, and
hit-stop.
