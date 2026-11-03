# Reviewing arte captures

Keep visual review separate from successful preparation or execution. Use the
current catalogue from `catalogue.py`; never inherit an old edition's numeric
exclusions. Full-game walkthroughs and mechanical equivalence sweeps are not
part of this visual pass.

The native GPU harness writes `capture-report.json` with actual PNGs. An
original-game reference report has `source_kind: "dolphin"`; a render using
GameCube assets in Resonance has `source_kind: "resonance"`. These references
remain distinct in the gallery. A report must retain the requested case ID,
observed admission, action-relative image ages, and its library/source provenance.

```sh
python tools/arte-visuals/gallery.py \
  --catalogue local/arte-visuals/catalogue104.json \
  --captures /path/to/steam/capture-report.json \
  --captures /path/to/gamecube/capture-report.json \
  --captures /path/to/original-game/nativecapture-report.json \
  --reviews local/arte-visuals/reviews.json \
  --output local/arte-visuals/review
```

`index.html` shows candidate references and captured phases. Click an image for
its full size. Check silhouette, particle shape and color progression, motion,
target placement, lighting, outlines, and recovery/cleanup. Minor texture
differences are allowed. Missing effects, different geometry, or incorrect
choreography need a fix. Steam additions require an explicit GC-style reference
choice; a missing counterpart is never a pass.

Reviews are a JSON object keyed by case ID. Copy the gallery's `evidence_key`
after inspecting the actual images and select an explicit reference ID:

```json
{
  "steam:party:1:1": {
    "status": "pending",
    "reference": "original:gamecube:party:1:1",
    "evidence_key": "copy-from-coverage.json",
    "notes": "Record the observed visual result here."
  }
}
```

Allowed results are `match`, `minor_texture_difference`, `needs_fix`, or
`pending`. Changed image bytes invalidate an earlier review. `coverage.json`
retains every case and recipe variant, failed/pending capture, pending review,
and original-game reference availability. An admitted action with screenshots
still starts pending visual review. Do not call the arte pass complete while
required captures or reviews are missing.

For stored spells, payment/release and the active scene are distinct milestones.
The render harness's `release_age` follows release feedback; `active_scene_age`
follows the matching `Casting::Released` after the opening transition. Phase
sheets prefer the latter, matching the native observer's active-scene milestone.
An older report's payment timestamp alone does not prove that an opening delay
is missing from the engine.

Capture reports are applied in command-line order. Later requested results
replace earlier attempts for the same case, including a later failure;
unrequested pending rows never replace evidence from another batch. Original
Dolphin captures retain a distinct `original:` identity alongside GC-cooked
renders. Replaced image bytes make earlier review decisions stale.

For an arte with no direct GC counterpart, explicitly choose a captured reference
and record `"basis": "gc_style"` with notes explaining the visual analogue.
This is a GC-style review, not evidence that the new arte existed in GC.
