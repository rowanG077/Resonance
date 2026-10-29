# Field preparation

Field preparation follows the current named documents and captures their
dependencies before gameplay starts. A field ID resolves through
`fields/mappings.json` to a directory such as `fields/iselia-1/`, whose
`field.json` references the scene, actors, collision, dialogue, scripts and audio.
The names are illustrative; use the installed mappings for actual assignments.

The cooked library contains no preload manifest, stored checksum, fingerprint
or hash-based resource name. Loading follows named paths. The relevant
code lives in `resonance_content::library`, `field::document`, `preparation` and
`prepared`, and presentation `new_game::fields`, `loading` and `field_warm`.
See [Editing named content](editable-content.md) for file formats and mod rules.

## Capturing the effective content

A preparation opens the base library with its ordered mod directories. Whole
files use the last override; mapping files merge by machine ID. One snapshot
resolves those mappings and follows direct references, retaining the selected
bytes while preparation discovers further dependencies. Resource paths are
relative to the library root and remain independent of original disc locations.

Discovery includes declared scene alternatives and hidden actors, field effects,
script-selected resources, shared character models, dialogue and UI artwork,
skits, menu model catalogues, audio and movie requests. Runtime script compilation
can capture additional declared dependencies through the same snapshot. A
converted physical resource does not by itself establish a supported gameplay
binding. Missing or unsupported optional resources follow the diagnostic policy;
`--paranoid` makes those diagnostics fatal. Cancellation always stops preparation.

Music selects shared WAV recordings under `audio/music/`; effects select their
editable programs or WAV replacements, and full voice IDs resolve directly to
WAVs through the speech mapping. Large media retains captured stream handles instead of loading the
whole soundtrack or movie into the resident byte store. File properties come
from the current media; modders do not maintain byte counts or digests.

Saves contain a format version, not a content fingerprint. Preparation reads the
selected destination and shared resources it uses. It never scans other fields
or computes a global gameplay identity. Available field IDs come from
`fields/mappings.json`; listing an ID does not read its target.

A snapshot retains bytes already read. A new preparation reads current files
without comparing them with earlier snapshots. Prepared field packages can be
reused within the running application; restart after editing to refresh all
resident assets. No changed-content error or fingerprint cache is involved.

## Activation

1. A background worker captures current documents, checks their structure and
   references, compiles scripts and prepares audio. The resulting `Files` view
   retains those bytes. Instrument decoding can reuse the same captured WAV
   buffer with the same tuning and loop settings; no content hash is calculated.
2. The application installs the completed view before renderer loading begins. The resident
   asset reader serves captured bytes; a missing path cannot fall back to disk
   once a prepared view is installed. Models retain their loaded glTF graphs,
   and textures retain their selected image and sampler bindings.
3. Offscreen draws prepare representatives of the required mesh/material
   pipeline variants, including depth-write states, dialogue, subtitles,
   captions, effects and shadows. Warm draws share compatible pipeline keys;
   the renderer waits for resource loading, shader compilation, expected draws
   and GPU completion before releasing the loading hold.
4. Destination audio attaches and gameplay resumes. Opening audio commands wait
   for readiness. The running application retains its two most recently used
   field packages, so quickload and a return to the previous field reuse the
   captured inputs and decoded skit programs. Each skit playback still creates
   its own VM and takes the caller's current story state. Older destinations can
   prepare again. Renderer artwork has
   its own cache. Transitions retire live actors, effects and UI instances and
   recreate them from prepared assets when returning. New unexpected reads,
   samplers or field pipelines are reported by the development guard. Dynamic
   geometry and uniform updates remain allowed.

New Game prepares the setup field; its transition prepares the classroom before
activation. Later destinations prepare on request, while checkpoint startup
prepares the saved field directly. Selected
menu pages decode their images from captured bytes on demand and hold input
until their images and GPU draw are ready. A memory read after activation is
valid; `unprepared_reads` counts paths absent from the installed view.

Offline recordings keep the game clock frozen during readiness waits. While an
independent CPU preparation worker runs, the recorder also stops redrawing the
unchanged scene. Application updates resume once the worker finishes, allowing
normal installation, error handling and GPU warmup within the same deadline.

## Editing and verification

Finish editing and restart the application. Restart recreates the asset server,
image/sampler caches and scene preparations. Re-entering a field or reopening a
preview does not reload edits. Existing snapshots retain their bytes and playing
media retains its selected handle. Mod overrides and renames require no recook
or regenerated dependency list; references and mappings must still resolve.

Focused coverage lives in content `library::tests` and `prepared::tests`, and
presentation `loading::tests`, `sampled_tests` and `new_game::fields::tests`.
The native field/mod/save tests require a completed current library through
`RESONANCE_TEST_ASSETS` or the fixture-specific variable in their ignore message.
Preparation alone does not prove that every native service is implemented.
Representative gameplay, GPU readiness and performance acceptance remain separate
checks; their completed results are recorded in the plan.
