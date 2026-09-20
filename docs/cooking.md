# Generic asset cooking

`resonance-import cook-all` converts both extracted GQSEAF revision 0 discs into
one shared asset library. It visits physical files and archive members, including
unused assets, without using the playable route or encounter selection as a filter.

```sh
resonance-import cook-all --jobs 6 \
  --extracted local/extracted/disc1 local/extracted/disc2 \
  --output local/all-assets --coefficients /path/to/Dolphin/Sys/GC/dsp_coef.bin
```

Cooking and codecs run in Rust. Disc extraction happens once; subsequent work
uses files. Conversion does not invoke FFmpeg, vgmstream, or KTX executables.
`--jobs` bounds the shared worker queue. Choose a worker count that fits available
memory; more workers are not useful when they force swapping.

## Shared library and preparation

- `assets/<hash>/` contains converted resource packages. Source identity and relevant
  dependencies participate in reuse; equal filenames do not imply equal content.
- `audio/` contains shared samples and prepared playback packages.
- `data/` contains parsed database, interface and native-parameter records.
- `sources.json` maps source provenance to shared publications. Disc labels appear
  here, not as separate copies of output assets.
- `summary.json`, `failures.json` and `excluded.json` account for the conversion.
  Unknown formats and failed members remain errors. Validated metadata and native
  code containers are classified explicitly; executable slices are not cooked assets.

The `cook-title`, `cook-field`, `cook-menu`, `cook-skits` and audio commands prepare
these shared publications for the player. Rerun `cook-all` after parser or format
changes, then rerun preparation. No cooked-format compatibility layer is required;
version mismatches request a recook.

Physical battle packages, action/effect records and embedded parameters are part
of generic recovery. Keeping their data does not enable combat: battle execution,
encounter integration and battle UI are separate work.

## Format recovery and validation

Use the original loaders and consumers in
`/home/rowan.goemans/Documents/engineering/Tales-of-Symphonia-decomp` to establish
record layouts and semantics. Implement shared parsers with bounds checks and
named fields; retain unresolved storage explicitly. Avoid asset-specific fallbacks
that hide a rejected format or missing dependency.

A successful full cook establishes conversion coverage for the supplied inputs.
It does not establish that every embedded table has been discovered, every native
controller has been implemented, or every scene renders equivalently. Continue
format work when a concrete resource fails or a consumer proves data was omitted.

Run workspace tests, a complete two-disc cook, and the existing field/menu/skit/save,
movie and audio regression cases after substantial changes. Dolphin comparisons
use fresh native output and verified source captures at native resolution. Keep
image/state/audio thresholds unchanged and all unattended playback silent. See
[the oracle tools](../tools/oracle/README.md) for capture and comparison commands.
