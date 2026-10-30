# Animation authoring with GLB

Animations use standard **glTF 2.0 animation in `.glb` files**. Edit them with
ordinary tools such as Blender. The standard channels are authoritative for
both rendering and gameplay attachments; no custom animation payload, extension
or JSON curve sidecar is required. Old motion JSON is unsupported.

## Files and ownership

Follow an actor's `model` reference, then its animation bindings. Shared field
motions belong to their known character or other logical owner:

```text
animations/
  mappings.json
  characters/
    lloyd/
      motion-e221.glb
      motion-e221.json
fields/
  iselia-0332/
    animations/             # only genuinely local performances
meshes/
  characters/
    lloyd/
      model.json
      animations/idle.glb   # intrinsic model motions
```

The adjacent JSON selects actor/part/gameplay requests; all animation curves and
timing live in the GLB. There is one shared performance, even when several
fields, costumes or model parts use it. Unknown action labels remain neutral
numbers until their meaning is established.

A GLB contains one animation and named joints. Known character motions include
a reference rig for editing; target models retain their own hierarchy and rest
poses at playback. Geometry and textures stay with the model. Other clips can
contain sparse named nodes without an established reference body.

Joint order is independent for every model. Matching joint names bind directly;
an optional `joints` object in the gameplay binding supplies only exceptions,
such as `{"hips":"mo6-hips","cape":null}`. A string chooses a different clip
joint; null leaves that model joint unanimated. Unmatched model joints and omitted
TRS channels retain the model's rest values. Affine helper nodes remain part of
the shared performance. Changing a reference rig's rest pose alone does not
change the target models' rest poses.

A clip's optional `pose_chains` names only the hair or cloth chains whose pose
attraction should change during field or model-preview playback. Battle dynamics
use the chain's normal attraction. Chain definitions and their GLB joint
references live on the model, with one gravity/damping value or an explicit
per-joint array. Ordinary animated limbs and facial joints need no such metadata.

The owning model's small JSON bindings select a clip for a gameplay ID. IDs
remain scoped to their model or animation bank. Dynamic animation resource IDs
point directly to GLBs. Each clip owns its joints and curve bindings through
standard glTF skin and animation data; there is no JSON curve data or second skeleton to synchronize.

Model GLBs also own their joint names; `model.json` has no duplicate joint list.
When editing a **model**, retain its `resonance_bone` node custom properties and
enable custom-property export. These indices bind gameplay attachments and node
visibility, including rigid parts without skins. The animation-only workflow
below does not need these properties.

The model GLB also owns the hierarchy and rest transforms used by combat.
Attachment slots reference joint names through `attachment_nodes`; there is no
separate rig document. Identity TRS fields may be omitted, and ordinary matrix
transforms are accepted. Gameplay rest poses must be representable as TRS;
unsupported shear is rejected instead of silently changing the attachment pose.
Ordinary parent objects, such as Blender's Armature object, retain their
transforms for both rendering and gameplay, including while clips play. Keep
those objects when exporting the model; they need no gameplay custom properties.

## Edit in Blender

This workflow was checked with Blender 5.2.2 LTS using an actual Lloyd clip:

1. Set the scene frame rate to **30 fps before importing**. This makes Blender's
   frame numbers match the game's clock; GLB itself stores time in seconds.
2. Import the clip using **File → Import → glTF 2.0**. Under Bone Dir, select
   **Blender (best for import/export round trip)**. Turn off **Guess Original
   Bind Pose** and enable **Disable Bone Shape**. The file supplies its rig; a
   custom importer is unnecessary.
3. Edit the action with Blender's normal animation tools. Preserve joint names,
   hierarchy and rest transforms when replacing only the performance. Import
   the model separately when you want its geometry as a visual reference.
4. Select the rig, including any transform helpers or `Clip timeline` object.
   Export it using **File → Export → glTF 2.0** with **Selected Objects** enabled,
   format **glTF Binary (.glb)**. Export animations in **Actions** mode. Turn off
   **Always Sample Animations** and animation optimization to preserve the
   action's fractional key times. Custom properties are unnecessary.
5. Put the exported GLB at the same relative path in a mod, then restart
   Resonance. Preserve the last key's time when the clip should keep its duration.

Replacing a file requires no mapping edit, hashes, registration or source recook.
To make a variant used by only one actor, save it under a new readable name and
change that actor's clip reference. Editing animation does not invalidate saves.

Blender's importer currently replaces glTF cubic tangents with automatic handles.
For that reason, the cooker emits **LINEAR/STEP** keys, adaptively sampling curved
source motion and removing redundant keys within a bounded error. This preserves
motion through ordinary import. Resonance also accepts standard **CUBICSPLINE**
channels exported by editors; when reimporting such a user-authored clip into
Blender, check the resulting curves. No special Resonance export plugin is needed.

## Standard animation contract

Channels animate translation, quaternion rotation and scale, with independent
input times in seconds and `STEP`, `LINEAR` or `CUBICSPLINE` interpolation.
Duration follows the last animation key. Playback speed, pauses, whole-clip loops
and game events remain game behavior. A hold-only clip uses an ordinary animated
`Clip timeline` node to carry its end time without changing a gameplay bone.

Joint names must be unique and readable within a rig. Model bindings translate
game bone indices to those names. Node array positions inside a GLB may change
when an editor exports it. Preserve the rig's rest transforms and hierarchy.
Unanimated components retain the model's rest values, including during layered
and sparse animation playback.

Original Euler curves, easing, spherical controls and per-track repetition are
importer concerns. Conversion preserves fractional-time behavior, discontinuities
and clip boundaries; runtime evaluates only standard glTF channels. The future
Steam importer targets this same contract, independent of source frame units.

For authored affine shear, the importer factors each matrix into **two ordinary
TRS nodes** in a hierarchy. Their combined transform preserves shear, reflections
and singular scales. These are editable standard transforms, with no animated
matrix extension or hidden native curve fallback. Keep those helper nodes when
editing such clips.

## Verification

The focused checks cover editor node reordering, standard cubic evaluation,
quaternion interpolation, repeated tracks, discontinuities and affine transforms.
A Blender import/export without custom properties preserved the representative
Lloyd clip within 0.000024 model units. Editing its hip X keys by +5 moved
the runtime's hand attachment by +5 X, with maximum error below 0.000046 model
units. A separate Blender round-trip of standard transform helpers preserved
shear and reflection within 0.000001 model units.

A model import/export with custom properties retained all 83 indexed Lloyd
joints and named attachment bindings. Moving Blender's Armature object by +5 X
moved both the rest pose and native-clip attachment poses by +5 X. The maximum
round-trip difference was 0.000585 model units at rest and 0.000619 while animated;
the rest pose also matches an independent standard glTF hierarchy traversal.

The before/after Blender exports were also loaded through ordinary mod
directories and rendered from the same Iselia save. The visible change was
confined to Lloyd, with no source recook or save update between the two captures.
The installed library also passed normal New Game through player control, and
a battle through combat, victory animation and return to the field.

The format is defined by the
[Khronos glTF 2.0 animation specification](https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html#animations).
